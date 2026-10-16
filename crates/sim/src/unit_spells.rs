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

use softfloat::{arith, conv, F32};

use crate::abilities::{buff_predicates, code, Buff};
use crate::cast::{begin_cast, effect_duration, CastError, CastRec, CastTarget, SpellLevel, SpellMsg};
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
pub const CRIPPLE: u32 = code(b"Acri");
pub const BUFF_CRIPPLE: u32 = code(b"Bcri");
pub const UNHOLY_FRENZY: u32 = code(b"Auhf");
pub const BUFF_UNHOLY_FRENZY: u32 = code(b"BUhf");
pub const REJUVENATION: u32 = code(b"Arej");
pub const BUFF_REJUVENATION: u32 = code(b"Brej");
pub const DISPEL_MAGIC: u32 = code(b"Adis");
pub const RAISE_DEAD: u32 = code(b"Arai");
pub const BUFF_RAISE_DEAD: u32 = code(b"Brai");
/// The default second summon of Raise Dead's second level (`DataD` of the data, the skeletal mage).
pub const SKELETAL_MAGE: u32 = code(b"uskm");
pub const ROAR: u32 = code(b"Aroa");
pub const BUFF_ROAR: u32 = code(b"Broa");
pub const FAERIE_FIRE: u32 = code(b"Afae");
pub const BUFF_FAERIE_FIRE: u32 = code(b"Bfae");
pub const CURSE: u32 = code(b"Acrs");
pub const BUFF_CURSE: u32 = code(b"Bcrs");
pub const FROST_ATTACK: u32 = code(b"Afra");
pub const BUFF_FROST: u32 = code(b"Bfro");
pub const SLOW_POISON: u32 = code(b"Aspo");
pub const BUFF_SLOW_POISON: u32 = code(b"Bspo");
pub const FEEDBACK: u32 = code(b"Afbk");
/// The damage type of Slow Poison's ticks: the class word `0x800000` of `CBuffSlowPoison` slot `0x374` is bit 23,
/// `DAMAGE_TYPE_SLOW_POISON`.
const DAMAGE_TYPE_SLOW_POISON: u32 = 23;
/// The values of Misc `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease` in the 1.26a `MiscData.txt` (the keys the
/// frost buff reads, `0x6F199EA0`); used when the host did not load a Misc layer.
const FROST_MOVE_DEFAULT: f32 = 0.5;
const FROST_ATTACK_DEFAULT: f32 = 0.25;
pub const HEALING_WARD: u32 = code(b"Ahwd");
pub const BUFF_HEALING_WARD: u32 = code(b"Bhwd");
pub const STASIS_TRAP: u32 = code(b"Asta");
pub const BUFF_STASIS_TRAP: u32 = code(b"Bsta");
pub const SENTRY_WARD: u32 = code(b"Aeye");
pub const BUFF_SENTRY_WARD: u32 = code(b"Beye");
pub const POLYMORPH: u32 = code(b"Aply");
pub const BUFF_POLYMORPH: u32 = code(b"Bply");
/// The default sheep types of Polymorph's data fields `DataB..DataE` (land, flying, amphibious, water).
pub const SHEEP: [u32; 4] = [code(b"nshe"), code(b"nshf"), code(b"nsha"), code(b"nshw")];
/// The RNG stream of the Polymorph type pick (`0x6F12DAA0` calls the roll wrapper with stream `0x18`).
pub const POLYMORPH_STREAM: usize = 24;
pub const ABOLISH_MAGIC: u32 = code(b"Aadm");
pub const ENSNARE: u32 = code(b"Aens");
pub const BUFF_ENSNARE_AIR: u32 = code(b"Bena");
pub const BUFF_ENSNARE_GROUND: u32 = code(b"Beng");
pub const WEB: u32 = code(b"Aweb");
pub const BUFF_WEB_AIR: u32 = code(b"Bwea");
pub const BUFF_WEB_GROUND: u32 = code(b"Bweb");
pub const LIGHTNING_SHIELD: u32 = code(b"Alsh");
pub const BUFF_LIGHTNING_SHIELD: u32 = code(b"Blsh");
pub const PURGE: u32 = code(b"Aprg");
pub const BUFF_PURGE: u32 = code(b"Bprg");
/// Anti-magic Shell's buff code, the one the dispel worker looks for (`0x6F110530` returns `0x42616D73`).
pub const BUFF_ANTI_MAGIC_SHELL: u32 = code(b"Bams");

pub const ORDER_HEAL: u32 = 852_063;
pub const ORDER_HEAL_ON: u32 = 852_064;
pub const ORDER_HEAL_OFF: u32 = 852_065;

/// Timer kind of the autocast scan (message `0xD01C2`).
pub const KIND_AUTOCAST: u8 = 1;
/// Timer kind of a damage/heal-over-time tick (message `0xD01C1`), the tick period of `CBuffDoT` is 1.0 s.
pub const KIND_TICK: u8 = 2;
/// The first tick of a damage-over-time buff, 0.01 s after the apply (`0xD01E2`, constant `0x6FAAE480`).
pub const KIND_TICK_FIRST: u8 = 3;
/// Purge's movement update (message `0xD01C1` on the buff) and attack update (`0xD01C3`), repeating timers.
pub const KIND_PURGE_MOVE: u8 = 4;
pub const KIND_PURGE_ATTACK: u8 = 5;
/// Lightning Shield's tick timer (message `0xD01C1` on the buff; a one-shot re-armed by each tick).
pub const KIND_SHIELD: u8 = 6;
/// A net (Ensnare / Web) arrives at its target.
pub const KIND_NET: u8 = 7;
/// The tick period (`CBuffDoT` slot `0x35C` = `0x6F0ADBE0`: the constant `0x6FAAE4F4` = 1.0).
const TICK_PERIOD: f32 = 1.0;
/// The first-tick delay (`0x6F0ADBD0`: the constant `0x6FAAE480`).
const TICK_FIRST_DELAY_BITS: u32 = 0x3C23_D70A;
/// Damage type of Unholy Frenzy's drain: the class word `0x40000` of `CBuffUnholyFrenzy` slot `0x374` is bit 18,
/// `DAMAGE_TYPE_DEATH`.
const DAMAGE_TYPE_DEATH: u32 = 18;
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
pub const SPELL_ORDERS: [(u32, u32); 20] = [
    (852_109, HEALING_WARD),
    (852_114, STASIS_TRAP),
    (852_105, SENTRY_WARD),
    (852_074, POLYMORPH),
    (852_132, ABOLISH_MAGIC),
    (852_106, ENSNARE),
    (852_211, WEB),
    (852_110, LIGHTNING_SHIELD),
    (852_111, PURGE),
    (852_190, CURSE),
    (852_164, ROAR),
    (852_197, RAISE_DEAD),
    (852_057, DISPEL_MAGIC),
    (852_160, REJUVENATION),
    (852_209, UNHOLY_FRENZY),
    (ORDER_HEAL, HEAL),
    (852_066, INNER_FIRE),
    (852_101, BLOODLUST),
    (852_189, CRIPPLE),
    (852_149, FAERIE_FIRE),
];

/// `(autocast on order, autocast off order, ability)`.
const TOGGLES: [(u32, u32, u32); 7] = [
    (852_133, 852_134, ABOLISH_MAGIC),
    (852_212, 852_213, WEB),
    (852_191, 852_192, CURSE),
    (852_198, 852_199, RAISE_DEAD),
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
pub fn takes_no_target(base: u32) -> bool {
    base == RAISE_DEAD || base == ROAR
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
    /// The attacker's chance to miss (Curse, `CBuffCurse` holder `+0xD0`). Not a stat sink: the attacker-side walk
    /// of the miss gate reads it from the buff ([`Game::attacker_miss_scores`]).
    pub miss_chance: F32,
    /// One root count while the buff exists (Ensnare, Web).
    pub root: bool,
    /// The buff holds a morph (Polymorph): its removal restores the unit's own form.
    pub morph: bool,
    /// One invulnerability count while the buff exists (the potion of invulnerability, `Bvul`).
    pub invuln: bool,
    /// A flat attack damage bonus (added to the unit's attack bonus, Battle Roar).
    pub damage_flat: F32,
    /// Flat attack damage added to the unit's attacks while the buff exists (the Potion of Vampirism, `BIpv`).
    pub damage_int: i32,
    /// The life steal fraction of the unit's attacks while the buff exists (`BIpv`; read by `item_abilities::on_hit`).
    pub life_steal: F32,
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
            miss_chance: F32::ZERO,
            root: false,
            morph: false,
            invuln: false,
            damage_flat: F32::ZERO,
            damage_int: 0,
            life_steal: F32::ZERO,
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
        if m.root {
            *self.spells.unit.rooted.entry(unit).or_insert(0) += 1;
        }
        if m.invuln {
            self.unit_mut(unit).invuln_count += 1;
        }
        if nonzero(m.damage_flat) {
            let u = self.unit_mut(unit);
            u.hero_attack = arith::add(u.hero_attack, m.damage_flat);
        }
        if m.damage_int != 0 {
            self.add_damage(unit, m.damage_int);
        }
    }

    /// Whether `unit` is rooted by a net (Ensnare, Web): its mover is halted while the count is above zero.
    pub fn is_rooted(&self, unit: ObjKey) -> bool {
        self.spells.unit.rooted.get(&unit).is_some_and(|&n| n > 0)
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
            miss_chance: F32::ZERO,
            root: false,
            morph: false,
            invuln: false,
            damage_flat: arith::neg(m.damage_flat),
            damage_int: m.damage_int.wrapping_neg(),
            life_steal: F32::ZERO,
        };
        self.apply_mods(unit, &neg);
        if m.invuln {
            let u = self.unit_mut(unit);
            u.invuln_count = (u.invuln_count - 1).max(0);
        }
        if m.morph {
            self.restore_morph(unit);
        }
        if m.root {
            if let Some(n) = self.spells.unit.rooted.get_mut(&unit) {
                *n = n.saturating_sub(1);
            }
        }
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

/// One over-time effect of a buff: damage (`hot` false) or healing per tick.
#[derive(Debug, Clone, Copy)]
struct TickRec {
    source: ObjKey,
    amount: F32,
    hot: bool,
    damage_type: u32,
    timer: Option<TimerId>,
}

/// What a polymorphed unit was: its type, its classification word and the abilities taken off (the sheep has none of
/// them: the original replaces the ability set with the new type's).
#[derive(Debug, Clone)]
struct MorphRec {
    orig_type: u32,
    orig_class: u32,
    abilities: Vec<crate::abilities::Ability>,
}

/// One Lightning Shield: the caster (damage source), the per-tick damage (`DataA / 2`), the tick period (0.5 s), the
/// damage radius (`Area`) and the pending tick timer.
#[derive(Debug, Clone, Copy)]
struct ShieldRec {
    source: ObjKey,
    amount: F32,
    period: F32,
    area: F32,
    timer: Option<TimerId>,
}

/// The decaying slow of one Purge buff (the fields of `CBuffPurge`): the current slow magnitudes of the movement and
/// attack speed sinks (`+0xDC`, `+0xE4`), their steps (`+0xEC`, `+0xF4`, the soft reciprocal of `DataA` / `DataB`) and the
/// repeating update timers.
#[derive(Debug, Clone, Copy)]
struct PurgeRec {
    move_value: F32,
    move_step: F32,
    attack_value: F32,
    attack_step: F32,
    move_timer: Option<TimerId>,
    attack_timer: Option<TimerId>,
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
    /// Damage / heal over time buffs by buff serial.
    ticks: BTreeMap<u32, TickRec>,
    /// The second unit type of Raise Dead (0 = the skeletal mage).
    pub raise_dead_second: u32,
    /// Misc `InvulnSummonDispelDamage` (read by the dispel worker; a missing key reads 0).
    pub invuln_summon_dispel_damage: bool,
    /// The decaying slows of Purge buffs by buff serial.
    purge: BTreeMap<u32, PurgeRec>,
    /// Lightning Shield buffs by serial.
    shield: BTreeMap<u32, ShieldRec>,
    /// Polymorphed units: what they were.
    morphs: BTreeMap<ObjKey, MorphRec>,
    /// Casts that the autocast scan started and whose effect has not run yet, by (caster, ability).
    auto_started: std::collections::BTreeSet<(ObjKey, u32)>,
    /// The sheep types by movement class (0 = [`SHEEP`]).
    pub sheep: [u32; 4],
    /// Misc `FrostMoveSpeedDecrease` / `FrostAttackSpeedDecrease` (None = the 1.26a data values 0.5 / 0.25).
    pub frost_move_decrease: Option<F32>,
    pub frost_attack_decrease: Option<F32>,
    /// Root counts (Ensnare, Web) by unit: a rooted unit's mover is halted.
    rooted: BTreeMap<ObjKey, u32>,
    /// Nets in flight by id (the timer's `aux`): (caster, target, base, the level record at the cast).
    nets: BTreeMap<u32, (ObjKey, ObjKey, u32, crate::cast::SpellLevel)>,
    next_net: u32,
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
    if let Some(r) = crate::unit_abilities_a::immediate_order(game, unit, id) {
        return Some(r);
    }
    if let Some(r) = crate::neutral_spells::toggle_order(game, unit, id) {
        return Some(r);
    }
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

/// The `targs` word of an ability level from the editor's token list (`AbilityData.slk` column `targs`). Bit values
/// are those of the weapon predicate (`target_accept.rs`: kinds `0x2..0x400`, relations `0x1000..0x10000`, vulnerable
/// `0x100000`, invulnerable `0x200000`, hero/nonhero, alive/dead, organic/mechanical, sapper, ancient). `friend`
/// stands for player + ally (LIKELY: the editor's friend group; a unit of the caster's own player is a friend).
pub fn targs_word(list: &str) -> u32 {
    let mut w = 0u32;
    for tok in list.split(',') {
        w |= match tok.trim() {
            "ground" => 0x2,
            "air" => 0x4,
            "structure" => 0x8,
            "ward" => 0x10,
            "item" => 0x20,
            "tree" => 0x40,
            "wall" => 0x80,
            "debris" => 0x100,
            "decoration" => 0x200,
            "bridge" => 0x400,
            "self" => 0x1000,
            "player" => 0x2000,
            "ally" => 0x4000,
            "friend" => 0x6000,
            "neutral" => 0x8000,
            "enemy" => 0x1_0000,
            "vuln" => 0x10_0000,
            "invu" => 0x20_0000,
            "hero" => 0x40_0000,
            "nonhero" => 0x80_0000,
            "alive" => 0x100_0000,
            "dead" => 0x200_0000,
            "organic" => 0x400_0000,
            "mechanical" => 0x800_0000,
            "nonsapper" => 0x1000_0000,
            "sapper" => 0x2000_0000,
            "nonancient" => 0x4000_0000,
            "ancient" => 0x8000_0000,
            "none" => 0x1,
            _ => 0,
        };
    }
    w
}

/// Whether `caster` may target `target` with the `targs` word `w`: the weapon predicate of S32 (`target_accept::predicate`,
/// CONFIRMED by Unicorn: kinds, alive/dead, vulnerable, organic/mechanical, hero, sapper, ancient, with the unnamed
/// groups widened by `expand`) plus the relation stage of `0x6F25D390` (structure CONFIRMED, spec `weapon-target.md`
/// section 3: self, same player, then the passive alliance row decides enemy against ally/neutral; the shared-spells
/// row that splits ally from neutral is not in the model, so a non-enemy of another player is an ally, and a
/// neutral-hostile owner (index 12 and up) that the passive row calls non-enemy a neutral: UNSURE).
pub fn targs_ok(gm: &Game, caster: ObjKey, target: ObjKey, w: u32) -> bool {
    if crate::target_accept::predicate(gm.target_flag_word(target), w) != 0 {
        return false;
    }
    if w & 0x1_F000 == 0 {
        return true;
    }
    let (co, to) = (gm.unit(caster).owner, gm.unit(target).owner);
    let rel = if caster == target {
        0x1000
    } else if co == to {
        0x2000
    } else if gm.is_enemy(co, to) {
        0x1_0000
    } else if to >= 12 {
        0x8000
    } else {
        0x4000
    };
    w & rel != 0
}

/// The deficit rule of Heal's slot `0x3F0`: `max life - life >= 1.0` (the error `0x1C` otherwise). A NaN deficit
/// passes (the unordered compare).
fn heal_needs(gm: &Game, target: ObjKey) -> bool {
    let u = gm.unit(target);
    let deficit = arith::sub(u.max_life, u.life);
    !arith::lt(deficit, F32::ONE)
}

/// Errors of a cast without a target (the readiness error "no corpse" of Raise Dead).
pub(crate) fn untargeted_error(gm: &Game, caster: ObjKey, base: u32, level: &crate::cast::SpellLevel) -> Option<CastError> {
    match base {
        RAISE_DEAD if pick_corpse(gm, caster, level.area).is_none() => Some(CastError::BadTarget),
        _ => None,
    }
}

/// Target validity beyond the range test (`0x6F04F7A0` + class slot `0x3F0`).
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    if gm.is_polymorphed(caster) {
        // A sheep has none of the unit's abilities.
        return Some(CastError::Stunned);
    }
    match base {
        POLYMORPH => {
            // targs: air,ground,nonhero,enemy,organic,neutral; slot 0x3F0 (`0x6F11C230`): a summoned unit is refused
            // (error 0x9F); the creep level rule (error 0x33) is not modelled.
            let w = targs_word("air,ground,nonhero,enemy,organic,neutral");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            if gm.unit(target).summoned {
                return Some(CastError::BadTarget);
            }
            None
        }
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
        REJUVENATION => {
            let t = gm.unit(target);
            // targs: air,ground,friend,vuln,invu,self,organic,neutral.
            if !(is_ally(gm, caster, target) || t.owner >= 12)
                || t.removed
                || is_unit_type(gm, t, ty::STRUCTURE)
                || is_unit_type(gm, t, ty::MECHANICAL)
            {
                return Some(CastError::BadTarget);
            }
            None
        }
        UNHOLY_FRENZY => {
            let t = gm.unit(target);
            // targs: air,ground,organic (any player, no structures, no mechanical units).
            if t.removed
                || is_unit_type(gm, t, ty::STRUCTURE)
                || is_unit_type(gm, t, ty::MECHANICAL)
                || gm.unit_is_invulnerable(target)
            {
                return Some(CastError::BadTarget);
            }
            None
        }
        CRIPPLE | FAERIE_FIRE => {
            let t = gm.unit(target);
            // Cripple targs: air,ground,enemy,organic,neutral; Faerie Fire: air,ground,enemy,neutral.
            let hostile = gm.is_enemy(gm.unit(caster).owner, t.owner) || t.owner >= 12;
            if !hostile
                || t.removed
                || is_unit_type(gm, t, ty::STRUCTURE)
                || (base == CRIPPLE && is_unit_type(gm, t, ty::MECHANICAL))
                || gm.unit_is_invulnerable(target)
            {
                return Some(CastError::BadTarget);
            }
            None
        }
        ENSNARE | WEB => {
            // targs: Ensnare ground,air,enemy,neutral; Web air,enemy,neutral. Extra rule (slot 0x3F0): the target needs a
            // mover (error 0x69), modelled as "not a structure and has a movement type".
            let w = targs_word(if base == WEB { "air,enemy,neutral" } else { "ground,air,enemy,neutral" });
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            if is_unit_type(gm, gm.unit(target), ty::STRUCTURE) {
                return Some(CastError::BadTarget);
            }
            None
        }
        LIGHTNING_SHIELD => {
            // targs: ground,friend,enemy,neutral (any ground unit, no structures: the kind group names only ground).
            let w = targs_word("ground,friend,enemy,neutral");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            None
        }
        ABOLISH_MAGIC => {
            // targs: air,ground,ward,invu,vuln,tree (no relation). The extra slot `0x3F4` (`0x6F0E2B60`) wants bit `0x80`
            // of a flag word of its argument: not decoded (UNSURE), not modelled.
            let w = targs_word("air,ground,ward,invu,vuln,tree");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            None
        }
        PURGE => {
            // targs: air,ground,ward,vuln,invu,tree (no relation: friends can be purged too).
            let w = targs_word("air,ground,ward,vuln,invu,tree");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            None
        }
        CURSE => {
            // targs: air,ground,enemy,organic,neutral (`AbilityData.slk`, S32 predicate + relation stage).
            let w = targs_word("air,ground,enemy,organic,neutral");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            None
        }
        _ => crate::neutral_spells::target_error(gm, caster, base, target)
            .or_else(|| crate::invisibility::target_error(gm, caster, base, target)),
    }
}

/// The class effects of this module. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        HEAL => {
            if let CastTarget::Unit(t) = rec.target {
                heal(game, rec, t);
            }
            true
        }
        UNHOLY_FRENZY => {
            if let CastTarget::Unit(t) = rec.target {
                unholy_frenzy(game, rec, t);
            }
            true
        }
        REJUVENATION => {
            if let CastTarget::Unit(t) = rec.target {
                rejuvenation(game, rec, t);
            }
            true
        }
        DISPEL_MAGIC => {
            dispel_magic(game, rt, rec);
            true
        }
        RAISE_DEAD => {
            raise_dead(game, rt, rec);
            true
        }
        ROAR => {
            roar(game, rec);
            true
        }
        INNER_FIRE | BLOODLUST | CRIPPLE | FAERIE_FIRE => {
            if let CastTarget::Unit(t) = rec.target {
                buff_spell(game, rec, t);
            }
            true
        }
        CURSE => {
            if let CastTarget::Unit(t) = rec.target {
                curse(game, rec, t);
            }
            true
        }
        PURGE => {
            if let CastTarget::Unit(t) = rec.target {
                purge(game, rt, rec, t);
            }
            true
        }
        LIGHTNING_SHIELD => {
            if let CastTarget::Unit(t) = rec.target {
                lightning_shield(game, rec, t);
            }
            true
        }
        ENSNARE | WEB => {
            if let CastTarget::Unit(t) = rec.target {
                net_spell(game, rec, t);
            }
            true
        }
        POLYMORPH => {
            if let CastTarget::Unit(t) = rec.target {
                polymorph(game, rt, rec, t);
            }
            true
        }
        HEALING_WARD => {
            ward(game, rec, BUFF_HEALING_WARD);
            true
        }
        STASIS_TRAP => {
            ward(game, rec, BUFF_STASIS_TRAP);
            true
        }
        SENTRY_WARD => {
            ward(game, rec, BUFF_SENTRY_WARD);
            true
        }
        ABOLISH_MAGIC => {
            if let CastTarget::Unit(t) = rec.target {
                abolish_magic(game, rt, rec, t);
            }
            true
        }
        _ => false,
    }
}

/// Unholy Frenzy (`0x6F18C070`, CONFIRMED structure): duration by the hero rule, `DataA` and `DataB` into the buff apply
/// `0x6F18BF60` (the buff is found by its code, `0x6F158DB0`, and re-applied through slot `0x37C`, else created).
/// `CBuffUnholyFrenzy` is a `CBuffDoT`: attack speed sink with `DataA` (editor: attack speed bonus) and a
/// damage-over-time tick of `DataB` (editor: damage per second) per period of 1.0 s, first tick 0.01 s after the
/// apply, damage class `0x40000` (death), source the caster. The tick is skipped while the target carries the buff
/// `BUsl` (`0x6F1D6000`; not modelled) or under the magic immunity rules of `MagicImmunesResistDamage` (`0x6F0D54B0`).
fn unholy_frenzy(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let d = &rec.level.data;
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_UNHOLY_FRENZY)
    };
    let mods = UnitMods {
        attack_delta: d[0],
        ..UnitMods::default()
    };
    let serial = gm.apply_mods_buff(target, BUFF_UNHOLY_FRENZY, preds, mods, dur);
    gm.register_tick(target, serial, rec.caster, d[1], false, DAMAGE_TYPE_DEATH, true);
}

/// Raise Dead (`Arai`, `CAbilityRaiseDead` vtable `0x6F8CB89C`; effect slot `0x424` = `0x6F0E6AA0`). CONFIRMED structure
/// (S38, re-read: the earlier reading "one corpse per unit" was wrong): the effect takes ONE corpse `c` (its argument,
/// chosen by the autocast search: filter slot `0x450`, comparator slot `0x458` = `0x6F0F5950`, which prefers a dead
/// candidate over a living one and otherwise falls back to the shared comparator `0x6F023400`), reads `a = f2i(DataA)`
/// and `b = f2i(DataB)`, the two unit types (`0x6F0E69E0`: `UnitID`, `0x6F0E6A40`: the second type, `DataD` in the
/// editor), the duration by the hero rule applied to the CORPSE (`HeroDur` when the corpse was a hero, else `Dur` +
/// ability `+0x9C`), makes `a` units of the first type and `b` of the second FROM THAT CORPSE
/// (`0x6F0DE9C0(ability, corpse, type, dur != 0)`; each is tied to the caster by `0x6F1D2600`, notified, given the
/// timed life `Brai` through slot `0x484` and registered), then removes the corpse (`0x6F2964E0`, event `0xD015A`,
/// `0x6F29ED60`). Level 1 therefore raises two skeleton warriors from one corpse, level 2 a warrior and a mage.
/// Afterwards the unit cap runs: `existing = slot 0x47C` (`0x6F0E6CC0`: the player's typed counts of both types,
/// flag `0x12`), `limit = slot 0x480` (`0x6F0E6D20`: the player's tech `max allowed` of `UnitID`, `INT_MAX` when never
/// set by `SetPlayerTechMaxAllowed`); when `limit - existing < 0` slot `0x488` (`0x6F0FBB30`) kills that many of the
/// raised units, ordered by a key (the remaining timed life of `Brai` times a constant, else 500; the sort and the
/// end it takes from are UNSURE: the model kills the oldest). The readiness slot `0x380` (`0x6F0E69A0`) and the
/// autocast gate `0x3D8` require `a + b + existing <= limit` (slot `0x478`) when the ability flag `0x200000` is set.
/// UNSURE stand-ins: the corpse choice (the model picks by the shared comparator over the circle query of radius
/// `Area`, which keeps the farthest of two corpses in range), the placement (all units at the corpse), the second type
/// (`State::raise_dead_second`, default the skeletal mage).
fn raise_dead(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let Some(corpse) = pick_corpse(&gm, rec.caster, rec.level.area) else {
            return;
        };
        let a = conv::f2i(rec.level.data[0]).max(0) as usize;
        let b = conv::f2i(rec.level.data[1]).max(0) as usize;
        let hero = gm.target_hero(corpse);
        let dur = effect_duration(&rec.level, hero, F32::ZERO);
        let owner = gm.unit(rec.caster).owner;
        let first = rec.level.unit_id;
        let second = if gm.spells.unit.raise_dead_second != 0 { gm.spells.unit.raise_dead_second } else { SKELETAL_MAGE };
        let (x, y, facing) = {
            let u = gm.unit(corpse);
            (u.x, u.y, u.facing)
        };
        crate::natives::unit::remove_unit(&mut gm, corpse);
        for (n, ty_id) in [(a, first), (b, second)] {
            if ty_id == 0 {
                continue;
            }
            for _ in 0..n {
                if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, ty_id, x, y, facing, true) {
                    gm.unit_mut(k).summoned = true;
                    gm.apply_timed_life(k, BUFF_RAISE_DEAD, dur);
                }
            }
        }
        // The unit cap: kill the surplus (oldest first).
        let limit = gm.player_tech_max_allowed(owner, first);
        let existing = raised_count(&gm, owner, first, second);
        if let Some(extra) = i64::from(existing).checked_sub(i64::from(limit)).filter(|&e| e > 0) {
            let mut mine: Vec<ObjKey> = (0..gm.object_count())
                .map(|i| ObjKey(i as u32))
                .filter(|&k| {
                    matches!(gm.object(k), crate::natives::game::Object::Unit(u)
                        if u.owner == owner && (u.type_id == first || u.type_id == second) && !u.dead && !u.removed && u.summoned)
                })
                .collect();
            mine.sort();
            for k in mine.into_iter().take(extra as usize) {
                if crate::natives::set_life(&mut gm, k, F32::ZERO) {
                    killed.push(k);
                }
            }
        }
    }
    for t in killed {
        crate::natives::dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// The player's living units of the two raised types (slot `0x47C`, `0x6F0E6CC0`: the typed counts of both types, the
/// second only when it differs from the first).
fn raised_count(gm: &Game, owner: u32, first: u32, second: u32) -> i32 {
    let n = |ty: u32| -> i32 {
        (0..gm.object_count())
            .filter(|&i| {
                matches!(gm.object(ObjKey(i as u32)), crate::natives::game::Object::Unit(u)
                    if u.owner == owner && u.type_id == ty && !u.dead && !u.removed)
            })
            .count() as i32
    };
    if first == second {
        n(first)
    } else {
        n(first) + n(second)
    }
}

/// Slot `0x478` (`0x6F0E6930`): whether raising `a + b` more units stays within the cap.
fn raise_dead_cap_ok(gm: &Game, caster: ObjKey, level: &crate::cast::SpellLevel) -> bool {
    let owner = gm.unit(caster).owner;
    let a = conv::f2i(level.data[0]);
    let b = conv::f2i(level.data[1]);
    let second = if gm.spells.unit.raise_dead_second != 0 { gm.spells.unit.raise_dead_second } else { SKELETAL_MAGE };
    let existing = raised_count(gm, owner, level.unit_id, second);
    let limit = gm.player_tech_max_allowed(owner, level.unit_id);
    i64::from(a) + i64::from(b) + i64::from(existing) <= i64::from(limit)
}

/// The corpse the effect gets: the circle query around the caster (radius `Area`), the corpses among them, the shared
/// comparator (`default_prefers`, all in range).
fn pick_corpse(gm: &Game, caster: ObjKey, area: F32) -> Option<ObjKey> {
    let t = arith::mul(area, area);
    let mut best: Option<(ObjKey, F32)> = None;
    let (cx, cy) = (gm.unit(caster).x, gm.unit(caster).y);
    for k in crate::natives::group::circle_candidates(gm, cx, cy, area) {
        if k == caster || !corpse_ok(gm, k) {
            continue;
        }
        let v = dist2(gm, caster, k);
        best = match best {
            None => Some((k, v)),
            Some((b, vb)) => {
                if raise_dead_prefers(true, true, v, vb, t) {
                    Some((k, v))
                } else {
                    Some((b, vb))
                }
            }
        };
    }
    best.map(|(k, _)| k)
}

fn corpse_ok(gm: &Game, k: ObjKey) -> bool {
    let u = gm.unit(k);
    u.dead && !u.removed && !u.illusion && !is_unit_type(gm, u, ty::STRUCTURE) && !is_unit_type(gm, u, ty::MECHANICAL)
}

/// Roar (`Aroa`, `CAbilityRoar` vtable `0x6F8A21BC`; instant effect slot `0x3C0` = `0x6F1EED60`, readiness `0x6F1EEBF0`):
/// every friendly ground or air unit (not structures) within `Area` of the caster, the caster included, gets the buff
/// `Broa` for the hero-rule duration. `CBuffRoar` shares the Inner Fire buff base (armor sink and the attack record
/// modifiers): `DataA` damage fraction, `DataB` armor, `DataC` life regeneration (editor labels ROA1..3). The group
/// order is the circle query order (LIKELY). The 1.26a data of the later fields (mana regeneration, prefer hostiles,
/// max units) is not modelled.
fn roar(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (cx, cy, owner) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner)
    };
    let d = &rec.level.data;
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_ROAR)
    };
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        if !gm.alive(k) || gm.unit(k).hidden {
            continue;
        }
        let u = gm.unit(k);
        let ally = k == rec.caster || u.owner == owner || !gm.is_enemy(owner, u.owner);
        if !ally || is_unit_type(&gm, u, ty::STRUCTURE) {
            continue;
        }
        let hero = gm.target_hero(k);
        let dur = effect_duration(&rec.level, hero, F32::ZERO);
        if !arith::lt(F32::ZERO, dur) {
            continue;
        }
        let mods = UnitMods {
            damage_pct: d[0],
            armor: d[1],
            life_regen: d[2],
            ..UnitMods::default()
        };
        gm.apply_mods_buff(k, BUFF_ROAR, preds, mods, dur);
    }
}

/// The inputs of the per-unit dispel worker `0x6F0EEF00` (shared by Dispel Magic, Purge and Abolish Magic). Every field
/// is a value the original reads from the target, the caster or a callee; the control flow is CONFIRMED by Unicorn
/// (`tools/emu/dispel_worker_emu.py`, 1,600 random cases over four seeds, calls recorded and compared with
/// `crates/sim/tests/dispel_worker_vectors.rs`).
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkerIn {
    /// The player argument handed to the relation test.
    pub player: u32,
    /// `0x6F285B20(target, player)`: the passive alliance row has NO bit for the target's owner (an enemy).
    pub rel: u32,
    /// `0x6F0787D0(target, 'Bams')`: the target carries Anti-magic Shell.
    pub bams: u32,
    /// Target `+0x248 & 4` (summoned) and `+0x5C & 0x40000000` (illusion).
    pub summoned: bool,
    pub illusion: bool,
    /// The target's virtual slot `0x11C` (nonzero: protected, taken as magic immune).
    pub v11c: u32,
    /// `0x6F28D750(target, a2, ability)`: nonzero when the target is blocked from the ability.
    pub blocked: u32,
    /// Misc `InvulnSummonDispelDamage` (read with the default `ebx`, 0).
    pub misc: u32,
    pub a2: u32,
    pub ability: u32,
    /// Arguments 4..7: remove positive buffs, remove negative buffs, two pass-through words of the buff remover.
    pub positive: u32,
    pub negative: u32,
    pub a6: u32,
    pub a7: u32,
    /// The summoned-unit damage (bits of the float the pointer argument 8 points at) and the mana loss (argument 9).
    pub damage_bits: u32,
    pub mana_bits: u32,
    /// Argument 10 (the damage record flags) and the target's `+0x20 & 8`.
    pub flags: u32,
    pub target_flag8: bool,
    /// The source handed to the damage record and the buff remover (the ability's owner).
    pub source: u32,
}

/// One call the worker makes, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStep {
    Rel(u32),
    FindBams,
    Slot11c,
    Blocked(u32, u32),
    Misc(u32),
    /// A damage record `(amount bits, class, source, flags)` and its application through the target slot `0x120`.
    Damage(u32, u32, u32, u32),
    /// The buff remover `0x6F079FE0` with its nine arguments.
    Buffs([u32; 9]),
    Mana(u32),
    Notify(u32),
}

/// The control flow of the worker `0x6F0EEF00` (CONFIRMED by Unicorn): the ordered calls and the return value.
///
/// 1. `rel = relation(target, player)`; `bams = find buff 'Bams'`.
/// 2. `protected = !illusion && (slot0x11C != 0 || blocked(a2, ability) != 0)` (the two calls are made in that order and
///    only for a non-illusion; the first nonzero one ends the chain).
/// 3. With `rel != 0`: a summoned or illusion target that has no `Bams` and is not protected takes the SUMMONED damage
///    record (class `0x4000`, amount argument 8, flags argument 10 with `0x20` added when the Misc flag
///    `InvulnSummonDispelDamage` is set), any other target takes the DISPEL record (amount 0.0, class 0, flags argument 10
///    with `0x10` added). The record is applied by the target's slot `0x120`.
/// 4. A protected target returns 1 after the record (or at once without a record when `rel == 0`).
/// 5. Otherwise the buff remover runs: both flags set: one call (positive, negative); exactly one set: one call with
///    that one (positive first); neither: none. Arguments `(0, 1, 0, 0, a6, positive, negative, source, a7)`.
/// 6. Unless the target's `+0x20 & 8`: without `Bams` a nonzero (or NaN) mana loss is applied, and a non-null ability
///    is notified of the target. Returns 0.
pub fn dispel_worker(i: &WorkerIn) -> (Vec<WorkerStep>, u32) {
    let mut out = vec![WorkerStep::Rel(i.player), WorkerStep::FindBams];
    let find = i.bams != 0;
    let var40 = i.summoned || i.illusion;
    let protected = if i.illusion {
        false
    } else {
        out.push(WorkerStep::Slot11c);
        if i.v11c != 0 {
            true
        } else {
            out.push(WorkerStep::Blocked(i.a2, i.ability));
            i.blocked != 0
        }
    };
    if i.rel != 0 {
        if !find && var40 && !protected {
            out.push(WorkerStep::Misc(0));
            let flags = if i.misc != 0 { i.flags | 0x20 } else { i.flags };
            out.push(WorkerStep::Damage(i.damage_bits, 0x4000, i.source, flags));
        } else {
            out.push(WorkerStep::Damage(0, 0, i.source, i.flags | 0x10));
        }
    }
    if protected {
        return (out, 1);
    }
    let buffs = |p: u32, n: u32| WorkerStep::Buffs([0, 1, 0, 0, i.a6, u32::from(p != 0), u32::from(n != 0), i.source, i.a7]);
    if i.positive != 0 && i.negative != 0 {
        out.push(buffs(1, 1));
    } else if i.positive != i.negative {
        if i.positive != 0 {
            out.push(buffs(1, 0));
        }
        if i.negative != 0 {
            out.push(buffs(0, 1));
        }
    }
    if !i.target_flag8 {
        if !find {
            let m = f32::from_bits(i.mana_bits);
            // x87 compare against 0.0: unequal or unordered runs the mana loss.
            if m != 0.0 {
                out.push(WorkerStep::Mana(i.mana_bits));
            }
        }
        if i.ability != 0 {
            out.push(WorkerStep::Notify(i.ability));
        }
    }
    (out, 0)
}

/// `CBuffPurge` apply `0x6F0EF400` for one of its two schedules (CONFIRMED by Unicorn, `tools/emu/purge_apply_emu.py` and
/// `tools/emu/purge_steps_emu.py`: the real apply and update code, 600 random cases, 0 mismatches,
/// `tests/purge_vectors.rs`). `data` is `DataA` (movement) or `DataB` (attack speed): the initial slow magnitude is 1.0
/// (a full slow) when it is nonzero, else 0.0; the step is the soft reciprocal `1 / data`; the repeating update timer
/// (message `0xD01C1` movement, `0xD01C3` attack) is started only for a magnitude that is not approximately zero, with
/// the timeout `dur / data` (soft division; `dur` is the hero rule `HeroDur` / `Dur` without the `0.01` that the buff's
/// own duration adds). So the slow falls linearly to zero over `dur`: in `data` equal steps.
pub fn purge_schedule(data: F32, dur: F32, div: &dyn Fn(F32, F32) -> F32) -> (F32, F32, Option<F32>) {
    let nonzero = data.to_f32() != 0.0;
    let value = if nonzero { F32::ONE } else { F32::ZERO };
    let step = if nonzero { div(F32::ONE, data) } else { F32::ZERO };
    let timeout = if arith::approx_eq(value, F32::ZERO) { None } else { Some(div(dur, data)) };
    (value, step, timeout)
}

/// One movement update of `CBuffPurge` (`0x6F0D9EE0`, message `0xD01C1`): nothing while the slow is approximately zero
/// (`|v| < 0.001`, `0x6F0222E0`), else `new = v - step` and the movement sink receives `v - new`. Returns `(new, sink)`.
pub fn purge_move_update(value: F32, step: F32) -> Option<(F32, F32)> {
    if arith::approx_eq(value, F32::ZERO) {
        return None;
    }
    let new = arith::sub(value, step);
    Some((new, arith::sub(value, new)))
}

/// One attack speed update (`0x6F0D9F80`, message `0xD01C3`): same stop rule; the attack sink first receives `+v`
/// (the old slow is taken off), then `-new` (the reduced one is put on). Returns `(new, first, second)`.
pub fn purge_attack_update(value: F32, step: F32) -> Option<(F32, F32, F32)> {
    if arith::approx_eq(value, F32::ZERO) {
        return None;
    }
    let new = arith::sub(value, step);
    Some((new, value, arith::neg(new)))
}

impl Game {
    pub(crate) fn soft_div(&self, a: F32, b: F32) -> F32 {
        match self.order_tables() {
            Some(t) => arith::div(a, b, t),
            // Without the soft-float tables (unit tests) a plain division stands in.
            None => F32::from_bits((a.to_f32() / b.to_f32()).to_bits()),
        }
    }

    /// Removes the buff with code `base` from `unit` (the removal routine `0x6F079CC0`): timers cancelled and effects
    /// undone.
    pub(crate) fn remove_buff_code(&mut self, unit: ObjKey, base: u32) {
        if let Some(i) = self.buff_position(unit, base) {
            let mut b = self.unit_mut(unit).buffs.remove(i);
            self.finish_buff(unit, &mut b);
        }
    }
}

/// Abolish Magic (`Aadm`, `CAbilityAutoDispelMagic` vtable `0x6F8C35AC`, an AutoTargetSpell; effect slot `0x424` =
/// `0x6F0EF0F0`, CONFIRMED structure): the target goes through the dispel worker with the summoned-unit damage `DataB`
/// (300), the mana loss `DataA` and, in smart mode, positive buffs removed from enemies and negative ones from the
/// others (see [`dispel_one`]). Targets: air, ground, ward, invulnerable, vulnerable, tree (no relation). The
/// autocast filter (slot `0x450` = `0x6F0E2B90`, CONFIRMED structure): the base filter, a class word rule, then the
/// candidate must have a removable buff of the polarity its relation asks for (allies: negative, enemies: positive,
/// through the buff scan `0x6F076D00` with the same arguments as the remover).
fn abolish_magic(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let killed = {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        let a7 = u32::from(arith::lt(F32::ZERO, rec.level.data[8]));
        // Smart mode (relation-aware removal) only for a cast the autocast started (see `scan`); a manual order removes
        // both polarities like Dispel Magic (UNSURE: the ability flag `0x200000` decides in the original).
        let smart = gm.spells.unit.auto_started.remove(&(rec.caster, ABOLISH_MAGIC));
        dispel_one(&mut gm, rec.caster, target, rec.level.data[1], rec.level.data[0], smart, a7)
    };
    if let Some(t) = killed {
        crate::natives::dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// Purge (`Aprg`, `CAbilityPurge` vtable `0x6F8C46CC`, effect slot `0x424` = `0x6F107E10`, CONFIRMED flow):
/// 1. the target goes through the dispel worker (`0x6F0EEF00`, [`dispel_worker`]) with the summoned-unit damage `DataC`
///    (400) and the mana loss `DataF`;
/// 2. a dead target (`+0x5C & 0x100`) or one the caster's player is passively allied with (`0x6F285AC0`) gets nothing
///    more;
/// 3. otherwise the hero rule picks `HeroDur` / `Dur`, an existing `Bprg` buff is removed (`0x6F079CC0`, so a second
///    Purge restarts the slow) and a new one is applied (`0x6F0EF400`): the buff lasts `dur + 0.01`, starts a full slow
///    of movement (when `DataA != 0`) and attack speed (when `DataB != 0`) and decays them linearly
///    ([`purge_schedule`], [`purge_move_update`], [`purge_attack_update`]); when the buff ends its remaining slow is
///    taken off (the removal slot `0x6F0E2C70`). Pause durations `DataD` / `DataE` (0 in 1.26a data) stop the target
///    for that long (not modelled: 0 in the data). The buff is negative and not magic (CBuffPurge overrides slot 118
///    to 0 and slot 120 to 1), so it is not dispellable (LIKELY: slot 204 reuses the magic test).
fn purge(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let killed = purge_body(&mut game.borrow_mut(), rec, target);
    if let Some(t) = killed {
        crate::natives::dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// The state part of [`purge`]: the dispel worker and the slow buff; returns the unit the worker killed (the caller runs
/// the death dispatch). Shared with the Orb of Lightning (old), which fires the carrier's `AIlp` ability on a hit.
pub(crate) fn purge_body(gm: &mut Game, rec: &CastRec, target: ObjKey) -> Option<ObjKey> {
    {
        if !gm.alive(target) {
            return None;
        }
        let killed = dispel_one(gm, rec.caster, target, rec.level.data[2], rec.level.data[5], false, 1);
        if killed.is_none() && gm.alive(target) {
            let owner = gm.unit(rec.caster).owner;
            if gm.is_enemy(owner, gm.unit(target).owner) {
                let hero = gm.target_hero(target);
                let dur = if hero { rec.level.hero_dur } else { rec.level.dur };
                let div = |a: F32, b: F32| gm.soft_div(a, b);
                let (mv, mstep, mtime) = purge_schedule(rec.level.data[0], dur, &div);
                let (av, astep, atime) = purge_schedule(rec.level.data[1], dur, &div);
                gm.remove_buff_code(target, BUFF_PURGE);
                let preds = crate::abilities::BuffPredicates {
                    negative: true,
                    ..buff_predicates(BUFF_PURGE)
                };
                let mods = UnitMods {
                    move_delta: arith::neg(mv),
                    attack_delta: arith::neg(av),
                    ..UnitMods::default()
                };
                let buff_dur = arith::add(dur, F32::from_bits(MARK_EXTRA_BITS));
                let serial = gm.apply_mods_buff(target, BUFF_PURGE, preds, mods, buff_dur);
                let start = |gm: &mut Game, kind: u8, timeout: Option<F32>| {
                    timeout.map(|t| {
                        gm.timers.start(
                            t.to_f32(),
                            true,
                            Payload::Spell(SpellMsg::UnitSpell {
                                kind,
                                unit: target,
                                base: 0,
                                aux: serial,
                            }),
                        )
                    })
                };
                let move_timer = start(gm, KIND_PURGE_MOVE, mtime);
                let attack_timer = start(gm, KIND_PURGE_ATTACK, atime);
                gm.spells.unit.purge.insert(
                    serial,
                    PurgeRec {
                        move_value: mv,
                        move_step: mstep,
                        attack_value: av,
                        attack_step: astep,
                        move_timer,
                        attack_timer,
                    },
                );
            }
        }
        killed
    }
}

impl Game {
    /// Mutable access to the unit spell state (the host's Misc values: `ethereal_heal_bonus`, the frost decreases, ...).
    pub fn unit_spell_state_mut(&mut self) -> &mut State {
        &mut self.spells.unit
    }

    /// The pre-damage hook of the passive attack modifiers (called by the attack execute after the dice): Feedback
    /// (`Afbk`, `CAbilityFeedback` `0x6F8F365C`, slot `0x154` = `0x6F1721C0`, CONFIRMED structure): against a target that
    /// passes `air,ground,enemy,neutral`, `burn = min(a, target mana)` with `a` = `DataC` for a hero target and `DataA`
    /// otherwise (the hero rule `0x6F1D2660`), the target loses `burn` mana through the dispel worker's mana step
    /// (so an invulnerable target or one with `Bams` loses none: the step's conditions, CONFIRMED, see
    /// [`dispel_worker`]), and the attack's damage grows by `burn * ratio` (soft multiply, soft add), the ratio being
    /// `DataD` for a hero and `DataB` otherwise. The burn happens even for an attack that the target later evades
    /// (the hook runs when the damage is built: UNSURE order against the miss gate).
    pub(crate) fn spell_attack_bonus(&mut self, attacker: ObjKey, target: ObjKey, damage: F32) -> F32 {
        // Demolish (`neutral_misc.rs`): a damage multiplier against buildings.
        let damage = self.neutral_attack_bonus(attacker, target, damage);
        if !self.unit(attacker).abilities.contains(FEEDBACK) {
            return damage;
        }
        let Some(level) = self.ability_level_record(attacker, FEEDBACK) else {
            return damage;
        };
        let w = targs_word("air,ground,enemy,neutral");
        if !self.alive(target) || !targs_ok(self, attacker, target, w) {
            return damage;
        }
        let hero = self.target_hero(target);
        let a = if hero { level.data[2] } else { level.data[0] };
        let ratio = if hero { level.data[3] } else { level.data[1] };
        let mana = self.unit(target).mana;
        let burned = if arith::lt(mana, a) { mana } else { a };
        let bams = self.buff_position(target, BUFF_ANTI_MAGIC_SHELL).is_some();
        if !self.unit_is_invulnerable(target) && !bams && burned.to_f32() != 0.0 {
            let next = if arith::lt(mana, burned) { F32::ZERO } else { arith::sub(mana, burned) };
            self.write_mana(target, next);
        }
        arith::add(damage, arith::mul(ratio, burned))
    }

    /// The post-hit hook of the passive on-hit buffs (called by the attack arrival for a target that survived): Frost
    /// Attack (`Afra`, `Bfro`) slows the target by the Misc decreases for `HeroDur` / `Dur`; Slow Poison (`Aspo`, `Bspo`)
    /// poisons an organic target: damage `DataA` per second (class 23), movement `-DataB`, attack speed `-DataC`.
    pub(crate) fn spell_on_hit(&mut self, attacker: ObjKey, target: ObjKey) {
        if !self.alive(target) {
            return;
        }
        if self.unit(attacker).abilities.contains(FROST_ATTACK) {
            self.frost_hit(attacker, target);
        }
        if self.unit(attacker).abilities.contains(SLOW_POISON) {
            self.poison_hit(attacker, target);
        }
        self.neutral_on_hit(attacker, target);
        self.m_on_hit(attacker, target);
    }

    /// Frost Attack (CONFIRMED structure: `0x6F1837C0` creates the effect from the ability's `Dur` / `HeroDur`; the buff
    /// reads the Misc keys `FrostMoveSpeedDecrease` and `FrostAttackSpeedDecrease`, `0x6F199EA0`, and applies them
    /// negated to the movement and attack speed sinks). The buff is negative and not magic (slots `0x1D8` = 0,
    /// `0x1E0` = 1), so it is not dispellable. targs `air,ground`.
    fn frost_hit(&mut self, attacker: ObjKey, target: ObjKey) {
        let Some(level) = self.ability_level_record(attacker, FROST_ATTACK) else {
            return;
        };
        if !targs_ok(self, attacker, target, targs_word("air,ground")) {
            return;
        }
        self.frost_apply(target, &level);
    }

    /// The Frost buff `Bfro` on `target` for the level's `HeroDur` / `Dur` (the body of Frost Attack, shared with the
    /// Orb of Frost, whose post-hit handler `0x6F108FC0` hands the same buff the same duration rule).
    pub(crate) fn frost_apply(&mut self, target: ObjKey, level: &crate::cast::SpellLevel) {
        let hero = self.target_hero(target);
        let dur = if hero { level.hero_dur } else { level.dur };
        if !arith::lt(F32::ZERO, dur) {
            return;
        }
        let mv = self.spells.unit.frost_move_decrease.unwrap_or(F32::from_bits(FROST_MOVE_DEFAULT.to_bits()));
        let at = self.spells.unit.frost_attack_decrease.unwrap_or(F32::from_bits(FROST_ATTACK_DEFAULT.to_bits()));
        let mods = UnitMods {
            move_delta: arith::neg(mv),
            attack_delta: arith::neg(at),
            ..UnitMods::default()
        };
        let preds = crate::abilities::BuffPredicates {
            negative: true,
            ..buff_predicates(BUFF_FROST)
        };
        self.apply_mods_buff(target, BUFF_FROST, preds, mods, dur);
    }

    /// Slow Poison (`CBuffSlowPoison` `0x6F8D5E94` over `CBuffPoisonAttack` / `CBuffLifeDoT` / `CBuffDoT`): the damage
    /// over time of `CBuffDoT` (first tick 0.01 s, then every 1.0 s) with the class word `0x800000`; the apply `0x6F128C20`
    /// takes `-DataB` off the movement sink and `-DataC` off the attack speed sink; the buff is negative and physical,
    /// not magic (slots `0x1D8` = 0, `0x1DC` = 1, `0x1E0` = 1). Stacking (`DataD`, the `Bssd` variant) is not modelled.
    /// targs `air,ground,organic`.
    fn poison_hit(&mut self, attacker: ObjKey, target: ObjKey) {
        let Some(level) = self.ability_level_record(attacker, SLOW_POISON) else {
            return;
        };
        if !targs_ok(self, attacker, target, targs_word("air,ground,organic")) {
            return;
        }
        let hero = self.target_hero(target);
        let dur = if hero { level.hero_dur } else { level.dur };
        if !arith::lt(F32::ZERO, dur) {
            return;
        }
        let preds = crate::abilities::BuffPredicates {
            negative: true,
            physical: true,
            ..buff_predicates(BUFF_SLOW_POISON)
        };
        let mods = UnitMods {
            move_delta: arith::neg(level.data[1]),
            attack_delta: arith::neg(level.data[2]),
            ..UnitMods::default()
        };
        let serial = self.apply_mods_buff(target, BUFF_SLOW_POISON, preds, mods, dur);
        self.register_tick(target, serial, attacker, level.data[0], false, DAMAGE_TYPE_SLOW_POISON, true);
    }
}

/// The ward spells (`Ahwd` `CAbilityHealingWard` `0x6F8B0A44`, `Asta` `CAbilityStasisTrap` `0x6F929174`, `Aeye`
/// `CAbilityEvilEye` `0x6F8A9B0C`; the point effect slot `0x418` = `0x6F0BABB0` / `0x6F1FD180` / `0x6F0B8510`).
/// CONFIRMED structure of the healing ward effect: the ability's `UnitID` is created at the target point for the
/// caster's player with the creation flags `0x2` when the duration `Dur + ability[+0x9C]` is nonzero (`0x502` when it
/// is zero, which adds the food bits: a ward does not use food), the facing is a constant fraction (`0x6F020D30` times
/// `0x6FAAE5E8` = pi / 180; its input was not decoded, the model keeps the caster's facing), and the timed life buff
/// of the ward type (`0x6F07AF00`, `Bhwd`) is given the duration `Dur` through its slots `0x324` / `0x31C`. The ward's
/// own aura or trap ability comes from the unit type's ability list (the unit builder adds it): not part of the spell.
/// Level fields used here: `UnitID`, `Dur`. Stasis Trap's `HeroDur` (2.5) and `DataA..D` and the sentry's data belong to
/// the ward unit's abilities.
fn ward(game: &RefCell<Game>, rec: &CastRec, buff: u32) {
    let mut gm = game.borrow_mut();
    let (x, y) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => (gm.unit(rec.caster).x, gm.unit(rec.caster).y),
    };
    let ty = rec.level.unit_id;
    if ty == 0 {
        return;
    }
    let (owner, facing) = {
        let c = gm.unit(rec.caster);
        (c.owner, c.facing)
    };
    if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, ty, x, y, facing, true) {
        gm.unit_mut(k).summoned = true;
        if arith::lt(F32::ZERO, rec.level.dur) {
            gm.apply_timed_life(k, buff, rec.level.dur);
        }
    }
}

/// Polymorph (`Aply`, `CAbilityPolymorph` vtable `0x6F8A922C`, effect slot `0x424` = `0x6F12F560`). CONFIRMED structure:
/// 1. an illusion target is killed (the target's slot `0x124` with a zero amount) and nothing else happens;
/// 2. otherwise the hero rule gives the duration (`HeroDur` / `Dur`), the type pick `0x6F12DAA0` reads the target's
///    movement class (`+0x1FC`: 2 = fly takes the list of `DataC`, `0x10` the list of `DataE`, `0x20` the list of
///    `DataD`, anything else `DataB`; the data rows are the sheep `nshe`, `nshf`, `nsha`, `nshw`), splits the list at
///    commas and draws ONE die on combat... stream `0x18` (24) through the roll wrapper `0x6F285350` (`count 1`, `sides`
///    = the number of valid types, `bonus -1`, so the index is `floor(r * n / 2^32)`; the draw is made even for a
///    one-entry list), and the ability's slot `0x43C` (`0x6F13F790`) creates the buff `Bply` (`CBuffPolymorph`, a
///    progress-bar buff) with the new type and the duration;
/// 3. the target rule (slot `0x3F0` = `0x6F11C230`): `DataA` (max creep level, 5) as an integer, and a target that
///    is a creep above that level gets error `0x33`; a summoned unit (`+0x248 & 4`) gets error `0x9F`.
/// The morph itself runs inside the buff's apply (`0x6F12A0B0` -> `0x6F0D28F0`), not decoded. Model (UNSURE): the unit
/// takes the sheep type (its life fraction kept, the sheep's maximum life when the host registered the type), loses all
/// abilities but its move (so it cannot attack or cast), and gets everything back when the buff ends (expiry or dispel).
/// The creep level rule is not modelled (the model has no creep levels).
fn polymorph(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let mut killed = None;
    {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        if gm.unit(target).illusion {
            if crate::natives::set_life(&mut gm, target, F32::ZERO) {
                killed = Some(target);
            }
        } else if gm.buff_position(target, BUFF_POLYMORPH).is_some() {
            // A second Polymorph refreshes the buff by the set-duration rule (the form stays).
            let hero = gm.target_hero(target);
            let dur = effect_duration(&rec.level, hero, F32::ZERO);
            if let Some(i) = gm.buff_position(target, BUFF_POLYMORPH) {
                let serial = gm.unit(target).buffs[i].serial;
                gm.set_buff_duration(target, serial, dur);
            }
        } else {
            let hero = gm.target_hero(target);
            let dur = effect_duration(&rec.level, hero, F32::ZERO);
            if !arith::lt(F32::ZERO, dur) {
                return;
            }
            let movement = gm
                .unit_types()
                .get(gm.unit(target).type_id)
                .map_or(0, |t| t.movement);
            let class = match movement {
                2 => 1,
                0x10 => 3,
                0x20 => 2,
                _ => 0,
            };
            let sheep = if gm.spells.unit.sheep[class] != 0 { gm.spells.unit.sheep[class] } else { SHEEP[class] };
            // The type pick draws one die of one side (index 0) on stream 24 whatever the list length.
            if let Some((table, streams)) = gm.rng_parts() {
                let _ = crate::damage::roll_total(streams.stream_mut(POLYMORPH_STREAM), table, 1, 1, -1);
            }
            gm.morph_unit(target, sheep);
            let preds = crate::abilities::BuffPredicates {
                negative: true,
                magic: true,
                dispellable: true,
                ..buff_predicates(BUFF_POLYMORPH)
            };
            let mods = UnitMods {
                morph: true,
                ..UnitMods::default()
            };
            gm.apply_mods_buff(target, BUFF_POLYMORPH, preds, mods, dur);
        }
    }
    if let Some(t) = killed {
        crate::natives::dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

impl Game {
    /// Whether `unit` is polymorphed (the combat tick treats it as disarmed).
    pub fn is_polymorphed(&self, unit: ObjKey) -> bool {
        self.spells.unit.morphs.contains_key(&unit)
    }

    pub(crate) fn morph_unit(&mut self, unit: ObjKey, to: u32) {
        let (orig_type, orig_class) = {
            let u = self.unit(unit);
            (u.type_id, u.class_flags)
        };
        let frac = life_fraction(self, unit);
        let removed: Vec<crate::abilities::Ability> = {
            let codes: Vec<u32> = self
                .unit(unit)
                .abilities
                .iter()
                .map(|a| a.base)
                .filter(|&b| b != crate::abilities::MOVE)
                .collect();
            let u = self.unit_mut(unit);
            codes.into_iter().filter_map(|b| u.abilities.remove(b)).collect()
        };
        self.unit_mut(unit).type_id = to;
        if let Some(ty) = self.unit_types().get(to).cloned() {
            self.unit_mut(unit).class_flags = ty.classification;
            self.write_max_life(unit, ty.max_life);
            self.write_life(unit, arith::mul(ty.max_life, frac));
        }
        self.sync_unit_spells(unit);
        self.spells.unit.morphs.insert(
            unit,
            MorphRec {
                orig_type,
                orig_class,
                abilities: removed,
            },
        );
    }

    pub(crate) fn restore_morph(&mut self, unit: ObjKey) {
        let Some(rec) = self.spells.unit.morphs.remove(&unit) else {
            return;
        };
        let frac = life_fraction(self, unit);
        self.unit_mut(unit).type_id = rec.orig_type;
        self.unit_mut(unit).class_flags = rec.orig_class;
        if let Some(ty) = self.unit_types().get(rec.orig_type).cloned() {
            self.write_max_life(unit, ty.max_life);
            self.write_life(unit, arith::mul(ty.max_life, frac));
        }
        for a in rec.abilities {
            self.unit_mut(unit).abilities.attach(a);
        }
        self.sync_unit_spells(unit);
    }
}

/// Ensnare (`Aens`, `CAbilityEnsnare` vtable `0x6F8A5B4C`, effect slot `0x424` = `0x6F0AAB90`) and Web (`Aweb`,
/// `CAbilityWeb` `0x6F8A5F7C`, an AutoTargetSpell, effect `0x6F0AADD0`). CONFIRMED structure: the effect reads the hero
/// rule duration (`HeroDur`, else `Dur` + the ability's `+0x9C`), `DataA`, `DataB`, `DataC` and builds a net MISSILE
/// from the caster towards the target (the missile object, `0x6F07EAE0`); the buff is applied when the net arrives
/// (`CBuffEnsnareGround` / `CBuffEnsnareAir`, `Beng` / `Bena`; Web: `Bweb` / `Bwea`, `CBuffWeb` derives from
/// `CBuffEnsnareGround`). The extra target rule (slot `0x3F0`, `0x6F07F1E0` / `0x6F07F260`): the target must have a
/// mover (`+0x1EC`), else the error `0x69`. The buff (`CBuffEnsnare` slots `0x2D4/0x2D8/0x314`, apply
/// `0x6F09B6C0`) disables the target's movement (`0x6F2031D0`, the same call Purge's pause uses; it is cleared on
/// removal) and rewrites the melee weapons' attack range from `DataC` (UNSURE: the weapon kinds checked are the melee
/// ones, `0x6F0C6B30` / `0x6F0C6AE0`; not modelled). `DataA` (air unit lower duration 0.6) and `DataB` (height 200)
/// drive the descent of a flying target (not modelled). Model: the net arrives after distance / missile speed
/// (`Game::set_spell_missile_speed`, instant when none: UNSURE, the flight arithmetic is open) and roots the target
/// for the duration; the buff is negative, magic and dispellable (LIKELY). `Bens` is also a shared-vision/detected
/// buff (the ensnared unit is revealed): not modelled.
fn net_spell(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let speed = gm.spell_missile_speed_of(rec.base);
    if arith::lt(F32::ZERO, speed) {
        let (a, b) = (gm.unit(rec.caster), gm.unit(target));
        let (dx, dy) = (arith::sub(b.x, a.x), arith::sub(b.y, a.y));
        let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
        let dist = F32::from_bits(d2.to_f32().sqrt().to_bits());
        let t = gm.soft_div(dist, speed);
        gm.spells.unit.next_net += 1;
        let id = gm.spells.unit.next_net;
        gm.spells.unit.nets.insert(id, (rec.caster, target, rec.base, rec.level));
        gm.timers.start(
            t.to_f32(),
            false,
            Payload::Spell(SpellMsg::UnitSpell {
                kind: KIND_NET,
                unit: target,
                base: rec.base,
                aux: id,
            }),
        );
    } else {
        apply_net(&mut gm, rec.base, &rec.level, target);
    }
}

fn apply_net(gm: &mut Game, base: u32, level: &crate::cast::SpellLevel, target: ObjKey) {
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let flying = gm.unit_types().get(gm.unit(target).type_id).is_some_and(|t| t.movement & 2 != 0);
    let code = match (base == WEB, flying) {
        (false, true) => BUFF_ENSNARE_AIR,
        (false, false) => BUFF_ENSNARE_GROUND,
        (true, true) => BUFF_WEB_AIR,
        (true, false) => BUFF_WEB_GROUND,
    };
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(code)
    };
    let mods = UnitMods {
        root: true,
        ..UnitMods::default()
    };
    gm.apply_mods_buff(target, code, preds, mods, dur);
}

/// A net arrived.
fn net_arrives(game: &RefCell<Game>, id: u32) {
    let mut gm = game.borrow_mut();
    let Some((_, target, base, level)) = gm.spells.unit.nets.remove(&id) else {
        return;
    };
    apply_net(&mut gm, base, &level, target);
}

/// Lightning Shield (`Alsh`, `CAbilityLightningShield` vtable `0x6F8DB48C`, effect slot `0x424` = `0x6F13C690`):
/// the hero rule duration of the target, `DataA` (damage per second, 20) halved into the per-tick damage
/// (`div(DataA, 2.0)`, tick period `recip(2.0)` = 0.5 s: CONFIRMED by the operands of the soft float calls in the
/// effect), `Area` (160). The buff `Blsh` is found by its code and refreshed (slot `0x35C`: never-shorten duration,
/// the holders `+0xD0/+0xD8/+0xE0` replaced) or created (slot `0x354`); its apply slot `0x358` arms a ONE-SHOT timer
/// `0xD01C1` with `0.01 s` (the constant `0x6FAAE480`); the handler (`0x6F110190`) re-arms the timer with the period
/// and runs the tick slot `0x360` (`0x6F1249A0`): the group of units in `Area` around the shielded unit goes through
/// the per-victim slot `0x364` (`0x6F13C410`, CONFIRMED structure): the shielded unit itself is skipped, a victim
/// that is blocked from the ability (`0x6F28D750`, taken as magic immune) is skipped, the others take the effect
/// (`0x6F053040`, the damage runs through the `Blsa` area buff `CBuffLightningShieldAoe`, whose `CBuffAoe` base is
/// not decoded: UNSURE). Model: `amount` of magic damage (type 14, UNSURE class) from the caster to every living
/// unit in `Area` of the shielded unit except itself, at 0.01, 0.51, 1.01 ... s. The `Blsa` marker that keeps two
/// shields from hitting one victim in the same tick is not modelled.
fn lightning_shield(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let amount = gm.soft_div(rec.level.data[0], F32::from_bits(0x4000_0000));
    let period = gm.soft_div(F32::ONE, F32::from_bits(0x4000_0000));
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_LIGHTNING_SHIELD)
    };
    let existing = gm.buff_position(target, BUFF_LIGHTNING_SHIELD).map(|i| gm.unit(target).buffs[i].serial);
    let serial = gm.apply_mods_buff(target, BUFF_LIGHTNING_SHIELD, preds, UnitMods::default(), dur);
    if let (Some(old), true) = (existing, existing == Some(serial)) {
        if let Some(r) = gm.spells.unit.shield.get_mut(&old) {
            r.source = rec.caster;
            r.amount = amount;
            r.period = period;
            r.area = rec.level.area;
            return;
        }
    }
    let timer = gm.timers.start(
        F32::from_bits(TICK_FIRST_DELAY_BITS).to_f32(),
        false,
        Payload::Spell(SpellMsg::UnitSpell {
            kind: KIND_SHIELD,
            unit: target,
            base: 0,
            aux: serial,
        }),
    );
    gm.spells.unit.shield.insert(
        serial,
        ShieldRec {
            source: rec.caster,
            amount,
            period,
            area: rec.level.area,
            timer: Some(timer),
        },
    );
}

/// A Lightning Shield tick: damages the units around the shielded unit and re-arms the one-shot timer.
fn shield_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, serial: u32) {
    let mut killed = Vec::new();
    let source;
    {
        let mut gm = game.borrow_mut();
        let Some(mut rec) = gm.spells.unit.shield.get(&serial).copied() else {
            return;
        };
        source = rec.source;
        let alive = gm.alive(unit) && gm.unit(unit).buffs.iter().any(|b| b.serial == serial);
        if !alive {
            gm.spells.unit.shield.remove(&serial);
            return;
        }
        rec.timer = Some(gm.timers.start(
            rec.period.to_f32(),
            false,
            Payload::Spell(SpellMsg::UnitSpell {
                kind: KIND_SHIELD,
                unit,
                base: 0,
                aux: serial,
            }),
        ));
        gm.spells.unit.shield.insert(serial, rec);
        let (cx, cy) = (gm.unit(unit).x, gm.unit(unit).y);
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.area) {
            if k == unit || !gm.alive(k) || gm.unit(k).hidden || gm.unit(k).magic_immune > 0 {
                continue;
            }
            if let Some(t) = gm.unit_damage_target(rec.source, k, rec.amount, false, false, 0, DAMAGE_TYPE_MAGIC) {
                killed.push(t);
            }
        }
    }
    for t in killed {
        crate::natives::dispatch_unit_death_by(game, rt, t, Some(source));
    }
}

/// A Purge update timer fired for buff `serial` on `unit`.
fn purge_tick(game: &RefCell<Game>, kind: u8, unit: ObjKey, serial: u32) {
    let mut gm = game.borrow_mut();
    let Some(mut rec) = gm.spells.unit.purge.get(&serial).copied() else {
        return;
    };
    let alive = gm.alive(unit) && gm.unit(unit).buffs.iter().any(|b| b.serial == serial);
    if !alive {
        for t in [rec.move_timer, rec.attack_timer].into_iter().flatten() {
            gm.timers.cancel(t);
        }
        gm.spells.unit.purge.remove(&serial);
        return;
    }
    if kind == KIND_PURGE_MOVE {
        if let Some((new, sink)) = purge_move_update(rec.move_value, rec.move_step) {
            gm.add_move(unit, sink);
            rec.move_value = new;
        }
    } else if let Some((new, first, second)) = purge_attack_update(rec.attack_value, rec.attack_step) {
        gm.add_attack_speed(unit, first);
        gm.add_attack_speed(unit, second);
        rec.attack_value = new;
    }
    gm.spells.unit.purge.insert(serial, rec);
    // The removal of the buff takes the remaining slow off again: keep the recorded modifiers equal to what is applied.
    if let Some(i) = gm.unit(unit).buffs.iter().position(|b| b.serial == serial) {
        gm.unit_mut(unit).buffs[i].effect = Some(crate::auras::BuffEffect::Mods(UnitMods {
            move_delta: arith::neg(rec.move_value),
            attack_delta: arith::neg(rec.attack_value),
            ..UnitMods::default()
        }));
    }
}

/// Damage type of Dispel Magic's summoned-unit damage: the class word `0x4000` of the worker `0x6F0EEF00` is bit 14,
/// `DAMAGE_TYPE_MAGIC`.
const DAMAGE_TYPE_MAGIC: u32 = 14;

/// Dispel Magic (`Adis`, `CAbilityDispelMagic` vtable `0x6F8C3A24`; area slot `0x418` = `0x6F0EF320`, the per-unit
/// worker `0x6F0EEF00` shared with Purge and Abolish Magic). The group is the units within `Area` of the target point;
/// per unit (LIKELY reading of the worker, whose dispel runs through a damage record with the class flags `0x10` /
/// `0x4000`): an enemy loses its positive dispellable buffs, an ally (or the caster) its negative ones; an enemy that is
/// summoned or an illusion also takes `DataB` ("summoned unit damage") of magic damage from the caster. `DataA` (mana
/// loss, 0 in the data) is NOT DONE. The cast is accepted on a unit or a point.
fn dispel_magic(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let (x, y) = match rec.target {
            CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
            CastTarget::Point(x, y) => (x, y),
            CastTarget::None => (gm.unit(rec.caster).x, gm.unit(rec.caster).y),
        };
        for k in crate::natives::group::circle_candidates(&gm, x, y, rec.level.area) {
            if !gm.alive(k) || gm.unit(k).hidden {
                continue;
            }
            if let Some(t) = dispel_one(&mut gm, rec.caster, k, rec.level.data[1], rec.level.data[0], false, 1) {
                killed.push(t);
            }
        }
    }
    for t in killed {
        crate::natives::dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// One unit of a dispel: builds the worker inputs from the model and carries out the calls of
/// [`dispel_worker`] (CONFIRMED control flow). Returns the unit if the summoned-unit damage killed it.
///
/// The worker's arguments 4..7 come from the caller's stack pushes (CONFIRMED by reading the three callers):
/// Dispel Magic (`0x6F0EF210`) and Purge (`0x6F107E10`) push the constants `(positive 1, negative 1, a6 0, a7 1)`, so they
/// remove the magic buffs of BOTH polarities from every unit they touch, friend or foe; Abolish Magic (`0x6F0EF0F0`) in
/// its "smart" mode passes `(enemy, not enemy, a6 1, a7)`: positive buffs from enemies, negative ones from the others.
/// (`smart` is `Misc AbolishMagicDispelSmart` ? `DataC != 0` : ability flag bit `0x200000`; the key is in none of the
/// game's Misc files, the flag's writer was not found, the model takes smart as true for Abolish: UNSURE.) The buff
/// remover `0x6F079FE0` (9 arguments, CONFIRMED by reading) removes buffs that are magic (argument 2 = 1), not timed
/// life and not aura (arguments 3, 4 = 0), whose polarity matches (arguments 6 positive, 7 negative), and, when argument 5
/// is set, that pass the buff's slot `0x330` with the source (modelled as the dispellable predicate).
///
/// Input mapping (LIKELY): `rel` = the target's owner is an enemy of the caster's player, `bams` = the target carries
/// `Bams`, `v11c` = the target is magic immune, `target_flag8` = the target is invulnerable (the worker skips the mana
/// loss and the ability notification for it). The dispel record (amount 0, record flag `0x10`, enemies only) is applied by
/// the target's damage handler, which is not decoded: no effect in the model. The mana loss takes `min(mana, amount)`
/// (UNSURE).
pub(crate) fn dispel_one(gm: &mut Game, caster: ObjKey, k: ObjKey, damage: F32, mana: F32, smart: bool, a7: u32) -> Option<ObjKey> {
    let owner = gm.unit(caster).owner;
    let (enemy, summoned, illusion, immune) = {
        let u = gm.unit(k);
        (gm.is_enemy(owner, u.owner), u.summoned, u.illusion, u.magic_immune > 0)
    };
    let bams = gm.buff_position(k, BUFF_ANTI_MAGIC_SHELL).is_some();
    let input = WorkerIn {
        player: owner,
        rel: u32::from(enemy),
        bams: u32::from(bams),
        summoned,
        illusion,
        v11c: u32::from(immune),
        blocked: 0,
        misc: u32::from(gm.spells.unit.invuln_summon_dispel_damage),
        a2: 0,
        ability: 1,
        positive: if smart { u32::from(enemy) } else { 1 },
        negative: if smart { u32::from(!enemy) } else { 1 },
        a6: u32::from(smart),
        a7,
        damage_bits: damage.to_bits(),
        mana_bits: mana.to_bits(),
        flags: 0,
        target_flag8: gm.unit_is_invulnerable(k),
        source: 0,
    };
    let (steps, _) = dispel_worker(&input);
    for step in steps {
        match step {
            WorkerStep::Damage(_, 0x4000, _, _) => {
                if let Some(t) = gm.unit_damage_target(caster, k, damage, false, false, 0, DAMAGE_TYPE_MAGIC) {
                    return Some(t);
                }
            }
            WorkerStep::Buffs(a) => {
                gm.remove_buffs(
                    k,
                    crate::abilities::BuffSelect {
                        remove_positive: a[5] != 0,
                        remove_negative: a[6] != 0,
                        magic: a[1] != 0,
                        physical: a[0] != 0,
                        timed_life: a[2] != 0,
                        aura: a[3] != 0,
                        dispellable: a[4] != 0,
                    },
                );
            }
            WorkerStep::Mana(bits) => {
                let loss = F32::from_bits(bits);
                let cur = gm.unit(k).mana;
                let next = if arith::lt(cur, loss) { F32::ZERO } else { arith::sub(cur, loss) };
                gm.write_mana(k, next);
            }
            _ => {}
        }
    }
    None
}

/// Rejuvenation (`0x6F108530` -> buff apply `0x6F0F0650`, CONFIRMED): the hero rule duration `T` (`HeroDur` / `Dur`);
/// the buff is applied for `T + 0.01` (constant `0x6FAAE480`, when `T > 0`) and heals `DataA / T` life per tick
/// (soft division) on a repeating 1.0 s timer (`0x6F0F05A0`: period constant `0x6FAAE4F4`, first tick after 1.0 s).
/// The tick (`0x6F0E3A20`) heals through the life adder, with the ethereal bonus, and skips an immune target
/// (`vf 0x11C`, not modelled). `DataB` (mana) goes through `0x6F279140` the same way and is not modelled (0 in the
/// data). The buff is found by its code (`0x6F0DAB00`) and re-applied, else created. UNSURE: whether a refreshed
/// buff restarts the 1.0 s phase (the model keeps the running timer).
fn rejuvenation(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let total = rec.level.data[0];
    let amount = match gm.order_tables() {
        Some(t) => arith::div(total, dur, t),
        None => F32::from_bits((total.to_f32() / dur.to_f32()).to_bits()),
    };
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_REJUVENATION)
    };
    let buff_dur = arith::add(dur, F32::from_bits(TICK_FIRST_DELAY_BITS));
    let serial = gm.apply_mods_buff(target, BUFF_REJUVENATION, preds, UnitMods::default(), buff_dur);
    gm.register_tick(target, serial, rec.caster, amount, true, 0, false);
}

impl Game {
    /// Registers (or updates) the over-time effect of buff `serial`: a new one arms the first tick.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn register_tick(&mut self, target: ObjKey, serial: u32, source: ObjKey, amount: F32, hot: bool, damage_type: u32, first_delay: bool) {
        if let Some(t) = self.spells.unit.ticks.get_mut(&serial) {
            t.source = source;
            t.amount = amount;
            return;
        }
        let (seconds, kind) = if first_delay {
            (F32::from_bits(TICK_FIRST_DELAY_BITS).to_f32(), KIND_TICK_FIRST)
        } else {
            (TICK_PERIOD, KIND_TICK)
        };
        let timer = self.timers.start(
            seconds,
            !first_delay,
            Payload::Spell(SpellMsg::UnitSpell {
                kind,
                unit: target,
                base: 0,
                aux: serial,
            }),
        );
        self.spells.unit.ticks.insert(
            serial,
            TickRec {
                source,
                amount,
                hot,
                damage_type,
                timer: Some(timer),
            },
        );
    }
}

/// One tick of buff `serial` on `unit`: damage (a kill runs the death dispatch with the source as the killer) or heal.
fn run_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, serial: u32, first: bool) {
    let killed;
    let source;
    {
        let mut gm = game.borrow_mut();
        let Some(mut rec) = gm.spells.unit.ticks.get(&serial).copied() else {
            return;
        };
        let alive = gm.alive(unit) && gm.unit(unit).buffs.iter().any(|b| b.serial == serial);
        if !alive {
            if let Some(t) = rec.timer {
                gm.timers.cancel(t);
            }
            gm.spells.unit.ticks.remove(&serial);
            return;
        }
        source = rec.source;
        if first {
            // `0xD01E2`: the 1.0 s repeating timer starts, then the first tick runs at once.
            rec.timer = Some(gm.timers.start(
                TICK_PERIOD,
                true,
                Payload::Spell(SpellMsg::UnitSpell {
                    kind: KIND_TICK,
                    unit,
                    base: 0,
                    aux: serial,
                }),
            ));
            gm.spells.unit.ticks.insert(serial, rec);
        }
        if rec.hot {
            let (life, max) = {
                let u = gm.unit(unit);
                (u.life, u.max_life)
            };
            let mut amount = rec.amount;
            if gm.unit(unit).ethereal > 0 {
                amount = arith::mul(amount, gm.spells.unit.ethereal_heal_bonus);
            }
            let sum = arith::add(amount, life);
            let next = if arith::lt(max, sum) { max } else { sum };
            set_life(&mut gm, unit, next);
            killed = None;
        } else {
            killed = gm.unit_damage_target(rec.source, unit, rec.amount, false, false, 0, rec.damage_type);
        }
    }
    if let Some(t) = killed {
        crate::natives::dispatch_unit_death_by(game, rt, t, Some(source));
    }
}

/// What Cripple's effect slot `0x6F1CC450` hands to its buff (CONFIRMED, **Unicorn 400/400**,
/// `tools/emu/spell_buff_emu.py`: the real effect, creator `0x6F1CC320`, apply slot `0x6F1B7FD0`, speed apply `0x6F0D6460`,
/// sink routines and the damage routine `0x6F1B2BC0`): the duration as for Bloodlust; the movement sink gets `-DataA`
/// and the attack speed sink `-DataB` (sign bit flipped); `DataC` reaches the damage holder unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrippleValues {
    pub dur: F32,
    pub move_delta: F32,
    pub attack_delta: F32,
    pub damage_fraction: F32,
}

pub fn cripple_values(l: &SpellLevel, hero: bool, extra: F32) -> CrippleValues {
    CrippleValues {
        dur: effect_duration(l, hero, extra),
        move_delta: arith::neg(l.data[0]),
        attack_delta: arith::neg(l.data[1]),
        damage_fraction: l.data[2],
    }
}

/// The change Cripple makes to one weapon slot's damage counter (`0x6F1B2BC0`, CONFIRMED, **Unicorn 400/400**, both
/// slots, no `targs` filter): `amount = F2I(add(mul(DataC, X), 0.5))` with `X` the weapon's AVERAGE damage
/// ([`crate::orc::command_base_damage`]); a negative amount is clamped from below by `-(bonus + count)`; the counter changes
/// by `-amount` (a zero amount changes nothing, and there is NO "at least one" sign rule here, unlike Inner Fire). The
/// returned value is that change (negative for the usual positive `DataC`).
pub fn cripple_weapon_change(fraction: F32, count: i32, sides: i32, bonus: i32) -> i32 {
    let x = crate::orc::command_base_damage(count, sides, bonus);
    let half = F32::from_bits(0x3F00_0000);
    let mut amount = conv::f2i(arith::add(arith::mul(fraction, x), half));
    if amount < 0 {
        let floor = bonus.wrapping_add(count).wrapping_neg();
        if amount < floor {
            amount = floor;
        }
    }
    amount.wrapping_neg()
}

/// What Bloodlust's effect slot `0x6F1CB080` hands to its buff (CONFIRMED, **Unicorn 400/400**,
/// `tools/emu/spell_buff_emu.py`: the real effect, the real buff creator `0x6F1CAF70`, the real `CBuffBloodlust` apply slot
/// `0x35C` and the real sink routines `0x6F0D59A0` / `0x6F0D59F0` on a fake buff): the duration is `HeroDur` for a hero and
/// `Dur` plus the ability's extra term (`+0x9C`, soft add) otherwise; the **movement** sink `0x6F202FF0` receives `DataB`
/// and the **attack speed** sink `0x6F0C9090` receives `DataA` (both unchanged). Returns `(duration, movement, attack)`.
pub fn bloodlust_values(l: &SpellLevel, hero: bool, extra: F32) -> (F32, F32, F32) {
    (effect_duration(l, hero, extra), l.data[1], l.data[0])
}

/// What Inner Fire's effect slot `0x6F13BFA0` hands to its buff (CONFIRMED, **Unicorn 400/400**, same harness):
/// the duration as for Bloodlust, `DataA` the damage fraction (see [`inner_fire_weapon_bonus`]), `DataB` the armor sink
/// `0x6F2769C0`, `DataD` the life regeneration and `DataE` the mana regeneration modifiers (created only when the value
/// differs from zero by the `0.001` property test, [`crate::cast::prop_differs`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InnerFireValues {
    pub dur: F32,
    pub damage_fraction: F32,
    pub armor: F32,
    pub life_regen: F32,
    pub mana_regen: F32,
}

pub fn inner_fire_values(l: &SpellLevel, hero: bool, extra: F32) -> InnerFireValues {
    InnerFireValues {
        dur: effect_duration(l, hero, extra),
        damage_fraction: l.data[0],
        armor: l.data[1],
        life_regen: l.data[3],
        mana_regen: l.data[4],
    }
}

/// The `targs` words of a weapon slot that Inner Fire's attack helper `0x6F1236F0` skips (CONFIRMED by Unicorn): `1`, `0x40`,
/// `0x80`, `0x100`.
pub const INNER_FIRE_SKIPPED_TARGS: [u32; 4] = [1, 0x40, 0x80, 0x100];

/// The damage Inner Fire adds to one weapon slot (`0x6F1236F0`, CONFIRMED, **Unicorn 400/400**, both slots): `None` when the
/// slot's `targs` word is in [`INNER_FIRE_SKIPPED_TARGS`]; otherwise `F2I(add(mul(DataA, X), 0.5))` (when that is 0 the
/// sign of `DataA` instead: +1, -1 or 0) with `X` the weapon's AVERAGE damage ([`crate::orc::command_base_damage`], the same `0x6F26D5F0` the Command Aura scales), clamped from below
/// by `-(bonus + count)` (the smallest possible roll, `0x6F0C6C70`). The amount is added to the attack ability's damage
/// counter (`0x6F0C6C10`) as an INTEGER and stored in the buff for the exact undo; it is not a percentage of the rolled damage.
pub fn inner_fire_weapon_bonus(fraction: F32, count: i32, sides: i32, bonus: i32, targs: u32) -> Option<i32> {
    if INNER_FIRE_SKIPPED_TARGS.contains(&targs) {
        return None;
    }
    let x = crate::orc::command_base_damage(count, sides, bonus);
    let half = F32::from_bits(0x3F00_0000);
    let mut amount = conv::f2i(arith::add(arith::mul(fraction, x), half));
    if amount == 0 {
        // A zero result takes the sign of the fraction instead (x87 compares of `0x6F1237F5`): at least one point of
        // damage for a positive fraction, -1 for a negative or NaN one, 0 for exactly zero.
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

/// The regeneration value Inner Fire passes on (`0x6F123920` / `0x6F123A00`, CONFIRMED by Unicorn): `value` when the
/// `0.001` property test says it differs from zero, otherwise nothing is created.
pub fn inner_fire_regen(value: F32) -> F32 {
    if crate::cast::prop_differs(value, F32::ZERO) {
        value
    } else {
        F32::ZERO
    }
}

/// Inner Fire (`0x6F13BFA0`) and Bloodlust (`0x6F1CB080`): the duration is `HeroDur` for a hero target, `Dur` plus the
/// unknown extra otherwise; the buff is found by its code (refresh) or created, and gets the level's data. The value
/// mappings are CONFIRMED by Unicorn ([`bloodlust_values`], [`inner_fire_values`], [`inner_fire_weapon_bonus`]). Not run:
/// the refresh path (`slot 0x358`) and the damage of the second weapon slot (the sim keeps one damage counter).
fn buff_spell(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let d = &rec.level.data;
    let (code, mods) = if rec.base == FAERIE_FIRE {
        // Faerie Fire (`0x6F0B8750`): `DataA` is the defense reduction, a negative armor (the buff has the armor
        // sink only, CONFIRMED); the reveal of invisible units is presentation/detection and not modelled.
        (
            BUFF_FAERIE_FIRE,
            UnitMods {
                armor: arith::neg(d[0]),
                ..UnitMods::default()
            },
        )
    } else if rec.base == CRIPPLE {
        let v = cripple_values(&rec.level, hero, F32::ZERO);
        let type_id = gm.unit(target).type_id;
        let ty = gm.attack_type(type_id);
        let reduction = cripple_weapon_change(
            v.damage_fraction,
            i32::try_from(ty.dice).unwrap_or(i32::MAX),
            i32::try_from(ty.sides).unwrap_or(i32::MAX),
            ty.dice_bonus,
        );
        (
            BUFF_CRIPPLE,
            UnitMods {
                move_delta: v.move_delta,
                attack_delta: v.attack_delta,
                damage_int: reduction,
                ..UnitMods::default()
            },
        )
    } else if rec.base == INNER_FIRE {
        let v = inner_fire_values(&rec.level, hero, F32::ZERO);
        // The damage is an integer bonus per weapon slot computed from the target's attack record at the moment of the
        // application (`inner_fire_weapon_bonus`); the sim keeps one damage counter, so slot 0 stands for the unit.
        let type_id = gm.unit(target).type_id;
        let ty = gm.attack_type(type_id);
        let targs = gm.weapon_extra(type_id, 0).map_or(0, |e| e.targs);
        let damage = inner_fire_weapon_bonus(
            v.damage_fraction,
            i32::try_from(ty.dice).unwrap_or(i32::MAX),
            i32::try_from(ty.sides).unwrap_or(i32::MAX),
            ty.dice_bonus,
            targs,
        )
        .unwrap_or(0);
        (
            BUFF_INNER_FIRE,
            UnitMods {
                damage_int: damage,
                armor: v.armor,
                life_regen: inner_fire_regen(v.life_regen),
                mana_regen: inner_fire_regen(v.mana_regen),
                ..UnitMods::default()
            },
        )
    } else {
        let (_, mv, atk) = bloodlust_values(&rec.level, hero, F32::ZERO);
        (
            BUFF_BLOODLUST,
            UnitMods {
                attack_delta: atk,
                move_delta: mv,
                ..UnitMods::default()
            },
        )
    };
    let bad = rec.base == CRIPPLE || rec.base == FAERIE_FIRE;
    let preds = crate::abilities::BuffPredicates {
        positive: !bad,
        negative: bad,
        magic: true,
        dispellable: true,
        ..buff_predicates(code)
    };
    if arith::lt(F32::ZERO, dur) {
        gm.apply_mods_buff(target, code, preds, mods, dur);
    }
}

/// The epsilon of the Curse compare: the runtime value of `0x6FAAE47C` (`0x3A83126F` = 0.001f, read from the emulator's
/// start-up image, `tools/emu/curse_emu.py`).
const CURSE_EPS_BITS: u32 = 0x3A83_126F;

/// `CBuffCurse::EvadeAttack` (`0x6F07F850`, the walk-B execute slot `0x168`, with the helper `0x6F07C4C0`): CONFIRMED
/// by Unicorn (`tools/emu/curse_emu.py`, 1,800 cases over three seeds with NaN/infinity/denormal/epsilon-edge bit
/// patterns, 0 mismatches; `tests/curse_vectors.rs`). One draw of combat stream 2 is taken first (the same
/// `0x6F2853D0` call with EDX = 2 as the evasion roll); the helper subtracts by soft float `|draw - chance|`
/// (`0x6F6EEEF0`, sign bit cleared) and reports "no evade" when that is below the epsilon `0.001` or when
/// `chance < draw`; otherwise (`chance > draw` by at least the epsilon, or an unordered value) the attack misses.
pub fn curse_evades(chance: F32, draw: F32) -> bool {
    let diff = arith::sub(draw, chance);
    let abs = f32::from_bits(diff.to_bits() & 0x7FFF_FFFF);
    let eps = f32::from_bits(CURSE_EPS_BITS);
    if eps > abs {
        return false;
    }
    !(chance.to_f32() < draw.to_f32())
}

impl Game {
    /// The scores of the attacker-side walk B of the miss gate (`0x6F077420`, score slot `0x15C`): one per buff of the
    /// attacker whose class implements the walk, in buff list order (CONFIRMED structure, `attack-damage.md` 12.7).
    /// `CBuffCurse` scores its chance holder (`0x6F07F8A0` returns the value at `+0xD0`). Drunken Haze, Silence and
    /// Cloud of Fog share the walk and are not modelled.
    pub fn attacker_miss_scores(&self, attacker: ObjKey) -> Vec<F32> {
        self.unit(attacker)
            .buffs
            .iter()
            .filter(|b| b.base == BUFF_CURSE || b.base == crate::neutral_spells::BUFF_HAZE)
            .filter_map(|b| match b.effect {
                Some(crate::auras::BuffEffect::Mods(m)) => Some(m.miss_chance),
                _ => None,
            })
            .collect()
    }
}

/// Curse (`Acrs`, `CAbilityCurse` vtable `0x6F8A9694`, an AutoTargetSpell; effect slot `0x424` = `0x6F0B8450`,
/// CONFIRMED structure): the hero rule duration (`HeroDur`, else `Dur` + the ability's `+0x9C`, 0 here), `DataA` (the
/// chance to miss, 0.33) and the creator go to the buff code `Bcrs` (`0x6F07F800`, found by its code, then the buff's
/// slot `0x358` = set-duration (never shorten, `0x6F0D4EF0`) + the chance holder + creator, else created through slot
/// `0x354`). The buff has no stat sink; its effect is the attacker-side miss walk (`curse_evades`).
fn curse(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_CURSE)
    };
    let mods = UnitMods {
        miss_chance: rec.level.data[0],
        ..UnitMods::default()
    };
    gm.apply_mods_buff(target, BUFF_CURSE, preds, mods, dur);
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
    if kind == KIND_TICK || kind == KIND_TICK_FIRST {
        run_tick(game, rt, unit, aux, kind == KIND_TICK_FIRST);
        return;
    }
    if kind == KIND_PURGE_MOVE || kind == KIND_PURGE_ATTACK {
        purge_tick(game, kind, unit, aux);
        return;
    }
    if kind == KIND_SHIELD {
        shield_tick(game, rt, unit, aux);
        return;
    }
    if kind == KIND_NET {
        net_arrives(game, aux);
        return;
    }
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
    if base == RAISE_DEAD {
        // No target: the scan looks for corpses; with some in reach the cast is ordered. The autocast gate `0x3D8`
        // also asks the unit cap (slot `0x478`).
        let ready = {
            let gm = game.borrow();
            let level = gm.ability_level_record(unit, base).unwrap_or_default();
            !gm.is_stunned(unit) && order_allows_autocast(&gm, unit) && raise_dead_cap_ok(&gm, unit, &level)
        };
        if ready && begin_cast(game, rt, unit, base, CastTarget::None).is_ok() {
            let gm = game.borrow();
            let level = gm.ability_level_record(unit, base).unwrap_or_default();
            let total = arith::add(level.cast, gm.cast_point_of(unit));
            return if arith::lt(total, F32::ONE) { 1.0 } else { total.to_f32() };
        }
        return SCAN_PERIOD;
    }
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
            // Remember that this cast came from the autocast scan: Abolish Magic's effect runs in its smart mode then
            // (the ability flag `0x200000`, UNSURE reading of what sets it).
            game.borrow_mut().spells.unit.auto_started.insert((unit, base));
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

/// Raise Dead's comparator `0x6F0F5950` (slot `0x458`; CONFIRMED by Unicorn, `tools/emu/raise_dead_cmp_emu.py`, 1,200
/// cases, 0 mismatches, `tests/raise_dead_vectors.rs`): a dead candidate (`+0x5C` bit 8) beats a living best, a living
/// candidate loses to a dead best, and when both are dead or both alive the shared comparator `0x6F023400`
/// ([`default_prefers`]) decides.
pub fn raise_dead_prefers(cand_dead: bool, best_dead: bool, vc: F32, vb: F32, t: F32) -> bool {
    match (cand_dead, best_dead) {
        (true, false) => true,
        (false, true) => false,
        _ => default_prefers(vc, vb, t),
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
        ABOLISH_MAGIC => {
            // `0x6F0E2B90` (CONFIRMED structure). The first class-word rule (the caster's `+0x5C & 4`, the candidate's
            // `+0x248 & 0x200`) is not decoded and not modelled. An ally (the shared-spells row, here: not an enemy)
            // needs a negative buff, an enemy a positive one; the scan asks for magic, not timed life, not aura,
            // dispellable buffs.
            let enemy = gm.is_enemy(gm.unit(unit).owner, gm.unit(k).owner);
            gm.unit(k).buffs.iter().any(|b| {
                let p = &b.predicates;
                p.magic && !p.timed_life && !p.aura && p.dispellable && if enemy { p.positive } else { p.negative }
            })
        }
        WEB => {
            // `0x6F07F220` (CONFIRMED structure): the base filter, then the candidate must not have an ability whose slot
            // `0x18C` is nonzero (the movement-disabled test: Purge's version reads its pause holder) and must pass the
            // negated slot `0x19C` scan (taken as "not stunned"). The model reads the root count and the stun count.
            !gm.is_rooted(k) && !gm.is_stunned(k)
        }
        CURSE => {
            // `0x6F09BF80` (CONFIRMED structure): the base filter, then the candidate must have the attack ability and
            // a current attack target whose owner satisfies the relation `0x6F285B60` against the caster's owner
            // (an alliance row test, which row UNSURE: taken as "not an enemy of the caster's player", i.e. the
            // candidate is attacking one of ours), and must not carry `Bcrs` yet.
            let Some(target) = gm.attack_view(k).and_then(|v| v.target) else {
                return false;
            };
            if !gm.alive(target) {
                return false;
            }
            if gm.is_enemy(gm.unit(unit).owner, gm.unit(target).owner) {
                return false;
            }
            gm.buff_position(k, BUFF_CURSE).is_none()
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
