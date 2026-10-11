//! The day cycle of Game.dll 1.26a against the original, run in Unicorn by `tools/emu/time_of_day_emu.py`
//! (`docs/spec/vision.md`, section 13). The vectors are the outputs of the original for the inputs listed; they hold no
//! game bytes.
//!
//! * `rate_matches_the_original` needs the reciprocal table of the user's own Game.dll: run with
//!   `GAME_DLL=/path/to/Game.dll cargo test -p sim --test time_of_day`. Without it the test returns early.
//! * `day_rule_matches_the_original` needs no table and always runs.

use sim::daynight::DayCycle;
use softfloat::{Tables, F32};

fn f(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// (DayLength, DayHours, scale, rate bits) as the original's setup stores them.
const RATE_VECTORS: &[(f32, f32, f32, u32)] = &[
    (240.0, 12.0, 1.0, 0x3D4C_CCCD),
    (240.0, 24.0, 0.5, 0x3D4C_CCCD),
    (37.0, 9.0, 2.5, 0x3F1B_ACFA),
    (3.0, 7.0, 0.1, 0x3E6E_EEEF),
];

/// (t, Dawn, Dusk, day flag) as the day switch computes them.
const DAY_VECTORS: &[(f32, f32, f32, bool)] = &[
    (0.0, 6.0, 18.0, false),
    (5.99, 6.0, 18.0, false),
    (6.0, 6.0, 18.0, true),
    (12.0, 6.0, 18.0, true),
    (17.99, 6.0, 18.0, true),
    (18.0, 6.0, 18.0, false),
    (23.5, 6.0, 18.0, false),
    (4.49, 4.5, 20.0, false),
    (4.5, 4.5, 20.0, true),
    (19.99, 4.5, 20.0, true),
    (20.0, 4.5, 20.0, false),
    (5.0, 20.0, 4.0, false),
    (21.0, 20.0, 4.0, false),
];

#[test]
fn rate_matches_the_original() {
    let Ok(path) = std::env::var("GAME_DLL") else {
        eprintln!("GAME_DLL not set: rate vectors skipped");
        return;
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    let tables = Tables::from_game_dll(&bytes).expect("reciprocal table in GAME_DLL");
    for &(length, hours, scale, want) in RATE_VECTORS {
        let cycle = DayCycle {
            day_length: f(length),
            day_hours: f(hours),
            ..DayCycle::default()
        };
        let got = cycle.rate(f(scale), &tables).to_bits();
        assert_eq!(
            got, want,
            "rate(length={length}, hours={hours}, scale={scale}): got {got:08X}, want {want:08X}"
        );
    }
}

#[test]
fn day_rule_matches_the_original() {
    for &(t, dawn, dusk, day) in DAY_VECTORS {
        let cycle = DayCycle {
            dawn: f(dawn),
            dusk: f(dusk),
            ..DayCycle::default()
        };
        assert_eq!(
            cycle.is_day(f(t)),
            day,
            "is_day(t={t}, dawn={dawn}, dusk={dusk})"
        );
    }
}
