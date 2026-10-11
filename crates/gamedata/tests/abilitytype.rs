//! Ability definitions and level records (`gamedata::abilitytype`). Synthetic SYLK tables only. The record layout
//! and the rules were checked against the original builder in Unicorn (`tools/emu/abilitytype_binder_emu.py`).

use gamedata::abilitytype::{
    clamp_cooldown, list_slots, load_all, raw_code, read_ability, read_level, LEVEL_RECORD_SIZE,
};
use gamedata::slk::Slk;

/// Builds a SYLK table: the first column is the row key. Numeric-looking cells are written as numbers.
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

fn cols() -> Vec<&'static str> {
    vec![
        "AbilityID", "levels", "version", "checkDep", "reqLevel", "levelSkip", "priority",
        "targs1", "Cast1", "Dur1", "HeroDur1", "Cost1", "Cool1", "Area1", "Rng1", "DataA1",
        "DataI1", "UnitID1", "BuffID1", "EfctID1", "Cast2", "Cool2", "BuffID2", "EfctID2",
    ]
}

#[test]
fn raw_code_packs_four_bytes_big_endian() {
    assert_eq!(raw_code("hfoo"), 0x6866_6F6F);
    assert_eq!(raw_code("abcde"), 0x6162_6364);
    assert_eq!(raw_code("a"), 0x61);
    assert_eq!(raw_code(""), 0);
}

#[test]
fn list_slots_follow_the_builder_count_rule() {
    // "BHwe,BUan": length 9 gives 2 slots
    assert_eq!(list_slots(Some("BHwe,BUan")), vec![0x4248_7765, 0x4255_616E]);
    // length 4 gives 1 slot; a missing text gives 1 zero slot
    assert_eq!(list_slots(Some("AHbz")), vec![0x4148_627A]);
    assert_eq!(list_slots(None), vec![0]);
    // "a,b,c,d" is 7 characters: 1 slot, only the first token fits
    assert_eq!(list_slots(Some("a,b,c,d")), vec![0x61]);
    // empty tokens do not take a slot: ",,AHbz" is 6 characters, so 1 slot
    assert_eq!(list_slots(Some(",,AHbz")), vec![0x4148_627A]);
    // 12 characters with one token: 3 slots, the rest zero
    assert_eq!(
        list_slots(Some("AHbz        ")),
        vec![0x4148_627A, 0, 0]
    );
}

#[test]
fn cooldown_clamp_matches_the_builder() {
    assert_eq!(clamp_cooldown(-3.0).to_bits(), 0.0f32.to_bits());
    // -0.0 satisfies 0.0 <= -0.0, so the value is kept as it is
    assert_eq!(clamp_cooldown(-0.0).to_bits(), (-0.0f32).to_bits());
    assert_eq!(clamp_cooldown(2.5), 2.5);
    assert_eq!(clamp_cooldown(f32::NAN).to_bits(), 0.0f32.to_bits());
}

#[test]
fn reads_one_level_with_all_fields() {
    let t = table(
        &cols(),
        &[&[
            "AHbz", "2", "1", "1", "6", "0", "3", "", "0.25", "10", "12.5", "75", "4.5", "250",
            "600", "1", "9", "hfoo", "BHwe,BUan", "Hfoo",
        ]],
    );
    let l = read_level(&t, "AHbz", 1);
    assert_eq!(l.targs, "");
    assert_eq!(l.cast, 0.25);
    assert_eq!(l.dur, 10.0);
    assert_eq!(l.hero_dur, 12.5);
    assert_eq!(l.cost, 75);
    assert_eq!(l.cool, 4.5);
    assert_eq!(l.area, 250.0);
    assert_eq!(l.rng, 600.0);
    assert_eq!(l.data[0], 1.0);
    assert_eq!(l.data[8], 9.0);
    assert_eq!(l.unit_id, 0x6866_6F6F);
    assert_eq!(l.buff_ids, vec![0x4248_7765, 0x4255_616E]);
    assert_eq!(l.efct_ids, vec![0x4866_6F6F]);
}

#[test]
fn missing_cells_give_defaults() {
    let t = table(&cols(), &[&["AHbz", "1"]]);
    let l = read_level(&t, "AHbz", 1);
    assert_eq!(l.cost, 0);
    assert_eq!(l.cool, 0.0);
    assert_eq!(l.unit_id, 0);
    assert_eq!(l.buff_ids, vec![0]);
    assert_eq!(l.efct_ids, vec![0]);
}

#[test]
fn integer_cells_use_the_strtol_rules() {
    let t = table(
        &cols(),
        &[&["AHbz", "1", "0", "0", "0", "0", "0", "", "0", "0", "0", "0x10", "-3"]],
    );
    // Cost1 = "0x10" -> 16, Cool1 = -3 clamped to 0
    let l = read_level(&t, "AHbz", 1);
    assert_eq!(l.cost, 16);
    assert_eq!(l.cool, 0.0);
}

#[test]
fn definition_reads_all_levels_in_order() {
    let t = table(
        &cols(),
        &[&["AHbz", "3", "2", "1", "6", "1", "4", "", "0", "0", "0", "0", "0", "0", "0", "0", "0", "", "", "", "", "2.5", "", ""]],
    );
    let d = read_ability(&t, "AHbz").expect("row exists");
    assert_eq!(d.code, raw_code("AHbz"));
    assert_eq!(d.version, 2);
    assert!(d.check_dep);
    assert_eq!(d.levels_max, 3);
    assert_eq!(d.req_level, 6);
    assert_eq!(d.level_skip, 1);
    assert_eq!(d.priority, 4);
    // only level 2 has a cooldown in this table; levels 1 and 3 read missing cells as 0
    assert_eq!(d.levels.len(), 3);
    assert_eq!(d.levels[1].cool, 2.5);
    assert_eq!(d.levels[0].cool, 0.0);
    assert_eq!(d.levels[2].cool, 0.0);
    assert_eq!(LEVEL_RECORD_SIZE, 0x68);
}

#[test]
fn zero_levels_and_missing_rows() {
    let t = table(&cols(), &[&["AHbz", "0"]]);
    assert!(read_ability(&t, "AHbz").expect("row exists").levels.is_empty());
    assert!(read_ability(&t, "ZZZZ").is_none());
}

#[test]
fn load_all_is_keyed_by_raw_code() {
    let t = table(
        &cols(),
        &[&["AHbz", "1"], &["AHbb", "2"]],
    );
    let all = load_all(&t);
    assert_eq!(all.len(), 2);
    assert_eq!(all[&raw_code("AHbz")].levels_max, 1);
    assert_eq!(all[&raw_code("AHbb")].levels_max, 2);
    // BTreeMap order: ascending raw code
    let keys: Vec<u32> = all.keys().copied().collect();
    assert!(keys.windows(2).all(|w| w[0] < w[1]));
}
