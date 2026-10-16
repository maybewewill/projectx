//! The smaller neutral abilities of S50: Battle Roar, Mind Rot, Demolish, Permanent Immolation, Rain of Chaos, Finger of
//! Death, Watery Minion, Parasite, Channel, Dark Portal. Spec: `docs/spec/abilities-neutral.md` section 8. Field roles
//! are the editor's labels (`WESTRING_AEVAL_*`); structure notes say where they come from.

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32};

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::cast::{CastError, CastRec, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::neutral_heroes2::start_timer;
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// Battle Roar (`CAbilityBattleRoar < CAbilitySimpleSpell`, buff `BNbr`).
pub const BATTLE_ROAR: u32 = code(b"ANbr");
pub const BUFF_BATTLE_ROAR: u32 = code(b"BNbr");
/// Mind Rot (`CAbilityMindRot < CAbilitySimpleSpell`, buff `BNmr`).
pub const MIND_ROT: u32 = code(b"ANmr");
pub const BUFF_MIND_ROT: u32 = code(b"BNmr");
/// Demolish (`CAbilityDemolish < CAbilityWarStomp`; the upgraded rows `ANd1..3` are aliases).
pub const DEMOLISH: u32 = code(b"ANde");
/// Permanent Immolation (`CAbilityPermImmolation < CAbilityPassive`; `Apmf`, `Apig` are aliases).
pub const PERM_IMMOLATION: u32 = code(b"ANpi");
pub const BUFF_PERM_IMMOLATION: u32 = code(b"BNpi");
/// Rain of Chaos (`CAbilityRainOfChaos < CAbilitySimpleSpell`).
pub const RAIN_OF_CHAOS: u32 = code(b"ANrc");
/// Finger of Death (`CAbilityFingerOfDeath < CAbilitySimpleSpell`).
pub const FINGER_OF_DEATH: u32 = code(b"ANfd");
/// Watery Minion (`CAbilityWateryMinion < CAbilityWaterElemental`, buff `BNwm`).
pub const WATERY_MINION: u32 = code(b"ANwm");
pub const BUFF_WATERY_MINION: u32 = code(b"BNwm");
/// Parasite (`CAbilityParasite < CAbilityAutoTargetSpell`, buffs `BNpa`, `BNpm`).
pub const PARASITE: u32 = code(b"ANpa");
pub const BUFF_PARASITE: u32 = code(b"BNpa");
pub const BUFF_PARASITE_MINION: u32 = code(b"BNpm");
/// Channel (`CAbilityChannel < CAbilitySimpleSpell`, the editor's trigger ability).
pub const CHANNEL: u32 = code(b"ANcl");
/// Dark Portal (`CAbilityDarkPortal < CAbilitySimpleSpell`).
pub const DARK_PORTAL: u32 = code(b"ANdp");
/// Inferno (neutral Pit Lord version, same data layout as the Dreadlord's `AUin`).
pub const NEUTRAL_INFERNO: u32 = code(b"ANin");

pub const ORDER_BATTLE_ROAR: u32 = 852_599;
pub const ORDER_MIND_ROT: u32 = 852_565;
pub const ORDER_RAIN_OF_CHAOS: u32 = 852_237;
pub const ORDER_FINGER_OF_DEATH: u32 = 852_230;
pub const ORDER_WATERY_MINION: u32 = 852_598;
pub const ORDER_PARASITE: u32 = 852_601;
pub const ORDER_DARK_PORTAL: u32 = 852_229;
/// The order of the Channel ability when its order string is empty or unknown (fallback of the class init `0x6F1AC760`,
/// CONFIRMED in the atlas notes).
pub const ORDER_CHANNEL: u32 = 852_600;

/// Timer kinds: the mana drain tick of Mind Rot, a Rain of Chaos infernal, a delayed Finger of Death.
pub const KIND_DRAIN: u8 = 0x69;
pub const KIND_CHAOS: u8 = 0x6A;
pub const KIND_FINGER: u8 = 0x6B;
pub const KIND_PERM_IMMOLATION: u8 = 0x6C;

/// The damage type of the damage over time and of the direct spell damage of this module (the neutral spell type).
const MISC_DAMAGE_TYPE: u32 = 15;
const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);

#[derive(Debug, Clone, Copy)]
struct Drain {
    target: ObjKey,
    per_second: F32,
    serial: u32,
}

#[derive(Debug, Clone, Copy)]
struct Chaos {
    caster: ObjKey,
    x: F32,
    y: F32,
    level: SpellLevel,
    inferno: SpellLevel,
    left: i32,
}

#[derive(Debug, Clone, Copy)]
struct Finger {
    caster: ObjKey,
    target: ObjKey,
    damage: F32,
}

/// A parasite mark on a target: the cast numbers the death needs.
#[derive(Debug, Clone, Copy)]
struct Infest {
    source: ObjKey,
    level: SpellLevel,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    drains: BTreeMap<u32, Drain>,
    chaos: BTreeMap<u32, Chaos>,
    fingers: BTreeMap<u32, Finger>,
    infested: BTreeMap<ObjKey, Infest>,
    /// Running timers of Permanent Immolation: `unit -> timer`.
    immolating: BTreeMap<ObjKey, crate::timer::TimerId>,
    /// Wander (`Awan`) state: `unit -> running timer` (see `wander.rs`).
    pub(crate) wander: BTreeMap<ObjKey, Option<crate::timer::TimerId>>,
}

pub(crate) fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (id, base) in [
        (ORDER_BATTLE_ROAR, BATTLE_ROAR),
        (ORDER_MIND_ROT, MIND_ROT),
        (ORDER_RAIN_OF_CHAOS, RAIN_OF_CHAOS),
        (ORDER_FINGER_OF_DEATH, FINGER_OF_DEATH),
        (ORDER_WATERY_MINION, WATERY_MINION),
        (ORDER_PARASITE, PARASITE),
        (ORDER_DARK_PORTAL, DARK_PORTAL),
        (ORDER_CHANNEL, CHANNEL),
    ] {
        ids.insert(id, base);
    }
}

pub(crate) fn takes_no_target(base: u32) -> bool {
    matches!(base, BATTLE_ROAR | WATERY_MINION | CHANNEL)
}

pub(crate) fn is_misc_spell(base: u32) -> bool {
    matches!(base, BATTLE_ROAR | MIND_ROT | RAIN_OF_CHAOS | FINGER_OF_DEATH | WATERY_MINION | PARASITE | CHANNEL | DARK_PORTAL)
}

/// The channel time of Channel: `DataA` ("Follow Through Time").
pub(crate) fn channel_time(base: u32, level: &SpellLevel) -> Option<F32> {
    (base == CHANNEL).then_some(level.data[0])
}

pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    let word = match base {
        MIND_ROT => targs_word("air,ground,enemy,neutral"),
        PARASITE => targs_word("air,ground,enemy,organic,neutral,nonhero"),
        FINGER_OF_DEATH => targs_word("air,ground,structure,nonhero,ancient,nonancient"),
        _ => return None,
    };
    if gm.unit(target).removed || !targs_ok(gm, caster, target, word) {
        return Some(CastError::BadTarget);
    }
    if base == FINGER_OF_DEATH && gm.unit(target).owner == gm.unit(caster).owner {
        return Some(CastError::BadTarget);
    }
    None
}

pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        BATTLE_ROAR => battle_roar(game, rec),
        MIND_ROT => {
            if let CastTarget::Unit(t) = rec.target {
                mind_rot(game, rec, t);
            }
        }
        RAIN_OF_CHAOS => rain_of_chaos(game, rec),
        FINGER_OF_DEATH => {
            if let CastTarget::Unit(t) = rec.target {
                finger_of_death(game, rec, t);
            }
        }
        WATERY_MINION => crate::human_heroes::summon(game, rec, BUFF_WATERY_MINION),
        PARASITE => {
            if let CastTarget::Unit(t) = rec.target {
                parasite(game, rec, t);
            }
        }
        DARK_PORTAL => dark_portal(game, rec),
        // Channel has no effect of its own: the events of the cast and the channel time are the ability.
        CHANNEL => {}
        _ => return false,
    }
    let _ = rt;
    true
}

pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, aux: u32) {
    match kind {
        KIND_DRAIN => drain_tick(game, aux),
        KIND_CHAOS => chaos_tick(game, rt, aux),
        KIND_FINGER => finger_hit(game, rt, aux),
        KIND_PERM_IMMOLATION => immolation_tick(game, rt, unit),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// Battle Roar (`ANbr`, order 852599, immediate; label `NBR1` "Damage Increase"; the field set is Roar's `ROA2..7`): every
/// friendly ground or air unit within `Area` of the caster (the caster included, `targs` `friend,self`) gets `BNbr` for
/// `Dur` / `HeroDur` and `DataA` more damage per attack. The flat reading of `DataA` (10, 20, 30, 40 over the four levels)
/// is UNSURE: Roar's own `DataA` is a fraction.
fn battle_roar(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (cx, cy, owner) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner)
    };
    let preds = BuffPredicates { positive: true, magic: true, dispellable: true, ..buff_predicates(BUFF_BATTLE_ROAR) };
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        let u = gm.unit(k);
        if !gm.alive(k) || u.hidden {
            continue;
        }
        let ally = k == rec.caster || u.owner == owner || !gm.is_enemy(owner, u.owner);
        if !ally || gm.unit_types().is_building(u.type_id) {
            continue;
        }
        let dur = crate::cast::effect_duration(&rec.level, gm.target_hero(k), F32::ZERO);
        if arith::lt(F32::ZERO, dur) {
            gm.apply_mods_buff(k, BUFF_BATTLE_ROAR, preds, UnitMods { damage_flat: rec.level.data[0], ..UnitMods::default() }, dur);
        }
    }
}

/// Mind Rot (`ANmr`, order 852565; label `NMR1` "Mana Drained per Second"): the target loses `DataA` mana every second
/// for `Dur` (`HeroDur` for a hero; 30 / 10 in the data) while `BNmr` lasts. The first drain is one second after the cast.
fn mind_rot(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let dur = crate::cast::effect_duration(&rec.level, gm.target_hero(target), F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = BuffPredicates { negative: true, magic: true, dispellable: true, ..buff_predicates(BUFF_MIND_ROT) };
    let serial = gm.apply_mods_buff(target, BUFF_MIND_ROT, preds, UnitMods::default(), dur);
    let id = gm.neutral2_next_id();
    gm.spells.neutral.h2.misc.drains.insert(id, Drain { target, per_second: rec.level.data[0], serial });
    start_timer(&mut gm, 1.0, target, MIND_ROT, id, KIND_DRAIN);
}

fn drain_tick(game: &RefCell<Game>, id: u32) {
    let mut gm = game.borrow_mut();
    let Some(d) = gm.spells.neutral.h2.misc.drains.get(&id).copied() else {
        return;
    };
    if !gm.alive(d.target) || !gm.unit(d.target).buffs.iter().any(|b| b.serial == d.serial) {
        gm.spells.neutral.h2.misc.drains.remove(&id);
        return;
    }
    let mana = gm.unit(d.target).mana;
    let taken = if arith::lt(mana, d.per_second) { mana } else { d.per_second };
    gm.write_mana(d.target, arith::sub(mana, taken));
    start_timer(&mut gm, 1.0, d.target, MIND_ROT, id, KIND_DRAIN);
}

// ---------------------------------------------------------------------------------------------------------------

impl Game {
    /// Demolish (`ANde`; labels `NDE1..4`: chance to demolish, damage multiplier against buildings, units, heroes): the
    /// attacker's damage against a target is multiplied by `DataB` (building), `DataC` (unit) or `DataD` (hero) when that
    /// multiplier is above zero, at a chance of `DataA` percent. Only the certain case (`DataA >= 100`, the data's value)
    /// is modelled; a smaller chance would need the roll's stream (UNSURE) and is ignored.
    pub(crate) fn neutral_attack_bonus(&mut self, attacker: ObjKey, target: ObjKey, damage: F32) -> F32 {
        if self.unit(attacker).abilities.get(DEMOLISH).is_none_or(|a| a.is_removed()) {
            return damage;
        }
        let Some(level) = self.ability_level_record(attacker, DEMOLISH) else {
            return damage;
        };
        if arith::lt(level.data[0], F32::from_bits(0x42C8_0000)) {
            return damage;
        }
        let t = self.unit(target);
        let factor = if self.unit_types().is_building(t.type_id) {
            level.data[1]
        } else if self.target_hero(target) {
            level.data[3]
        } else {
            level.data[2]
        };
        if arith::lt(F32::ZERO, factor) {
            arith::mul(damage, factor)
        } else {
            damage
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// Keeps the Permanent Immolation timers in step with the abilities a unit has (called from `sync_neutral_spells`).
pub(crate) fn sync(gm: &mut Game, unit: ObjKey) {
    let has = gm.unit(unit).abilities.get(PERM_IMMOLATION).is_some_and(|a| !a.is_removed());
    let running = gm.spells.neutral.h2.misc.immolating.contains_key(&unit);
    if has && !running {
        let level = gm.spell_level(unit, PERM_IMMOLATION);
        let period = if arith::lt(F32::ZERO, level.dur) { level.dur.to_f32() } else { 1.0 };
        let id = gm.timers.start(
            period,
            true,
            crate::natives::game::Payload::Spell(crate::cast::SpellMsg::UnitSpell {
                kind: KIND_PERM_IMMOLATION,
                unit,
                base: PERM_IMMOLATION,
                aux: 0,
            }),
        );
        gm.spells.neutral.h2.misc.immolating.insert(unit, id);
    } else if !has && running {
        if let Some(id) = gm.spells.neutral.h2.misc.immolating.remove(&unit) {
            gm.timers.cancel(id);
        }
    }
}

/// Permanent Immolation (`ANpi`; labels `EIM1..3` shared with Immolation; tooltip: "Burns nearby enemy units for DataA
/// points of damage per second"): every `Dur` (1 s) the enemy ground organic units within `Area` take `DataA`.
fn immolation_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey) {
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let ok = gm.alive(unit) && gm.unit(unit).abilities.get(PERM_IMMOLATION).is_some_and(|a| !a.is_removed());
        if !ok {
            if let Some(id) = gm.spells.neutral.h2.misc.immolating.remove(&unit) {
                gm.timers.cancel(id);
            }
            return;
        }
        let level = gm.spell_level(unit, PERM_IMMOLATION);
        let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
        let word = targs_word("ground,enemy,neutral,organic");
        for k in crate::natives::group::circle_candidates(&gm, x, y, level.area) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden || u.dead {
                    continue;
                }
            }
            if !crate::orc::area_predicate(&gm, unit, k, word) {
                continue;
            }
            if let Some(d) = gm.unit_damage_target(unit, k, level.data[0], false, false, 0, MISC_DAMAGE_TYPE) {
                killed.push(d);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(unit));
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// Rain of Chaos (`ANrc`, order 852237, point; labels `NRC1/2`): `DataA` is the ability the infernals are made with
/// (`ANin`), `DataB` the number of them; they fall one every `Dur` seconds (1 / 0.8 / 0.6) at random points of the
/// `Area` disc around the target (two stream-4 draws per infernal: angle and radius, UNSURE), each as a cast of the
/// Inferno effect with the level record of `DataA`'s row.
fn rain_of_chaos(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (x, y) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => return,
    };
    let left = conv::f2i(rec.level.data[1]);
    let Some(inferno) = gm.spell_levels_of(NEUTRAL_INFERNO).first().copied() else {
        return;
    };
    if left <= 0 {
        return;
    }
    let id = gm.neutral2_next_id();
    gm.spells.neutral.h2.misc.chaos.insert(id, Chaos { caster: rec.caster, x, y, level: rec.level, inferno, left });
    // The first infernal falls at once.
    start_timer(&mut gm, 0.0, rec.caster, RAIN_OF_CHAOS, id, KIND_CHAOS);
}

fn chaos_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let (c, point) = {
        let mut gm = game.borrow_mut();
        let Some(c) = gm.spells.neutral.h2.misc.chaos.get(&id).copied() else {
            return;
        };
        if !gm.alive(c.caster) {
            gm.spells.neutral.h2.misc.chaos.remove(&id);
            return;
        }
        (c, random_point(&mut gm, c.x, c.y, c.level.area))
    };
    crate::undead_ne::run_effect(game, rt, c.caster, crate::undead_ne::INFERNO, &c.inferno, CastTarget::Point(point.0, point.1));
    let mut gm = game.borrow_mut();
    let again = match gm.spells.neutral.h2.misc.chaos.get_mut(&id) {
        Some(e) => {
            e.left -= 1;
            e.left > 0
        }
        None => false,
    };
    if again {
        let gap = if arith::lt(F32::ZERO, c.level.dur) { c.level.dur.to_f32() } else { 0.0 };
        start_timer(&mut gm, gap, c.caster, RAIN_OF_CHAOS, id, KIND_CHAOS);
    } else {
        gm.spells.neutral.h2.misc.chaos.remove(&id);
    }
}

/// A random point of the disc of `radius` around `(x, y)`: angle `2 pi U`, distance `radius * U` (stream 4). The centre
/// without the RNG table (UNSURE: the original's helper).
pub(crate) fn random_point(gm: &mut Game, x: F32, y: F32, radius: F32) -> (F32, F32) {
    let tables = gm.order_tables().cloned();
    let Some(tables) = tables else {
        return (x, y);
    };
    let Some((table, streams)) = gm.rng_parts() else {
        return (x, y);
    };
    let table = table.clone();
    let rng = streams.stream_mut(4);
    let u1 = rng.random_fraction(&table);
    let u2 = rng.random_fraction(&table);
    let angle = arith::mul(TWO_PI, u1);
    let r = arith::mul(radius, u2);
    (
        arith::add(x, arith::mul(r, softfloat::math::cos(angle, &tables))),
        arith::add(y, arith::mul(r, softfloat::math::sin(angle, &tables))),
    )
}

// ---------------------------------------------------------------------------------------------------------------

/// Finger of Death (`ANfd`, order 852230; labels `NFD1..3`: graphic delay 0.25 s, graphic duration, damage 500): after the
/// graphic delay the target (a unit or a building of another player, not a hero) takes `DataC` damage. The damage type
/// is the neutral spell type (UNSURE).
fn finger_of_death(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let id = gm.neutral2_next_id();
    gm.spells.neutral.h2.misc.fingers.insert(id, Finger { caster: rec.caster, target, damage: rec.level.data[2] });
    let delay = rec.level.data[0];
    start_timer(&mut gm, if arith::lt(F32::ZERO, delay) { delay.to_f32() } else { 0.0 }, rec.caster, FINGER_OF_DEATH, id, KIND_FINGER);
}

fn finger_hit(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(f) = game.borrow_mut().spells.neutral.h2.misc.fingers.remove(&id) else {
        return;
    };
    let died = {
        let mut gm = game.borrow_mut();
        if !gm.alive(f.target) {
            return;
        }
        gm.unit_damage_target(f.caster, f.target, f.damage, false, false, 0, MISC_DAMAGE_TYPE)
    };
    if let Some(t) = died {
        dispatch_unit_death_by(game, rt, t, Some(f.caster));
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// Parasite (`ANpa`, order 852601 cast, 852602 / 852603 autocast toggles; labels of `Apoi` `POI1..4` + `NPA5/6`): the
/// target (not a hero) is poisoned: `DataA` damage per second for `Dur` (the poison fields; the slows `DataB` / `DataC`
/// are zero in the data). When it dies while infested, `DataE` units of the level's `UnitID` appear at the corpse for the
/// owner of the caster and live `Cast` seconds (the editor's "Summoned Unit Duration", which the class stores in the Cast
/// field, 90 s). The autocast is not modelled (UNSURE).
fn parasite(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let dur = rec.level.dur;
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = BuffPredicates { negative: true, physical: true, ..buff_predicates(BUFF_PARASITE) };
    let serial = gm.apply_mods_buff(target, BUFF_PARASITE, preds, UnitMods::default(), dur);
    gm.register_tick(target, serial, rec.caster, rec.level.data[0], false, MISC_DAMAGE_TYPE, true);
    gm.spells.neutral.h2.misc.infested.insert(target, Infest { source: rec.caster, level: rec.level });
}

/// The removal notification of the parasite (the infested unit died while the buff lasted).
pub(crate) fn on_unit_died(game: &RefCell<Game>, unit: ObjKey) {
    let mut gm = game.borrow_mut();
    let Some(m) = gm.spells.neutral.h2.misc.infested.remove(&unit) else {
        return;
    };
    if !gm.unit(unit).buffs.iter().any(|b| b.base == BUFF_PARASITE) || m.level.unit_id == 0 {
        return;
    }
    let (x, y, facing) = {
        let u = gm.unit(unit);
        (u.x, u.y, u.facing)
    };
    let owner = gm.unit(m.source).owner;
    for _ in 0..conv::f2i(m.level.data[4]).max(0) {
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, m.level.unit_id, x, y, facing, true) {
            gm.apply_timed_life(k, BUFF_PARASITE_MINION, m.level.cast);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// Dark Portal (`ANdp`, order 852229, point; labels `NDP1..3`): `DataB` to `DataC` units (a count between the minimum and
/// the maximum, a stream-4 draw, UNSURE) of the types in the list `DataA` (taken in turn) step out of the portal at the
/// point for the caster's owner. The campaign ability's units are permanent here (UNSURE).
fn dark_portal(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (x, y) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => return,
    };
    let types = gm.neutral_list(rec.caster, DARK_PORTAL, 0);
    if types.is_empty() {
        return;
    }
    let (min, max) = (conv::f2i(rec.level.data[1]).max(0), conv::f2i(rec.level.data[2]).max(0));
    let span = (max - min).max(0) + 1;
    let mut n = min;
    if span > 1 {
        if let Some((table, streams)) = gm.rng_parts() {
            let table = table.clone();
            let u = streams.stream_mut(4).random_fraction(&table);
            n += conv::f2i(arith::mul(u, conv::i2r(span))).min(span - 1);
        }
    }
    let (owner, facing) = {
        let c = gm.unit(rec.caster);
        (c.owner, c.facing)
    };
    for i in 0..n {
        let ty = types[(i as usize) % types.len()];
        let _ = crate::natives::unit::build(&mut gm, owner, ty, x, y, facing, true);
    }
}
