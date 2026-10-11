//! Unit type records (`gamedata::unittype`): cell rules, the field map, the load-time rule and the missing-row
//! defaults. Synthetic SYLK tables only; the integer cell reader was also checked against the original in Unicorn
//! (`tools/emu/unittype_int_emu.py`, 25 cases, 0 mismatches).

use gamedata::slk::Slk;
use gamedata::unittype::{
    cell_int, cell_int_bits, cell_real, load_all, load_unit_type, unquote, UnitTypeData,
    UnitTypeError, UnitTypeTables,
};

/// Builds a SYLK table from a header row and data rows. Numeric-looking cells are written as numbers, the rest as
/// quoted text. Synthetic data only.
fn table(cols: &[&str], rows: &[&[&str]]) -> Slk {
    let mut s = String::from("ID;PWXL;N;E\n");
    for (x, name) in cols.iter().enumerate() {
        s.push_str(&format!("C;X{};Y1;K\"{}\"\n", x + 1, name));
    }
    for (r, row) in rows.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            if cell.is_empty() {
                continue;
            }
            let numeric =
                cell.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'));
            if numeric {
                s.push_str(&format!("C;X{};Y{};K{}\n", x + 1, r + 2, cell));
            } else {
                s.push_str(&format!("C;X{};Y{};K\"{}\"\n", x + 1, r + 2, cell));
            }
        }
    }
    s.push_str("E\n");
    Slk::parse(&s).expect("synthetic table parses")
}

#[test]
fn strtol_matches_the_original_cell_reader() {
    // The same inputs as tools/emu/unittype_int_emu.py, where the original's reader ran in Unicorn.
    let cases: &[(&str, i32)] = &[
        ("0", 0),
        ("7", 7),
        ("-7", -7),
        ("+7", 7),
        ("  12", 12),
        ("12abc", 12),
        ("abc", 0),
        ("", 0),
        ("0x1f", 0x1f),
        ("0X1F", 0x1f),
        ("-0x10", -16),
        ("017", 15),
        ("08", 0),
        ("2147483647", i32::MAX),
        ("2147483648", i32::MAX),
        ("-2147483648", i32::MIN),
        ("-2147483649", i32::MIN),
        ("4294967296", i32::MAX),
        ("0x7fffffffff", i32::MAX),
        ("0x", 0),
        ("99999999999999999999999999999999", i32::MAX),
    ];
    for (text, want) in cases {
        assert_eq!(cell_int(text), *want, "strtol({text:?})");
    }
}

#[test]
fn raw_code_cells_pack_big_endian() {
    // Emulator-checked: 'hfoo' -> 0x68666F6F, 'ab' -> 0x6162, 'abcdef' -> its first four characters.
    assert_eq!(cell_int_bits("'hfoo'"), 0x6866_6F6F);
    assert_eq!(cell_int_bits("'ab'"), 0x0000_6162);
    assert_eq!(cell_int_bits("'abcdef'"), 0x6162_6364);
    assert_eq!(cell_int_bits("'a"), 0x61);
    assert_eq!(cell_int_bits("'"), 0);
}

#[test]
fn real_cells_take_the_leading_number_and_round_to_single() {
    assert_eq!(cell_real("1.5"), 1.5);
    assert_eq!(cell_real("  -2.25e1xyz"), -22.5);
    assert_eq!(cell_real("1e"), 1.0);
    assert_eq!(cell_real("2."), 2.0);
    assert_eq!(cell_real(".5"), 0.5);
    assert_eq!(cell_real("abc"), 0.0);
    assert_eq!(cell_real(""), 0.0);
    // single rounding of the decimal value: the nearest f32 to 0.1
    assert_eq!(cell_real("0.1").to_bits(), 0x3DCC_CCCD);
}

#[test]
fn quotes_are_removed_from_text_cells() {
    assert_eq!(unquote("\"medium\""), "medium");
    assert_eq!(unquote("  \" a b \" "), "a b");
    assert_eq!(unquote("3"), "3");
}

fn fixture() -> (Slk, Slk, Slk, Slk) {
    let ui = table(
        &["unitUIID", "fogRad", "scaleBull"],
        &[&["hfoo", "1100", "1.0"], &["hmil", "900", "1.0"]],
    );
    let data = table(
        &["unitID", "turnRate", "canSleep", "movetp"],
        &[&["hfoo", "0.6", "0", "foot"], &["hmil", "0.5", "1", "foot"]],
    );
    let balance = table(
        &[
            "unitBalanceID",
            "goldcost",
            "lumbercost",
            "fmade",
            "fused",
            "spd",
            "minSpd",
            "maxSpd",
            "HP",
            "def",
            "defType",
            "sight",
            "nsight",
            "bldtm",
            "STR",
        ],
        &[&[
            "hfoo", "275", "0", "1", "0", "270", "10", "320", "420", "2", "medium", "1400", "800",
            "35", "3",
        ]],
    );
    let weapons = table(
        &[
            "unitWeapID",
            "weapsOn",
            "acquire",
            "impactZ",
            "launchX",
            "launchY",
            "launchZ",
            "castpt",
            "dice1",
            "sides1",
            "dmgplus1",
            "cool1",
            "dmgpt1",
            "backSw1",
            "rangeN1",
            "atkType1",
            "weapType1",
            "targs1",
            "showUI1",
        ],
        &[&[
            "hfoo",
            "1",
            "0.5",
            "42.5",
            "10",
            "-4",
            "0.25",
            "0.2",
            "2",
            "3",
            "1",
            "1.25",
            "0.4",
            "0.5",
            "1",
            "normal",
            "metal",
            "ground,air",
            "1",
        ]],
    );
    (ui, data, balance, weapons)
}

#[test]
fn full_record_is_read_from_the_four_tables() {
    let (ui, data, balance, weapons) = fixture();
    let tables = UnitTypeTables {
        data: Some(&data),
        balance: Some(&balance),
        ui: Some(&ui),
        weapons: Some(&weapons),
    };
    let u: UnitTypeData = load_unit_type(&tables, "hfoo").expect("valid unit");
    assert_eq!(u.rawcode, Some(0x6866_6F6F));
    assert_eq!(u.speed, 270.0);
    assert_eq!(u.min_speed, 10.0);
    assert_eq!(u.max_speed, 320.0);
    assert_eq!(u.turn_rate, 0.6_f64 as f32);
    assert_eq!(u.hp, 420.0);
    assert_eq!(u.armor, 2.0);
    assert_eq!(u.armor_type, "medium");
    assert_eq!(u.gold_cost, 275);
    assert_eq!(u.food_made, 1);
    assert_eq!(u.build_time, 35);
    assert_eq!(u.sight, 1400.0);
    assert_eq!(u.night_sight, 800.0);
    assert_eq!(u.attributes[0], 3);
    assert_eq!(u.fog_radius, 1100.0);
    assert_eq!(u.move_type, "foot");
    assert_eq!(u.can_sleep, 0);
    assert_eq!(u.impact_z, 42.5);
    assert_eq!(u.launch, [10.0, -4.0, 0.25]);
    assert_eq!(u.cast_point, 0.2_f64 as f32);
    // acquire 0.5 is below 1.0, so it becomes sight
    assert_eq!(u.acquire, 1400.0);
    let w = &u.weapons[0];
    assert_eq!((w.dice, w.sides, w.dmg_plus), (2, 3, 1));
    assert_eq!(w.cool, 1.25);
    assert_eq!(w.dmg_pt, 0.4_f64 as f32);
    assert_eq!(w.back_swing, 0.5);
    assert_eq!(w.range, 1.0);
    assert_eq!(w.atk_type, "normal");
    assert_eq!(w.weap_type, "metal");
    assert_eq!(w.targs, "ground,air");
    assert_eq!(w.show_ui, 1);
    // the second slot has no cells: zeros
    assert_eq!(u.weapons[1].dice, 0);
    assert_eq!(u.weapons[1].dmg_pt, 0.0);
}

#[test]
fn acquire_at_least_one_is_kept() {
    let (ui, data, balance, _) = fixture();
    let weapons = table(&["unitWeapID", "acquire"], &[&["hfoo", "600"]]);
    let tables = UnitTypeTables {
        data: Some(&data),
        balance: Some(&balance),
        ui: Some(&ui),
        weapons: Some(&weapons),
    };
    let u = load_unit_type(&tables, "hfoo").unwrap();
    assert_eq!(u.acquire, 600.0);
}

#[test]
fn missing_ui_or_data_row_is_an_error() {
    let (ui, data, balance, weapons) = fixture();
    let tables = UnitTypeTables {
        data: Some(&data),
        balance: Some(&balance),
        ui: Some(&ui),
        weapons: Some(&weapons),
    };
    assert_eq!(
        load_unit_type(&tables, "hxxx"),
        Err(UnitTypeError::MissingUiRow("hxxx".to_owned()))
    );
    let no_data = UnitTypeTables {
        data: None,
        ..tables
    };
    assert_eq!(
        load_unit_type(&no_data, "hfoo"),
        Err(UnitTypeError::MissingDataRow("hfoo".to_owned()))
    );
}

#[test]
fn missing_balance_row_gives_acquire_one_and_zero_balance() {
    let (ui, data, _balance, weapons) = fixture();
    let tables = UnitTypeTables {
        data: Some(&data),
        balance: None,
        ui: Some(&ui),
        weapons: Some(&weapons),
    };
    let u = load_unit_type(&tables, "hfoo").unwrap();
    assert_eq!(u.speed, 0.0);
    assert_eq!(u.sight, 0.0);
    assert_eq!(u.acquire, 1.0);
    // the weapon row is still read
    assert_eq!(u.impact_z, 42.5);
}

#[test]
fn missing_weapon_row_gives_sight_and_default_slots() {
    let (ui, data, balance, _) = fixture();
    let tables = UnitTypeTables {
        data: Some(&data),
        balance: Some(&balance),
        ui: Some(&ui),
        weapons: None,
    };
    let u = load_unit_type(&tables, "hfoo").unwrap();
    assert_eq!(u.acquire, 1400.0);
    assert_eq!(u.impact_z, 0.0);
    assert_eq!(u.weapons[0].dmg_pt, 1.0);
    assert_eq!(u.weapons[1].dmg_pt, 1.0);
    assert_eq!(u.weapons[0].show_ui, 1);
    assert_eq!(u.weapons[0].dice, 0);
    // the balance row is still read
    assert_eq!(u.speed, 270.0);
}

#[test]
fn load_all_skips_invalid_units_and_is_ordered() {
    let (_, data, balance, weapons) = fixture();
    let ui = table(
        &["unitUIID", "fogRad", "scaleBull"],
        &[
            &["hmil", "900", "1.0"],
            &["hfoo", "1100", "1.0"],
            &["hbad", "1", "1"],
        ],
    );
    let tables = UnitTypeTables {
        data: Some(&data),
        balance: Some(&balance),
        ui: Some(&ui),
        weapons: Some(&weapons),
    };
    let all = load_all(&tables);
    let keys: Vec<&str> = all.keys().map(String::as_str).collect();
    // "hbad" has a UI row but no UnitData row, so it is skipped
    assert_eq!(keys, vec!["hfoo", "hmil"]);
}
