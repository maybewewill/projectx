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
use crate::natives::game::{item_flags, Game, ObjKey};
use crate::natives::{fire_observers_in, set_life, ScriptRuntime, KEY_BASE};

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
    /// `cooldownID`: the ability id whose cooldown the item shares (0 = its own first ability).
    pub cooldown_id: u32,
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
    /// Misc `IllusionsCanRestoreLife`-like switch of the area callbacks (UNSURE key).
    pub illusions_heal: bool,
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
        }
        for (&alias, &base) in &data.bases {
            self.set_item_ability_base(alias, base);
        }
        for (&alias, levels) in &data.levels {
            self.set_spell_levels(alias, levels.clone());
        }
    }

    /// Registers the abilities of item type `type_id`.
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
            let v = conv::f2i(a);
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
            let (agi, int, str) = (conv::f2i(level.data[0]), conv::f2i(level.data[1]), conv::f2i(level.data[2]));
            add_attributes(gm, unit, agi, int, str);
            Applied::Attrs { agi, int, str }
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
            friend && (!u.illusion || gm.spells.item.illusions_heal)
        })
        .collect()
}

/// Runs the effect of `fam` for `caster` (state only: no events are needed, so the powerup path shares it).
fn apply_effect(gm: &mut Game, fam: Family, level: &SpellLevel, caster: ObjKey, target: CastTarget) {
    let d = &level.data;
    match fam {
        Family::Heal => {
            let t = effect_target(caster, target);
            if gm.alive(t) {
                add_life(gm, t, d[0]);
            }
        }
        Family::Mana => {
            let t = effect_target(caster, target);
            if gm.alive(t) {
                add_mana(gm, t, d[0]);
            }
        }
        Family::Restore => {
            let t = effect_target(caster, target);
            if gm.alive(t) {
                add_life(gm, t, d[0]);
                add_mana(gm, t, d[1]);
            }
        }
        Family::HealAoe | Family::ManaAoe | Family::RestoreAoe => {
            for k in area_units(gm, caster, target, level) {
                match fam {
                    Family::HealAoe => add_life(gm, k, d[0]),
                    Family::ManaAoe => add_mana(gm, k, d[0]),
                    _ => {
                        add_life(gm, k, d[0]);
                        add_mana(gm, k, d[1]);
                    }
                }
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
            add_attributes(gm, t, conv::f2i(d[0]), conv::f2i(d[1]), conv::f2i(d[2]));
        }
        Family::TomeAttack => {
            let t = effect_target(caster, target);
            // `0x6F0DE190`: only a unit with an attack ability; the value goes into both weapons' base bonuses.
            gm.add_damage(t, conv::f2i(d[0]));
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
    Some(spec.abilities.iter().copied().find(|&a| gm.spells.item.family(a).is_some_and(Family::castable)))
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
        gm.spells.item.pending.insert((unit, alias), item);
    }
    let ok = begin_cast(game, rt, unit, alias, target).is_ok();
    if !ok {
        game.borrow_mut().spells.item.pending.remove(&(unit, alias));
    }
    Some(ok)
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
        // The cooldown group (UNSURE): the shared id cools as long as the ability's own cooldown.
        if let Some(&g) = gm.spells.item.groups.get(&rec.base) {
            gm.start_cooldown(rec.caster, g, rec.level.cool);
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
        let (flags, charges) = charge_step(it.flags, it.charges);
        let Some(charges) = charges else {
            return;
        };
        it.charges = charges;
        let _ = flags;
        holder = it.holder;
    }
    if let Some(unit) = holder {
        fire_use_events(game, rt, unit, item);
    }
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
