//! Vectors from the original `Game.dll` for Bash's roll (`tools/emu/human_heroes_emu.py`, 400 cases per check, 0
//! mismatches against the model). Only derived results are stored. The distribution constant needs the soft-float
//! tables and the draw needs the RNG table, so those two checks run with `GAME_DLL` and return early without it.

use sim::human_heroes::{bash_procs, prd_effective_chance};
use sim::rng::{Rng, RngTable};
use softfloat::{arith, Tables, F32};

fn rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.split_whitespace()
                .map(|w| {
                    if w.len() == 8 {
                        u32::from_str_radix(w, 16).expect("hex")
                    } else {
                        w.parse::<u32>().expect("decimal")
                    }
                })
                .collect()
        })
        .collect()
}

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

#[test]
fn effective_chance_matches_the_original() {
    let Some(dll) = game_dll() else {
        return;
    };
    let tables = Tables::from_game_dll(&dll).expect("tables");
    let data = rows(include_str!("data/human_heroes/chance.txt"));
    assert!(data.len() >= 300);
    let mut bad = Vec::new();
    for r in &data {
        let got = prd_effective_chance(F32::from_bits(r[0]), r[1] as i32, &tables);
        if got.to_bits() != r[2] {
            bad.push(format!("chance {:08x} count {} got {:08x} want {:08x}", r[0], r[1] as i32, got.to_bits(), r[2]));
        }
    }
    assert!(bad.is_empty(), "{} mismatches, first: {:#?}", bad.len(), &bad[..bad.len().min(12)]);
}

#[test]
fn proc_compare_matches_the_original() {
    let data = rows(include_str!("data/human_heroes/compare.txt"));
    assert!(data.len() >= 300);
    let mut procs = 0;
    for r in &data {
        let got = bash_procs(F32::from_bits(r[0]), F32::from_bits(r[1]));
        assert_eq!(u32::from(got), r[2], "draw {:08x} chance {:08x}", r[0], r[1]);
        procs += r[2];
    }
    assert!(procs > 50 && procs < data.len() as u32, "both outcomes occur ({procs})");
}

#[test]
fn stream_three_draw_matches_the_original() {
    let Some(dll) = game_dll() else {
        return;
    };
    let table = RngTable::from_game_dll(&dll).expect("RNG table");
    let data = rows(include_str!("data/human_heroes/draw.txt"));
    assert!(data.len() >= 300);
    for r in &data {
        let mut rng = Rng { a: r[0], b: r[1] };
        let u = rng.random_fraction(&table);
        let got = arith::add(F32::ZERO, arith::mul(F32::ONE, u));
        assert_eq!(got.to_bits(), r[2], "state {:08x} {:08x}", r[0], r[1]);
    }
}
