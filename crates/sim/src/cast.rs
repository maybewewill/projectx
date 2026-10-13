//! The spell casting core: check, event sequence, mana payment, cooldown, Dur versus HeroDur, and the effects of
//! Storm Bolt (`AHtb`), Holy Light (`AHhb`) and Slow (`Aslo`). Spec: `docs/spec/abilities-5.md`.
//!
//! Source of every rule below is the decompile of the concrete classes (vtables `0x6F8F56DC`, `0x6F8B1B4C`,
//! `0x6F92280C`, one shared cast body). Labels: CONFIRMED = read in the decompile and, where a number says so,
//! checked in Unicorn (`tools/emu/spells_emu.py`); LIKELY = strong inference; UNSURE = a stand-in.
//!
//! ## The cast sequence (CONFIRMED structure)
//! 1. Order entry (`0x6F054880`, then `0x6F0290E0`, then the cast start `0x6F074080`): refused when the caster is
//!    stunned-like (LIKELY, unit counters `+0x54`/flags `+0x5C`); the readiness check `0x6F050880` returns an error
//!    code: `209` not enough mana (`0x6F283C70`), `210` cooling down (flag `0x200` of the ability).
//! 2. Stage 1 (`0x6F071570`): event CHANNEL (`0x6F29A0F0`, key `0x80310` player-unit / `0x80321` unit). The
//!    readiness is checked again (`0x6F04FED0`, stage argument 1). A delay of the ability's cast time (`+0x84`,
//!    UNSURE source: the level's `Cast`) is a timer with message `0xD01BB`, else stage 2 runs at once.
//! 3. Stage 2 (`0x6F06C180`): readiness again (stage 2), event CAST (`0x6F29A280`, key `0x80311`/`0x80322`). A second
//!    delay (`+0x8C`, UNSURE source: the unit type's `castpt`) is a timer with message `0xD01BC`.
//! 4. Stage 3 (`0x6F063760`): readiness again (stage 3), event EFFECT (`0x6F29A410`, key `0x80312`/`0x80323`), then
//!    the effect body `0x6F05C840`: **pays the mana** (`0x6F283DB0`), runs the **class effect** (slot `0x424`),
//!    then starts the **cooldown** (slot `0x3A4` = `0x6F050B70`) when `Cool > 0`. Then the finish (`0x6F05BCD0` ->
//!    `0x6F050AF0`): event FINISH (`0x6F29A5A0`, key `0x80313`/`0x80324`). A channel time would insert a timer
//!    `0xD01BD` first (this model has no channelled spell).
//! 5. ENDCAST (`0x6F050950` -> `0x6F29A730`, key `0x80314`/`0x80325`) is posted when the cast ends; the model fires
//!    it after FINISH on completion and alone when a recheck fails (LIKELY).
//!
//! ## Rules
//! * Mana check `0x6F283C70` (Unicorn): true iff `mana >= cost` as an x87 compare (NaN is false); the cost value
//!   is `add(I2R(Cost), 0.0)` (abilities-4). Payment `0x6F283DB0` (Unicorn): paid = `min(cost, mana)` (the clamp
//!   replaces the paid amount only when `mana < cost`, so a NaN mana pays the cost), new mana = `mana + (-paid)`
//!   by soft add; the property is written by the agile setter.
//! * Cooldown (CONFIRMED structure): flag `0x200` on the ability plus a timer on the agile clock (`this+0xD0`,
//!   message `0xD01BE`, timeout `Cool`); the expiry clears the flag. `UnitResetCooldown` re-arms a running
//!   cooldown to `0.01` s when more than that is left (`0x6F037E10`, constant `0x6FAAE480`).
//! * Dur versus HeroDur (CONFIRMED in the Slow and Storm Bolt effect slots): a hero target takes `HeroDur`; any
//!   other target `Dur + ability+0x9C` (the extra term is `0.0` here, UNSURE). The hero test `0x6F1D2660` is
//!   "first character of the unit type id is `A`..`Z`" or two helper tests (Unicorn 2,000 cases).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, F32};

use crate::abilities::{buff_predicates, code};
use crate::natives::event::{Datum, Field, Firing};
use crate::natives::game::{Game, ObjKey, Object, Payload};
use crate::natives::{dispatch_unit_death_by, fire_observers_in, set_life, ScriptRuntime, KEY_BASE};
use crate::timer::TimerId;

/// Storm Bolt, Holy Light, Slow base codes.
pub const STORM_BOLT: u32 = code(b"AHtb");
pub const HOLY_LIGHT: u32 = code(b"AHhb");
pub const SLOW: u32 = code(b"Aslo");
/// Buff codes the three spells apply.
pub const BUFF_STUN: u32 = code(b"BSTN");
pub const BUFF_SLOW: u32 = code(b"Bslo");

/// Readiness error codes of `0x6F050880` (CONFIRMED).
pub const ERR_DISABLED: u32 = 221;
pub const ERR_NO_MANA: u32 = 209;
pub const ERR_COOLING: u32 = 210;

/// Spell event ids, player-unit table (`0x110`..) and unit table (`0x121`..), in the order CHANNEL, CAST, EFFECT,
/// FINISH, ENDCAST (CONFIRMED: the poster addresses use the keys `0x80310..0x80314` and `0x80321..0x80325`).
pub const EVENT_PLAYER_UNIT_BASE: u32 = 0x110;
pub const EVENT_UNIT_BASE: u32 = 0x121;
pub const SPELL_CHANNEL: u32 = 0;
pub const SPELL_CAST: u32 = 1;
pub const SPELL_EFFECT: u32 = 2;
pub const SPELL_FINISH: u32 = 3;
pub const SPELL_ENDCAST: u32 = 4;

/// Message ids of the stage timers (CONFIRMED in the message switch `0x6F074C60`).
pub const MSG_STAGE2: u32 = 0xD01BB;
pub const MSG_STAGE3: u32 = 0xD01BC;
pub const MSG_CHANNEL_END: u32 = 0xD01BD;
pub const MSG_COOLDOWN_END: u32 = 0xD01BE;

/// `UnitResetCooldown` re-arm time, the constant `0x6FAAE480` (runtime value `0x3C23D70A`).
pub const RESET_COOLDOWN_SECONDS: F32 = F32::from_bits(0x3C23_D70A);

/// Damage classes of the effects (the record's class word): Storm Bolt `0x8000` (slot `0x42C` of the class),
/// Holy Light's damage to undead `0x2000` (the constant passed to `0x6F2556E0`). As `unit_damage_target` takes the
/// damage type number, the bit index is used.
const STORM_BOLT_DAMAGE_TYPE: u32 = 15;
const HOLY_LIGHT_DAMAGE_TYPE: u32 = 13;
/// The unit class bit of UNDEAD in the classification word (`unitclass`, bit value 2; the original tests
/// `unit+0x248 & 2` in `0x6F08EAC0`).
const CLASS_UNDEAD: u32 = 0x2;

/// What an ability needs from its level record (the fields of `abilitytype::AbilityLevel` the spells read).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpellLevel {
    pub cast: F32,
    pub dur: F32,
    pub hero_dur: F32,
    pub cost: i32,
    pub cool: F32,
    pub area: F32,
    pub rng: F32,
    pub data: [F32; 9],
    /// The level's `UnitID` (raw code, record `+0x44`, 0 when empty): the unit type Water Elemental summons.
    pub unit_id: u32,
}

/// The cast point and backswing of a unit type (`castpt`, `castbsw`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CastTiming {
    pub point: F32,
    pub backswing: F32,
}

/// The target of a cast.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CastTarget {
    None,
    Unit(ObjKey),
    Point(F32, F32),
}

/// Why a cast did not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastError {
    /// The caster is dead or removed, or has no such ability.
    NoAbility,
    /// The caster cannot act (stunned).
    Stunned,
    /// `209`.
    NoMana,
    /// `210`.
    Cooling,
    /// The target is missing, dead or of the wrong kind.
    BadTarget,
    /// The target is farther than the level's range. The model does not walk the caster into range (UNSURE).
    OutOfRange,
}

impl CastError {
    /// The original's readiness error code, where the model has one.
    pub fn code(self) -> Option<u32> {
        match self {
            CastError::NoMana => Some(ERR_NO_MANA),
            CastError::Cooling => Some(ERR_COOLING),
            _ => None,
        }
    }
}

/// Timer messages of the spell system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellMsg {
    /// `0xD01BB`: stage 2 of cast `id`.
    Stage2(u32),
    /// `0xD01BC`: stage 3 of cast `id`.
    Stage3(u32),
    /// `0xD01BE`: the cooldown of `base` on `unit` ends.
    CooldownEnd { unit: ObjKey, base: u32 },
    /// A missile of Storm Bolt arrives.
    Impact(u32),
    /// A spell buff runs out.
    BuffEnd { unit: ObjKey, serial: u32 },
    /// A timer of the hero spells in `undead_ne.rs` (`kind` is its `TICK_*`).
    HeroSpell { kind: u8, unit: ObjKey, aux: u32 },
    /// A timer of the Human hero spells in `human_heroes.rs` (`kind` is its `TICK_*`, `id` the effect).
    Human { kind: u8, id: u32 },
    /// A timer of the unit spells in `unit_spells.rs` (autocast scans).
    UnitSpell { kind: u8, unit: ObjKey, base: u32, aux: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CastRec {
    pub(crate) caster: ObjKey,
    pub(crate) base: u32,
    pub(crate) target: CastTarget,
    pub(crate) level: SpellLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Missile {
    caster: ObjKey,
    target: ObjKey,
    base: u32,
    level: SpellLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BuffEffect {
    Stun,
    /// A buff with no stat effect of its own; the module that applied it reads it (`orc.rs`).
    Marker,
    /// Sleep (`'BUsl'`, `undead_ne.rs`): disables like a stun and ends on damage.
    Sleep,
    /// The move speed delta (negative slows) and the attack speed delta (stored only).
    Slow { move_delta: F32, attack_delta: F32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SpellBuff {
    unit: ObjKey,
    effect: BuffEffect,
    timer: Option<TimerId>,
    code: u32,
}

/// The state of the spell casting system (owned by [`Game`]).
#[derive(Debug, Clone, Default)]
pub struct SpellSystem {
    levels: BTreeMap<u32, Vec<SpellLevel>>,
    order_ids: BTreeMap<u32, u32>,
    timings: BTreeMap<u32, CastTiming>,
    missile_speed: BTreeMap<u32, F32>,
    next_id: u32,
    casts: BTreeMap<u32, CastRec>,
    current: BTreeMap<ObjKey, u32>,
    cooling: BTreeMap<(ObjKey, u32), TimerId>,
    missiles: BTreeMap<u32, Missile>,
    buffs: BTreeMap<u32, SpellBuff>,
    base_speed: BTreeMap<ObjKey, F32>,
    /// Bash state per owner (`docs/spec/abilities-human-heroes.md` section 3).
    pub(crate) bash: BTreeMap<ObjKey, crate::human_heroes::BashState>,
    /// The stage log: `(caster, base, event index)` of every spell event fired, in order (for tests and the host).
    pub log: Vec<(ObjKey, u32, u32)>,
    /// State of the Undead / Night Elf hero spells (`undead_ne.rs`).
    pub hero: crate::undead_ne::State,
    /// State of the Blizzard and Mass Teleport effects (`human_heroes.rs`).
    pub human: crate::human_heroes::State,
    /// State of the unit spells (`unit_spells.rs`).
    pub unit: crate::unit_spells::State,
}

// ---- pure rules (Unicorn-checked) ----

/// The x87 compare `a >= b` as `0x6F283C70` ends: false when either is NaN.
fn x87_ge(a: F32, b: F32) -> bool {
    f64::from(a.to_f32()) >= f64::from(b.to_f32())
}

/// The unit-side mana check `0x6F283C70` for a unit without cost-changing abilities and without the flag bit
/// that selects the other resource: `mana >= cost` (Unicorn, `tools/emu/spells_emu.py`).
pub fn mana_check(mana: F32, cost: F32) -> bool {
    x87_ge(mana, cost)
}

/// The mana payment `0x6F283DB0` for a unit without cost modifiers and without the free-cast cheat (Unicorn):
/// returns `(reported amount, new mana)`.
pub fn pay_mana(mana: F32, cost: F32) -> (F32, F32) {
    let diff = arith::sub(cost, cost);
    // `min`: replaced only when mana < cost as an ordered compare (an unordered compare keeps the cost).
    let paid = if f64::from(mana.to_f32()) < f64::from(cost.to_f32()) {
        mana
    } else {
        cost
    };
    let reported = arith::add(diff, paid);
    let new_mana = arith::add(mana, arith::neg(paid));
    (reported, new_mana)
}

/// The hero test `0x6F1D2660` (Unicorn, 2,000 cases): the first character of the type id in `A..=Z`, or either of
/// the two helper tests (`extra`).
pub fn is_hero_target(type_id: u32, extra: bool) -> bool {
    let first = (type_id >> 24) as u8;
    first.is_ascii_uppercase() || extra
}

/// The duration a target-effect slot takes (CONFIRMED in `0x6F1FC2A0` and `0x6F181C70`): a hero takes `HeroDur`,
/// anyone else `Dur + bonus` by soft add (`bonus` = ability `+0x9C`, `0.0` here).
pub fn effect_duration(level: &SpellLevel, hero: bool, bonus: F32) -> F32 {
    if hero {
        level.hero_dur
    } else {
        arith::add(level.dur, bonus)
    }
}

/// The cost value handed to the unit check: `add(I2R(Cost), 0.0)`.
pub fn cost_value(cost: i32) -> F32 {
    crate::abilities::cast_cost_value(cost)
}

// ---- registry API ----

impl SpellSystem {
    pub fn new() -> Self {
        let mut s = Self::default();
        // Original cast-order getters (slot 0x348), Unicorn-checked in tools/analysis/ability_orders.py.
        s.order_ids.insert(852_095, STORM_BOLT);
        s.order_ids.insert(852_092, HOLY_LIGHT);
        s.order_ids.insert(852_075, SLOW);
        crate::orc::register_orders(&mut s.order_ids);
        crate::unit_spells::register_orders(&mut s.order_ids);
        for (id, base) in crate::human_heroes::SPELL_ORDERS {
            s.order_ids.insert(id, base);
        }
        // Undead / night elf hero spells: cast getters (slot 0x348) from tools/analysis/ability_orders.py.
        for (id, base) in [
            (852_222, crate::undead_ne::DEATH_COIL),
            (852_226, crate::undead_ne::FROST_NOVA),
            (852_171, crate::undead_ne::ENTANGLING_ROOTS),
            (852_176, crate::undead_ne::FORCE_OF_NATURE),
            (852_223, crate::undead_ne::DEATH_PACT),
            (852_219, crate::undead_ne::DARK_RITUAL),
            (852_218, crate::undead_ne::CARRION_SWARM),
            (852_227, crate::undead_ne::SLEEP),
            (852_527, crate::undead_ne::SHADOW_STRIKE),
            (852_525, crate::undead_ne::BLINK),
            (852_177, crate::undead_ne::IMMOLATION),
        ] {
            s.order_ids.insert(id, base);
        }
        // Storm Bolt missile speed (UNSURE: AbilityFunc `Missilespeed`).
        s.missile_speed.insert(STORM_BOLT, F32::from_bits(1000.0f32.to_bits()));
        s
    }
}

impl Game {
    /// Installs the level records of ability `base` (index 0 = level 1).
    pub fn set_spell_levels(&mut self, base: u32, levels: Vec<SpellLevel>) {
        self.spells.levels.insert(base, levels);
    }

    /// Installs the cast point and backswing of a unit type.
    pub fn set_cast_timing(&mut self, type_id: u32, timing: CastTiming) {
        self.spells.timings.insert(type_id, timing);
    }

    /// Maps an order id to the ability that handles it.
    pub fn set_spell_order(&mut self, order_id: u32, base: u32) {
        self.spells.order_ids.insert(order_id, base);
    }

    /// Sets the missile speed of an ability (0 = instant).
    pub fn set_spell_missile_speed(&mut self, base: u32, speed: F32) {
        self.spells.missile_speed.insert(base, speed);
    }

    /// The missile speed registered for `base` (0.0 when none).
    pub fn spell_missile_speed_of(&self, base: u32) -> F32 {
        self.spells.missile_speed.get(&base).copied().unwrap_or_default()
    }

    /// The ability handling `order_id`, if the unit has it.
    pub fn spell_for_order(&self, unit: ObjKey, order_id: u32) -> Option<u32> {
        let base = *self.spells.order_ids.get(&order_id)?;
        let ab = self.unit(unit).abilities.get(base)?;
        (!ab.is_removed()).then_some(base)
    }

    /// The level record of ability `base` at the unit's current level, if both the ability and the table exist
    /// (the evasion gate must tell "no data" from an all-zero record).
    pub(crate) fn ability_level_record(&self, unit: ObjKey, base: u32) -> Option<SpellLevel> {
        let ab = self.unit(unit).abilities.get(base)?;
        let i = usize::try_from(ab.level_index).ok()?;
        self.spells.levels.get(&base)?.get(i).copied()
    }

    fn spell_level(&self, unit: ObjKey, base: u32) -> SpellLevel {
        let idx = self
            .unit(unit)
            .abilities
            .get(base)
            .map_or(0, |a| a.level_index);
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.spells.levels.get(&base).and_then(|l| l.get(i)))
            .copied()
            .unwrap_or_default()
    }

    /// The spell event log: `(caster, ability, event index)` in firing order.
    pub fn spell_log(&self) -> &[(ObjKey, u32, u32)] {
        &self.spells.log
    }

    /// Whether the ability `base` of `unit` is cooling down (flag `0x200`).
    pub fn is_cooling(&self, unit: ObjKey, base: u32) -> bool {
        self.spells.cooling.contains_key(&(unit, base))
    }

    /// Seconds left on a running cooldown (0 when ready).
    pub fn cooldown_remaining(&self, unit: ObjKey, base: u32) -> F32 {
        let Some(&id) = self.spells.cooling.get(&(unit, base)) else {
            return F32::ZERO;
        };
        let Some(expiry) = self.timers.expiry_of(id) else {
            return F32::ZERO;
        };
        let left = arith::sub(F32::from_bits(expiry.to_bits()), F32::from_bits(self.timers.now().to_bits()));
        if arith::lt(left, F32::ZERO) {
            F32::ZERO
        } else {
            left
        }
    }

    /// `UnitResetCooldown` (CONFIRMED structure, `0x6F037E10`): a cooldown with more than 0.01 s left is re-armed
    /// to 0.01 s.
    pub fn reset_cooldowns(&mut self, unit: ObjKey) {
        let keys: Vec<(ObjKey, u32)> = self
            .spells
            .cooling
            .keys()
            .filter(|(u, _)| *u == unit)
            .copied()
            .collect();
        for key in keys {
            let left = self.cooldown_remaining(key.0, key.1);
            if arith::lt(RESET_COOLDOWN_SECONDS, left) {
                if let Some(old) = self.spells.cooling.remove(&key) {
                    self.timers.cancel(old);
                }
                let id = self.timers.start(
                    RESET_COOLDOWN_SECONDS.to_f32(),
                    false,
                    Payload::Spell(SpellMsg::CooldownEnd {
                        unit: key.0,
                        base: key.1,
                    }),
                );
                self.spells.cooling.insert(key, id);
            }
        }
    }

    /// Whether `unit` is under an active stun from a spell (the buff entry still exists).
    pub fn is_spell_stunned(&self, unit: ObjKey) -> bool {
        self.spells.buffs.iter().any(|(serial, b)| {
            b.unit == unit
                && matches!(b.effect, BuffEffect::Stun)
                && self.unit(unit).buffs.iter().any(|x| x.serial == *serial)
        })
    }

    /// The readiness of a cast (`0x6F050880` order: mana `209`, then cooling `210`).
    fn spell_ready(&self, caster: ObjKey, base: u32, level: &SpellLevel) -> Result<(), CastError> {
        let u = self.unit(caster);
        if !mana_check(u.mana, cost_value(level.cost)) {
            return Err(CastError::NoMana);
        }
        if self.is_cooling(caster, base) {
            return Err(CastError::Cooling);
        }
        Ok(())
    }

    /// The cast timing of a unit type, if installed.
    pub(crate) fn spell_cast_timing(&self, type_id: u32) -> Option<CastTiming> {
        self.spells.timings.get(&type_id).copied()
    }

    pub(crate) fn spells_hero_state_mut(&mut self) -> &mut crate::undead_ne::State {
        &mut self.spells.hero
    }

    /// Starts a one-shot `HeroSpell` timer.
    pub(crate) fn timers_start_hero(&mut self, seconds: f32, kind: u8, unit: ObjKey, aux: u32) {
        self.timers
            .start(seconds, false, Payload::Spell(SpellMsg::HeroSpell { kind, unit, aux }));
    }

    /// Whether the spell buff `serial` is still on `unit`.
    pub(crate) fn spell_buff_alive(&self, unit: ObjKey, serial: u32) -> bool {
        self.spells.buffs.get(&serial).is_some_and(|b| b.unit == unit)
            && self.unit(unit).buffs.iter().any(|x| x.serial == serial)
    }

    pub(crate) fn alive(&self, k: ObjKey) -> bool {
        matches!(self.object(k), Object::Unit(u) if !u.dead && !u.removed)
    }

    /// The effective move speed after the active slows: recomputed from the unit's slow buffs.
    fn refresh_slow(&mut self, unit: ObjKey) {
        let mut worst: Option<F32> = None;
        for (serial, b) in &self.spells.buffs {
            if b.unit != unit {
                continue;
            }
            let BuffEffect::Slow { move_delta, .. } = b.effect else {
                continue;
            };
            if !self.unit(unit).buffs.iter().any(|x| x.serial == *serial) {
                continue;
            }
            worst = Some(match worst {
                Some(w) if arith::lt(w, move_delta) => w,
                _ => move_delta,
            });
        }
        match worst {
            Some(delta) => {
                let cur = self.unit(unit).move_speed;
                let base = *self.spells.base_speed.entry(unit).or_insert(cur);
                let factor = arith::add(F32::ONE, delta);
                self.unit_mut(unit).move_speed = arith::mul(base, factor);
            }
            None => {
                if let Some(base) = self.spells.base_speed.remove(&unit) {
                    self.unit_mut(unit).move_speed = base;
                }
            }
        }
    }

    /// Replaces the slow amounts of a live spell buff (the Shadow Strike decay, S37) and recomputes the speed.
    pub(crate) fn set_spell_buff_slow(&mut self, serial: u32, move_delta: F32, attack_delta: F32) {
        let Some(b) = self.spells.buffs.get_mut(&serial) else {
            return;
        };
        b.effect = BuffEffect::Slow { move_delta, attack_delta };
        let unit = b.unit;
        self.refresh_slow(unit);
    }

    /// Adds a spell buff: replaces a buff of the same code from an earlier cast (UNSURE: stacking), arms the
    /// expiry timer when `dur > 0`. For stun buffs, increments the stun counter.
    pub fn add_spell_buff(&mut self, unit: ObjKey, code: u32, effect: BuffEffect, dur: F32) -> u32 {
        let old: Vec<u32> = self
            .spells
            .buffs
            .iter()
            .filter(|(_, b)| b.unit == unit && b.code == code)
            .map(|(s, _)| *s)
            .collect();
        for s in old {
            self.drop_spell_buff(s);
        }
        if !arith::lt(F32::ZERO, dur) {
            return 0;
        }
        let serial = self.add_buff(unit, code, buff_predicates(code));
        let timer = self.timers.start(
            dur.to_f32(),
            false,
            Payload::Spell(SpellMsg::BuffEnd { unit, serial }),
        );
        self.spells.buffs.insert(
            serial,
            SpellBuff {
                unit,
                effect,
                timer: Some(timer),
                code,
            },
        );
        // Apply stun effect: increment stun counter
        if code == BUFF_STUN || matches!(effect, BuffEffect::Sleep) {
            self.unit_mut(unit).bonuses.stun += 1;
        }
        self.refresh_slow(unit);
        serial
    }

    pub fn drop_spell_buff(&mut self, serial: u32) {
        let Some(b) = self.spells.buffs.remove(&serial) else {
            return;
        };
        if let Some(t) = b.timer {
            self.timers.cancel(t);
        }
        // Remove stun effect: decrement stun counter
        if b.code == BUFF_STUN || matches!(b.effect, BuffEffect::Sleep) {
            self.unit_mut(b.unit).bonuses.stun = self.unit(b.unit).bonuses.stun.saturating_sub(1);
        }
        self.unit_mut(b.unit).buffs.retain(|x| x.serial != serial);
        crate::orc::on_buff_dropped(self, serial);
        self.refresh_slow(b.unit);
    }

    pub(crate) fn start_cooldown(&mut self, unit: ObjKey, base: u32, cool: F32) {
        // The level builder clamps a negative or NaN `Cool` to 0.0; the cooldown starts only when it is above zero.
        if !arith::lt(F32::ZERO, cool) {
            return;
        }
        if let Some(old) = self.spells.cooling.remove(&(unit, base)) {
            self.timers.cancel(old);
        }
        let id = self.timers.start(
            cool.to_f32(),
            false,
            Payload::Spell(SpellMsg::CooldownEnd { unit, base }),
        );
        self.spells.cooling.insert((unit, base), id);
    }

    pub(crate) fn target_hero(&self, target: ObjKey) -> bool {
        let u = self.unit(target);
        is_hero_target(u.type_id, u.hero.is_some())
    }
}

// ---- the sequence ----

fn point_of(gm: &Game, c: &CastRec) -> (Option<ObjKey>, F32, F32) {
    match c.target {
        CastTarget::Unit(t) => {
            let u = gm.unit(t);
            (Some(t), u.x, u.y)
        }
        CastTarget::Point(x, y) => (None, x, y),
        CastTarget::None => (None, F32::ZERO, F32::ZERO),
    }
}

/// Fires spell event `index` (CHANNEL.. ENDCAST) for `cast`: the owner's player-unit table first, then the caster's
/// unit table.
fn fire_spell_event(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, index: u32, cast: &CastRec) {
    let (owner, data) = {
        let gm = game.borrow();
        let (t, x, y) = point_of(&gm, cast);
        let mut data = vec![
            (Field::SpellCaster, Datum::Object(cast.caster)),
            (Field::SpellAbilityCode, Datum::Int(cast.base as i32)),
            (Field::SpellX, Datum::Real(x)),
            (Field::SpellY, Datum::Real(y)),
        ];
        if let Some(t) = t {
            data.push((Field::SpellTarget, Datum::Object(t)));
        }
        (gm.unit(cast.caster).owner, data)
    };
    game.borrow_mut().spells.log.push((cast.caster, cast.base, index));
    for (agent, base) in [
        (ObjKey(owner), EVENT_PLAYER_UNIT_BASE),
        (cast.caster, EVENT_UNIT_BASE),
    ] {
        let firing = {
            let mut gm = game.borrow_mut();
            gm.firings.push(Firing {
                id: base + index,
                data: data.clone(),
            });
            (gm.firings.len() - 1) as u32
        };
        fire_observers_in(game, rt, agent, KEY_BASE + base + index, cast.caster, Some(firing));
    }
}

/// Starts a cast of `base` by `caster` on `target` (the order entry plus stage 1 and the stages that need no
/// delay). Returns the cast id.
pub fn begin_cast(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    caster: ObjKey,
    base: u32,
    target: CastTarget,
) -> Result<u32, CastError> {
    let rec = {
        let gm = game.borrow();
        if !gm.alive(caster) {
            return Err(CastError::NoAbility);
        }
        match gm.unit(caster).abilities.get(base) {
            Some(a) if !a.is_removed() => {}
            _ => return Err(CastError::NoAbility),
        }
        if gm.is_stunned(caster) {
            return Err(CastError::Stunned);
        }
        let level = gm.spell_level(caster, base);
        gm.spell_ready(caster, base, &level)?;
        match target {
            CastTarget::Unit(t) => {
                if !gm.alive(t) {
                    return Err(CastError::BadTarget);
                }
                if let Some(e) = crate::unit_spells::target_error(&gm, caster, base, t) {
                    return Err(e);
                }
                if arith::lt(F32::ZERO, level.rng) {
                    let (a, b) = (gm.unit(caster), gm.unit(t));
                    let dx = arith::sub(b.x, a.x);
                    let dy = arith::sub(b.y, a.y);
                    let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
                    if arith::lt(arith::mul(level.rng, level.rng), d2) {
                        return Err(CastError::OutOfRange);
                    }
                }
            }
            CastTarget::Point(px, py) => {
                if arith::lt(F32::ZERO, level.rng) {
                    let a = gm.unit(caster);
                    let dx = arith::sub(px, a.x);
                    let dy = arith::sub(py, a.y);
                    let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
                    if arith::lt(arith::mul(level.rng, level.rng), d2) {
                        return Err(CastError::OutOfRange);
                    }
                }
            }
            CastTarget::None => {
                if !crate::orc::takes_no_target(base)
                    && !crate::unit_spells::takes_no_target(base)
                    && !crate::human_heroes::is_immediate_spell(base)
                    && !crate::undead_ne::is_immediate(base)
                {
                    return Err(CastError::BadTarget);
                }
                if let Some(e) = crate::unit_spells::untargeted_error(&gm, caster, base, &level) {
                    return Err(e);
                }
            }
        }
        CastRec {
            caster,
            base,
            target,
            level,
        }
    };
    // A new order replaces the unit's current cast.
    let previous = game.borrow().spells.current.get(&caster).copied();
    if let Some(prev) = previous {
        end_cast(game, rt, prev, false);
    }
    let id = {
        let mut gm = game.borrow_mut();
        gm.spells.next_id += 1;
        let id = gm.spells.next_id;
        gm.spells.casts.insert(id, rec);
        gm.spells.current.insert(caster, id);
        id
    };
    stage1(game, rt, id);
    Ok(id)
}

/// Whether a delay is a real wait: above zero (a NaN or non-positive delay runs the next stage at once).
fn waits(delay: F32) -> bool {
    arith::lt(F32::ZERO, delay)
}

fn recheck(game: &RefCell<Game>, id: u32) -> Option<CastRec> {
    let gm = game.borrow();
    let rec = *gm.spells.casts.get(&id)?;
    if !gm.alive(rec.caster) || gm.is_stunned(rec.caster) {
        return None;
    }
    if let CastTarget::Unit(t) = rec.target {
        if !gm.alive(t) {
            return None;
        }
    }
    gm.spell_ready(rec.caster, rec.base, &rec.level).ok()?;
    Some(rec)
}

fn stage1(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(rec) = game.borrow().spells.casts.get(&id).copied() else {
        return;
    };
    fire_spell_event(game, rt, SPELL_CHANNEL, &rec);
    // A trigger may have killed the caster or ended the cast.
    if recheck(game, id).is_none() {
        end_cast(game, rt, id, false);
        return;
    }
    if waits(rec.level.cast) && !crate::undead_ne::cast_field_is_period(rec.base) {
        game.borrow_mut().timers.start(
            rec.level.cast.to_f32(),
            false,
            Payload::Spell(SpellMsg::Stage2(id)),
        );
    } else {
        stage2(game, rt, id);
    }
}

fn stage2(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(rec) = recheck(game, id) else {
        end_cast(game, rt, id, false);
        return;
    };
    fire_spell_event(game, rt, SPELL_CAST, &rec);
    if recheck(game, id).is_none() {
        end_cast(game, rt, id, false);
        return;
    }
    let point = {
        let gm = game.borrow();
        gm.spells
            .timings
            .get(&gm.unit(rec.caster).type_id)
            .map_or(F32::ZERO, |t| t.point)
    };
    if waits(point) {
        game.borrow_mut()
            .timers
            .start(point.to_f32(), false, Payload::Spell(SpellMsg::Stage3(id)));
    } else {
        stage3(game, rt, id);
    }
}

fn stage3(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(rec) = recheck(game, id) else {
        end_cast(game, rt, id, false);
        return;
    };
    fire_spell_event(game, rt, SPELL_EFFECT, &rec);
    if !game.borrow().alive(rec.caster) {
        end_cast(game, rt, id, false);
        return;
    }
    // 0x6F05C840: pay the mana, run the class effect, start the cooldown.
    {
        let mut gm = game.borrow_mut();
        let cost = cost_value(rec.level.cost);
        if !arith::eq(cost, F32::ZERO) {
            let mana = gm.unit(rec.caster).mana;
            let (_, new_mana) = pay_mana(mana, cost);
            gm.write_mana(rec.caster, new_mana);
        }
    }
    run_effect(game, rt, &rec);
    game.borrow_mut().start_cooldown(rec.caster, rec.base, rec.level.cool);
    end_cast(game, rt, id, true);
}

/// Ends a cast: FINISH when it completed, then ENDCAST.
fn end_cast(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32, completed: bool) {
    let Some(rec) = game.borrow().spells.casts.get(&id).copied() else {
        return;
    };
    {
        let mut gm = game.borrow_mut();
        gm.spells.casts.remove(&id);
        if gm.spells.current.get(&rec.caster) == Some(&id) {
            gm.spells.current.remove(&rec.caster);
        }
    }
    if completed {
        fire_spell_event(game, rt, SPELL_FINISH, &rec);
    }
    fire_spell_event(game, rt, SPELL_ENDCAST, &rec);
}

// ---- the class effects (slot 0x424) ----

fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    if crate::human_heroes::is_immediate_spell(rec.base) {
        crate::human_heroes::run_immediate_effect(game, rt, rec);
        return;
    }
    if crate::human_heroes::run_target_effect(game, rec) {
        return;
    }
    if crate::orc::run_effect(game, rt, rec) {
        return;
    }
    if crate::undead_ne::run_effect(game, rt, rec.caster, rec.base, &rec.level, rec.target) {
        return;
    }
    if crate::unit_spells::run_effect(game, rt, rec) {
        return;
    }
    let CastTarget::Unit(target) = rec.target else {
        return;
    };
    match rec.base {
        STORM_BOLT => launch_storm_bolt(game, rt, rec, target),
        HOLY_LIGHT => holy_light(game, rt, rec, target),
        SLOW => slow(game, rec, target),
        _ => {}
    }
}

fn launch_storm_bolt(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let (id, delay) = {
        let mut gm = game.borrow_mut();
        let speed = gm.spells.missile_speed.get(&STORM_BOLT).copied().unwrap_or_default();
        let (a, b) = (gm.unit(rec.caster), gm.unit(target));
        let dx = b.x.to_f32() - a.x.to_f32();
        let dy = b.y.to_f32() - a.y.to_f32();
        // UNSURE: the flight arithmetic of the original is open (missiles.md); a plain distance over speed.
        let seconds = if speed.to_f32() > 0.0 {
            (dx * dx + dy * dy).sqrt() / speed.to_f32()
        } else {
            0.0
        };
        gm.spells.next_id += 1;
        let id = gm.spells.next_id;
        gm.spells.missiles.insert(
            id,
            Missile {
                caster: rec.caster,
                target,
                base: rec.base,
                level: rec.level,
            },
        );
        (id, seconds)
    };
    if delay > 0.0 {
        game.borrow_mut()
            .timers
            .start(delay, false, Payload::Spell(SpellMsg::Impact(id)));
    } else {
        impact(game, rt, id);
    }
}

fn impact(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(m) = game.borrow_mut().spells.missiles.remove(&id) else {
        return;
    };
    let killed = {
        let mut gm = game.borrow_mut();
        if !gm.alive(m.target) {
            return;
        }
        // Damage (DataA, class 0x8000, row 0 = spells: UNSURE row), then the stun buff of the level.
        let killed = gm.unit_damage_target(m.caster, m.target, m.level.data[0], false, false, 0, STORM_BOLT_DAMAGE_TYPE);
        if killed.is_none() && gm.alive(m.target) {
            let hero = gm.target_hero(m.target);
            let dur = effect_duration(&m.level, hero, F32::ZERO);
            gm.add_spell_buff(m.target, BUFF_STUN, BuffEffect::Stun, dur);
        }
        killed
    };
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(m.caster));
    }
}

fn holy_light(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let amount = rec.level.data[0];
    let mut killed = None;
    {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        let (t_owner, undead) = {
            let u = gm.unit(target);
            (u.owner, u.class_flags & CLASS_UNDEAD != 0)
        };
        let c_owner = gm.unit(rec.caster).owner;
        let enemy = gm.is_enemy(c_owner, t_owner);
        // `0x6F08EAC0` picks heal (+1), nothing (0) or damage (-1) from the relation table by the target's
        // undead state: a living target is healed unless an enemy; an undead target is hurt unless it is the
        // caster's own (UNSURE for allied undead).
        let relation: i32 = if undead {
            if c_owner == t_owner && !enemy {
                0
            } else {
                -1
            }
        } else if enemy {
            0
        } else {
            1
        };
        match relation {
            1 => {
                let u = gm.unit(target);
                let sum = arith::add(amount, u.life);
                let next = if arith::lt(u.max_life, sum) { u.max_life } else { sum };
                set_life(&mut gm, target, next);
            }
            -1 => {
                // DIV(amount, 2.0): an exact halving (the divisor is a power of two).
                let half = arith::mul(amount, F32::from_bits(0x3F00_0000));
                killed = gm.unit_damage_target(rec.caster, target, half, false, false, 0, HOLY_LIGHT_DAMAGE_TYPE);
            }
            _ => {}
        }
    }
    if let Some(t) = killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

fn slow(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    // The effect slot hands the buff -DataA (move) and -DataB (attack speed), `0x6F1FC2A0`.
    let effect = BuffEffect::Slow {
        move_delta: arith::neg(rec.level.data[0]),
        attack_delta: arith::neg(rec.level.data[1]),
    };
    gm.add_spell_buff(target, BUFF_SLOW, effect, dur);
}

/// The attack speed delta of the strongest active slow on `unit` (stored only; the attack cycle does not read it).
pub fn slow_attack_delta(game: &Game, unit: ObjKey) -> Option<F32> {
    game.spells.buffs.iter().find_map(|(s, b)| match b.effect {
        BuffEffect::Slow { attack_delta, .. }
            if b.unit == unit && game.unit(unit).buffs.iter().any(|x| x.serial == *s) =>
        {
            Some(attack_delta)
        }
        _ => None,
    })
}

// ---- timers ----

/// Handles a due spell timer (called by the script host with the game time at the expiry).
pub fn expire_spell_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, msg: SpellMsg) {
    match msg {
        SpellMsg::Stage2(id) => stage2(game, rt, id),
        SpellMsg::Stage3(id) => stage3(game, rt, id),
        SpellMsg::Impact(id) => impact(game, rt, id),
        SpellMsg::CooldownEnd { unit, base } => {
            let mut gm = game.borrow_mut();
            // The timer that fired is gone; a restarted cooldown has a new id and is still in the map.
            let live = gm
                .spells
                .cooling
                .get(&(unit, base))
                .is_some_and(|&id| gm.timers.expiry_of(id).is_some());
            if !live {
                gm.spells.cooling.remove(&(unit, base));
            }
        }
        SpellMsg::BuffEnd { unit, serial } => {
            let _ = unit;
            game.borrow_mut().drop_spell_buff(serial);
        }
        SpellMsg::HeroSpell { kind, unit, aux } if kind >= 0x80 => crate::orc::on_timer(game, rt, kind, unit, aux),
        SpellMsg::HeroSpell { kind, unit, aux } => crate::undead_ne::on_timer(game, rt, kind, unit, aux),
        SpellMsg::Human { kind, id } => crate::human_heroes::on_timer(game, rt, kind, id),
        SpellMsg::UnitSpell { kind, unit, base, aux } => crate::unit_spells::on_timer(game, rt, kind, unit, base, aux),
    }
}
