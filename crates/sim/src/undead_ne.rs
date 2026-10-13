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
/// Dark Ritual shares the effect `0x6F09FD50` with Death Pact (`CAbilityDeathPact` derives from `CAbilityDarkRitual`;
/// both have slot `0x424` = `0x6F09FD50`, S33). Order id 852219 (getter `0x6F0BD230`).
pub const DARK_RITUAL: u32 = code(b"AUdr");
pub const CARRION_SWARM: u32 = code(b"AUcs");
pub const SLEEP: u32 = code(b"AUsl");
pub const SHADOW_STRIKE: u32 = code(b"AEsh");
pub const BLINK: u32 = code(b"AEbl");
pub const IMMOLATION: u32 = code(b"AEim");
/// Formerly misnamed `IMMOLATION_UNDEAD`: `AUim` is the Crypt Lord's Impale ([`IMPALE`], S37; stock rows and the atlas).
#[deprecated(note = "AUim is Impale")]
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
/// The victim marker of Immolation (`CBuffImmolationAoe`, a `CBuffAoe`: a duration-only buff, S37).
pub const BUFF_IMMOLATION_AOE: u32 = code(b"BEia");
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
    /// Running Shadow Strike slow decays: buff serial -> record (S37).
    strike_slows: BTreeMap<u32, StrikeSlow>,
    next_strike: u32,
    /// Misc `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease` (S33, CONFIRMED source of the Frost Nova slow);
    /// `0.0` when the key is absent, as the original's reader defaults.
    frost_misc: (F32, F32),
    /// Units hit by Carrion Swarm, `(caster, victim, damage)` in hit order (for tests and the host).
    pub swarm_hits: Vec<(ObjKey, ObjKey, F32)>,
    /// Running swath missiles (Carrion Swarm, S37): id -> record.
    swarms: BTreeMap<u32, Swarm>,
    next_swarm: u32,
    /// Inferno landings waiting for their impact delay (S37).
    infernos: BTreeMap<u32, Inferno>,
    next_inferno: u32,
    /// Running Death and Decay effects (S37).
    dnds: BTreeMap<u32, DeathAndDecay>,
    next_dnd: u32,
    /// Running Impales (S37).
    impales: BTreeMap<u32, Impale>,
    next_impale: u32,
    /// Running Frost Armors: buff serial -> record (S37).
    frost_armors: BTreeMap<u32, FrostArmor>,
}

#[derive(Debug, Clone, Copy)]
struct Immolation {
    serial: u32,
    damage: F32,
    drain: F32,
    area: F32,
    /// The level's Dur: the pulse interval and the base of the victim marker's duration.
    dur: F32,
}

/// The state of one Shadow Strike slow decay: DataB / DataC / DataD, the buff duration, the 1.0 s ticks done, the
/// factor applied last and the move / attack speed factors (start 1.0, the setters add to them).
#[derive(Debug, Clone, Copy)]
struct StrikeSlow {
    m: F32,
    a: F32,
    d: F32,
    dur: F32,
    ticks: u32,
    prev: F32,
    fm: F32,
    fa: F32,
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
/// A swath missile tick (`aux` = the swarm id), S37.
pub const TICK_SWARM: u8 = 5;
/// The 1.0 s slow update of a Shadow Strike buff (`aux` = the buff serial), S37.
pub const TICK_STRIKE_SLOW: u8 = 7;
/// The impact of an Inferno (`aux` = the landing id), S37.
pub const TICK_INFERNO: u8 = 8;
/// The 0.1 s poll of a Death and Decay effect (`aux` = the effect id), S37.
pub const TICK_DND: u8 = 9;
/// A Death and Decay victim pulse (`unit` = the victim, `aux` = `(effect id << 16) | victim index`), S37.
pub const TICK_DND_PULSE: u8 = 10;
/// An Impale spike (`aux` = the impale id), S37.
pub const TICK_IMPALE: u8 = 11;
/// A Frost Armor ends (`aux` = the buff serial), S37.
pub const TICK_FROST_ARMOR_END: u8 = 12;
/// A swath missile arrives (`aux` = the swarm id), S37.
pub const TICK_SWARM_ARRIVE: u8 = 6;

/// Whether the level's `Cast` field of `base` is not a cast-time delay: Shadow Strike's Cast (3 s in the stock data)
/// is the pulse period of its buff (S33, CONFIRMED by the property flow of the launcher and the buff init), so the
/// cast pipeline must not wait on it.
pub fn cast_field_is_period(base: u32) -> bool {
    base == SHADOW_STRIKE
}

/// Whether `base` is cast without a target (an immediate order). Only Immolation (the toggle).
pub fn is_immediate(base: u32) -> bool {
    base == IMMOLATION
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
        (DEATH_PACT | DARK_RITUAL, CastTarget::Unit(t)) => death_pact(game, rt, caster, level, t),
        (SLEEP, CastTarget::Unit(t)) => sleep(game, caster, level, t),
        (SHADOW_STRIKE, CastTarget::Unit(t)) => shadow_strike(game, rt, caster, level, t),
        (BLINK, CastTarget::Point(x, y)) => blink(game, caster, level, x, y),
        (CARRION_SWARM, CastTarget::Point(x, y)) => carrion_swarm(game, rt, caster, level, x, y),
        (INFERNO, CastTarget::Point(x, y)) => inferno(game, rt, caster, level, x, y),
        (DEATH_AND_DECAY, CastTarget::Point(x, y)) => death_and_decay(game, caster, level, x, y),
        (IMPALE, CastTarget::Point(x, y)) => impale(game, caster, level, x, y),
        (FROST_ARMOR, CastTarget::Unit(t)) => frost_armor(game, caster, level, t),
        (IMMOLATION, CastTarget::None) => immolation(game, caster, base, level),
        (
            DEATH_COIL | FROST_NOVA | ENTANGLING_ROOTS | FORCE_OF_NATURE | DEATH_PACT | DARK_RITUAL | SLEEP | SHADOW_STRIKE | BLINK
            | CARRION_SWARM | IMMOLATION | INFERNO | DEATH_AND_DECAY | IMPALE | FROST_ARMOR,
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
// The swath engine shared by Carrion Swarm (`AUcs`) and Shockwave (`AOsh`, `CAbilityShockwave` derives from
// `CAbilityCarrionSwarm`, so the orc lane reuses everything below; only the level data differ). S37.

/// The "safe division" of the property helpers `0x6F022530` / `0x6F0223C0`: `a / b` through the soft divide unless
/// `b == 0.0` (x87 compare: `-0.0` is zero, NaN is not); then `a == 0.0` gives `1.0` (`0x6FAAE4F4`), `a < 0.0` gives
/// `-FLT_MAX` (`0x6FAAE628`), anything else (also NaN) `+FLT_MAX` (`0x6FAAE624`).
pub fn safe_div(a: F32, b: F32, t: &Tables) -> F32 {
    if b.to_f32() != 0.0 {
        return arith::div(a, b, t);
    }
    let af = a.to_f32();
    if af == 0.0 {
        F32::from_bits(0x3F80_0000)
    } else if af < 0.0 {
        F32::from_bits(0xFF7F_FFFF)
    } else {
        F32::from_bits(0x7F7F_FFFF)
    }
}

/// The robust "greater than" `0x6F07C450` (also the test of `0x6F022270` against 0.0): true when `a > b` by the x87
/// compare and `|soft_sub(a, b)| >= 0.001` (`0x6FAAE47C`). `0x6F022270(prop, &0.0)` is `gt_eps(prop, 0.0)`.
pub fn gt_eps(a: F32, b: F32) -> bool {
    if !(b.to_f32() < a.to_f32()) {
        return false;
    }
    let d = F32::from_bits(arith::sub(a, b).to_bits() & 0x7FFF_FFFF);
    f32::from_bits(0x3A83_126F) <= d.to_f32()
}

/// `0x6F07B870`: rescales `(x, y)` to length `len`: `k = len * recip(sqrt(x*x + y*y))`, `(x*k, y*k)` (the same
/// normalisation Blink uses, CONFIRMED by Unicorn there and here).
pub fn rescale(x: F32, y: F32, len: F32, t: &Tables) -> (F32, F32) {
    let yy = arith::mul(y, y);
    let xx = arith::mul(x, x);
    let n2 = arith::add(xx, yy);
    let inv = arith::recip(softfloat::math::sqrt(n2, t), t);
    let k = arith::mul(len, inv);
    (arith::mul(x, k), arith::mul(y, k))
}

/// The start and end point of a swath missile (`0x6F1DD4F0`, CONFIRMED by Unicorn): with `(dx, dy)` the vector from
/// the caster to the target point and `d2 = dx*dx + dy*dy`, the direction is `(dx, dy)` rescaled to length `area`
/// when `d2 >= 20.0^2` (`0x6FAB35F4` squared by the soft multiply; NaN and smaller use the facing), else the polar
/// vector `(area * cos(facing), area * sin(facing))` (`0x6F10E370`, the facing in radians). `start = caster + v1`
/// (so the swarm starts `Area` units in front of the caster), `end = start + v1 rescaled to length distance`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwathLaunch {
    pub start: (F32, F32),
    pub end: (F32, F32),
}

pub fn swath_launch(
    caster: (F32, F32),
    target: (F32, F32),
    facing: F32,
    area: F32,
    distance: F32,
    t: &Tables,
) -> SwathLaunch {
    let dy = arith::sub(target.1, caster.1);
    let dx = arith::sub(target.0, caster.0);
    let yy = arith::mul(dy, dy);
    let xx = arith::mul(dx, dx);
    let d2 = arith::add(xx, yy);
    let twenty = F32::from_bits(0x41A0_0000);
    let v1 = if d2.to_f32() >= arith::mul(twenty, twenty).to_f32() {
        rescale(dx, dy, area, t)
    } else {
        let s = softfloat::math::sin(facing, t);
        let c = softfloat::math::cos(facing, t);
        let y = arith::mul(area, s);
        let x = arith::mul(area, c);
        (x, y)
    };
    let start = (arith::add(caster.0, v1.0), arith::add(caster.1, v1.1));
    let v2 = rescale(v1.0, v1.1, distance, t);
    SwathLaunch { start, end: (arith::add(start.0, v2.0), arith::add(start.1, v2.1)) }
}

/// The swath data of a running missile (`CMissileCarrionSwarm` properties, CONFIRMED mapping by the setup
/// `0x6F1F28C0`): `start` (`+0x90`/`+0x98`), `distance` = DataC (`+0xA0`), `width0` = Area (`+0xC8`), `width1` =
/// DataD (`+0xD0`, the final width), `damage` = DataA (`+0xD8`), `cap` = DataB (`+0xE0`, the total that may still be
/// dealt; the stock rows call it "max damage").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwathData {
    pub start: (F32, F32),
    pub distance: F32,
    pub width0: F32,
    pub width1: F32,
    pub damage: F32,
    pub cap: F32,
}

/// The radius of the query circle at the missile position (`0x6F1EF6D0`): `dist = |pos - start|` (soft sqrt of
/// `dx*dx + dy*dy`), `frac = safe_div(dist, distance)`, `radius = width0 + (width1 - width0) * frac`.
pub fn swath_radius(d: &SwathData, pos: (F32, F32), t: &Tables) -> F32 {
    let dy = arith::sub(pos.1, d.start.1);
    let dx = arith::sub(pos.0, d.start.0);
    let yy = arith::mul(dy, dy);
    let xx = arith::mul(dx, dx);
    let dist = softfloat::math::sqrt(arith::add(xx, yy), t);
    let frac = safe_div(dist, d.distance, t);
    arith::add(d.width0, arith::mul(arith::sub(d.width1, d.width0), frac))
}

/// The damage each unit found by one tick receives: `share = safe_div(cap, I2R(n))`; `damage` when `share > damage`
/// (robust compare), else `share` (the remaining total is spread equally over the units of this tick).
pub fn swath_damage_each(d: &SwathData, n: usize, t: &Tables) -> F32 {
    let share = safe_div(d.cap, softfloat::conv::i2r(n as i32), t);
    if gt_eps(share, d.damage) {
        d.damage
    } else {
        share
    }
}

/// The cap after a unit lost `before - after` life: `cap - (before - after)` (the cap counts the life really lost,
/// not the nominal damage, CONFIRMED order and operands).
pub fn swath_cap_after(cap: F32, before: F32, after: F32) -> F32 {
    arith::sub(cap, arith::sub(before, after))
}

/// Whether the tick re-arms its 0.1 s timer: the remaining cap is above 0.01 by the robust compare.
pub fn swath_rearm(cap: F32) -> bool {
    gt_eps(cap, F32::from_bits(0x3C23_D70A))
}

/// The property `+0xEC` after a tick: `3.0 - (0.1 + 0.02)`, one soft subtract per tick (a countdown the original
/// keeps; nothing found reads it for gameplay).
pub fn swath_countdown(ec: F32) -> F32 {
    arith::sub(ec, arith::add(F32::from_bits(0x3DCC_CCCD), F32::from_bits(0x3CA3_D70A)))
}

/// Tick period (`0x6FAB35F8` = 0.1) and the lifetime of the "hit" marker buff (`0.1 + 0.02`, `0x6F1E79E0`).
pub const SWATH_TICK_BITS: u32 = 0x3DCC_CCCD;
/// The query cap of the swarm callback `0x6F1E7B00`: at most 64 units per tick.
pub const SWATH_MAX_UNITS: usize = 64;

// ---------------------------------------------------------------------------------------------------------------
// Carrion Swarm

/// `CAbilityCarrionSwarm` effect (`0x6F1DD4F0`, S37). CONFIRMED (Unicorn, `tools/emu/undead_ne4_emu.py`): the
/// launcher ([`swath_launch`]) builds the start point (`Area` units in front of the caster along `normalize(point -
/// caster)`, the caster's facing when the point is closer than 20) and the end point (`DataC` further), and creates
/// `CMissileCarrionSwarm` (`0x6F8FBAAC`) whose setup `0x6F1F28C0` stores Area = start width, DataD = final width,
/// DataA = damage, DataB = total cap, DataC = distance. The setup runs one tick at once (at the start point); the tick
/// ([`swarm_tick`]) re-arms itself every 0.1 s while the remaining cap exceeds 0.01, and the arrival of the missile
/// at the end point runs one last tick when the cap is above 0.001 (`0x6F1F2A20`).
///
/// A tick queries the circle around the missile (radius from [`swath_radius`], shape 11, mask `0x1F`; the callback
/// `0x6F1E7B00` keeps at most 64 units that pass the enemy-only target acceptance with the ability's `targs` and are
/// not marked), gives each unit `min(DataA, cap / n)` ([`swath_damage_each`]) as a record of class `0x8000`
/// (record flags `0x41`, `0x51` for a blocked unit) and lowers the cap by the life each unit really lost. Each hit
/// unit gets the marker buff `'BUcs'` for `0.1 + 0.02` s; the callback refreshes the marker of a unit it meets again
/// and rejects it, so a unit that stays in the swath is hit once (LIKELY: callback `0x6F1E79E0` read, not run).
/// UNSURE: the missile's flight (the model moves it linearly at the ability's `Missilespeed` from the start to the
/// end point) and the damage record's class mapping of the sim.
fn carrion_swarm(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    caster: ObjKey,
    level: &SpellLevel,
    x: F32,
    y: F32,
) {
    swath_cast(game, rt, caster, level, (x, y), SWARM_TARGS, SWATH_DAMAGE_TYPE, CARRION_SWARM);
}

/// `targs` of `AUcs`: ground, air, organic (`AbilityData.slk`); the relation `enemy` is part of the callback.
pub const SWARM_TARGS: u32 = 0x2 | 0x4 | 0x0400_0000;
/// The damage record class of the swath hits: `0x8000` = bit 15 (the sim's damage type numbering is the bit index).
pub const SWATH_DAMAGE_TYPE: u32 = 15;
/// `targs` of `AOsh` (Shockwave): ground, structure. Shockwave's missile class `CMissileShockwave` derives from
/// `CMissileCarrionSwarm` and overrides none of the tick, hit or setup code (only the code getter and four id getters,
/// RTTI-checked), so the orc lane starts it with [`swath_cast`] and the same level mapping (Area start width, DataA damage,
/// DataB cap, DataC distance, DataD final width, `Missilespeed` 1050).
pub const SHOCKWAVE_TARGS: u32 = 0x2 | 0x8;
/// Lifetime of the marker buff: `0.1 + 0.02` (`0x6F1E79E0`).
const SWATH_MARK_SECONDS: f32 = 0.12;

/// Starts a swath missile for `caster` (shared by Carrion Swarm and, for the orc lane, Shockwave): the level data are
/// Area (start width), DataA damage, DataB total cap, DataC distance, DataD final width.
#[allow(clippy::too_many_arguments)]
pub fn swath_cast(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    caster: ObjKey,
    level: &SpellLevel,
    point: (F32, F32),
    targs: u32,
    damage_type: u32,
    base: u32,
) {
    let id = {
        let mut gm = game.borrow_mut();
        if !gm.object_is_live_unit(caster) {
            return;
        }
        let c = (gm.unit(caster).x, gm.unit(caster).y);
        let facing = gm.unit(caster).facing;
        let (area, distance) = (level.area, level.data[2]);
        let launch = match gm.order_tables() {
            Some(t) => swath_launch(c, point, facing, area, distance, t),
            None => swath_launch_plain(c, point, facing, area, distance),
        };
        let speed = gm.spell_missile_speed(base).to_f32();
        let data = SwathData {
            start: launch.start,
            distance,
            width0: area,
            width1: level.data[3],
            damage: level.data[0],
            cap: level.data[1],
        };
        let st = gm.spells_hero_state_mut();
        st.next_swarm += 1;
        let id = st.next_swarm;
        st.swarms.insert(
            id,
            Swarm { caster, data, end: launch.end, speed, tick: 0, marks: BTreeMap::new(), targs, damage_type },
        );
        id
    };
    // The setup runs the first tick at once (at the start point); the arrival is a timer at distance / speed.
    swarm_tick(game, rt, id);
    let mut gm = game.borrow_mut();
    if !gm.spells_hero_state_mut().swarms.contains_key(&id) {
        return;
    }
    let sw = gm.spells_hero_state_mut().swarms[&id].clone();
    let flight = if sw.speed > 0.0 { sw.data.distance.to_f32() / sw.speed } else { 0.0 };
    gm.timers_start_hero(flight, TICK_SWARM_ARRIVE, sw.caster, id);
}

/// The plain `f32` launch geometry for hosts without the soft-float tables (UNSURE bits).
fn swath_launch_plain(caster: (F32, F32), target: (F32, F32), facing: F32, area: F32, distance: F32) -> SwathLaunch {
    let (cx, cy) = (caster.0.to_f32(), caster.1.to_f32());
    let (dx, dy) = (target.0.to_f32() - cx, target.1.to_f32() - cy);
    let (ux, uy) = if dx * dx + dy * dy >= 400.0 {
        let l = (dx * dx + dy * dy).sqrt();
        (dx / l, dy / l)
    } else {
        (facing.to_f32().cos(), facing.to_f32().sin())
    };
    let (a, d) = (area.to_f32(), distance.to_f32());
    let (sx, sy) = (cx + ux * a, cy + uy * a);
    SwathLaunch {
        start: (F32::from_bits(sx.to_bits()), F32::from_bits(sy.to_bits())),
        end: (F32::from_bits((sx + ux * d).to_bits()), F32::from_bits((sy + uy * d).to_bits())),
    }
}

/// A running swath missile.
#[derive(Debug, Clone)]
struct Swarm {
    caster: ObjKey,
    data: SwathData,
    end: (F32, F32),
    /// Missile speed in world units per second (0 = the missile arrives at once).
    speed: f32,
    /// Ticks run so far (the tick `k` happens `k * 0.1` s after the launch).
    tick: u32,
    /// Marker buffs: victim -> expiry in seconds since the launch.
    marks: BTreeMap<ObjKey, f32>,
    targs: u32,
    damage_type: u32,
}

impl Swarm {
    /// The missile position after `t` seconds of flight (UNSURE: linear at constant speed).
    fn position(&self, t: f32) -> (F32, F32) {
        let dist = self.data.distance.to_f32();
        let travelled = if self.speed > 0.0 { (self.speed * t).min(dist) } else { dist };
        let frac = if dist > 0.0 { travelled / dist } else { 0.0 };
        let (sx, sy) = (self.data.start.0.to_f32(), self.data.start.1.to_f32());
        let (ex, ey) = (self.end.0.to_f32(), self.end.1.to_f32());
        let (px, py) = (sx + (ex - sx) * frac, sy + (ey - sy) * frac);
        (F32::from_bits(px.to_bits()), F32::from_bits(py.to_bits()))
    }
}

/// One tick (`0x6F1EF6D0`) at tick index `swarm.tick`, then the re-arm. The time of tick `k` is `k * 0.1` s.
fn swarm_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let t = game.borrow_mut().spells_hero_state_mut().swarms.get(&id).map_or(0.0, |s| s.tick as f32 * 0.1);
    swarm_run(game, rt, id, t, false);
}

/// Runs one hit pass of swarm `id` at flight time `t` (the arrival pass uses the end point regardless of `t`).
fn swarm_run(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32, t: f32, arrival: bool) {
    let mut deaths = Vec::new();
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(mut sw) = gm.spells_hero_state_mut().swarms.get(&id).cloned() else {
            return;
        };
        caster = sw.caster;
        let pos = if arrival { sw.end } else { sw.position(t) };
        let radius = match gm.order_tables() {
            Some(tb) => swath_radius(&sw.data, pos, tb),
            None => {
                let (dx, dy) = (pos.0.to_f32() - sw.data.start.0.to_f32(), pos.1.to_f32() - sw.data.start.1.to_f32());
                let frac = if sw.data.distance.to_f32() != 0.0 { (dx * dx + dy * dy).sqrt() / sw.data.distance.to_f32() } else { 1.0 };
                let (w0, w1) = (sw.data.width0.to_f32(), sw.data.width1.to_f32());
                F32::from_bits((w0 + (w1 - w0) * frac).to_bits())
            }
        };
        let owner = if gm.object_is_live_unit(caster) { Some(gm.unit(caster).owner) } else { None };
        let mut list: Vec<ObjKey> = Vec::new();
        if let Some(owner) = owner {
            for k in circle_candidates(&gm, pos.0, pos.1, radius) {
                if list.len() >= SWATH_MAX_UNITS {
                    break;
                }
                if !gm.object_is_live_unit(k) || !gm.is_enemy(owner, gm.unit(k).owner) {
                    continue;
                }
                if crate::target_accept::predicate(gm.target_flag_word(k), sw.targs) != 0 {
                    continue;
                }
                if sw.marks.get(&k).is_some_and(|&expiry| expiry > t) {
                    // The marker is alive: refreshed, the unit is not hit again.
                    sw.marks.insert(k, t + SWATH_MARK_SECONDS);
                    continue;
                }
                list.push(k);
            }
        }
        if !list.is_empty() {
            let each = match gm.order_tables() {
                Some(tb) => swath_damage_each(&sw.data, list.len(), tb),
                None => {
                    let share = sw.data.cap.to_f32() / list.len() as f32;
                    if share > sw.data.damage.to_f32() { sw.data.damage } else { F32::from_bits(share.to_bits()) }
                }
            };
            let mut cap = sw.data.cap;
            for k in list {
                sw.marks.insert(k, t + SWATH_MARK_SECONDS);
                let before = gm.unit(k).life;
                gm.spells_hero_state_mut().swarm_hits.push((caster, k, each));
                if let Some(d) = gm.unit_damage_target(caster, k, each, false, false, 0, sw.damage_type) {
                    deaths.push(d);
                }
                let after = gm.unit(k).life;
                cap = swath_cap_after(cap, before, after);
            }
            sw.data.cap = cap;
        }
        let rearm = !arrival && swath_rearm(sw.data.cap);
        sw.tick += 1;
        gm.spells_hero_state_mut().swarms.insert(id, sw);
        if rearm {
            gm.timers_start_hero(f32::from_bits(SWATH_TICK_BITS), TICK_SWARM, caster, id);
        }
    }
    for d in deaths {
        dispatch_unit_death_by(game, rt, d, Some(caster));
    }
}

/// The arrival of the swath missile (`0x6F1F2A20`): one last tick at the end point when the cap is above 0.001, then
/// the missile is gone.
fn swarm_arrive(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let (t, last) = {
        let mut gm = game.borrow_mut();
        let Some(sw) = gm.spells_hero_state_mut().swarms.get(&id) else {
            return;
        };
        let flight = if sw.speed > 0.0 { sw.data.distance.to_f32() / sw.speed } else { 0.0 };
        (flight, gt_eps(sw.data.cap, F32::ZERO))
    };
    if last {
        swarm_run(game, rt, id, t, true);
    }
    game.borrow_mut().spells_hero_state_mut().swarms.remove(&id);
}

// ---------------------------------------------------------------------------------------------------------------
// Inferno (S37)

pub const INFERNO: u32 = code(b"AUin");

/// Cast-order ids of the S37 abilities (the original getters, `tools/analysis/ability_orders.py --verify`: AUin
/// 852224, AUdd 852221, AEsf 852183, AEtq 852184, AEme 852180).
pub const S37_ORDERS: &[(u32, u32)] = &[(852_224, INFERNO), (852_221, DEATH_AND_DECAY), (852_555, IMPALE), (852_225, FROST_ARMOR)];
/// The Infernal's timed-life buff id (`'BNin'`, the level's BuffID).
pub const BUFF_INFERNAL: u32 = code(b"BNin");
/// `targs` of `AUin`: ground, structure (+ debris, enemy, neutral): the relation is the enemy test.
pub const INFERNO_TARGS: u32 = 0x2 | 0x8;
/// The damage record class of the landing (`0x20000`, set in the landing function `0x6F142E60`): bit 17.
pub const INFERNO_DAMAGE_TYPE: u32 = 17;

/// The landing context of `CAbilityInferno` (`0x6F13BC60` -> `0x6F117B40`, CONFIRMED by Unicorn, 320 cases): the level
/// record fields the Infernal's buff `CBuffInfernal` keeps. Roles: Area = the circle of the landing, DataA = damage,
/// DataB = the Infernal's lifetime (a timed life), DataC = the impact delay (the buff arms `0xD01C1` with it), Dur /
/// HeroDur = the stun durations (a hero takes HeroDur, `0x6F1D2660`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InfernoCtx {
    pub area: F32,
    pub damage: F32,
    pub life: F32,
    pub delay: F32,
    pub dur: F32,
    pub hero_dur: F32,
    pub unit_id: u32,
}

pub fn inferno_ctx(level: &SpellLevel) -> InfernoCtx {
    InfernoCtx {
        area: level.area,
        damage: level.data[0],
        life: level.data[1],
        delay: level.data[2],
        dur: level.dur,
        hero_dur: level.hero_dur,
        unit_id: level.unit_id,
    }
}

/// A landing in progress.
#[derive(Debug, Clone, Copy)]
struct Inferno {
    caster: ObjKey,
    infernal: Option<ObjKey>,
    point: (F32, F32),
    ctx: InfernoCtx,
}

/// `CAbilityInferno` effect: creates the Infernal at the point (`0x6F13BB20`, with the timed life and the buff
/// `'BNin'`) and, after the impact delay, runs the landing: every enemy unit passing the ability's `targs` within
/// Area of the Infernal takes DataA (record class `0x20000`) and a stun of Dur (HeroDur for a hero) through the
/// stun buff. UNSURE: the unit is created at once (the original creates it in the effect, the fall is the buff's
/// progress bar), the debris query of the landing is not modelled, and the stun is the sim's generic stun buff.
fn inferno(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, caster: ObjKey, level: &SpellLevel, x: F32, y: F32) {
    let _ = rt;
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(caster) {
        return;
    }
    let ctx = inferno_ctx(level);
    let unit_type = if ctx.unit_id != 0 {
        Some(ctx.unit_id)
    } else {
        gm.spells_hero_state_mut().summon_unit.get(&INFERNO).copied()
    };
    let owner = gm.unit(caster).owner;
    let mut infernal = None;
    if let Some(ty) = unit_type {
        if let Some((u, _)) = crate::natives::unit::build(&mut gm, owner, ty, x, y, F32::ZERO, true) {
            gm.apply_timed_life(u, BUFF_INFERNAL, ctx.life);
            infernal = Some(u);
        }
    }
    let st = gm.spells_hero_state_mut();
    st.next_inferno += 1;
    let id = st.next_inferno;
    st.infernos.insert(id, Inferno { caster, infernal, point: (x, y), ctx });
    gm.timers_start_hero(ctx.delay.to_f32(), TICK_INFERNO, caster, id);
}

fn inferno_land(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let mut deaths = Vec::new();
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(inf) = gm.spells_hero_state_mut().infernos.remove(&id) else {
            return;
        };
        caster = inf.caster;
        let center = match inf.infernal {
            Some(u) if gm.object_is_live_unit(u) => (gm.unit(u).x, gm.unit(u).y),
            _ => inf.point,
        };
        let owner = gm.unit(caster).owner;
        let victims: Vec<ObjKey> = circle_candidates(&gm, center.0, center.1, inf.ctx.area)
            .into_iter()
            .filter(|&k| Some(k) != inf.infernal && gm.object_is_live_unit(k) && gm.is_enemy(owner, gm.unit(k).owner))
            .filter(|&k| crate::target_accept::predicate(gm.target_flag_word(k), INFERNO_TARGS) == 0)
            .collect();
        for k in victims {
            if let Some(d) = gm.unit_damage_target(caster, k, inf.ctx.damage, false, false, 0, INFERNO_DAMAGE_TYPE) {
                deaths.push(d);
                continue;
            }
            let hero = gm.unit(k).hero.is_some() || is_hero_id(gm.unit(k).type_id);
            let dur = if hero { inf.ctx.hero_dur } else { inf.ctx.dur };
            gm.add_spell_buff(k, crate::cast::BUFF_STUN, BuffEffect::Stun, dur);
        }
    }
    for d in deaths {
        dispatch_unit_death_by(game, rt, d, Some(caster));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Death and Decay (S37)

pub const DEATH_AND_DECAY: u32 = code(b"AUdd");
/// The victim buff `'BUdd'` (`CBuffDeathAndDecayAoe`).
pub const BUFF_DEATH_AND_DECAY: u32 = code(b"BUdd");
/// `targs` of `AUdd`: air, ground, structure, ward (the original's relation test is the range listener's, UNSURE:
/// the model hurts enemies only).
pub const DND_TARGS: u32 = 0x4 | 0x2 | 0x8 | 0x10;
/// The damage record class of a pulse (`0x4000000`, `0x6F098180`): bit 26.
pub const DND_DAMAGE_TYPE: u32 = 26;
/// The RNG stream of the first pulse's random delay (`0x6F09FBF0` calls `0x6F2853D0` with EDX = `0x16`).
const DND_STREAM: usize = 0x16;

/// What one pulse of a Death and Decay victim buff (`CBuffDeathAndDecayAoe` message handler `0x6F098180`, CONFIRMED
/// by Unicorn, 320 cases) damages: `DataA * max_life` through the soft multiply, class `0x4000000`, attacker = the
/// source unit of the buff, record flags 0. The pulse re-arms its timer with 1.0 s first (`0x6FAAE4F4`), delivers the
/// damage even when the amount is 0.0, and removes the buff afterwards when the victim's dead flag (`+0x5C & 0x100`)
/// is set.
pub fn dnd_pulse_amount(fraction: F32, max_life: F32) -> F32 {
    arith::mul(fraction, max_life)
}

/// A running Death and Decay.
#[derive(Debug, Clone)]
struct DeathAndDecay {
    caster: ObjKey,
    center: (F32, F32),
    area: F32,
    fraction: F32,
    /// Seconds the effect has left to run (Dur / HeroDur: the level's Dur, 35 s in the stock data).
    left: f32,
    /// Victims inside the area: unit -> pulse generation (a stale pulse timer is ignored).
    members: BTreeMap<ObjKey, u32>,
}

/// `CAbilityDeathAndDecay` effect (`0x6F0A98F0`): a channelled area effect `XUdd` at the point for the level's
/// Dur (the channel ends it early, [`dnd_stop`]). The model polls the area every 0.1 s (the original uses the effect's
/// range listener: UNSURE), gives each unit that enters the buff `'BUdd'` and a first pulse after a stream-22 draw
/// `U` (seconds, `0x6F09FBF0`), then pulses every 1.0 s while the unit stays inside. LOCAL channel model: to be
/// replaced by the shared channel lifecycle of S35.
fn death_and_decay(game: &RefCell<Game>, caster: ObjKey, level: &SpellLevel, x: F32, y: F32) {
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(caster) {
        return;
    }
    // The effect function removes the caster's earlier channel effects first (`0x6F0A98F0` looks up and removes three
    // buff kinds): a second Death and Decay of the same caster replaces the first.
    let old: Vec<u32> = gm.spells_hero_state_mut().dnds.iter().filter(|(_, d)| d.caster == caster).map(|(k, _)| *k).collect();
    for id in old {
        gm.spells_hero_state_mut().dnds.remove(&id);
    }
    let hero = gm.unit(caster).hero.is_some() || is_hero_id(gm.unit(caster).type_id);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO).to_f32();
    let st = gm.spells_hero_state_mut();
    st.next_dnd += 1;
    let id = st.next_dnd;
    st.dnds.insert(
        id,
        DeathAndDecay { caster, center: (x, y), area: level.area, fraction: level.data[0], left: dur, members: BTreeMap::new() },
    );
    gm.timers_start_hero(DND_POLL_SECONDS, TICK_DND, caster, id);
}

const DND_POLL_SECONDS: f32 = 0.1;

/// Stops the Death and Decay of `caster` (the channel ended). Hook for the shared channel lifecycle.
pub fn dnd_stop(game: &RefCell<Game>, caster: ObjKey) {
    let mut gm = game.borrow_mut();
    let ids: Vec<u32> = gm.spells_hero_state_mut().dnds.iter().filter(|(_, d)| d.caster == caster).map(|(k, _)| *k).collect();
    for id in ids {
        gm.spells_hero_state_mut().dnds.remove(&id);
    }
}

fn dnd_poll(game: &RefCell<Game>, id: u32) {
    let mut gm = game.borrow_mut();
    let Some(mut d) = gm.spells_hero_state_mut().dnds.get(&id).cloned() else {
        return;
    };
    d.left -= DND_POLL_SECONDS;
    if d.left <= 0.0 || !gm.object_is_live_unit(d.caster) {
        gm.spells_hero_state_mut().dnds.remove(&id);
        return;
    }
    let owner = gm.unit(d.caster).owner;
    let inside: Vec<ObjKey> = circle_candidates(&gm, d.center.0, d.center.1, d.area)
        .into_iter()
        .filter(|&k| gm.object_is_live_unit(k) && gm.is_enemy(owner, gm.unit(k).owner))
        .filter(|&k| crate::target_accept::predicate(gm.target_flag_word(k), DND_TARGS) == 0)
        .collect();
    d.members.retain(|k, _| inside.contains(k));
    for k in inside {
        if d.members.contains_key(&k) {
            continue;
        }
        let gen = d.members.values().max().copied().unwrap_or(0) + 1;
        d.members.insert(k, gen);
        // The first pulse comes after a random delay U in [0, 1) of stream 22; later pulses every 1.0 s.
        let delay = match gm.rng_parts() {
            Some((table, streams)) => {
                let u = streams.stream_mut(DND_STREAM).random_fraction(table);
                arith::add(F32::ZERO, arith::mul(F32::ONE, u)).to_f32()
            }
            None => 1.0,
        };
        let aux = (id << 16) | (k.0 & 0xFFFF);
        gm.timers_start_hero(delay.max(MIN_PERIOD_SECONDS), TICK_DND_PULSE, k, aux);
    }
    let caster = d.caster;
    gm.spells_hero_state_mut().dnds.insert(id, d);
    gm.timers_start_hero(DND_POLL_SECONDS, TICK_DND, caster, id);
}

fn dnd_pulse(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, victim: ObjKey, aux: u32) {
    let id = aux >> 16;
    let death;
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(d) = gm.spells_hero_state_mut().dnds.get(&id).cloned() else {
            return;
        };
        caster = d.caster;
        if !d.members.contains_key(&victim) || !gm.object_is_live_unit(victim) {
            return;
        }
        let amount = dnd_pulse_amount(d.fraction, gm.unit(victim).max_life);
        death = gm.unit_damage_target(caster, victim, amount, false, false, 0, DND_DAMAGE_TYPE);
        if death.is_none() {
            gm.timers_start_hero(1.0, TICK_DND_PULSE, victim, aux);
        }
    }
    if let Some(t) = death {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Impale (S37)

pub const IMPALE: u32 = code(b"AUim");

/// The spike line of `CAbilityImpale` (slot `0x418` = `0x6F144810`, CONFIRMED by Unicorn, 400 cases): with
/// `half = Area / 2` (an exponent decrement), `N = max(2, f2i((DataA + Area) / half))` spikes, a step of
/// `(DataA + Area - Area) / I2R(N - 1)` along the direction `(point - caster)` (the caster's facing when the squared distance is
/// below 400.0, `0x6FAAE598`), the first spike `caster + rescale(step, half)`, the interval `DataB / I2R(N - 1)` and the first timer
/// `DataB + 0.1`. Each spike (`0x6F144280`) queries the circle of radius `Area / 2` at the current spike point and then advances it by
/// the step; the timer re-arms with the interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpalePlan {
    pub count: i32,
    pub step: (F32, F32),
    pub first: (F32, F32),
    pub interval: F32,
    pub first_delay: F32,
}

/// Halves `x` by the exponent decrement of the original (`ecx - 0x1000000`, with the denormal / zero guard of the sign trick).
pub fn halve(x: F32) -> F32 {
    let b = x.to_bits();
    // `lea eax,[ecx-0x1000000]; xor eax,ecx; sar eax,31; not eax; add ecx,0xff800000; and eax,ecx`: the result is
    // `x - 0x0080_0000` unless the exponent field underflows (then 0).
    let t = b.wrapping_sub(0x0100_0000);
    let mask = !(((t ^ b) as i32) >> 31) as u32;
    F32::from_bits(mask & b.wrapping_add(0xFF80_0000))
}

pub fn impale_plan(
    caster: (F32, F32),
    target: (F32, F32),
    facing: F32,
    area: F32,
    data_a: F32,
    data_b: F32,
    t: &Tables,
) -> ImpalePlan {
    let half = halve(area);
    let total = arith::add(data_a, area);
    let n = softfloat::conv::f2i(arith::div(total, half, t)).max(2);
    let steps = softfloat::conv::i2r(n - 1);
    let step_len = arith::div(arith::sub(total, area), steps, t);
    let dy = arith::sub(target.1, caster.1);
    let dx = arith::sub(target.0, caster.0);
    let yy = arith::mul(dy, dy);
    let xx = arith::mul(dx, dx);
    let d2 = arith::add(xx, yy);
    let step = if d2.to_f32() >= 400.0 {
        rescale(dx, dy, step_len, t)
    } else {
        // The facing branch: the polar vector of length `step_len` is the step itself (no second rescale).
        let sn = softfloat::math::sin(facing, t);
        let cs = softfloat::math::cos(facing, t);
        let y = arith::mul(step_len, sn);
        let x = arith::mul(step_len, cs);
        (x, y)
    };
    let off = rescale(step.0, step.1, half, t);
    let first = (arith::add(caster.0, off.0), arith::add(caster.1, off.1));
    let interval = arith::div(data_b, softfloat::conv::i2r(n - 1), t);
    let first_delay = arith::add(data_b, F32::from_bits(0x3DCC_CCCD));
    ImpalePlan { count: n, step, first, interval, first_delay }
}

/// The plain `f32` version of [`impale_plan`] for hosts without the soft-float tables (UNSURE bits).
fn impale_plan_plain(caster: (F32, F32), target: (F32, F32), facing: F32, area: F32, data_a: F32, data_b: F32) -> ImpalePlan {
    let (a, b, ar) = (data_a.to_f32(), data_b.to_f32(), area.to_f32());
    let half = ar / 2.0;
    let n = (((a + ar) / half) as i32).max(2);
    let step_len = a / (n - 1) as f32;
    let (cx, cy) = (caster.0.to_f32(), caster.1.to_f32());
    let (dx, dy) = (target.0.to_f32() - cx, target.1.to_f32() - cy);
    let d2 = dx * dx + dy * dy;
    let (ux, uy) = if d2 >= 400.0 { (dx / d2.sqrt(), dy / d2.sqrt()) } else { (facing.to_f32().cos(), facing.to_f32().sin()) };
    let to = |v: f32| F32::from_bits(v.to_bits());
    ImpalePlan {
        count: n,
        step: (to(ux * step_len), to(uy * step_len)),
        first: (to(cx + ux * half), to(cy + uy * half)),
        interval: to(b / (n - 1) as f32),
        first_delay: to(b + 0.1),
    }
}

/// `targs` of `AUim`: ground, organic (`ground,enemy,neutral,organic`).
pub const IMPALE_TARGS: u32 = 0x2 | 0x0400_0000;
/// UNSURE: the damage type of the Impale damage (the sim's generic spell class).
const IMPALE_DAMAGE_TYPE: u32 = 9;

/// A running Impale.
#[derive(Debug, Clone)]
struct Impale {
    caster: ObjKey,
    next: (F32, F32),
    step: (F32, F32),
    left: i32,
    interval: F32,
    radius: F32,
    damage: F32,
    dur: F32,
    hero_dur: F32,
    hit: Vec<ObjKey>,
}

/// `CAbilityImpale` effect (slot `0x418`): plans the spike line ([`impale_plan`]) and runs the spikes from a timer: the first
/// after `DataB + 0.1` s, then every `DataB / (N - 1)` s. Each spike queries the circle of radius `Area / 2` around the spike
/// point (`0x6F144280`) and the callback `0x6F142E20` hands every accepted enemy to `0x6F13BA30`, which gives the unit the
/// Impale buff `'BUim'` with `HeroDur` (hero) or `Dur` as its duration (stun and lift). UNSURE: that the damage `DataC`
/// is dealt at the spike (the buff's own handlers carry it in the original), the damage class, that a unit is hit by one spike
/// only, and the stun being the sim's generic stun.
fn impale(game: &RefCell<Game>, caster: ObjKey, level: &SpellLevel, x: F32, y: F32) {
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(caster) {
        return;
    }
    let (cx, cy, facing) = (gm.unit(caster).x, gm.unit(caster).y, gm.unit(caster).facing);
    let plan = match gm.order_tables() {
        Some(t) => impale_plan((cx, cy), (x, y), facing, level.area, level.data[0], level.data[1], t),
        None => impale_plan_plain((cx, cy), (x, y), facing, level.area, level.data[0], level.data[1]),
    };
    let st = gm.spells_hero_state_mut();
    st.next_impale += 1;
    let id = st.next_impale;
    st.impales.insert(
        id,
        Impale {
            caster,
            next: plan.first,
            step: plan.step,
            left: plan.count,
            interval: plan.interval,
            radius: halve(level.area),
            damage: level.data[2],
            dur: level.dur,
            hero_dur: level.hero_dur,
            hit: Vec::new(),
        },
    );
    gm.timers_start_hero(plan.first_delay.to_f32(), TICK_IMPALE, caster, id);
}

fn impale_spike(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let mut deaths = Vec::new();
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(mut im) = gm.spells_hero_state_mut().impales.get(&id).cloned() else {
            return;
        };
        caster = im.caster;
        if !gm.object_is_live_unit(caster) {
            gm.spells_hero_state_mut().impales.remove(&id);
            return;
        }
        let owner = gm.unit(caster).owner;
        let victims: Vec<ObjKey> = circle_candidates(&gm, im.next.0, im.next.1, im.radius)
            .into_iter()
            .filter(|&k| gm.object_is_live_unit(k) && gm.is_enemy(owner, gm.unit(k).owner) && !im.hit.contains(&k))
            .filter(|&k| crate::target_accept::predicate(gm.target_flag_word(k), IMPALE_TARGS) == 0)
            .collect();
        for k in victims {
            im.hit.push(k);
            if let Some(d) = gm.unit_damage_target(caster, k, im.damage, false, false, 0, IMPALE_DAMAGE_TYPE) {
                deaths.push(d);
                continue;
            }
            let hero = gm.unit(k).hero.is_some() || is_hero_id(gm.unit(k).type_id);
            let dur = crate::cast::effect_duration(&SpellLevel { dur: im.dur, hero_dur: im.hero_dur, ..SpellLevel::default() }, hero, F32::ZERO);
            gm.add_spell_buff(k, crate::cast::BUFF_STUN, BuffEffect::Stun, dur);
        }
        im.left -= 1;
        im.next = (arith::add(im.next.0, im.step.0), arith::add(im.next.1, im.step.1));
        if im.left > 0 {
            gm.timers_start_hero(im.interval.to_f32().max(MIN_PERIOD_SECONDS), TICK_IMPALE, caster, id);
            gm.spells_hero_state_mut().impales.insert(id, im);
        } else {
            gm.spells_hero_state_mut().impales.remove(&id);
        }
    }
    for d in deaths {
        dispatch_unit_death_by(game, rt, d, Some(caster));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Frost Armor, Thorns Aura, Spiked Carapace: retaliation against melee attackers (S37)

pub const FROST_ARMOR: u32 = code(b"AUfa");
pub const FROST_ARMOR_AUTO: u32 = code(b"AUfu");
pub const BUFF_FROST_ARMOR: u32 = code(b"BUfa");
pub const THORNS_AURA: u32 = code(b"AEah");
pub const SPIKED_CARAPACE: u32 = code(b"AUts");
/// The sim's damage type of reflected damage (class `0x100000`, the record class of `0x6F1AD4E0`): bit 20.
const THORNS_DAMAGE_TYPE: u32 = 20;

/// The reflected damage of a Thorns buff (`CBuffAuraThorns` event handler `0x6F1AD4E0`, slot 63; gates and arithmetic read
/// from the machine code, LIKELY): `strength * (amount - reduced)` with the soft multiply, where `amount` is the damage
/// record's amount (`+0x10`) and `reduced` its `+0x18` (0.0 at creation, `0x6F2556E0`; the writer of that field was not found,
/// the model takes 0.0), raised to 1.0 when it is below 1.0 (`0x6F00C110`, `lt(amount, 1.0)`). Gates in the original order:
/// the record's ignore flag (bit 0, "ranged"), a class other than `0x10` (a basic attack), an allied attacker (`0x6F285AC0`),
/// an invulnerable attacker (slot `0x11C`), a blocked attacker (`0x6F28D750`); an illusion owner reflects nothing but still
/// plays the effect.
pub fn thorns_reflect(strength: F32, amount: F32, reduced: F32) -> F32 {
    let d = arith::sub(amount, reduced);
    let v = arith::mul(strength, d);
    if arith::lt(v, F32::ONE) {
        F32::ONE
    } else {
        v
    }
}

/// A Frost Armor on a unit: the armor it added, the chill the attackers get and when it ends.
#[derive(Debug, Clone, Copy)]
struct FrostArmor {
    unit: ObjKey,
    armor: F32,
    /// The ability level the chill takes its durations from (Dur / HeroDur, the Misc strengths are the Frost Nova's).
    chill: SpellLevel,
}

/// `CAbilityFrostArmor` effect (unit target, `0x6F0BA520`): the buff creator receives `(info, DataA, DataB, Dur or HeroDur)`
/// (CONFIRMED by a Unicorn run of the effect): the buff `'BUfa'` lasts DataA (45 s: the buff creator's duration argument),
/// adds DataB armor (the strength cell, LIKELY) and chills melee attackers for Dur / HeroDur (`CBuffFrostArmor`'s event
/// handler `0x6F0BA3F0` applies the Frost Nova buff `'Bfro'`, id getter `0x6F1998D0`, to a non-invulnerable attacker of a
/// basic attack). UNSURE: the sign and role of the armor cell, the autocast variant `AUfu`.
fn frost_armor(game: &RefCell<Game>, caster: ObjKey, level: &SpellLevel, target: ObjKey) {
    let _ = caster;
    let mut gm = game.borrow_mut();
    if !gm.object_is_live_unit(target) {
        return;
    }
    let serial = gm.add_spell_buff(target, BUFF_FROST_ARMOR, BuffEffect::Marker, level.data[0]);
    if serial == 0 {
        return;
    }
    // A second application replaces the first (`add_spell_buff` drops the old buff): take the old armor back first.
    let old: Vec<u32> = gm.spells_hero_state_mut().frost_armors.iter().filter(|(_, f)| f.unit == target).map(|(s, _)| *s).collect();
    for s in old {
        if let Some(f) = gm.spells_hero_state_mut().frost_armors.remove(&s) {
            gm.add_armor(f.unit, arith::neg(f.armor));
        }
    }
    gm.add_armor(target, level.data[1]);
    gm.spells_hero_state_mut().frost_armors.insert(serial, FrostArmor { unit: target, armor: level.data[1], chill: *level });
    gm.timers_start_hero(level.data[0].to_f32().max(MIN_PERIOD_SECONDS), TICK_FROST_ARMOR_END, target, serial);
}

fn frost_armor_end(game: &RefCell<Game>, aux: u32) {
    let mut gm = game.borrow_mut();
    if let Some(f) = gm.spells_hero_state_mut().frost_armors.remove(&aux) {
        gm.add_armor(f.unit, arith::neg(f.armor));
    }
}

impl Game {
    /// What a basic attack of `attacker` that hit `victim` for `damage` triggers on the victim's side: the chill of a Frost
    /// Armor, the reflection of Thorns Aura and Spiked Carapace. Only melee attackers (no missile) that are enemies of the
    /// victim and not invulnerable trigger them (the ranged exclusion is the damage record's ignore flag, LIKELY). Returns
    /// the units that died.
    pub(crate) fn apply_retaliation(&mut self, attacker: ObjKey, victim: ObjKey, damage: F32) -> Vec<ObjKey> {
        let mut died = Vec::new();
        if !self.object_is_live_unit(attacker) || !self.object_is_live_unit(victim) {
            return died;
        }
        let a_ty = self.unit(attacker).type_id;
        if self.attack_type(a_ty).missile_speed.is_some() {
            return died;
        }
        let (a_owner, v_owner) = (self.unit(attacker).owner, self.unit(victim).owner);
        if !self.is_enemy(v_owner, a_owner) || self.unit_is_invulnerable(attacker) {
            return died;
        }
        // Frost Armor: the chill (a live buff is a record).
        let chills: Vec<SpellLevel> = self
            .spells_hero_state_mut()
            .frost_armors
            .values()
            .filter(|f| f.unit == victim)
            .map(|f| f.chill)
            .collect();
        for level in chills {
            apply_frost_slow(self, attacker, &level);
        }
        // Thorns: the strongest of the victim's own Spiked Carapace and the Thorns Auras of friends within Area.
        let mut strength: Option<F32> = None;
        let mut consider = |s: F32| {
            if strength.is_none_or(|cur| arith::lt(cur, s)) {
                strength = Some(s);
            }
        };
        if let Some(l) = self.ability_level_record(victim, SPIKED_CARAPACE) {
            consider(l.data[0]);
        }
        for k in (0..self.object_count()).map(|i| ObjKey(i as u32)) {
            if !self.object_is_live_unit(k) || self.is_enemy(v_owner, self.unit(k).owner) {
                continue;
            }
            if let Some(l) = self.ability_level_record(k, THORNS_AURA) {
                let (vx, vy, kx, ky) = (self.unit(victim).x, self.unit(victim).y, self.unit(k).x, self.unit(k).y);
                if within(vx, vy, kx, ky, l.area) {
                    consider(l.data[0]);
                }
            }
        }
        if let Some(s) = strength {
            if !self.unit(victim).illusion {
                let reflect = thorns_reflect(s, damage, F32::ZERO);
                if let Some(d) = self.unit_damage_target(victim, attacker, reflect, false, false, 0, THORNS_DAMAGE_TYPE) {
                    died.push(d);
                }
            }
        }
        died
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
            // S37 (CONFIRMED amounts, UNSURE timer reading): the creation call applies `m * 0 - m * 1.0` = -DataB to the
            // move factor and `-DataC` to the attack factor; DataC is absent (0) in the stock rows, so only the movement
            // slows. A 1.0 s timer then follows the decay factor ([`strike_decay_factor`]).
            let (m, a, d) = (level.data[1], level.data[2], level.data[3]);
            let first = strike_slow_amounts(F32::ZERO, m, a, F32::ONE, 0);
            let (fm, fa) = (arith::add(F32::ONE, first.move_amount), arith::add(F32::ONE, arith::add(first.attack_prev, first.attack_new)));
            let effect = BuffEffect::Slow { move_delta: arith::sub(fm, F32::ONE), attack_delta: arith::sub(fa, F32::ONE) };
            let serial = gm.add_spell_buff(st.target, BUFF_SHADOW_STRIKE, effect, dur);
            if serial != 0 {
                let period = level.cast.to_f32().max(MIN_PERIOD_SECONDS);
                gm.spells_hero_state_mut().strike_dots.insert(serial, (caster, level.data[0], period));
                gm.timers_start_hero(period, TICK_STRIKE_DOT, st.target, serial);
                gm.spells_hero_state_mut().strike_slows.insert(serial, StrikeSlow { m, a, d, dur, ticks: 0, prev: F32::ONE, fm, fa });
                gm.timers_start_hero(1.0, TICK_STRIKE_SLOW, st.target, serial);
            }
        }
    }
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

/// The 1.0 s update of the Shadow Strike slow (`0xD01C3`, `0x6F1D53B0`): the factor `f` (clamped to >= 0.0), then the
/// amounts of [`strike_slow_amounts`] added to the move and attack factors; the buff stores `f` as the new `prev`.
/// UNSURE: the timer value `T` of [`strike_decay_factor`] is taken as the elapsed time, which makes the slow start at
/// DataB and decay to 0 over the duration (the machine code's sign convention for `T` was not run).
fn strike_slow_tick(game: &RefCell<Game>, unit: ObjKey, serial: u32) {
    let mut gm = game.borrow_mut();
    let Some(mut sl) = gm.spells_hero_state_mut().strike_slows.get(&serial).copied() else {
        return;
    };
    if !gm.spell_buff_alive(unit, serial) {
        gm.spells_hero_state_mut().strike_slows.remove(&serial);
        return;
    }
    sl.ticks += 1;
    let elapsed = F32::from_bits((sl.ticks as f32).to_bits());
    let mut f = match gm.order_tables() {
        Some(t) => strike_decay_factor(true, sl.dur, elapsed, sl.d, t),
        None => {
            let (dur, d) = (sl.dur.to_f32(), sl.d.to_f32());
            let v = ((elapsed.to_f32() - dur).abs().powf(d)) / dur.powf(d);
            F32::from_bits(v.to_bits())
        }
    };
    if !(0.0 <= f.to_f32()) {
        f = F32::ZERO;
    }
    let amounts = strike_slow_amounts(sl.prev, sl.m, sl.a, f, 0);
    sl.fm = arith::add(sl.fm, amounts.move_amount);
    sl.fa = arith::add(arith::add(sl.fa, amounts.attack_prev), amounts.attack_new);
    sl.prev = f;
    gm.set_spell_buff_slow(serial, arith::sub(sl.fm, F32::ONE), arith::sub(sl.fa, F32::ONE));
    gm.spells_hero_state_mut().strike_slows.insert(serial, sl);
    gm.timers_start_hero(1.0, TICK_STRIKE_SLOW, unit, serial);
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

/// The Shadow Strike slow factor `f` (`CBuffShadowStrike` slot `0x390` = `0x6F1DD190`, CONFIRMED by Unicorn, 400
/// cases): `0.0` while the buff flag `0x80` is clear or when `Dur` equals 0.0 within 0.001; else
/// `pow(|T - Dur|, DataD) / pow(Dur, DataD)` (soft `pow` and divide) where `T` is the buff timer's slot `0x1C`
/// value, snapped to `0.0` when its magnitude is below 0.001 (`0x6F1D2A10`). The handler of the 1.0 s timer
/// (`0xD01C3`, `0x6F1D53B0`) clamps a negative or NaN `f` to 0.0.
pub fn strike_decay_factor(applied: bool, dur: F32, t_value: F32, exponent: F32, tables: &Tables) -> F32 {
    if !applied || arith::approx_eq(dur, F32::ZERO) {
        return F32::ZERO;
    }
    let num = softfloat::math::pow(F32::from_bits(arith::sub(t_value, dur).to_bits() & 0x7FFF_FFFF), exponent, tables);
    let den = softfloat::math::pow(dur, exponent, tables);
    let f = arith::div(num, den, tables);
    let mag = F32::from_bits(arith::sub(f, F32::ZERO).to_bits() & 0x7FFF_FFFF);
    if f32::from_bits(0x3A83_126F) <= mag.to_f32() {
        f
    } else {
        F32::ZERO
    }
}

/// What `CBuffShadowStrike` slot `0x37C` (`0x6F11BBF0`) hands to the slow setters (CONFIRMED by Unicorn, 400 cases):
/// the move amount `m * prev - m * f` (soft multiplies and subtract) for `0x6F202FF0`, and for the attack ability
/// `0x6F0C9090` two calls `a * prev` and `(-a) * f`, both with the flag `(buff_flags >> 26) & 1`. `prev` is the value
/// of slot `0x384` (`+0x144`: the factor applied last, 0.0 at the creation call and 1.0 afterwards) and `m`, `a` are
/// DataB / DataC. The setters ADD their amount to a speed factor that starts at 1.0, so the net effect is a slow of
/// `m * f` that starts at `m` (creation: `prev = 0`, `f = 1`) and follows `f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrikeSlowAmounts {
    pub move_amount: F32,
    pub attack_prev: F32,
    pub attack_new: F32,
    pub attack_flag: u32,
}

pub fn strike_slow_amounts(prev: F32, m: F32, a: F32, f: F32, buff_flags: u32) -> StrikeSlowAmounts {
    StrikeSlowAmounts {
        move_amount: arith::sub(arith::mul(m, prev), arith::mul(m, f)),
        attack_prev: arith::mul(a, prev),
        attack_new: arith::mul(F32::from_bits(a.to_bits() ^ 0x8000_0000), f),
        attack_flag: (buff_flags >> 26) & 1,
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
        Immolation { serial, damage: level.data[0], drain: level.data[1], area: level.area, dur: level.dur },
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
            // The per-victim callback (`0x6F13B2A0`) gives the victim the marker `'BEia'` (refreshing a live one) with
            // the duration `Dur - 0.01` (CBuffAoe slot `0x358`, [`aoe_marker_duration`]).
            gm.add_spell_buff(k, BUFF_IMMOLATION_AOE, BuffEffect::Marker, aoe_marker_duration(run.dur));
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

/// The duration `CBuffAoe` gives its buff (slots `0x358` apply / `0x350` refresh, CONFIRMED by Unicorn, 320 cases):
/// `value - 0.01` (soft subtract, `0x6FAAE480`) when `0.0 < value` by the x87 compare (NaN and non-positive give `0.0`).
pub fn aoe_marker_duration(value: F32) -> F32 {
    if 0.0 < value.to_f32() {
        arith::sub(value, F32::from_bits(0x3C23_D70A))
    } else {
        F32::ZERO
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
        TICK_SWARM => swarm_tick(game, rt, aux),
        TICK_STRIKE_SLOW => strike_slow_tick(game, unit, aux),
        TICK_INFERNO => inferno_land(game, rt, aux),
        TICK_DND => dnd_poll(game, aux),
        TICK_DND_PULSE => dnd_pulse(game, rt, unit, aux),
        TICK_IMPALE => impale_spike(game, rt, aux),
        TICK_FROST_ARMOR_END => frost_armor_end(game, aux),
        TICK_SWARM_ARRIVE => swarm_arrive(game, rt, aux),
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
