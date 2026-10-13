//! Orc hero abilities (spec: `docs/spec/abilities-orc-heroes.md`).
//!
//! Labels: CONFIRMED = read in the disassembly and, where a number says so, checked in Unicorn; LIKELY = strong
//! inference; UNSURE = a stand-in.
//!
//! ## Critical Strike (`AOcr`, class `CAbilityCriticalStrike`, vtable `0x6F909D74`)
//! * The roll happens at the swing START (the attack ability broadcasts the order `0xD000F` to every ability of the
//!   attacker through `0x6F281580`, which calls the arming slot `0x124` = `0x6F1B2EC0`; callers `0x6F0CD722`,
//!   `0x6F0D0CE5`), not at the damage point. The arming slot first clears the armed flag, then (not under the
//!   level-gate buff, count gate and target filters) takes ONE draw of combat stream **3** and compares
//!   `draw <= effective chance` (soft `le`, `0x6F01FD60`). On success the flag `0x200` is set and the hit counter
//!   (`+0x84`) is reset to 1, otherwise the counter is incremented.
//! * The effective chance is a PSEUDO-RANDOM DISTRIBUTION: `c` = `DataA` of the level in PERCENT units (the data
//!   holds 15 for 15 %), `table = PRD[clamp(f2i(c * 0.2 + 0.5), 0, 20)]` (`0x6F1D21F0`; 0.2 and 0.5 are the runtime
//!   constants `0x6FAAE4B0` / `0x6FAAE4D0`), effective = `I2R(counter) * table` (`0x6F1A3470`). The 21 table entries
//!   are `I2R(x) / I2R(10000)` for x = 0, 38, 147, 320, 547, 819, 1126, 1461, 1815, 2184, 2562, 2953, 3333, 3781,
//!   4203, 4607, 5000, 5799, 6666, 7706, 10000. Unicorn: 400 table cases and 400 effective-chance cases (random bit
//!   patterns incl. NaN/inf, negative and huge counters), 0 mismatches (`tools/emu/crit_emu.py`).
//! * The counter starts at 1 (the init slot `0x6F1A3440` writes `+0x84 = 1`).
//! * At the execute, right after the dice (`0x6F2763A0`, category `0xB`, slot `0x134` = `0x6F1A3590`): when the flag
//!   is set and `DataB` (the multiplier) is non-zero, `extra = mul(D, DataB) - D` is added to the record damage `D`
//!   through `0x6F255720`, which does nothing when the record already carries a positive bonus (`+0x18 > 0`) and
//!   otherwise stores the bonus in `+0x18`. The result feeds the target side (armor, damage type table), so the
//!   multiplier applies to the RAW damage (CONFIRMED order).
//! * `DataE` non-zero sets the record flag `0x400` (never miss: the miss gate is skipped). Not modelled (the 1.26a
//!   data has `DataE` empty). `DataC` is read by the after-hit slot `0x154` = `0x6F1CC530` (shared with Bash, flat
//!   bonus damage as a second hit; the 1.26a data has 0): not modelled. `DataD` (chance to evade) shares the evasion
//!   walk slots `0x158`/`0x164`; with `DataD` = 0 the score is 0 and the entry is never selected (CONFIRMED by
//!   reading `0x6F1A3540`/`0x6F1A34C0`; data 0).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

use crate::abilities::code;
use crate::cast::{effect_duration, BuffEffect, CastRec, CastTarget, SpellLevel, BUFF_STUN};
use crate::natives::game::{Game, ObjKey};
use crate::natives::unit_attr::is_unit_type;
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::unitclass::ty;

/// Critical Strike's base code.
pub const CRITICAL_STRIKE: u32 = code(b"AOcr");
/// The combat RNG stream of the arming roll (`mov edx, 3` before the draw at `0x6F1B2FD3`).
pub const CRIT_STREAM: usize = 3;

/// The 21 entries of the PRD table as float bits (Unicorn: `W` lines of `tests/data/orc/crit.txt`).
pub const PRD_TABLE: [u32; 21] = [
    0x0000_0000, 0x3B79_096C, 0x3C70_D845, 0x3D03_126F, 0x3D60_0D1C, 0x3DA7_BB30, 0x3DE6_9AD4, 0x3E15_9B3D,
    0x3E39_DB23, 0x3E5F_A440, 0x3E83_2CA5, 0x3E97_3190, 0x3EAA_A64C, 0x3EC1_9653, 0x3ED7_3190, 0x3EEB_E0DF,
    0x3F00_0000, 0x3F14_7454, 0x3F2A_A64C, 0x3F45_460B, 0x3F80_0000,
];

/// `0x6F1D21F0`: the PRD table value for the nominal chance `c` in percent units.
pub fn prd_value(chance: F32) -> F32 {
    let scaled = arith::mul(chance, F32::from_bits(0x3E4C_CCCD)); // 0.2
    let idx = conv::f2i(arith::add(scaled, F32::from_bits(0x3F00_0000))); // + 0.5
    let idx = idx.clamp(0, 20) as usize;
    F32::from_bits(PRD_TABLE[idx])
}

/// `0x6F1A3470`: the effective chance for the hit counter `count`.
pub fn effective_chance(chance: F32, count: i32) -> F32 {
    arith::mul(conv::i2r(count), prd_value(chance))
}

/// The arming decision: `draw <= effective` (soft `le`).
pub fn crit_armed(draw: F32, effective: F32) -> bool {
    arith::le(draw, effective)
}

/// The bonus handler `0x6F1A3590` + `0x6F255720` on a record with damage `damage` and no earlier bonus
/// (`+0x18 = 0`): the new damage `damage + (damage * mult - damage)`. A zero multiplier leaves the record alone.
pub fn crit_damage(damage: F32, mult: F32) -> F32 {
    // The x87 equality test skips only an exact zero (either sign); NaN goes on (unordered sets the parity flag).
    if mult.to_f32() == 0.0 {
        return damage;
    }
    let bonus = arith::sub(arith::mul(damage, mult), damage);
    arith::add(arith::sub(damage, F32::ZERO), bonus)
}

// ---- Mirror Image numeric cores (CONFIRMED in Unicorn: `tools/emu/mirror_emu.py`, `tests/data/orc/mirror.txt`) ----

/// The combat RNG stream of the slot pick of Mirror Image: the roll wrapper `0x6F285350` is called with `EDX = 0xC`
/// at `0x6F13DA6C` (CONFIRMED by disassembly and by the Unicorn run of the wrapper on stream 12).
pub const MIRROR_STREAM: usize = 12;
/// The cap of the polygon: `min(DataA, 9)` images (`cmp ebx, 9` at `0x6F13DAAC`).
pub const MIRROR_MAX_IMAGES: i32 = 9;
/// `2 pi` as the float constant `0x6FAAE608`.
const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);

/// The slot of the real hero among the `images + 1` polygon slots: `R2I(roll(1 die of images + 1 sides, stream
/// 12)) - 1` (`0x6F13DA6C..0x6F13DA82`). The sides use `DataA` itself, not the capped count.
pub fn mirror_pick(stream: &mut crate::rng::Rng, table: &crate::rng::RngTable, images: i32) -> Option<i32> {
    let sides = u32::try_from(images.saturating_add(1)).unwrap_or(0);
    let total = crate::damage::roll_total(stream, table, 1, sides, 0)?;
    Some(total.wrapping_sub(1))
}

/// `0x6F1127D0`: the offset of slot `k` of `n` on the circle of radius `r`: `(r * sin(a), r * cos(a))` with
/// `a = (I2R(k) * 2 pi) / I2R(n)` (soft float throughout).
pub fn mirror_offset(k: i32, n: i32, r: F32, t: &softfloat::Tables) -> (F32, F32) {
    let angle = arith::div(arith::mul(conv::i2r(k), TWO_PI), conv::i2r(n), t);
    let s = softfloat::math::sin(angle, t);
    let c = softfloat::math::cos(angle, t);
    (arith::mul(r, s), arith::mul(r, c))
}

/// `0x6F4AFCD0` for the axis `(0, 0, 1)`: the 3x3 rotation matrix (row-major) of the caster's facing, as the axis-angle
/// formula leaves it in the soft float: `m22 = c + (1 - c)` (so `0.99999994` for most angles), signed zeros as in the
/// original.
pub fn mirror_rotation(angle: F32, t: &softfloat::Tables) -> [F32; 9] {
    let s = softfloat::math::sin(angle, t);
    let c = softfloat::math::cos(angle, t);
    let neg_zero = F32::from_bits(0x8000_0000);
    [c, s, neg_zero, arith::neg(s), c, F32::ZERO, F32::ZERO, neg_zero, arith::add(c, arith::sub(F32::ONE, c))]
}

/// `0x6F07B950`: `out[c] = (v0 * m[c] + v1 * m[3 + c]) + v2 * m[6 + c]` (soft float, that association).
pub fn mirror_transform(m: &[F32; 9], v: [F32; 3]) -> [F32; 3] {
    let mut out = [F32::ZERO; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let a = arith::mul(v[0], m[c]);
        let b = arith::mul(v[1], m[3 + c]);
        let d = arith::mul(v[2], m[6 + c]);
        *o = arith::add(arith::add(a, b), d);
    }
    out
}

/// The other seven abilities' base codes.
pub const WIND_WALK: u32 = code(b"AOwk");
pub const MIRROR_IMAGE: u32 = code(b"AOmi");
pub const SHOCKWAVE: u32 = code(b"AOsh");
pub const WAR_STOMP: u32 = code(b"AOws");
pub const CHAIN_LIGHTNING: u32 = code(b"AOcl");
pub const FAR_SIGHT: u32 = code(b"AOfs");
pub const FERAL_SPIRIT: u32 = code(b"AOsf");
/// Buff codes.
pub const BUFF_WIND_WALK: u32 = code(b"BOwk");
pub const BUFF_FAR_SIGHT: u32 = code(b"BOfs");

/// Damage type of the spell hits (the class word `0x8000` of the stomp record, slot `0x42C` = `0x6F18F880`; the same
/// bit index as Storm Bolt in `cast.rs`).
const SPELL_DAMAGE_TYPE: u32 = 15;

/// Timer kinds of [`crate::cast::SpellMsg::HeroSpell`] used by the orc spells (the undead / night elf spells use
/// the small numbers).
pub const TICK_MIRROR: u8 = 0x81;
/// The next-hop timer of Chain Lightning.
pub const TICK_CHAIN: u8 = 0x82;

/// Original cast-order getters (slot 0x348), Unicorn-checked by tools/analysis/ability_orders.py.
pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    ids.insert(852_127, WAR_STOMP);
    ids.insert(852_125, SHOCKWAVE);
    ids.insert(852_119, CHAIN_LIGHTNING);
    ids.insert(852_129, WIND_WALK);
    ids.insert(852_123, MIRROR_IMAGE);
    ids.insert(852_122, FAR_SIGHT);
    ids.insert(852_126, FERAL_SPIRIT);
}

/// Abilities cast without a target (`targs` empty or area around the caster).
pub fn takes_no_target(base: u32) -> bool {
    matches!(base, WAR_STOMP | WIND_WALK | MIRROR_IMAGE | FERAL_SPIRIT)
}

/// Abilities cast on a point.
pub fn takes_point(base: u32) -> bool {
    matches!(base, SHOCKWAVE | FAR_SIGHT)
}

/// Per-game state of the orc abilities.
#[derive(Debug, Default, Clone)]
pub struct OrcState {
    /// Critical Strike hit counters (`+0x84`) per unit; absent = 1.
    crit_count: BTreeMap<ObjKey, i32>,
    /// Units whose Critical Strike is armed for the coming execute (flag `0x200`).
    crit_armed: BTreeSet<ObjKey>,
    /// The summoned unit type of each level of Feral Spirit (`UnitID`), index = level - 1.
    summon_units: BTreeMap<u32, Vec<u32>>,
    /// Mirror Image copies: (damage dealt factor, damage taken factor).
    illusions: BTreeMap<ObjKey, (F32, F32)>,
    /// Far Sight reveals: marker buff serial -> fog modifier.
    reveals: BTreeMap<u32, ObjKey>,
    /// Mirror Image: the images of the last cast of each caster (killed by the next cast).
    mirror_images: BTreeMap<ObjKey, Vec<ObjKey>>,
    /// Mirror Image casts waiting for the `DataD` delay: caster -> (cast serial, level record).
    mirror_pending: BTreeMap<ObjKey, (u32, SpellLevel)>,
    mirror_serial: u32,
    /// Chain Lightning chains in flight, by serial.
    chains: BTreeMap<u32, ChainState>,
    chain_serial: u32,
}

impl OrcState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Game {
    /// Installs the summoned unit types of an ability (`UnitID` per level).
    pub fn set_summon_units(&mut self, base: u32, units: Vec<u32>) {
        self.orc.summon_units.insert(base, units);
    }

    /// Swing start: the Critical Strike arming roll (see the module notes). One stream-3 draw per ability.
    pub(crate) fn orc_swing_start(&mut self, unit: ObjKey) {
        self.orc.crit_armed.remove(&unit);
        let alive = {
            let u = self.unit(unit);
            !u.dead && !u.removed
        };
        if !alive || self.unit(unit).abilities.get(CRITICAL_STRIKE).is_none_or(|a| a.is_removed()) {
            return;
        }
        let Some(record) = self.ability_level_record(unit, CRITICAL_STRIKE) else {
            return;
        };
        let count = *self.orc.crit_count.get(&unit).unwrap_or(&1);
        let eff = effective_chance(record.data[0], count);
        let Some((table, streams)) = self.rng_parts() else {
            return;
        };
        let draw = streams.stream_mut(CRIT_STREAM).random_fraction(table);
        if crit_armed(draw, eff) {
            self.orc.crit_armed.insert(unit);
            self.orc.crit_count.insert(unit, 1);
        } else {
            self.orc.crit_count.insert(unit, count.wrapping_add(1));
        }
    }

    /// The execute: applies the orc modifiers of an attacker to the raw damage (before armor): an armed Critical
    /// Strike (CONFIRMED order and arithmetic), then the Wind Walk first-hit bonus and the Mirror Image damage factor
    /// (their order relative to the crit is UNSURE).
    pub fn orc_crit_raw(&mut self, attacker: ObjKey, raw: F32) -> F32 {
        let mut raw = raw;
        if self.orc.crit_armed.contains(&attacker) {
            if let Some(record) = self.ability_level_record(attacker, CRITICAL_STRIKE) {
                raw = crit_damage(raw, record.data[1]);
            }
        }
        // Wind Walk: the first strike ends the buff and adds DataC (flat; UNSURE whether DataC is a flat bonus).
        let ww = self.unit(attacker).buffs.iter().find(|b| b.base == BUFF_WIND_WALK).map(|b| b.serial);
        if let Some(serial) = ww {
            if let Some(record) = self.ability_level_record(attacker, WIND_WALK) {
                raw = arith::add(raw, record.data[2]);
            }
            self.drop_spell_buff(serial);
        }
        if let Some(&(dealt, _)) = self.orc.illusions.get(&attacker) {
            raw = arith::mul(raw, dealt);
        }
        raw
    }

    /// The Mirror Image damage taken factor of an illusion (DataC); 1.0 for any other unit. Not applied to damage yet
    /// (UNSURE where the original applies it).
    pub fn illusion_taken_factor(&self, unit: ObjKey) -> F32 {
        self.orc.illusions.get(&unit).map_or(F32::ONE, |&(_, taken)| taken)
    }

    /// Whether the unit's Critical Strike is armed for its coming hit (for tests and the host).
    pub fn crit_is_armed(&self, unit: ObjKey) -> bool {
        self.orc.crit_armed.contains(&unit)
    }

    /// The Critical Strike hit counter of a unit (1 before any roll).
    pub fn crit_counter(&self, unit: ObjKey) -> i32 {
        *self.orc.crit_count.get(&unit).unwrap_or(&1)
    }
}


// ---- the class effects ----

fn spell_target_ok(gm: &Game, owner: u32, k: ObjKey) -> bool {
    let u = gm.unit(k);
    !u.dead && !u.removed && !u.hidden && gm.is_enemy(owner, u.owner) && !is_unit_type(gm, u, ty::MAGIC_IMMUNE)
}

/// Dispatches the effect of an orc hero ability. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        WAR_STOMP => war_stomp(game, rt, rec),
        CHAIN_LIGHTNING => chain_lightning(game, rt, rec),
        SHOCKWAVE => shockwave(game, rt, rec),
        WIND_WALK => wind_walk(game, rec),
        MIRROR_IMAGE => mirror_image(game, rt, rec),
        FERAL_SPIRIT => feral_spirit(game, rec),
        FAR_SIGHT => far_sight(game, rec),
        _ => return false,
    }
    true
}

/// War Stomp (`0x6F1608B0` instant slot, per-unit effect `0x6F1607B0`, CONFIRMED structure): every unit of the area
/// around the caster that the group builder lets through takes `DataA` damage (class `0x8000`), and a unit that is
/// still alive is stunned for `HeroDur` (hero) or `Dur` (slot `0x430` = `0x6F18ABF0`, same duration rule as Storm
/// Bolt). A unit the immunity test `0x6F28D750` rejects gets a zero-damage record and no stun (modelled as untouched).
/// The units come in the area query's order (x-major cells). UNSURE: the builder's relation filter (enemies of the
/// caster's owner here), the ground/organic target flags (`ground,organic`: no air, structures, mechanical).
fn war_stomp(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let (cx, cy, owner) = {
            let u = gm.unit(rec.caster);
            (u.x, u.y, u.owner)
        };
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
            if k == rec.caster || !spell_target_ok(&gm, owner, k) {
                continue;
            }
            let u = gm.unit(k);
            if !is_unit_type(&gm, u, ty::GROUND)
                || is_unit_type(&gm, u, ty::STRUCTURE)
                || is_unit_type(&gm, u, ty::MECHANICAL)
            {
                continue;
            }
            if let Some(t) = gm.unit_damage_target(rec.caster, k, rec.level.data[0], false, false, 0, SPELL_DAMAGE_TYPE) {
                killed.push(t);
                continue;
            }
            if gm.alive(k) {
                let hero = gm.target_hero(k);
                let dur = effect_duration(&rec.level, hero, F32::ZERO);
                gm.add_spell_buff(k, BUFF_STUN, BuffEffect::Stun, dur);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

// ---- Chain Lightning numeric cores (CONFIRMED in Unicorn: `tools/emu/chain_emu.py`, `tests/data/orc/chain.txt`) ----

/// The delay from the start of one bolt to the next hop: the bolt effect (`CEffectChainLightning`, start slot
/// `0x6F1B09C0`) starts the timer message `0xD01D2` with the float at `0x6FAAE4B8` (0.25) and the destroy message
/// `0xD01D3` with 1.0; the `0xD01D2` handler (effect slot `0x84` = `0x6F1C3360`) calls the ability's next-hop gate
/// `0x6F1C3200` (Unicorn: the hop passes the 0.25 and 1.0 pointers, `H` rows of `chain.txt`).
pub const CHAIN_HOP_DELAY_BITS: u32 = 0x3E80_0000;
/// The damage record flags of a hop: `0x400` (never miss), written by the hop body at `0x6F1C30EF`.
pub const CHAIN_RECORD_FLAGS: u32 = 0x400;

/// The damage of a hop: `DataA * factor` (soft multiply, `DataA` first), `0x6F1C2FA6`.
pub fn chain_hop_amount(data_a: F32, factor: F32) -> F32 {
    arith::mul(data_a, factor)
}

/// The factor after a hop: `factor * (1.0 - DataC)` (soft sub then mul), written through the agile cell setter
/// (`0x6F1C2FDA`..`0x6F1C2FEE`).
pub fn chain_next_factor(factor: F32, data_c: F32) -> F32 {
    arith::mul(factor, arith::sub(F32::ONE, data_c))
}

/// The next-hop gate `0x6F1C3200`: another hop only while `count < F2I(DataB)` (signed compare, `jge` ends the chain).
pub fn chain_gate_open(count: i32, data_b: F32) -> bool {
    count < conv::f2i(data_b)
}

/// The flag word handed to the next-target search (slot `0x43C`) by `0x6F1C3200`: the ability's target word
/// (`+0x4C`) with bits 12..16 cleared, bit 16 set, and bit 15 set when the ability flag `0x80000` (enemy cast) is on
/// (`0x6F1C3253`..`0x6F1C327E`; the slot `0x42C` is 0 for this class).
pub fn chain_next_flags(targs: u32, ability_flags: u32) -> u32 {
    let mut w = (targs & 0xFFFE_0FFF) | 0x1_0000;
    if ability_flags & 0x8_0000 != 0 {
        w |= 0x8000;
    }
    w
}

/// The state of one running Chain Lightning (the ability's factor cell `+0x128/+0x12C`, hit count `+0x124`, hit list
/// `+0x134`).
#[derive(Debug, Clone)]
pub(crate) struct ChainState {
    caster: ObjKey,
    level: SpellLevel,
    factor: F32,
    hits: Vec<ObjKey>,
}

/// Chain Lightning (class `0x6F8B0E74`; cast slot `0x424` = `0x6F1CB550`, hop body `0x6F1C2F50`, next-hop gate
/// `0x6F1C3200`, next-target search `0x6F1B6F30`; spec section 4).
///
/// CONFIRMED (Unicorn, the real hop body and gate on a fake ability: 400 + 400 cases, 0 mismatches): the cast sets the
/// factor cell to 1.0 and the hit count to 0 and hops once at once; a hop's damage is `DataA * factor`, the record flags
/// `0x400`, the factor becomes `factor * (1 - DataC)`, the target is appended to the hit list, and the bolt effect
/// starts the next-hop timer of 0.25 s. The gate: another hop only while `count < F2I(DataB)`.
/// CONFIRMED by disassembly only: the next target is the NEAREST valid unit (the collector `0x6F1B0780` keeps a
/// candidate only when its distance is strictly smaller, distance mode because slot `0x438` returns 1 and slot `0x434`
/// returns 0), never one already hit; the damage source of every hop is the caster. UNSURE: the instant the damage lands
/// (here at the hop; the bolt effect has a 0.1 s repeating fade timer `0xD01A3`, but the code that applies the record
/// was not found), the distance (the original measures to the candidate's footprint point, `0x6F476860` with flag 1,
/// here the centre distance), the relation filter (`0x6F1B06F0`: `0x6F284EE0(owner, unit, 0, 4)`; here enemy, not magic
/// immune, not structure or mechanical), the damage class, the vanishing of the chain when the last target is removed
/// (modelled: it ends).
fn chain_lightning(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let CastTarget::Unit(first) = rec.target else {
        return;
    };
    let serial = {
        let mut gm = game.borrow_mut();
        gm.orc.chain_serial = gm.orc.chain_serial.wrapping_add(1);
        let serial = gm.orc.chain_serial;
        gm.orc.chains.insert(
            serial,
            ChainState { caster: rec.caster, level: rec.level, factor: F32::ONE, hits: Vec::new() },
        );
        serial
    };
    chain_hop(game, rt, serial, first);
}

/// One hop at `target`: damage, factor, hit list, and the timer of the next hop.
fn chain_hop(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, serial: u32, target: ObjKey) {
    let killed = {
        let mut gm = game.borrow_mut();
        let Some(st) = gm.orc.chains.get_mut(&serial) else {
            return;
        };
        let amount = chain_hop_amount(st.level.data[0], st.factor);
        st.factor = chain_next_factor(st.factor, st.level.data[2]);
        st.hits.push(target);
        let caster = st.caster;
        gm.timers_start_hero(f32::from_bits(CHAIN_HOP_DELAY_BITS), TICK_CHAIN, caster, serial);
        gm.unit_damage_target(caster, target, amount, false, false, 0, SPELL_DAMAGE_TYPE)
    };
    if let Some(t) = killed {
        let caster = game.borrow().orc.chains.get(&serial).map(|s| s.caster);
        dispatch_unit_death_by(game, rt, t, caster);
    }
}

/// The next-hop timer (`0xD01D2`, handler `0x6F1C3360` -> gate `0x6F1C3200`).
fn chain_next(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, serial: u32) {
    let next = {
        let gm = game.borrow();
        let Some(st) = gm.orc.chains.get(&serial) else {
            return;
        };
        let last = st.hits.last().copied();
        let open = last.is_some_and(|l| !gm.unit(l).removed)
            && chain_gate_open(i32::try_from(st.hits.len()).unwrap_or(i32::MAX), st.level.data[1]);
        if !open {
            None
        } else {
            let last = last.expect("checked");
            let owner = gm.unit(st.caster).owner;
            let (cx, cy) = {
                let u = gm.unit(last);
                (u.x, u.y)
            };
            let mut best: Option<(ObjKey, F32)> = None;
            for k in crate::natives::group::circle_candidates(&gm, cx, cy, st.level.area) {
                if st.hits.contains(&k) || k == st.caster || !spell_target_ok(&gm, owner, k) {
                    continue;
                }
                let u = gm.unit(k);
                if is_unit_type(&gm, u, ty::STRUCTURE) || is_unit_type(&gm, u, ty::MECHANICAL) {
                    continue;
                }
                let d = arith::dist2(cx, cy, u.x, u.y);
                if best.is_none_or(|(_, bd)| arith::lt(d, bd)) {
                    best = Some((k, d));
                }
            }
            best.map(|(k, _)| k)
        }
    };
    match next {
        Some(k) => chain_hop(game, rt, serial, k),
        None => {
            game.borrow_mut().orc.chains.remove(&serial);
        }
    }
}

/// Shockwave (class `0x6F92075C`): a line missile from the caster towards the point. Data: `DataA` damage, `DataB`
/// damage cap, `DataC` travel distance, `Area`/`DataD` start/end width. The missile flight is the engine projectile
/// system (not decoded), so this is a MODEL (UNSURE): at the effect time every ground unit or structure of an enemy
/// whose distance to the segment is within the width is hit once, in the order of travel (projection along the line,
/// ties in the area query's order), for `min(DataA, DataB)`.
fn shockwave(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let CastTarget::Point(px, py) = rec.target else {
        return;
    };
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let (cx, cy, owner) = {
            let u = gm.unit(rec.caster);
            (u.x, u.y, u.owner)
        };
        let dx = arith::sub(px, cx);
        let dy = arith::sub(py, cy);
        let dd = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
        if !arith::lt(F32::ZERO, dd) {
            return;
        }
        let len = rec.level.data[2];
        let width = if arith::lt(rec.level.area, rec.level.data[3]) { rec.level.data[3] } else { rec.level.area };
        let reach = arith::add(len, width);
        let l2dd = arith::mul(arith::mul(len, len), dd);
        let w2dd = arith::mul(arith::mul(width, width), dd);
        let amount = if arith::lt(rec.level.data[1], rec.level.data[0]) { rec.level.data[1] } else { rec.level.data[0] };
        let mut ordered: Vec<(F32, ObjKey)> = Vec::new();
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, reach) {
            if k == rec.caster || !spell_target_ok(&gm, owner, k) {
                continue;
            }
            let u = gm.unit(k);
            if is_unit_type(&gm, u, ty::FLYING) {
                continue;
            }
            let vx = arith::sub(u.x, cx);
            let vy = arith::sub(u.y, cy);
            let dot = arith::add(arith::mul(vx, dx), arith::mul(vy, dy));
            let cross = arith::sub(arith::mul(vx, dy), arith::mul(vy, dx));
            if arith::lt(dot, F32::ZERO) || arith::lt(l2dd, arith::mul(dot, dot)) {
                continue;
            }
            if arith::lt(w2dd, arith::mul(cross, cross)) {
                continue;
            }
            // Stable insertion by the projection (equal keys keep the query order).
            let at = ordered.iter().position(|o| arith::lt(dot, o.0)).unwrap_or(ordered.len());
            ordered.insert(at, (dot, k));
        }
        for (_, k) in ordered {
            if let Some(t) = gm.unit_damage_target(rec.caster, k, amount, false, false, 0, SPELL_DAMAGE_TYPE) {
                killed.push(t);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// Wind Walk (class `0x6F8F2534`, effect `0x6F18CAA0`): the duration is `HeroDur` for a hero caster, else `Dur`; the
/// buff carries `DataB` (movement speed bonus, modelled as a speed factor `1 + DataB`) and `DataC` (the bonus of the
/// first strike, see [`Game::orc_crit_raw`]); `DataA` is the fade time (presentation). Invisibility is not modelled.
fn wind_walk(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let effect = BuffEffect::Slow { move_delta: rec.level.data[1], attack_delta: F32::ZERO };
    gm.add_spell_buff(rec.caster, BUFF_WIND_WALK, effect, dur);
}

fn rad_to_deg(r: F32) -> F32 {
    arith::mul(r, F32::from_bits(0x4265_2EE1))
}

/// Mirror Image (class `0x6F8D9E5C`, cast effect `0x6F135740`, delayed handler `0x6F13D990`). Spec section 7.
///
/// The cast effect (CONFIRMED structure) first runs the class slot `0x3E4` = `0x6F11B0C0`, which walks the list of the
/// images of the previous cast (12-byte entries at ability `+0xF4`, count `+0xF0`) and calls the unit slot `0x124` with
/// `0.0` on each (LIKELY: that slot sets the life, so they die), then marks the caster and starts the timer message
/// `0xD01C1` with `DataD` seconds (`0x6F02EB30`, 0.5). Everything else happens in the handler ([`mirror_fire`]).
fn mirror_image(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let old: Vec<ObjKey> = {
        let mut gm = game.borrow_mut();
        if !gm.alive(rec.caster) {
            return;
        }
        gm.orc.mirror_images.remove(&rec.caster).unwrap_or_default()
    };
    for k in old {
        let died = {
            let mut gm = game.borrow_mut();
            gm.alive(k) && crate::natives::set_life(&mut gm, k, F32::ZERO)
        };
        if died {
            dispatch_unit_death_by(game, rt, k, None);
        }
    }
    let mut gm = game.borrow_mut();
    gm.orc.mirror_serial = gm.orc.mirror_serial.wrapping_add(1);
    let serial = gm.orc.mirror_serial;
    gm.orc.mirror_pending.insert(rec.caster, (serial, rec.level));
    gm.timers_start_hero(rec.level.data[3].to_f32(), TICK_MIRROR, rec.caster, serial);
}

/// The timer handler of Mirror Image (message `0xD01C1`, `0x6F13D990`). CONFIRMED by disassembly, the numeric cores by
/// Unicorn ([`mirror_pick`], [`mirror_offset`], [`mirror_rotation`], [`mirror_transform`]):
/// 1. `n = F2I(DataA)`, slots `c + 1` with `c = min(n, 9)`; the hero's slot `h = R2I(roll(1 die of n + 1 sides, stream
///    12)) - 1` is drawn FIRST (one draw per cast).
/// 2. The polygon: slot `i` is at `centre + M * (r sin a_i, r cos a_i, 0)`, `a_i = 2 pi i / (c + 1)`, `r` = the level's
///    `Rng` (128), `M` the rotation about the vertical axis by the caster's facing.
/// 3. Pass one creates a unit of the caster's type at every slot, in ascending slot order (each allocates its handle id
///    when built). Pass two (ascending): the slot `h` takes the hero (the caster is moved there, the placeholder unit is
///    removed, freeing its id), every other unit becomes an image (illusion with the factors `DataB`/`DataC`, living
///    `Dur + ability+0x9C` seconds). LIKELY structure (the slot-`h` branch is read from the arguments of the pass-two
///    calls, not run); UNSURE: placement failures (the original drops the slot when the placement search fails), the
///    state of the caster during the delay, abilities and the 0xC8 buff application of the images.
///
/// Without the soft-float or RNG tables (a host that has none) the images stand at the caster, no draw is made.
fn mirror_fire(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, caster: ObjKey, serial: u32) {
    let _ = rt;
    let mut gm = game.borrow_mut();
    let Some((s, level)) = gm.orc.mirror_pending.get(&caster).copied() else {
        return;
    };
    if s != serial {
        return;
    }
    gm.orc.mirror_pending.remove(&caster);
    if gm.unit(caster).removed {
        return;
    }
    let n = conv::f2i(level.data[0]);
    let c = n.min(MIRROR_MAX_IMAGES);
    let slots = c.saturating_add(1);
    let (owner, type_id, cx, cy, facing) = {
        let u = gm.unit(caster);
        (u.owner, u.type_id, u.x, u.y, u.facing)
    };
    let tables = gm.softfloat_tables().cloned();
    let mut chosen: Option<i32> = None;
    if tables.is_some() {
        if let Some((table, streams)) = gm.rng_parts() {
            chosen = mirror_pick(streams.stream_mut(MIRROR_STREAM), table, n);
        }
    }
    // Pass one: the polygon and the units.
    let mut made: Vec<(i32, ObjKey, F32, F32)> = Vec::new();
    for i in 0..slots.max(0) {
        let (px, py) = match tables.as_ref() {
            Some(t) => {
                let (o1, o2) = mirror_offset(i, slots, level.rng, t);
                let m = mirror_rotation(facing, t);
                let o = mirror_transform(&m, [o1, o2, F32::ZERO]);
                (arith::add(o[0], cx), arith::add(o[1], cy))
            }
            None => (cx, cy),
        };
        // Without the tables there is no hero slot: only the images are made.
        if tables.is_none() && i >= c {
            continue;
        }
        let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, type_id, px, py, rad_to_deg(facing), true) else {
            continue;
        };
        made.push((i, k, px, py));
    }
    // Pass two.
    let dur = arith::add(level.dur, F32::ZERO);
    let mut images = Vec::new();
    for (i, k, px, py) in made {
        if chosen == Some(i) {
            let (x, y) = gm.clamp_unit_move(px, py);
            gm.store_unit_position(caster, x, y);
            crate::natives::unit::remove_unit(&mut gm, k);
            continue;
        }
        gm.unit_mut(k).illusion = true;
        gm.orc.illusions.insert(k, (level.data[1], level.data[2]));
        gm.apply_timed_life(k, crate::abilities::TIMED_LIFE, dur);
        images.push(k);
    }
    gm.orc.mirror_images.insert(caster, images);
}

/// Timer dispatch of the orc spells (`SpellMsg::HeroSpell` kinds from `0x80`).
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, aux: u32) {
    match kind {
        TICK_MIRROR => mirror_fire(game, rt, unit, aux),
        TICK_CHAIN => chain_next(game, rt, aux),
        _ => {}
    }
}

/// Feral Spirit (class `0x6F927B2C`, effect `0x6F1DF330`): `DataB` units of the level's `UnitID`, summoned, living
/// `Dur`/`HeroDur`. No RNG draw is reachable through direct calls. UNSURE: the placement (at the caster here).
fn feral_spirit(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let level_index =
        usize::try_from(gm.unit(rec.caster).abilities.get(FERAL_SPIRIT).map_or(0, |a| a.level_index)).unwrap_or(0);
    let Some(&unit_type) = gm.orc.summon_units.get(&FERAL_SPIRIT).and_then(|v| v.get(level_index)) else {
        return;
    };
    let count = conv::f2i(rec.level.data[1]).max(0);
    let (owner, x, y, facing) = {
        let u = gm.unit(rec.caster);
        (u.owner, u.x, u.y, u.facing)
    };
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    for _ in 0..count {
        let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, unit_type, x, y, rad_to_deg(facing), true) else {
            continue;
        };
        {
            let u = gm.unit_mut(k);
            u.summoned = true;
            u.class_flags |= 0x4;
        }
        gm.apply_timed_life(k, crate::abilities::TIMED_LIFE, dur);
    }
}

/// Far Sight (class `0x6F88B25C`): reveals the circle of `Area` around the point to the caster's owner for `Dur`.
/// MODEL (UNSURE): a visible-state fog modifier started now and stopped when the marker buff on the caster ends.
fn far_sight(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Point(px, py) = rec.target else {
        return;
    };
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let owner = gm.unit(rec.caster).owner;
    let cells = gm.fog_grid().circle_shape(px, py, rec.level.area);
    let handle = gm.fog_create(owner, crate::natives::fog::FOG_VISIBLE, cells, false, false);
    let Some(key) = gm.fog_modifier_of_handle(handle) else {
        return;
    };
    gm.fog_start(key);
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let serial = gm.add_spell_buff(rec.caster, BUFF_FAR_SIGHT, BuffEffect::Marker, dur);
    if serial == 0 {
        gm.fog_stop(key);
    } else {
        gm.orc.reveals.insert(serial, key);
    }
}

/// Called by `Game::drop_spell_buff` when a spell buff ends: stops the Far Sight reveal tied to it.
pub(crate) fn on_buff_dropped(game: &mut Game, serial: u32) {
    if let Some(key) = game.orc.reveals.remove(&serial) {
        game.fog_stop(key);
    }
}
