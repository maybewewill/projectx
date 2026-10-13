//! Unicorn vectors of the Shadow Strike slow (`tools/emu/undead_ne4_emu.py`, CHECK=decay and CHECK=apply).

use sim::undead_ne::{strike_decay_factor, strike_slow_amounts};
use softfloat::{Tables, F32};

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn f(s: &str) -> F32 {
    F32::from_bits(hex(s))
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

#[test]
fn strike_decay_factor_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: strike decay vectors skipped");
        return;
    };
    let text = include_str!("data/undead_ne/strike_decay.txt");
    let (mut live, mut zero) = (0, 0);
    for line in text.lines() {
        let h: Vec<&str> = line.split_whitespace().collect();
        let got = strike_decay_factor(hex(h[0]) & 0x80 != 0, f(h[2]), f(h[3]), f(h[1]), &t);
        assert_eq!(got.to_bits(), hex(h[4]), "row {line}");
        if got.to_f32() != 0.0 {
            live += 1;
        } else {
            zero += 1;
        }
    }
    assert!(live >= 150 && zero >= 20, "{live} {zero}");
}

#[test]
fn strike_slow_amounts_match_the_original() {
    let text = include_str!("data/undead_ne/strike_apply.txt");
    let mut n = 0;
    for line in text.lines() {
        let (head, tail) = line.split_once(" | ").expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let got = strike_slow_amounts(f(h[0]), f(h[1]), f(h[2]), f(h[3]), hex(h[4]));
        let want = format!(
            "m:{:08x} a:{:08x}:{:08x} a:{:08x}:{:08x}",
            got.move_amount.to_bits(),
            got.attack_prev.to_bits(),
            got.attack_flag,
            got.attack_new.to_bits(),
            got.attack_flag
        );
        assert_eq!(want, tail.trim(), "row {line}");
        n += 1;
    }
    assert!(n >= 300);
}
