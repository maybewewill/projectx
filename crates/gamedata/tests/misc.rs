//! `MiscData` on synthetic `[Misc]` layers. Nothing here comes from the game. The S2R check runs only with
//! `GAME_DLL` set to a 1.26a `Game.dll`; otherwise it is skipped.

use gamedata::misc::{ARMOR_COLUMNS, ATTACK_ROWS, ETHEREAL_ELEMENT_BY_ROW, ETHEREAL_ENTRIES};
use gamedata::real::{is_real_syntax, parse_real};
use gamedata::{MiscData, Profile};
use softfloat::{Tables, F32};

fn layer(text: &str) -> Profile {
    Profile::parse(text).expect("fixture parses")
}

fn f(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

#[test]
fn no_layers_gives_the_reader_defaults_of_zero() {
    // The binary's reader returns 0.0 for an absent key (CONFIRMED, attack-damage.md section 4.1).
    let m = MiscData::from_layers(&[], None);
    assert_eq!(m.defense_armor, F32::ZERO);
    assert!(m.damage_bonus.iter().flatten().all(|&v| v == F32::ZERO));
    assert!(m.ethereal_damage_bonus.iter().all(|&v| v == F32::ZERO));
    assert_eq!(m.reaction_delay, F32::ZERO);
    assert_eq!(m.attack_half_angle, None);
    assert_eq!(m.layers_with_misc, 0);
    assert!(m.defaulted.contains(&"DefenseArmor"));
    assert!(m.defaulted.contains(&"DamageBonusHero"));
    assert!(m.defaulted.contains(&"EtherealDamageBonus"));
    assert!(m.notes.is_empty());
}

#[test]
fn later_layers_override_earlier_ones_key_by_key() {
    let game = layer("[Misc]\nDefenseArmor=0.05\nReactionDelay=0.3\n");
    let misc = layer("[Misc]\n// the map changes only the armor constant\nDefenseArmor=0.1\n");
    let m = MiscData::from_layers(&[&game, &misc], None);
    assert_eq!(m.defense_armor, f(0.1f32), "map layer wins");
    assert_eq!(
        m.reaction_delay,
        f(0.3f32),
        "untouched key keeps the earlier layer's value"
    );
    assert_eq!(m.layers_with_misc, 2);
    assert!(!m.defaulted.contains(&"DefenseArmor"));
    assert!(m.defaulted.contains(&"AttackHalfAngle"));
}

#[test]
fn a_layer_without_a_misc_section_is_not_counted() {
    let other = layer("[Other]\nDefenseArmor=0.5\n");
    let m = MiscData::from_layers(&[&other], None);
    assert_eq!(m.layers_with_misc, 0);
    assert_eq!(
        m.defense_armor,
        F32::ZERO,
        "keys outside [Misc] are not read"
    );
}

#[test]
fn damage_bonus_rows_follow_the_binary_order_and_columns_are_the_list_order() {
    // Row r has values 10*r + c, so the row and column are visible in the value.
    let mut text = String::from("[Misc]\n");
    let keys = [
        "DamageBonusSpells",
        "DamageBonusNormal",
        "DamageBonusPierce",
        "DamageBonusSiege",
        "DamageBonusMagic",
        "DamageBonusChaos",
        "DamageBonusHero",
    ];
    for (r, key) in keys.iter().enumerate() {
        let values: Vec<String> = (0..ARMOR_COLUMNS)
            .map(|c| (10 * r + c).to_string())
            .collect();
        text.push_str(&format!("{key}={}\n", values.join(",")));
    }
    let misc = layer(&text);
    let m = MiscData::from_layers(&[&misc], None);
    assert_eq!(ATTACK_ROWS, 7);
    for r in 0..ATTACK_ROWS {
        for c in 0..ARMOR_COLUMNS {
            assert_eq!(
                m.damage_bonus[r][c],
                f((10 * r + c) as f32),
                "row {r} column {c}"
            );
        }
    }
    assert!(
        keys.iter().all(|k| !m.defaulted.contains(k)),
        "the seven damage keys are set: {:?}",
        m.defaulted
    );
}

#[test]
fn a_short_list_leaves_its_tail_at_zero_and_a_long_list_is_cut_at_eight() {
    // Elements are read by index: element 3 past the end of a short list is the reader's 0.0 (not a rejection).
    let misc = layer("[Misc]\nDamageBonusSiege=1,2,3\nDamageBonusMagic=1,2,3,4,5,6,7,8,9,10\n");
    let m = MiscData::from_layers(&[&misc], None);
    assert_eq!(m.damage_bonus[3][..3], [f(1.0), f(2.0), f(3.0)]);
    assert!(m.damage_bonus[3][3..].iter().all(|&v| v == F32::ZERO));
    assert!(!m.defaulted.contains(&"DamageBonusSiege"));
    assert!(
        m.notes.iter().any(|n| n.contains("3 values, 8 read")),
        "{:?}",
        m.notes
    );
    assert_eq!(
        m.damage_bonus[4][7],
        f(8.0),
        "element 8 and later are not read"
    );
}

#[test]
fn a_missing_damage_cell_is_zero_not_neutral() {
    let misc = layer("[Misc]\nDamageBonusNormal=1,1,1,1,1,1,1,1\n");
    let m = MiscData::from_layers(&[&misc], None);
    assert!(
        m.damage_bonus[0].iter().all(|&v| v == F32::ZERO),
        "absent row"
    );
    assert!(m.defaulted.contains(&"DamageBonusSpells"));
    assert!(m.damage_bonus[1].iter().all(|&v| v == F32::ONE));
}

#[test]
fn ethereal_entries_are_stored_in_the_binary_slot_order() {
    // Element k of EtherealDamageBonus is k + 1. The table slot of attack row r holds element ETHEREAL_ELEMENT_BY_ROW[r].
    assert_eq!(ETHEREAL_ELEMENT_BY_ROW, [5, 0, 1, 2, 3, 4, 6]);
    let misc = layer("[Misc]\nEtherealDamageBonus=1,2,3,4,5,6,7\n");
    let m = MiscData::from_layers(&[&misc], None);
    let by_row: Vec<F32> = (0..ATTACK_ROWS)
        .map(|r| m.ethereal_for_attack_type(r))
        .collect();
    assert_eq!(
        by_row,
        [6.0, 1.0, 2.0, 3.0, 4.0, 5.0, 7.0].map(f).to_vec(),
        "Spells gets element 5 (value 6), Hero gets element 6 (value 7)"
    );
    assert_eq!(m.ethereal_for_attack_type(7), F32::ZERO);
}

#[test]
fn a_malformed_real_is_rejected_and_defaults() {
    let misc = layer("[Misc]\nDefenseArmor=abc\nReactionDelay=1e3\n");
    let m = MiscData::from_layers(&[&misc], None);
    assert_eq!(m.defense_armor, F32::ZERO);
    assert_eq!(m.reaction_delay, F32::ZERO);
    assert_eq!(m.notes.len(), 2, "{:?}", m.notes);
}

#[test]
fn quoted_scalars_are_unquoted_and_half_angle_is_optional() {
    let misc = layer(
        "[Misc]\nDefenseArmor=\"0.2\"\nAttackHalfAngle=0.5\nEtherealDamageBonus=1,2,3,4,5,6,7\n",
    );
    let m = MiscData::from_layers(&[&misc], None);
    assert_eq!(m.defense_armor, f(0.2f32));
    assert_eq!(m.attack_half_angle, Some(f(0.5f32)));
    for (i, v) in m.ethereal_damage_bonus.iter().enumerate() {
        assert_eq!(*v, f((i + 1) as f32));
    }
    assert_eq!(ETHEREAL_ENTRIES, 7);
    assert_eq!(m.defense_armor, f(0.2f32));
}

#[test]
fn real_syntax_accepts_plain_decimals_only() {
    for ok in ["0", "1.", ".5", "-0.06", "+2", "10.25"] {
        assert!(is_real_syntax(ok), "{ok}");
    }
    for bad in ["", ".", "-", "1e3", "--1", "1.2.3", "0x10", "abc"] {
        assert!(!is_real_syntax(bad), "{bad}");
    }
    assert_eq!(parse_real("\"0.25\"", None), Some(f(0.25)));
    assert_eq!(parse_real("1e3", None), None);
}

/// With the game's tables the reals go through S2R: `1.5` is `0x3FC00001`, the value the original's S2R gives
/// (`docs/spec/softfloat.md`, the same DIV as the JASS literal path).
#[test]
fn reals_use_the_s2r_rules_with_the_game_tables() {
    let Ok(path) = std::env::var("GAME_DLL") else {
        eprintln!("GAME_DLL not set: skipping the S2R check");
        return;
    };
    let dll = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    let tables = Tables::from_game_dll(&dll).expect("tables");
    assert_eq!(
        parse_real("1.5", Some(&tables)).map(F32::to_bits),
        Some(0x3FC0_0001)
    );
    let misc = layer("[Misc]\nReactionDelay=1.5\n");
    let m = MiscData::from_layers(&[&misc], Some(&tables));
    assert_eq!(m.reaction_delay.to_bits(), 0x3FC0_0001);
}
