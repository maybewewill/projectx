//! The kill-XP position normaliser and candidate range test of Game.dll 1.26a (`docs/spec/heroes.md`, section 8.3)
//! against the original. The vector file is written by `tools/emu/hero_normaliser_emu.py` to a path the caller
//! chooses; it holds synthetic inputs and the outputs of the original, and is not committed.
//!
//! `N` lines check `sim::heroes::normalise` (the normaliser `0x6F4743A0`, bit for bit). `R` lines check
//! `kill_distance2` (the distance `D` the routine `0x6F28B990` reads) and `in_range` (listed unless `V < D`).
//!
//! Run: `KILL_RANGE_VECTORS=/path/to/vectors.txt cargo test -p sim --test kill_range_vectors`.
//! Without the variable the test returns early (and says so).

use sim::heroes::{in_range, kill_distance2, normalise, GridCell, NormUnit, WorldOrigin};
use softfloat::F32;

fn bits(tok: &str) -> u32 {
    u32::from_str_radix(tok, 16).unwrap_or_else(|e| panic!("bad hex {tok}: {e}"))
}

fn f(tok: &str) -> F32 {
    F32(bits(tok))
}

#[test]
fn normaliser_and_range_match_original() {
    let Ok(path) = std::env::var("KILL_RANGE_VECTORS") else {
        eprintln!("kill_range_vectors: KILL_RANGE_VECTORS not set, skipped");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let (mut n_cases, mut n_bad) = (0usize, 0usize);
    let (mut r_cases, mut r_bad) = (0usize, 0usize);
    let (mut listed, mut out, mut ties, mut nan_v) = (0usize, 0usize, 0usize, 0usize);
    for line in text.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        match t.first().copied() {
            Some("N") if t.len() == 18 => {
                n_cases += 1;
                let unit = NormUnit {
                    grid_word: bits(t[3]),
                    i: f(t[4]),
                    j: bits(t[5]) as i32,
                    x: f(t[6]),
                    y: f(t[7]),
                    xv: f(t[8]),
                    yv: f(t[9]),
                };
                let grid_a = GridCell { origin: f(t[10]), row: bits(t[11]) as i32, scale: f(t[12]) };
                let grid_b = GridCell { origin: f(t[13]), row: bits(t[14]) as i32, scale: f(t[15]) };
                let origin = WorldOrigin { x: f(t[1]), y: f(t[2]) };
                let got = normalise(&unit, [grid_a, grid_b], origin);
                if got != (f(t[16]), f(t[17])) {
                    n_bad += 1;
                    if n_bad <= 5 {
                        eprintln!("normaliser mismatch: {line}\n  got {:08x} {:08x}", got.0 .0, got.1 .0);
                    }
                }
            }
            Some("R") if t.len() == 8 => {
                r_cases += 1;
                let rec = (f(t[1]), f(t[2]));
                let v = f(t[3]);
                let cand = (f(t[4]), f(t[5]));
                let d_want = f(t[6]);
                let listed_want = t[7] == "1";
                let d = kill_distance2(rec, cand);
                let decided = in_range(d, v);
                if (v.0 & 0x7F80_0000) == 0x7F80_0000 && (v.0 & 0x007F_FFFF) != 0 {
                    nan_v += 1;
                }
                if d != d_want || decided != listed_want {
                    r_bad += 1;
                    if r_bad <= 5 {
                        eprintln!("range mismatch: {line}\n  D {:08x} listed {decided}", d.0);
                    }
                }
                if decided {
                    listed += 1;
                } else {
                    out += 1;
                }
                if v == d_want {
                    ties += 1;
                }
            }
            _ => {}
        }
    }
    eprintln!(
        "normaliser: {n_cases} cases, {n_bad} mismatches; range: {r_cases} cases, {r_bad} mismatches \
         (listed {listed}, out {out}, ties {ties}, NaN V {nan_v})"
    );
    assert!(n_cases > 0 && r_cases > 0, "no vectors read");
    assert_eq!(n_bad, 0, "normaliser differs from the original");
    assert_eq!(r_bad, 0, "range test differs from the original");
}
