//! Unit combat rows: weapon 1 and the defense of each unit type (`docs/spec/attack-damage.md`, sections 2 and 3.4).
//!
//! This module reads nothing itself. The rows come from [`crate::unittype`] (the single reader of the unit SLK
//! tables, `Units\UnitUI.slk`, `UnitData.slk`, `UnitBalance.slk`, `UnitWeapons.slk`): see [`from_units`]. A unit
//! type is a row here when its record is valid (see [`crate::unittype::load_unit_type`]) and it has a weapon row
//! or a balance row.
//!
//! - Weapon 1 is read from the record's weapon slot 1 (`dice1`, `sides1`, `dmgplus1`, `cool1`, `dmgpt1`, `backSw1`,
//!   `rangeN1`, `atkType1`). A unit without a `UnitWeapons` row has no weapon.
//! - Defense is read from the balance record (`def`, `defType`). A unit without a `UnitBalance` row has no defense.
//! - Weapon 2 is in the record but not here: the simulation's attack record holds one weapon per unit type (UNSURE
//!   how the original picks between two weapons; see `sim::combat`).
//!
//! Notes are kept for the two names the binary's readers do not know: an unknown `atkType` takes Spells (0) and an
//! unknown `defType` takes Normal (4), as the binary does (`0x6F262B00` / `0x6F262C10`, CONFIRMED by the disassembly,
//! `docs/spec/attack-damage.md` section 2.1), and a negative dice count or sides count takes 0.
//!
//! The armor-type column order of [`ArmorType`] is CONFIRMED (section 2.2 of the spec). UNSURE: the column names of
//! `dice1`/`sides1` as dice count and sides (the roll wrapper's argument order, spec section 3.3). The armor factor
//! does not depend on the attack type: a basic attack's damage class word is always `0x10` (CONFIRMED,
//! `sim::damage::ATTACK_DAMAGE_CLASS`), so no physical set is read from the data.

use std::collections::BTreeMap;

use softfloat::F32;

use crate::unittype::UnitTypeData;

/// The attack-type rows of the damage-type table, in the binary order of the spec (section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DamageType {
    Spells = 0,
    Normal = 1,
    Pierce = 2,
    Siege = 3,
    Magic = 4,
    Chaos = 5,
    Hero = 6,
}

impl DamageType {
    /// The row of the damage-type table.
    pub fn row(self) -> usize {
        usize::from(self as u8)
    }

    /// The type named `name` (case-insensitive), or `None` for a name the binary does not know. `unknown` is
    /// Spells in the binary (`0x6F262B00`, CONFIRMED). `melee` is not a name the binary reads (it is `None`).
    pub fn from_name(name: &str) -> Option<DamageType> {
        Some(match name.to_ascii_lowercase().as_str() {
            "spells" | "unknown" => DamageType::Spells,
            "normal" => DamageType::Normal,
            "pierce" => DamageType::Pierce,
            "siege" => DamageType::Siege,
            "magic" => DamageType::Magic,
            "chaos" => DamageType::Chaos,
            "hero" => DamageType::Hero,
            _ => return None,
        })
    }
}

/// The armor-type columns of the damage-type table. The order is CONFIRMED by the string reader `0x6F262C10`:
/// small 0, medium 1, large 2, fort 3, normal 4, hero 5, divine 6, none 7. Unknown names and an empty string are
/// Normal (4) in the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArmorType {
    Small = 0,
    Medium = 1,
    Large = 2,
    Fort = 3,
    Normal = 4,
    Hero = 5,
    Divine = 6,
    /// The `none` type (column 7).
    Unarmored = 7,
}

impl ArmorType {
    /// The column of the damage-type table.
    pub fn column(self) -> u8 {
        self as u8
    }

    /// The type named `name` (case-insensitive), or `None` for a name the binary does not know. `unknown` is Normal.
    pub fn from_name(name: &str) -> Option<ArmorType> {
        Some(match name.to_ascii_lowercase().as_str() {
            "unknown" => ArmorType::Normal,
            "small" => ArmorType::Small,
            "medium" => ArmorType::Medium,
            "large" => ArmorType::Large,
            "fort" => ArmorType::Fort,
            "normal" => ArmorType::Normal,
            "hero" => ArmorType::Hero,
            "divine" => ArmorType::Divine,
            "none" => ArmorType::Unarmored,
            _ => return None,
        })
    }
}

/// Weapon 1 of a unit type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weapon {
    /// Number of dice (`dice1`).
    pub dice: u32,
    /// Sides of each die (`sides1`).
    pub sides: u32,
    /// Flat bonus added to the dice total (`dmgplus1`).
    pub dmg_plus: i32,
    /// Cooldown in seconds (`cool1`).
    pub cooldown: F32,
    /// Damage point in seconds (`dmgpt1`).
    pub damage_point: F32,
    /// Backswing in seconds (`backSw1`).
    pub backswing: F32,
    /// Attack range in world units (`rangeN1`).
    pub range: F32,
    /// Attack type (`atkType1`).
    pub damage_type: DamageType,
}

/// The defense of a unit type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Defense {
    /// Armor value (`def`). Negative values use the negative-armor table.
    pub armor: F32,
    /// Armor type (`defType`).
    pub armor_type: ArmorType,
}

/// The combat rows of one unit type.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitCombat {
    /// Weapon 1, or `None` when the type has no weapon row.
    pub weapon: Option<Weapon>,
    /// The defense, or `None` when the type has no balance row.
    pub defense: Option<Defense>,
}

/// The combat rows of all unit types, keyed by the raw code text (`hfoo`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitCombatData {
    units: BTreeMap<String, UnitCombat>,
    /// Unknown names and negative counts, one line each.
    pub notes: Vec<String>,
}

impl UnitCombatData {
    /// The rows of the unit type with raw code `code`.
    pub fn get(&self, code: &str) -> Option<&UnitCombat> {
        self.units.get(code)
    }

    /// All unit types with rows, in code order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &UnitCombat)> + '_ {
        self.units.iter().map(|(code, row)| (code.as_str(), row))
    }

    /// The number of unit types with rows.
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// Whether no unit type has a row.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
}

/// The 32-bit raw id of a four-character unit code: the characters packed big-endian, as the JASS lexer packs
/// rawcodes. `None` for any other length or non-ASCII text.
pub fn rawcode(code: &str) -> Option<u32> {
    let b = code.as_bytes();
    if b.len() != 4 || !b.is_ascii() {
        return None;
    }
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Builds the combat rows from the unit type records (see [`crate::unittype::load_all`]).
pub fn from_units(units: &BTreeMap<String, UnitTypeData>) -> UnitCombatData {
    let mut out = UnitCombatData::default();
    for (code, ty) in units {
        let weapon = weapon_of(code, ty, &mut out.notes);
        let defense = defense_of(code, ty, &mut out.notes);
        if weapon.is_some() || defense.is_some() {
            out.units.insert(code.clone(), UnitCombat { weapon, defense });
        }
    }
    out
}

/// The `f32` of a record value as the soft float the simulation uses (same bits).
fn soft(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

fn weapon_of(code: &str, ty: &UnitTypeData, notes: &mut Vec<String>) -> Option<Weapon> {
    if !ty.has_weapons_row {
        return None;
    }
    let slot = &ty.weapons[0];
    if slot.dice < 0 || slot.sides < 0 {
        notes.push(format!("{code}: negative dice or sides; 0 used"));
    }
    let damage_type = if slot.atk_type.is_empty() {
        DamageType::Spells
    } else {
        DamageType::from_name(&slot.atk_type).unwrap_or_else(|| {
            notes.push(format!(
                "{code}.atkType1: {:?} unknown; Spells (0) used, as the binary does",
                slot.atk_type
            ));
            DamageType::Spells
        })
    };
    Some(Weapon {
        dice: u32::try_from(slot.dice).unwrap_or(0),
        sides: u32::try_from(slot.sides).unwrap_or(0),
        dmg_plus: slot.dmg_plus,
        cooldown: soft(slot.cool),
        damage_point: soft(slot.dmg_pt),
        backswing: soft(slot.back_swing),
        range: soft(slot.range),
        damage_type,
    })
}

fn defense_of(code: &str, ty: &UnitTypeData, notes: &mut Vec<String>) -> Option<Defense> {
    if !ty.has_balance_row {
        return None;
    }
    let armor_type = if ty.armor_type.is_empty() {
        ArmorType::Normal
    } else {
        ArmorType::from_name(&ty.armor_type).unwrap_or_else(|| {
            notes.push(format!(
                "{code}.defType: {:?} unknown; Normal used",
                ty.armor_type
            ));
            ArmorType::Normal
        })
    };
    Some(Defense {
        armor: soft(ty.armor),
        armor_type,
    })
}
