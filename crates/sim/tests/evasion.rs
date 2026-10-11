//! Tests of the evasion decision and the chance table (`sim::damage::evades`, `sim::damage::evasion_chance`,
//! `docs/spec/attack-damage.md` section 12). Synthetic cases only; the same rules were checked against the original in
//! Unicorn (`tools/emu/evasion_emu.py`: 300 decision cases, 100 table cases, 0 mismatches).

use sim::damage::{evades, evasion_chance, EVASION_STREAM};
use softfloat::F32;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

#[test]
fn evasion_uses_combat_stream_two() {
    assert_eq!(EVASION_STREAM, 2);
}

#[test]
fn evades_only_when_chance_is_strictly_above_the_draw() {
    assert!(evades(f(0.5), f(0.25)));
    assert!(evades(f(1.0), f(0.999_999)));
    assert!(!evades(f(0.25), f(0.5)));
    // equal values do not evade (the compare is strict)
    assert!(!evades(f(0.5), f(0.5)));
    assert!(!evades(f(0.0), f(0.0)));
}

#[test]
fn zero_chance_never_evades_and_a_certain_chance_always_does_for_draws_below_one() {
    for k in 0..1000u32 {
        let draw = f(k as f32 / 1000.0); // draws in [0, 0.999]
        assert!(!evades(F32::ZERO, draw), "chance 0 evaded on draw {k}");
        assert!(evades(f(1.0), draw), "chance 1 missed on draw {k}");
    }
}

#[test]
fn unordered_compare_evades() {
    // NaN on either side is unordered; the original's `fcom` sets the parity flag and the routine evades
    assert!(evades(f(f32::NAN), f(0.5)));
    assert!(evades(f(0.5), f(f32::NAN)));
}

#[test]
fn chance_table_reads_the_level_and_gives_zero_outside_it() {
    let table = [f(0.1), f(0.2), f(0.3)];
    assert_eq!(evasion_chance(&table, 0).to_bits(), f(0.1).to_bits());
    assert_eq!(evasion_chance(&table, 2).to_bits(), f(0.3).to_bits());
    assert_eq!(evasion_chance(&table, 3).to_bits(), F32::ZERO.to_bits());
    assert_eq!(evasion_chance(&table, -1).to_bits(), F32::ZERO.to_bits());
    // a definition without its data has an empty table
    assert_eq!(evasion_chance(&[], 0).to_bits(), F32::ZERO.to_bits());
}
