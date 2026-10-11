//! Game data for the simulation: the `[Misc]` constants, the weapon-1 and defense rows of the unit types, and the
//! damage model that they select (`docs/spec/attack-damage.md`).
//!
//! Sources, all read at boot from the user's own files (nothing is committed):
//! - the game archives, in the order given (`War3.mpq`, then `War3x.mpq`; a later archive overrides an earlier one):
//!   `Units\MiscData.txt`, `Units\MiscGame.txt`, `Units\UnitBalance.slk`, `Units\UnitWeapons.slk`;
//! - the map: `war3mapMisc.txt`, the last Misc layer.
//!
//! The damage model is [`sim::damage::SpecDamage`] only when all three hold: the Game.dll tables are given (they give
//! the negative-armor table and the S2R reals), some `[Misc]` layer is present, and some unit balance or weapon file
//! is present. Otherwise the game keeps [`sim::combat::PlaceholderDamage`], and the boot report says why.

use gamedata::combat::{self, rawcode};
use gamedata::misc::MiscData;
use gamedata::{Profile, Slk};
use mpq::Archive;
use sim::combat::{AttackType, CombatMisc, Defense};
use sim::damage::{DamageMisc, DamageTypeTable, NegativeArmorTable, SpecDamage};
use sim::Game;
use softfloat::{Tables, F32};

use crate::Error;

/// The Misc files of the game archives, in layer order (a later file overrides an earlier one).
pub const MISC_FILES: [&str; 2] = ["Units\\MiscData.txt", "Units\\MiscGame.txt"];
/// The map's Misc file: the last layer.
pub const MAP_MISC_FILE: &str = "war3mapMisc.txt";
/// The unit balance table (`def`, `defType`).
pub const UNIT_BALANCE_FILE: &str = "Units\\UnitBalance.slk";
/// The unit weapon table (weapon 1).
pub const UNIT_WEAPONS_FILE: &str = "Units\\UnitWeapons.slk";

/// What was read at boot.
#[derive(Debug, Clone)]
pub struct GameData {
    /// The Misc constants (defaults where no layer sets a key).
    pub misc: MiscData,
    /// The combat rows of the unit types.
    pub units: combat::UnitCombatData,
    /// Whether `UnitBalance.slk` was found.
    pub has_balance: bool,
    /// Whether `UnitWeapons.slk` was found.
    pub has_weapons: bool,
    /// Missing or malformed files, skipped cells and rejected values, one line each.
    pub notes: Vec<String>,
}

/// What [`install`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombatSetup {
    /// The damage model in use: `SpecDamage`, or `PlaceholderDamage` with the reasons it was not installed.
    pub damage_model: String,
    /// The unit types whose rows were installed (with a valid raw code).
    pub unit_types: usize,
}

/// Reads the game data (see the module documentation).
pub fn load(
    archives: &[&[u8]],
    map: &Archive<'_>,
    tables: Option<&Tables>,
) -> Result<GameData, Error> {
    let mut notes = Vec::new();
    if archives.is_empty() {
        notes.push("no game archives: Misc constants and unit rows are defaults".to_string());
    }
    let mut layers = Vec::new();
    for name in MISC_FILES {
        if let Some(bytes) = read_last(archives, name)? {
            match Profile::parse_bytes(&bytes) {
                Ok(profile) => layers.push(profile),
                Err(e) => notes.push(format!("{name}: {e}; skipped")),
            }
        }
    }
    if let Some(bytes) = read_map(map, MAP_MISC_FILE)? {
        match Profile::parse_bytes(&bytes) {
            Ok(profile) => layers.push(profile),
            Err(e) => notes.push(format!("{MAP_MISC_FILE}: {e}; skipped")),
        }
    }
    let refs: Vec<&Profile> = layers.iter().collect();
    let misc = MiscData::from_layers(&refs, tables);
    notes.extend(misc.notes.iter().map(|n| format!("Misc {n}")));

    let balance = read_slk(archives, UNIT_BALANCE_FILE, &mut notes)?;
    let weapons = read_slk(archives, UNIT_WEAPONS_FILE, &mut notes)?;
    let units = combat::load(balance.as_ref(), weapons.as_ref(), tables);
    notes.extend(units.notes.iter().cloned());
    for (code, _) in units.iter() {
        if rawcode(code).is_none() {
            notes.push(format!(
                "unit row {code:?} is not a four-character code; skipped"
            ));
        }
    }
    Ok(GameData {
        misc,
        units,
        has_balance: balance.is_some(),
        has_weapons: weapons.is_some(),
        notes,
    })
}

/// Installs the unit rows and the Misc values into `game`, and the damage model when its inputs are all there.
pub fn install(game: &mut Game, data: &GameData, tables: Option<&Tables>) -> CombatSetup {
    let mut unit_types = 0;
    for (code, row) in data.units.iter() {
        let Some(id) = rawcode(code) else {
            continue;
        };
        unit_types += 1;
        if let Some(w) = row.weapon {
            game.set_attack_type(id, attack_type(&w));
        }
        if let Some(d) = row.defense {
            game.set_unit_defense(
                id,
                Defense {
                    armor: d.armor,
                    armor_type: d.armor_type.column(),
                },
            );
        }
    }
    game.set_combat_misc(CombatMisc {
        attack_half_angle: data.misc.attack_half_angle,
        reaction_delay: data.misc.reaction_delay,
    });

    let mut missing = Vec::new();
    if tables.is_none() {
        missing.push("no Game.dll tables");
    }
    if data.misc.layers_with_misc == 0 {
        missing.push("no [Misc] data");
    }
    if !data.has_balance && !data.has_weapons {
        missing.push("no unit balance or weapon data");
    }
    let damage_model = match tables {
        Some(tables) if missing.is_empty() => {
            let misc = DamageMisc {
                defense_armor: data.misc.defense_armor,
                type_table: DamageTypeTable(data.misc.damage_bonus),
                ethereal_by_attack_type: std::array::from_fn(|row| {
                    data.misc.ethereal_for_attack_type(row)
                }),
            };
            let negative = NegativeArmorTable::computed(tables);
            game.set_damage_model(Box::new(SpecDamage::new(tables.clone(), misc, negative)));
            "SpecDamage".to_string()
        }
        _ => format!("PlaceholderDamage ({})", missing.join(", ")),
    };
    CombatSetup {
        damage_model,
        unit_types,
    }
}

/// The attack record of a weapon row. The placeholder damage (used only without [`SpecDamage`]) is the flat bonus
/// `dmgplus1`; the collision size is not read yet (0, a TODO of the combat spec).
fn attack_type(w: &combat::Weapon) -> AttackType {
    AttackType {
        cooldown: w.cooldown,
        damage_point: w.damage_point,
        backswing: w.backswing,
        range: w.range,
        collision: F32::ZERO,
        damage: F32::from_bits((w.dmg_plus as f32).to_bits()),
        missile_speed: None,
        dice: w.dice,
        sides: w.sides,
        dice_bonus: w.dmg_plus,
        damage_type: w.damage_type.row() as u8,
        physical: w.damage_type.is_physical(),
    }
}

/// Reads the last archive's copy of `name`, if any archive has it.
fn read_last(archives: &[&[u8]], name: &str) -> Result<Option<Vec<u8>>, Error> {
    let mut found = None;
    for (index, bytes) in archives.iter().enumerate() {
        let archive = Archive::open(bytes).map_err(|source| Error::Archive { index, source })?;
        match archive.read(name) {
            Ok(data) => found = Some(data),
            Err(mpq::Error::NotFound(_)) => {}
            Err(source) => return Err(Error::Archive { index, source }),
        }
    }
    Ok(found)
}

/// Reads `name` from the map archive, `None` when the map does not have it.
fn read_map(map: &Archive<'_>, name: &str) -> Result<Option<Vec<u8>>, Error> {
    match map.read(name) {
        Ok(data) => Ok(Some(data)),
        Err(mpq::Error::NotFound(_)) => Ok(None),
        Err(source) => Err(Error::Archive { index: 0, source }),
    }
}

/// Reads and parses a unit table. A malformed table is noted and treated as missing.
fn read_slk(archives: &[&[u8]], name: &str, notes: &mut Vec<String>) -> Result<Option<Slk>, Error> {
    let Some(bytes) = read_last(archives, name)? else {
        return Ok(None);
    };
    match Slk::parse_bytes(&bytes) {
        Ok(slk) => Ok(Some(slk)),
        Err(e) => {
            notes.push(format!("{name}: {e}; treated as missing"));
            Ok(None)
        }
    }
}
