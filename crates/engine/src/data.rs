//! Game data for the simulation: the `[Misc]` constants, the unit type records (`gamedata::unittype`), the combat
//! rows built from them, and the damage model that they select (`docs/spec/attack-damage.md`).
//!
//! Sources, all read at boot from the user's own files (nothing is committed):
//! - the game archives, in the order given (`War3.mpq`, then `War3x.mpq`; a later archive overrides an earlier one):
//!   `Units\MiscData.txt`, `Units\MiscGame.txt`, and the four unit tables `Units\UnitUI.slk`, `Units\UnitData.slk`,
//!   `Units\UnitBalance.slk`, `Units\UnitWeapons.slk` (read once, by `gamedata::unittype`);
//! - the map: `war3mapMisc.txt`, the last Misc layer.
//!
//! The damage model is [`sim::damage::SpecDamage`] only when all three hold: the Game.dll tables are given (they give
//! the negative-armor table), some `[Misc]` layer is present, and some unit balance or weapon file is present.
//! Otherwise the game keeps [`sim::combat::PlaceholderDamage`], and the boot report says why.
//!
//! What the simulation's unit registry takes from the records (`sim::natives::game::UnitType`): hit points and mana,
//! move speed, turn speed, prop window, fly height (`moveHeight`), acquire range (after the sight rule), point value,
//! food, gold and lumber cost, and the display name. Not filled yet (no record field or no simulation field):
//! `inventory_size` and `abilities`. Weapon 2 is not in the simulation's attack record (one per type).

use std::collections::BTreeMap;

use gamedata::combat::{self, rawcode, UnitCombatData};
use gamedata::misc::MiscData;
use gamedata::unittype::{self, UnitTypeData, UnitTypeTables};
use gamedata::{Profile, Slk};
use mpq::Archive;
use sim::combat::{AttackType, CombatMisc, Defense};
use sim::damage::{DamageMisc, DamageTypeTable, NegativeArmorTable, SpecDamage};
use sim::economy::{EconomyConstants, UpkeepTable};
use sim::natives::game::UnitType;
use sim::Game;
use softfloat::{conv, Tables, F32};

use crate::Error;

/// The Misc files of the game archives, in layer order (a later file overrides an earlier one).
pub const MISC_FILES: [&str; 2] = ["Units\\MiscData.txt", "Units\\MiscGame.txt"];
/// The map's Misc file: the last layer.
pub const MAP_MISC_FILE: &str = "war3mapMisc.txt";

/// What was read at boot.
#[derive(Debug, Clone)]
pub struct GameData {
    /// The Misc constants (defaults where no layer sets a key).
    pub misc: MiscData,
    /// The unit type records, by unit code (valid units only: a `UnitUI` row and a `UnitData` row).
    pub unit_types: BTreeMap<String, UnitTypeData>,
    /// The combat rows (weapon 1 and defense) built from the records.
    pub units: UnitCombatData,
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
    /// The unit types whose records were installed (with a valid raw code).
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

    let ui = read_slk(archives, unittype::UNIT_UI_FILE, &mut notes)?;
    let data = read_slk(archives, unittype::UNIT_DATA_FILE, &mut notes)?;
    let balance = read_slk(archives, unittype::UNIT_BALANCE_FILE, &mut notes)?;
    let weapons = read_slk(archives, unittype::UNIT_WEAPONS_FILE, &mut notes)?;
    if ui.is_none() || data.is_none() {
        notes.push("UnitUI.slk or UnitData.slk missing: no unit types".to_string());
    }
    let unit_types = unittype::load_all(&UnitTypeTables {
        data: data.as_ref(),
        balance: balance.as_ref(),
        ui: ui.as_ref(),
        weapons: weapons.as_ref(),
    });
    for slk in [balance.as_ref(), weapons.as_ref()].into_iter().flatten() {
        for row in slk.rows() {
            if !unit_types.contains_key(row.key()) {
                notes.push(format!(
                    "unit row {:?} has no valid unit record (UnitUI and UnitData); skipped",
                    row.key()
                ));
            }
        }
    }
    let units = combat::from_units(&unit_types);
    notes.extend(units.notes.iter().cloned());
    for code in unit_types.keys() {
        if rawcode(code).is_none() {
            notes.push(format!(
                "unit row {code:?} is not a four-character code; skipped"
            ));
        }
    }
    Ok(GameData {
        misc,
        unit_types,
        units,
        has_balance: balance.is_some(),
        has_weapons: weapons.is_some(),
        notes,
    })
}

/// Installs the unit types, the combat rows and the Misc values into `game`, and the damage model when its inputs
/// are all there.
pub fn install(game: &mut Game, data: &GameData, tables: Option<&Tables>) -> CombatSetup {
    let mut unit_types = 0;
    for (code, ty) in &data.unit_types {
        let Some(id) = rawcode(code) else {
            continue;
        };
        unit_types += 1;
        game.unit_types_mut().insert(id, sim_unit_type(ty));
        // Hero attributes (STR, AGI and their gains) of a hero type; a hero starts from them (`sim::heroes`).
        if is_hero_type(id) {
            game.set_hero_attributes(id, hero_attributes(ty));
        }
    }
    // The hero experience tables (`docs/spec/heroes.md`). The builder needs the reciprocal table of Game.dll.
    if let Some(t) = tables {
        game.set_hero_xp(sim::heroes::build(&hero_xp_inputs(&data.misc), t));
    }
    for (code, row) in data.units.iter() {
        let Some(id) = rawcode(code) else {
            continue;
        };
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

/// The Misc keys of the economy (see `gamedata::misc`), listed in the boot report when they are absent.
pub const ECONOMY_KEYS: [&str; 7] = [
    "UpkeepUsage",
    "UpkeepGoldTax",
    "UpkeepLumberTax",
    "TrainRefundRate",
    "ResearchRefundRate",
    "ReviveRefundRate",
    "LowGoldAmount",
];

/// What the economy constants were built from (see [`economy_constants`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomySetup {
    /// `true` when some `[Misc]` layer was read; `false` means the built-in defaults of `sim::economy` are in use.
    pub from_misc: bool,
    /// How many layers had a `[Misc]` section.
    pub layers: usize,
    /// The economy keys that were absent (or malformed) and took the reader's default (`0.0`, or 0 for the integers).
    pub absent: Vec<&'static str>,
}

/// Builds the economy constants from the Misc values, or the built-in defaults when no `[Misc]` layer exists at all
/// (spec `economy-3.md` sections 2.2 and 4). With some layer present, a missing key takes the reader's default, which is
/// `0.0` (so a missing `UpkeepUsage` is an all-zero threshold row). UNSURE: the original's behaviour for a missing
/// `UpkeepUsage` key is not traced.
pub fn economy_constants(misc: &MiscData) -> (EconomyConstants, EconomySetup) {
    if misc.layers_with_misc == 0 {
        let setup = EconomySetup {
            from_misc: false,
            layers: 0,
            absent: Vec::new(),
        };
        return (EconomyConstants::default(), setup);
    }
    let constants = EconomyConstants {
        upkeep: UpkeepTable {
            thresholds: misc.upkeep_usage,
            gold_tax: misc.upkeep_gold_tax,
            lumber_tax: misc.upkeep_lumber_tax,
        },
        train_refund_rate: misc.train_refund_rate,
        research_refund_rate: misc.research_refund_rate,
        revive_refund_rate: misc.revive_refund_rate,
        low_gold_amount: conv::f2i(misc.low_gold_amount),
    };
    let absent = misc
        .defaulted
        .iter()
        .copied()
        .filter(|k| ECONOMY_KEYS.contains(k))
        .collect();
    let setup = EconomySetup {
        from_misc: true,
        layers: misc.layers_with_misc,
        absent,
    };
    (constants, setup)
}

/// Installs the economy constants into `game` (see [`economy_constants`]) and returns what they came from.
pub fn install_economy(game: &mut Game, misc: &MiscData) -> EconomySetup {
    let (constants, setup) = economy_constants(misc);
    game.set_economy_constants(constants);
    setup
}

/// The simulation's unit type registry entry of a unit record. Every field is a direct copy of the record's value
/// (no load-time rule beyond the acquire rule already applied by `gamedata::unittype`).
fn sim_unit_type(ty: &UnitTypeData) -> UnitType {
    UnitType {
        name: ty.name.clone(),
        max_life: soft(ty.hp),
        max_mana: soft(ty.mana_max),
        start_mana: soft(ty.mana0),
        // TODO(gamedata): `UnitInventorySize` is not in the record reader yet.
        inventory_size: 0,
        move_speed: soft(ty.speed),
        turn_speed: soft(ty.turn_rate),
        prop_window: soft(ty.prop_win),
        // The record's `moveHeight` is the default fly height (`docs/spec/unit-types.md` section 4.3).
        fly_height: soft(ty.move_height),
        acquire_range: soft(ty.acquire),
        point_value: ty.points,
        food_used: ty.food_used,
        food_made: ty.food_made,
        // TODO(gamedata): the type's ability list is not in the record reader yet.
        abilities: Vec::new(),
        gold_cost: ty.gold_cost,
        wood_cost: ty.lumber_cost,
    }
}

/// The attack record of a weapon row. The placeholder damage (used only without [`SpecDamage`]) is the flat bonus
/// `dmgplus1`. The collision size is not filled: the record's `collision` (`UnitBalance`) and the attack record's
/// collision (`sim::combat`) are not confirmed to be the same value or unit (UNSURE), so it stays 0.
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
        damage_class: sim::damage::ATTACK_DAMAGE_CLASS,
    }
}

/// The `f32` of a record value as the soft float the simulation uses (same bits).
fn soft(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// Whether a raw type id is a hero type: its top byte is an uppercase letter (`IsHeroUnitId`, CONFIRMED).
fn is_hero_type(id: u32) -> bool {
    (id >> 24).wrapping_sub(0x41) < 0x1A
}

/// The hero attributes of a unit record (`STR`, `AGI` and the per-level gains).
fn hero_attributes(ty: &UnitTypeData) -> sim::heroes::HeroAttributes {
    sim::heroes::HeroAttributes {
        str_base: ty.attributes[0],
        str_plus: soft(ty.attribute_plus[0]),
        agi_base: ty.attributes[1],
        agi_plus: soft(ty.attribute_plus[1]),
    }
}

/// The experience-builder inputs from the `[Misc]` hero keys.
fn hero_xp_inputs(misc: &MiscData) -> sim::heroes::XpInputs {
    let h = &misc.hero;
    sim::heroes::XpInputs {
        max_hero_level: h.max_hero_level,
        need: h.need_hero_xp.clone(),
        need_formula: h.need_hero_xp_formula,
        grant: h.grant_hero_xp.clone(),
        grant_formula: h.grant_hero_xp_formula,
        factor: h.hero_factor_xp.clone(),
        max_unit_level: h.max_unit_level,
        normal: h.grant_normal_xp.clone(),
        normal_formula: h.grant_normal_xp_formula,
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
