//! Unit combat rows from synthetic SLK tables. Nothing here comes from the game.

use gamedata::combat::{self, ArmorType, DamageType};
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
                    Some("x"),
                    None,
                    None,
                    None,
                    Some("\"Plasma\""),
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
    let data = combat::load(Some(&balance), Some(&weapons), None);

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
    let data = combat::load(Some(&balance), Some(&weapons), None);

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
    let data = combat::load(None, Some(&weapons), None);

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
fn malformed_cells_take_defaults_and_are_noted() {
    let weapons = weapon_fixture();
    let data = combat::load(None, Some(&weapons), None);

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
        "malformed real becomes 0.0"
    );
    assert_eq!(bad.dmg_plus, 0, "a real in an integer cell is 0");
    let text = data.notes.join("\n");
    assert!(text.contains("negative dice"), "{text}");
    assert!(text.contains("hbad.cool1"), "{text}");
    assert!(text.contains("Plasma"), "{text}");
}

#[test]
fn a_missing_file_gives_no_rows_for_it() {
    let data = combat::load(None, None, None);
    assert!(data.is_empty());
    let balance = balance_fixture();
    let only_defense = combat::load(Some(&balance), None, None);
    assert_eq!(only_defense.len(), 3);
    assert!(only_defense.iter().all(|(_, u)| u.weapon.is_none()));
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
    let weapons = sylk(
        &["dice1", "sides1", "atkType1"],
        &[("hemp", vec![Some("1"), Some("4"), Some("\"\"")])],
    );
    let data = combat::load(None, Some(&weapons), None);
    let w = data.get("hemp").and_then(|u| u.weapon).expect("weapon");
    assert_eq!(w.damage_type, DamageType::Spells);
    assert!(data.notes.is_empty(), "{:?}", data.notes);
}

#[test]
fn damage_and_armor_type_tables_follow_the_spec_order() {
    assert_eq!(DamageType::Spells.row(), 0);
    assert_eq!(DamageType::Normal.row(), 1);
    assert_eq!(DamageType::Hero.row(), 6);
    assert_eq!(ArmorType::Small.column(), 0);
    assert_eq!(ArmorType::Unarmored.column(), 7);
    assert!(DamageType::Pierce.is_physical());
    assert!(!DamageType::Magic.is_physical());
}
