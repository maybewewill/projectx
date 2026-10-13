//! Hex creature pick against the original (Unicorn vectors from `tools/emu/hex_emu.py`, `tests/data/orc/hex.txt`): the
//! real selector `0x6F12DAA0` with the real roll wrapper on combat stream 24. Needs `GAME_DLL` (the RNG table).

use std::collections::BTreeMap;

use sim::orc::*;
use sim::rng::{Rng, RngTable};

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn creature_pick_matches_the_original() {
    let Some(dll) = game_dll() else {
        return;
    };
    let table = RngTable::from_game_dll(&dll).expect("rng table");
    let (mut n, mut picked, mut none) = (0, 0, 0);
    for line in include_str!("data/orc/hex.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let (state, result) = tail.split_once('|').unwrap();
        let w: Vec<&str> = head.split_whitespace().collect();
        let class: u32 = w[1].parse().unwrap();
        let mut texts: BTreeMap<u32, String> = BTreeMap::new();
        let (text_field, exists_field) = if w[2].contains('=') || w[2] == "-" { (w[2], w[3]) } else { ("-", w[2]) };
        if text_field != "-" {
            for part in text_field.split(';') {
                let (k, v) = part.split_once('=').unwrap();
                let v = if v == "<empty>" { String::new() } else { v.replace('~', " ").replace('^', "	") };
                texts.insert(k.parse().unwrap(), v);
            }
        }
        let exists: Vec<u32> = exists_field.split(',').map(h).collect();
        let s: Vec<&str> = state.split_whitespace().collect();
        let mut rng = Rng { a: h(s[0]), b: h(s[1]) };
        let r: Vec<&str> = result.split_whitespace().collect();
        let got = hex_pick(&mut rng, &table, class, texts.get(&class).map(String::as_str), |c| exists.contains(&c));
        assert_eq!(got.unwrap_or(0), h(r[0]), "{line}");
        assert_eq!((rng.a, rng.b), (h(r[1]), h(r[2])), "{line}");
        if got.is_some() {
            picked += 1;
        } else {
            none += 1;
        }
        n += 1;
    }
    assert!(n >= 300 && picked > 100 && none > 5, "{n} {picked} {none}");
}

#[test]
fn class_tables() {
    assert_eq!(HEX_STREAM, 24);
    assert_eq!(polymorph_data_column(2), 2);
    assert_eq!(polymorph_data_column(0x10), 4);
    assert_eq!(polymorph_data_column(0x20), 3);
    assert_eq!(polymorph_data_column(1), 1);
    assert_eq!(polymorph_tokens("a-b_c, nfro,"), vec![0x61, 0x62, 0x63, 0x6E66726F]);
}
