//! Bladestorm set-up against the original (Unicorn vectors from `tools/emu/bs_emu.py`, `tests/data/orc/bladestorm.txt`):
//! the real class initialiser `0x6F163570` ran on a fake ability with a real level record. Needs `GAME_DLL` (soft-float
//! tables); without it the test skips.

use sim::orc::*;
use softfloat::{Tables, F32};

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn ticks_damage_and_radius_match_the_original() {
    let Ok(path) = std::env::var("GAME_DLL") else {
        return;
    };
    let dll = std::fs::read(&path).unwrap();
    let t = Tables::from_game_dll(&dll).expect("tables");
    let mut n = 0;
    for line in include_str!("data/orc/bladestorm.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w[0] == "K" {
            assert_eq!(h(w[1]), BLADESTORM_TICK_RATE_BITS);
            continue;
        }
        // B Dur HeroDur Area DataA x9c | ticks dmg radius
        let total = bladestorm_total(f(w[1]), f(w[5]));
        let ticks = bladestorm_ticks(total);
        assert_eq!(ticks as i64, w[7].parse::<i32>().unwrap() as i64, "{line}");
        assert_eq!(bladestorm_tick_damage(f(w[4]), total, ticks, &t).to_bits(), h(w[8]), "{line}");
        assert_eq!(h(w[9]), h(w[3]), "{line}: the radius cell is Area");
        n += 1;
    }
    assert!(n >= 300, "{n}");
}
