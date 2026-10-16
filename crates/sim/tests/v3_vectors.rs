//! Wave V3 verification cores against `Game.dll` 1.26a. `v3_vectors.txt` is the output of
//! `tools/emu/v3_cores_emu.py crates/sim/tests/v3_vectors.txt 300 7`: every row is the behaviour of the ORIGINAL code
//! (Unicorn) on fake objects, callees that need the object model stubbed and recorded. Row formats: the module notes of
//! the script. The vectors are plain numbers (no Blizzard code or data); the tests run without `GAME_DLL`.

use sim::invisibility::*;
use softfloat::F32;

const ROWS: &str = include_str!("v3_vectors.txt");

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("hex {s}"))
}

fn rows(kind: &str) -> Vec<Vec<&'static str>> {
    ROWS.lines()
        .filter(|l| l.split(' ').next() == Some(kind))
        .map(|l| l.split(' ').collect())
        .collect()
}

fn class_char(c: CounterClass) -> &'static str {
    match c {
        CounterClass::Nothing => "-",
        CounterClass::Notify => "N",
        CounterClass::Immediate => "I",
        CounterClass::Ramp => "R",
    }
}

fn counter_rows(kind: &str, step: fn(i32, u32, F32) -> (i32, u32, CounterClass)) {
    let rs = rows(kind);
    assert!(rs.len() >= 300, "{kind}: {} rows", rs.len());
    let mut seen = std::collections::BTreeSet::new();
    for r in &rs {
        assert_eq!(r[4], "|");
        let (c, fl, cl) = step(h(r[1]) as i32, h(r[2]), F32::from_bits(h(r[3])));
        assert_eq!((c as u32, fl, class_char(cl)), (h(r[5]), h(r[6]), r[7]), "row {r:?}");
        seen.insert(r[7]);
    }
    assert!(seen.len() >= 3, "{kind}: the rows reach several branches: {seen:?}");
}

#[test]
fn invisibility_increment_matches_the_original() {
    counter_rows("IC", counter_increment);
}

#[test]
fn invisibility_decrement_matches_the_original() {
    counter_rows("ID", counter_decrement);
}

#[test]
fn viewer_gate_matches_the_original() {
    let rs = rows("VG");
    assert!(rs.len() >= 300);
    let (mut pass, mut fail) = (0, 0);
    for r in &rs {
        assert_eq!(r[6], "|");
        let got = viewer_gate_core(h(r[1]), h(r[2]), h(r[3]), h(r[4]), h(r[5]));
        assert_eq!(u32::from(got), h(r[7]), "row {r:?}");
        if got {
            pass += 1
        } else {
            fail += 1
        }
    }
    assert!(pass > 50 && fail > 50, "both outcomes are reached: {pass}/{fail}");
}
