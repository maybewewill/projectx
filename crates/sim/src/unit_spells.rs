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
use crate::cast::{begin_cast, effect_duration, CastError, CastRec, CastTarget, SpellMsg};
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::unit_attr::is_unit_type;
use crate::natives::{set_life, ScriptRuntime};
use crate::timer::TimerId;
use crate::unitclass::ty;

pub const HEAL: u32 = code(b"Ahea");
pub const BUFF_HEAL: u32 = code(b"Bhea");
pub const INNER_FIRE: u32 = code(b"Ainf");
pub const BUFF_INNER_FIRE: u32 = code(b"Binf");
pub const BLOODLUST: u32 = code(b"Ablo");
pub const BUFF_BLOODLUST: u32 = code(b"Bblo");

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
pub const SPELL_ORDERS: [(u32, u32); 3] = [(ORDER_HEAL, HEAL), (852_066, INNER_FIRE), (852_101, BLOODLUST)];

/// `(autocast on order, autocast off order, ability)`.
const TOGGLES: [(u32, u32, u32); 3] = [
    (ORDER_HEAL_ON, ORDER_HEAL_OFF, HEAL),
    (852_067, 852_068, INNER_FIRE),
    (852_102, 852_103, BLOODLUST),
];

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

/// The stat modifiers a spell buff adds (deltas; the sinks of `auras.rs`). Undone exactly on removal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitMods {
    pub armor: F32,
    /// Added to the movement multiplier (`0x6F202FF0`).
    pub move_delta: F32,
    /// Added to the attack speed multiplier (`0x6F0C9090`).
    pub attack_delta: F32,
    /// Added to the percentage damage modifier.
    pub damage_pct: F32,
    pub life_regen: F32,
    pub mana_regen: F32,
}

impl Default for UnitMods {
    fn default() -> Self {
        UnitMods {
            armor: F32::ZERO,
            move_delta: F32::ZERO,
            attack_delta: F32::ZERO,
            damage_pct: F32::ZERO,
            life_regen: F32::ZERO,
            mana_regen: F32::ZERO,
        }
    }
}

fn nonzero(x: F32) -> bool {
    x.to_bits() & 0x7FFF_FFFF != 0
}

impl Game {
    fn apply_mods(&mut self, unit: ObjKey, m: &UnitMods) {
        if nonzero(m.armor) {
            self.add_armor(unit, m.armor);
        }
        if nonzero(m.move_delta) {
            self.add_move(unit, m.move_delta);
        }
        if nonzero(m.attack_delta) {
            self.add_attack_speed(unit, m.attack_delta);
        }
        if nonzero(m.damage_pct) {
            let u = self.unit_mut(unit);
            u.bonuses.damage_pct = arith::add(u.bonuses.damage_pct, m.damage_pct);
        }
        if nonzero(m.life_regen) {
            self.add_life_regen(unit, m.life_regen);
        }
        if nonzero(m.mana_regen) {
            self.add_mana_regen(unit, m.mana_regen);
        }
    }

    /// Takes the modifiers of a removed buff off `unit`.
    pub(crate) fn undo_mods(&mut self, unit: ObjKey, m: &UnitMods) {
        let neg = UnitMods {
            armor: arith::neg(m.armor),
            move_delta: arith::neg(m.move_delta),
            attack_delta: arith::neg(m.attack_delta),
            damage_pct: arith::neg(m.damage_pct),
            life_regen: arith::neg(m.life_regen),
            mana_regen: arith::neg(m.mana_regen),
        };
        self.apply_mods(unit, &neg);
    }

    /// Applies (or refreshes) the buff `base` with the stat modifiers `mods` for `duration` seconds. An existing buff
    /// of the same code gets its modifiers replaced and its duration set by the never-shorten rule (CONFIRMED for
    /// Inner Fire: the creator finds the buff by its code and calls its apply slot `0x358`, which runs the
    /// set-duration slot first and rebuilds the effects).
    pub fn apply_mods_buff(
        &mut self,
        unit: ObjKey,
        base: u32,
        predicates: crate::abilities::BuffPredicates,
        mods: UnitMods,
        duration: F32,
    ) -> u32 {
        let serial = match self.buff_position(unit, base) {
            Some(i) => {
                let serial = self.unit(unit).buffs[i].serial;
                self.set_buff_duration(unit, serial, duration);
                if let Some(crate::auras::BuffEffect::Mods(old)) = self.unit(unit).buffs[i].effect.clone() {
                    self.undo_mods(unit, &old);
                }
                serial
            }
            None => {
                let serial = self.next_buff_serial();
                self.unit_mut(unit).buffs.push(Buff {
                    serial,
                    base,
                    predicates,
                    ..Buff::default()
                });
                self.set_buff_duration(unit, serial, duration);
                serial
            }
        };
        self.apply_mods(unit, &mods);
        if let Some(i) = self.unit(unit).buffs.iter().position(|b| b.serial == serial) {
            self.unit_mut(unit).buffs[i].effect = Some(crate::auras::BuffEffect::Mods(mods));
        }
        serial
    }
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
        INNER_FIRE | BLOODLUST => {
            let t = gm.unit(target);
            // targs: air,ground,friend,neutral,self (Bloodlust adds organic); no invulnerable targets.
            if !(is_ally(gm, caster, target) || t.owner >= 12) || t.removed || gm.unit_is_invulnerable(target) {
                return Some(CastError::BadTarget);
            }
            if is_unit_type(gm, t, ty::STRUCTURE) {
                return Some(CastError::BadTarget);
            }
            if base == BLOODLUST && is_unit_type(gm, t, ty::MECHANICAL) {
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
        INNER_FIRE | BLOODLUST => {
            if let CastTarget::Unit(t) = rec.target {
                buff_spell(game, rec, t);
            }
            true
        }
        _ => false,
    }
}

/// Inner Fire (`0x6F13BFA0`) and Bloodlust (`0x6F1CB080`), CONFIRMED structure: the duration is `HeroDur` for a hero
/// target, `Dur` plus the unknown extra otherwise; the buff is found by its code (refresh) or created, and gets the
/// level's data. Inner Fire: `DataA` damage fraction, `DataB` armor, `DataD` life regeneration, `DataE` mana
/// regeneration (`DataC` is the autocast range, read by the scan); Bloodlust: `DataA` attack speed, `DataB` movement
/// speed (`DataC` is the model scale, presentation). The meaning of each field is LIKELY (the editor labels), the
/// sinks are CONFIRMED (Inner Fire: armor sink `0x6F2769C0` and the attack record modifiers; Bloodlust: the attack
/// speed and movement sinks).
fn buff_spell(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let d = &rec.level.data;
    let (code, mods) = if rec.base == INNER_FIRE {
        (
            BUFF_INNER_FIRE,
            UnitMods {
                damage_pct: d[0],
                armor: d[1],
                life_regen: d[3],
                mana_regen: d[4],
                ..UnitMods::default()
            },
        )
    } else {
        (
            BUFF_BLOODLUST,
            UnitMods {
                attack_delta: d[0],
                move_delta: d[1],
                ..UnitMods::default()
            },
        )
    };
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(code)
    };
    if arith::lt(F32::ZERO, dur) {
        gm.apply_mods_buff(target, code, preds, mods, dur);
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

/// The shared comparator `0x6F023400` (`CAbilityAutoTargetSpell`, used by Inner Fire, Bloodlust ...; Unicorn 600
/// cases, slot `0x464` is 0 for them): an in-range candidate beats an out-of-range best, an out-of-range one loses;
/// two in-range units: the NEARER wins only if it is not strictly nearer than the best (the original keeps the
/// farther of two in-range units: `Vb > Vc` returns the best); two out-of-range units: the strictly nearer wins.
pub fn default_prefers(vc: F32, vb: F32, t: F32) -> bool {
    let g = |x: F32| f64::from(x.to_f32());
    let a = g(vc) <= g(t);
    let b = g(vb) <= g(t);
    match (a, b) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => !(g(vb) > g(vc)),
        (false, false) => g(vb) > g(vc),
    }
}

/// The life fraction below which Inner Fire / Bloodlust refuse (`0x6FAAE4A0` = 0.1); the mana rule thresholds are
/// `0x6FAAE4BC` = 0.3 (Inner Fire) and `0x6FAAE4D0` = 0.5 (Bloodlust).
const FRACTION_REFUSE: f32 = 0.1;

/// The class filters of slot `0x450` (CONFIRMED, static): Heal `0x6F0F0370`, Inner Fire `0x6F123DC0`, Bloodlust
/// `0x6F1AFF10`. Common to all: the base filter `0x6F030730` and the illusion rule. Bloodlust and Inner Fire accept
/// only a candidate that has the attack ability, is attacking a unit that is not a friend of the candidate's player
/// (`0x6F285AC0`, the passive-alliance mask), does not carry the buff yet, and whose life fraction `f` is above
/// 0.1; below the mana threshold (0.5 Bloodlust, 0.3 Inner Fire) the caster must also be at full mana. Inner Fire
/// further needs the candidate to be within its attack reach of that target (`0x6F0CBAE0` + `0x6F0207A0`, LIKELY
/// reading) and ends in the call `0x6F1452A0` when the ability's flag 2 is set (not decoded, taken as true).
fn class_filter(gm: &Game, unit: ObjKey, base: u32, k: ObjKey) -> bool {
    if gm.unit(k).illusion && !gm.spells.unit.illusions_get_autocast {
        return false;
    }
    match base {
        HEAL => {
            if let Some(src) = gm.heal_mark_source(k) {
                if src != Some(unit) {
                    return false;
                }
            }
            true
        }
        INNER_FIRE | BLOODLUST => {
            let (buff, threshold) = if base == INNER_FIRE {
                (BUFF_INNER_FIRE, 0.3f32)
            } else {
                (BUFF_BLOODLUST, 0.5f32)
            };
            if gm.buff_position(k, buff).is_some() {
                return false;
            }
            let Some(target) = gm.attack_view(k).and_then(|v| v.target) else {
                return false;
            };
            if !gm.alive(target) {
                return false;
            }
            // `0x6F285AC0` is true for a passive-friendly player: the attack target must be hostile.
            if !gm.is_enemy(gm.unit(k).owner, gm.unit(target).owner) {
                return false;
            }
            let f = f64::from(life_fraction(gm, k).to_f32());
            if f <= f64::from(FRACTION_REFUSE) {
                return false;
            }
            if f < f64::from(threshold) {
                let c = gm.unit(unit);
                if f64::from(c.mana.to_f32()) != f64::from(c.max_mana.to_f32()) {
                    return false;
                }
            }
            if base == INNER_FIRE {
                let ty = gm.attack_type(gm.unit(k).type_id);
                if !gm.in_attack_range(k, target, &ty) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

/// Slots `0x44C` / `0x450` / `0x458`: walks the candidates in the circle query's order and keeps the best.
fn pick_target(gm: &Game, unit: ObjKey, base: u32) -> Option<ObjKey> {
    if !is_autocast(base) {
        return None;
    }
    let level = gm.ability_level_record(unit, base)?;
    let (cx, cy) = {
        let u = gm.unit(unit);
        (u.x, u.y)
    };
    let t = arith::mul(level.rng, level.rng);
    let mut best: Option<(ObjKey, F32)> = None;
    for k in crate::natives::group::circle_candidates(gm, cx, cy, level.rng) {
        if !gm.alive(k) || gm.unit(k).hidden {
            continue;
        }
        if target_error(gm, unit, base, k).is_some() || !class_filter(gm, unit, base, k) {
            continue;
        }
        let v = dist2(gm, unit, k);
        best = match best {
            None => Some((k, v)),
            Some((b, vb)) => {
                let wins = if base == HEAL {
                    heal_prefers(v, life_fraction(gm, k), vb, life_fraction(gm, b), t)
                } else {
                    default_prefers(v, vb, t)
                };
                if wins {
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
