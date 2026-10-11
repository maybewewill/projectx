//! Text and object data of the game files that the script natives read, loaded at boot from the user's own archives
//! (nothing is committed): the global string table behind `GetLocalizedString`, the object names behind
//! `GetObjectName`, the ability art and sound lists behind `GetAbilityEffectById` / `GetAbilitySoundById`, the item
//! and unit stock data behind the shop natives, and the gold amount of the gold mine ability.
//!
//! Sources (checked against the original, see `docs/spec/real-map-boot.md`):
//! - `GetLocalizedString` (`0x6F3BACF0`) substitutes `TRIGSTR_nnn`, then looks the key up in the engine's string
//!   table (`0x6F5C9650` -> `0x6F5C8ED0`), which the frame definition loader fills from the `StringList` blocks of
//!   the FDF files listed in `UI\FrameDef\FrameDef.toc` (`GlobalStrings.fdf` and others). Keys are case-insensitive.
//! - The object records come from the profile files of the pointer table at `0x6F936B10` (Units\*Func.txt and
//!   Units\*Strings.txt; the order of the table is the load order, a later file wins per key). `GetObjectName`
//!   (`0x6F3BB6B0`) reads index 0 of the `Name` list of the record and returns null for a missing or empty name.
//!   The art getters (`0x6F32E2D0..0x6F32E450`) read the lists `effectArt`, `targetArt`, `casterArt`, `specialArt`,
//!   `areaEffectArt`, `missileArt` and `lightningEffect` (effect types 0 to 6); the sound getters read `effectSound`
//!   and `effectSoundLooped`. Each is a comma list; an index above the last is clamped to the last entry.
//! - Shop stock: `Units\ItemData.slk` columns `stockMax` / `stockRegen` (items) and the unit type records of
//!   `Units\UnitBalance.slk` (units, read by `gamedata::unittype`).
//! - `Agld` (gold mine): `Units\AbilityData.slk`, level 1 `DataA` is the starting gold (UNSURE: the ability class
//!   that reads it was not traced; the column is the ability's first data value).
//!
//! Not modelled: the map's own object data (`war3map.w3u` / `w3a` / `w3t`, `war3mapSkin.txt`), which override these
//! records in the original.

use std::collections::BTreeMap;

use gamedata::{Profile, Slk};
use sim::shop::TypeStock;
use sim::Game;
use softfloat::F32;

use crate::Error;

/// The FDF table of contents of the frame definition loader (`0x6F875FBC`).
pub const FRAMEDEF_TOC: &str = "UI\\FrameDef\\FrameDef.toc";
/// The item table.
pub const ITEM_DATA_FILE: &str = "Units\\ItemData.slk";
/// The ability table.
pub const ABILITY_DATA_FILE: &str = "Units\\AbilityData.slk";
/// The gold mine ability.
pub const GOLD_MINE_ABILITY: &str = "Agld";

/// The object profile files in the load order of the original's pointer table (`0x6F936B10`).
pub const OBJECT_PROFILES: [&str; 45] = [
    "Units\\CampaignUnitStrings.txt",
    "Units\\HumanUnitStrings.txt",
    "Units\\NeutralUnitStrings.txt",
    "Units\\NightElfUnitStrings.txt",
    "Units\\OrcUnitStrings.txt",
    "Units\\UndeadUnitStrings.txt",
    "Units\\UnitGlobalStrings.txt",
    "Units\\CampaignUnitFunc.txt",
    "Units\\HumanUnitFunc.txt",
    "Units\\NeutralUnitFunc.txt",
    "Units\\NightElfUnitFunc.txt",
    "Units\\OrcUnitFunc.txt",
    "Units\\UndeadUnitFunc.txt",
    "Units\\CampaignAbilityStrings.txt",
    "Units\\CommonAbilityStrings.txt",
    "Units\\HumanAbilityStrings.txt",
    "Units\\NeutralAbilityStrings.txt",
    "Units\\NightElfAbilityStrings.txt",
    "Units\\OrcAbilityStrings.txt",
    "Units\\UndeadAbilityStrings.txt",
    "Units\\ItemAbilityStrings.txt",
    "Units\\CampaignAbilityFunc.txt",
    "Units\\CommonAbilityFunc.txt",
    "Units\\HumanAbilityFunc.txt",
    "Units\\NeutralAbilityFunc.txt",
    "Units\\NightElfAbilityFunc.txt",
    "Units\\OrcAbilityFunc.txt",
    "Units\\UndeadAbilityFunc.txt",
    "Units\\ItemAbilityFunc.txt",
    "Units\\CampaignUpgradeStrings.txt",
    "Units\\HumanUpgradeStrings.txt",
    "Units\\NightElfUpgradeStrings.txt",
    "Units\\OrcUpgradeStrings.txt",
    "Units\\UndeadUpgradeStrings.txt",
    "Units\\NeutralUpgradeStrings.txt",
    "Units\\CampaignUpgradeFunc.txt",
    "Units\\HumanUpgradeFunc.txt",
    "Units\\NightElfUpgradeFunc.txt",
    "Units\\OrcUpgradeFunc.txt",
    "Units\\UndeadUpgradeFunc.txt",
    "Units\\NeutralUpgradeFunc.txt",
    "Units\\CommandStrings.txt",
    "Units\\CommandFunc.txt",
    "Units\\ItemStrings.txt",
    "Units\\ItemFunc.txt",
];

/// The art keys in effect-type order 0..=6 (`GetAbilityEffectById`).
pub const ART_KEYS: [&str; 7] = [
    "effectArt",
    "targetArt",
    "casterArt",
    "specialArt",
    "areaEffectArt",
    "missileArt",
    "lightningEffect",
];
/// The sound keys in sound-type order 0..=1.
pub const SOUND_KEYS: [&str; 2] = ["effectSound", "effectSoundLooped"];

/// What was read.
#[derive(Debug, Clone, Default)]
pub struct ObjectText {
    /// The global string table (key lower-case).
    pub localized: BTreeMap<String, String>,
    /// Object names by raw code.
    pub names: BTreeMap<u32, String>,
    /// Ability art lists by (raw code, effect type).
    pub art: BTreeMap<(u32, u32), Vec<String>>,
    /// Ability sound lists by (raw code, 0 effect / 1 looping).
    pub sound: BTreeMap<(u32, u32), Vec<String>>,
    /// Item stock data by raw code.
    pub item_stock: BTreeMap<u32, TypeStock>,
    /// The starting gold of `Agld` (level 1 `DataA`), when the ability table has the row.
    pub gold_mine_amount: Option<i32>,
    /// Missing or malformed files, one line each.
    pub notes: Vec<String>,
}

/// Packs a four-byte text big-endian (a raw code); `None` unless it has exactly four bytes.
fn raw_code(text: &str) -> Option<u32> {
    let b = text.as_bytes();
    (b.len() == 4).then(|| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_last(archives: &[&[u8]], name: &str) -> Result<Option<Vec<u8>>, Error> {
    let mut found = None;
    for (index, bytes) in archives.iter().enumerate() {
        let archive = mpq::Archive::open(bytes).map_err(|source| Error::Archive { index, source })?;
        match archive.read(name) {
            Ok(data) => found = Some(data),
            Err(mpq::Error::NotFound(_)) => {}
            Err(source) => return Err(Error::Archive { index, source }),
        }
    }
    Ok(found)
}

/// Reads everything from the game archives (a later archive overrides an earlier one).
pub fn load(archives: &[&[u8]]) -> Result<ObjectText, Error> {
    let mut out = ObjectText::default();
    load_global_strings(archives, &mut out)?;
    load_profiles(archives, &mut out)?;
    load_items(archives, &mut out)?;
    load_gold_mine(archives, &mut out)?;
    Ok(out)
}

fn load_global_strings(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let Some(toc) = read_last(archives, FRAMEDEF_TOC)? else {
        out.notes.push(format!("{FRAMEDEF_TOC} missing: GetLocalizedString has no table"));
        return Ok(());
    };
    let entries = fdf::parse_toc(&gamedata::decode_text(&toc));
    let mut library = fdf::Library::new();
    let mut failed = 0usize;
    // The loader hands out files from the archives; a parse error is noted per top-level file and the strings read
    // before the error stay (the library adds a document's items in order).
    let mut loader = |path: &str| -> Option<String> {
        read_last(archives, path).ok().flatten().map(|b| gamedata::decode_text(&b))
    };
    for entry in &entries {
        if let Err(e) = library.load(entry, &mut loader) {
            failed += 1;
            if failed <= 5 {
                out.notes.push(format!("{entry}: {e}"));
            }
        }
    }
    if failed > 5 {
        out.notes.push(format!("{} more FDF files failed to load", failed - 5));
    }
    out.localized = library.strings().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Ok(())
}

fn load_profiles(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let mut merged = Profile::default();
    for name in OBJECT_PROFILES {
        let Some(bytes) = read_last(archives, name)? else {
            out.notes.push(format!("{name} missing"));
            continue;
        };
        // Lenient: the shipped files have stray lines (`/ Stuffed Penguin`) that the original's reader ignores.
        merged.merge(&Profile::parse_lenient_bytes(&bytes));
    }
    for section in merged.sections() {
        let Some(code) = raw_code(section) else { continue };
        if let Some(name) = merged.get_list_ci(section, "Name").into_iter().next() {
            out.names.insert(code, name);
        }
        for (kind, key) in ART_KEYS.iter().enumerate() {
            let list = merged.get_list_ci(section, key);
            if !list.is_empty() {
                out.art.insert((code, kind as u32), list);
            }
        }
        for (kind, key) in SOUND_KEYS.iter().enumerate() {
            let list = merged.get_list_ci(section, key);
            if !list.is_empty() {
                out.sound.insert((code, kind as u32), list);
            }
        }
    }
    Ok(())
}

fn load_items(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let Some(bytes) = read_last(archives, ITEM_DATA_FILE)? else {
        out.notes.push(format!("{ITEM_DATA_FILE} missing: no item stock data"));
        return Ok(());
    };
    let slk = match Slk::parse_bytes(&bytes) {
        Ok(s) => s,
        Err(e) => {
            out.notes.push(format!("{ITEM_DATA_FILE}: {e}; treated as missing"));
            return Ok(());
        }
    };
    for row in slk.rows() {
        let Some(code) = raw_code(row.key()) else { continue };
        out.item_stock.insert(code, item_stock(row.raw("stockMax"), row.raw("stockRegen")));
    }
    Ok(())
}

/// The stock data of an item row. A missing or unreadable cell reads as 0 (`stockMax`) / 0.0 (`stockRegen`).
/// UNSURE: the integer and real cell readers of the item table are the unit table's (`strtol`, `atof`).
pub fn item_stock(stock_max: Option<&str>, stock_regen: Option<&str>) -> TypeStock {
    let max = stock_max
        .and_then(|t| t.trim().trim_matches('"').parse::<i32>().ok())
        .unwrap_or(0);
    let regen = stock_regen
        .and_then(|t| gamedata::real::atof_f32(t.trim().trim_matches('"')))
        .unwrap_or(0.0);
    TypeStock { stock_max: max, regen: F32::from_bits(regen.to_bits()) }
}

fn load_gold_mine(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let Some(bytes) = read_last(archives, ABILITY_DATA_FILE)? else {
        out.notes.push(format!("{ABILITY_DATA_FILE} missing: gold mines start at 0"));
        return Ok(());
    };
    let slk = match Slk::parse_bytes(&bytes) {
        Ok(s) => s,
        Err(e) => {
            out.notes.push(format!("{ABILITY_DATA_FILE}: {e}; treated as missing"));
            return Ok(());
        }
    };
    if slk.row(GOLD_MINE_ABILITY).is_some() {
        let level = gamedata::abilitytype::read_level(&slk, GOLD_MINE_ABILITY, 1);
        out.gold_mine_amount = Some(level.data[0] as i32);
    } else {
        out.notes.push(format!("{ABILITY_DATA_FILE}: no {GOLD_MINE_ABILITY} row"));
    }
    Ok(())
}

/// Installs the tables into `game`. `unit_stock` is (raw code, stockMax, stockRegen) of the unit types.
pub fn install(game: &mut Game, text: &ObjectText, unit_stock: &[(u32, i32, f32)]) {
    game.lane.localized = text.localized.clone();
    game.lane.object_names = text.names.clone();
    game.lane.ability_art = text.art.clone();
    game.lane.ability_sound = text.sound.clone();
    for (&code, &ts) in &text.item_stock {
        game.shops.item_stock.insert(code, ts);
    }
    for &(code, max, regen) in unit_stock {
        game.shops
            .unit_stock
            .insert(code, TypeStock { stock_max: max, regen: F32::from_bits(regen.to_bits()) });
    }
    if let Some(amount) = text.gold_mine_amount {
        if let Some(code) = raw_code(GOLD_MINE_ABILITY) {
            game.set_ability_resource_amount(code, amount);
        }
    }
}
