//! Hero revive cost and time against the original (Unicorn vectors of `tools/emu/revive_cost_emu.py`).
//! Needs `GAME_DLL` for the reciprocal table; without it the test returns early.

use sim::revive::{cost, time, ReviveConfig};
use softfloat::{Tables, F32};

mod revive_data {
    include!("revive_vectors/data.rs");
}

#[test]
fn revive_cost_and_time_match_the_original() {
    let Ok(path) = std::env::var("GAME_DLL") else {
        eprintln!("GAME_DLL not set: skipped");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let tb = Tables::from_game_dll(&bytes).expect("tables");
    let f = F32::from_bits;
    let mut bad = 0;
    for (i, &(level, tg, tl, bt, r, ints, eg, el, et)) in revive_data::REVIVE.iter().enumerate() {
        let cfg = ReviveConfig {
            base_factor: f(r[0]),
            level_factor: f(r[1]),
            max_factor: f(r[2]),
            base_lumber_factor: f(r[3]),
            lumber_level_factor: f(r[4]),
            time_factor: f(r[5]),
            max_time_factor: f(r[6]),
            max_time: f(r[7]),
            max_cost_gold: ints[0] as u32,
            max_cost_lumber: ints[1] as u32,
            ..ReviveConfig::default()
        };
        let (g, l) = cost(&cfg, level, tg, tl, &tb);
        let t = time(&cfg, level, bt, &tb).to_bits();
        if (g, l, t) != (eg as u32, el as u32, et) {
            bad += 1;
            if bad < 8 {
                eprintln!("case {i}: got ({g}, {l}, {t:#x}) want ({eg}, {el}, {et:#x}) level {level}");
            }
        }
    }
    assert_eq!(bad, 0, "mismatches");
}
