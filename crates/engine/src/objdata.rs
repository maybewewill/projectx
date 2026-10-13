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
/// The upgrade (research) table.
pub const UPGRADE_DATA_FILE: &str = "Units\\UpgradeData.slk";
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

/// The projectile columns of a unit's profile section, one entry per weapon.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MissileSpec {
    /// `Missilespeed` (world units per second).
    pub speed: Vec<f32>,
    /// `Missilearc` (a fraction of the distance; visual).
    pub arc: Vec<f32>,
    /// `MissileHoming`.
    pub homing: Vec<bool>,
}

/// What was read.
#[derive(Debug, Clone, Default)]
pub struct ObjectText {
    /// The projectile columns by unit raw code.
    pub missiles: BTreeMap<u32, MissileSpec>,
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
    /// The level records of the spells the cast core knows (`sim::cast`), by raw code.
    pub spell_levels: BTreeMap<u32, Vec<sim::cast::SpellLevel>>,
    /// The skill data `(levels, reqLevel, levelSkip)` of every ability row, by raw code (hero skill slots).
    pub ability_skill: BTreeMap<u32, (i32, i32, i32)>,
    /// The levels of the aura abilities the model knows (`sim::auras::AuraKind`), by raw code: `Area`, `DataA` to
    /// `DataD`, the first `BuffID` and `targs`.
    pub auras: BTreeMap<u32, Vec<sim::auras::AuraLevel>>,
    /// What each unit type trains, builds and researches, and what it requires (`Trains`, `Builds`, `Researches`,
    /// `Requires`, `Upgrade` of the unit profiles), by raw code.
    pub tech: BTreeMap<u32, sim::production::TypeTech>,
    /// The upgrade rows (`UpgradeData.slk`) by raw code.
    pub upgrades: BTreeMap<u32, sim::production::UpgradeType>,
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
    load_upgrades(archives, &mut out)?;
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
        let codes = |key: &str| -> Vec<u32> {
            merged
                .get_list_ci(section, key)
                .iter()
                .filter_map(|c| raw_code(c.trim()))
                .collect()
        };
        let mut tech = sim::production::TypeTech {
            trains: codes("Trains"),
            builds: codes("Builds"),
            researches: codes("Researches"),
            upgrade_to: codes("Upgrade"),
            ..Default::default()
        };
        let amounts: Vec<i32> = merged
            .get_list_ci(section, "Requiresamount")
            .iter()
            .map(|a| a.trim().parse().unwrap_or(1))
            .collect();
        tech.requires = codes("Requires")
            .into_iter()
            .enumerate()
            .map(|(i, c)| (c, amounts.get(i).copied().unwrap_or(1)))
            .collect();
        // A hero type lists one requirement set per hero already owned: `Requires`, `Requires1`, ...
        let tiers = merged
            .get_ci(section, "Requirescount")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        for n in 0..tiers {
            let key = if n == 0 { "Requires".to_string() } else { format!("Requires{n}") };
            tech.hero_requires.push(codes(&key));
        }
        if tech != sim::production::TypeTech::default() {
            out.tech.insert(code, tech);
        }
        // The projectile of the unit's weapons: `Missilespeed`, `Missilearc` and `MissileHoming`, one entry per weapon
        // (comma list, `docs/spec/missiles.md` section 11).
        let reals = |key: &str| -> Vec<f32> {
            merged
                .get_list_ci(section, key)
                .iter()
                .filter_map(|v| v.trim().parse::<f32>().ok())
                .collect()
        };
        let speed = reals("Missilespeed");
        if !speed.is_empty() {
            let homing = merged
                .get_list_ci(section, "MissileHoming")
                .iter()
                .map(|v| v.trim().parse::<i32>().unwrap_or(0) != 0)
                .collect();
            out.missiles.insert(code, MissileSpec { speed, arc: reals("Missilearc"), homing });
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

fn load_upgrades(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let Some(bytes) = read_last(archives, UPGRADE_DATA_FILE)? else {
        out.notes.push(format!("{UPGRADE_DATA_FILE} missing: no research data"));
        return Ok(());
    };
    let slk = match Slk::parse_bytes(&bytes) {
        Ok(s) => s,
        Err(e) => {
            out.notes.push(format!("{UPGRADE_DATA_FILE}: {e}; treated as missing"));
            return Ok(());
        }
    };
    for row in slk.rows() {
        let Some(code) = raw_code(row.key()) else { continue };
        let int = |col: &str| -> i32 {
            row.raw(col).and_then(|t| t.trim().trim_matches('"').parse::<i32>().ok()).unwrap_or(0)
        };
        out.upgrades.insert(
            code,
            sim::production::UpgradeType {
                gold_base: int("goldbase"),
                gold_mod: int("goldmod"),
                lumber_base: int("lumberbase"),
                lumber_mod: int("lumbermod"),
                time_base: int("timebase"),
                time_mod: int("timemod"),
                max_level: int("maxlevel"),
            },
        );
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

/// The aura classes whose levels are read from the ability table (`sim::auras`).
const AURA_KINDS: [sim::auras::AuraKind; 6] = [
    sim::auras::AuraKind::Devotion,
    sim::auras::AuraKind::Endurance,
    sim::auras::AuraKind::Unholy,
    sim::auras::AuraKind::Brilliance,
    sim::auras::AuraKind::Command,
    sim::auras::AuraKind::Trueshot,
];

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
    for row in slk.rows() {
        if let Some(s) = gamedata::abilitytype::read_skill(&slk, row.key()) {
            out.ability_skill.insert(gamedata::abilitytype::raw_code(row.key()), s);
        }
    }
    for (name, code) in [
        ("AHtb", sim::cast::STORM_BOLT),
        ("AHhb", sim::cast::HOLY_LIGHT),
        ("Aslo", sim::cast::SLOW),
        ("AEev", sim::abilities::EVASION),
        ("AOcr", sim::orc::CRITICAL_STRIKE),
    ] {
        if let Some(def) = gamedata::abilitytype::read_ability(&slk, name) {
            let f = |v: f32| F32::from_bits(v.to_bits());
            let levels = def
                .levels
                .iter()
                .map(|l| {
                    let mut data = [F32::ZERO; 9];
                    for (d, v) in data.iter_mut().zip(l.data.iter()) {
                        *d = f(*v);
                    }
                    sim::cast::SpellLevel {
                        cast: f(l.cast),
                        dur: f(l.dur),
                        hero_dur: f(l.hero_dur),
                        cost: l.cost,
                        cool: f(l.cool),
                        area: f(l.area),
                        rng: f(l.rng),
                        data,
                    }
                })
                .collect();
            out.spell_levels.insert(code, levels);
        }
    }
    for kind in AURA_KINDS {
        let base = kind.ability();
        let key = String::from_utf8_lossy(&base.to_be_bytes()).into_owned();
        if let Some(data) = gamedata::abilitytype::read_ability(&slk, &key) {
            let f = |x: f32| F32::from_bits(x.to_bits());
            let levels = data
                .levels
                .iter()
                .map(|l| sim::auras::AuraLevel {
                    area: f(l.area),
                    data: [f(l.data[0]), f(l.data[1]), f(l.data[2]), f(l.data[3])],
                    buff: l.buff_ids.first().copied().unwrap_or(0),
                    targs: l.targs.clone(),
                })
                .collect();
            out.auras.insert(base, levels);
        }
    }
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
    // The tech data merges into what `data::install` set (the harvest flags).
    for (&code, t) in &text.tech {
        let e = game.production_mut().tech.entry(code).or_default();
        e.trains = t.trains.clone();
        e.builds = t.builds.clone();
        e.researches = t.researches.clone();
        e.requires = t.requires.clone();
        e.hero_requires = t.hero_requires.clone();
        e.upgrade_to = t.upgrade_to.clone();
    }
    for (&code, &u) in &text.upgrades {
        game.production_mut().upgrades.insert(code, u);
    }
    // The first weapon's projectile. Only the plain missile type (`weapTp` 2, CONFIRMED launch path) gets a timed flight;
    // the other flying kinds keep the instant hit (their launch routines are not decoded).
    for (&code, spec) in &text.missiles {
        let plain_missile = game
            .unit_types()
            .get(code)
            .is_some_and(|t| sim::missiles::MissileType::from_weap_tp(t.weapon_kind[0])
                == Some(sim::missiles::MissileType::Missile));
        let mut md = game.missile_data(code);
        md.arc = spec.arc.first().map_or(F32::ZERO, |&a| F32::from_bits(a.to_bits()));
        md.homing = spec.homing.first().copied().unwrap_or(false);
        game.set_missile_data(code, md);
        if plain_missile {
            let mut at = game.attack_type(code);
            at.missile_speed = Some(F32::from_bits(spec.speed[0].to_bits()));
            game.set_attack_type(code, at);
        }
    }
    for (&code, levels) in &text.spell_levels {
        game.set_spell_levels(code, levels.clone());
    }
    for (&base, levels) in &text.auras {
        game.aura_types_mut().insert(base, levels.clone());
    }
    if let Some(amount) = text.gold_mine_amount {
        if let Some(code) = raw_code(GOLD_MINE_ABILITY) {
            game.set_ability_resource_amount(code, amount);
        }
    }
}

/// Installs the trees of the map for the production model: the doodads of `war3map.doo` whose type is a tree.
/// A type is a tree when its second letter is `T`, as in `LTlt`, `ATtr`, `ZTtw` (UNSURE heuristic: the category
/// column of `DestructableData.slk` is `D` for these rows, so it cannot tell trees apart). Each tree holds a fixed amount of lumber
/// (`sim::production::TREE_WOOD`, a stand-in). Returns the number of trees installed.
pub fn install_trees(
    game: &mut Game,
    map: &mpq::Archive<'_>,
    notes: &mut Vec<String>,
) -> usize {
    let Ok(bytes) = map.read("war3map.doo") else {
        notes.push("no war3map.doo: workers find no trees".to_string());
        return 0;
    };
    let doodads = match w3map::Doodads::parse(&bytes) {
        Ok(d) => d,
        Err(e) => {
            notes.push(format!("war3map.doo not used: {e}"));
            return 0;
        }
    };
    let mut n = 0usize;
    let trees = &mut game.production_mut().trees;
    for d in &doodads.doodads {
        if d.id[1] == b'T' {
            trees.push(sim::production::Tree { x: d.position[0], y: d.position[1], wood: sim::production::TREE_WOOD });
            n += 1;
        }
    }
    n
}
