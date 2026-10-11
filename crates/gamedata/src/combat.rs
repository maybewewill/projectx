//! Unit combat rows: weapon 1 and the defense of each unit type (`docs/spec/attack-damage.md`, sections 2 and 3.4).
//!
//! Sources: `Units\UnitWeapons.slk` (weapon 1: `dice1`, `sides1`, `dmgplus1`, `cool1`, `dmgpt1`, `backSw1`,
//! `rangeN1`, `atkType1`) and `Units\UnitBalance.slk` (defense: `def`, `defType`). Row keys are the unit raw codes
//! (see [`rawcode`]). Units without a weapon row, or without a defense row, get `None` for that part.
//!
//! Defaults for a missing cell (no note): integers 0, reals 0.0, `defType` = Normal, `atkType1` = Spells.
//! The attack-type default is the binary's own: its string-to-type reader (`0x6F262B00`) returns 0 (Spells) for an
//! empty string and for any name it does not know (CONFIRMED by the disassembly, `docs/spec/attack-damage.md`
//! section 2.1). The armor reader (`0x6F262C10`) returns 4 (Normal) for the same cases.
//! A malformed cell (not an integer, not a real) takes 0 and adds a note; an unknown non-empty type name takes the
//! same default as the binary and adds a note.
//! Only weapon 1 is read: weapon 2 is not in the attack record the simulation holds (UNSURE, see `sim::combat`).
//!
//! The armor-type column order of [`ArmorType`] is CONFIRMED (section 2.2 of the spec). UNSURE: the column names of
//! `dice1`/`sides1` as dice count and sides (the roll wrapper's argument order, spec section 3.3). The armor factor
//! does not depend on the attack type: a basic attack's damage class word is always `0x10` (CONFIRMED,
//! `sim::damage::ATTACK_DAMAGE_CLASS`), so no physical set is read from the data.

use std::collections::{BTreeMap, BTreeSet};

use softfloat::{Tables, F32};

use crate::real::{parse_int, parse_real, scalar};
use crate::Slk;

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
    /// Malformed cells, one line each.
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

/// Reads the combat rows. `balance` is `Units\UnitBalance.slk` and `weapons` is `Units\UnitWeapons.slk`; a missing
/// file is `None`. `tables` selects the S2R real parser (see [`crate::real`]).
pub fn load(
    balance: Option<&Slk>,
    weapons: Option<&Slk>,
    tables: Option<&Tables>,
) -> UnitCombatData {
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for slk in [balance, weapons].into_iter().flatten() {
        keys.extend(slk.rows().map(|row| row.key().to_owned()));
    }
    let mut out = UnitCombatData::default();
    for key in keys {
        let weapon = weapons.and_then(|slk| weapon_of(slk, &key, tables, &mut out.notes));
        let defense = balance.and_then(|slk| defense_of(slk, &key, tables, &mut out.notes));
        if weapon.is_some() || defense.is_some() {
            out.units.insert(key, UnitCombat { weapon, defense });
        }
    }
    out
}

fn weapon_of(
    slk: &Slk,
    key: &str,
    tables: Option<&Tables>,
    notes: &mut Vec<String>,
) -> Option<Weapon> {
    let present = ["dice1", "sides1", "dmgplus1"]
        .iter()
        .any(|col| slk.raw(key, col).is_some());
    if !present {
        return None;
    }
    let dice = int_cell(slk, key, "dice1", notes);
    let sides = int_cell(slk, key, "sides1", notes);
    if dice < 0 || sides < 0 {
        notes.push(format!("{key}: negative dice or sides; 0 used"));
    }
    let damage_type = match slk.raw(key, "atkType1") {
        None => DamageType::Spells,
        Some(raw) if scalar(raw).is_empty() => DamageType::Spells,
        Some(raw) => DamageType::from_name(scalar(raw)).unwrap_or_else(|| {
            notes.push(format!(
                "{key}.atkType1: {:?} unknown; Spells (0) used, as the binary does",
                scalar(raw)
            ));
            DamageType::Spells
        }),
    };
    Some(Weapon {
        dice: u32::try_from(dice).unwrap_or(0),
        sides: u32::try_from(sides).unwrap_or(0),
        dmg_plus: int_cell(slk, key, "dmgplus1", notes),
        cooldown: real_cell(slk, key, "cool1", tables, notes),
        damage_point: real_cell(slk, key, "dmgpt1", tables, notes),
        backswing: real_cell(slk, key, "backSw1", tables, notes),
        range: real_cell(slk, key, "rangeN1", tables, notes),
        damage_type,
    })
}

fn defense_of(
    slk: &Slk,
    key: &str,
    tables: Option<&Tables>,
    notes: &mut Vec<String>,
) -> Option<Defense> {
    if slk.raw(key, "def").is_none() && slk.raw(key, "defType").is_none() {
        return None;
    }
    let armor_type = match slk.raw(key, "defType") {
        None => ArmorType::Normal,
        Some(raw) => ArmorType::from_name(scalar(raw)).unwrap_or_else(|| {
            notes.push(format!(
                "{key}.defType: {:?} unknown; Normal used",
                scalar(raw)
            ));
            ArmorType::Normal
        }),
    };
    Some(Defense {
        armor: real_cell(slk, key, "def", tables, notes),
        armor_type,
    })
}

fn int_cell(slk: &Slk, key: &str, column: &str, notes: &mut Vec<String>) -> i32 {
    match slk.raw(key, column) {
        None => 0,
        Some(raw) => parse_int(raw).unwrap_or_else(|| {
            notes.push(format!(
                "{key}.{column}: {:?} is not an integer; 0 used",
                scalar(raw)
            ));
            0
        }),
    }
}

fn real_cell(
    slk: &Slk,
    key: &str,
    column: &str,
    tables: Option<&Tables>,
    notes: &mut Vec<String>,
) -> F32 {
    match slk.raw(key, column) {
        None => F32::ZERO,
        Some(raw) => parse_real(raw, tables).unwrap_or_else(|| {
            notes.push(format!(
                "{key}.{column}: {:?} is not a real; 0 used",
                scalar(raw)
            ));
            F32::ZERO
        }),
    }
}
