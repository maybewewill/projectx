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

fn level(dur: u32, herodur: u32, data: &[(usize, u32)]) -> sim::cast::SpellLevel {
    let mut d = [F32::ZERO; 9];
    for &(i, v) in data {
        d[i] = F32::from_bits(v);
    }
    sim::cast::SpellLevel {
        dur: F32::from_bits(dur),
        hero_dur: F32::from_bits(herodur),
        data: d,
        ..Default::default()
    }
}

fn opt(r: &[&str], i: usize) -> Option<u32> {
    (r[i] == "1").then(|| h(r[i + 1]))
}

/// The soft float tables are needed by the duration (a soft add): read from `GAME_DLL` like the other soft-float tests.
fn need_tables(what: &str) -> bool {
    let Ok(path) = std::env::var("GAME_DLL") else {
        eprintln!("GAME_DLL not set: {what} vectors skipped");
        return false;
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    softfloat::Tables::from_game_dll(&bytes).expect("tables are in the image");
    true
}

#[test]
fn scroll_of_protection_callback_matches_the_original() {
    if !need_tables("AIda") {
        return;
    }
    let rs = rows("DA");
    assert!(rs.len() >= 300);
    let (mut healed, mut buffed, mut both_off) = (0, 0, 0);
    for r in &rs {
        assert_eq!(r[10], "|");
        let lv = level(h(r[4]), h(r[5]), &[(0, h(r[7])), (1, h(r[8])), (2, h(r[9]))]);
        let out = sim::item_abilities::defense_callback(
            h(r[1]) & 0x4000_0000 != 0,
            h(r[2]) != 0,
            h(r[3]) != 0,
            &lv,
            F32::from_bits(h(r[6])),
        );
        assert_eq!(out.life.map(|v| v.to_bits()), opt(&r, 11), "life, row {r:?}");
        assert_eq!(out.mana.map(|v| v.to_bits()), opt(&r, 13), "mana, row {r:?}");
        let buff = (r[15] == "1").then(|| (h(r[16]), h(r[17])));
        assert_eq!(out.buff.map(|(a, d)| (a.to_bits(), d.to_bits())), buff, "buff, row {r:?}");
        healed += u32::from(out.life.is_some());
        buffed += u32::from(out.buff.is_some());
        both_off += u32::from(out.life.is_none() && out.buff.is_none());
    }
    assert!(healed > 50 && buffed > 50 && both_off > 5, "{healed} {buffed} {both_off}");
}
