//! The derived attribute bonus readers (`sim::heroes::derived_bonuses`, `docs/spec/heroes.md` section 9) against the
//! original `Game.dll` readers, run in Unicorn by `tools/emu/hero_bonus_emu.py`. The vector file holds synthetic hero
//! values, Misc reals and the original's results and is not committed.
//!
//! Run: `HERO_BONUS_VECTORS=/path/to/vectors.txt cargo test -p sim --test hero_bonus_vectors -- --nocapture`.
//! Without the variable the test returns early.
//!
//! Line format (one reader per line): name, primary `cc`, strength base and gain bits, agility base and gain bits,
//! the intelligence base counter (unused field), the intelligence gain bits, the level counter, the intelligence base
//! counter, eight Misc reals (bits), the factor bits (HP only), out1 bits, out2 bits, return value.

use sim::heroes::{derived_bonuses, BonusRates};
use sim::natives::game::HeroObj;
use softfloat::F32;

fn bits(s: &str) -> F32 {
    F32(u32::from_str_radix(s, 16).expect("hex bits"))
}

#[test]
fn readers_match_the_original_in_unicorn() {
    let Ok(path) = std::env::var("HERO_BONUS_VECTORS") else {
        eprintln!("HERO_BONUS_VECTORS not set: skipping the derived bonus vectors");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let mut cases = 0usize;
    let mut mismatches = 0usize;
    let mut per_reader = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 22, "fields in {line:?}");
        let name = f[0];
        let cc: i32 = f[1].parse().expect("cc");
        let base_str: i32 = f[2].parse().expect("base str");
        let gs = bits(f[3]);
        let base_agi: i32 = f[4].parse().expect("base agi");
        let ga = bits(f[5]);
        let gi = bits(f[7]);
        let counter: i32 = f[8].parse().expect("counter");
        let base_int_counter: i32 = f[9].parse().expect("int base counter");
        let misc: Vec<F32> = f[10..18].iter().map(|s| bits(s)).collect();
        let factor = bits(f[18]);
        let o1 = bits(f[19]);
        let o2 = bits(f[20]);
        let ret = u32::from_str_radix(f[21], 16).expect("return");
        assert_eq!(ret, 0x0020_3000, "the readers return their first pointer ({line:?})");

        let h = HeroObj {
            level: counter + 1,
            str_base: base_str,
            str_plus: gs,
            agi_base: base_agi,
            agi_plus: ga,
            int_base: base_int_counter,
            int_plus: gi,
            primary: cc,
            ..HeroObj::default()
        };
        let rates = BonusRates {
            str_hit_point: misc[0],
            str_regen: misc[1],
            str_attack: misc[2],
            agi_defense_base: misc[3],
            agi_defense: misc[4],
            agi_move: misc[5],
            agi_attack_speed: misc[6],
            int_mana: misc[7],
        };
        let d = derived_bonuses(&h, &rates, factor);
        let got = match name {
            "HP" => d.hit_points,
            "REGEN" => d.regen,
            "STRATK" => d.attack,
            "ARMOR" => d.armor,
            "MOVE" => d.move_speed,
            "ASPD" => d.attack_speed,
            "MANA" => d.mana,
            other => panic!("unknown reader {other}"),
        };
        // Only the first pointer is written; the HP factor pointer keeps its value (the harness writes the factor
        // there for every reader, so the other readers' second pointer is not checked).
        let second_ok = name != "HP" || o2 == factor;
        cases += 1;
        let entry = per_reader.entry(name.to_string()).or_default();
        entry.0 += 1;
        if got != o1 || !second_ok {
            mismatches += 1;
            entry.1 += 1;
            if mismatches <= 20 {
                eprintln!("MISMATCH {line}: got {:08x}", got.0);
            }
        }
    }
    for (name, (n, bad)) in &per_reader {
        println!("  {name}: {n} cases, {bad} mismatches");
    }
    println!("hero derived bonus readers: {cases} cases, {mismatches} mismatches");
    assert_eq!(mismatches, 0);
}
