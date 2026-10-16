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

// ---- Wind Walk first strike (CONFIRMED in Unicorn: `tools/emu/ww_emu.py`, `tests/data/orc/windwalk.txt`) ----

/// The shared bonus writer `0x6F255720` on a damage record (`+0x10` damage `a`, `+0x18` bonus `b`): refused when the
/// record already carries a positive bonus (x87 `0.0 < b`; NaN goes on), otherwise `a := (a - b) + v` and `b := v`.
/// Critical Strike and the Wind Walk first strike both write through it, so whichever handler runs first keeps the record.
pub fn record_bonus(a: F32, b: F32, v: F32) -> Option<(F32, F32)> {
    if 0.0f32 < b.to_f32() {
        return None;
    }
    Some((arith::add(arith::sub(a, b), v), v))
}

/// The buff handler `0x6F1637B0` (slot `0x134`, category `0x0B`): with the buff flag `0x200` set it adds the buff bonus
/// (`buff+0xE8` = DataC) to the record through [`record_bonus`], ORs the record flag `0x400` when the buff flag `0x400`
/// (set by the constructor when DataD is non-zero) is set, and asks the buff to end in 0.01 s (slot `0x34C`, the
/// constant `[0x6FAAE480]`). Returns the new record `(damage, bonus, flags)`; `None` when the buff is not armed.
/// Unicorn: 400 cases, 0 mismatches (damage, bonus, flags, event and slot call).
pub fn ww_first_strike(buff_flags: u32, a: F32, b: F32, v: F32, rec_flags: u32) -> Option<(F32, F32, u32)> {
    if buff_flags & 0x200 == 0 {
        return None;
    }
    let (a, b) = record_bonus(a, b, v).unwrap_or((a, b));
    let flags = if buff_flags & 0x400 != 0 { rec_flags | 0x400 } else { rec_flags };
    Some((a, b, flags))
}

/// The arming slot `0x124` (`0x6F171870`, order broadcast `0xD000F` at the swing start): clears `0x200`; with a target and
/// buff flag `0x100` clear the target filter `0x6F26D590(owner, target, buff+0xB8)` decides: zero arms the buff (flag
/// `0x200`, owner event 0xE), non-zero removes the buff (`0x6F079CC0`). Returns `(flags, removed, event)`.
pub fn ww_arm(has_target: bool, buff_flags: u32, filter_answer: u32) -> (u32, bool, bool) {
    let flags = buff_flags & !0x200;
    if !has_target || flags & 0x100 != 0 {
        return (flags, false, false);
    }
    if filter_answer == 0 {
        (flags | 0x200, false, true)
    } else {
        (flags, true, false)
    }
}

/// Seconds until the Wind Walk buff ends after the strike: `[0x6FAAE480]` = 0.01 (`0x3C23D70A`).
pub const WW_END_DELAY_BITS: u32 = 0x3C23_D70A;
/// Timer kind that ends the buff after the first strike.
pub const TICK_WW_END: u8 = 0x83;
/// Timer kind of the Bladestorm damage tick (message `0xD01C1`).
pub const TICK_BLADE: u8 = 0x84;

// ---- Bladestorm set-up (CONFIRMED in Unicorn: `tools/emu/bs_emu.py`, `tests/data/orc/bladestorm.txt`) ----

/// The tick rate constant `[0x6FAB1F4C]` = 5.0 (ticks per second); the tick period is its soft reciprocal, 0.2 s.
pub const BLADESTORM_TICK_RATE_BITS: u32 = 0x40A0_0000;

/// The total duration of a Bladestorm: `Dur + ability+0x9C` (soft add); the class initialiser `0x6F163570` uses `Dur` for
/// heroes too (no `HeroDur`).
pub fn bladestorm_total(dur: F32, bonus: F32) -> F32 {
    arith::add(dur, bonus)
}

/// The tick count `0x6F163570` stores at `+0xF8`: `f2i(total * 5.0)`, at least 1.
pub fn bladestorm_ticks(total: F32) -> i32 {
    let n = conv::f2i(arith::mul(total, F32::from_bits(BLADESTORM_TICK_RATE_BITS)));
    if n > 0 {
        n
    } else {
        1
    }
}

/// The damage of one tick (the value `+0x104` of the agile cell `+0x100`): `DataA * total / I2R(ticks)`.
pub fn bladestorm_tick_damage(data_a: F32, total: F32, ticks: i32, t: &softfloat::Tables) -> F32 {
    arith::div(arith::mul(data_a, total), conv::i2r(ticks), t)
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
/// Serpent Ward: the ability id is `AOsw` (its base code in the factory is `AOwd`, class `CAbilityWard`).
pub const SERPENT_WARD: u32 = code(b"AOsw");
/// The factory base code of the Serpent Ward (`CAbilityWard`): a unit that carries the base row runs the same effect.
pub const SERPENT_WARD_BASE: u32 = code(b"AOwd");
pub const HEALING_WAVE: u32 = code(b"AOhw");
pub const BLADESTORM: u32 = code(b"AOww");
pub const BUFF_BLADESTORM: u32 = code(b"BOww");
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

/// Original cast-order getters (slot 0x348, Unicorn-checked by tools/analysis/ability_orders.py; healing wave 852501 agrees
/// with slot 0x308 = 0xD0215).
pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    ids.insert(852_127, WAR_STOMP);
    ids.insert(852_125, SHOCKWAVE);
    ids.insert(852_119, CHAIN_LIGHTNING);
    ids.insert(852_129, WIND_WALK);
    ids.insert(852_123, MIRROR_IMAGE);
    ids.insert(852_122, FAR_SIGHT);
    ids.insert(852_126, FERAL_SPIRIT);
    ids.insert(852_504, SERPENT_WARD);
    ids.insert(852_502, HEX);
    ids.insert(852_503, VOODOO);
    ids.insert(852_121, EARTHQUAKE);
    ids.insert(852_501, HEALING_WAVE);
    ids.insert(852_128, BLADESTORM);
}

/// Abilities cast without a target (`targs` empty or area around the caster).
pub fn takes_no_target(base: u32) -> bool {
    matches!(base, WAR_STOMP | WIND_WALK | MIRROR_IMAGE | FERAL_SPIRIT | BLADESTORM | VOODOO)
}

/// Abilities cast on a point.
pub fn takes_point(base: u32) -> bool {
    matches!(base, SHOCKWAVE | FAR_SIGHT | SERPENT_WARD | SERPENT_WARD_BASE | EARTHQUAKE)
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
    /// Units whose Wind Walk buff is armed for the coming execute (buff flag `0x200`).
    ww_armed: BTreeSet<ObjKey>,
    /// Bladestorms in progress, by serial.
    blades: BTreeMap<u32, BladeState>,
    blade_serial: u32,
    /// The `targs` word of each level of an ability (host data); absent = the shipped default.
    targs: BTreeMap<u32, Vec<u32>>,
    /// The creature lists (`DataB`..`DataE` texts) of each level of an ability (host data).
    lists: BTreeMap<u32, Vec<[String; 4]>>,
    /// Hexed units: unit -> (buff serial, the creature it became).
    hexed: BTreeMap<ObjKey, (u32, u32)>,
    /// Big Bad Voodoos in progress, by serial, and the buffs they gave (buff serial -> unit, for the invulnerability count).
    voodoos: BTreeMap<u32, VoodooState>,
    voodoo_serial: u32,
    voodoo_buffs: BTreeMap<u32, ObjKey>,
    /// Earthquakes in progress, by serial.
    quakes: BTreeMap<u32, QuakeState>,
    quake_serial: u32,
    /// Pending Reincarnations: unit -> serial of the revival timer.
    reincarnating: BTreeMap<ObjKey, u32>,
    reincarnate_serial: u32,
}

impl OrcState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Game {
    /// Installs the `targs` words of an ability's levels (the parsed `targs` column).
    pub fn set_orc_targs(&mut self, base: u32, words: Vec<u32>) {
        self.orc.targs.insert(base, words);
    }

    /// The `targs` word of a level (`level_index` 0-based, clamped to the last level), or the shipped default.
    pub(crate) fn orc_targs(&self, base: u32, level_index: usize) -> u32 {
        match self.orc.targs.get(&base) {
            Some(v) if !v.is_empty() => v[level_index.min(v.len() - 1)],
            _ => match base {
                WAR_STOMP => STOMP_TARGS_DEFAULT,
                VOODOO => VOODOO_TARGS_DEFAULT,
                BLADESTORM => BLADESTORM_TARGS_DEFAULT,
                EARTHQUAKE => QUAKE_TARGS_DEFAULT,
                _ => 0,
            },
        }
    }

    /// Installs the summoned unit types of an ability (`UnitID` per level).
    pub fn set_summon_units(&mut self, base: u32, units: Vec<u32>) {
        self.orc.summon_units.insert(base, units);
    }

    /// The critical-strike ability a unit has: `AOcr`, or the Drunken Brawler (`ANdb`, a `CAbilityCriticalStrike` with the
    /// same `DataA` chance and `DataB` multiplier, S44).
    pub(crate) fn crit_base(&self, unit: ObjKey) -> Option<u32> {
        [CRITICAL_STRIKE, crate::neutral_spells::BRAWLER]
            .into_iter()
            .find(|&c| self.unit(unit).abilities.get(c).is_some_and(|a| !a.is_removed()))
    }

    /// Swing start: the Critical Strike arming roll (see the module notes). One stream-3 draw per ability.
    pub fn orc_swing_start(&mut self, unit: ObjKey) {
        // The Wind Walk buff arms itself on the same broadcast (target filter assumed to accept, UNSURE).
        self.orc.ww_armed.remove(&unit);
        if self.unit(unit).buffs.iter().any(|b| b.base == BUFF_WIND_WALK) {
            self.orc.ww_armed.insert(unit);
        }
        self.orc.crit_armed.remove(&unit);
        let alive = {
            let u = self.unit(unit);
            !u.dead && !u.removed
        };
        let Some(crit) = self.crit_base(unit).filter(|_| alive) else {
            return;
        };
        let Some(record) = self.ability_level_record(unit, crit) else {
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
        let mut bonus = F32::ZERO;
        // The walk of `0x6F2763A0` visits the unit's abilities newest first (`0x6F079990` inserts at the list head), so
        // the Wind Walk buff (added at cast time) comes before Critical Strike: it takes the record's bonus field and a
        // positive bonus makes the Critical Strike writer refuse (`record_bonus`). CONFIRMED order rule; the exception
        // (Critical Strike learned while the buff is up) is not modelled.
        if self.orc.ww_armed.remove(&attacker) {
            let ww = self.unit(attacker).buffs.iter().find(|b| b.base == BUFF_WIND_WALK).map(|b| b.serial);
            if let (Some(serial), Some(record)) = (ww, self.ability_level_record(attacker, WIND_WALK)) {
                if let Some((a, b, _)) = ww_first_strike(0x200, raw, bonus, record.data[2], 0) {
                    raw = a;
                    bonus = b;
                }
                self.timers_start_hero(f32::from_bits(WW_END_DELAY_BITS), TICK_WW_END, attacker, serial);
            }
        }
        if self.orc.crit_armed.contains(&attacker) {
            if let Some(record) = self.crit_base(attacker).and_then(|c| self.ability_level_record(attacker, c)) {
                let mult = record.data[1];
                // The x87 equality test skips only an exact zero; NaN goes on.
                if mult.to_f32() != 0.0 {
                    let extra = arith::sub(arith::mul(raw, mult), raw);
                    if let Some((a, b)) = record_bonus(raw, bonus, extra) {
                        raw = a;
                        bonus = b;
                    }
                }
            }
        }
        let _ = bonus;
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
        CHAIN_LIGHTNING => chain_lightning(game, rt, rec, false),
        HEALING_WAVE => chain_lightning(game, rt, rec, true),
        BLADESTORM => bladestorm(game, rt, rec),
        SHOCKWAVE => shockwave(game, rt, rec),
        WIND_WALK => wind_walk(game, rec),
        MIRROR_IMAGE => mirror_image(game, rt, rec),
        FERAL_SPIRIT => feral_spirit(game, rec),
        SERPENT_WARD | SERPENT_WARD_BASE => serpent_ward(game, rec),
        HEX => hex_cast(game, rt, rec),
        VOODOO => voodoo(game, rec),
        EARTHQUAKE => earthquake(game, rec),
        FAR_SIGHT => far_sight(game, rec),
        _ => return false,
    }
    true
}

/// The target flag word the stomp area builder uses (`0x6F176CD0`, slot `0x434` shared with Thunder Clap and Storm Bolt;
/// CONFIRMED by reading): `(targs & 0xFFFF0FFF) | 0x10000`, and `| 0x8000` when the third argument is non-zero, which
/// the stomp's instant slot `0x6F1608B0` passes as 1. So the relation bits are enemy and neutral only (self, player
/// and ally are cleared), and the rest of the ability's `targs` goes to the predicate unchanged.
pub fn stomp_word(targs: u32) -> u32 {
    (targs & 0xFFFF_0FFF) | 0x1_0000 | 0x8000
}

/// The `targs` text of War Stomp in the 1.26a AbilityData (`ground,organic`) as a word (`ground` 0x2, `organic`
/// `0x4000000`), used when the host gives none (`Game::set_orc_targs`).
pub const STOMP_TARGS_DEFAULT: u32 = 0x0400_0002;

/// What the per-unit body of War Stomp (`0x6F1607B0`) does with one unit that the area builder let through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StompUnit {
    /// The damage record is sent with the zero amount (the unit is invulnerable by slot `0x11C` or the spell
    /// immunity test `0x6F28D750` refused it); its flags are `0x50` instead of `0x40`.
    pub zero_record: bool,
    /// The unit is put into the ability's hit array (`array[owner index + 1]`), when the handle resolves.
    pub hit_slot: Option<u32>,
    /// The stun slot `0x430` runs.
    pub stun: bool,
}

/// The damage record flags of the stomp: `0x40`, plus `0x10` on the zero-damage path. CONFIRMED (Unicorn).
pub fn stomp_record_flags(zero_record: bool) -> u32 {
    0x40 | if zero_record { 0x10 } else { 0 }
}

/// `0x6F1607B0` after the damage: `resolved` = the handle resolver `0x6F282F60` still returns the unit (valid, not
/// removed, not dead), `target_type` = the unit's slot `0x114` result (`unit+0x24C`: bit 8 = structure). The stun runs
/// only for a resolved, non-structure unit on the normal path. CONFIRMED by Unicorn (`tests/data/orc/stomp.txt`).
pub fn stomp_unit(zero_record: bool, resolved: bool, target_type: u32, owner_index: u32) -> StompUnit {
    StompUnit {
        zero_record,
        hit_slot: resolved.then_some(owner_index),
        stun: resolved && target_type & 8 == 0 && !zero_record,
    }
}

/// The candidate facts of a unit for the splash / area predicate (same construction as the bounce picker).
pub(crate) fn splash_candidate(gm: &Game, k: ObjKey) -> crate::splash::Candidate {
    let u = gm.unit(k);
    crate::splash::Candidate {
        dead: u.dead,
        invulnerable_flag: gm.unit_is_invulnerable(k),
        targ_type: gm.target_flag_word(k) & 0x1E,
        type_char: (u.type_id >> 24) as u8,
        class_word: u.class_flags,
        ethereal: u.ethereal,
        magic_immune: u.magic_immune,
        invulnerable: gm.unit_is_invulnerable(k),
        owner: u.owner,
    }
}

/// The area builder's candidate test for the `0x1D` tag (predicate `0x6F26E510` -> `0x6F25D6F0(unit, word, source,
/// -1)`, CONFIRMED by reading): the splash candidate filter without the attack-type gates.
pub(crate) fn area_predicate(gm: &Game, caster: ObjKey, k: ObjKey, word: u32) -> bool {
    let cand = splash_candidate(gm, k);
    let owner = gm.unit(caster).owner;
    let src = if k == caster { crate::splash::Source::Itself } else { crate::splash::Source::Other(owner) };
    let passive = |a: u32, b: u32| a < 16 && b < 16 && gm.player(a).alliances[0] & (1 << b) != 0;
    let shared = |a: u32, b: u32| a < 16 && b < 16 && gm.player(a).alliances[4] & (1 << b) != 0;
    crate::splash::candidate_filter(&cand, word, src, owner, false, false, passive, shared)
}

/// War Stomp (`0x6F1608B0` instant slot, per-unit body `0x6F1607B0`, CONFIRMED by reading and Unicorn): the area
/// builder (`0x6F2A3270` with the tags area, `0x1D` word [`stomp_word`], mode 0xF) walks the circle around the CASTER in
/// the area query's order and keeps the units the predicate `0x6F25D6F0` accepts (alive, vulnerable, kind and
/// organic/mechanical from the `targs`, relation enemy or neutral). The body sends `DataA` (class `0x8000`, flags
/// `0x40`) to every unit; a unit that is invulnerable by slot `0x11C` or fails the spell immunity test gets the zero
/// record (flags `0x50`) and no stun. A unit still alive and not a structure (`targType` bit 8) is stunned `HeroDur`
/// (hero) or `Dur` (+ `+0x9C`, slot `0x430` = `0x6F18ABF0`, the Storm Bolt rule).
fn war_stomp(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let (cx, cy) = {
            let u = gm.unit(rec.caster);
            (u.x, u.y)
        };
        let level_index =
            usize::try_from(gm.unit(rec.caster).abilities.get(WAR_STOMP).map_or(0, |a| a.level_index)).unwrap_or(0);
        let word = stomp_word(gm.orc_targs(WAR_STOMP, level_index));
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden {
                    continue;
                }
            }
            if !area_predicate(&gm, rec.caster, k, word) {
                continue;
            }
            let zero_record = gm.unit_is_invulnerable(k) || is_unit_type(&gm, gm.unit(k), ty::MAGIC_IMMUNE);
            let amount = if zero_record { F32::ZERO } else { rec.level.data[0] };
            if let Some(t) = gm.unit_damage_target(rec.caster, k, amount, false, false, 0, SPELL_DAMAGE_TYPE) {
                killed.push(t);
            }
            let target_type = gm.target_flag_word(k) & 0x1E;
            let resolved = gm.alive(k);
            let out = stomp_unit(zero_record, resolved, target_type, gm.unit(k).owner);
            if out.stun {
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

/// The flag word of the next-target search for a class whose slot `0x42C` is `heals` (`0x6F1C3253`..`0x6F1C327E`):
/// the base is `0x10000` for Chain Lightning and `0x7000` for Healing Wave (`neg; sbb; and 0xFFFF7000; add 0x10000`).
/// Unicorn: `HG` rows of `tests/data/orc/heal.txt` (400 cases, 0 mismatches).
pub fn chain_next_flags_for(heals: bool, targs: u32, ability_flags: u32) -> u32 {
    let base = if heals { 0x7000 } else { 0x1_0000 };
    let mut w = (targs & 0xFFFE_0FFF) | base;
    if ability_flags & 0x8_0000 != 0 {
        w |= 0x8000;
    }
    w
}

/// The record amount of a hop: Healing Wave (slot `0x42C` non-zero) negates it (sign bit flipped, `0x6F1C30C8`..), the
/// record then heals. Unicorn `HH` rows of `heal.txt`.
pub fn chain_hop_record_amount(heals: bool, data_a: F32, factor: F32) -> F32 {
    let a = chain_hop_amount(data_a, factor);
    if heals {
        F32::from_bits(a.to_bits() ^ 0x8000_0000)
    } else {
        a
    }
}

/// The destroy delay of the bolt effect: the float at `0x6FAAE4F4` (1.0) for Chain Lightning, at `0x6FAAE504` (2.0) for
/// Healing Wave (`0x6F1C3144`..`0x6F1C314B`). Unicorn `HH` rows.
pub fn chain_destroy_delay_bits(heals: bool) -> u32 {
    if heals {
        0x4000_0000
    } else {
        0x3F80_0000
    }
}

/// `0x6F01FE40`: `|a - b| < 2e-7` (soft subtract, sign cleared, an x87 compare against `[0x6FAAE614]`; NaN is false).
pub fn approx_equal_tiny(a: F32, b: F32) -> bool {
    let d = F32::from_bits(arith::sub(a, b).to_bits() & 0x7FFF_FFFF);
    f32::from_bits(0x3456_BF95) > d.to_f32()
}

/// The class parameters read by the candidate collector `0x6F1B0780`: slot `0x42C` (`heals`), `0x434` (score mode) and
/// `0x438` (distance mode). Chain Lightning: (false, false, true); Healing Wave: (true, true, false).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainClass {
    pub heals: bool,
    pub score_mode: bool,
    pub dist_mode: bool,
}

pub const CHAIN_CLASS: ChainClass = ChainClass { heals: false, score_mode: false, dist_mode: true };
pub const HEAL_CLASS: ChainClass = ChainClass { heals: true, score_mode: true, dist_mode: false };

/// One candidate as the collector sees it. `dist` is the squared distance `0x6F476860` returns (the model supplies it).
#[derive(Debug, Clone, Copy)]
pub struct ChainCandidate {
    pub life: F32,
    pub max_life: F32,
    pub dist: F32,
    /// `unit+0x5C`; bit `0x40000000` = illusion.
    pub flags_5c: u32,
    /// `unit+0xEC` (signed).
    pub field_ec: i32,
    /// `0x6F284EE0`: the owner's player can see the unit.
    pub visible: bool,
    /// `0x6F28D750` (immunity vs. the ability's slot `0x348` word).
    pub immune: bool,
    /// `0x6F1A2110`: already in the hit list.
    pub already_hit: bool,
    /// Misc `IllusionsCanRestoreLife`.
    pub illusions_restore: bool,
}

/// The search record the collector updates (`S+0x14` best distance, `S+0x18` best score).
#[derive(Debug, Clone, Copy)]
pub struct ChainBest {
    pub dist: F32,
    pub score: F32,
}

/// The validity chain `0x6F1B06F0` + slot `0x3F0` (`0x6F1B0650`). Unicorn `CB` rows (800 cases).
pub fn chain_candidate_valid(class: ChainClass, ability_flags: u32, c: &ChainCandidate) -> bool {
    if !c.visible {
        return false;
    }
    if class.heals {
        // slot 0x3F0: a unit at full life (|life - max| < 2e-7) is refused; an illusion is refused unless Misc allows.
        if approx_equal_tiny(c.life, c.max_life) {
            return false;
        }
        if !c.illusions_restore && c.flags_5c & 0x4000_0000 != 0 {
            return false;
        }
    }
    if c.immune || c.already_hit {
        return false;
    }
    // flag 0x100000 clear, slot 0x42C zero: a candidate with `+0xEC > 0` is refused.
    if ability_flags & 0x10_0000 == 0 && !class.heals && c.field_ec > 0 {
        return false;
    }
    true
}

/// The collector `0x6F1B0780` for one candidate: returns whether it became the new best and updates `best`.
/// Distance mode (Chain Lightning): strictly nearer wins (soft `lt`). Score mode (Healing Wave): the score is
/// `max - life` (soft sub) for a healer, the life alone otherwise; strictly greater (soft `gt`) wins. Unicorn
/// `tests/data/orc/heal.txt` `CB` rows: 800 cases (hits and misses of both classes), 0 mismatches.
pub fn chain_candidate_update(class: ChainClass, ability_flags: u32, c: &ChainCandidate, best: &mut ChainBest) -> bool {
    if !chain_candidate_valid(class, ability_flags, c) {
        return false;
    }
    if class.dist_mode {
        if arith::lt(c.dist, best.dist) {
            best.dist = c.dist;
            return true;
        }
        return false;
    }
    if class.score_mode {
        let score = if class.heals { arith::sub(c.max_life, c.life) } else { c.life };
        if arith::gt(score, best.score) {
            best.score = score;
            return true;
        }
    }
    false
}

/// The state of one running Chain Lightning (the ability's factor cell `+0x128/+0x12C`, hit count `+0x124`, hit list
/// `+0x134`).
#[derive(Debug, Clone)]
pub(crate) struct ChainState {
    /// Healing Wave (class slot `0x42C` non-zero): the hop heals instead of hurting.
    heals: bool,
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
fn chain_lightning(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, heals: bool) {
    let CastTarget::Unit(first) = rec.target else {
        return;
    };
    let serial = {
        let mut gm = game.borrow_mut();
        gm.orc.chain_serial = gm.orc.chain_serial.wrapping_add(1);
        let serial = gm.orc.chain_serial;
        gm.orc.chains.insert(
            serial,
            ChainState { heals, caster: rec.caster, level: rec.level, factor: F32::ONE, hits: Vec::new() },
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
        let heals = st.heals;
        gm.timers_start_hero(f32::from_bits(CHAIN_HOP_DELAY_BITS), TICK_CHAIN, caster, serial);
        if heals {
            // The record carries the negated amount; the model adds the life, clamped to the maximum (as Holy Light).
            if gm.alive(target) {
                let u = gm.unit(target);
                let sum = arith::add(u.life, amount);
                let next = if arith::lt(u.max_life, sum) { u.max_life } else { sum };
                crate::natives::unit::set_life(&mut gm, target, next);
            }
            None
        } else {
            gm.unit_damage_target(caster, target, amount, false, false, 0, SPELL_DAMAGE_TYPE)
        }
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
            let class = if st.heals { HEAL_CLASS } else { CHAIN_CLASS };
            let mut best_unit: Option<ObjKey> = None;
            let mut best = ChainBest { dist: F32::from_bits(0x7F7F_FFFF), score: F32::ZERO };
            for k in crate::natives::group::circle_candidates(&gm, cx, cy, st.level.area) {
                if st.hits.contains(&k) || k == st.caster {
                    continue;
                }
                let u = gm.unit(k);
                if u.dead || u.removed || u.hidden {
                    continue;
                }
                let enemy = gm.is_enemy(owner, u.owner);
                if st.heals == enemy {
                    continue;
                }
                if st.heals {
                    if is_unit_type(&gm, u, ty::STRUCTURE) || is_unit_type(&gm, u, ty::MECHANICAL) {
                        continue;
                    }
                } else if is_unit_type(&gm, u, ty::MAGIC_IMMUNE)
                    || is_unit_type(&gm, u, ty::STRUCTURE)
                    || is_unit_type(&gm, u, ty::MECHANICAL)
                {
                    continue;
                }
                let cand = ChainCandidate {
                    life: u.life,
                    max_life: u.max_life,
                    dist: arith::dist2(cx, cy, u.x, u.y),
                    flags_5c: 0,
                    field_ec: 0,
                    visible: true,
                    immune: false,
                    already_hit: false,
                    illusions_restore: false,
                };
                if chain_candidate_update(class, 0x10_0000, &cand, &mut best) {
                    best_unit = Some(k);
                }
            }
            best_unit
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
    // The invisibility counter rises (`invisibility.rs`: the fade-in starts).
    gm.invisibility_note(rec.caster);
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
    // The cast effect `0x6F135740` makes the caster invulnerable for the delay (`0x6F0796F0`: counter `+0xE8`, unit flag 8),
    // disables its abilities (`0x6F0767F0`, pause counters) and sets `+0x20 |= 0x800000`; the hero-slot branch of the handler
    // undoes all three (`0x6F1280D0` -> `0x6F079730`, `0x6F0767F0`, flag clear) and issues a stop order (`0x6F2A4B80`).
    let had = gm.orc.mirror_pending.insert(rec.caster, (serial, rec.level)).is_some();
    if !had {
        gm.unit_mut(rec.caster).invuln_count += 1;
    }
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
    {
        let u = gm.unit_mut(caster);
        u.invuln_count = (u.invuln_count - 1).max(0);
    }
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

// ---- Bladestorm model ----

/// A Bladestorm in progress (ability fields `+0xFC` ticks left, cells `+0x100` damage and `+0x108` radius).
#[derive(Debug, Clone)]
pub(crate) struct BladeState {
    caster: ObjKey,
    left: i32,
    damage: F32,
    radius: F32,
    buff: u32,
}

/// Bladestorm (class `0x6F8F2144`, order 852128): the effect `0x6F1712D0` starts a repeating timer (message `0xD01C1`,
/// period = soft reciprocal of 5.0 = 0.2 s), disables the caster's attack and orders; the level initialiser (Unicorn,
/// `bladestorm_*`) fixes the tick count `f2i((Dur + x) * 5)` (min 1) and the damage per tick `DataA * total / ticks`. Every
/// tick (`0x6F1912A0`) decrements the counter, ends the spin when it reaches 0 and STILL deals that tick's damage
/// (the ring `(Area, damage)` around the caster through the area damage body `0x6F2C31D0`, damage class `0x20`).
/// UNSURE: the target filter (here: living enemy ground non-structure units), the 0.2 s tick being a repeating timer
/// (modelled as a re-armed one-shot), the missing softfloat tables fallback (0.2 s, no damage rounding).
fn bladestorm(game: &RefCell<Game>, _rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let total = bladestorm_total(rec.level.dur, F32::ZERO);
    let ticks = bladestorm_ticks(total);
    let damage = match gm.softfloat_tables() {
        Some(t) => bladestorm_tick_damage(rec.level.data[0], total, ticks, t),
        None => rec.level.data[0],
    };
    let buff = gm.add_spell_buff(rec.caster, BUFF_BLADESTORM, BuffEffect::Marker, total);
    gm.orc.blade_serial = gm.orc.blade_serial.wrapping_add(1);
    let serial = gm.orc.blade_serial;
    gm.orc.blades.insert(serial, BladeState { caster: rec.caster, left: ticks, damage, radius: rec.level.area, buff });
    let period = f32::from_bits(blade_period_bits(&gm));
    gm.timers_start_hero(period, TICK_BLADE, rec.caster, serial);
}

fn blade_period_bits(gm: &Game) -> u32 {
    match gm.softfloat_tables() {
        Some(t) => arith::recip(F32::from_bits(BLADESTORM_TICK_RATE_BITS), t).to_bits(),
        None => 0x3E4C_CCCD,
    }
}

/// The shipped `targs` of Bladestorm: `ground,structure,debris,enemy,neutral` = ground 2, structure 8, debris `0x100`,
/// enemy `0x10000`, neutral `0x8000`.
pub const BLADESTORM_TARGS_DEFAULT: u32 = 0x0001_810A;

/// One tick of the spin (`0x6F1912A0`, CONFIRMED by reading and Unicorn, `tools/emu/bladetick_emu.py`, 400 cases): the
/// counter `+0xFC` is decremented first (wrapping); when it is then exactly 0 the end routine `0x6F171450` runs; then, when
/// the pause counter `+0x3C` is NOT positive (signed compare), the area body `0x6F2C31D0` is called with ONE ring
/// `(radius = +0x10C cell, amount = +0x104 cell)`, class `0x20`, the ability's `+0x4C` word unchanged, the caster as source
/// and owner -1. Returns `(counter after, end routine runs, damage is dealt)`.
pub fn blade_tick_step(left: i32, pause: i32) -> (i32, bool, bool) {
    let after = left.wrapping_sub(1);
    (after, after == 0, pause <= 0)
}

fn blade_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, serial: u32) {
    let mut killed = Vec::new();
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(st) = gm.orc.blades.get_mut(&serial) else {
            return;
        };
        caster = st.caster;
        let (left, ends, deals) = blade_tick_step(st.left, 0);
        st.left = left;
        let (damage, radius, buff) = (st.damage, st.radius, st.buff);
        let _ = deals;
        if !gm.alive(caster) {
            gm.orc.blades.remove(&serial);
            return;
        }
        if ends || left < 0 {
            gm.orc.blades.remove(&serial);
            gm.drop_spell_buff(buff);
        } else {
            let period = f32::from_bits(blade_period_bits(&gm));
            gm.timers_start_hero(period, TICK_BLADE, caster, serial);
        }
        let (cx, cy) = {
            let u = gm.unit(caster);
            (u.x, u.y)
        };
        let level_index =
            usize::try_from(gm.unit(caster).abilities.get(BLADESTORM).map_or(0, |a| a.level_index)).unwrap_or(0);
        let word = gm.orc_targs(BLADESTORM, level_index);
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, radius) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden {
                    continue;
                }
            }
            if !area_predicate(&gm, caster, k, word) {
                continue;
            }
            // Damage class 0x20 = 1 << 5 (damage type ENHANCED).
            if let Some(t) = gm.unit_damage_target(caster, k, damage, false, false, 0, 5) {
                killed.push(t);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

/// Timer dispatch of the orc spells (`SpellMsg::HeroSpell` kinds from `0x80`).
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, aux: u32) {
    match kind {
        TICK_MIRROR => mirror_fire(game, rt, unit, aux),
        TICK_CHAIN => chain_next(game, rt, aux),
        TICK_WW_END => game.borrow_mut().drop_spell_buff(aux),
        TICK_BLADE => blade_tick(game, rt, aux),
        TICK_VOODOO => voodoo_tick(game, aux),
        TICK_QUAKE => quake_tick(game, rt, aux),
        TICK_REINCARNATE => reincarnate(game, unit, aux),
        _ => {}
    }
}

/// What the Feral Spirit effect `0x6F1DF330` does before it creates the wolves (CONFIRMED by Unicorn,
/// `tools/emu/feral_emu.py`, 400 `E` cases): `count = F2I(DataB)`, the duration handed to every wolf's buff is
/// `Dur + ability+0x9C` (soft add; **there is no HeroDur branch**, a hero caster uses the same `Dur`), and the creator
/// flag word is `0x502` when that duration is exactly zero (x87 equality, so NaN counts as non-zero) and `2` otherwise
/// (the creator ORs 2 on top). A negative count loops 2^32 - |n| times in the original (never reached with real data).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeralPlan {
    pub count: i32,
    pub flags: u32,
    pub duration: F32,
}

pub fn feral_plan(dur: F32, plus_9c: F32, data_b: F32) -> FeralPlan {
    let total = arith::add(dur, plus_9c);
    FeralPlan {
        count: conv::f2i(data_b),
        flags: if total.to_f32() == 0.0 { 0x502 } else { 2 },
        duration: total,
    }
}

/// The point the creator wrapper `0x6F1D27A0` -> `0x6F1D26B0` hands to the unit creator: the caster's position plus
/// `M * (Area, 0, 0)` where `M` is the rotation about the vertical axis by the caster's facing (the same matrix and
/// transform as Mirror Image). Every wolf of one cast is requested at this point; the unit creator `0x6F29F990` spreads
/// them by its own placement search (not decoded). The facing passed on is the caster's facing. CONFIRMED (Unicorn, 400
/// `P` cases, soft-float tables from the DLL).
pub fn feral_place(cx: F32, cy: F32, facing: F32, area: F32, t: &softfloat::Tables) -> (F32, F32) {
    let m = mirror_rotation(facing, t);
    let o = mirror_transform(&m, [area, F32::ZERO, F32::ZERO]);
    // The wrapper adds the caster's coordinate FIRST (`edx` = the caster's point, the stack argument = the rotated
    // offset): the operand order decides the sign of a zero sum.
    (arith::add(cx, o[0]), arith::add(cy, o[1]))
}

/// Feral Spirit (class `0x6F927B2C`, effect `0x6F1DF330`): `F2I(DataB)` units of the level's `UnitID`, created for the
/// caster's owner at [`feral_place`] facing like the caster, each given the timed buff `BOsf` (`CBuffSpiritWolf`, a
/// `CBuffTimedLife`: class flag 4 = summoned, duration `Dur + ability+0x9C`, zero = no expiry). No RNG draw. UNSURE:
/// the creator's placement search that separates the wolves (the model builds them at the same point), the abilities
/// of the wolves.
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
    let plan = feral_plan(rec.level.dur, F32::ZERO, rec.level.data[1]);
    let (owner, x, y, facing) = {
        let u = gm.unit(rec.caster);
        (u.owner, u.x, u.y, u.facing)
    };
    let (px, py) = match gm.softfloat_tables().cloned() {
        Some(t) => feral_place(x, y, facing, rec.level.area, &t),
        None => (x, y),
    };
    for _ in 0..plan.count.max(0) {
        let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, unit_type, px, py, rad_to_deg(facing), true)
        else {
            continue;
        };
        {
            let u = gm.unit_mut(k);
            u.summoned = true;
            u.class_flags |= 0x4;
        }
        gm.apply_timed_life(k, crate::abilities::TIMED_LIFE, plan.duration);
    }
}

/// The facing the wards are created with: the Misc `BuildingAngle` through `0x6F020D30` / the ward cast's copy of it:
/// `v = F2I(misc) % 360` (C remainder), rounded toward zero to a multiple of 90 (`v - v % 90`), a negative result gets
/// +360, then `I2R(v) * (pi / 180)` (`0x6FAAE5E8`, soft multiply). CONFIRMED by Unicorn (`tools/emu/ward_emu.py`; the
/// DLL's value of 90 / 180 / 270 gives 1.5708 / 3.1416 / 4.7124 exactly as the creator receives them). The AI helper
/// `ai_site::building_angle` has no rounding (it only ever sees 0/90/180/270).
pub fn building_angle_radians(misc: F32) -> F32 {
    let mut v = conv::f2i(misc).wrapping_rem(360);
    let r = v % 90;
    if v != 0 && r != 0 {
        v -= r;
    }
    if v < 0 {
        v += 360;
    }
    arith::mul(conv::i2r(v), F32::from_bits(0x3C8E_FA35))
}

/// Serpent Ward point cast (class `CAbilityWard`, slot `0x418` = `0x6F162DC0`): `n = F2I(DataA)` wards of the level's
/// `UnitID` for the caster's owner, each created AT THE TARGET POINT (the creator `0x6F29F990` receives the cast's own
/// x and y pointers) facing [`building_angle_radians`] with the same count / duration / flag arithmetic as Feral Spirit
/// (`Dur + ability+0x9C`, no `HeroDur`; flag `0x502` for a zero duration else `2`; `ward_plan` = `feral_plan`), then the
/// ward buff `BOwd` through slot `0x430` (`0x6F18C650`: buff factory `0x6F189900`, init `0x324`, apply `0x31C` with the
/// unit and a pointer to the duration). CONFIRMED by Unicorn (`ward_emu.py`, 400 cases).
pub fn ward_plan(dur: F32, plus_9c: F32, data_a: F32) -> FeralPlan {
    feral_plan(dur, plus_9c, data_a)
}

fn serpent_ward(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Point(px, py) = rec.target else {
        return;
    };
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let level_index =
        usize::try_from(gm.unit(rec.caster).abilities.get(SERPENT_WARD).map_or(0, |a| a.level_index)).unwrap_or(0);
    let Some(&unit_type) = gm.orc.summon_units.get(&SERPENT_WARD).and_then(|v| v.get(level_index)) else {
        return;
    };
    let plan = ward_plan(rec.level.dur, F32::ZERO, rec.level.data[0]);
    let owner = gm.unit(rec.caster).owner;
    let facing = building_angle_radians(gm.production().building_angle);
    for _ in 0..plan.count.max(0) {
        let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, unit_type, px, py, rad_to_deg(facing), true)
        else {
            continue;
        };
        {
            let u = gm.unit_mut(k);
            u.summoned = true;
            u.class_flags |= 0x4;
        }
        gm.apply_timed_life(k, crate::abilities::TIMED_LIFE, plan.duration);
    }
}

// ---- Hex (`AOhx`, `CAbilityHex`, vtable `0x6F8B1704`, a `CAbilityPolymorph`): the creature pick ----

/// Hex's base code.
pub const HEX: u32 = code(b"AOhx");
/// The combat RNG stream of the creature pick: the roll wrapper `0x6F285350` is called with `EDX = 0x18` at `0x6F12DC14`.
pub const HEX_STREAM: usize = 24;

/// The default list of a movement class when the ability row has none: slots `0x42C` / `0x430` / `0x434` / `0x438` of the
/// class (`nshe`, `nshf`, `nsha`, `nshw` = the sheep family). The selector `0x6F12DAA0` maps the class value `unit+0x1FC`
/// of the target: 2 -> slot `0x430`, 16 -> `0x438`, 32 -> `0x434`, anything else -> `0x42C`.
pub fn polymorph_default_list(class: u32) -> &'static str {
    match class {
        2 => "nshf",
        0x10 => "nshw",
        0x20 => "nsha",
        _ => "nshe",
    }
}

/// The column of the level record's data list that the profile lookup `0x6F113DE0` reads for a class: 2 -> `DataC`
/// (index 2), 16 -> `DataE` (index 4), 32 -> `DataD` (index 3), anything else -> `DataB` (index 1) (indices into
/// `SpellLevel::data` where `DataA` is 0). Read from the key table at `0x6FA6F298..0x6FA6F2A4`.
pub fn polymorph_data_column(class: u32) -> usize {
    match class {
        2 => 2,
        0x10 => 4,
        0x20 => 3,
        _ => 1,
    }
}

/// The type code of one list token: the first four bytes as a big-endian word (`0x6F4C3F50`; shorter tokens give a
/// shorter number).
pub fn polymorph_code(token: &str) -> u32 {
    token.bytes().take(4).fold(0u32, |a, b| (a << 8) | u32::from(b))
}

/// The tokens of a list: Storm's `SStrTokenize` with the delimiters blank, comma, hyphen, underscore and tab (`0x6F88F9FC`).
pub fn polymorph_tokens(text: &str) -> Vec<u32> {
    text.split([' ', ',', '-', '_', '\t']).filter(|t| !t.is_empty()).map(polymorph_code).collect()
}

/// `0x6F12DAA0` (CONFIRMED by Unicorn, `tools/emu/hex_emu.py`): the creature a target becomes. `list` is the ability
/// row's text for the class (`None` or empty = the class default); only the types the unit database knows (`exists`)
/// count; with none left the result is `None` and NO draw is made, otherwise ONE draw of combat stream 24, `roll(1 die of
/// n sides, bonus -1)` = an index `0..n-1`.
pub fn hex_pick(
    stream: &mut crate::rng::Rng,
    table: &crate::rng::RngTable,
    class: u32,
    list: Option<&str>,
    exists: impl Fn(u32) -> bool,
) -> Option<u32> {
    let text = match list {
        Some(t) if !t.is_empty() => t,
        _ => polymorph_default_list(class),
    };
    let candidates: Vec<u32> = polymorph_tokens(text).into_iter().filter(|&c| exists(c)).collect();
    if candidates.is_empty() {
        return None;
    }
    let n = u32::try_from(candidates.len()).ok()?;
    let index = crate::damage::roll_total(stream, table, 1, n, -1)?;
    candidates.get(usize::try_from(index).ok()?).copied()
}

/// The shipped lists of Hex (`DataB`..`DataE` of AbilityData, the same on every level), used when the host gives none.
const HEX_DEFAULT_LISTS: [&str; 4] = [
    "npig,nsea,ncrb,nhmc,nrat,nfro,nech,necr,nrac",
    "nalb,nvul,nsno",
    "nsha,npng",
    "nshw,npnw",
];

/// Buff code of Hex.
pub const BUFF_HEX: u32 = code(b"BOhx");

impl Game {
    /// Installs the creature lists of an ability's levels (`DataB`, `DataC`, `DataD`, `DataE` texts per level).
    pub fn set_orc_lists(&mut self, base: u32, lists: Vec<[String; 4]>) {
        self.orc.lists.insert(base, lists);
    }

    fn orc_list(&self, base: u32, level_index: usize, column: usize) -> Option<String> {
        let c = column.checked_sub(1)?;
        match self.orc.lists.get(&base) {
            Some(v) if !v.is_empty() => v.get(level_index.min(v.len() - 1)).map(|l| l[c].clone()),
            _ if base == HEX => Some(HEX_DEFAULT_LISTS[c].to_owned()),
            _ => None,
        }
    }

    /// The creature a hexed unit became (for tests and hosts); `None` when the unit is not hexed.
    pub fn hex_form(&self, unit: ObjKey) -> Option<u32> {
        self.orc.hexed.get(&unit).map(|&(_, form)| form)
    }
}

/// Hex (`CAbilityHex`, unit target slot `0x424` = `0x6F12F560`, shared with Polymorph; CONFIRMED by reading, the creature
/// pick by Unicorn): an ILLUSION target dies at once (slot `0x124` with life 0.0). Otherwise the duration is `HeroDur`
/// for a hero (the hero test `0x6F1D2660`) and `Dur + ability+0x9C` (`0x6F02F880`) for everyone else (`0x6F12F5C3`..
/// `0x6F12F5DD`), the creature is [`hex_pick`] over the list of the target's movement class (2 fly -> `DataC`, 16 float ->
/// `DataE`, 32 amphibious -> `DataD`, else `DataB`; the class is the target's `movetp` word), and slot `0x43C` =
/// `0x6F0BAD50` applies the morph buff `BOhx` with the form and the duration. Not modelled: the stat and ability swap of the
/// morph (the hexed unit cannot attack or cast), only the buff and the recorded form. A pick with no existing type
/// leaves the target untouched (the original passes type 0 on; UNSURE what the buff does then).
fn hex_cast(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let CastTarget::Unit(target) = rec.target else {
        return;
    };
    let mut killed = None;
    {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) || !gm.alive(rec.caster) {
            return;
        }
        if gm.unit(target).illusion {
            if crate::natives::unit::set_life(&mut gm, target, F32::ZERO) {
                killed = Some(target);
            }
        } else {
            let hero = gm.target_hero(target);
            let dur = effect_duration(&rec.level, hero, F32::ZERO);
            let class = gm.unit_types().get(gm.unit(target).type_id).map_or(1, |t| t.movement);
            let level_index =
                usize::try_from(gm.unit(rec.caster).abilities.get(HEX).map_or(0, |a| a.level_index)).unwrap_or(0);
            let list = gm.orc_list(HEX, level_index, polymorph_data_column(class));
            let known: std::collections::BTreeSet<u32> = polymorph_tokens(list.as_deref().unwrap_or(""))
                .into_iter()
                .chain(polymorph_tokens(polymorph_default_list(class)))
                .filter(|&c| gm.unit_types().get(c).is_some())
                .collect();
            let form = match gm.rng_parts() {
                Some((table, streams)) => hex_pick(
                    streams.stream_mut(HEX_STREAM),
                    table,
                    class,
                    list.as_deref(),
                    |c| known.contains(&c),
                ),
                None => None,
            };
            if let Some(form) = form {
                let serial = gm.add_spell_buff(target, BUFF_HEX, BuffEffect::Marker, dur);
                if serial != 0 {
                    gm.orc.hexed.insert(target, (serial, form));
                }
            }
        }
    }
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

// ---- Big Bad Voodoo (`AOvd`, `CAbilityVoodoo`, vtable `0x6F8F8124`) ----

/// Big Bad Voodoo's ability id (its factory base code is the same).
pub const VOODOO: u32 = code(b"AOvd");
/// The buff the units in the area carry (`BOvd`; `BOvc` is the caster's channel buff, not modelled).
pub const BUFF_VOODOO: u32 = code(b"BOvd");
/// The pulse timer kind of the voodoo (`SpellMsg::HeroSpell`).
pub const TICK_VOODOO: u8 = 0x85;
/// The pulse period: the float at `0x6FAAE4D0`, 0.5 s (the timer message `0xD01C1` of the effect `0x6F18C340`).
pub const VOODOO_PULSE_BITS: u32 = 0x3F00_0000;
/// The shipped `targs` of Big Bad Voodoo (`air,ground,friend,vuln,invu`): air 4, ground 2, friend `0x6000`, vulnerable
/// `0x100000`, invulnerable `0x200000`. `friend` carries no `self` bit, so the area predicate never accepts the caster.
pub const VOODOO_TARGS_DEFAULT: u32 = 0x0030_6006;

/// The duration of the buff each pulse gives (`0x6F18C580`: `add(0.5, 0.5)` through the soft add, i.e. 1.0 s; an
/// existing buff is refreshed through its slot `0x350` instead of getting a second one). CONFIRMED by reading.
pub fn voodoo_buff_duration() -> F32 {
    arith::add(F32::from_bits(VOODOO_PULSE_BITS), F32::from_bits(VOODOO_PULSE_BITS))
}

/// A Big Bad Voodoo in progress.
#[derive(Debug, Clone)]
pub(crate) struct VoodooState {
    caster: ObjKey,
    left: F32,
    area: F32,
    word: u32,
}

/// Big Bad Voodoo cast (effect `0x6F18C340`, CONFIRMED structure): clears the caster's earlier voodoo buffs, starts the
/// pulse timer (message `0xD01C1`, 0.5 s) and creates an area watcher of radius `Area` around the caster (message
/// `0xD01C7` per unit that enters; the handler `0x6F18C630` -> `0x6F18C580` gives each accepted unit the buff `BOvd` for
/// [`voodoo_buff_duration`], refreshing a buff it already has). While the buff lasts the unit is invulnerable. MODEL (UNSURE):
/// the watcher is a pulse every 0.5 s over the circle (no per-entry events), the run lasts `Dur` (the channel; the real
/// one also ends when the channel is interrupted, see the shared channel code), the per-unit gate of slot `0x3EC` is not
/// decoded, the caster is not affected (no `self` in `targs`).
fn voodoo(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let level_index =
        usize::try_from(gm.unit(rec.caster).abilities.get(VOODOO).map_or(0, |a| a.level_index)).unwrap_or(0);
    let word = gm.orc_targs(VOODOO, level_index);
    gm.orc.voodoo_serial = gm.orc.voodoo_serial.wrapping_add(1);
    let serial = gm.orc.voodoo_serial;
    gm.orc.voodoos.insert(serial, VoodooState { caster: rec.caster, left: rec.level.dur, area: rec.level.area, word });
    voodoo_pulse(&mut gm, serial);
}

fn voodoo_pulse(gm: &mut Game, serial: u32) {
    let Some(st) = gm.orc.voodoos.get(&serial).cloned() else {
        return;
    };
    let (cx, cy) = {
        let u = gm.unit(st.caster);
        (u.x, u.y)
    };
    let dur = voodoo_buff_duration();
    for k in crate::natives::group::circle_candidates(gm, cx, cy, st.area) {
        {
            let u = gm.unit(k);
            if u.removed || u.hidden {
                continue;
            }
        }
        if !area_predicate(gm, st.caster, k, st.word) {
            continue;
        }
        let buff = gm.add_spell_buff(k, BUFF_VOODOO, BuffEffect::Marker, dur);
        if buff != 0 {
            gm.unit_mut(k).invuln_count += 1;
            gm.orc.voodoo_buffs.insert(buff, k);
        }
    }
    let period = F32::from_bits(VOODOO_PULSE_BITS);
    let left = arith::sub(st.left, period);
    if left.to_f32() > 0.0 {
        if let Some(s) = gm.orc.voodoos.get_mut(&serial) {
            s.left = left;
        }
        gm.timers_start_hero(f32::from_bits(VOODOO_PULSE_BITS), TICK_VOODOO, st.caster, serial);
    } else {
        gm.orc.voodoos.remove(&serial);
    }
}

fn voodoo_tick(game: &RefCell<Game>, serial: u32) {
    let mut gm = game.borrow_mut();
    let Some(st) = gm.orc.voodoos.get(&serial).cloned() else {
        return;
    };
    if !gm.alive(st.caster) {
        gm.orc.voodoos.remove(&serial);
        return;
    }
    voodoo_pulse(&mut gm, serial);
}

// ---- Command Aura (`AOac`): the base damage it multiplies ----

/// The average damage of one weapon slot that the Command Aura scales (`0x6F0C6D00`, reached through `0x6F26D5F0`):
/// `I2R(bonus) + I2R(count) * (I2R(sides) * 0.5 + 0.5)` in the soft float, i.e. `bonus + count * (sides + 1) / 2`. The fields
/// are the attack ability's `+0x88 + 4w` (dice count), `+0x94 + 4w` (sides) and `+0xA0 + 4w` (base bonus); the extra bonus
/// `+0xAC` is NOT included. A unit without an attack ability gives 0.0. CONFIRMED by Unicorn (`tools/emu/cmdaura_emu.py`,
/// 400 + 200 cases). The aura's bonus is then `F2I(add(mul(DataA, this), 0.5))` (`0x6F1ACC40`), or `F2I(DataA)` when flag D is set.
pub fn command_base_damage(count: i32, sides: i32, bonus: i32) -> F32 {
    let half = F32::from_bits(0x3F00_0000);
    let per_die = arith::add(arith::mul(conv::i2r(sides), half), half);
    arith::add(conv::i2r(bonus), arith::mul(conv::i2r(count), per_die))
}

/// Whether a weapon `targs` word is one of the four the Command Aura routine treats as "allowed" (`1`, `0x40`, `0x80`,
/// `0x100`; `0x6F1ACC40`).
fn command_targs_allowed(targs: u32) -> bool {
    matches!(targs, 1 | 0x40 | 0x80 | 0x100)
}

/// The damage bonus the Command and Trueshot aura buffs give one weapon slot (`0x6F1ACC40`, CONFIRMED, **Unicorn 400/400**,
/// `tools/emu/spell_buff_emu.py`, the real routine on a fake buff). `flags` is the buff's flag word (`+0x20`): bit 8
/// (`DataB`), bit 9 (`DataC`) and bit 10 (`DataD`, flat). `kind` is the weapon kind (`+0xDC + 4w`, the `weapTp` enum),
/// `targs` the weapon's target word (`+0x218 + 4w`). `None` = the slot is skipped: a kind-1 weapon whose `targs` is not
/// allowed is skipped unless bit 8 is set, and a kind 2..=8 weapon whose `targs` is not allowed is skipped unless bit 9
/// is set. Otherwise `F2I(fraction)` when bit 10 is set, else `F2I(add(mul(fraction, X), 0.5))` with `X` the weapon's
/// average damage ([`command_base_damage`]); a zero result takes the sign of `fraction` (+1, -1, NaN -1, 0 for exactly
/// zero); a negative result is clamped from below by `-(bonus + count)`. The amount is added to the weapon's damage
/// counter and kept in the buff for the undo.
pub fn command_aura_weapon_amount(fraction: F32, flags: u32, count: i32, sides: i32, bonus: i32, kind: u32, targs: u32) -> Option<i32> {
    let (bit8, bit9, flat) = (flags & 0x100 != 0, flags & 0x200 != 0, flags & 0x400 != 0);
    if !command_targs_allowed(targs) && ((!bit8 && kind == 1) || (!bit9 && (2..=8).contains(&kind))) {
        return None;
    }
    let mut amount = if flat {
        conv::f2i(fraction)
    } else {
        let half = F32::from_bits(0x3F00_0000);
        conv::f2i(arith::add(arith::mul(fraction, command_base_damage(count, sides, bonus)), half))
    };
    if amount == 0 {
        let v = fraction.to_f32();
        amount = if v == 0.0 {
            0
        } else if v > 0.0 {
            1
        } else {
            -1
        };
    }
    if amount < 0 {
        let floor = bonus.wrapping_add(count).wrapping_neg();
        if amount < floor {
            amount = floor;
        }
    }
    Some(amount)
}

// ---- Earthquake (`AOeq`, `CAbilityEarthquake`, vtable `0x6F8A8154`; effect `CEffectEarthquake`, vtable `0x6F88F86C`) ----

/// Earthquake's ability id.
pub const EARTHQUAKE: u32 = code(b"AOeq");
/// The slow buff on the units in the quake (`BOeq`, id `0x424F6571` at `0x6F0B736E`; the structure marker buff is `BOea`).
pub const BUFF_QUAKE: u32 = code(b"BOeq");
/// The timer kind of the quake (`SpellMsg::HeroSpell`): the start after `DataA`, then the 1.0 s ticks.
pub const TICK_QUAKE: u8 = 0x86;
/// The shipped `targs` of Earthquake (`ground,structure,debris,tree`): ground 2, structure 8, debris `0x100`, tree `0x40`.
/// No relation word: the predicate accepts any relation, so the quake also hurts the caster's own buildings.
pub const QUAKE_TARGS_DEFAULT: u32 = 0x0000_014A;
/// The damage class of the per-structure damage record: `0x4000000` (damage type index 26).
pub const QUAKE_DAMAGE_TYPE: u32 = 26;

/// What the effect initialiser `0x6F0C4230` keeps (CONFIRMED by Unicorn, `tools/emu/quake_emu.py`, 400 runs of the real
/// timer handler `0x6F0C4420` + initialiser on a fake ability): the centre cells `+0x24` / `+0x2C` are the ability's stored
/// point, cell `+0x38` is `DataB` (the damage per tick on structures), cell `+0x40` is `Area` (the radius), cell `+0x64` is
/// `DataC` (the factor of the slow), and the tick counter `+0x34` is `F2I(Dur + ability+0x9C)` (soft add; no `HeroDur`).
/// Cell `+0x48` (a soft-float value that depends on the centre) is presentation. `DataD` is not read by the effect; `DataA`
/// is the delay of the timer message `0xD01C1` that starts the effect (0x6F08AE40 starts it with `DataA` seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuakePlan {
    pub ticks: i32,
    pub damage: F32,
    pub radius: F32,
    pub slow: F32,
}

pub fn quake_plan(dur: F32, plus_9c: F32, data_b: F32, data_c: F32, area: F32) -> QuakePlan {
    QuakePlan { ticks: conv::f2i(arith::add(dur, plus_9c)), damage: data_b, radius: area, slow: data_c }
}

/// A running earthquake.
#[derive(Debug, Clone)]
pub(crate) struct QuakeState {
    caster: ObjKey,
    x: F32,
    y: F32,
    plan: QuakePlan,
    word: u32,
    started: bool,
    left: i32,
}

/// Earthquake cast (point order 852121): the cast point is stored (cells `+0x150`/`+0x158` of the ability), the effect timer
/// runs `DataA` seconds (`0x6F08AE40`: `DataA` read by `0x6F02E9E0`, timer message `0xD01C1` at `+0x13C`), then
/// [`quake_start`] creates the effect. CONFIRMED by reading (cast) and Unicorn (effect numbers).
fn earthquake(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Point(px, py) = rec.target else {
        return;
    };
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let level_index =
        usize::try_from(gm.unit(rec.caster).abilities.get(EARTHQUAKE).map_or(0, |a| a.level_index)).unwrap_or(0);
    let word = gm.orc_targs(EARTHQUAKE, level_index);
    let plan = quake_plan(rec.level.dur, F32::ZERO, rec.level.data[1], rec.level.data[2], rec.level.area);
    gm.orc.quake_serial = gm.orc.quake_serial.wrapping_add(1);
    let serial = gm.orc.quake_serial;
    gm.orc.quakes.insert(
        serial,
        QuakeState { caster: rec.caster, x: px, y: py, plan, word, started: false, left: plan.ticks },
    );
    gm.timers_start_hero(rec.level.data[0].to_f32(), TICK_QUAKE, rec.caster, serial);
}

fn quake_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, serial: u32) {
    let mut killed = Vec::new();
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(st) = gm.orc.quakes.get_mut(&serial) else {
            return;
        };
        st.started = true;
        caster = st.caster;
        let (x, y, plan, word) = (st.x, st.y, st.plan, st.word);
        // The tick (`0x6F0C3780`): the counter drops first, the group is collected and visited, then the next tick is armed
        // (1.0 s, `0x6FAAE4F4`) while the counter is positive, else the effect is destroyed.
        st.left -= 1;
        let left = st.left;
        if left > 0 {
            gm.timers_start_hero(1.0, TICK_QUAKE, caster, serial);
        } else {
            gm.orc.quakes.remove(&serial);
        }
        for k in crate::natives::group::circle_candidates(&gm, x, y, plan.radius) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden {
                    continue;
                }
            }
            if !area_predicate(&gm, caster, k, word) {
                continue;
            }
            // `0x6F0B72C0`: the zero path (invulnerable slot or spell immunity) skips both effects.
            let zero = gm.unit_is_invulnerable(k) || is_unit_type(&gm, gm.unit(k), ty::MAGIC_IMMUNE);
            if gm.target_flag_word(k) & 8 != 0 {
                // A structure: the damage record (class `0x4000000`, flags `0x40`, `0x50` on the zero path).
                let amount = if zero { F32::ZERO } else { plan.damage };
                if let Some(t) = gm.unit_damage_target(caster, k, amount, false, false, 0, QUAKE_DAMAGE_TYPE) {
                    killed.push(t);
                }
            } else if !zero {
                // Any other unit: the slow buff `BOeq` with `-DataC` movement for 2.0 s (`[0x6FAAE504]`), refreshed per tick.
                let neg = F32::from_bits(plan.slow.to_bits() ^ 0x8000_0000);
                gm.add_spell_buff(
                    k,
                    BUFF_QUAKE,
                    BuffEffect::Slow { move_delta: neg, attack_delta: F32::ZERO },
                    F32::from_bits(0x4000_0000),
                );
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

/// What the Far Sight cast (`0x6F06FA50`) hands to the reveal effect creator `0x6F06F8D0` (CONFIRMED by Unicorn,
/// `tools/emu/farsight_emu.py`, 400 cases): the effect id (the level's `EfctID`, `Xbdt`), pointers to the cast point x and y and
/// to the terrain height at it, the caster's owner index, the DURATION `Dur + ability+0x9C` (class slot `0x430`; no `HeroDur`),
/// the radius `Area` (slot `0x42C`) and `F2I(DataA)` (slot `0x434`, 3 in the data). The effect itself lives for `duration +
/// 0.1` seconds (`0x6F069210` starts its expiry message `0xD01C4` with `add(duration, [0x6FAAE4A0])`, CONFIRMED by reading).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FarSightPlan {
    pub duration: F32,
    pub lifetime: F32,
    pub radius: F32,
    pub count: i32,
}

pub fn far_sight_plan(dur: F32, plus_9c: F32, data_a: F32, area: F32) -> FarSightPlan {
    let duration = arith::add(dur, plus_9c);
    FarSightPlan {
        duration,
        lifetime: arith::add(duration, F32::from_bits(0x3DCC_CCCD)),
        radius: area,
        count: conv::f2i(data_a),
    }
}

// ---- Reincarnation (`AOre`, `CAbilityReincarnation`, vtable `0x6F9199C4`; effect `CEffectReincarnation`, vtable `0x6F916934`) ----

/// Reincarnation's ability id.
pub const REINCARNATION: u32 = code(b"AOre");
/// The timer kind of the revival (`SpellMsg::HeroSpell`).
pub const TICK_REINCARNATE: u8 = 0x87;

/// The conditions of the death handler (slot `0x284` = `0x6F1E5E30`, CONFIRMED by reading): the ability's pause counter
/// (`+0x3C`) is not positive, the ability is enabled (slot `0x268`), the dying unit is not an illusion (`unit+0x5C` bit
/// `0x40000000`) and has bit 0 of `unit+0x20` clear, the unit passes `0x6F1DB6E0`, the ability is ready (slot `0x370`) and not on
/// its cooldown flag (`+0x20` bit `0x200`), and the death point passes the pathing probe `0x6F468F90(point, 2)` (a non-zero
/// answer blocks the revival). A positive `Cool` (`0x6F02E9F0`-style getter through slot `0x2EC`) then starts the cooldown
/// (slot `0x3A4`), and the revival effect starts (slot `0x3E8` = `0x6F1F1C10`).
pub fn reincarnation_allowed(illusion: bool, cooling: bool, blocked_by_pathing: bool) -> bool {
    !illusion && !cooling && !blocked_by_pathing
}

impl Game {
    /// Called by the unit death dispatch for every dying unit: starts the Reincarnation of a unit that carries a ready
    /// ability. UNSURE (see the spec): the delay is `DataA` seconds (the effect `0x6F1F1C10` stores `DataA` into the effect's
    /// agile cell `+0xB8`; the effect's expiry message `0xD01C4` runs the revival `0x6F1EBE30`), the pathing probe is not
    /// modelled, the revival gives full life (`resurrect_unit`) at the death point and leaves the mana alone.
    pub(crate) fn orc_on_unit_died(&mut self, unit: ObjKey) {
        if self.unit(unit).abilities.get(REINCARNATION).is_none_or(|a| a.is_removed()) {
            return;
        }
        let illusion = self.unit(unit).illusion;
        let cooling = self.is_cooling(unit, REINCARNATION);
        if !reincarnation_allowed(illusion, cooling, false) {
            return;
        }
        let Some(level) = self.ability_level_record(unit, REINCARNATION) else {
            return;
        };
        if level.cool.to_f32() > 0.0 {
            self.start_cooldown(unit, REINCARNATION, level.cool);
        }
        self.start_reincarnation(unit, level.data[0].to_f32());
    }

    /// Starts the revival of the dead `unit` after `delay` seconds (the effect `CEffectReincarnation`: the timer of the
    /// revival, shared with the item `AIrc`, `item_abilities::on_unit_died`).
    pub(crate) fn start_reincarnation(&mut self, unit: ObjKey, delay: f32) {
        self.orc.reincarnate_serial = self.orc.reincarnate_serial.wrapping_add(1);
        let serial = self.orc.reincarnate_serial;
        self.orc.reincarnating.insert(unit, serial);
        self.timers_start_hero(delay, TICK_REINCARNATE, unit, serial);
    }

    /// Whether a Reincarnation of `unit` is pending (for tests and hosts).
    pub fn reincarnation_pending(&self, unit: ObjKey) -> bool {
        self.orc.reincarnating.contains_key(&unit)
    }
}

/// The expiry of the revival effect (`0x6F1EBF80` on message `0xD01C4` -> slot `0x80` = `0x6F1EBE30`): the unit must still
/// resolve and be dead (`unit+0x5C` bit `0x100`); it is moved to its own point (slot `0x180`), revived (`0x6F2A64A0` with the
/// flags `0x500`) and its food is corrected (the dead unit's food use was released).
fn reincarnate(game: &RefCell<Game>, unit: ObjKey, serial: u32) {
    let mut gm = game.borrow_mut();
    if gm.orc.reincarnating.get(&unit) != Some(&serial) {
        return;
    }
    gm.orc.reincarnating.remove(&unit);
    let u = gm.unit(unit);
    if u.removed || !u.dead {
        return;
    }
    gm.resurrect_unit(unit);
}

/// Far Sight (class `0x6F88B25C`): reveals the circle of `Area` around the point to the caster's owner for [`far_sight_plan`]'s
/// lifetime. MODEL (UNSURE): a visible-state fog modifier started now and stopped when the marker buff on the caster ends (the
/// original's reveal is a vision effect object with an area watcher, message `0xD01B4`, not decoded).
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
    let dur = far_sight_plan(rec.level.dur, F32::ZERO, rec.level.data[0], rec.level.area).lifetime;
    let serial = gm.add_spell_buff(rec.caster, BUFF_FAR_SIGHT, BuffEffect::Marker, dur);
    if serial == 0 {
        gm.fog_stop(key);
    } else {
        gm.orc.reveals.insert(serial, key);
    }
}

/// Called by `Game::drop_spell_buff` when a spell buff ends: stops the Far Sight reveal tied to it.
pub(crate) fn on_buff_dropped(game: &mut Game, serial: u32) {
    game.orc.hexed.retain(|_, &mut (s, _)| s != serial);
    if let Some(k) = game.orc.voodoo_buffs.remove(&serial) {
        let u = game.unit_mut(k);
        u.invuln_count = (u.invuln_count - 1).max(0);
    }
    if let Some(key) = game.orc.reveals.remove(&serial) {
        game.fog_stop(key);
    }
}

impl Game {
    /// Registers `unit` as an illusion with damage factors (dealt, taken), the way Mirror Image does; used by the item
    /// ability `AIil` (S45).
    pub(crate) fn register_illusion(&mut self, unit: ObjKey, dealt: F32, taken: F32) {
        self.orc.illusions.insert(unit, (dealt, taken));
    }
}
