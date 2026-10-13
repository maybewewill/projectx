//! Feral Spirit against the original (Unicorn vectors from `tools/emu/feral_emu.py`, `tests/data/orc/feral.txt`).
//! `E` lines: the real cast effect `0x6F1DF330` on a fake ability (count, flag, duration rule). `P` lines: the real
//! creator wrapper `0x6F1D27A0` (placement). The `P` lines need `GAME_DLL` (soft-float tables).

use sim::orc::*;
use softfloat::{Tables, F32};

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn effect_count_flag_and_duration_rule_match_the_original() {
    let mut n = 0;
    for line in include_str!("data/orc/feral.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w[0] != "E" {
            continue;
        }
        let (dur, plus, data_b) = (f(w[1]), f(w[2]), f(w[3]));
        let unit_id = h(w[5]);
        let calls: usize = w[8].parse().unwrap();
        let out = feral_plan(dur, plus, data_b);
        assert_eq!(out.count as usize, calls, "{line}");
        if calls > 0 {
            // per call: type is-caster owner-arg pointer count-arg flag
            let rec: Vec<&str> =
                line.split('|').nth(1).unwrap().split(';').next().unwrap().split_whitespace().skip(1).collect();
            assert_eq!(h(rec[0]), unit_id, "{line}");
            assert_eq!(rec[1], "1", "{line}");
            assert_eq!(rec[2], "1", "{line}");
            assert_eq!(rec[3], "1", "{line}");
            assert_eq!(rec[4], "1", "{line}");
            assert_eq!(h(rec[5]), out.flags, "{line}");
        }
        let n3e4: usize = line.split('|').nth(2).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(n3e4, calls, "{line}");
        n += 1;
    }
    assert!(n >= 300, "{n}");
}

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

#[test]
fn placement_matches_the_original() {
    let Some(dll) = game_dll() else {
        return;
    };
    let t = Tables::from_game_dll(&dll).expect("softfloat tables");
    let mut n = 0;
    for line in include_str!("data/orc/feral.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w[0] != "P" {
            continue;
        }
        let (cx, cy, facing, area) = (f(w[1]), f(w[2]), f(w[3]), f(w[4]));
        let flag = h(w[7]);
        // | owner x y facing flags one cell type
        let (x, y) = feral_place(cx, cy, facing, area, &t);
        assert_eq!((x.to_bits(), y.to_bits()), (h(w[10]), h(w[11])), "{line}");
        assert_eq!(h(w[12]), facing.to_bits(), "{line}");
        assert_eq!(h(w[13]), flag | 2, "{line}");
        n += 1;
    }
    assert!(n >= 300, "{n}");
}
