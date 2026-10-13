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

use softfloat::{arith, F32};

use crate::abilities::code;
use crate::cast::{BuffEffect, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey, Object};
use crate::natives::group::circle_candidates;
use crate::natives::{dispatch_unit_death_by, set_life, ScriptRuntime};

pub const DEATH_COIL: u32 = code(b"AUdc");
pub const FROST_NOVA: u32 = code(b"AUfn");
pub const ENTANGLING_ROOTS: u32 = code(b"AEer");
pub const FORCE_OF_NATURE: u32 = code(b"AEfn");

/// Buff ids (UNSURE: the level's BuffID list is not in the model).
pub const BUFF_FROST_NOVA: u32 = code(b"Bfro");
pub const BUFF_ROOTS: u32 = code(b"BEer");
/// The treants' timed life buff (`BEfn` is one of the 7 special timed life ids, abilities.md).
pub const BUFF_TREANT_LIFE: u32 = code(b"BEfn");

/// Damage class bits (CONFIRMED values: Death Coil's record `0x40000`, Frost Nova's slot `0x42C` returns `0x200`);
/// `unit_damage_target` takes the bit index.
const DEATH_COIL_DAMAGE_TYPE: u32 = 18;
const FROST_NOVA_DAMAGE_TYPE: u32 = 9;
/// UNSURE: the class of the roots' damage.
const ROOTS_DAMAGE_TYPE: u32 = 9;

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

/// Spell state kept by the module (owned by `SpellSystem`).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Unit type created by Force of Nature, per base ability (the level's UnitID, which `SpellLevel` lacks).
    summon_unit: BTreeMap<u32, u32>,
    /// Running root damage: buff serial -> (caster, damage per tick).
    roots: BTreeMap<u32, (ObjKey, F32)>,
    /// Treants created, `(caster, unit)` in creation order (for tests and the host).
    pub treants: Vec<(ObjKey, ObjKey)>,
}

impl State {
    pub fn set_summon_unit(&mut self, base: u32, unit_type: u32) {
        self.summon_unit.insert(base, unit_type);
    }
}

/// Timer kinds of [`crate::cast::SpellMsg::HeroSpell`].
pub const TICK_ROOTS: u8 = 1;

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
        (DEATH_COIL | FROST_NOVA | ENTANGLING_ROOTS | FORCE_OF_NATURE, _) => {}
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
        // Primary target: DataB, then the slow (0x6F1606C0).
        let invulnerable = gm.unit_is_invulnerable(target);
        if !invulnerable {
            if let Some(d) = gm.unit_damage_target(caster, target, level.data[1], false, false, 0, FROST_NOVA_DAMAGE_TYPE) {
                deaths.push(d);
            }
            apply_frost_slow(&mut gm, target, level);
        }
        // Area: every other live enemy of the caster within Area of the target (0x6F1607B0).
        let victims: Vec<ObjKey> = circle_candidates(&gm, tx, ty, level.area)
            .into_iter()
            .filter(|&k| k != target && gm.object_is_live_unit(k) && gm.is_enemy(c_owner, gm.unit(k).owner))
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
    }
    for t in deaths {
        dispatch_unit_death_by(game, rt, t, Some(caster));
    }
}

fn apply_frost_slow(gm: &mut Game, unit: ObjKey, level: &SpellLevel) {
    if !gm.object_is_live_unit(unit) {
        return;
    }
    let hero = gm.unit(unit).hero.is_some() || is_hero_id(gm.unit(unit).type_id);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
    let effect = BuffEffect::Slow {
        move_delta: arith::neg(level.data[2]),
        attack_delta: arith::neg(level.data[3]),
    };
    gm.add_spell_buff(unit, BUFF_FROST_NOVA, effect, dur);
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
    gm.timers_start_hero(ROOT_TICK_SECONDS, TICK_ROOTS, target, serial);
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
    if kind != TICK_ROOTS {
        return;
    }
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
        if gm.object_is_live_unit(unit) {
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

    /// The treants Force of Nature created, `(caster, unit)` in creation order.
    pub fn treants(&self) -> &[(ObjKey, ObjKey)] {
        &self.spells.hero.treants
    }

    fn object_is_live_unit(&self, k: ObjKey) -> bool {
        matches!(self.object(k), Object::Unit(u) if !u.dead && !u.removed)
    }
}
