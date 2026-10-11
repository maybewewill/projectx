//! The strength and agility getters (`sim::heroes::attribute`) against the original `Game.dll` getters, run in Unicorn
//! by `tools/emu/hero_stat_emu.py`. The vector file holds synthetic values and the original's results and is not
//! committed.
//!
//! Run: `HERO_STAT_VECTORS=/path/to/vectors.txt cargo test -p sim --test hero_stat_vectors -- --nocapture`.
//! Without the variable the test returns early.

use sim::heroes::attribute;
use softfloat::F32;

#[test]
fn getters_match_the_original_in_unicorn() {
    let Ok(path) = std::env::var("HERO_STAT_VECTORS") else {
        eprintln!("HERO_STAT_VECTORS not set: skipping the hero attribute vectors");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let mut cases = 0usize;
    let mut mismatches = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 5, "fields in {line:?}");
        let base: i32 = f[1].parse().expect("base");
        let gain = F32(u32::from_str_radix(f[2], 16).expect("gain bits"));
        let counter: i32 = f[3].parse().expect("counter");
        let want: i32 = f[4].parse().expect("result");
        let got = attribute(base, gain, counter);
        cases += 1;
        if got != want {
            mismatches += 1;
            eprintln!("MISMATCH {line}: got {got}");
        }
    }
    println!("hero attribute getters: {cases} cases, {mismatches} mismatches");
    assert_eq!(mismatches, 0);
}
