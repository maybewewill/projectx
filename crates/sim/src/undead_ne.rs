//! Undead and Night Elf hero spells: Death Coil (`AUdc`), Frost Nova (`AUfn`), Entangling Roots (`AEer`) and Force
//! of Nature (`AEfn`). Spec: `docs/spec/abilities-undead-ne-heroes.md`. They run inside the cast pipeline of
//! `cast.rs` (check, events, payment, cooldown); this module is the class effect (slot `0x424` / `0x418`).
//!
//! Classes (RTTI, vtables): `CAbilityDeathCoil` `0x6F8B1F84`, `CAbilityFrostNova` `0x6F8F6384`,
//! `CAbilityEntanglingRoots` `0x6F8A6824`, `CAbilityForceOfNature` `0x6F8AF504`. Labels: CONFIRMED = read in the
//! disassembly (numbers say where Unicorn ran), LIKELY = strong inference, UNSURE = stand-in.
//!
//! ## Level record fields used (CONFIRMED by the getters `0x6F02E9E0` = DataA `+0x20`, `0x6F02EA50` = DataB `+0x24`,
//! `0x6F02E900` = Area `+0x18`, `0x6F02E760` = Dur, `0x6F02E7D0` = HeroDur)
//! * Death Coil `0x6F0AD110`: `v = DataA`. The relation selector `0x6F08EAC0` picks heal (+1), nothing (0) or damage
//!   (-1) from the table `0x6F88E1F4` (`[-1,1,0,1,-1,0,-1,0]`, index `2*(ally | 2*enemy) + undead`, where `enemy` =
//!   `0x6F285B00` (the caster's owner is not passively allied to the target's owner), `ally` = `0x6F285B80` (second
//!   relation mask, modelled as "same owner" like Holy Light, UNSURE) and `undead` = class bit 2 of the target or
//!   race 5). Damage: `v / 2.0` with damage class `0x40000`. Heal: `v`, negated into the record (the ethereal
//!   `EtherealHealBonus` factor applies only when the ability's slot `0x42C` says so, not modelled). Unicorn
//!   (`tools/emu/undead_ne_emu.py`): the selector with both tables, 800 cases, 0 mismatches.
//! * Frost Nova `0x6F176CD0` / per-unit `0x6F1607B0`: the primary target takes DataB (`0x6F1606C0`), every unit in
//!   Area around the target takes DataA through the damage slot with class `0x200` (`0x6F18F7E0`), and each hit
//!   unit gets the slow buff of slot `0x430` (`0x6F18A830`: HeroDur for a hero, else Dur). A target that is
//!   invulnerable (slot `0x11C`) takes 0 and gets no buff. The area unit set excludes allies of the caster (LIKELY
//!   from the `0x8000` enemy-only filter flag). The slow strength is NOT decoded (the buff class carries it):
//!   DataC / DataD stand in as move / attack deltas (UNSURE).
//! * Entangling Roots `0x6F0B8290`: duration HeroDur / Dur (+ `0x6F6EF000` add of the ability's `+0x9C` term), buff
//!   created through `0x6F0B8220`. Model: the root = a full move slow (move speed 0, attacks stay possible) plus
//!   damage of DataA per second in one-second ticks while the buff lives (UNSURE: tick period and class).
//! * Force of Nature `0x6F0B9CE0` (slot `0x418`): collects the trees (destructables) within Area of the target
//!   point (`0x6F2AE9E0` with filter `0x6F09DF40`), `N = F2I(DataA)` caps the count, the units of the level's UnitID
//!   are created at the trees with a timed life of Dur (`0x6F6EF000` add of `+0x9C`), and the trees die. Tree order
//!   (destructable creation order) and the tree test (second character of the type id is `T`) are UNSURE.

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, Tables, F32};

use crate::abilities::code;
use crate::cast::{BuffEffect, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey, Object};
use crate::natives::group::circle_candidates;
use crate::natives::{dispatch_unit_death_by, set_life, ScriptRuntime};

pub const DEATH_COIL: u32 = code(b"AUdc");
pub const FROST_NOVA: u32 = code(b"AUfn");
pub const ENTANGLING_ROOTS: u32 = code(b"AEer");
pub const FORCE_OF_NATURE: u32 = code(b"AEfn");
pub const DEATH_PACT: u32 = code(b"AUdp");
pub const CARRION_SWARM: u32 = code(b"AUcs");
pub const SLEEP: u32 = code(b"AUsl");
pub const SHADOW_STRIKE: u32 = code(b"AEsh");
pub const BLINK: u32 = code(b"AEbl");
pub const IMMOLATION: u32 = code(b"AEim");
/// The Demon / Doomguard variant of Immolation (`AUim`) is the same class `CAbilityImmolation`.
pub const IMMOLATION_UNDEAD: u32 = code(b"AUim");
pub const VAMPIRIC_AURA: u32 = code(b"AUav");

/// Buff ids. CONFIRMED (S28): the id getters `0x6F1998D0` = `'Bfro'` (Frost Nova) and `0x6F07F2B0` = `'BEer'` (Entangling
/// Roots); the level's BuffID list overrides them in the original, the model has no such list.
pub const BUFF_FROST_NOVA: u32 = code(b"Bfro");
pub const BUFF_ROOTS: u32 = code(b"BEer");
/// CONFIRMED by the buff classes' code getters (slot 7): `CBuffSleep` `'BUsl'`, `CBuffShadowStrike` `'BEsh'`,
/// `CBuffImmolation` `'BEim'`, `CBuffAuraVampiric` `'BUav'`; `'BUst'` (`CBuffSleepStun`) and `'BUsp'`
/// (`CBuffSleepPause`) are the buffs that keep a sleeper asleep through damage.
pub const BUFF_SLEEP: u32 = code(b"BUsl");
pub const BUFF_SLEEP_STATIONARY: u32 = code(b"BUst");
pub const BUFF_SLEEP_PAUSE: u32 = code(b"BUsp");
pub const BUFF_SHADOW_STRIKE: u32 = code(b"BEsh");
pub const BUFF_IMMOLATION: u32 = code(b"BEim");
pub const BUFF_VAMPIRIC: u32 = code(b"BUav");
/// The treants' timed life buff (`BEfn` is one of the 7 special timed life ids, abilities.md).
pub const BUFF_TREANT_LIFE: u32 = code(b"BEfn");

/// Damage class bits (CONFIRMED values: Death Coil's record `0x40000`, Frost Nova's slot `0x42C` returns `0x200`);
/// `unit_damage_target` takes the bit index.
const DEATH_COIL_DAMAGE_TYPE: u32 = 18;
const FROST_NOVA_DAMAGE_TYPE: u32 = 9;
/// CONFIRMED (S33): the periodic pulse of `CBuffEntanglingRoots` hands the class word `0x100000` (slot `0x374` =
/// `0x6F0AEED0`) to the damage record, i.e. damage type 20.
const ROOTS_DAMAGE_TYPE: u32 = 20;
/// CONFIRMED (S33): the first pulse comes 0.01 s after the buff is applied (slot `0x358` = `0x6F0ADBD0` returns the
/// start-up value 0.01 of `0x6FAAE480`), then every 1.0 s (slot `0x35C` = `0x6F0ADBE0` returns `0x6FAAE4F4`).
const ROOT_FIRST_TICK_SECONDS: f32 = 0.01;

/// The unit class bit of UNDEAD and the race value of undead (`0x6F08EAC0`: `+0x248 & 2` or `0x6F29C1B0 == 5`).
const CLASS_UNDEAD: u32 = 0x2;

/// The Death Coil relation table at `0x6F88E1F4` (read from the DLL; the values are plain small integers).
pub const DEATH_COIL_TABLE: [i32; 8] = [-1, 1, 0, 1, -1, 0, -1, 0];
/// The Holy Light table at `0x6F88E1D4`.
pub const HOLY_LIGHT_TABLE: [i32; 8] = [1, -1, 1, 0, 0, -1, 0, -1];

/// The selector `0x6F08EAC0`: `table[2 * (ally | 2 * enemy) + undead]` (+1 heal, 0 nothing, -1 damage).
pub fn relation(table: &[i32; 8], enemy: bool, ally: bool, undead: bool) -> i32 {
    table[2 * (usize::from(ally) | (2 * usize::from(enemy))) + usize::from(undead)]
}

/// One-second period of the roots' damage (UNSURE).
const ROOT_TICK_SECONDS: f32 = 1.0;
/// Shadow Strike damage tick and Immolation pulse periods (UNSURE: public data).
/// CONFIRMED (S33, Unicorn `tools/emu/undead_ne3_emu.py` CHECK=launch / buffinit): the pulse period of
/// `CBuffShadowStrike` is the ability's Cast field (stored as a property at `+0x128`, read back by slots `0x358` and
/// `0x35C` = `0x6F1F61B0` / `0x6F1F61C0`), also for the first pulse. The pulse class is `0x2000000` (slot `0x374`).
const STRIKE_DOT_DAMAGE_TYPE: u32 = 25;
/// The CAgentTimer start clamp (`0x6F477F80`): a timeout below 1e-4 becomes 1e-4.
const MIN_PERIOD_SECONDS: f32 = 1.0e-4;
const IMMOLATION_PULSE_SECONDS: f32 = 1.0;
/// UNSURE: Shadow Strike's damage class (the spell class `0x200` like Frost Nova).
const SHADOW_STRIKE_DAMAGE_TYPE: u32 = 9;

/// Spell state kept by the module (owned by `SpellSystem`).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Unit type created by Force of Nature, per base ability (the level's UnitID, which `SpellLevel` lacks).
    summon_unit: BTreeMap<u32, u32>,
    /// Running root damage: buff serial -> (caster, damage per tick).
    roots: BTreeMap<u32, (ObjKey, F32)>,
    /// Treants created, `(caster, unit)` in creation order (for tests and the host).
    pub treants: Vec<(ObjKey, ObjKey)>,
    /// Running Immolations: `(unit, base)` -> the buff serial and the level data of the toggle.
    immolations: BTreeMap<(ObjKey, u32), Immolation>,
    /// Shadow Strike missiles in flight: id -> record.
    strikes: BTreeMap<u32, Strike>,
    /// Running Shadow Strike damage over time: buff serial -> (caster, damage per tick, period in seconds).
    strike_dots: BTreeMap<u32, (ObjKey, F32, f32)>,
    next_strike: u32,
    /// Misc `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease` (S33, CONFIRMED source of the Frost Nova slow);
    /// `0.0` when the key is absent, as the original's reader defaults.
    frost_misc: (F32, F32),
    /// Units hit by Carrion Swarm, `(caster, victim, damage)` in hit order (for tests and the host).
    pub swarm_hits: Vec<(ObjKey, ObjKey, F32)>,
}

#[derive(Debug, Clone, Copy)]
struct Immolation {
    serial: u32,
    damage: F32,
    drain: F32,
    area: F32,
}

#[derive(Debug, Clone, Copy)]
struct Strike {
    caster: ObjKey,
    target: ObjKey,
    level: SpellLevel,
}

impl State {
    pub fn set_summon_unit(&mut self, base: u32, unit_type: u32) {
        self.summon_unit.insert(base, unit_type);
    }
}

/// Timer kinds of [`crate::cast::SpellMsg::HeroSpell`].
pub const TICK_ROOTS: u8 = 1;
/// The Immolation pulse (`aux` = the toggle's buff serial).
pub const TICK_IMMOLATION: u8 = 2;
/// A Shadow Strike missile arrives (`aux` = the strike id).
pub const TICK_STRIKE_HIT: u8 = 3;
/// A Shadow Strike damage tick (`aux` = the buff serial).
pub const TICK_STRIKE_DOT: u8 = 4;

/// Whether the level's `Cast` field of `base` is not a cast-time delay: Shadow Strike's Cast (3 s in the stock data)
/// is the pulse period of its buff (S33, CONFIRMED by the property flow of the launcher and the buff init), so the
/// cast pipeline must not wait on it.
pub fn cast_field_is_period(base: u32) -> bool {
    base == SHADOW_STRIKE
}

/// Whether `base` is cast without a target (an immediate order). Only Immolation (the toggle).
pub fn is_immediate(base: u32) -> bool {
    base == IMMOLATION || base == IMMOLATION_UNDEAD
}

/// Runs the class effect of `base` if this module owns it. Returns whether it did.
pub(crate) fn run_effect(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    caster: ObjKey,
    base: u32,
    level: &SpellLevel,
    target: CastTarget,
) -> bool {
    match (base, target) {
        (DEATH_COIL, CastTarget::Unit(t)) => death_coil(game, rt, caster, level, t),
        (FROST_NOVA, CastTarget::Unit(t)) => frost_nova(game, rt, caster, level, t),
        (ENTANGLING_ROOTS, CastTarget::Unit(t)) => entangling_roots(game, caster, level, t),
        (FORCE_OF_NATURE, CastTarget::Point(x, y)) => force_of_nature(game, rt, caster, level, x, y),
        (DEATH_PACT, CastTarget::Unit(t)) => death_pact(game, rt, caster, level, t),
        (SLEEP, CastTarget::Unit(t)) => sleep(game, caster, level, t),
        (SHADOW_STRIKE, CastTarget::Unit(t)) => shadow_strike(game, rt, caster, level, t),
        (BLINK, CastTarget::Point(x, y)) => blink(game, caster, level, x, y),
        (CARRION_SWARM, CastTarget::Point(x, y)) => carrion_swarm(game, rt, caster, level, x, y),
        (IMMOLATION | IMMOLATION_UNDEAD, CastTarget::None) => immolation(game, caster, base, level),
        (
            DEATH_COIL | FROST_NOVA | ENTANGLING_ROOTS | FORCE_OF_NATURE | DEATH_PACT | SLEEP | SHADOW_STRIKE | BLINK
            | CARRION_SWARM | IMMOLATION | IMMOLATION_UNDEAD,
            _,
        ) => {}
        _ => return false,
    }
    true
}

fn death_coil(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, caster: ObjKey, level: &SpellLevel, target: ObjKey) {
    let amount = level.data[0];
    let mut killed = None;
    {
        let mut gm = game.borrow_mut();
        if !gm.object_is_live_unit(target) {
            return;
        }
        let (t_owner, undead) = {
            let u = gm.unit(target);
            (u.owner, u.class_flags & CLASS_UNDEAD != 0)
        };
        let c_owner = gm.unit(caster).owner;
        let enemy = gm.is_enemy(c_owner, t_owner);
        let ally = c_owner == t_owner;
        match relation(&DEATH_COIL_TABLE, enemy, ally, undead) {
            1 => {
                let u = gm.unit(target);
                let sum = arith::add(amount, u.life);
                let next = if arith::lt(u.max_life, sum) { u.max_life } else { sum };
                set_life(&mut gm, target, next);
            }
            -1 => {
                // DIV(DataA, 2.0): an exact halving.
                let half = arith::mul(amount, F32::from_bits(0x3F00_0000));
                killed = gm.unit_damage_target(caster, target, half, false, false, 0, DEATH_COIL_DAMAGE_TYPE);
            }
            _ => {}
        }
    }
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

fn frost_nova(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, caster: ObjKey, level: &SpellLevel, target: ObjKey) {
    let mut deaths = Vec::new();
    {
        let mut gm = game.borrow_mut();
        if !gm.object_is_live_unit(target) {
            return;
        }
        let c_owner = gm.unit(caster).owner;
        let (tx, ty) = (gm.unit(target).x, gm.unit(target).y);
        // The area part runs FIRST (`0x6F1606C0` calls slot 0x434 before its own damage). Every enemy within Area
        // of the target (the target included: nothing in the per-unit callback `0x6F1607B0` excludes it, LIKELY)
        // takes DataA and, when it is not invulnerable and survives, the slow buff of slot 0x430.
        let victims: Vec<ObjKey> = circle_candidates(&gm, tx, ty, level.area)
            .into_iter()
            .filter(|&k| gm.object_is_live_unit(k) && gm.is_enemy(c_owner, gm.unit(k).owner))
            .collect();
        for k in victims {
            if gm.unit_is_invulnerable(k) {
                continue;
            }
            if let Some(d) = gm.unit_damage_target(caster, k, level.data[0], false, false, 0, FROST_NOVA_DAMAGE_TYPE) {
                deaths.push(d);
            } else {
                apply_frost_slow(&mut gm, k, level);
            }
        }
        // Primary target: DataB through the damage slot with class `0x200`, no buff of its own (`0x6F1606C0`).
        if gm.object_is_live_unit(target) && !gm.unit_is_invulnerable(target) {
            if let Some(d) = gm.unit_damage_target(caster, target, level.data[1], false, false, 0, FROST_NOVA_DAMAGE_TYPE) {
                deaths.push(d);
            }
        }
    }
    for t in deaths {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

/// The slow buff of slot `0x430` (`0x6F18A830`): duration HeroDur for a hero else Dur (+ the `+0x9C` term); the
/// strengths are NOT ability data but the Misc keys `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease`, handed to
/// `CBuffFrost` slot `0x360` (`0x6F199F40`) and negated (Unicorn, `tools/emu/undead_ne3_emu.py` CHECK=frost, 400
/// cases, 0 mismatches). Stock Misc values are 0.5 and 0.25.
fn apply_frost_slow(gm: &mut Game, unit: ObjKey, level: &SpellLevel) {
    if !gm.object_is_live_unit(unit) {
        return;
    }
    let hero = gm.unit(unit).hero.is_some() || is_hero_id(gm.unit(unit).type_id);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
    let (mv, at) = gm.spells_hero_state_mut().frost_misc;
    let effect = BuffEffect::Slow { move_delta: frost_strength(mv), attack_delta: frost_strength(at) };
    gm.add_spell_buff(unit, BUFF_FROST_NOVA, effect, dur);
}

/// The stored strength of the Frost buff: the Misc value with the sign bit flipped (`xor 0x80000000`).
pub fn frost_strength(misc: F32) -> F32 {
    F32::from_bits(misc.to_bits() ^ 0x8000_0000)
}

fn is_hero_id(type_id: u32) -> bool {
    crate::cast::is_hero_target(type_id, false)
}

fn entangling_roots(game: &RefCell<Game>, caster: ObjKey, level: &SpellLevel, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(target) {
        return;
    }
    let hero = gm.unit(target).hero.is_some() || is_hero_id(gm.unit(target).type_id);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
    // A full slow: move speed times (1 - 1) = 0; the unit can still attack.
    let effect = BuffEffect::Slow {
        move_delta: arith::neg(F32::ONE),
        attack_delta: F32::ZERO,
    };
    let serial = gm.add_spell_buff(target, BUFF_ROOTS, effect, dur);
    if serial == 0 {
        return;
    }
    gm.spells_hero_state_mut().roots.insert(serial, (caster, level.data[0]));
    gm.timers_start_hero(ROOT_FIRST_TICK_SECONDS, TICK_ROOTS, target, serial);
}

fn force_of_nature(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    caster: ObjKey,
    level: &SpellLevel,
    x: F32,
    y: F32,
) {
    let _ = rt;
    let mut gm = game.borrow_mut();
    let Some(unit_type) = gm.spells_hero_state_mut().summon_unit.get(&FORCE_OF_NATURE).copied() else {
        return;
    };
    let count = softfloat::conv::f2i(level.data[0]);
    let owner = gm.unit(caster).owner;
    let trees: Vec<(ObjKey, F32, F32)> = (0..gm.object_count())
        .map(|i| ObjKey(i as u32))
        .filter_map(|k| match gm.object(k) {
            Object::Destructable(d)
                if !d.dead && !d.removed && is_tree(d.type_id) && within(d.x, d.y, x, y, level.area) =>
            {
                Some((k, d.x, d.y))
            }
            _ => None,
        })
        .collect();
    let dur = arith::add(level.dur, F32::ZERO);
    for (tree, tx, ty) in trees.into_iter().take(usize::try_from(count).unwrap_or(0)) {
        {
            let d = gm.destructable_mut(tree);
            d.life = F32::ZERO;
            d.dead = true;
            d.blocks_pathing = false;
        }
        if let Some((u, _)) = crate::natives::unit::build(&mut gm, owner, unit_type, tx, ty, F32::ZERO, true) {
            gm.apply_timed_life(u, BUFF_TREANT_LIFE, dur);
            gm.spells_hero_state_mut().treants.push((caster, u));
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// S28 numeric cores (Unicorn: tools/emu/undead_ne2_emu.py, vectors in tests/data/undead_ne/)

/// Death Pact's amounts (`0x6F09FD50`): the target's current life `life` and the level's DataA..DataE give
/// `(mana, life)` for the caster. DataC / DataD equal to 0.0 (x87 compare, NaN is not equal) make DataA / DataB a
/// *factor* of the life (soft multiply `life * Data`); otherwise they are the amounts themselves.
pub fn death_pact_amounts(life: F32, data: &[F32; 5]) -> (F32, F32) {
    let zero = |x: F32| x.to_f32() == 0.0;
    let mana = if zero(data[2]) { arith::mul(life, data[0]) } else { data[0] };
    let heal = if zero(data[3]) { arith::mul(life, data[1]) } else { data[1] };
    (mana, heal)
}

/// Blink's point validation (`0x6F1AEB10`): when the squared distance from the caster to `target` exceeds `range^2`
/// (DataA squared by the soft multiply), the point becomes `caster + (target - caster) * (0.8 * range / |target -
/// caster|)` with the soft square root / reciprocal (`0x6F07B870`, the constant 0.8 is `0x6FAAE4EC`); otherwise the
/// target is unchanged. The placement query that follows in the original (pathing) is not modelled.
pub fn blink_project(caster: (F32, F32), target: (F32, F32), range: F32, tables: &Tables) -> (F32, F32) {
    let dx = arith::sub(target.0, caster.0);
    let dy = arith::sub(target.1, caster.1);
    let r2 = arith::mul(range, range);
    let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
    if !arith::gt(d2, r2) {
        return target;
    }
    let scale = arith::mul(range, F32::from_bits(0x3F4C_CCCD));
    let inv = arith::recip(softfloat::math::sqrt(d2, tables), tables);
    let k = arith::mul(scale, inv);
    (
        arith::add(caster.0, arith::mul(dx, k)),
        arith::add(caster.1, arith::mul(dy, k)),
    )
}

/// The gates of the Vampiric Aura damage handler (`0x6F1AD980`, buff vtable slot `0x170`) and its result: the life the
/// attacker gets, `damage * fraction` (soft multiply), or `None` when a gate stops it. Gate order as in the original:
/// the record's ignore flag, a mechanical victim (class bit `0x8`), not an enemy (`0x6F285B20`), an invulnerable
/// victim, a victim that answers message `0xD0142` (`blocked`, not modelled by the sim), damage <= 0 (soft `le`).
/// For an illusion on either side the heal needs Misc `IllusionsCanRestoreLife` (`illusion_ok`).
pub struct VampiricGate {
    pub ignore_flag: bool,
    pub victim_class: u32,
    pub enemy: bool,
    pub invulnerable: bool,
    pub blocked: bool,
    pub illusion: bool,
    pub illusion_ok: bool,
}

pub fn vampiric_heal(gate: &VampiricGate, damage: F32, fraction: F32) -> Option<F32> {
    vampiric_outcome(gate, damage, fraction).1
}

/// [`vampiric_heal`] with the presentation call: `(reached, heal)`. `reached` is whether the handler got past the gates
/// to the point where it calls the effect routine `0x6F053040` (also when an illusion stops the heal).
pub fn vampiric_outcome(gate: &VampiricGate, damage: F32, fraction: F32) -> (bool, Option<F32>) {
    if gate.ignore_flag || gate.victim_class & 0x8 != 0 || !gate.enemy || gate.invulnerable || gate.blocked {
        return (false, None);
    }
    if arith::le(damage, F32::ZERO) {
        return (false, None);
    }
    let heal = arith::mul(damage, fraction);
    (true, (!gate.illusion || gate.illusion_ok).then_some(heal))
}

// ---------------------------------------------------------------------------------------------------------------
// Death Pact

/// `CAbilityDeathPact` effect (`0x6F09FD50`, CONFIRMED structure, amounts Unicorn-checked). With DataE == 0 the
/// target dies (kill credit to the caster, unit flags `0x200` cleared and `0x1000` set: UNSURE meaning, modelled as
/// "no further effect"), then, unless the target is an illusion, the caster gets the mana amount (`0x6F279140`) and
/// the life amount (`0x6F277C20`), each only when non-zero. DataE != 0 is a drain variant of the same body (the
/// target loses both amounts as life, the caster gets what the target's life setter reports): UNSURE, not modelled.
fn death_pact(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, caster: ObjKey, level: &SpellLevel, target: ObjKey) {
    let mut killed = None;
    {
        let mut gm = game.borrow_mut();
        if !gm.object_is_live_unit(target) {
            return;
        }
        let data = [level.data[0], level.data[1], level.data[2], level.data[3], level.data[4]];
        if data[4].to_f32() != 0.0 {
            return;
        }
        let life = gm.unit(target).life;
        let (mana, heal) = death_pact_amounts(life, &data);
        let illusion = gm.unit(target).illusion;
        if set_life(&mut gm, target, F32::ZERO) {
            killed = Some(target);
        }
        if !illusion && gm.object_is_live_unit(caster) {
            if nonzero(mana) {
                let (m, max) = (gm.unit(caster).mana, gm.unit(caster).max_mana);
                let sum = arith::add(m, mana);
                gm.write_mana(caster, if arith::lt(max, sum) { max } else { sum });
            }
            if nonzero(heal) {
                heal_unit(&mut gm, caster, heal);
            }
        }
    }
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

/// The x87 `0.0 != x` test of the original (a NaN counts as non-zero).
pub fn nonzero(x: F32) -> bool {
    x.to_f32() != 0.0
}

/// `AddLife` of `amount` clamped to the maximum (the life listeners `68..82` of `0x6F277C20` are not modelled).
fn heal_unit(gm: &mut Game, unit: ObjKey, amount: F32) {
    let u = gm.unit(unit);
    let sum = arith::add(u.life, amount);
    let next = if arith::lt(u.max_life, sum) { u.max_life } else { sum };
    set_life(gm, unit, next);
}

// ---------------------------------------------------------------------------------------------------------------
// Blink

/// `CAbilityBlink` (point order). Cast start `0x6F1AEFF0`: the point is validated by [`blink_project`] and, if it is
/// still beyond DataA (squared compare), replaced by the caster's own position (no move). Effect `0x6F1AF180`: the
/// caster is placed at the point (unit slot `0x180`, placement flags from `0x6F2ABF20`: pathing, not modelled) after
/// the playable-area clamp.
fn blink(game: &RefCell<Game>, caster: ObjKey, level: &SpellLevel, x: F32, y: F32) {
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(caster) {
        return;
    }
    let c = (gm.unit(caster).x, gm.unit(caster).y);
    let mut p = match gm.order_tables() {
        Some(t) => blink_project(c, (x, y), level.data[0], t),
        None => (x, y),
    };
    let dx = arith::sub(p.0, c.0);
    let dy = arith::sub(p.1, c.1);
    let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
    if arith::lt(arith::mul(level.data[0], level.data[0]), d2) {
        p = c;
    }
    let (px, py) = gm.clamp_unit_move(p.0, p.1);
    gm.store_unit_position(caster, px, py);
}

// ---------------------------------------------------------------------------------------------------------------
// Carrion Swarm

/// `CAbilityCarrionSwarm` effect (`0x6F1DD4F0`, argument roles CONFIRMED, geometry and hit order UNSURE). The caster
/// launches a swarm missile along `normalize(point - caster)` (the caster's facing when the point is closer than 20
/// units: the compare against the constant `0x6FAB35F4` = 20.0), with Area as the width at the start, DataC as the
/// width at the end, DataB the distance, DataA the damage to each unit and DataD the maximum total damage. The
/// missile class `CMissileCarrionSwarm` (`0x6F8FBAAC`) does the hits; its walk is not decoded, so the model hits
/// every live enemy non-structure unit in the swept trapezoid once, in order of distance along the line, and stops
/// when DataD is used up (the last hit is cut to what is left). Damage class: the spell class `0x200`.
fn carrion_swarm(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    caster: ObjKey,
    level: &SpellLevel,
    x: F32,
    y: F32,
) {
    let mut deaths = Vec::new();
    {
        let mut gm = game.borrow_mut();
        if !gm.object_is_live_unit(caster) {
            return;
        }
        let (cx, cy, facing) = (gm.unit(caster).x.to_f32(), gm.unit(caster).y.to_f32(), gm.unit(caster).facing.to_f32());
        let (mut dx, mut dy) = (x.to_f32() - cx, y.to_f32() - cy);
        if dx * dx + dy * dy < 400.0 {
            let a = facing.to_radians();
            (dx, dy) = (a.cos(), a.sin());
        }
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let dist = level.data[1].to_f32();
        let (w0, w1) = (level.area.to_f32(), level.data[2].to_f32());
        let c_owner = gm.unit(caster).owner;
        let reach = dist + w0.max(w1);
        let mut hits: Vec<(f32, ObjKey)> =
            circle_candidates(&gm, F32::from_bits(cx.to_bits()), F32::from_bits(cy.to_bits()), F32::from_bits(reach.to_bits()))
                .into_iter()
                .filter(|&k| gm.object_is_live_unit(k) && gm.is_enemy(c_owner, gm.unit(k).owner))
                .filter(|&k| !gm.unit_types().is_building(gm.unit(k).type_id))
                .filter_map(|k| {
                    let (px, py) = (gm.unit(k).x.to_f32() - cx, gm.unit(k).y.to_f32() - cy);
                    let along = px * ux + py * uy;
                    let side = (px * -uy + py * ux).abs();
                    if !(0.0..=dist).contains(&along) {
                        return None;
                    }
                    let width = if dist > 0.0 { w0 + (w1 - w0) * along / dist } else { w0 };
                    (side <= width).then_some((along, k))
                })
                .collect();
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut left = level.data[3];
        let unlimited = left.to_f32() <= 0.0;
        for (_, k) in hits {
            if !unlimited && !arith::lt(F32::ZERO, left) {
                break;
            }
            let dmg = if unlimited || !arith::lt(left, level.data[0]) { level.data[0] } else { left };
            if gm.unit_is_invulnerable(k) {
                continue;
            }
            gm.spells_hero_state_mut().swarm_hits.push((caster, k, dmg));
            if let Some(d) = gm.unit_damage_target(caster, k, dmg, false, false, 0, FROST_NOVA_DAMAGE_TYPE) {
                deaths.push(d);
            }
            if !unlimited {
                left = arith::sub(left, dmg);
            }
        }
    }
    for t in deaths {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Sleep

/// `CAbilitySleep` effect (`0x6F1FBA80`): the target gets the Sleep buff `'BUsl'` for HeroDur (hero) or Dur and is
/// disabled while it lasts (the model counts it like a stun). The wake rule is CONFIRMED from `CBuffSleep`'s event
/// handler `0x6F1F6870`: a damage event with a non-zero amount ends the buff unless the unit has `'BUst'` or `'BUsp'`
/// (see [`Game::wake_sleeping`]). The second buff the original applies with DataA as its argument (`'BUst'` /
/// `'BUsp'` through the buff factory or `CBuffSleepStun`) is not modelled: UNSURE what DataA is.
fn sleep(game: &RefCell<Game>, caster: ObjKey, level: &SpellLevel, target: ObjKey) {
    let _ = caster;
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(target) {
        return;
    }
    let hero = gm.unit(target).hero.is_some() || is_hero_id(gm.unit(target).type_id);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
    gm.add_spell_buff(target, BUFF_SLEEP, BuffEffect::Sleep, dur);
    // The second buff (S33, CONFIRMED: `0x6F1FBA80` looks up `'BUst'` and `'BUsp'` after the main buff): DataA is the
    // duration handed to it. A target that already has `'BUst'` gets that buff refreshed (BuffID list index 2),
    // otherwise `'BUsp'` (index 1; `CBuffSleepPause`, ctor `0x6F1F9DA0`) is created or refreshed. While either exists
    // damage does not wake the sleeper. A zero DataA gives a buff without a timer in the original (UNSURE what ends
    // it); the model applies none.
    let keeper = if gm.unit(target).buffs.iter().any(|b| b.base == BUFF_SLEEP_STATIONARY) {
        BUFF_SLEEP_STATIONARY
    } else {
        BUFF_SLEEP_PAUSE
    };
    gm.add_spell_buff(target, keeper, BuffEffect::Marker, level.data[0]);
}

// ---------------------------------------------------------------------------------------------------------------
// Shadow Strike

/// `CAbilityShadowStrike` effect (`0x6F1F2670`): launches `CMissileShadowStrike` (`0x6F916CFC`) at the target. CONFIRMED:
/// the damage record the missile carries holds DataE as its amount; the buff `'BEsh'` is created on arrival
/// (`0x6F1FB760`) from the missile's stored copies of DataA..DataD and has one slow factor applied to both movement
/// and attack speed (`0x6F128E70`, `0x6F202FF0` + `0x6F0C9090`). UNSURE: which Data field is which (the model takes
/// DataA = damage per second, DataB = slow fraction, DataC / DataD = decay, unused) and the slow decay, which is not
/// decoded (constant slow). Arrival time = distance over the ability's missile speed (instant without one).
fn shadow_strike(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, caster: ObjKey, level: &SpellLevel, target: ObjKey) {
    let (id, delay) = {
        let mut gm = game.borrow_mut();
        if !gm.object_is_live_unit(target) || !gm.object_is_live_unit(caster) {
            return;
        }
        let speed = gm.spell_missile_speed(SHADOW_STRIKE);
        let (a, b) = (gm.unit(caster), gm.unit(target));
        let dx = b.x.to_f32() - a.x.to_f32();
        let dy = b.y.to_f32() - a.y.to_f32();
        let seconds = if speed.to_f32() > 0.0 { (dx * dx + dy * dy).sqrt() / speed.to_f32() } else { 0.0 };
        let st = gm.spells_hero_state_mut();
        st.next_strike += 1;
        let id = st.next_strike;
        st.strikes.insert(id, Strike { caster, target, level: *level });
        (id, seconds)
    };
    if delay > 0.0 {
        game.borrow_mut().timers_start_hero(delay, TICK_STRIKE_HIT, target, id);
    } else {
        strike_hit(game, rt, id);
    }
}

fn strike_hit(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let killed;
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(st) = gm.spells_hero_state_mut().strikes.remove(&id) else {
            return;
        };
        caster = st.caster;
        if !gm.object_is_live_unit(st.target) {
            return;
        }
        let level = st.level;
        killed = gm.unit_damage_target(caster, st.target, level.data[4], false, false, 0, SHADOW_STRIKE_DAMAGE_TYPE);
        if killed.is_none() {
            let hero = gm.unit(st.target).hero.is_some() || is_hero_id(gm.unit(st.target).type_id);
            let dur = crate::cast::effect_duration(&level, hero, F32::ZERO);
            let slow = arith::neg(F32::from_bits(level.data[1].to_bits() & 0x7FFF_FFFF));
            let effect = BuffEffect::Slow { move_delta: slow, attack_delta: slow };
            let serial = gm.add_spell_buff(st.target, BUFF_SHADOW_STRIKE, effect, dur);
            if serial != 0 {
                let period = level.cast.to_f32().max(MIN_PERIOD_SECONDS);
                gm.spells_hero_state_mut().strike_dots.insert(serial, (caster, level.data[0], period));
                gm.timers_start_hero(period, TICK_STRIKE_DOT, st.target, serial);
            }
        }
    }
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

fn strike_dot(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, serial: u32) {
    let mut killed = None;
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(&(c, dmg, period)) = gm.spells_hero_state_mut().strike_dots.get(&serial) else {
            return;
        };
        caster = c;
        if !gm.spell_buff_alive(unit, serial) {
            gm.spells_hero_state_mut().strike_dots.remove(&serial);
            return;
        }
        if gm.object_is_live_unit(unit) {
            killed = gm.unit_damage_target(c, unit, dmg, false, false, 0, STRIKE_DOT_DAMAGE_TYPE);
        }
        if killed.is_none() {
            gm.timers_start_hero(period, TICK_STRIKE_DOT, unit, serial);
        } else {
            gm.spells_hero_state_mut().strike_dots.remove(&serial);
        }
    }
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Immolation

/// `CAbilityImmolation` (an immediate toggle, buff `'BEim'`). CONFIRMED: the buff is created with a mana drain
/// (`0x6F12C4D0`: a regeneration modifier of minus the drain on the mana property, the pulse message `0xD01C1`
/// armed through `0x6F124800`); the ability's getter at slot `0x36C` returns DataC. UNSURE (public data): DataA =
/// damage per pulse to enemy units within Area, DataB = mana drain per second, DataC = mana that must be available
/// to turn it on, pulse every 1.0 s. The model drains and damages at each pulse and turns itself off when the mana
/// cannot pay the drain.
fn immolation(game: &RefCell<Game>, caster: ObjKey, base: u32, level: &SpellLevel) {
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(caster) {
        return;
    }
    if let Some(run) = gm.spells_hero_state_mut().immolations.remove(&(caster, base)) {
        gm.unit_mut(caster).buffs.retain(|b| b.serial != run.serial);
        return;
    }
    if arith::lt(gm.unit(caster).mana, level.data[2]) {
        return;
    }
    let serial = gm.add_buff(caster, BUFF_IMMOLATION, crate::abilities::buff_predicates(BUFF_IMMOLATION));
    gm.spells_hero_state_mut().immolations.insert(
        (caster, base),
        Immolation { serial, damage: level.data[0], drain: level.data[1], area: level.area },
    );
    gm.timers_start_hero(IMMOLATION_PULSE_SECONDS, TICK_IMMOLATION, caster, serial);
}

fn immolation_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, serial: u32) {
    let mut deaths = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let found = gm
            .spells_hero_state_mut()
            .immolations
            .iter()
            .find(|(k, r)| k.0 == unit && r.serial == serial)
            .map(|(k, r)| (*k, *r));
        let Some(((u, base), run)) = found else {
            return;
        };
        if !gm.object_is_live_unit(u) {
            gm.spells_hero_state_mut().immolations.remove(&(u, base));
            return;
        }
        if arith::lt(gm.unit(u).mana, run.drain) {
            gm.spells_hero_state_mut().immolations.remove(&(u, base));
            gm.unit_mut(u).buffs.retain(|b| b.serial != serial);
            return;
        }
        let mana = arith::sub(gm.unit(u).mana, run.drain);
        gm.write_mana(u, mana);
        let (ux, uy) = (gm.unit(u).x, gm.unit(u).y);
        let owner = gm.unit(u).owner;
        let victims: Vec<ObjKey> = circle_candidates(&gm, ux, uy, run.area)
            .into_iter()
            .filter(|&k| k != u && gm.object_is_live_unit(k) && gm.is_enemy(owner, gm.unit(k).owner))
            .collect();
        for k in victims {
            if gm.unit_is_invulnerable(k) {
                continue;
            }
            if let Some(d) = gm.unit_damage_target(u, k, run.damage, false, false, 0, FROST_NOVA_DAMAGE_TYPE) {
                deaths.push(d);
            }
        }
        gm.timers_start_hero(IMMOLATION_PULSE_SECONDS, TICK_IMMOLATION, u, serial);
    }
    for t in deaths {
        dispatch_unit_death_by(game, rt, t, Some(unit));
    }
}

/// Inclusive circle test in world units through the soft float (UNSURE: the tree query's own test is not decoded).
fn within(px: F32, py: F32, cx: F32, cy: F32, r: F32) -> bool {
    let dx = arith::sub(px, cx);
    let dy = arith::sub(py, cy);
    let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
    !arith::lt(arith::mul(r, r), d2)
}

/// Trees are destructables whose id has `T` as its second character (`LTlt`, `ATtr`, ...): UNSURE heuristic.
pub fn is_tree(type_id: u32) -> bool {
    (type_id >> 16) & 0xFF == u32::from(b'T')
}

/// A due module timer.
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, aux: u32) {
    match kind {
        TICK_ROOTS => roots_tick(game, rt, unit, aux),
        TICK_IMMOLATION => immolation_tick(game, rt, unit, aux),
        TICK_STRIKE_HIT => strike_hit(game, rt, aux),
        TICK_STRIKE_DOT => strike_dot(game, rt, unit, aux),
        _ => {}
    }
}

fn roots_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, aux: u32) {
    let mut killed = None;
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(&(c, dmg)) = gm.spells_hero_state_mut().roots.get(&aux) else {
            return;
        };
        if !gm.spell_buff_alive(unit, aux) {
            gm.spells_hero_state_mut().roots.remove(&aux);
            return;
        }
        caster = Some(c);
        // The pulse does nothing while the target has the Sleep buff `'BUsl'` (`0x6F0D54B0` looks it up first); the
        // timer is re-armed before the pulse body runs, so the ticks go on.
        let asleep = gm.object_is_live_unit(unit) && gm.unit(unit).buffs.iter().any(|b| b.base == BUFF_SLEEP);
        if gm.object_is_live_unit(unit) && !asleep {
            killed = gm.unit_damage_target(c, unit, dmg, false, false, 0, ROOTS_DAMAGE_TYPE);
        }
        if killed.is_none() {
            gm.timers_start_hero(ROOT_TICK_SECONDS, TICK_ROOTS, unit, aux);
        } else {
            gm.spells_hero_state_mut().roots.remove(&aux);
        }
    }
    if let (Some(t), Some(c)) = (killed, caster) {
        dispatch_unit_death_by(game, rt, t, Some(c));
    }
}

impl Game {
    /// Host API: the unit type Force of Nature creates (the level's UnitID, which `SpellLevel` does not carry).
    pub fn set_summon_unit(&mut self, base: u32, unit_type: u32) {
        self.spells_hero_state_mut().set_summon_unit(base, unit_type);
    }

    /// Sets the Misc `FrostMoveSpeedDecrease` and `FrostAttackSpeedDecrease` used by Frost Nova's slow.
    pub fn set_frost_misc(&mut self, move_decrease: F32, attack_decrease: F32) {
        self.spells_hero_state_mut().frost_misc = (move_decrease, attack_decrease);
    }

    /// The treants Force of Nature created, `(caster, unit)` in creation order.
    pub fn treants(&self) -> &[(ObjKey, ObjKey)] {
        &self.spells.hero.treants
    }

    fn object_is_live_unit(&self, k: ObjKey) -> bool {
        matches!(self.object(k), Object::Unit(u) if !u.dead && !u.removed)
    }

    /// A damage event with a non-zero amount reached `unit`: its Sleep buff ends unless the unit has `'BUst'` or
    /// `'BUsp'` (CONFIRMED rule of `CBuffSleep`'s handler `0x6F1F6870`). Called by the damage paths.
    pub(crate) fn wake_sleeping(&mut self, unit: ObjKey) {
        let u = self.unit(unit);
        if !u.buffs.iter().any(|b| b.base == BUFF_SLEEP) {
            return;
        }
        if u.buffs.iter().any(|b| b.base == BUFF_SLEEP_STATIONARY || b.base == BUFF_SLEEP_PAUSE) {
            return;
        }
        let serials: Vec<u32> = u.buffs.iter().filter(|b| b.base == BUFF_SLEEP).map(|b| b.serial).collect();
        for s in serials {
            self.drop_spell_buff(s);
        }
    }

    /// Vampiric Aura's life steal after `attacker` dealt `damage` to `victim` (`0x6F1AD980`, gates in
    /// [`vampiric_heal`]). The attacker needs a Vampiric buff (the strength is its stored DataA fraction).
    pub(crate) fn apply_lifesteal(&mut self, attacker: ObjKey, victim: ObjKey, damage: F32) {
        let Some(fraction) = self.vampiric_fraction(attacker) else {
            return;
        };
        if !self.object_is_live_unit(attacker) {
            return;
        }
        let (a_owner, v_owner) = (self.unit(attacker).owner, self.unit(victim).owner);
        let gate = VampiricGate {
            ignore_flag: false,
            victim_class: self.unit(victim).class_flags,
            enemy: self.is_enemy(a_owner, v_owner),
            invulnerable: self.unit_is_invulnerable(victim),
            // The message `0xD0142` query of the victim's abilities (`0x6F28D750`) is not modelled.
            blocked: false,
            illusion: self.unit(attacker).illusion || self.unit(victim).illusion,
            // Misc `IllusionsCanRestoreLife`: a missing key reads 0 (the Misc reader rule); not loaded by the sim.
            illusion_ok: false,
        };
        if let Some(heal) = vampiric_heal(&gate, damage, fraction) {
            heal_unit(self, attacker, heal);
        }
    }

    /// The units Carrion Swarm hit, `(caster, victim, damage)` in hit order.
    pub fn swarm_hits(&self) -> &[(ObjKey, ObjKey, F32)] {
        &self.spells.hero.swarm_hits
    }

    pub(crate) fn spell_missile_speed(&self, base: u32) -> F32 {
        self.spell_missile_speed_of(base)
    }
}

/// What one periodic pulse of a buff of the `0x6F0D54B0` family does (CONFIRMED structure, Unicorn
/// `tools/emu/undead_ne3_emu.py` CHECK=pulse on `CBuffEntanglingRoots`, 600 cases, 0 mismatches).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pulse {
    /// The pulse does nothing (the target sleeps, or the Misc immunity gate rejects it).
    Skip,
    /// A damage record with this amount (class `0x100000` for Roots) and record flags `2` (`0` while the buff's
    /// first-pulse bit `0x8000000` is still set) is delivered to the target.
    Damage { amount: F32, record_flags: u32 },
    /// A negative amount: multiplied by Misc `EtherealHealBonus` and added through AddLife (`0x6F277C20`).
    AddLife(F32),
}

/// Inputs of [`pulse`]: the target's sleep state, the Misc gate `MagicImmunesResistDamage`, the class tests of the
/// damage class word (`0x6F2555E0` / `0x6F2555F0`), the target's immunity counters (`+0x15C` / `+0x158`) and the
/// buff's first-pulse bit.
#[derive(Debug, Clone, Copy)]
pub struct PulseInput {
    pub asleep: bool,
    pub gate: bool,
    pub class_a: bool,
    pub class_b: bool,
    pub immune_a: bool,
    pub immune_b: bool,
    pub first_pulse: bool,
    pub ethereal_bonus: F32,
}

/// The pulse body of `0x6F0D54B0` for an amount (`buff + 0xEC`).
pub fn pulse(input: &PulseInput, amount: F32) -> Pulse {
    if input.asleep {
        return Pulse::Skip;
    }
    if input.gate && ((input.immune_a && input.class_a) || (input.immune_b && input.class_b)) {
        return Pulse::Skip;
    }
    // x87 `fcompp` of 0.0 against the amount: the damage branch is taken unless `amount < 0.0` (NaN included).
    if arith::lt(amount, F32::ZERO) {
        return Pulse::AddLife(arith::mul(amount, input.ethereal_bonus));
    }
    Pulse::Damage { amount, record_flags: if input.first_pulse { 0 } else { 2 } }
}
