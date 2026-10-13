//! Weapon 2 and the target and splash columns, from synthetic SLK tables through the single unit-record reader.

use gamedata::combat;
use gamedata::unittype::{load_all, UnitTypeTables};
use gamedata::Slk;

/// SYLK text with one header row and one data row per `(key, cells)`; cells are raw `K` tokens.
fn sylk(columns: &[&str], rows: &[(&str, Vec<&str>)]) -> Slk {
    let mut s = String::from("ID;PWXL;N;E\n");
    let mut put = |x: usize, y: usize, raw: &str| s.push_str(&format!("C;X{x};Y{y};K{raw}\n"));
    put(1, 1, "\"unitID\"");
    for (i, c) in columns.iter().enumerate() {
        put(i + 2, 1, &format!("\"{c}\""));
    }
    for (r, (key, cells)) in rows.iter().enumerate() {
        put(1, r + 2, &format!("\"{key}\""));
        for (i, raw) in cells.iter().enumerate() {
            put(i + 2, r + 2, raw);
        }
    }
    s.push_str("E\n");
    Slk::parse(&s).expect("fixture parses")
}

#[test]
fn both_weapons_target_type_and_splash_columns_are_read() {
    let ui = sylk(&["name"], &[("hgry", vec!["\"Gryphon\""]), ("hmor", vec!["\"Mortar\""])]);
    let data = sylk(
        &["canSleep", "targType"],
        &[("hgry", vec!["0", "\"air,organic\""]), ("hmor", vec!["0", "\"ground,organic\""])],
    );
    let weapons = sylk(
        &[
            "weapsOn", "dice1", "sides1", "dmgplus1", "cool1", "dmgpt1", "backSw1", "rangeN1", "atkType1", "weapTp1",
            "targs1", "splashTargs1", "Farea1", "Harea1", "Qarea1", "Hfact1", "Qfact1", "dice2", "sides2", "dmgplus2",
            "cool2", "dmgpt2", "backSw2", "rangeN2", "atkType2", "weapTp2", "targs2", "targCount2", "damageLoss2",
        ],
        &[(
            "hgry",
            vec![
                "3", "1", "5", "2", "1.5", "0.3", "0.3", "128", "\"normal\"", "\"normal\"", "\"ground,structure\"",
                "\"_\"", "0", "0", "0", "0", "0", "2", "6", "3", "1.8", "0.4", "0.4", "600", "\"pierce\"",
                "\"mbounce\"", "\"air\"", "3", "0.4",
            ],
        )],
    );
    let units = load_all(&UnitTypeTables {
        abilities: None,
        data: Some(&data),
        balance: None,
        ui: Some(&ui),
        weapons: Some(&weapons),
    });
    let g = &units["hgry"];
    assert_eq!(g.targ_type, 0x4, "air only, organic is masked away");
    assert_eq!(units["hmor"].targ_type, 0x2);
    let rows = combat::from_units(&units);
    let row = rows.get("hgry").expect("row");
    let w1 = row.weapon.expect("weapon 1");
    let w2 = row.weapon2.expect("weapon 2");
    assert_eq!(w1.targs, 2 | 8);
    assert_eq!(w2.targs, 4);
    assert_eq!(w2.kind, 6);
    assert_eq!(w2.targ_count, 3);
    assert_eq!(w2.dice, 2);
    assert_eq!(w2.sides, 6);
    assert_eq!(w2.damage_type, combat::DamageType::Pierce);
    // a unit whose weapsOn has no bit 1 has no weapon 2
    assert!(combat::from_units(&units).get("hmor").map_or(true, |r| r.weapon2.is_none()));
}
