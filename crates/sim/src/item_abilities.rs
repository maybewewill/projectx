//! Item abilities: how an item's ability is cast from the pack (`UnitUseItem*`), the charge and removal rules, the
//! cooldown group, and the families that matter for melee games: restoration items (`AIhe`, `AIma`, `AIre` and the area
//! forms `AIha`, `AImr`, `AIra`), tomes (`AImi`, `AImn`, `AIem`, `AIlm`, the attribute tomes `AIxm`/`AIam`/`AIim`/`AIsm`,
//! `AIaa`, `AIgo`, `AIlu`) and the persistent bonuses (`AIat`, `AIde`, `AIas`, `AIms`, `AIml`, `AImm`, `AIsi`, `AIab`).
//! Spec: `docs/spec/abilities-items.md` (labels CONFIRMED / LIKELY / UNSURE as there). Unicorn harnesses:
//! `tools/emu/item_trace_emu.py` (call tracer used for the decoding) and `tools/emu/item_cores_emu.py` (the numeric
//! cores, vectors in `crates/sim/tests/item_ability_vectors.rs`).
//!
//! ## Model
//! * An item type has up to four ability ids (`ItemData.slk` `abilList`, [`ItemSpec`]). An id is an *alias* row of
//!   `AbilityData.slk`; its `code` column is the factory base code that picks the class ([`Family`]). The level records
//!   are registered under the alias like any spell's (`Game::set_spell_levels`).
//! * Taking an item into the pack attaches its abilities to the unit and applies the persistent bonuses; leaving the
//!   pack takes them off again (`on_pack_add` / `on_pack_remove`, hooked in `natives/item.rs`).
//! * `UnitUseItem*` casts the item's first castable ability through the cast core (`cast::begin_cast`); the charge is
//!   used when the cast completes (`on_cast_end`).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32, Tables};

use crate::abilities::{code, Ability};
use crate::cast::{begin_cast, CastRec, CastTarget, SpellLevel};
use crate::heroes::{self, BonusEnv, EngineEnv, Gain};
use crate::natives::event::{Datum, Field, Firing};
use crate::natives::game::{item_flags, Game, ItemType, ObjKey};
use crate::natives::{fire_observers_in, set_life, ScriptRuntime, KEY_BASE};
use crate::timer::TimerId;

// ---- the base codes of the families ----

pub const ITEM_HEAL: u32 = code(b"AIhe");
pub const ITEM_MANA: u32 = code(b"AIma");
pub const ITEM_RESTORE: u32 = code(b"AIre");
pub const ITEM_HEAL_AOE: u32 = code(b"AIha");
pub const ITEM_MANA_AOE: u32 = code(b"AImr");
pub const ITEM_RESTORE_AOE: u32 = code(b"AIra");
pub const TOME_LIFE: u32 = code(b"AImi");
pub const TOME_MANA: u32 = code(b"AImn");
pub const TOME_XP: u32 = code(b"AIem");
pub const TOME_LEVEL: u32 = code(b"AIlm");
pub const TOME_ATTRS: u32 = code(b"AIxm");
pub const TOME_AGI: u32 = code(b"AIam");
pub const TOME_INT: u32 = code(b"AIim");
pub const TOME_STR: u32 = code(b"AIsm");
pub const TOME_ATTACK: u32 = code(b"AIaa");
pub const GIVE_GOLD: u32 = code(b"AIgo");
pub const GIVE_LUMBER: u32 = code(b"AIlu");
pub const BONUS_ATTACK: u32 = code(b"AIat");
pub const BONUS_DEFENSE: u32 = code(b"AIde");
pub const BONUS_ATTACK_SPEED: u32 = code(b"AIas");
pub const BONUS_MOVE_SPEED: u32 = code(b"AIms");
pub const BONUS_LIFE: u32 = code(b"AIml");
pub const BONUS_MANA: u32 = code(b"AImm");
pub const BONUS_SIGHT: u32 = code(b"AIsi");
pub const BONUS_ATTRS: u32 = code(b"AIab");
pub const ITEM_SPEED: u32 = code(b"AIsp");
pub const ITEM_SPEED_AOE: u32 = code(b"AIsa");
pub const TOWN_PORTAL: u32 = code(b"AItp");
pub const ITEM_REGEN: u32 = code(b"AIrg");
pub const BONUS_LIFE_REGEN: u32 = code(b"Arel");
pub const BONUS_MANA_REGEN: u32 = code(b"AIrm");
pub const ITEM_INVIS: u32 = code(b"AIvi");
pub const ITEM_INVUL: u32 = code(b"AIvu");
pub const ITEM_RAISE_DEAD: u32 = code(b"AIrd");
pub const ITEM_RESURRECTION: u32 = code(b"AIrs");
/// The buffs of the invisibility and invulnerability potions (the codes the effect bodies hand to the buff finder:
/// `0x6F10F580` returns `'Binv'`, `0x6F10F990` returns `'Bvul'`).
pub const BUFF_INVIS: u32 = code(b"Binv");
pub const BUFF_INVUL: u32 = code(b"Bvul");
/// The buff of the regeneration items (the first of `BuffID` `BIrg,BIrl,BIrm`).
pub const BUFF_REGEN: u32 = code(b"BIrg");
/// The buff of the item speed abilities (`BuffID` of `AIsp` and `AIsa`).
pub const BUFF_SPEED: u32 = code(b"Bspe");

/// The behaviour class of an item ability base code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `CAbilityItemHeal`: life by `DataA`.
    Heal,
    /// `CAbilityItemManaRestore`: mana by `DataA`.
    Mana,
    /// `CAbilityItemRestore`: life by `DataA`, then mana by `DataB`.
    Restore,
    HealAoe,
    ManaAoe,
    RestoreAoe,
    /// `CAbilityMaxLifeMod`: maximum life by the integer `DataA` of level 1 (life sink, mode 2).
    TomeLife,
    /// `CAbilityMaxManaMod`: the same for mana (mana sink, mode 2).
    TomeMana,
    /// `CAbilityExperienceMod`: experience by the integer `DataA` of level 1.
    TomeXp,
    /// `CAbilityLevelMod`: `DataA` levels, keeping the progress inside the level.
    TomeLevel,
    /// `CAbilityAttributeMod` and its three children: agility `DataA`, intelligence `DataB`, strength `DataC`.
    TomeAttrs,
    /// `CAbilityAttackMod`: permanent damage by the integer `DataA` of level 1.
    TomeAttack,
    Gold,
    Lumber,
    BonusAttack,
    BonusDefense,
    BonusAttackSpeed,
    BonusMoveSpeed,
    BonusLife,
    BonusMana,
    BonusSight,
    BonusAttrs,
    /// `CAbilityItemSpeed` / `CAbilityItemSpeedAoe`: the buff `Bspe` with movement `DataA` and attack speed `DataB`.
    Speed,
    SpeedAoe,
    /// `CAbilityItemTownPortal` (UNSURE model, see [`town_portal`]).
    TownPortal,
    /// `CAbilityRegeneration` (`AIrg`, a `CAbilityRejuvination` child): life `DataA` and mana `DataB` over the duration.
    Regen,
    /// `CAbilityRegenLife` (`Arel`, persistent): a life regeneration modifier of `DataA` per second while carried.
    BonusLifeRegen,
    /// `CAbilityItemRegenMana` (`AIrm`, persistent, heroes only): a mana regeneration modifier of `DataA` times the hero's
    /// base mana regeneration.
    BonusManaRegen,
    /// `CAbilityItemInvis` (`AIvi`): the buff `Binv` for the hero-rule duration.
    Invis,
    /// `CAbilityItemInvul` (`AIvu`): the buff `Bvul` for the hero-rule duration; the holder is invulnerable meanwhile.
    Invul,
}

impl Family {
    /// Whether the ability is cast (an order exists) rather than a passive bonus.
    pub fn castable(self) -> bool {
        !matches!(
            self,
            Family::BonusAttack
                | Family::BonusDefense
                | Family::BonusAttackSpeed
                | Family::BonusMoveSpeed
                | Family::BonusLife
                | Family::BonusMana
                | Family::BonusSight
                | Family::BonusAttrs
                | Family::BonusLifeRegen
                | Family::BonusManaRegen
        )
    }

    /// The family of a factory base code.
    pub fn of_base(base: u32) -> Option<Family> {
        Some(match base {
            ITEM_HEAL => Family::Heal,
            ITEM_MANA => Family::Mana,
            ITEM_RESTORE => Family::Restore,
            ITEM_HEAL_AOE => Family::HealAoe,
            ITEM_MANA_AOE => Family::ManaAoe,
            ITEM_RESTORE_AOE => Family::RestoreAoe,
            TOME_LIFE => Family::TomeLife,
            TOME_MANA => Family::TomeMana,
            TOME_XP => Family::TomeXp,
            TOME_LEVEL => Family::TomeLevel,
            TOME_ATTRS | TOME_AGI | TOME_INT | TOME_STR => Family::TomeAttrs,
            TOME_ATTACK => Family::TomeAttack,
            GIVE_GOLD => Family::Gold,
            GIVE_LUMBER => Family::Lumber,
            BONUS_ATTACK => Family::BonusAttack,
            BONUS_DEFENSE => Family::BonusDefense,
            BONUS_ATTACK_SPEED => Family::BonusAttackSpeed,
            BONUS_MOVE_SPEED => Family::BonusMoveSpeed,
            BONUS_LIFE => Family::BonusLife,
            BONUS_MANA => Family::BonusMana,
            BONUS_SIGHT => Family::BonusSight,
            BONUS_ATTRS => Family::BonusAttrs,
            ITEM_SPEED => Family::Speed,
            ITEM_SPEED_AOE => Family::SpeedAoe,
            TOWN_PORTAL => Family::TownPortal,
            ITEM_REGEN => Family::Regen,
            BONUS_LIFE_REGEN => Family::BonusLifeRegen,
            BONUS_MANA_REGEN => Family::BonusManaRegen,
            ITEM_INVIS => Family::Invis,
            ITEM_INVUL => Family::Invul,
            _ => return None,
        })
    }
}

/// What an item type needs for its abilities (`ItemData.slk`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemSpec {
    /// `abilList`: up to four alias ids, in list order.
    pub abilities: Vec<u32>,
    /// `perishable`: the item is destroyed when the last charge is used.
    pub perishable: bool,
    /// `usable`: the item can be used from the pack.
    pub usable: bool,
    /// `cooldownID`: the ability code whose cooldown the item shares (the real table names the base code, e.g. `AIhe`
    /// for every healing potion; 0 = none).
    pub cooldown_id: u32,
    /// `ignoreCD`: the item does not take part in the cooldown group.
    pub ignore_cooldown: bool,
    /// `uses`: the charges of a new item (0 = no charges).
    pub uses: i32,
    /// `powerup`: consumed when taken.
    pub powerup: bool,
    /// `sellable` and `pawnable`.
    pub sellable: bool,
    pub pawnable: bool,
    /// `level`: the item level (`GetItemLevel`).
    pub level: i32,
    /// Whether `install_item_abilities` also registers the item type (charges and flags) when it is missing.
    pub registers_type: bool,
}

/// Everything the engine loads for the item abilities: the item specs, the base code of every alias and the level
/// records of every alias.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemAbilityData {
    pub specs: BTreeMap<u32, ItemSpec>,
    pub bases: BTreeMap<u32, u32>,
    pub levels: BTreeMap<u32, Vec<SpellLevel>>,
}

/// The effect a persistent bonus applied (kept so that it is taken off exactly).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Applied {
    Armor(F32),
    AttackSpeed(F32),
    Damage(i32),
    MoveSpeed(F32),
    Life(F32),
    Mana(F32),
    Sight(F32),
    Attrs { agi: i32, int: i32, str: i32 },
    /// A regeneration modifier of the agile engine (or, without stats, the aura bonus fallback).
    Regen { mana: bool, rate: F32, modifier: Option<crate::agile::ModId> },
}

/// State of the item abilities (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    specs: BTreeMap<u32, ItemSpec>,
    bases: BTreeMap<u32, u32>,
    /// `alias -> cooldown group` for the aliases whose item names a group other than the alias itself.
    groups: BTreeMap<u32, u32>,
    /// The items that hold an ability on a unit.
    holders: BTreeMap<(ObjKey, u32), Vec<ObjKey>>,
    /// Persistent bonuses by `(item, alias)`.
    applied: BTreeMap<(ObjKey, u32), (ObjKey, Applied)>,
    /// Casts started by a use order and not ended yet: `(caster, alias) -> item`.
    pending: BTreeMap<(ObjKey, u32), ObjKey>,
    /// Regeneration buffs running by buff serial.
    regen: BTreeMap<u32, RegenRec>,
    /// Misc `IllusionsCanRestoreLife` (key CONFIRMED at `0x6F876B18`): illusions heal and carry life regeneration items.
    pub illusions_heal: bool,
    /// Misc `IllusionsCanRestoreMana` (key at `0x6F876B30`): the same for the mana regeneration item.
    pub illusions_mana: bool,
    /// Misc `IntRegenBonus` (read by `0x6F2082C0`): mana regeneration per point of hero intelligence.
    pub int_regen_bonus: F32,
}

/// One running regeneration (`AIrg`): the amounts per 1.0 s tick and the repeating timer.
#[derive(Debug, Clone, Copy)]
struct RegenRec {
    life: F32,
    mana: F32,
    timer: Option<TimerId>,
}

impl State {
    fn base_of(&self, alias: u32) -> u32 {
        self.bases.get(&alias).copied().unwrap_or(alias)
    }

    fn family(&self, alias: u32) -> Option<Family> {
        Family::of_base(self.base_of(alias))
    }
}

// ---- registry API ----

impl Game {
    /// Installs the item specs, alias bases and level records (see [`ItemAbilityData`]).
    pub fn install_item_abilities(&mut self, data: &ItemAbilityData) {
        for (&ty, spec) in &data.specs {
            self.set_item_spec(ty, spec.clone());
            if self.item_types().get(ty).is_none() && spec.registers_type {
                let mut flags = 0;
                if spec.perishable {
                    flags |= item_flags::PERISHABLE;
                }
                if spec.powerup {
                    flags |= item_flags::POWERUP;
                }
                // Class and the random-selection flag stay 0 until the item rows are decoded fully: the type is
                // registered for creation (charges, flags), not for `ChooseRandomItem`.
                self.item_types_mut().insert(
                    ty,
                    ItemType {
                        class: 0,
                        level_ex: spec.level,
                        level_plain: spec.level,
                        flag_48: 0,
                        charges: spec.uses,
                        powerup: spec.powerup,
                        sellable: spec.sellable,
                        pawnable: spec.pawnable,
                        flags,
                    },
                );
            }
        }
        for (&alias, &base) in &data.bases {
            self.set_item_ability_base(alias, base);
        }
        for (&alias, levels) in &data.levels {
            self.set_spell_levels(alias, levels.clone());
        }
    }

    /// Registers the abilities of item type `type_id`. The cooldown group of the item is its `cooldownID` (the key of the
    /// unit's group table, `0x6F2B8D70`): an alias of the item whose own id differs from the group is checked against
    /// the group when it is cast from a command card (the use path checks the item's group itself).
    pub fn set_item_spec(&mut self, type_id: u32, spec: ItemSpec) {
        if spec.cooldown_id != 0 {
            for &a in &spec.abilities {
                if a != spec.cooldown_id {
                    self.spells.item.groups.entry(a).or_insert(spec.cooldown_id);
                }
            }
        }
        self.spells.item.specs.insert(type_id, spec);
    }

    /// The Misc inputs of the regeneration items: `IntRegenBonus` and the two illusion switches.
    pub fn set_item_misc(&mut self, int_regen_bonus: F32, illusions_heal: bool, illusions_mana: bool) {
        let st = &mut self.spells.item;
        st.int_regen_bonus = int_regen_bonus;
        st.illusions_heal = illusions_heal;
        st.illusions_mana = illusions_mana;
    }

    /// Registers the factory base code of an alias ability id.
    pub fn set_item_ability_base(&mut self, alias: u32, base: u32) {
        self.spells.item.bases.insert(alias, base);
    }

    /// The item spec of a type.
    pub fn item_spec(&self, type_id: u32) -> Option<&ItemSpec> {
        self.spells.item.specs.get(&type_id)
    }
}

/// Whether `base` (an alias or a base code) is a castable item ability that takes no target (the cast core's
/// order-class gate): every castable family does, a unit target or a point is accepted too.
pub(crate) fn takes_no_target(gm: &Game, base: u32) -> bool {
    gm.spells.item.family(base).is_some_and(Family::castable)
}

/// The spell classes an item ability can borrow unchanged: the item row's `code` names the same class as the unit
/// spell (`AEbl` Dagger of Escape, `AHds` Potion of Divinity, `AHhb`, `AHmt` Staff of Teleportation, `AHre` and its
/// item child `AIrs`, `Arai` and `AIrd`, wards, Lightning Shield, Purge, Roar, Chain Lightning, Death Pact, Inferno):
/// the effect slots are the unit spell's own (the factory atlas lists the item child without an override of them), so
/// the unit spell's effect runs with the item's level record. CONFIRMED for `AIrs` (only the code and order getters
/// of `CAbilityResurrection` are overridden) and `AIrd` (the same effect slot `0x424` = `0x6F0E6AA0` as `Arai`; the
/// child only changes the autocast gate `0x380`); LIKELY for the others (same class).
const DELEGATED: [u32; 14] = [
    crate::undead_ne::BLINK,
    crate::human_heroes::DIVINE_SHIELD,
    crate::cast::HOLY_LIGHT,
    crate::human_heroes::MASS_TELEPORT,
    crate::human_heroes::RESURRECTION,
    crate::unit_spells::RAISE_DEAD,
    crate::unit_spells::SENTRY_WARD,
    crate::unit_spells::HEALING_WARD,
    crate::unit_spells::LIGHTNING_SHIELD,
    crate::unit_spells::PURGE,
    crate::unit_spells::ROAR,
    crate::orc::CHAIN_LIGHTNING,
    crate::undead_ne::DEATH_PACT,
    crate::undead_ne::INFERNO,
];

/// The class code that runs an ability id's effect: the factory base code of the alias, with the two item classes that
/// inherit a spell class mapped to it (`AIrs` -> `AHre`, `AIrd` -> `Arai`). An id with no registered base maps to itself.
pub(crate) fn effect_base(gm: &Game, alias: u32) -> u32 {
    match gm.spells.item.base_of(alias) {
        ITEM_RESURRECTION => crate::human_heroes::RESURRECTION,
        ITEM_RAISE_DEAD => crate::unit_spells::RAISE_DEAD,
        b => b,
    }
}

/// Whether the ability id is an item ability that borrows a unit spell class ([`DELEGATED`]).
pub(crate) fn delegated(gm: &Game, alias: u32) -> bool {
    gm.spells.item.bases.contains_key(&alias) && DELEGATED.contains(&effect_base(gm, alias))
}

/// `rec` with the alias replaced by its class code, for the unit spell modules (they match on the class code).
pub(crate) fn resolve_rec(gm: &Game, rec: &CastRec) -> CastRec {
    if delegated(gm, rec.base) {
        CastRec {
            base: effect_base(gm, rec.base),
            ..*rec
        }
    } else {
        *rec
    }
}

/// Whether the cooldown group of `base` is cooling on `unit` (UNSURE: the group is the item's `cooldownID`).
pub(crate) fn group_cooling(gm: &Game, unit: ObjKey, base: u32) -> bool {
    gm.spells.item.groups.get(&base).is_some_and(|&g| gm.is_cooling(unit, g))
}

// ---- the pack hooks ----

fn level_of(gm: &Game, alias: u32) -> SpellLevel {
    gm.spells_level_record(alias, 0)
}

/// Taking `item` into the pack of `unit`: its abilities are attached and its persistent bonuses applied.
pub(crate) fn on_pack_add(gm: &mut Game, unit: ObjKey, item: ObjKey) {
    let ty = gm.item(item).type_id;
    let Some(spec) = gm.spells.item.specs.get(&ty).cloned() else {
        return;
    };
    for alias in spec.abilities {
        if !gm.unit(unit).abilities.contains(alias) {
            gm.unit_mut(unit).abilities.attach(Ability::new(alias, None));
        }
        gm.spells.item.holders.entry((unit, alias)).or_default().push(item);
        if let Some(fam) = gm.spells.item.family(alias) {
            if !fam.castable() {
                apply_passive(gm, unit, item, alias, fam);
            }
        }
    }
}

/// `item` leaves the pack of `unit` (dropped, removed or consumed): its persistent bonuses come off and the abilities
/// it alone held are removed from the unit.
pub(crate) fn on_pack_remove(gm: &mut Game, unit: ObjKey, item: ObjKey) {
    let ty = gm.item(item).type_id;
    let Some(spec) = gm.spells.item.specs.get(&ty).cloned() else {
        return;
    };
    for alias in spec.abilities {
        if let Some((u, applied)) = gm.spells.item.applied.remove(&(item, alias)) {
            undo_applied(gm, u, applied);
        }
        let empty = {
            let list = gm.spells.item.holders.entry((unit, alias)).or_default();
            list.retain(|&i| i != item);
            list.is_empty()
        };
        if empty {
            gm.spells.item.holders.remove(&(unit, alias));
            gm.unit_mut(unit).abilities.remove(alias);
        }
    }
}

/// A powerup taken by `unit` (consumed at once): its castable abilities run their effect on the unit with no mana,
/// cooldown or charge (UNSURE: the original adds the item abilities to the unit through `0x6F2B9600` and the
/// ability acts at once).
pub(crate) fn on_powerup(gm: &mut Game, unit: ObjKey, item: ObjKey) {
    let ty = gm.item(item).type_id;
    let Some(spec) = gm.spells.item.specs.get(&ty).cloned() else {
        return;
    };
    for alias in spec.abilities {
        let Some(fam) = gm.spells.item.family(alias) else {
            continue;
        };
        if fam.castable() {
            let level = level_of(gm, alias);
            apply_effect(gm, fam, &level, unit, CastTarget::Unit(unit));
        }
    }
}

// ---- persistent bonuses ----

fn apply_passive(gm: &mut Game, unit: ObjKey, item: ObjKey, alias: u32, fam: Family) {
    let level = level_of(gm, alias);
    let a = level.data[0];
    let applied = match fam {
        Family::BonusAttack => {
            let v = damage_bonus_value(a);
            gm.add_damage(unit, v);
            Applied::Damage(v)
        }
        Family::BonusDefense => {
            gm.add_armor(unit, a);
            Applied::Armor(a)
        }
        Family::BonusAttackSpeed => {
            gm.add_attack_speed(unit, a);
            Applied::AttackSpeed(a)
        }
        Family::BonusMoveSpeed => {
            add_move_speed(gm, unit, a);
            Applied::MoveSpeed(a)
        }
        Family::BonusLife => {
            life_sink_delta(gm, unit, a, 1);
            Applied::Life(a)
        }
        Family::BonusMana => {
            mana_sink_delta(gm, unit, a, 1);
            Applied::Mana(a)
        }
        Family::BonusSight => {
            add_sight(gm, unit, a);
            Applied::Sight(a)
        }
        Family::BonusAttrs => {
            let (agi, int, str) = attribute_values(&level.data);
            add_attributes(gm, unit, agi, int, str);
            Applied::Attrs { agi, int, str }
        }
        Family::BonusLifeRegen => {
            let illusion = gm.unit(unit).illusion;
            if !regen_item_gate(illusion, gm.spells.item.illusions_heal) {
                return;
            }
            let modifier = gm.add_life_regen_modifier(unit, a);
            if modifier.is_none() {
                gm.add_life_regen(unit, a);
            }
            Applied::Regen { mana: false, rate: a, modifier }
        }
        Family::BonusManaRegen => {
            // `0x6F0FB370`: heroes only, and the illusion gate with the mana switch.
            let Some(h) = gm.unit(unit).hero.as_ref() else {
                return;
            };
            let illusion = gm.unit(unit).illusion;
            if !regen_item_gate(illusion, gm.spells.item.illusions_mana) {
                return;
            }
            let int_total = heroes::intelligence(h.int_base, h.int_plus, h.level - 1);
            let type_regen = gm.unit_types().get(gm.unit(unit).type_id).map_or(F32::ZERO, |t| t.regen_mana);
            let term = hero_int_regen_term(gm.spells.item.int_regen_bonus, int_total);
            let rate = mana_regen_item_rate(type_regen, term, a);
            let modifier = gm.add_mana_regen_modifier(unit, rate);
            if modifier.is_none() {
                gm.add_mana_regen(unit, rate);
            }
            Applied::Regen { mana: true, rate, modifier }
        }
        _ => return,
    };
    gm.spells.item.applied.insert((item, alias), (unit, applied));
}

fn undo_applied(gm: &mut Game, unit: ObjKey, applied: Applied) {
    match applied {
        Applied::Damage(v) => gm.add_damage(unit, v.wrapping_neg()),
        Applied::Armor(a) => gm.add_armor(unit, arith::neg(a)),
        Applied::AttackSpeed(a) => gm.add_attack_speed(unit, arith::neg(a)),
        Applied::MoveSpeed(a) => add_move_speed(gm, unit, arith::neg(a)),
        Applied::Life(a) => life_sink_delta(gm, unit, arith::neg(a), 1),
        Applied::Mana(a) => mana_sink_delta(gm, unit, arith::neg(a), 1),
        Applied::Sight(a) => add_sight(gm, unit, arith::neg(a)),
        Applied::Attrs { agi, int, str } => {
            add_attributes(gm, unit, agi.wrapping_neg(), int.wrapping_neg(), str.wrapping_neg())
        }
        Applied::Regen { modifier: Some(m), .. } => gm.release_modifier(m),
        Applied::Regen { mana, rate, modifier: None } => {
            if mana {
                gm.add_mana_regen(unit, arith::neg(rate));
            } else {
                gm.add_life_regen(unit, arith::neg(rate));
            }
        }
    }
}

/// The move speed bonus of `AIms` (UNSURE: the sink `0x6F201D50` and the Misc switch `MoveSpeedBonusesStack` are not
/// decoded; the model adds the bonus to the unit's speed).
fn add_move_speed(gm: &mut Game, unit: ObjKey, delta: F32) {
    let u = gm.unit_mut(unit);
    u.move_speed = arith::add(u.move_speed, delta);
    gm.mover_speed_changed(unit);
}

/// The sight bonus of `AIsi` (UNSURE: the sink `0x6F27A4E0` divides by the constant `0x6FAAE50C` and writes a property;
/// the model adds the value to the day and night radii).
fn add_sight(gm: &mut Game, unit: ObjKey, delta: F32) {
    let u = gm.unit_mut(unit);
    u.sight = arith::add(u.sight, delta);
    u.night_sight = arith::add(u.night_sight, delta);
}

/// The life sink `0x6F2AC450(unit, &value, mode)` on a unit (the maximum life changes, the life follows by the mode).
fn life_sink_delta(gm: &mut Game, unit: ObjKey, value: F32, mode: u32) {
    let flagged = gm.unit(unit).dead;
    let mut cells = gm.life_cells(unit);
    let tables = gm.order_tables().cloned();
    heroes::life_sink(&mut cells, flagged, value, mode, tables.as_ref());
    gm.put_life_cells(unit, cells);
}

/// The mana sink `0x6F278F50(unit, &value, mode)`.
fn mana_sink_delta(gm: &mut Game, unit: ObjKey, value: F32, mode: u32) {
    let mut cells = gm.mana_cells(unit);
    let tables = gm.order_tables().cloned();
    heroes::mana_sink(&mut cells, value, mode, tables.as_ref());
    gm.put_mana_cells(unit, cells);
}

/// The hero attribute adders `0x6F20A1E0` (agility), `0x6F20A260` (intelligence), `0x6F20A160` (strength) with the
/// show flag 0: the base counter grows and the derived values are refreshed (`natives::hero::change_hero_attributes`).
fn add_attributes(gm: &mut Game, unit: ObjKey, agi: i32, int: i32, str: i32) {
    if gm.unit(unit).hero.is_none() {
        return;
    }
    crate::natives::hero::change_hero_attributes(gm, unit, false, |h| {
        h.agi_base = h.agi_base.wrapping_add(agi);
        h.int_base = h.int_base.wrapping_add(int);
        h.str_base = h.str_base.wrapping_add(str);
    });
}

// ---- the pure cores (Unicorn-checked against the original, `tools/emu/item_cores_emu.py`) ----

/// The illusion gate of the regeneration items (`0x6F0F4840` for `Arel`, `0x6F0F4AE0` for `AIrm`, Unicorn `RL` rows): the
/// modifier is made unless the carrier is an illusion and the Misc switch (`IllusionsCanRestoreLife` /
/// `IllusionsCanRestoreMana`) is off.
pub fn regen_item_gate(illusion: bool, misc: bool) -> bool {
    !illusion || misc
}

/// The hero term of the mana regeneration item (`0x6F2082C0`, Unicorn `RH` rows): `Misc IntRegenBonus * I2R(int)`.
pub fn hero_int_regen_term(misc: F32, int_total: i32) -> F32 {
    arith::mul(misc, conv::i2r(int_total))
}

/// The rate of the mana regeneration item (`0x6F0E64E0`, Unicorn `RM` rows): `(type regenMana + hero term) * DataA`.
pub fn mana_regen_item_rate(type_regen: F32, hero_term: F32, data_a: F32) -> F32 {
    arith::mul(arith::add(type_regen, hero_term), data_a)
}

/// Where a restoration amount goes: the life adder `0x6F277C20` or the mana adder `0x6F279140`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    Life,
    Mana,
}

/// The sink calls of a restoration effect body (`0x6F0E3990`, `0x6F119F10`, `0x6F1DBB10`): the amounts are the level
/// data as they are, in this order (Unicorn: no ethereal factor, the class slot `0x42C` answers 0 for all three).
pub fn restore_plan(fam: Family, d: &[F32; 9]) -> Vec<(Sink, F32)> {
    match fam {
        Family::Heal | Family::HealAoe => vec![(Sink::Life, d[0])],
        Family::Mana | Family::ManaAoe => vec![(Sink::Mana, d[0])],
        Family::Restore | Family::RestoreAoe => vec![(Sink::Life, d[0]), (Sink::Mana, d[1])],
        _ => Vec::new(),
    }
}

/// The area callback `0x6F0F0470`: the candidate is skipped when it is immune, or when it is an illusion and the Misc
/// switch is off; otherwise the life adder gets `DataA` (the callback always returns 1).
pub fn area_callback_amount(illusion: bool, misc: bool, immune: bool, a: F32) -> Option<F32> {
    if immune || (!misc && illusion) {
        None
    } else {
        Some(a)
    }
}

/// The three attribute values the attribute tome and the attribute bonus read (`0x6F0E5D50`, `0x6F0E57E0`): the
/// integer parts (`F2I`) of `DataA`, `DataB` and `DataC`, handed to the agility, intelligence and strength adders.
pub fn attribute_values(d: &[F32; 9]) -> (i32, i32, i32) {
    (conv::f2i(d[0]), conv::f2i(d[1]), conv::f2i(d[2]))
}

/// The flat damage bonus of `AIat` (`F2I(DataA)`; the removal is the negative).
pub fn damage_bonus_value(a: F32) -> i32 {
    conv::f2i(a)
}

/// The outcome of the charge routine `0x6F2B8390` for an item: `None` when it returns at once, else `(new charges, the
/// holder is told, the item is destroyed)`. `holder_live` is the answer of the key resolver for the holder key.
pub fn charge_outcome(flags: u32, charges: i32, holder_live: bool) -> Option<(i32, bool, bool)> {
    let (_, new) = charge_step(flags, charges);
    let new = new?;
    Some((new, holder_live, destroys(flags, new)))
}

// ---- the effects ----

/// The level-1 record of an alias: the tomes and attribute bonuses read `DataA..C` at level 1 whatever the level
/// (the profile reader is called with the constant level 1, `0x6F0E6580`, `0x6F0E5D50`, `0x6F0E57E0`).
fn level_one(gm: &Game, alias: u32) -> SpellLevel {
    gm.spells_level_record(alias, 0)
}

/// The life adder `0x6F277C20` (no healing modifiers in the model) on `target`: add and clamp to the maximum.
fn add_life(gm: &mut Game, target: ObjKey, amount: F32) {
    let (life, max) = {
        let u = gm.unit(target);
        (u.life, u.max_life)
    };
    let sum = arith::add(amount, life);
    let next = if arith::lt(max, sum) { max } else { sum };
    set_life(gm, target, next);
}

/// The mana adder `0x6F279140` on `target` (UNSURE clamp: the property setter limits the value to the maximum).
fn add_mana(gm: &mut Game, target: ObjKey, amount: F32) {
    let (mana, max) = {
        let u = gm.unit(target);
        (u.mana, u.max_mana)
    };
    let sum = arith::add(mana, amount);
    let next = if arith::lt(max, sum) { max } else { sum };
    gm.write_mana(target, next);
}

fn run_plan(gm: &mut Game, t: ObjKey, plan: &[(Sink, F32)]) {
    for &(sink, amount) in plan {
        match sink {
            Sink::Life => add_life(gm, t, amount),
            Sink::Mana => add_mana(gm, t, amount),
        }
    }
}

/// The target of an effect: the ability's unit target, else its owner (`[ability+0x30]`, `0x6F472890`).
fn effect_target(caster: ObjKey, target: CastTarget) -> ObjKey {
    match target {
        CastTarget::Unit(t) => t,
        _ => caster,
    }
}

/// The center of an area effect: the position of the effect target (a unit target or the caster).
fn area_center(gm: &Game, caster: ObjKey, target: CastTarget) -> (F32, F32) {
    let u = gm.unit(effect_target(caster, target));
    (u.x, u.y)
}

/// The area callbacks (`0x6F0F0470` and the mana and restore twins): immune units are skipped (not modelled), an
/// illusion is skipped unless the Misc switch allows it (CONFIRMED structure, UNSURE key), the amount is `DataA`.
fn area_units(gm: &Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) -> Vec<ObjKey> {
    let (cx, cy) = area_center(gm, caster, target);
    let owner = gm.unit(caster).owner;
    crate::natives::circle_candidates(gm, cx, cy, level.area)
        .into_iter()
        .filter(|&k| {
            if !gm.alive(k) || gm.unit(k).hidden {
                return false;
            }
            let u = gm.unit(k);
            // UNSURE: the group builder's relation flags (`0x1D` / `0x1F`) are not decoded; friends and the caster.
            let friend = k == caster || u.owner == owner || !gm.is_enemy(owner, u.owner);
            friend && area_callback_amount(u.illusion, gm.spells.item.illusions_heal, false, F32::ZERO).is_some()
        })
        .collect()
}

/// Runs the effect of `fam` for `caster` (state only: no events are needed, so the powerup path shares it).
fn apply_effect(gm: &mut Game, fam: Family, level: &SpellLevel, caster: ObjKey, target: CastTarget) {
    let d = &level.data;
    match fam {
        Family::Heal | Family::Mana | Family::Restore => {
            let t = effect_target(caster, target);
            if gm.alive(t) {
                run_plan(gm, t, &restore_plan(fam, d));
            }
        }
        Family::HealAoe | Family::ManaAoe | Family::RestoreAoe => {
            let plan = restore_plan(fam, d);
            for k in area_units(gm, caster, target, level) {
                run_plan(gm, k, &plan);
            }
        }
        Family::TomeLife => {
            let t = effect_target(caster, target);
            life_sink_delta(gm, t, conv::i2r(conv::f2i(d[0])), 2);
        }
        Family::TomeMana => {
            let t = effect_target(caster, target);
            mana_sink_delta(gm, t, conv::i2r(conv::f2i(d[0])), 2);
        }
        Family::TomeXp => {
            let t = effect_target(caster, target);
            add_hero_xp(gm, t, conv::i2r(conv::f2i(d[0])), true);
        }
        Family::TomeLevel => {
            let t = effect_target(caster, target);
            let n = conv::f2i(d[0]);
            if let Some(amount) = level_mod_amount(gm, t, n) {
                add_hero_xp(gm, t, amount, false);
            }
        }
        Family::TomeAttrs => {
            let t = effect_target(caster, target);
            let (agi, int, str) = attribute_values(d);
            add_attributes(gm, t, agi, int, str);
        }
        Family::TomeAttack => {
            let t = effect_target(caster, target);
            // `0x6F0DE190`: only a unit with an attack ability; the value goes into both weapons' base bonuses.
            gm.add_damage(t, conv::f2i(d[0]));
        }
        Family::Speed => {
            let t = effect_target(caster, target);
            apply_speed(gm, t, level);
        }
        Family::SpeedAoe => {
            for k in area_units(gm, caster, target, level) {
                apply_speed(gm, k, level);
            }
        }
        Family::TownPortal => town_portal(gm, caster, target, level),
        Family::Invis | Family::Invul => {
            let t = effect_target(caster, target);
            apply_potion_buff(gm, fam, t, level);
        }
        Family::Regen => {
            // The scroll forms (`Area` above 0) act on the units around the target or owner (`0x6F1088D0`), the potions on
            // the target or the owner (`0x6F108AC0`).
            let units = if arith::lt(F32::ZERO, level.area) {
                area_units(gm, caster, target, level)
            } else {
                vec![effect_target(caster, target)]
            };
            for k in units {
                apply_regen(gm, caster, k, level);
            }
        }
        Family::Gold | Family::Lumber => {
            let t = effect_target(caster, target);
            let owner = gm.unit(t).owner;
            let res = if fam == Family::Gold {
                crate::economy::Res::Gold
            } else {
                crate::economy::Res::Lumber
            };
            gm.credit_resource(owner, res, conv::f2i(d[0]), crate::economy::Flags::NONE);
        }
        _ => {}
    }
}

/// The duration the potion buff effects hand to the buff apply (`0x6F13C180` for `AIvi`, `0x6F13C270` for `AIvu`,
/// Unicorn `VI` rows): `HeroDur` for a hero target, else `Dur` plus the ability's extra term (`+0x9C`, a soft add).
pub fn potion_buff_duration(level: &SpellLevel, hero: bool, extra: F32) -> F32 {
    crate::cast::effect_duration(level, hero, extra)
}

/// `AIvi` / `AIvu` on one unit: the buff `Binv` (invisibility) or `Bvul` (invulnerability) for [`potion_buff_duration`].
/// The effect bodies are CONFIRMED (they find or create the buff by its code and call its apply slot `0x350` with the
/// duration); the buff classes are not decoded: invisibility has no model beyond the buff (no detection in the sim),
/// invulnerability adds one count (the same flag the `Avul` ability sets); both are positive and magic (UNSURE).
fn apply_potion_buff(gm: &mut Game, fam: Family, target: ObjKey, level: &SpellLevel) {
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = potion_buff_duration(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let (code, invuln) = if fam == Family::Invul { (BUFF_INVUL, true) } else { (BUFF_INVIS, false) };
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(code)
    };
    let mods = crate::unit_spells::UnitMods {
        invuln,
        ..Default::default()
    };
    gm.apply_mods_buff(target, code, preds, mods, dur);
}

/// The per-tick amounts of a regeneration: the totals `DataA` (life) and `DataB` (mana) spread over the duration (soft
/// division, like Rejuvenation `0x6F0F0650`).
pub fn regen_tick_amounts(level: &SpellLevel, dur: F32, t: Option<&Tables>) -> (F32, F32) {
    let div = |total: F32| match t {
        Some(t) => arith::div(total, dur, t),
        None => F32::from_bits((total.to_f32() / dur.to_f32()).to_bits()),
    };
    (div(level.data[0]), div(level.data[1]))
}

/// `AIrg` on one unit: the hero rule duration, the buff `BIrg` for the duration plus 0.01 s and a repeating 1.0 s tick that
/// adds the per-tick life and mana (LIKELY: the class reuses the Rejuvenation tick `0x6F0E3A20`; a refresh replaces the
/// amounts and keeps the running timer).
fn apply_regen(gm: &mut Game, caster: ObjKey, target: ObjKey, level: &SpellLevel) {
    let _ = caster;
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let tables = gm.order_tables().cloned();
    let (life, mana) = regen_tick_amounts(level, dur, tables.as_ref());
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(BUFF_REGEN)
    };
    let buff_dur = arith::add(dur, F32::from_bits(0x3C23_D70A));
    let serial = gm.apply_mods_buff(target, BUFF_REGEN, preds, crate::unit_spells::UnitMods::default(), buff_dur);
    if let Some(rec) = gm.spells.item.regen.get_mut(&serial) {
        rec.life = life;
        rec.mana = mana;
        return;
    }
    let timer = gm.timers.start(
        1.0,
        true,
        crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Item { kind: TICK_REGEN, unit: target, aux: serial }),
    );
    gm.spells.item.regen.insert(serial, RegenRec { life, mana, timer: Some(timer) });
}

/// The timer kind of a regeneration tick.
pub const TICK_REGEN: u8 = 1;

/// A timer of [`crate::cast::SpellMsg::Item`]: one regeneration tick while the buff lives.
pub(crate) fn on_timer(game: &RefCell<Game>, _rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, serial: u32) {
    if kind != TICK_REGEN {
        return;
    }
    let mut gm = game.borrow_mut();
    let Some(rec) = gm.spells.item.regen.get(&serial).copied() else {
        return;
    };
    let alive = gm.alive(unit) && gm.unit(unit).buffs.iter().any(|b| b.serial == serial);
    if !alive {
        if let Some(t) = rec.timer {
            gm.timers.cancel(t);
        }
        gm.spells.item.regen.remove(&serial);
        return;
    }
    if rec.life.to_bits() & 0x7FFF_FFFF != 0 {
        add_life(&mut gm, unit, rec.life);
    }
    if rec.mana.to_bits() & 0x7FFF_FFFF != 0 {
        add_mana(&mut gm, unit, rec.mana);
    }
}

/// The speed buff arguments of `CAbilityItemSpeed::vf424` (`0x6F1FC4C0`): the duration by the hero rule (`HeroDur` for a
/// hero target, else `Dur` plus the ability's extra term, a soft add), then `DataA` and `DataB`. The buff's apply slot
/// (`CBuffSpeed`, `0x6F0D6500`) gives `DataA` to the movement sink (`0x6F202FF0`, holder `+0xCC`) and `DataB` to the attack
/// speed sink (`0x6F0C9090`, holder `+0xD4`). Unicorn: `item_cores_emu.py` rows `SP`.
pub fn speed_buff_args(level: &SpellLevel, hero: bool, extra: F32) -> (F32, F32, F32) {
    (crate::cast::effect_duration(level, hero, extra), level.data[0], level.data[1])
}

fn apply_speed(gm: &mut Game, target: ObjKey, level: &SpellLevel) {
    if !gm.alive(target) {
        return;
    }
    let hero = gm.target_hero(target);
    let (dur, mv, atk) = speed_buff_args(level, hero, F32::ZERO);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..crate::abilities::buff_predicates(BUFF_SPEED)
    };
    let mods = crate::unit_spells::UnitMods {
        move_delta: mv,
        attack_delta: atk,
        ..Default::default()
    };
    gm.apply_mods_buff(target, BUFF_SPEED, preds, mods, dur);
}

/// Town portal (`AItp`, `CAbilityItemTownPortal`): UNSURE model. Decoded: a target structure (`targs`), the cast time
/// `Cast` (5 s) is the cast core's delay, `Area` 1100 around the caster, the class slot `0x418` runs the area group
/// builder with the callback `0x6F0264B0` (not decoded). The model reads `DataA` as the count of teleported units and
/// `DataB` as the clustering flag (the Mass Teleport fields: `human_heroes::teleport_select`): the caster and the chosen
/// own or allied non-structure units in the area go to the target's position, keeping their offsets when clustering.
fn town_portal(gm: &mut Game, caster: ObjKey, target: CastTarget, level: &SpellLevel) {
    let CastTarget::Unit(dest) = target else {
        return;
    };
    if !gm.alive(caster) || !gm.alive(dest) {
        return;
    }
    let (cx, cy, owner) = {
        let c = gm.unit(caster);
        (c.x, c.y, c.owner)
    };
    let (dx, dy) = {
        let d = gm.unit(dest);
        (d.x, d.y)
    };
    let mut recs = Vec::new();
    for k in crate::natives::circle_candidates(gm, cx, cy, level.area) {
        if k == caster || !gm.alive(k) {
            continue;
        }
        let u = gm.unit(k);
        if gm.is_enemy(owner, u.owner) || gm.unit_types().is_building(u.type_id) {
            continue;
        }
        let (ex, ey) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
        let d2 = arith::add(arith::add(arith::mul(ex, ex), arith::mul(ey, ey)), F32::ZERO);
        let hero = u.hero.as_ref().map(|h| h.level);
        recs.push((
            k,
            crate::human_heroes::TeleportRec {
                dist: conv::f2i(d2),
                key: hero.unwrap_or(0),
                hero: i32::from(hero.is_some() && !u.illusion),
                own: i32::from(u.owner == owner),
                id: k.0 as i32,
            },
        ));
    }
    let chosen = crate::human_heroes::teleport_select(recs, conv::f2i(level.data[0]));
    let cluster = level.data[1].to_bits() & 0x7FFF_FFFF != 0;
    let (px, py) = gm.clamp_unit_move(dx, dy);
    gm.store_unit_position(caster, px, py);
    for (k, _) in chosen {
        let (ux, uy) = {
            let u = gm.unit(k);
            (u.x, u.y)
        };
        let (tx, ty) = if cluster {
            (arith::add(dx, arith::sub(ux, cx)), arith::add(dy, arith::sub(uy, cy)))
        } else {
            (dx, dy)
        };
        let (px, py) = gm.clamp_unit_move(tx, ty);
        gm.store_unit_position(k, px, py);
    }
}

/// The experience a `CAbilityLevelMod` of `n` levels adds (`0x6F0DE200`): the hero ends `n` levels higher with the same
/// fraction of progress inside the level. `None` for a unit that is not a hero or when the tables are missing.
pub fn level_mod_amount(gm: &Game, unit: ObjKey, n: i32) -> Option<F32> {
    let h = gm.unit(unit).hero.as_ref()?;
    let tables = gm.order_tables()?;
    let xp = gm.hero_xp();
    Some(level_mod_value(h.level, h.xp, n, &|l| xp.need_xp(l), tables))
}

/// The arithmetic of [`level_mod_amount`] on plain numbers (Unicorn-checked against `0x6F0DE200`).
pub fn level_mod_value(level: i32, xp: i32, n: i32, need: &dyn Fn(i32) -> i32, t: &Tables) -> F32 {
    let esi = level.wrapping_sub(1);
    let top = n.wrapping_add(esi); // N + L - 1
    let target = top.wrapping_add(1); // N + L
    let a = if esi > 0 { need(esi) } else { 0 };
    let b = need(top);
    let c = need(target);
    let r1 = conv::i2r(b.wrapping_sub(a));
    let r2 = conv::i2r(xp.wrapping_sub(a));
    let f = arith::div(r2, r1, t);
    let r3 = conv::i2r(c.wrapping_sub(b));
    let prod = arith::mul(f, r3);
    let whole = conv::f2i(prod);
    conv::i2r(whole.wrapping_add(b.wrapping_sub(xp)))
}

/// Gives experience through the experience engine `0x6F20B080` (the tome of experience passes the range flag, which
/// applies the upgrade bonus; the model has no such upgrade, so it is a plain gain).
fn add_hero_xp(gm: &mut Game, unit: ObjKey, value: F32, range: bool) {
    let Some(tables) = gm.order_tables().cloned() else {
        return;
    };
    let xp = gm.hero_xp().clone();
    let owner = gm.unit(unit).owner;
    let handicap = gm.misc.players[(owner as usize).min(15)].handicap_xp;
    let caps = crate::kill_xp::caps_of(gm.unit(unit));
    let no_levels = |_: u32| 0i32;
    let no_value = |_: u32, _: i32| F32::ZERO;
    let env = EngineEnv {
        handicap,
        bonus: BonusEnv {
            b1: 0,
            slots: 0,
            codes: [0; 9],
            levels: &no_levels,
            value: &no_value,
        },
        caps,
    };
    if let Some(h) = gm.unit_mut(unit).hero.as_mut() {
        heroes::experience_engine(&xp, &tables, h, value, Gain { handicap: false, walk: false, range }, &env);
    }
}

/// The class effect of a cast item ability (called from `cast::run_effect`). Returns whether `rec.base` is an item
/// ability handled here.
pub(crate) fn run_effect(game: &RefCell<Game>, rec: &CastRec) -> bool {
    let mut gm = game.borrow_mut();
    let Some(fam) = gm.spells.item.family(rec.base) else {
        return false;
    };
    if !fam.castable() {
        return true;
    }
    let level = match fam {
        // The tomes read level 1 whatever the ability level.
        Family::TomeLife
        | Family::TomeMana
        | Family::TomeXp
        | Family::TomeLevel
        | Family::TomeAttrs
        | Family::TomeAttack => level_one(&gm, rec.base),
        _ => rec.level,
    };
    apply_effect(&mut gm, fam, &level, rec.caster, rec.target);
    true
}

// ---- the use path ----

/// The first castable ability of an item (`0x6F2B6CC0`: the first ability whose class answers as usable).
fn usable_alias(gm: &Game, item: ObjKey) -> Option<Option<u32>> {
    let spec = gm.spells.item.specs.get(&gm.item(item).type_id)?;
    Some(spec.abilities.iter().copied().find(|&a| gm.spells.item.family(a).is_some_and(Family::castable) || delegated(gm, a)))
}

/// Starts the use of `item` by `unit`: the cast of the item's first castable ability on `target`. `None` when the item
/// type has no registered item abilities (the caller keeps the plain recorded order). `Some(false)` when the item has
/// no castable ability or the cast does not start (the validator or the readiness refuses).
pub fn start_use(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    item: ObjKey,
    target: CastTarget,
) -> Option<bool> {
    let alias = {
        let gm = game.borrow();
        match usable_alias(&gm, item)? {
            Some(a) => a,
            None => return Some(false),
        }
    };
    {
        let mut gm = game.borrow_mut();
        if !gm.alive(unit) || gm.item(item).removed || gm.item(item).holder != Some(unit) {
            return Some(false);
        }
        // The item cools while its group cools on the holder (`0x6F2B6CC0`: the item's cooling flag `0x200` yields no order).
        let g = item_group(&gm, item);
        if g != 0 && gm.is_cooling(unit, g) {
            return Some(false);
        }
        gm.spells.item.pending.insert((unit, alias), item);
    }
    let ok = begin_cast(game, rt, unit, alias, target).is_ok();
    if !ok {
        game.borrow_mut().spells.item.pending.remove(&(unit, alias));
    }
    Some(ok)
}

/// The group cooldown a finished item cast starts (`0x6F05C840`, the item cast end, CONFIRMED structure, Unicorn `GR`
/// rows): the ability's `Cool` is handed to the unit's group routine (`0x6F0E3F30`) for the item named by the ability's
/// item key, unless the item type's `ignoreCD` flag (`0x6F2B8DA0`) zeroes it, and only when it is above 0.0 (the soft
/// compare `0x6F01FD40`).
pub fn group_cooldown_start(cool: F32, ignore: bool) -> Option<F32> {
    if ignore || !arith::lt(F32::ZERO, cool) {
        None
    } else {
        Some(cool)
    }
}

/// The cooldown group of an item: its type's `cooldownID` (the key the unit's group table compares, `0x6F2B8D70`).
fn item_group(gm: &Game, item: ObjKey) -> u32 {
    gm.spells.item.specs.get(&gm.item(item).type_id).map_or(0, |s| s.cooldown_id)
}

/// The end of a cast (`0x6F050950`): a cast started by a use order uses one charge when it completed (UNSURE for a
/// cast that was interrupted: the original marks the ability with flag `0x4000` and uses the charge at the end).
pub(crate) fn on_cast_end(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, completed: bool) {
    let item = {
        let mut gm = game.borrow_mut();
        let Some(item) = gm.spells.item.pending.remove(&(rec.caster, rec.base)) else {
            return;
        };
        if !completed {
            return;
        }
        // The cooldown group (CONFIRMED, see [`group_cooldown_start`]): keyed by the item type's `cooldownID`, started with
        // the ability's `Cool` for every item of the group the unit carries.
        let g = item_group(&gm, item);
        let ignore = gm.spells.item.specs.get(&gm.item(item).type_id).is_some_and(|s| s.ignore_cooldown);
        if g != 0 && g != rec.base {
            if let Some(cool) = group_cooldown_start(rec.level.cool, ignore) {
                gm.start_cooldown(rec.caster, g, cool);
            }
        }
        item
    };
    use_charge(game, rt, item);
}

/// The charge routine `0x6F2B8390` (CONFIRMED structure by disassembly, Unicorn `item_cores_emu.py`): an item with the
/// no-charges flag keeps its count; otherwise a count at or below 0 does nothing and any other count drops by one.
/// Then the holder is told (`0x6F299750`, event `0x80257`: the use-item events), and a perishable item whose count is
/// 0 is destroyed.
pub fn use_charge(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, item: ObjKey) {
    let holder;
    {
        let mut gm = game.borrow_mut();
        let it = gm.item_mut(item);
        if it.removed {
            return;
        }
        let Some((charges, _, _)) = charge_outcome(it.flags, it.charges, true) else {
            return;
        };
        it.charges = charges;
        holder = it.holder;
    }
    if let Some(unit) = holder {
        fire_use_events(game, rt, unit, item);
    }
    // The destroy test reads the flags and the count again, after the event handlers ran.
    let mut gm = game.borrow_mut();
    let it = gm.item(item);
    if destroys(it.flags, it.charges) && !it.removed {
        crate::natives::item::remove_item(&mut gm, item);
    }
}

/// The decrement of the charge routine: `(flags, new charges)`, or `None` when the routine returns early (charges at or
/// below 0 without the no-charges flag).
pub fn charge_step(flags: u32, charges: i32) -> (u32, Option<i32>) {
    if flags & item_flags::NO_CHARGES != 0 {
        return (flags, Some(charges));
    }
    if charges <= 0 {
        return (flags, None);
    }
    (flags, Some(charges - 1))
}

/// The destroy rule after a use: no no-charges flag, the perishable flag and no charge left.
pub fn destroys(flags: u32, charges: i32) -> bool {
    flags & item_flags::NO_CHARGES == 0 && flags & item_flags::PERISHABLE != 0 && charges == 0
}

/// Fires the use-item events: player-unit `0x32`, then unit `0x57` (the key `0x80257` is the unit one), with the unit
/// as manipulating unit and the item as manipulated item.
fn fire_use_events(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, item: ObjKey) {
    let owner = game.borrow().unit(unit).owner;
    for (agent, id) in [(ObjKey(owner), 0x32u32), (unit, 0x57u32)] {
        let firing = {
            let mut gm = game.borrow_mut();
            gm.firings.push(Firing {
                id,
                data: vec![
                    (Field::Manipulator, Datum::Object(unit)),
                    (Field::Manipulated, Datum::Object(item)),
                ],
            });
            (gm.firings.len() - 1) as u32
        };
        fire_observers_in(game, rt, agent, KEY_BASE + id, unit, Some(firing));
    }
}

// ---- helpers on Game used above ----

impl Game {
    /// The level record `index` (0 = level 1) of ability `alias` (an empty record when none is installed).
    pub(crate) fn spells_level_record(&self, alias: u32, index: usize) -> SpellLevel {
        self.spell_levels_of(alias).get(index).copied().unwrap_or_default()
    }
}
