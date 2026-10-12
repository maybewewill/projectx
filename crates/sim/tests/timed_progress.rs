//! Vector tests for the timed start `0x6F030920` and the construction life ramp `0x6F045600`
//! (docs/spec/construction.md section 13). Vectors come from `tools/emu/timed_start_emu.py` and
//! `tools/emu/construction_tick_emu.py`; the tests need `GAME_DLL` for the reciprocal table.

use sim::construction::{life_ramp, timed_start, COMPLETION_MESSAGE};
use softfloat::{Tables, F32};

const START: &str = include_str!("vectors/timed_start.tsv");
const TICK: &str = include_str!("vectors/construction_tick.tsv");

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn timed_start_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    for line in START.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let gate: i32 = f[0].parse().unwrap();
        let d = hex(f[1]);
        let r = hex(f[2]);
        let got = timed_start(F32(r), F32(d), gate, &t);
        if f[3] == "1" {
            let g = got.unwrap_or_else(|| panic!("expected start for D {d:08x} R {r:08x} gate {gate}"));
            assert_eq!(g.slope.0, hex(f[4]), "slope D {d:08x}");
            assert_eq!(g.horizon.0, hex(f[5]), "horizon D {d:08x} R {r:08x}");
            assert_eq!(hex(f[6]), 0, "listener threshold");
            assert_eq!(hex(f[7]), COMPLETION_MESSAGE);
        } else {
            assert!(got.is_none(), "expected no start for D {d:08x} gate {gate}");
        }
        n += 1;
    }
    assert!(n >= 500);
}

#[test]
fn life_ramp_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    for line in TICK.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (total, r, a, k) = (hex(f[0]), hex(f[1]), hex(f[2]), hex(f[3]));
        let got = life_ramp(F32(a), F32(total), F32(r), F32(k), &t);
        if f[4] == "-" {
            assert!(got.is_none(), "expected none: total {total:08x} k {k:08x}");
        } else {
            let (rate, hor) = got.unwrap_or_else(|| panic!("expected ramp: total {total:08x} k {k:08x}"));
            assert_eq!(rate.0, hex(f[4]), "rate total {total:08x} r {r:08x} a {a:08x} k {k:08x}");
            assert_eq!(hor.0, hex(f[5]), "horizon r {r:08x} k {k:08x}");
        }
        n += 1;
    }
    assert!(n >= 700);
}
