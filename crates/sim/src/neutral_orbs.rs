//! Attack modifiers of the neutral heroes (S50): Black Arrow (Dark Ranger) and Incinerate (Firelord). Spec:
//! `docs/spec/abilities-neutral.md` section 8.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::cast::{CastRec, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::neutral_heroes2::start_timer;
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// Dark Ranger: Black Arrow (`CAbilityBlackArrow < CAbilityFlamingArrows`, buff `CBuffBlackArrow`) and its spell form
/// (`CAbilityBlackArrowSpell < CAbilitySimpleSpell`, the item / creep version).
pub const BLACK_ARROW: u32 = code(b"ANba");
pub const BLACK_ARROW_SPELL: u32 = code(b"ANbs");
pub const BUFF_BLACK_ARROW: u32 = code(b"BNba");
pub const BUFF_DARK_MINION: u32 = code(b"BNdm");
/// Firelord: Incinerate (`CAbilityIncinerate < CAbilityCleavingAttack`, buff `CBuffIncinerate`).
pub const INCINERATE: u32 = code(b"ANic");
pub const BUFF_INCINERATE: u32 = code(b"BNic");

/// Black Arrow orders: autocast on / off (slots `0x308` / `0x318`) and the cast (slot `0x348`, shared with the spell form).
pub const ORDER_BLACK_ARROW_ON: u32 = 852_578;
pub const ORDER_BLACK_ARROW_OFF: u32 = 852_579;
pub const ORDER_BLACK_ARROW_CAST: u32 = 852_577;

/// Timer kind: the delayed death blast of Incinerate (`aux` = blast id).
pub const KIND_INCINERATE: u8 = 0x68;

/// The damage type of the bonus damage and of the death blast (the neutral spell type; the class words were not read,
/// UNSURE).
const ORB_DAMAGE_TYPE: u32 = 15;

/// A mark on a target: who put it there and the level numbers the death needs.
#[derive(Debug, Clone, Copy)]
struct Mark {
    source: ObjKey,
    level: SpellLevel,
    /// Incinerate: the attacks that stacked.
    count: i32,
}

/// A pending Incinerate death blast.
#[derive(Debug, Clone, Copy)]
struct Blast {
    source: ObjKey,
    x: F32,
    y: F32,
    level: SpellLevel,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    /// Units whose Black Arrow autocast is on (default off).
    autocast: BTreeSet<ObjKey>,
    black: BTreeMap<ObjKey, Mark>,
    incinerate: BTreeMap<ObjKey, Mark>,
    blasts: BTreeMap<u32, Blast>,
}

pub(crate) fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    ids.insert(ORDER_BLACK_ARROW_CAST, BLACK_ARROW_SPELL);
}

/// The Black Arrow autocast toggles. `None` when `id` is not one of them.
pub(crate) fn toggle_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    if id != ORDER_BLACK_ARROW_ON && id != ORDER_BLACK_ARROW_OFF {
        return None;
    }
    let mut gm = game.borrow_mut();
    if gm.unit(unit).abilities.get(BLACK_ARROW).is_none_or(|a| a.is_removed()) {
        return Some(false);
    }
    let h = &mut gm.spells.neutral.h2.orbs;
    if id == ORDER_BLACK_ARROW_ON {
        h.autocast.insert(unit);
    } else {
        h.autocast.remove(&unit);
    }
    Some(true)
}

impl Game {
    /// Whether Black Arrow autocast is on for `unit`.
    pub fn black_arrow_autocast(&self, unit: ObjKey) -> bool {
        self.spells.neutral.h2.orbs.autocast.contains(&unit)
    }

    /// Whether `unit` carries a live Black Arrow mark.
    pub fn black_arrow_marked(&self, unit: ObjKey) -> bool {
        self.unit(unit).buffs.iter().any(|b| b.base == BUFF_BLACK_ARROW)
    }

    /// The Incinerate stack count on `unit` (0 without the buff).
    pub fn incinerate_stacks(&self, unit: ObjKey) -> i32 {
        if !self.unit(unit).buffs.iter().any(|b| b.base == BUFF_INCINERATE) {
            return 0;
        }
        self.spells.neutral.h2.orbs.incinerate.get(&unit).map_or(0, |m| m.count)
    }

    /// The attack hook (called after a hit that left the victim alive; returns the units that died of the bonus damage).
    ///
    /// Black Arrow (`ANba`; labels `NBA1..3`, mana `Cost` 6 per arrow): with the autocast on and enough mana the arrow
    /// costs the mana, adds `DataA` damage and marks the target with `BNba` for `Dur` (2 s). Incinerate (`ANic`; labels
    /// `NIC1..6`, passive): each attack stacks the burn on the target (`BNic`, `Dur` 2 s refreshed) and adds
    /// `DataA * stacks` damage ("these flames add DataA damage on the first attack, twice as much on the second, ...").
    /// Both are LIKELY: the field roles are the editor's, the stacking and the timing follow the tooltips.
    pub(crate) fn neutral_orb_on_hit(&mut self, attacker: ObjKey, target: ObjKey) -> Vec<ObjKey> {
        let mut died = Vec::new();
        if self.alive(target) && self.unit(attacker).abilities.get(BLACK_ARROW).is_some_and(|a| !a.is_removed()) {
            if let Some(d) = self.black_arrow_hit(attacker, target) {
                died.push(d);
            }
        }
        if self.alive(target) && self.unit(attacker).abilities.get(INCINERATE).is_some_and(|a| !a.is_removed()) {
            if let Some(d) = self.incinerate_hit(attacker, target) {
                died.push(d);
            }
        }
        died
    }

    fn black_arrow_hit(&mut self, attacker: ObjKey, target: ObjKey) -> Option<ObjKey> {
        if !self.black_arrow_autocast(attacker) {
            return None;
        }
        let level = self.ability_level_record(attacker, BLACK_ARROW)?;
        if !targs_ok(self, attacker, target, targs_word("air,ground,enemy,organic,neutral")) {
            return None;
        }
        let cost = conv::i2r(level.cost);
        let mana = self.unit(attacker).mana;
        if arith::lt(mana, cost) {
            return None;
        }
        self.write_mana(attacker, arith::sub(mana, cost));
        mark_black_arrow(self, attacker, target, &level);
        if arith::lt(F32::ZERO, level.data[0]) {
            return self.unit_damage_target(attacker, target, level.data[0], false, false, 0, ORB_DAMAGE_TYPE);
        }
        None
    }

    fn incinerate_hit(&mut self, attacker: ObjKey, target: ObjKey) -> Option<ObjKey> {
        let level = self.ability_level_record(attacker, INCINERATE)?;
        if !targs_ok(self, attacker, target, targs_word("enemy,neutral,organic,nonancient")) {
            return None;
        }
        let live = self.unit(target).buffs.iter().any(|b| b.base == BUFF_INCINERATE);
        let prev = self.spells.neutral.h2.orbs.incinerate.get(&target).copied().filter(|m| live && m.source == attacker);
        let count = prev.map_or(1, |m| m.count + 1);
        let hero = self.target_hero(target);
        let dur = crate::cast::effect_duration(&level, hero, F32::ZERO);
        let preds = BuffPredicates { negative: true, magic: true, dispellable: true, ..buff_predicates(BUFF_INCINERATE) };
        if arith::lt(F32::ZERO, dur) {
            self.apply_mods_buff(target, BUFF_INCINERATE, preds, UnitMods::default(), dur);
        }
        self.spells.neutral.h2.orbs.incinerate.insert(target, Mark { source: attacker, level, count });
        let bonus = arith::mul(level.data[0], conv::i2r(count));
        if arith::lt(F32::ZERO, bonus) {
            return self.unit_damage_target(attacker, target, bonus, false, false, 0, ORB_DAMAGE_TYPE);
        }
        None
    }
}

fn mark_black_arrow(gm: &mut Game, source: ObjKey, target: ObjKey, level: &SpellLevel) {
    let hero = gm.target_hero(target);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = BuffPredicates { negative: true, magic: true, dispellable: true, ..buff_predicates(BUFF_BLACK_ARROW) };
    gm.apply_mods_buff(target, BUFF_BLACK_ARROW, preds, UnitMods::default(), dur);
    gm.spells.neutral.h2.orbs.black.insert(target, Mark { source, level: *level, count: 0 });
}

/// The spell form `ANbs` (cast on a unit): the target gets the Black Arrow mark for `Dur` (3 s); no damage (the class has
/// only the effect and the target slots).
pub(crate) fn run_effect(game: &RefCell<Game>, rec: &CastRec) -> bool {
    if rec.base != BLACK_ARROW_SPELL {
        return false;
    }
    if let CastTarget::Unit(t) = rec.target {
        let mut gm = game.borrow_mut();
        if gm.alive(t) {
            mark_black_arrow(&mut gm, rec.caster, t, &rec.level);
        }
    }
    true
}

pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<crate::cast::CastError> {
    if base != BLACK_ARROW_SPELL {
        return None;
    }
    let w = targs_word("air,ground,enemy,organic,neutral");
    (gm.unit(target).removed || !targs_ok(gm, caster, target, w)).then_some(crate::cast::CastError::BadTarget)
}

/// The removal notifications of the marks (the death of the marked unit).
///
/// Black Arrow (`CBuffBlackArrow` slot `0x284`): `f2i(DataB)` units of the level's `UnitID` rise at the corpse for the
/// owner of the archer and live `DataC` seconds (buff `BNdm`). Incinerate (`CBuffIncinerate` slot `0x284`): after `DataF`
/// seconds the corpse's place burns: enemies of the burner's owner within `DataC` take `DataB`, those within `DataE` take
/// `DataD` ("up to DataB damage to all nearby hostile units"). The ring split is the editor's "Full / Half" pair.
pub(crate) fn on_unit_died(game: &RefCell<Game>, unit: ObjKey) {
    let mut gm = game.borrow_mut();
    let black = gm.spells.neutral.h2.orbs.black.remove(&unit);
    if let Some(m) = black.filter(|_| gm.unit(unit).buffs.iter().any(|b| b.base == BUFF_BLACK_ARROW)) {
        if m.level.unit_id != 0 {
            let (x, y, facing) = {
                let u = gm.unit(unit);
                (u.x, u.y, u.facing)
            };
            let owner = gm.unit(m.source).owner;
            for _ in 0..conv::f2i(m.level.data[1]).max(0) {
                if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, m.level.unit_id, x, y, facing, true) {
                    gm.apply_timed_life(k, BUFF_DARK_MINION, m.level.data[2]);
                }
            }
        }
    }
    let inc = gm.spells.neutral.h2.orbs.incinerate.remove(&unit);
    if let Some(m) = inc.filter(|_| gm.unit(unit).buffs.iter().any(|b| b.base == BUFF_INCINERATE)) {
        let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
        let id = gm.neutral2_next_id();
        gm.spells.neutral.h2.orbs.blasts.insert(id, Blast { source: m.source, x, y, level: m.level });
        let delay = m.level.data[5];
        start_timer(&mut gm, if arith::lt(F32::ZERO, delay) { delay.to_f32() } else { 0.0 }, m.source, INCINERATE, id, KIND_INCINERATE);
    }
}

pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, aux: u32) {
    if kind != KIND_INCINERATE {
        return;
    }
    let Some(b) = game.borrow_mut().spells.neutral.h2.orbs.blasts.remove(&aux) else {
        return;
    };
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let word = targs_word("air,ground,enemy,neutral");
        let (full_area, half_area) = (b.level.data[2], b.level.data[4]);
        let outer = if arith::lt(full_area, half_area) { half_area } else { full_area };
        let owner = gm.unit(b.source).owner;
        for k in crate::natives::group::circle_candidates(&gm, b.x, b.y, outer) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden || u.dead || !gm.is_enemy(owner, u.owner) {
                    continue;
                }
            }
            if !crate::orc::area_predicate(&gm, b.source, k, word) {
                continue;
            }
            let u = gm.unit(k);
            let (dx, dy) = (arith::sub(u.x, b.x), arith::sub(u.y, b.y));
            let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
            let amount = if !arith::lt(arith::mul(full_area, full_area), d2) { b.level.data[1] } else { b.level.data[3] };
            if let Some(d) = gm.unit_damage_target(b.source, k, amount, false, false, 0, ORB_DAMAGE_TYPE) {
                killed.push(d);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(b.source));
    }
}
