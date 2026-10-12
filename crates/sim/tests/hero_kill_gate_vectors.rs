//! The kill experience gates (`sim::kill_xp::{structure_blocks, mask_has}`) against the original routines `0x6F26DE50`,
//! `0x6F3A3400` and `0x6F3A34B0`, run in Unicorn by `tools/emu/hero_kill_gate_emu.py` (which also checks that the mask
//! reads ask for `ext+0x38` / `ext+0x68`). The vector file is not committed.
//!
//! Run: `HERO_GATE_VECTORS=gate.txt cargo test -p sim --test hero_kill_gate_vectors -- --nocapture`.

use sim::kill_xp::{mask_has, structure_blocks};

#[test]
fn the_gates_match_the_original() {
    let Ok(path) = std::env::var("HERO_GATE_VECTORS") else {
        eprintln!("HERO_GATE_VECTORS not set: skipping");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let (mut cases, mut mismatches) = (0usize, 0usize);
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        cases += 1;
        let ok = match f[0] {
            "FLAG" => {
                let flags = u32::from_str_radix(f[1], 16).unwrap();
                let want: u32 = f[2].parse().unwrap();
                // The builder stops when the original answers non-zero and BuildingKillsGiveExp is off.
                structure_blocks(flags, false) == (want != 0) && !structure_blocks(flags, true)
            }
            _ => {
                let mask = u32::from_str_radix(f[1], 16).unwrap();
                let bit: u32 = f[2].parse().unwrap();
                let want: u32 = f[3].parse().unwrap();
                mask_has(mask, bit) == (want != 0)
            }
        };
        if !ok {
            mismatches += 1;
            eprintln!("MISMATCH {line}");
        }
    }
    println!("kill gates: {cases} cases, {mismatches} mismatches");
    assert_eq!(mismatches, 0);
}
