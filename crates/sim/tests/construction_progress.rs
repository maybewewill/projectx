//! Vector test for the construction percent setter (`0x6F054690`, see `docs/spec/construction.md` section 5).
//!
//! The vectors in `tests/vectors/construction_progress.tsv` were recorded by running the original handler in
//! Unicorn (`tools/emu/construction_progress_emu.py --vectors`). Each line is `factor bits, p, life bits,
//! completion flag, tick flag`. The test needs the reciprocal table of the user's `Game.dll` and returns early
//! without `GAME_DLL`.

use sim::construction::{percent_life, remaining_after_percent, PROGRESS_COMPLETE};
use softfloat::{Tables, F32};

const VECTORS: &str = include_str!("vectors/construction_progress.tsv");
const BASE_VECTORS: &str = include_str!("vectors/base_progress.tsv");

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

#[test]
fn percent_setter_matches_the_original() {
    let Some(t) = tables() else {
        return;
    };
    let mut n = 0usize;
    for line in VECTORS.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let factor = u32::from_str_radix(f[0], 16).unwrap();
        let p: i32 = f[1].parse().unwrap();
        let life = u32::from_str_radix(f[2], 16).unwrap();
        let got = percent_life(F32(factor), p, &t);
        assert_eq!(got.0, life, "factor {factor:08x} p {p}");
        // The completion and tick flags are the handler's branch on p (checked in the vector file).
        let complete: bool = f[3] == "1";
        assert_eq!(complete, p == PROGRESS_COMPLETE, "completion branch at p {p}");
        n += 1;
    }
    assert!(n > 1000, "vector file too small: {n}");
}

#[test]
fn base_percent_setter_matches_the_original() {
    let Some(t) = tables() else {
        return;
    };
    let mut n = 0usize;
    for line in BASE_VECTORS.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let total = u32::from_str_radix(f[0], 16).unwrap();
        // The vector file writes the signed argument; the setter sees it as a 32-bit value.
        let p: i32 = f[1].parse().unwrap();
        let written = u32::from_str_radix(f[2], 16).unwrap();
        let got = remaining_after_percent(F32(total), p, &t);
        assert_eq!(got.0, written, "total {total:08x} p {p}");
        n += 1;
    }
    assert!(n > 6000, "vector file too small: {n}");
}
