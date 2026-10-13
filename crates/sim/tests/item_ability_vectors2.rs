//! The numeric cores of the remaining item abilities against `Game.dll` 1.26a (`docs/spec/abilities-items.md`, S41).
//!
//! `item_ability_vectors2.txt` is the output of `tools/emu/item_cores2_emu.py 350 7`: every row is the behaviour of the
//! ORIGINAL code (Unicorn) on fake objects, with the callees that need the object model stubbed and recorded (the row
//! formats are in the module notes of the script). Each test recomputes the rows with `sim::item_abilities`.

use sim::cast::SpellLevel;
use sim::item_abilities::*;
use softfloat::F32;

const ROWS: &str = include_str!("item_ability_vectors2.txt");

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("hex {s}"))
}

fn rows(kind: &str) -> Vec<Vec<&'static str>> {
    ROWS.lines()
        .filter(|l| l.split(' ').next() == Some(kind))
        .map(|l| l.split(' ').collect())
        .collect()
}

fn f(b: u32) -> F32 {
    F32::from_bits(b)
}

#[test]
fn group_cooldown_start_matches_the_original() {
    let rs = rows("GR");
    assert!(rs.len() >= 300);
    let mut started = 0;
    for r in &rs {
        let (cool, ignore) = (h(r[1]), h(r[2]) != 0);
        assert_eq!(r[3], "|");
        let want = if r[4] == "1" { Some(h(r[5])) } else { None };
        let got = group_cooldown_start(f(cool), ignore).map(|c| c.to_bits());
        assert_eq!(got, want, "row {r:?}");
        started += usize::from(want.is_some());
    }
    assert!(started > 20, "the rows reach the start path");
}

#[test]
fn potion_buff_duration_matches_the_original() {
    let rs = rows("VI");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (hero, dur, herodur, extra) = (h(r[2]) != 0, h(r[3]), h(r[4]), h(r[5]));
        assert_eq!(r[6], "|");
        let level = SpellLevel {
            dur: f(dur),
            hero_dur: f(herodur),
            ..Default::default()
        };
        let got = potion_buff_duration(&level, hero, f(extra)).to_bits();
        assert_eq!(got, h(r[7]), "row {r:?}");
    }
}

#[test]
fn life_regen_item_gate_matches_the_original() {
    let rs = rows("RL");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (v88, illusion, misc) = (h(r[1]), h(r[2]) != 0, h(r[3]) != 0);
        assert_eq!(r[4], "|");
        let called = regen_item_gate(illusion, misc);
        assert_eq!(called, r[5] == "1", "row {r:?}");
        if called {
            assert_eq!(v88, h(r[6]), "the modifier rate is the ability value, row {r:?}");
        }
    }
}

#[test]
fn mana_regen_item_rate_matches_the_original() {
    let rs = rows("RM");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (type_regen, x, v88) = (h(r[1]), h(r[2]), h(r[3]));
        assert_eq!(r[4], "|");
        let got = mana_regen_item_rate(f(type_regen), f(x), f(v88)).to_bits();
        assert_eq!(got, h(r[5]), "row {r:?}");
    }
}

#[test]
fn hero_int_regen_term_matches_the_original() {
    let rs = rows("RH");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (misc, int) = (h(r[1]), h(r[2]) as i32);
        assert_eq!(r[3], "|");
        let got = hero_int_regen_term(f(misc), int).to_bits();
        assert_eq!(got, h(r[4]), "row {r:?}");
    }
}
