//! The `ChanceToMiss` tail of the miss gate (`sim::missiles::chance_to_miss`, `miss_cell`) against the Unicorn run of
//! the real gate `0x6F2BEAC0` of Game.dll 1.26a (`tools/emu/miss_gate_emu.py`: 1,200 cases without evasion entries
//! plus 1,200 mixed cases with random evasion lists, 0 mismatches against its Python model). The vectors are embedded,
//! so this test needs no `GAME_DLL`. The terrain level of the harness is a fixed asymmetric stand-in of the two
//! integer cell arguments (the real level function needs a loaded map); the test uses the same stand-in.

#[path = "missile/miss_gate_data.rs"]
mod miss_gate_data;

use sim::missiles::{chance_to_miss, miss_cell};
use softfloat::F32;

/// The harness' level stand-in (`level_fn` in the emulator script).
fn level(a: i32, b: i32) -> i32 {
    let (a, b) = (i64::from(a), i64::from(b));
    ((a * 7 + b * 3 + (a ^ b)).rem_euclid(6)) as i32
}

#[test]
fn chance_to_miss_tail_matches_the_original() {
    let mut missed = 0usize;
    let mut tail_reached = 0usize;
    for (i, &(r, m, exempt, ox, oy, a, t, result)) in miss_gate_data::ROWS.iter().enumerate() {
        let (ox, oy) = (F32::from_bits(ox), F32::from_bits(oy));
        let cell = |p: [u32; 3]| {
            (
                miss_cell(F32::from_bits(p[0]), ox),
                miss_cell(F32::from_bits(p[1]), oy),
            )
        };
        let (ca, ct) = (cell(a), cell(t));
        let mut reached = false;
        let got = chance_to_miss(F32::from_bits(m), F32::from_bits(r), exempt, || {
            reached = true;
            (level(ca.0, ca.1), level(ct.0, ct.1))
        });
        assert_eq!(got, result, "row {i}");
        tail_reached += usize::from(reached);
        missed += usize::from(got);
    }
    // Coverage of the vectors: both outcomes, and the level compare actually reached.
    assert!(missed > 100, "missed {missed}");
    assert!(tail_reached > 400, "tail reached {tail_reached}");
}

#[test]
fn unordered_chance_or_draw_misses_before_the_level_rule() {
    let nan = F32::from_bits(0x7FC0_0000);
    let half = F32::from_bits(0x3F00_0000);
    // `chance <= draw` is false for NaN on either side, so the gate goes on to the levels.
    assert!(chance_to_miss(nan, half, false, || (0, 1)));
    assert!(chance_to_miss(half, nan, false, || (0, 1)));
    // Equal values: ordered `<=` holds, no miss.
    assert!(!chance_to_miss(half, half, false, || (0, 1)));
    // Same level: no miss; a flying attacker never misses.
    assert!(!chance_to_miss(half, F32::ZERO, false, || (1, 1)));
    assert!(!chance_to_miss(half, F32::ZERO, true, || (0, 1)));
}
