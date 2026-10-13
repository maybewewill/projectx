//! Unit (non-hero) spells of the melee races. Spec: `docs/spec/abilities-unit-spells.md`.
//!
//! One module for the spells the computer players and the autocast rules use. The cast core (`cast.rs`) calls
//! [`register_orders`], [`takes_no_target`], [`target_error`], [`run_effect`] and [`on_timer`]; `auras.rs` calls
//! [`Game::sync_unit_spells`] when abilities change. Labels as in the other ability modules: CONFIRMED = read in the
//! machine code (and Unicorn-run where a count is given), LIKELY = strong inference, UNSURE = a stand-in.
//!
//! ## The autocast lifecycle of `CAbilityAutoTargetSpell` (shared by Heal, Inner Fire, Bloodlust, Raise Dead, Curse,
//! Faerie Fire, Web, Abolish Magic ... 17 classes; CONFIRMED structure, static)
//! * A one-shot agent timer (message `0xD01C2`) on the ability. Activation (`0x6F044340`, slot `0x438`) arms it with
//!   the delay `0x6FAAE484` = 0.02 s (runtime value `0x3CA3D70A`).
//! * The handler (`0x6F0443A0`) does nothing when the ability flag `0x100000` (disabled) is set. Otherwise it re-reads
//!   the period from slot `0x468` (`0x6F0231E0`, the constant `0x6FAAE50C` = **3.0 s**). It scans only when the
//!   autocast flag `0x80000` is on, the ability is not unit-owned-blocked (`0x20`), slot `0x3D8` (`0x6F0379E0`)
//!   holds - the caster's current order is empty or **attack (`0xD000F`) or attack-move (`0xD0014`)** - and the
//!   caster is not under flags `+0x5C & 0x1000000`, `+0x20 & 0x4000000`, `+0x198 > 0` (not modelled). The target
//!   search is slot `0x448` (`0x6F030680`): a group around the caster (radius from slot `0x440`, the level's `Rng`
//!   plus the caster's collision, scaled), the per-unit callback `0x6F0232E0` -> slot `0x44C` (`0x6F044570`).
//!   When a target is found the order is issued (`0x6F0292A0` / `0x6F028D80`) and the next delay is
//!   `max(total cast time (slot 0x3E0), 1.0)`; otherwise the period of 3.0 s.
//! * Slot `0x44C` (`0x6F044570`) per candidate, in this order (CONFIRMED): the visibility test `0x6F284EE0`;
//!   the target validity slot `0x3EC` (targs flags plus the class slot `0x3F0`; nonzero = error); a hostile
//!   candidate is rejected unless its flag `+0x5C & 0x100`; the class filter slot `0x450`; then the comparator slot
//!   `0x458` with the candidate's distance value `V` (squared distance, `0x6F476860`), the best so far and its `V`,
//!   and a threshold `T` (the squared search range).
//!
//! ## Heal (`AHea`, `CAbilityHeal`, vtable `0x6F8C9674`)
//! * Effect slot `0x424` = `0x6F108280` (CONFIRMED): heal `DataA`; an ethereal target (`+0x158 > 0`) gets
//!   `DataA * Misc EtherealHealBonus`; the life adder `0x6F277C20`; then a marker buff `Bhea` (code
//!   `0x42686561`, getter `0x6F0DA880`) on the target with duration `1.0 + (Cool + 0.01)` (the cooldown plus the
//!   constant `0x6FAAE480`, stored in the ability's float holder `+0x90/+0x94` and added to the constant `0x6FAAE4F4`;
//!   LIKELY reading of the holder), applied by the set-duration rule (never shorten).
//! * Extra target check, slot `0x3F0` (`0x6F0F0290`, CONFIRMED): a target illusion is refused unless Misc
//!   `IllusionsCanRestoreLife`; a target with `max life - life < 1.0` gets error `0x1C`.
//! * Autocast filter, slot `0x450` (`0x6F0F0370`, CONFIRMED): the base filter `0x6F030730` (caster not under flag
//!   `+0x20 & 0x40000000`, relation test), an illusion candidate is refused unless Misc `IllusionsGetAutocast`, the
//!   candidate must be an ally of the caster (`0x6F285B80`), its `Bhea` buff, if any, must have been made by this
//!   caster (a buff from another healer refuses it, so two priests do not heal one unit in the same window).
//! * Comparator, slot `0x458` (`0x6F0DADD0`, CONFIRMED by disassembly): `a` = candidate in range (`V <= T`), `b` =
//!   best in range. `a && !b` -> candidate; `!a && b` -> best; both in range -> the lower life fraction (strict, ties
//!   keep the best); neither -> the smaller `V` (strict). The life fraction is `life / maxlife` (`0x6F020640`,
//!   LIKELY).
//! * Order ids from the registration code of the DLL (immediates pushed beside the order strings): `heal` 852063,
//!   `healon` 852064, `healoff` 852065 (CONFIRMED).
//! * UNSURE stand-ins: the search radius is `Rng` (the original adds the collision size and scales), `V`/`T` are
//!   squared world distances (the original uses cell units), an out-of-range best target is only cast when it is in
//!   range (the original walks to it), Heal defaults to autocast ON (the default flag source was not found).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, F32};

use crate::abilities::{buff_predicates, code, Buff};
use crate::cast::{begin_cast, CastError, CastRec, CastTarget, SpellMsg};
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::unit_attr::is_unit_type;
use crate::natives::{set_life, ScriptRuntime};
use crate::timer::TimerId;
use crate::unitclass::ty;

pub const HEAL: u32 = code(b"Ahea");
pub const BUFF_HEAL: u32 = code(b"Bhea");

pub const ORDER_HEAL: u32 = 852_063;
pub const ORDER_HEAL_ON: u32 = 852_064;
pub const ORDER_HEAL_OFF: u32 = 852_065;

/// Timer kind of the autocast scan (message `0xD01C2`).
pub const KIND_AUTOCAST: u8 = 1;
/// First scan delay after activation (`0x6FAAE484`, runtime value `0x3CA3D70A`).
pub const FIRST_SCAN_BITS: u32 = 0x3CA3_D70A;
/// The scan period (`0x6FAAE50C` = 3.0).
pub const SCAN_PERIOD: f32 = 3.0;
/// The marker's extra: the constant `0x6FAAE480` (0.01).
const MARK_EXTRA_BITS: u32 = 0x3C23_D70A;
/// Order ids attack and attack-move (slot `0x3D8`).
const ORDER_ATTACK: u32 = 0xD000F;
const ORDER_AI_MOVE: u32 = 0xD0014;

/// `(order id, ability)` pairs registered with the cast core.
pub const SPELL_ORDERS: [(u32, u32); 1] = [(ORDER_HEAL, HEAL)];

/// `(autocast on order, autocast off order, ability)`.
const TOGGLES: [(u32, u32, u32); 1] = [(ORDER_HEAL_ON, ORDER_HEAL_OFF, HEAL)];

pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (id, base) in SPELL_ORDERS {
        ids.insert(id, base);
    }
}

/// Abilities cast without a target.
pub fn takes_no_target(_base: u32) -> bool {
    false
}

/// Whether `base` is an autocast ability of this module.
pub fn is_autocast(base: u32) -> bool {
    TOGGLES.iter().any(|t| t.2 == base)
}

/// Whether the ability starts with autocast on (UNSURE: the default flag source is not found).
fn default_autocast_on(base: u32) -> bool {
    base == HEAL
}

#[derive(Debug, Clone, Copy)]
struct AutoSlot {
    on: bool,
    generation: u32,
    timer: Option<TimerId>,
}

/// State of the unit spells (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    auto: BTreeMap<(ObjKey, u32), AutoSlot>,
    /// Heal marker buffs: serial -> the healer.
    marks: BTreeMap<u32, ObjKey>,
    /// Misc `EtherealHealBonus` (a missing key reads 0.0).
    pub ethereal_heal_bonus: F32,
    /// Misc `IllusionsCanRestoreLife` / `IllusionsGetAutocast` (missing = off).
    pub illusions_can_restore_life: bool,
    pub illusions_get_autocast: bool,
}

impl Game {
    /// Whether autocast of `base` is on for `unit`.
    pub fn autocast_on(&self, unit: ObjKey, base: u32) -> bool {
        self.spells.unit.auto.get(&(unit, base)).is_some_and(|s| s.on)
    }

    /// Turns autocast of `base` on or off (the orders `healon` / `healoff`). Turning on arms the first scan.
    pub fn set_autocast(&mut self, unit: ObjKey, base: u32, on: bool) {
        if !self.unit(unit).abilities.contains(base) {
            return;
        }
        let key = (unit, base);
        let mut slot = self.spells.unit.auto.get(&key).copied().unwrap_or(AutoSlot {
            on: false,
            generation: 0,
            timer: None,
        });
        if let Some(t) = slot.timer.take() {
            self.timers.cancel(t);
        }
        slot.on = on;
        slot.generation = slot.generation.wrapping_add(1);
        if on {
            slot.timer = Some(self.arm_scan(unit, base, slot.generation, F32::from_bits(FIRST_SCAN_BITS).to_f32()));
        }
        self.spells.unit.auto.insert(key, slot);
    }

    fn arm_scan(&mut self, unit: ObjKey, base: u32, generation: u32, seconds: f32) -> TimerId {
        self.timers.start(
            seconds,
            false,
            Payload::Spell(SpellMsg::UnitSpell {
                kind: KIND_AUTOCAST,
                unit,
                base,
                aux: generation,
            }),
        )
    }

    /// Creates the autocast slots of the abilities a unit has (default on) and drops those of abilities it lost.
    /// Called with the aura sync when abilities change and when a unit is created.
    pub fn sync_unit_spells(&mut self, unit: ObjKey) {
        let have: Vec<u32> = self
            .unit(unit)
            .abilities
            .iter()
            .map(|a| a.base)
            .filter(|&b| is_autocast(b))
            .collect();
        let stale: Vec<u32> = self
            .spells
            .unit
            .auto
            .keys()
            .filter(|(u, b)| *u == unit && !have.contains(b))
            .map(|&(_, b)| b)
            .collect();
        for b in stale {
            if let Some(s) = self.spells.unit.auto.remove(&(unit, b)) {
                if let Some(t) = s.timer {
                    self.timers.cancel(t);
                }
            }
        }
        for b in have {
            if !self.spells.unit.auto.contains_key(&(unit, b)) {
                if default_autocast_on(b) {
                    self.set_autocast(unit, b, true);
                } else {
                    self.spells.unit.auto.insert(
                        (unit, b),
                        AutoSlot {
                            on: false,
                            generation: 0,
                            timer: None,
                        },
                    );
                }
            }
        }
    }

    /// Puts (or refreshes, by the set-duration rule) the `Bhea` marker of `healer` on `target`.
    pub fn heal_mark(&mut self, target: ObjKey, healer: ObjKey, duration: F32) {
        let serial = match self.buff_position(target, BUFF_HEAL) {
            Some(i) => self.unit(target).buffs[i].serial,
            None => {
                let serial = self.next_buff_serial();
                self.unit_mut(target).buffs.push(Buff {
                    serial,
                    base: BUFF_HEAL,
                    predicates: buff_predicates(BUFF_HEAL),
                    ..Buff::default()
                });
                serial
            }
        };
        self.set_buff_duration(target, serial, duration);
        self.spells.unit.marks.insert(serial, healer);
    }

    /// The healer that made the `Bhea` marker on `unit`, if it has one.
    fn heal_mark_source(&self, unit: ObjKey) -> Option<Option<ObjKey>> {
        let b = self.unit(unit).buffs.iter().find(|b| b.base == BUFF_HEAL)?;
        Some(self.spells.unit.marks.get(&b.serial).copied())
    }
}

/// The immediate orders of this module: the autocast toggles. `None` when `id` is not one of them.
pub(crate) fn immediate_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    for (on, off, base) in TOGGLES {
        if id == on || id == off {
            let mut gm = game.borrow_mut();
            if !gm.unit(unit).abilities.contains(base) {
                return Some(false);
            }
            gm.set_autocast(unit, base, id == on);
            return Some(true);
        }
    }
    None
}

fn is_ally(gm: &Game, caster: ObjKey, other: ObjKey) -> bool {
    let (a, b) = (gm.unit(caster).owner, gm.unit(other).owner);
    a == b || !gm.is_enemy(a, b)
}

/// The deficit rule of Heal's slot `0x3F0`: `max life - life >= 1.0` (the error `0x1C` otherwise). A NaN deficit
/// passes (the unordered compare).
fn heal_needs(gm: &Game, target: ObjKey) -> bool {
    let u = gm.unit(target);
    let deficit = arith::sub(u.max_life, u.life);
    !arith::lt(deficit, F32::ONE)
}

/// Target validity beyond the range test (`0x6F04F7A0` + class slot `0x3F0`).
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    match base {
        HEAL => {
            let t = gm.unit(target);
            if !is_ally(gm, caster, target) || t.removed {
                return Some(CastError::BadTarget);
            }
            // targs: air,ground,friend,... organic (no structures, no mechanical units), nonancient.
            if is_unit_type(gm, t, ty::STRUCTURE) || is_unit_type(gm, t, ty::MECHANICAL) || is_unit_type(gm, t, ty::ANCIENT)
            {
                return Some(CastError::BadTarget);
            }
            if t.illusion && !gm.spells.unit.illusions_can_restore_life {
                return Some(CastError::BadTarget);
            }
            if !heal_needs(gm, target) {
                return Some(CastError::BadTarget);
            }
            None
        }
        _ => None,
    }
}

/// The class effects of this module. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, _rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        HEAL => {
            if let CastTarget::Unit(t) = rec.target {
                heal(game, rec, t);
            }
            true
        }
        _ => false,
    }
}

/// `0x6F108280`: heal, then the marker buff.
fn heal(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let mut amount = rec.level.data[0];
    if gm.unit(target).ethereal > 0 {
        amount = arith::mul(amount, gm.spells.unit.ethereal_heal_bonus);
    }
    let (life, max) = {
        let u = gm.unit(target);
        (u.life, u.max_life)
    };
    let sum = arith::add(amount, life);
    let next = if arith::lt(max, sum) { max } else { sum };
    set_life(&mut gm, target, next);
    // The marker: duration 1.0 + (Cool + 0.01), the set-duration rule when the buff already exists.
    let extra = arith::add(rec.level.cool, F32::from_bits(MARK_EXTRA_BITS));
    let duration = arith::add(F32::ONE, extra);
    gm.heal_mark(target, rec.caster, duration);
}

// ---- the autocast scan ----

/// A timer of this module fired.
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, base: u32, aux: u32) {
    if kind != KIND_AUTOCAST {
        return;
    }
    {
        let mut gm = game.borrow_mut();
        match gm.spells.unit.auto.get_mut(&(unit, base)) {
            Some(s) if s.on && s.generation == aux => s.timer = None,
            _ => return,
        }
        if !gm.alive(unit) || !gm.unit(unit).abilities.contains(base) {
            return;
        }
    }
    let delay = scan(game, rt, unit, base);
    let mut gm = game.borrow_mut();
    let Some(mut slot) = gm.spells.unit.auto.get(&(unit, base)).copied() else {
        return;
    };
    if !slot.on || slot.generation != aux || !gm.alive(unit) {
        return;
    }
    slot.timer = Some(gm.arm_scan(unit, base, aux, delay));
    gm.spells.unit.auto.insert((unit, base), slot);
}

/// One autocast scan of `base` for `unit`; returns the delay to the next one.
fn scan(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, base: u32) -> f32 {
    let target = {
        let gm = game.borrow();
        if gm.is_stunned(unit) || !order_allows_autocast(&gm, unit) {
            return SCAN_PERIOD;
        }
        pick_target(&gm, unit, base)
    };
    let Some(target) = target else {
        return SCAN_PERIOD;
    };
    match begin_cast(game, rt, unit, base, CastTarget::Unit(target)) {
        Ok(_) => {
            // max(total cast time, 1.0): the level's Cast plus the cast point (UNSURE composition of slot 0x3E0).
            let gm = game.borrow();
            let level = gm.ability_level_record(unit, base).unwrap_or_default();
            let point = gm.cast_point_of(unit);
            let total = arith::add(level.cast, point);
            if arith::lt(total, F32::ONE) {
                1.0
            } else {
                total.to_f32()
            }
        }
        Err(_) => SCAN_PERIOD,
    }
}

/// Slot `0x3D8` (`0x6F0379E0`): the caster has no order, or its current order is attack or attack-move.
fn order_allows_autocast(gm: &Game, unit: ObjKey) -> bool {
    match gm.order_head(unit) {
        None => true,
        Some(o) => o.id == ORDER_ATTACK || o.id == ORDER_AI_MOVE,
    }
}

fn life_fraction(gm: &Game, k: ObjKey) -> F32 {
    let u = gm.unit(k);
    if arith::eq(u.max_life, F32::ZERO) {
        return F32::ZERO;
    }
    match gm.order_tables() {
        Some(t) => arith::div(u.life, u.max_life, t),
        // Without the soft-float tables (unit tests) a plain division stands in.
        None => F32::from_bits((u.life.to_f32() / u.max_life.to_f32()).to_bits()),
    }
}

fn dist2(gm: &Game, a: ObjKey, b: ObjKey) -> F32 {
    let (p, q) = (gm.unit(a), gm.unit(b));
    let dx = arith::sub(p.x, q.x);
    let dy = arith::sub(p.y, q.y);
    arith::add(arith::mul(dx, dx), arith::mul(dy, dy))
}

/// The comparator `0x6F0DADD0` of Heal: true when the candidate replaces the best. The compares are x87 compares of
/// the stored singles (Unicorn, `tools/emu/unit_spells_emu.py`, vectors in `tests/data/unit_spells/comparator.txt`):
/// "in range" is `V <= T` (ordered), the strict orderings are ordered compares, so a NaN keeps the best.
pub fn heal_prefers(vc: F32, fc: F32, vb: F32, fb: F32, t: F32) -> bool {
    let g = |x: F32| f64::from(x.to_f32());
    let a = g(vc) <= g(t);
    let b = g(vb) <= g(t);
    match (a, b) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => g(fb) > g(fc),
        (false, false) => g(vb) > g(vc),
    }
}

/// Slots `0x44C` / `0x450` / `0x458`: walks the candidates in the circle query's order and keeps the best.
fn pick_target(gm: &Game, unit: ObjKey, base: u32) -> Option<ObjKey> {
    if base != HEAL {
        return None;
    }
    let level = gm.ability_level_record(unit, base)?;
    let (cx, cy, owner) = {
        let u = gm.unit(unit);
        (u.x, u.y, u.owner)
    };
    let _ = owner;
    let t = arith::mul(level.rng, level.rng);
    let mut best: Option<(ObjKey, F32)> = None;
    for k in crate::natives::group::circle_candidates(gm, cx, cy, level.rng) {
        if !gm.alive(k) || gm.unit(k).hidden {
            continue;
        }
        if target_error(gm, unit, base, k).is_some() {
            continue;
        }
        // Slot 0x450 (0x6F0F0370): illusions, and a marker buff made by another healer.
        if gm.unit(k).illusion && !gm.spells.unit.illusions_get_autocast {
            continue;
        }
        if let Some(src) = gm.heal_mark_source(k) {
            if src != Some(unit) {
                continue;
            }
        }
        let v = dist2(gm, unit, k);
        best = match best {
            None => Some((k, v)),
            Some((b, vb)) => {
                if heal_prefers(v, life_fraction(gm, k), vb, life_fraction(gm, b), t) {
                    Some((k, v))
                } else {
                    Some((b, vb))
                }
            }
        };
    }
    best.map(|(k, _)| k)
}

impl Game {
    /// The caster's cast point (`castpt`), 0 when unknown.
    pub(crate) fn cast_point_of(&self, unit: ObjKey) -> F32 {
        self.spell_cast_timing(self.unit(unit).type_id).map_or(F32::ZERO, |t| t.point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    #[test]
    fn comparator_prefers_in_range_then_lowest_life_then_nearest() {
        let t = f(100.0);
        // candidate in range, best out of range
        assert!(heal_prefers(f(50.0), f(0.9), f(200.0), f(0.1), t));
        // candidate out of range, best in range
        assert!(!heal_prefers(f(200.0), f(0.1), f(50.0), f(0.9), t));
        // both in range: strictly lower fraction wins, ties keep the best
        assert!(heal_prefers(f(10.0), f(0.3), f(50.0), f(0.5), t));
        assert!(!heal_prefers(f(10.0), f(0.5), f(50.0), f(0.5), t));
        // neither in range: strictly nearer wins
        assert!(heal_prefers(f(150.0), f(0.9), f(300.0), f(0.1), t));
        assert!(!heal_prefers(f(300.0), f(0.9), f(300.0), f(0.1), t));
    }
}
