//! Neutral and creep abilities (S42). Spec: `docs/spec/abilities-neutral.md`.
//!
//! ## How a creep version relates to the modelled ability
//!
//! The creep camps use ability rows of `AbilityData.slk` that are *aliases*: a row whose `code` column names a base
//! ability (`ACcl` has `code` `AOcl`, `ACbl` `Ablo`, `ACsw` `Aslo`). The game builds the base ability's class and reads
//! the level records of the alias row, so every creep Chain Lightning, Bloodlust or Slow is the hero / unit ability
//! with other numbers. The model does the same: a unit that gets an alias row in its `abilList` receives the modelled
//! ability (`Ability::base` = the modelled code) whose [`crate::abilities::Ability::source`] is the alias row, and the
//! level lookups (`Game::ability_level_record`) read the alias row's records. Everything that is keyed by the base code
//! (orders, the effect dispatch, the passive hooks) works unchanged. The engine fills the table from the `code` column
//! ([`standard_for_group`], `engine::objdata`).
//!
//! A few creep abilities have a class of their own that derives from a modelled class (RTTI parent, and the
//! overrides read from the machine code): [`STATIC_PARENTS`]. Their class overrides are presentation (the effect
//! spawn slot `0x42c` / `0x430`) or order ids, so they run the parent's effect (LIKELY, see the spec).
//!
//! Labels: CONFIRMED = read in the machine code, LIKELY = strong inference, UNSURE = a stand-in.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, F32};

use crate::abilities::{buff_predicates, code};
use crate::natives::game::{Game, ObjKey};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};

/// Envenomed Spears (`Aven`, `CAbilityVenomSpear`), Magic Immunity (`Amim`), Pulverize (`Awar`).
pub const VENOM_SPEAR: u32 = code(b"Aven");
pub const MAGIC_IMMUNITY: u32 = code(b"Amim");
pub const PULVERIZE: u32 = code(b"Awar");
/// The poison buff of Envenomed Spears (`BuffID` `Bpoi`).
pub const BUFF_POISON: u32 = code(b"Bpoi");
/// The combat stream of the Pulverize roll (`sub_6F2853D0(…, 40)` at `0x6F170FF0`).
pub const PULVERIZE_STREAM: usize = 40;
/// Damage type of the poison tick (class word `0x800000`, `unit_spells::DAMAGE_TYPE_SLOW_POISON`).
const POISON_DAMAGE_TYPE: u32 = 23;
/// Damage type of the Pulverize area damage (class `0x20` = 1 << 5, ENHANCED).
const PULVERIZE_DAMAGE_TYPE: u32 = 5;

/// `(derived class code, modelled code it derives from, order id of the derived class)`. Parents are the RTTI
/// parents of the atlas (`docs/research/ability-atlas/factory.tsv`); the order ids are the Unicorn-checked cast getters
/// (`tools/analysis/ability_orders.py`, slot `0x348`).
pub const STATIC_PARENTS: [(u32, u32, u32); 7] = [
    // CAbilityNeutralHeal < CAbilityHeal: only the order ids (slots 0x348 / 0x308 / 0x318) differ.
    (code(b"Anhe"), code(b"Ahea"), ORDER_NEUTRAL_HEAL),
    // CAbilityCreepThunderBolt < CAbilityThunderBolt, no override but the codes.
    (code(b"ACtb"), code(b"AHtb"), 852_252),
    // CAbilityCreepThunderClap < CAbilityThunderClap; the only extra override (slot 0x430) spawns the effect.
    (code(b"ACtc"), code(b"AHtc"), 852_253),
    // CAbilityFireBolt < CAbilityThunderBolt; the only extra override (slot 0x42c) spawns the effect.
    (code(b"ANfb"), code(b"AHtb"), 852_231),
    // CAbilityBreathOfFire < CAbilityCarrionSwarm (overrides: AI hint slot 0x100 and the effect slot 0x42c).
    (code(b"ANbf"), code(b"AUcs"), 852_580),
    // CAbilityBreathOfFrost < CAbilityBreathOfFire (effect slot 0x42c only).
    (code(b"ACbf"), code(b"AUcs"), 852_560),
    // CAbilityCreepReincarnation < CAbilityReincarnation: slot 0x80 only adds the order ids 852314 / 852315.
    (code(b"ACrn"), code(b"AOre"), 0),
];

/// Neutral Heal (`Anhe`, `CAbilityNeutralHeal < CAbilityHeal`): the cast, autocast-on and autocast-off order ids are the
/// only overrides (slots `0x348`, `0x308`, `0x318`).
pub const NEUTRAL_HEAL: u32 = code(b"Anhe");
const ORDER_NEUTRAL_HEAL: u32 = 852_248;
const ORDER_NEUTRAL_HEAL_ON: u32 = 852_249;
const ORDER_NEUTRAL_HEAL_OFF: u32 = 852_250;

/// The codes the model implements with level data (the hero, unit and creep spells and passives). The engine uses it to
/// decide which alias rows of a `code` group stand for a modelled ability.
pub const MODELLED: &[[u8; 4]] = &[
    *b"Aadm", *b"AEah", *b"AEar", *b"AEbl", *b"AEer", *b"AEev", *b"AEfn", *b"AEim", *b"Aens", *b"AEsh", *b"Aeye",
    *b"Afae", *b"Afbk", *b"Afra", *b"AHab", *b"AHad", *b"AHav", *b"AHbh", *b"AHbn", *b"AHbz", *b"AHdr", *b"AHds",
    *b"Ahea", *b"AHer", *b"AHfs", *b"AHhb", *b"AHmt", *b"AHpx", *b"AHre", *b"AHtb", *b"AHtc", *b"Ahwd", *b"AHwe",
    *b"Ainf", *b"Alsh", *b"AOac", *b"AOae", *b"AOcl", *b"AOcr", *b"AOeq", *b"AOfs", *b"AOhw", *b"AOhx", *b"AOmi",
    *b"AOre", *b"AOsf", *b"AOsh", *b"AOsw", *b"AOvd", *b"AOwk", *b"AOws", *b"AOww", *b"Aply", *b"Aprg", *b"Arai",
    *b"Arej", *b"Aroa", *b"Aslo", *b"Aspo", *b"Asta", *b"AUau", *b"AUav", *b"AUcs", *b"AUdc", *b"AUdd", *b"AUdp",
    *b"AUdr", *b"AUfa", *b"AUfn", *b"AUfu", *b"Auhf", *b"AUim", *b"AUin", *b"AUsl", *b"AUts", *b"Aweb", *b"Ablo",
    *b"Acri", *b"Acrs", *b"Adis", *b"Aven", *b"Amim", *b"Awar",
];

fn is_modelled(c: u32) -> bool {
    MODELLED.iter().any(|m| u32::from_be_bytes(*m) == c)
}

/// The modelled ability the alias rows of one `code` group stand for, if any. `code_col` is the group's `code` column,
/// `ids` the rows that carry it. A static parent class wins, then a modelled row of the group (`AOsw` for the
/// `AOwd` group), then the code itself.
pub fn standard_for_group(code_col: u32, ids: &[u32]) -> Option<u32> {
    if let Some(&(_, parent, _)) = STATIC_PARENTS.iter().find(|p| p.0 == code_col) {
        return Some(parent);
    }
    if is_modelled(code_col) {
        return Some(code_col);
    }
    ids.iter().copied().find(|&i| is_modelled(i))
}

/// Whether an alias row is left to the item ability system (`AI..` rows are item abilities).
pub fn is_item_row(id: u32) -> bool {
    (id >> 24) == u32::from(b'A') && ((id >> 16) & 0xFF) == u32::from(b'I')
}

/// State of the neutral spells (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// `alias row -> modelled code`.
    alias: BTreeMap<u32, u32>,
    /// Units that carry one Magic Immunity count from `Amim`.
    immune: BTreeSet<ObjKey>,
    /// Pulverize hit counters (1 before any roll) and armed flags (`ability+0x20 & 0x200`).
    pulv_count: BTreeMap<ObjKey, i32>,
    pulv_armed: BTreeSet<ObjKey>,
}

/// The order ids this module adds: `(order id, modelled code)`.
pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (_, parent, order) in STATIC_PARENTS {
        if order != 0 {
            ids.insert(order, parent);
        }
    }
}

/// The autocast toggle orders of Neutral Heal (`healon` / `healoff` of the neutral class). `None` for any other id.
pub(crate) fn toggle_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    if id != ORDER_NEUTRAL_HEAL_ON && id != ORDER_NEUTRAL_HEAL_OFF {
        return None;
    }
    let mut gm = game.borrow_mut();
    if !gm.unit(unit).abilities.contains(crate::unit_spells::HEAL) {
        return Some(false);
    }
    gm.set_autocast(unit, crate::unit_spells::HEAL, id == ORDER_NEUTRAL_HEAL_ON);
    Some(true)
}

impl Game {
    /// Registers `alias` (an ability row) as an alias of the modelled ability `base`.
    pub fn set_spell_alias(&mut self, alias: u32, base: u32) {
        if alias != base {
            self.spells.neutral.alias.insert(alias, base);
        }
    }

    /// The modelled code a unit gets for the ability row `id` (`id` itself when it is no alias).
    pub fn spell_alias_base(&self, id: u32) -> u32 {
        self.spells.neutral.alias.get(&id).copied().unwrap_or(id)
    }

    /// Keeps the passive counters in step with the abilities a unit has (called with the aura sync, so on creation and
    /// whenever abilities are added or removed). Magic Immunity (`Amim`, `CAbilityMagicImmunity`): the init slot
    /// `0x80` (`0x6F110730`) calls the immunity increment `0x6F07A3A0` and the removal slot `0x6F110700` the
    /// decrement `0x6F077320` (CONFIRMED); the counter is the one `magic_immune` of Avatar and Divine Shield.
    pub fn sync_neutral_spells(&mut self, unit: ObjKey) {
        let has = self.unit(unit).abilities.get(MAGIC_IMMUNITY).is_some_and(|a| !a.is_removed());
        let counted = self.spells.neutral.immune.contains(&unit);
        if has && !counted {
            self.spells.neutral.immune.insert(unit);
            self.unit_mut(unit).magic_immune += 1;
            if self.unit(unit).magic_immune == 1 {
                self.magic_immunity_dispel(unit);
            }
        } else if !has && counted {
            self.spells.neutral.immune.remove(&unit);
            self.unit_mut(unit).magic_immune -= 1;
        }
    }

    /// Envenomed Spears (`Aven`): `CAbilityVenomSpear < CAbilityPoisonAttack` has no override but its codes, so the
    /// on-hit buff is the poison attack of Slow Poison without the slow part (the `0x34C..0x354` overrides of `Aspo`
    /// add the slows). The buff is `Bpoi` (the level's first `BuffID`); `DataA` is the damage per second (class 23),
    /// `Dur` / `HeroDur` the duration, targets `air,ground,organic`. The stacking variant `Bpsd` (`DataD`) is not
    /// modelled (UNSURE, as for Slow Poison).
    pub(crate) fn neutral_on_hit(&mut self, attacker: ObjKey, target: ObjKey) {
        if !self.alive(target) {
            return;
        }
        if self.unit(attacker).abilities.get(VENOM_SPEAR).is_none_or(|a| a.is_removed()) {
            return;
        }
        let Some(level) = self.ability_level_record(attacker, VENOM_SPEAR) else {
            return;
        };
        if !crate::unit_spells::targs_ok(self, attacker, target, crate::unit_spells::targs_word("air,ground,organic")) {
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
            ..buff_predicates(BUFF_POISON)
        };
        let serial = self.apply_mods_buff(target, BUFF_POISON, preds, crate::unit_spells::UnitMods::default(), dur);
        self.register_tick(target, serial, attacker, level.data[0], false, POISON_DAMAGE_TYPE, true);
    }

    /// Pulverize swing start (`CAbilityWarStomp`, `Awar`, slot `0x124` = `0x6F170F90`, CONFIRMED structure): the
    /// effective chance is `count * PRD(DataA)` (`0x6F163160`: `I2R(count)` times the table value of `0x6F1D21F0`, the
    /// same pair as Critical Strike, `orc::effective_chance`); a stream-40 draw `d` arms the ability when
    /// `!(chance < d)` and resets the counter to 1, otherwise the counter grows by one. A target that the spell
    /// immunity test `0x6F28D750` refuses ends the handler before the roll (modelled as: magic immune target).
    pub fn neutral_swing_start(&mut self, unit: ObjKey) {
        self.spells.neutral.pulv_armed.remove(&unit);
        let alive = {
            let u = self.unit(unit);
            !u.dead && !u.removed
        };
        if !alive || self.unit(unit).abilities.get(PULVERIZE).is_none_or(|a| a.is_removed()) {
            return;
        }
        let Some(level) = self.ability_level_record(unit, PULVERIZE) else {
            return;
        };
        let count = *self.spells.neutral.pulv_count.get(&unit).unwrap_or(&1);
        let eff = crate::orc::effective_chance(level.data[0], count);
        let Some((table, streams)) = self.rng_parts() else {
            return;
        };
        let draw = streams.stream_mut(PULVERIZE_STREAM).random_fraction(table);
        // `*chance < draw` -> not armed (an ordered x87 compare, soft `lt`).
        if arith::lt(eff, draw) {
            self.spells.neutral.pulv_count.insert(unit, count.wrapping_add(1));
        } else {
            self.spells.neutral.pulv_armed.insert(unit);
            self.spells.neutral.pulv_count.insert(unit, 1);
        }
    }

    /// Whether Pulverize is armed for the coming hit of `unit` (tests and the host).
    pub fn pulverize_armed(&self, unit: ObjKey) -> bool {
        self.spells.neutral.pulv_armed.contains(&unit)
    }

    /// The Pulverize hit counter of a unit (1 before any roll).
    pub fn pulverize_count(&self, unit: ObjKey) -> i32 {
        *self.spells.neutral.pulv_count.get(&unit).unwrap_or(&1)
    }

    /// The execute point (slot `0x170` = `0x6F1775F0`, CONFIRMED): an armed ability clears its flag and damages the
    /// area around the CASTER with two rings `(DataC, DataB)` and `(DataD, 0.5 * DataB)` through the area body
    /// `0x6F2C31D0` (damage class `0x20`, ability word = the level's `targs`); a unit takes the amount of the smallest
    /// ring that contains it (collector `0x6F2C2E80`). The callback `0x6F171180` refuses the whole damage when the
    /// pulverizing unit is an illusion. Returns the units that died. The ring membership is the plain circle (the
    /// collision-adjusted test `0x6F476C40` is UNSURE, as for the other area spells).
    pub fn neutral_execute(&mut self, unit: ObjKey) -> Vec<ObjKey> {
        if !self.spells.neutral.pulv_armed.remove(&unit) {
            return Vec::new();
        }
        let Some(level) = self.ability_level_record(unit, PULVERIZE) else {
            return Vec::new();
        };
        if self.unit(unit).illusion || !self.alive(unit) {
            return Vec::new();
        }
        let (cx, cy) = {
            let u = self.unit(unit);
            (u.x, u.y)
        };
        let word = crate::unit_spells::targs_word("ground,enemy,neutral");
        let (full, damage, half_radius) = (level.data[2], level.data[1], level.data[3]);
        let half_damage = arith::mul(F32::from_bits(0x3F00_0000), damage);
        let inner: BTreeSet<ObjKey> = crate::natives::group::circle_candidates(self, cx, cy, full).into_iter().collect();
        let mut killed = Vec::new();
        for k in crate::natives::group::circle_candidates(self, cx, cy, half_radius) {
            {
                let u = self.unit(k);
                if u.removed || u.hidden {
                    continue;
                }
            }
            if !crate::orc::area_predicate(self, unit, k, word) {
                continue;
            }
            let amount = if inner.contains(&k) { damage } else { half_damage };
            if let Some(t) = self.unit_damage_target(unit, k, amount, false, false, 0, PULVERIZE_DAMAGE_TYPE) {
                killed.push(t);
            }
        }
        killed
    }
}

/// Dispatches the deaths of a Pulverize hit (the host side of [`Game::neutral_execute`] when the caller owns the game
/// cell).
pub fn dispatch_deaths(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, killer: ObjKey, killed: Vec<ObjKey>) {
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(killer));
    }
}
