//! Ability definitions and their level records, read from the ability data table (`AbilityData.slk`).
//!
//! Layout and rules follow the definition builder of Game.dll 1.26a (decompile-driven, checked in Unicorn
//! by `tools/emu/abilitytype_binder_emu.py`, 94 checks, 0 mismatches). Spec: `docs/spec/abilities-3.md`.
//!
//! Per definition: `levels` (the number of levels, at definition offset `+0x3C`), `version`, `checkDep`,
//! `reqLevel`, `levelSkip`, `priority`. Per level `k` (1-based) the column for each field is read with the
//! level number appended (`Cast1`, `Cast2`, ...). This suffix rule is LIKELY (the original passes the level
//! number to the Storm cell reader as the row argument; the column naming is from the old notes).
//!
//! The level record is 0x68 bytes in the original (26 dwords). Its words, as this module stores them:
//! - `+0x00` target flags (text; the flag decoder of the original is not modelled, see [`AbilityLevel::targs`])
//! - `+0x04` Cast, `+0x08` Dur, `+0x0C` HeroDur (reals), `+0x10` Cost (integer)
//! - `+0x14` Cool: stored as `cool` when `0.0 <= cool`, otherwise `0.0` (NaN gives `0.0`)
//! - `+0x18` Area, `+0x1C` Rng, `+0x20..=+0x40` DataA..DataI (reals)
//! - `+0x44` UnitID as a raw code (0 when empty)
//! - `+0x48..+0x5C` BuffID and EfctID lists: the count is `max(1, strlen >> 2)`, the slots hold the raw
//!   codes of the non-empty tokens in order (separators ` ,-_` and tab) and the rest stay zero.
//!
//! Missing cells: integer 0, real 0.0, text empty (UNSURE, the original's default for a missing real cell is
//! not read).

use std::collections::BTreeMap;

use crate::real::atof_f32;
use crate::slk::Slk;
use crate::unittype::{cell_int_bits, unquote};

/// Size of one level record in the original.
pub const LEVEL_RECORD_SIZE: usize = 0x68;

/// Separators of the BuffID and EfctID lists (the original's token splitter set).
const LIST_SEPARATORS: &[char] = &[' ', ',', '-', '_', '\t'];

/// One level of an ability (one 0x68-byte record of the original).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AbilityLevel {
    /// Target flags text (`targs`, record `+0x00`). The original parses it with its target flag reader; the
    /// flag values are not decoded here (UNSURE).
    pub targs: String,
    /// Cast time (`+0x04`).
    pub cast: f32,
    /// Duration (`+0x08`).
    pub dur: f32,
    /// Hero duration (`+0x0C`).
    pub hero_dur: f32,
    /// Cost, the integer cell bits (`+0x10`).
    pub cost: i32,
    /// Cooldown after the clamp rule of the builder (`+0x14`).
    pub cool: f32,
    /// Area (`+0x18`).
    pub area: f32,
    /// Range (`+0x1C`).
    pub rng: f32,
    /// DataA..DataI (`+0x20..=+0x40`).
    pub data: [f32; 9],
    /// UnitID as a raw code, 0 when empty (`+0x44`).
    pub unit_id: u32,
    /// BuffID slots (`+0x48..+0x50`). Length is the list count of the original.
    pub buff_ids: Vec<u32>,
    /// EfctID slots (`+0x58..+0x60`).
    pub efct_ids: Vec<u32>,
}

/// One ability definition with its level records.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AbilityTypeData {
    /// Raw code of the ability (the row key).
    pub code: u32,
    /// `version` (definition `+0x1F8`).
    pub version: i32,
    /// `checkDep` as a flag (definition `+0x38`).
    pub check_dep: bool,
    /// `levels`: the maximum level (definition `+0x3C`).
    pub levels_max: i32,
    /// `reqLevel` (definition `+0x40`).
    pub req_level: i32,
    /// `levelSkip` (definition `+0x44`).
    pub level_skip: i32,
    /// `priority` (definition `+0x48`).
    pub priority: i32,
    /// Level records, index 0 is level 1.
    pub levels: Vec<AbilityLevel>,
}

/// The raw code of a text: up to four bytes, packed big-endian, stopped by the end of the text (the
/// original's parser, FUN_6F4C3F50, checked in Unicorn).
pub fn raw_code(text: &str) -> u32 {
    let mut v = 0u32;
    for &b in text.as_bytes().iter().take(4) {
        if b == 0 {
            break;
        }
        v = (v << 8) | u32::from(b);
    }
    v
}

/// The raw codes of a list text, with the builder's slot rule: `count = max(1, len >> 2)` slots, the first
/// non-empty tokens fill them in order, the rest are zero.
pub fn list_slots(text: Option<&str>) -> Vec<u32> {
    let text = text.unwrap_or("");
    let n = text.len() >> 2;
    let count = if n < 2 { 1 } else { n };
    let mut slots = vec![0u32; count];
    let tokens = text
        .split(LIST_SEPARATORS)
        .filter(|t| !t.is_empty())
        .map(raw_code);
    for (slot, code) in slots.iter_mut().zip(tokens) {
        *slot = code;
    }
    slots
}

/// The clamp of the builder for the cooldown: kept when `0.0 <= cool`, else `0.0` (NaN gives `0.0`).
pub fn clamp_cooldown(cool: f32) -> f32 {
    if 0.0 <= cool {
        cool
    } else {
        0.0
    }
}

fn cell<'a>(slk: &'a Slk, key: &str, column: &str) -> Option<&'a str> {
    let raw = slk.raw(key, column)?;
    let t = unquote(raw);
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn real(slk: &Slk, key: &str, column: &str) -> f32 {
    cell(slk, key, column).map_or(0.0, |t| atof_f32(t).unwrap_or(0.0))
}

fn int(slk: &Slk, key: &str, column: &str) -> i32 {
    cell(slk, key, column).map_or(0, |t| cell_int_bits(t) as i32)
}

/// Reads level `level` (1-based) of the row `key`.
pub fn read_level(slk: &Slk, key: &str, level: u32) -> AbilityLevel {
    let col = |name: &str| format!("{name}{level}");
    let data_names = [
        "DataA", "DataB", "DataC", "DataD", "DataE", "DataF", "DataG", "DataH", "DataI",
    ];
    let mut data = [0.0f32; 9];
    for (slot, name) in data.iter_mut().zip(data_names) {
        *slot = real(slk, key, &col(name));
    }
    AbilityLevel {
        targs: cell(slk, key, &col("targs")).unwrap_or("").to_owned(),
        cast: real(slk, key, &col("Cast")),
        dur: real(slk, key, &col("Dur")),
        hero_dur: real(slk, key, &col("HeroDur")),
        cost: int(slk, key, &col("Cost")),
        cool: clamp_cooldown(real(slk, key, &col("Cool"))),
        area: real(slk, key, &col("Area")),
        rng: real(slk, key, &col("Rng")),
        data,
        unit_id: cell(slk, key, &col("UnitID")).map_or(0, raw_code),
        buff_ids: list_slots(cell(slk, key, &col("BuffID"))),
        efct_ids: list_slots(cell(slk, key, &col("EfctID"))),
    }
}

/// Reads the definition of the row `key` with all its levels. `None` when the row is missing.
pub fn read_ability(slk: &Slk, key: &str) -> Option<AbilityTypeData> {
    slk.row(key)?;
    let levels_max = int(slk, key, "levels");
    let levels = (1..=levels_max.max(0) as u32)
        .map(|k| read_level(slk, key, k))
        .collect();
    Some(AbilityTypeData {
        code: raw_code(key),
        version: int(slk, key, "version"),
        check_dep: int(slk, key, "checkDep") != 0,
        levels_max,
        req_level: int(slk, key, "reqLevel"),
        level_skip: int(slk, key, "levelSkip"),
        priority: int(slk, key, "priority"),
        levels,
    })
}

/// The skill data of a row: `(levels, reqLevel, levelSkip)` (definition `+0x3C`, `+0x40`, `+0x44`), what a hero skill
/// slot copies (`0x6F20AA10`). `None` when the row is missing.
pub fn read_skill(slk: &Slk, key: &str) -> Option<(i32, i32, i32)> {
    slk.row(key)?;
    Some((int(slk, key, "levels"), int(slk, key, "reqLevel"), int(slk, key, "levelSkip")))
}

/// Reads every row of the table, keyed by raw code (deterministic order).
pub fn load_all(slk: &Slk) -> BTreeMap<u32, AbilityTypeData> {
    slk.rows()
        .filter_map(|row| read_ability(slk, row.key()))
        .map(|d| (d.code, d))
        .collect()
}
