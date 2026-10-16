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
    /// The first level record of the harvest, mine and entangle abilities (`Ahar`, `Ahrl`, `Aaha`, `Awha`, `Abgm`,
    /// `Aegm`, `Aenc`, `Aent`), by raw code (`docs/spec/economy-5.md`).
    pub harvest_levels: BTreeMap<u32, gamedata::abilitytype::AbilityLevel>,
    /// The level records of the spells the cast core knows (`sim::cast`), by raw code.
    pub spell_levels: BTreeMap<u32, Vec<sim::cast::SpellLevel>>,
    /// The `UnitID` of each level of the spell abilities (0 when empty).
    pub summon_units: BTreeMap<u32, Vec<u32>>,
    /// The parsed `targs` word of each level of the orc spells that filter an area (`sim::orc::Game::set_orc_targs`).
    pub orc_targs: BTreeMap<u32, Vec<u32>>,
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
    /// The item abilities: item specs, the base code of each ability alias an item names and their level records
    /// (`sim::item_abilities`, `docs/spec/abilities-items.md`).
    pub item_abilities: sim::item_abilities::ItemAbilityData,
    /// Creep / neutral alias rows of `AbilityData.slk`: `alias row -> modelled code` (`sim::neutral_spells`).
    pub spell_aliases: BTreeMap<u32, u32>,
    /// Registered ability codes with no `AbilityData.slk` row (no level data possible).
    pub missing_ability_rows: Vec<u32>,
    /// The unit and ability code lists of the level data of the S50 neutral spells (`sim::neutral_forms`): `(row, level
    /// index, data index) -> codes`.
    pub neutral_lists: BTreeMap<(u32, usize, usize), Vec<u32>>,
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
    load_item_abilities(archives, &mut out)?;
    load_neutral_aliases(archives, &mut out)?;
    load_registered(archives, &mut out)?;
    load_neutral_lists(archives, &mut out)?;
    Ok(out)
}

/// The rows whose data cells name units or abilities (their numeric reading is zero): Storm, Earth and Fire `DataA` (the
/// three elementals), Pocket Factory `DataB` (the goblin) and Engineering Upgrade `DataC..F` (`from,to` ability pairs).
/// Every row of those groups is read (the upgrade rows `ANs1` .. carry their own goblin).
fn load_neutral_lists(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let Some(bytes) = read_last(archives, ABILITY_DATA_FILE)? else {
        return Ok(());
    };
    let Ok(slk) = Slk::parse_bytes(&bytes) else {
        return Ok(());
    };
    const FIELDS: [(&str, &[(&str, usize)]); 4] = [
        ("ANef", &[("DataA", 0)]),
        ("ANdp", &[("DataA", 0)]),
        ("ANsy", &[("DataB", 1)]),
        ("ANeg", &[("DataC", 2), ("DataD", 3), ("DataE", 4), ("DataF", 5)]),
    ];
    for row in slk.rows() {
        let Some(id) = raw_code(row.key()) else { continue };
        let code_col = slk.raw(row.key(), "code").map(|t| t.trim().trim_matches('"')).and_then(raw_code).unwrap_or(id);
        for (group, fields) in FIELDS {
            if raw_code(group) != Some(code_col) {
                continue;
            }
            for level in 1..=4usize {
                for &(name, index) in fields {
                    let col = format!("{name}{level}");
                    let Some(text) = slk.raw(row.key(), &col) else { continue };
                    let text = gamedata::unittype::unquote(text);
                    if text.is_empty() || text.as_bytes()[0].is_ascii_digit() || text.starts_with('-') {
                        continue;
                    }
                    let codes = gamedata::abilitytype::list_slots(Some(text));
                    out.neutral_lists.insert((id, level - 1, index), codes.into_iter().filter(|&c| c != 0).collect());
                }
            }
        }
    }
    Ok(())
}

/// The level records of one `AbilityData.slk` row (`None` when the row is missing or has no level).
fn spell_levels_of_row(slk: &Slk, id: u32) -> Option<Vec<sim::cast::SpellLevel>> {
    let name = String::from_utf8_lossy(&id.to_be_bytes()).into_owned();
    let def = gamedata::abilitytype::read_ability(slk, &name)?;
    let f = |v: f32| F32::from_bits(v.to_bits());
    Some(
        def.levels
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
                    unit_id: l.unit_id,
                }
            })
            .collect(),
    )
}

/// Loads the level records of every ability code the simulation registers (`sim::ability_registry`, derived from the
/// module registries, no hand list) that no earlier pass loaded, and the morph forms (`set_morph_alt` inputs, filled
/// at install). A code without an `AbilityData.slk` row is noted.
fn load_registered(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let Some(bytes) = read_last(archives, ABILITY_DATA_FILE)? else {
        return Ok(());
    };
    let Ok(slk) = Slk::parse_bytes(&bytes) else {
        return Ok(());
    };
    // A class code that has no row of its own in 1.26a (`AOac`, `Aarm`, `Acar`, ...) is the `code` column of other rows
    // (`ACac` -> `AOac`): the first such row (key order) stands in for its level data (LIKELY: same data layout).
    let mut by_class: BTreeMap<u32, u32> = BTreeMap::new();
    for row in slk.rows() {
        let Some(id) = raw_code(row.key()) else { continue };
        let class = slk
            .raw(row.key(), "code")
            .map(|t| t.trim().trim_matches('"'))
            .and_then(raw_code)
            .unwrap_or(id);
        if class != id {
            by_class.entry(class).or_insert(id);
        }
    }
    for code in sim::ability_registry::registered_codes() {
        if out.spell_levels.contains_key(&code) {
            continue;
        }
        let own = spell_levels_of_row(&slk, code).filter(|l| !l.is_empty());
        let levels = own.or_else(|| by_class.get(&code).and_then(|&row| spell_levels_of_row(&slk, row)).filter(|l| !l.is_empty()));
        match levels {
            Some(levels) => {
                out.spell_levels.insert(code, levels);
            }
            None => out.missing_ability_rows.push(code),
        }
    }
    Ok(())
}

/// The morph alternate forms: for every unit type that lists a morph ability, `form -> original` where the form is the
/// ability's level-1 `UnitID` (`sim::Game::set_morph_alt`: the unit placed already in the other form takes the alternate
/// order back to its original type). `unit_abilities` is `(unit code, abilList codes)`.
pub fn morph_alternates(text: &ObjectText, unit_abilities: &[(u32, Vec<u32>)]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for &(ability, _, _) in sim::ability_registry::morph_abilities() {
        let Some(form) = text.spell_levels.get(&ability).and_then(|l| l.first()).map(|l| l.unit_id).filter(|&u| u != 0) else {
            continue;
        };
        for (unit, list) in unit_abilities {
            if *unit != form && list.contains(&ability) {
                out.push((form, *unit));
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Reads the creep / neutral alias rows: every `AbilityData.slk` row whose `code` column names a modelled ability (or a
/// class derived from one, `sim::neutral_spells::STATIC_PARENTS`) while the row itself is another id. The alias gets its
/// own level records; the modelled row's records are read too when no earlier pass loaded them.
fn load_neutral_aliases(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let Some(bytes) = read_last(archives, ABILITY_DATA_FILE)? else {
        return Ok(());
    };
    let Ok(slk) = Slk::parse_bytes(&bytes) else {
        return Ok(());
    };
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for row in slk.rows() {
        let Some(id) = raw_code(row.key()) else { continue };
        let code_col = slk
            .raw(row.key(), "code")
            .map(|t| t.trim().trim_matches('"'))
            .and_then(raw_code)
            .unwrap_or(id);
        groups.entry(code_col).or_default().push(id);
    }
    let level_of = |id: u32| -> Option<Vec<sim::cast::SpellLevel>> {
        let name = String::from_utf8_lossy(&id.to_be_bytes()).into_owned();
        let def = gamedata::abilitytype::read_ability(&slk, &name)?;
        let f = |v: f32| F32::from_bits(v.to_bits());
        Some(
            def.levels
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
                        unit_id: l.unit_id,
                    }
                })
                .collect(),
        )
    };
    for (code_col, ids) in groups {
        let Some(std) = sim::neutral_spells::standard_for_group(code_col, &ids) else {
            continue;
        };
        if !out.spell_levels.contains_key(&std) {
            if let Some(levels) = level_of(std) {
                out.spell_levels.insert(std, levels);
            }
        }
        for id in ids {
            if id == std || sim::neutral_spells::is_item_row(id) {
                continue;
            }
            if let Some(levels) = level_of(id) {
                out.spell_levels.insert(id, levels);
                out.spell_aliases.insert(id, std);
            }
        }
    }
    Ok(())
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

/// Reads the item abilities: for every item row its `abilList` (up to four ability ids), `uses`, `perishable`,
/// `usable`, `cooldownID` and the type flags, and for every ability id named there the base `code` column and the
/// level records of the `AbilityData.slk` row (ids of classes the model does not know are kept too and ignored).
fn load_item_abilities(archives: &[&[u8]], out: &mut ObjectText) -> Result<(), Error> {
    let (Some(items), Some(abilities)) = (read_last(archives, ITEM_DATA_FILE)?, read_last(archives, ABILITY_DATA_FILE)?) else {
        return Ok(());
    };
    let (Ok(items), Ok(abilities)) = (Slk::parse_bytes(&items), Slk::parse_bytes(&abilities)) else {
        out.notes.push("item abilities: ItemData or AbilityData unreadable".to_string());
        return Ok(());
    };
    let int = |slk: &Slk, key: &str, col: &str| -> i32 {
        slk.raw(key, col)
            .and_then(|t| t.trim().trim_matches('"').parse::<i32>().ok())
            .unwrap_or(0)
    };
    let flag = |slk: &Slk, key: &str, col: &str| int(slk, key, col) != 0;
    for row in items.rows() {
        let key = row.key();
        let Some(code) = raw_code(key) else { continue };
        let list = row.raw("abilList").map(|t| t.trim().trim_matches('"')).unwrap_or("");
        let aliases: Vec<u32> = gamedata::abilitytype::list_slots(Some(list))
            .into_iter()
            .filter(|&a| a != 0)
            .take(4)
            .collect();
        let spec = sim::item_abilities::ItemSpec {
            abilities: aliases.clone(),
            perishable: flag(&items, key, "perishable"),
            usable: flag(&items, key, "usable"),
            cooldown_id: row
                .raw("cooldownID")
                .map(|t| gamedata::abilitytype::raw_code(t.trim().trim_matches('"')))
                .unwrap_or(0),
            uses: int(&items, key, "uses"),
            powerup: flag(&items, key, "powerup"),
            sellable: flag(&items, key, "sellable"),
            pawnable: flag(&items, key, "pawnable"),
            level: int(&items, key, "Level"),
            ignore_cooldown: flag(&items, key, "ignoreCD"),
            registers_type: true,
        };
        for alias in aliases {
            let name = String::from_utf8_lossy(&alias.to_be_bytes()).into_owned();
            if out.item_abilities.levels.contains_key(&alias) {
                continue;
            }
            let Some(def) = gamedata::abilitytype::read_ability(&abilities, &name) else {
                continue;
            };
            if let Some(base) = abilities.raw(&name, "code").map(|t| t.trim().trim_matches('"')).filter(|t| !t.is_empty()) {
                out.item_abilities.bases.insert(alias, gamedata::abilitytype::raw_code(base));
            }
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
                        unit_id: l.unit_id,
                    }
                })
                .collect();
            out.item_abilities.levels.insert(alias, levels);
            // The figurines name their units in `DataC` / `DataD` (raw codes, not numbers).
            let unit_code = |col: &str| -> u32 {
                abilities
                    .raw(&name, col)
                    .map(|t| t.trim().trim_matches('"'))
                    .filter(|t| t.len() == 4 && !t.bytes().all(|b| b.is_ascii_digit() || b == b'.' || b == b'-'))
                    .map_or(0, gamedata::abilitytype::raw_code)
            };
            // The build scrolls (`AIbl`) list one unit per race in `UnitID`.
            if let Some(list) = abilities.raw(&name, "UnitID1").map(|t| t.trim().trim_matches('"')) {
                let parts: Vec<u32> = list
                    .split(',')
                    .map(|c| c.trim())
                    .filter(|c| c.len() == 4)
                    .map(gamedata::abilitytype::raw_code)
                    .collect();
                if parts.len() > 1 {
                    out.item_abilities.unit_lists.insert(alias, parts);
                }
            }
            let codes = [unit_code("DataC1"), unit_code("DataD1")];
            if codes != [0, 0] {
                out.item_abilities.unit_codes.insert(alias, codes);
            }
            // The aura classes read `Area`, `DataA..D`, `BuffID` and `targs` of the item row.
            if out
                .item_abilities
                .bases
                .get(&alias)
                .is_some_and(|&b| sim::auras::AuraKind::of(b).is_some())
            {
                let f = |x: f32| F32::from_bits(x.to_bits());
                let levels = def
                    .levels
                    .iter()
                    .map(|l| sim::auras::AuraLevel {
                        area: f(l.area),
                        data: [f(l.data[0]), f(l.data[1]), f(l.data[2]), f(l.data[3])],
                        buff: l.buff_ids.first().copied().unwrap_or(0),
                        targs: l.targs.clone(),
                    })
                    .collect();
                out.item_abilities.auras.insert(alias, levels);
            }
        }
        if !spec.abilities.is_empty() || spec.uses != 0 || spec.powerup {
            out.item_abilities.specs.insert(code, spec);
        }
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
const AURA_KINDS: [sim::auras::AuraKind; 7] = [
    sim::auras::AuraKind::Devotion,
    sim::auras::AuraKind::Endurance,
    sim::auras::AuraKind::Unholy,
    sim::auras::AuraKind::Brilliance,
    sim::auras::AuraKind::Command,
    sim::auras::AuraKind::Trueshot,
    sim::auras::AuraKind::Vampiric,
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
        ("AOwk", sim::orc::WIND_WALK),
        ("AOmi", sim::orc::MIRROR_IMAGE),
        ("AOsh", sim::orc::SHOCKWAVE),
        ("AOws", sim::orc::WAR_STOMP),
        ("AOcl", sim::orc::CHAIN_LIGHTNING),
        ("AOfs", sim::orc::FAR_SIGHT),
        ("AOsf", sim::orc::FERAL_SPIRIT),
        ("AOsw", sim::orc::SERPENT_WARD),
        ("AOhx", sim::orc::HEX),
        ("AOvd", sim::orc::VOODOO),
        ("AOeq", sim::orc::EARTHQUAKE),
        ("AOre", sim::orc::REINCARNATION),
        ("AOww", sim::orc::BLADESTORM),
        ("AOhw", sim::orc::HEALING_WAVE),
        ("AHtc", sim::human_heroes::THUNDER_CLAP),
        ("AHbh", sim::human_heroes::BASH),
        ("AHav", sim::human_heroes::AVATAR),
        ("AHds", sim::human_heroes::DIVINE_SHIELD),
        ("AHre", sim::human_heroes::RESURRECTION),
        ("AHwe", sim::human_heroes::WATER_ELEMENTAL),
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
                        unit_id: l.unit_id,
                    }
                })
                .collect();
            out.spell_levels.insert(code, levels);
            out.summon_units.insert(code, def.levels.iter().map(|l| l.unit_id).collect());
            out.orc_targs.insert(code, def.levels.iter().map(|l| gamedata::unitclass::parse_target_flags(&l.targs)).collect());
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
    for name in HARVEST_ABILITIES {
        if slk.row(name).is_some() {
            if let Some(code) = raw_code(name) {
                out.harvest_levels.insert(code, gamedata::abilitytype::read_level(&slk, name, 1));
            }
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

/// The ability rows the worker, mine and entangle model reads.
const HARVEST_ABILITIES: [&str; 8] = ["Ahar", "Ahrl", "Aaha", "Awha", "Abgm", "Aegm", "Aenc", "Aent"];

/// Fills the harvest numbers of the worker types (`data::install` set the modes and the ability codes), the mine specs
/// and the entangle ability from the ability tables.
fn install_harvest(game: &mut Game, text: &ObjectText) {
    let level = |name: &str| raw_code(name).and_then(|c| text.harvest_levels.get(&c));
    let codes: Vec<u32> = game.production().tech.keys().copied().collect();
    for code in codes {
        let t = game.production_mut().tech.get_mut(&code).expect("key");
        let h = &mut t.harvest;
        let abil_name = |c: u32| String::from_utf8_lossy(&c.to_be_bytes()).into_owned();
        if h.gold_abil == raw_code("Ahar").unwrap_or(0) {
            if let Some(l) = level("Ahar") {
                h.gold_cap = l.data[2] as i32;
            }
        }
        for c in [h.wood_abil, h.gold_abil] {
            if c == 0 {
                continue;
            }
            let name = abil_name(c);
            let Some(l) = level(&name) else { continue };
            match name.as_str() {
                "Ahar" | "Ahrl" => {
                    h.per_hit = (l.data[0] as i32).max(1);
                    h.wood_cap = l.data[1] as i32;
                    if name == "Ahar" {
                        h.gold_cap = l.data[2] as i32;
                    }
                }
                "Awha" => {
                    h.renew_amount = l.data[0] as i32;
                    h.renew_seconds = l.dur;
                }
                _ => {}
            }
        }
        if let Some(e) = t.entangle.as_mut() {
            if let Some(l) = level("Aent") {
                e.rng = l.rng;
                e.cast = l.cast;
                if l.unit_id != 0 {
                    e.unit = l.unit_id;
                }
            }
        }
    }
    // The haunted and entangled mine: gold per payment `DataA`, countdown `DataB`, slots `DataC` (at most 8) of `Abgm`;
    // the entangled mine's slots are the cargo of `Aenc` (`DataA`, UNSURE which ability gives them).
    let mut specs = Vec::new();
    if let Some(l) = level("Abgm") {
        specs.push((
            sim::ai_towns::HAUNTED_MINE,
            sim::production::MineSpec { amount: l.data[0] as i32, interval: l.data[1], slots: (l.data[2] as i32).clamp(1, 8) },
        ));
    }
    if let Some(l) = level("Aegm") {
        let slots = level("Aenc").map_or(5, |c| c.data[0] as i32).clamp(1, 8);
        specs.push((
            sim::ai_towns::ENTANGLED_MINE,
            sim::production::MineSpec { amount: l.data[0] as i32, interval: l.data[1], slots },
        ));
    }
    for (code, spec) in specs {
        game.production_mut().mine_specs.insert(code, spec);
    }
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
    game.install_item_abilities(&text.item_abilities);
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
    // The weapons' projectiles. The flying kinds (`weapTp` 2 missile, 3 artillery, 5 msplash, 6 mbounce) get a timed
    // flight over the distance (`docs/spec/missiles.md`; the arrival of 3, 5 and 6 applies the splash or bounce of
    // `docs/spec/weapon-splash.md`); the line kinds keep the instant hit (their launch classes are not decoded).
    for (&code, spec) in &text.missiles {
        let kinds = game.unit_types().get(code).map_or([0, 0], |t| t.weapon_kind);
        let mut md = game.missile_data(code);
        md.arc = spec.arc.first().map_or(F32::ZERO, |&a| F32::from_bits(a.to_bits()));
        md.homing = spec.homing.first().copied().unwrap_or(false);
        game.set_missile_data(code, md);
        for (w, &kind) in kinds.iter().enumerate() {
            if !matches!(kind, 2 | 3 | 5 | 6) {
                continue;
            }
            let Some(&speed) = spec.speed.get(w) else {
                continue;
            };
            let speed = F32::from_bits(speed.to_bits());
            if w == 0 {
                let mut at = game.attack_type(code);
                at.missile_speed = Some(speed);
                game.set_attack_type(code, at);
            } else if let Some(mut at) = game.attack_type2(code) {
                at.missile_speed = Some(speed);
                game.set_attack_type2(code, at);
            }
        }
    }
    install_harvest(game, text);
    for (&code, levels) in &text.spell_levels {
        game.set_spell_levels(code, levels.clone());
    }
    for (&alias, &base) in &text.spell_aliases {
        game.set_spell_alias(alias, base);
    }
    for (&(row, level, data), codes) in &text.neutral_lists {
        game.set_neutral_unit_list(row, level, data, codes.clone());
    }
    for (&base, words) in &text.orc_targs {
        if matches!(base, sim::orc::WAR_STOMP | sim::orc::VOODOO | sim::orc::EARTHQUAKE | sim::orc::BLADESTORM) {
            game.set_orc_targs(base, words.clone());
        }
    }
    for (&code, units) in &text.summon_units {
        if code == sim::orc::FERAL_SPIRIT || code == sim::orc::SERPENT_WARD {
            game.set_summon_units(code, units.clone());
        }
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
