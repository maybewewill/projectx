//! Tests of the target side of one damage event (`sim::damage::target_side_amount`, `docs/spec/attack-damage.md`
//! section 11). The first group is synthetic and runs everywhere. The second compares bit for bit with the original,
//! run in Unicorn by `tools/emu/target_damage_emu.py`; it needs `GAME_DLL` (for the lookup tables) and
//! `TARGET_DAMAGE_VECTORS` (the file written by that script, kept outside the repository).

use sim::damage::{
    status_flag, target_side_amount, DamageMisc, DamageRules, DamageTypeTable, NegativeArmorTable,
    TargetSide, ATTACK_DAMAGE_CLASS, ARMOR_TYPE_COLS, ATTACK_TYPE_ROWS, NEG_ARMOR_LEN,
};
use softfloat::{Tables, F32};

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

/// A negative-armor table whose entry 0 is +0 (so armor in (-1, 0] is no armor, with no division) and whose entry
/// 20 is 0.95 (armor -40 leaves 5 percent).
fn negative() -> NegativeArmorTable {
    let mut bits = [0u32; NEG_ARMOR_LEN];
    for (i, b) in bits.iter_mut().enumerate().skip(1) {
        *b = f(0.1 * i as f32).to_bits();
    }
    bits[20] = f(0.95).to_bits();
    NegativeArmorTable::from_bits(bits).expect("non-zero")
}

fn misc(ethereal: f32) -> DamageMisc {
    DamageMisc {
        defense_armor: F32::ZERO,
        type_table: DamageTypeTable([[F32::ONE; ARMOR_TYPE_COLS]; ATTACK_TYPE_ROWS]),
        ethereal_by_attack_type: [f(ethereal); ATTACK_TYPE_ROWS],
    }
}

/// A basic attack on a target with no armor and no flags, from a source: every test changes one thing.
fn side(raw: f32) -> TargetSide {
    TargetSide {
        raw: f(raw),
        class: ATTACK_DAMAGE_CLASS,
        category: 1,
        source_present: true,
        record_flags: 0x100,
        target_flags: 0,
        target_flags20: 0,
        armor: f(-0.5), // table entry 0 = 0: no armor, no division
        armor_type: 4,
        counter_158: 0,
        counter_15c: 0,
        invulnerable: false,
        not_allied: false,
        relation: None,
    }
}

fn run(s: &TargetSide, m: &DamageMisc, rules: &DamageRules) -> F32 {
    target_side_amount(s, m, rules, &negative(), &Tables::default())
}

#[test]
fn plain_attack_passes_the_raw_amount() {
    let r = DamageRules::default();
    assert_eq!(run(&side(10.0), &misc(1.0), &r), f(10.0));
}

#[test]
fn minimum_one_is_taken_before_the_armor_step() {
    let r = DamageRules::default();
    // Raw 0.5 stays 0.5.
    assert_eq!(run(&side(0.5), &misc(1.0), &r), f(0.5));
    // Raw 7 under armor -40 (5 percent left, 0.35) is raised to 1.0: min(7, 1.0) is the floor.
    let mut s = side(7.0);
    s.armor = f(-40.0);
    assert_eq!(run(&s, &misc(1.0), &r), F32::ONE);
    // Raw 7 with class not 0x10: no armor step, and the amount stays 7.
    s.class = 0x20;
    assert_eq!(run(&s, &misc(1.0), &r), f(7.0));
}

#[test]
fn status_flag_skips_armor_and_uses_column_four() {
    assert!(status_flag(0x0001_0000 | 0x0200_0000));
    assert!(!status_flag(0x0200_0000), "needs bit 16 too");
    assert!(!status_flag(0x0001_0000), "needs bit 25 too");
    assert!(
        status_flag(0x0001_0000 | 0x0200_0000 | 0x8000_0000),
        "bit 31 is not tested: the routine tests the low byte's sign bit"
    );
    assert!(!status_flag(0x0001_0000 | 0x0200_0000 | 0x80), "bit 7 without bit 27 blocks it");
    assert!(status_flag(0x0001_0000 | 0x0200_0000 | 0x80 | 0x0800_0000));
    // With the flag, the armor -40 step is skipped: raw 7 stays 7 (the table is 1.0).
    let mut s = side(7.0);
    s.armor = f(-40.0);
    s.target_flags = 0x0001_0000 | 0x0200_0000;
    assert_eq!(run(&s, &misc(1.0), &DamageRules::default()), f(7.0));
}

#[test]
fn table_factor_is_the_attack_row_and_armor_column() {
    let mut m = misc(1.0);
    let mut rows = [[F32::ONE; ARMOR_TYPE_COLS]; ATTACK_TYPE_ROWS];
    rows[1][2] = f(0.5);
    m.type_table = DamageTypeTable(rows);
    let mut s = side(8.0);
    s.armor_type = 2;
    assert_eq!(run(&s, &m, &DamageRules::default()), f(4.0));
    s.armor_type = 3;
    assert_eq!(run(&s, &m, &DamageRules::default()), f(8.0));
}

#[test]
fn ethereal_factor_needs_a_condition_and_a_positive_counter() {
    let r = DamageRules::default();
    let m = misc(2.0);
    let mut s = side(5.0);
    s.counter_158 = 1;
    // Source present, no ally rule, not an enemy: no ethereal factor.
    assert_eq!(run(&s, &m, &r), f(5.0));
    // Source missing: the factor applies.
    s.source_present = false;
    assert_eq!(run(&s, &m, &r), f(10.0));
    // Enemy (not allied): the factor applies.
    s.source_present = true;
    s.not_allied = true;
    assert_eq!(run(&s, &m, &r), f(10.0));
    // Counter zero: never.
    s.counter_158 = 0;
    assert_eq!(run(&s, &m, &r), f(5.0));
    // Ally rule on: applies to an allied source too.
    s.counter_158 = 1;
    s.not_allied = false;
    let rules = DamageRules {
        ethereal_damage_bonus_ally: true,
        ..DamageRules::default()
    };
    assert_eq!(run(&s, &m, &rules), f(10.0));
}

#[test]
fn relation_factor_multiplies_or_zeroes_the_amount() {
    let r = DamageRules::default();
    let mut s = side(3.0);
    s.class = 0x20; // no armor step, so the factor is the only change
    s.relation = Some((false, true));
    assert_eq!(run(&s, &misc(1.0), &r), f(300.0));
    s.relation = Some((true, false));
    assert_eq!(run(&s, &misc(1.0), &r), F32::ZERO);
    s.relation = Some((true, true));
    assert_eq!(run(&s, &misc(1.0), &r), f(3.0));
    s.relation = None;
    assert_eq!(run(&s, &misc(1.0), &r), f(3.0));
}

#[test]
fn gates_stop_the_damage_by_class_and_counters() {
    let r = DamageRules::default();
    let m = misc(1.0);
    // Rule off: class 0x40 is gated by the first gate (counter 0x15C or the invulnerable answer).
    let mut s = side(4.0);
    s.class = 0x40;
    s.counter_15c = 1;
    assert_eq!(run(&s, &m, &r), F32::ZERO);
    s.counter_15c = 0;
    s.invulnerable = true;
    assert_eq!(run(&s, &m, &r), F32::ZERO);
    s.invulnerable = false;
    assert_eq!(run(&s, &m, &r), f(4.0));
    // Class 0x10 is not gated (rule off): the invulnerable answer is not asked.
    let mut t = side(4.0);
    t.invulnerable = true;
    assert_eq!(run(&t, &m, &r), f(4.0));
    // Rule on: attack row 4 uses the first gate with the counter 0x15C.
    let rules = DamageRules {
        magic_immunes_resist_damage: true,
        ..DamageRules::default()
    };
    let mut u = side(4.0);
    u.category = 4;
    u.counter_15c = 1;
    assert_eq!(run(&u, &m, &rules), F32::ZERO);
    u.category = 0;
    assert_eq!(run(&u, &m, &rules), f(4.0));
}

#[test]
fn record_and_target_flags_stop_the_damage() {
    let r = DamageRules::default();
    let m = misc(1.0);
    let mut s = side(4.0);
    s.target_flags = 0x100;
    assert_eq!(run(&s, &m, &r), F32::ZERO, "dead or untargetable");
    let mut s = side(4.0);
    s.record_flags = 0x4;
    assert_eq!(run(&s, &m, &r), F32::ZERO);
    s.record_flags = 0x10;
    assert_eq!(run(&s, &m, &r), F32::ZERO);
    let mut s = side(4.0);
    s.target_flags20 = 0x8;
    assert_eq!(run(&s, &m, &r), F32::ZERO, "flag 8 without record flag 0x20");
    s.record_flags = 0x20;
    assert_eq!(run(&s, &m, &r), f(4.0));
}

/// The vector file written by `tools/emu/target_damage_emu.py`: 21 input columns, 56 table cells, 7 ethereal slots,
/// 21 negative-armor entries, the output and the value sent to the HP change. `# neg` lines are skipped.
#[test]
fn target_side_matches_the_original_bit_for_bit() {
    let (Some(dll_path), Some(vec_path)) = (
        std::env::var_os("GAME_DLL"),
        std::env::var_os("TARGET_DAMAGE_VECTORS"),
    ) else {
        eprintln!("skipped: GAME_DLL and TARGET_DAMAGE_VECTORS not both set");
        return;
    };
    let dll = std::fs::read(dll_path).expect("read GAME_DLL");
    let tables = Tables::from_game_dll(&dll).expect("lookup tables in GAME_DLL");
    let text = std::fs::read_to_string(vec_path).expect("read TARGET_DAMAGE_VECTORS");
    let (mut cases, mut mismatches, mut nonzero) = (0u32, 0u32, 0u32);
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let v: Vec<u32> = line
            .split_whitespace()
            .map(|t| u32::from_str_radix(t, 16).expect("hex column"))
            .collect();
        assert_eq!(v.len(), 21 + 56 + 7 + 21 + 2, "columns per case");
        let raw = v[0];
        let side = TargetSide {
            raw: F32::from_bits(raw),
            class: v[1],
            category: v[2],
            source_present: v[3] != 0,
            record_flags: v[4],
            target_flags: v[5],
            target_flags20: v[6],
            armor: F32::from_bits(v[7]),
            armor_type: v[8] as u8,
            counter_158: v[9] as i32,
            counter_15c: v[10] as i32,
            invulnerable: v[11] != 0,
            not_allied: v[14] != 0,
            relation: (v[15] != 0).then_some((v[16] != 0, v[17] != 0)),
        };
        let misc = DamageMisc {
            defense_armor: F32::from_bits(v[18]),
            type_table: DamageTypeTable(std::array::from_fn(|r| {
                std::array::from_fn(|c| F32::from_bits(v[21 + r * 8 + c]))
            })),
            ethereal_by_attack_type: std::array::from_fn(|k| F32::from_bits(v[77 + k])),
        };
        let neg_bits: [u32; NEG_ARMOR_LEN] = std::array::from_fn(|i| v[84 + i]);
        let neg = NegativeArmorTable::from_bits(neg_bits).expect("negative table");
        let rules = DamageRules {
            magic_immunes_resist_damage: v[12] != 0,
            ethereal_damage_bonus_ally: v[13] != 0,
            relation_constant: F32::from_bits(0x42C8_0000),
        };
        let got = target_side_amount(&side, &misc, &rules, &neg, &tables).to_bits();
        let want = v[105];
        cases += 1;
        if want != 0 {
            nonzero += 1;
        }
        if got != want {
            mismatches += 1;
            if mismatches <= 10 {
                eprintln!(
                    "mismatch case {cases}: got {got:#010x} want {want:#010x} (raw {raw:#010x} cat {} class {:#x})",
                    side.category, side.class
                );
            }
        }
    }
    eprintln!("target side: {cases} cases ({nonzero} with damage), {mismatches} mismatches");
    assert!(cases > 0, "no cases in the vector file");
    assert_eq!(mismatches, 0);
}
