//! Unit combat rows from synthetic SLK tables, built through the single unit-record reader
//! (`unittype::load_all` -> `combat::from_units`). Nothing here comes from the game.

use std::collections::BTreeMap;

use gamedata::combat::{self, ArmorType, DamageType, UnitCombatData};
use gamedata::unittype::{load_all, UnitTypeData, UnitTypeTables};
use gamedata::Slk;

/// Builds SYLK text: the first row is the header, `rows` are `(key, cells)` where each cell is the raw `K` token
/// (`"text"` quoted, numbers bare) or `None` for an empty cell.
fn sylk(columns: &[&str], rows: &[(&str, Vec<Option<&str>>)]) -> Slk {
    let mut s = String::from("ID;PWXL;N;E\n");
    let mut put = |x: usize, y: usize, raw: &str| {
        s.push_str(&format!("C;X{x};Y{y};K{raw}\n"));
    };
    put(1, 1, "\"unitID\"");
    for (i, c) in columns.iter().enumerate() {
        put(i + 2, 1, &format!("\"{c}\""));
    }
    for (r, (key, cells)) in rows.iter().enumerate() {
        let y = r + 2;
        put(1, y, &format!("\"{key}\""));
        for (i, cell) in cells.iter().enumerate() {
            if let Some(raw) = cell {
                put(i + 2, y, raw);
            }
        }
    }
    s.push_str("E\n");
    Slk::parse(&s).expect("fixture parses")
}

/// A `UnitUI` table with one row per key (the rows are the unit types; a unit needs this row to be valid).
fn ui_fixture(keys: &[&str]) -> Slk {
    let rows: Vec<(&str, Vec<Option<&str>>)> = keys
        .iter()
        .map(|k| (*k, vec![Some("\"Name\"")]))
        .collect();
    sylk(&["name"], &rows)
}

/// A `UnitData` table with one row per key.
fn data_fixture(keys: &[&str]) -> Slk {
    let rows: Vec<(&str, Vec<Option<&str>>)> = keys
        .iter()
        .map(|k| (*k, vec![Some("0")]))
        .collect();
    sylk(&["canSleep"], &rows)
}

const KEYS: &[&str] = &["hfoo", "hbow", "hbad", "htow", "hnone", "hemp", "hcool"];

/// The unit records of the fixtures, through the single reader.
fn units(balance: Option<&Slk>, weapons: Option<&Slk>) -> BTreeMap<String, UnitTypeData> {
    let ui = ui_fixture(KEYS);
    let data = data_fixture(KEYS);
    load_all(&UnitTypeTables {
        data: Some(&data),
        balance,
        ui: Some(&ui),
        weapons,
    })
}

/// The combat rows built from the fixtures.
fn combat_of(balance: Option<&Slk>, weapons: Option<&Slk>) -> UnitCombatData {
    combat::from_units(&units(balance, weapons))
}

#[test]
fn rawcode_packs_four_characters_big_endian() {
    assert_eq!(combat::rawcode("hfoo"), Some(0x6866_6F6F));
    assert_eq!(combat::rawcode("hfo"), None);
    assert_eq!(combat::rawcode("hfooo"), None);
    assert_eq!(combat::rawcode("éaa"), None);
}

fn weapon_fixture() -> Slk {
    sylk(
        &[
            "dice1", "sides1", "dmgplus1", "cool1", "dmgpt1", "backSw1", "rangeN1", "atkType1",
        ],
        &[
            (
                "hfoo",
                vec![
                    Some("2"),
                    Some("6"),
                    Some("3"),
                    Some("1.35"),
                    Some("0.5"),
                    Some("0.5"),
                    Some("60"),
                    Some("\"Normal\""),
                ],
            ),
            (
                "hbow",
                vec![
                    Some("1"),
                    Some("4"),
                    None,
                    Some("2.0"),
                    None,
                    None,
                    Some("600"),
                    Some("\"pierce\""),
                ],
            ),
            (
                "hbad",
                vec![
                    Some("-1"),
                    Some("4"),
                    Some("1.5"),
                    Some("\"x\""),
                    None,
                    None,
                    None,
                    Some("\"Plasma\""),
                ],
            ),
            (
                "hemp",
                vec![Some("1"), Some("4"), None, None, None, None, None, Some("\"\"")],
            ),
            (
                "hcool",
                vec![
                    Some("1"),
                    Some("4"),
                    None,
                    Some("\"1.5x\""),
                    None,
                    None,
                    None,
                    Some("\"chaos\""),
                ],
            ),
        ],
    )
}

fn balance_fixture() -> Slk {
    sylk(
        &["def", "defType"],
        &[
            ("hfoo", vec![Some("2"), Some("\"medium\"")]),
            ("htow", vec![Some("10"), Some("\"fort\"")]),
            ("hnone", vec![None, Some("\"Divine\"")]),
        ],
    )
}

#[test]
fn weapon_and_defense_rows_are_read_by_column_name() {
    let weapons = weapon_fixture();
    let balance = balance_fixture();
    let data = combat_of(Some(&balance), Some(&weapons));

    let foo = data.get("hfoo").expect("hfoo has rows");
    let w = foo.weapon.expect("hfoo has a weapon");
    assert_eq!((w.dice, w.sides, w.dmg_plus), (2, 6, 3));
    assert_eq!(w.cooldown.to_bits(), 1.35f32.to_bits());
    assert_eq!(w.damage_point.to_bits(), 0.5f32.to_bits());
    assert_eq!(w.backswing.to_bits(), 0.5f32.to_bits());
    assert_eq!(w.range.to_bits(), 60f32.to_bits());
    assert_eq!(w.damage_type, DamageType::Normal);
    let d = foo.defense.expect("hfoo has a defense");
    assert_eq!(d.armor.to_bits(), 2f32.to_bits());
    assert_eq!(d.armor_type, ArmorType::Medium);
}

#[test]
fn a_type_with_only_a_balance_row_has_no_weapon_and_the_names_are_case_insensitive() {
    let weapons = weapon_fixture();
    let balance = balance_fixture();
    let data = combat_of(Some(&balance), Some(&weapons));

    let tower = data.get("htow").expect("htow has rows");
    assert_eq!(tower.weapon, None);
    assert_eq!(tower.defense.map(|d| d.armor_type), Some(ArmorType::Fort));
    let none = data.get("hnone").expect("hnone has rows");
    assert_eq!(none.defense.map(|d| d.armor_type), Some(ArmorType::Divine));
    assert_eq!(none.defense.map(|d| d.armor.to_bits()), Some(0));
}

#[test]
fn a_type_with_only_a_weapon_row_has_no_defense_and_missing_cells_default() {
    let weapons = weapon_fixture();
    let data = combat_of(None, Some(&weapons));

    let bow = data.get("hbow").expect("hbow has rows");
    assert_eq!(bow.defense, None);
    let w = bow.weapon.expect("bow weapon");
    assert_eq!((w.dice, w.sides, w.dmg_plus), (1, 4, 0));
    assert_eq!(
        w.damage_type,
        DamageType::Pierce,
        "case-insensitive attack type"
    );
    assert_eq!(w.backswing.to_bits(), 0f32.to_bits(), "absent real is 0.0");
    assert!(
        data.notes.iter().all(|n| n.starts_with("hbad")),
        "only the malformed row is noted: {:?}",
        data.notes
    );
}

#[test]
fn malformed_cells_take_defaults_and_unknown_names_are_noted() {
    let weapons = weapon_fixture();
    let data = combat_of(None, Some(&weapons));

    let bad = data
        .get("hbad")
        .and_then(|u| u.weapon)
        .expect("hbad weapon");
    assert_eq!(bad.dice, 0, "negative dice become 0");
    assert_eq!(
        bad.damage_type,
        DamageType::Spells,
        "unknown attack type becomes Spells (0), as the binary's reader does"
    );
    assert_eq!(
        bad.cooldown.to_bits(),
        0f32.to_bits(),
        "a real cell without a number is 0.0 (atof)"
    );
    assert_eq!(bad.dmg_plus, 1, "an integer cell reads like strtol: 1.5 -> 1");
    let text = data.notes.join("\n");
    assert!(text.contains("negative dice"), "{text}");
    assert!(text.contains("hbad.atkType1"), "{text}");
    assert!(text.contains("Plasma"), "{text}");
    assert!(!text.contains("cool1"), "no note for a real cell: {text}");
}

#[test]
fn real_cells_follow_the_atof_model() {
    // "1.5x" -> the leading number 1.5 (atof stops at the first invalid character).
    let weapons = weapon_fixture();
    let data = combat_of(None, Some(&weapons));
    let w = data
        .get("hcool")
        .and_then(|u| u.weapon)
        .expect("hcool weapon");
    assert_eq!(w.cooldown.to_bits(), 1.5f32.to_bits());
    assert_eq!(w.damage_type, DamageType::Chaos);
}

#[test]
fn a_missing_file_gives_no_rows_for_it() {
    let data = combat_of(None, None);
    assert!(data.is_empty());
    let balance = balance_fixture();
    let only_defense = combat_of(Some(&balance), None);
    assert_eq!(only_defense.len(), 3);
    assert!(only_defense.iter().all(|(_, u)| u.weapon.is_none()));
}

#[test]
fn a_unit_without_a_ui_row_has_no_combat_row() {
    // The record rule: no UnitUI row, no unit type (the same rule the simulation's records follow).
    let weapons = weapon_fixture();
    let data_table = data_fixture(&["hfoo"]);
    let ui_table = ui_fixture(&["hfoo"]);
    let records = load_all(&UnitTypeTables {
        data: Some(&data_table),
        balance: None,
        ui: Some(&ui_table),
        weapons: Some(&weapons),
    });
    let data = combat::from_units(&records);
    assert_eq!(data.len(), 1);
    assert!(data.get("hbow").is_none());
}

#[test]
fn attack_and_armor_names_follow_the_binary_readers() {
    // Attack type: the reader 0x6F262B00 maps unknown, spells, empty and any other name to 0 (Spells).
    assert_eq!(DamageType::from_name("UNKNOWN"), Some(DamageType::Spells));
    assert_eq!(DamageType::from_name("Chaos"), Some(DamageType::Chaos));
    assert_eq!(
        DamageType::from_name("melee"),
        None,
        "not a name the binary reads"
    );
    // Armor type: the reader 0x6F262C10 maps unknown and other names to 4 (Normal), none to 7.
    assert_eq!(ArmorType::from_name("unknown"), Some(ArmorType::Normal));
    assert_eq!(ArmorType::from_name("None"), Some(ArmorType::Unarmored));
    assert_eq!(ArmorType::from_name("Plasma"), None);
    assert_eq!(ArmorType::Normal.column(), 4);
    assert_eq!(DamageType::Hero.row(), 6);
}

#[test]
fn an_empty_attack_type_cell_is_spells_and_is_not_noted() {
    let weapons = weapon_fixture();
    let data = combat_of(None, Some(&weapons));
    let w = data.get("hemp").and_then(|u| u.weapon).expect("weapon");
    assert_eq!(w.damage_type, DamageType::Spells);
    assert!(
        data.notes.iter().all(|n| !n.contains("hemp")),
        "{:?}",
        data.notes
    );
}

#[test]
fn damage_and_armor_type_tables_follow_the_spec_order() {
    assert_eq!(DamageType::Spells.row(), 0);
    assert_eq!(DamageType::Normal.row(), 1);
    assert_eq!(DamageType::Hero.row(), 6);
    assert_eq!(ArmorType::Small.column(), 0);
    assert_eq!(ArmorType::Unarmored.column(), 7);
}
