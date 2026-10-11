//! The intelligence getter (`sim::heroes::intelligence`) against the original `Game.dll` core `0x6F0DB3A0`, run in
//! Unicorn by `tools/emu/hero_int_emu.py`. The vector file holds synthetic values and the original's results and is
//! not committed.
//!
//! Run: `HERO_INT_VECTORS=/path/to/vectors.txt cargo test -p sim --test hero_int_vectors -- --nocapture`.
//! Without the variable the test returns early.

use sim::heroes::intelligence;
use softfloat::F32;

#[test]
fn intelligence_matches_the_original_in_unicorn() {
    let Ok(path) = std::env::var("HERO_INT_VECTORS") else {
        eprintln!("HERO_INT_VECTORS not set: skipping the hero intelligence vectors");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let mut cases = 0usize;
    let mut mismatches = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 4, "fields in {line:?}");
        let base: i32 = f[0].parse().expect("base counter");
        let level: i32 = f[1].parse().expect("level counter");
        let gain = F32(u32::from_str_radix(f[2], 16).expect("gain bits"));
        let want: i32 = f[3].parse().expect("result");
        let got = intelligence(base, gain, level);
        cases += 1;
        if got != want {
            mismatches += 1;
            eprintln!("MISMATCH {line}: got {got}");
        }
    }
    println!("hero intelligence getter: {cases} cases, {mismatches} mismatches");
    assert_eq!(mismatches, 0);
}
