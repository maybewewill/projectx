//! Anti-Magic Shell against the original (`tools/emu/v9_emu.py` checks `ams_effect` / `ami_effect`, docs/spec/abilities-v9.md): the real effect
//! `0x6F13C900` (the instant class enters through `0x6F143040` with the owner as target) with the level getters, the hero test, `0x6F13C790`
//! and the code getters running for real; the dispel worker, the buff lookup / factories and the buff slots are recording stubs.
//! Also the magic immunity counters (`immunity`): `0x6F07A3A0` / `0x6F077320`, the Magic Immunity init / removal slots.

use sim::cast::SpellLevel;
use sim::unit_abilities_a::ams_plan;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

const BUFF: u32 = 0x40_1000; // a new Bams
const EX1: u32 = 0x40_5A00; // the existing Bams
const EX2: u32 = 0x40_5B00; // the existing Bam2
const NEW2: u32 = 0x40_5C00; // a new Bam2

fn check(text: &str, instant: bool) {
    // 0..4 DataA..D | 4 dur 5 herodur 6 extra 7 hero 8 ex_bams 9 ex_bam2 10 locked 11 list item | 12..24 worker (ecx edx target order
    // ability pos neg a6 a7 damage mana flags) | 24..30 slot 0x324 (calls ecx edx arg - -) | 30..36 slot 0x354 (calls ecx edx target dur
    // data) | 36..42 slot 0x358 (calls ecx edx ref dur data)
    let rows = hex_rows(text);
    assert!(rows.len() >= 300);
    let (mut bams_new, mut bams_old, mut two_new, mut two_old, mut locked, mut nan) = (0, 0, 0, 0, 0, 0);
    let mut target = None;
    for r in &rows {
        let f = |i: usize| F32::from_bits(r[i]);
        let l = SpellLevel {
            dur: f(4),
            hero_dur: f(5),
            data: [f(0), f(1), f(2), f(3), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let plan = ams_plan(&l, r[7] != 0, f(6), r[8] != 0, r[9] != 0, r[10] != 0 && !instant);
        let w = &r[12..24];
        assert_eq!(w[3], 852_186, "order id, row {:08x?}", r);
        assert_eq!((w[5], w[6], w[7], w[8]), (plan.worker.positive, plan.worker.negative, plan.worker.a6, plan.worker.a7), "row {:08x?}", r);
        assert_eq!((w[9], w[10], w[11]), (plan.worker.damage.to_bits(), plan.worker.mana.to_bits(), plan.worker.flags), "row {:08x?}", r);
        target.get_or_insert(w[2]);
        assert_eq!(Some(w[2]), target, "worker target, row {:08x?}", r);
        let (c324, c354, c358) = (r[24], r[30], r[36]);
        match plan.buff {
            None => {
                assert_eq!((c324, c354, c358), (0, 0, 0), "locked target, row {:08x?}", r);
                locked += 1;
            }
            Some(b) => {
                if b.existing {
                    let ex = if b.immune { EX1 } else { EX2 };
                    assert_eq!((c324, c354, c358), (0, 0, 1), "refresh, row {:08x?}", r);
                    assert_eq!((r[37], r[40], r[41]), (ex, b.dur.to_bits(), b.data.to_bits()), "refresh arguments, row {:08x?}", r);
                } else {
                    let new = if b.immune { BUFF } else { NEW2 };
                    assert_eq!((c324, c354, c358), (1, 1, 0), "create, row {:08x?}", r);
                    assert_eq!((r[25], r[31]), (new, new), "new buff, row {:08x?}", r);
                    assert_eq!((r[33], r[34], r[35]), (target.unwrap(), b.dur.to_bits(), b.data.to_bits()), "apply arguments, row {:08x?}", r);
                }
                match (b.immune, b.existing) {
                    (true, false) => bams_new += 1,
                    (true, true) => bams_old += 1,
                    (false, false) => two_new += 1,
                    (false, true) => two_old += 1,
                }
            }
        }
        nan += usize::from(f(2).to_f32().is_nan());
    }
    assert!(bams_new > 30 && bams_old > 10 && two_new > 30 && two_old > 10 && (locked > 30 || instant) && nan > 3, "{bams_new} {bams_old} {two_new} {two_old} {locked} {nan}");
}

#[test]
fn anti_magic_shell_effect_matches_the_original() {
    check(include_str!("data/v9/ams_effect.txt"), false);
}

#[test]
fn instant_anti_magic_shell_effect_matches_the_original() {
    check(include_str!("data/v9/ami_effect.txt"), true);
}

#[test]
fn magic_immunity_counters_match_the_original() {
    // start+0x15C start+0x160 purge ops | +0x15C +0x160 | enter leave | lookups removals | walker calls, walker args (9)
    let mut n = 0;
    let (mut enters, mut leaves, mut second_enter) = (0, 0, 0);
    for line in include_str!("data/v9/immunity.txt").lines().filter(|l| !l.trim().is_empty()) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let h = |i: usize| u32::from_str_radix(w[i], 16).unwrap();
        let mut counts = (h(0) as i32, h(1) as i32);
        let purge = h(2) != 0;
        let (mut en, mut le) = (0u32, 0u32);
        for op in w[3].chars() {
            // A / R: the Magic Immunity init / removal slots (flag 1); a / r: the counters with flag 0; B / D: the Bams apply / removal
            // (flag 0); C: the Bam2 apply (no count)
            let (next, hit) = match op {
                'A' => sim::neutral_spells::immunity_add(counts, true),
                'a' | 'B' => sim::neutral_spells::immunity_add(counts, false),
                'R' => sim::neutral_spells::immunity_remove(counts, true),
                'C' => (counts, false),
                _ => sim::neutral_spells::immunity_remove(counts, false),
            };
            if op == 'A' || op == 'a' || op == 'B' {
                en += u32::from(hit);
                // the second counter reaching 1 while the first is already above 1 enters again
                second_enter += u32::from(hit && next.0 != 1);
            } else {
                le += u32::from(hit);
            }
            counts = next;
        }
        assert_eq!((h(4), h(5)), (counts.0 as u32, counts.1 as u32), "counters, row {line}");
        assert_eq!((h(6), h(7)), (en, le), "enter / leave work, row {line}");
        // every enter looks the purge debuff up, removes it when present and runs the dispel walker once
        assert_eq!((h(8), h(9), h(10)), (en, if purge { en } else { 0 }, en), "lookups / removals / walker calls, row {line}");
        if en > 0 {
            // the walker arguments of the last call: unit-less 0,1,0,0,0,1,1,0,1 (MAGIC_IMMUNITY_DISPEL)
            let args: Vec<u32> = (0..9).map(|i| h(11 + i)).collect();
            assert_eq!(args, [0, 1, 0, 0, 0, 1, 1, 0, 1], "walker arguments, row {line}");
        }
        enters += en;
        leaves += le;
        n += 1;
    }
    assert!(n >= 400 && enters > 200 && leaves > 150 && second_enter > 20, "{n} {enters} {leaves} {second_enter}");
}
