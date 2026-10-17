//! Vectors from the original `Game.dll` for the V9 verification wave (`tools/emu/v9_emu.py`, docs/spec/abilities-v9.md).
//! The original effect and buff routines run on fake objects; only derived results are stored.

use sim::cast::SpellLevel;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

fn f(x: u32) -> F32 {
    F32::from_bits(x)
}

fn level(dur: u32, hdur: u32, data: &[u32]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel { dur: f(dur), hero_dur: f(hdur), data: d, ..SpellLevel::default() }
}

#[test]
fn frenzy_self_effect_matches_the_original() {
    // dur herodur A B C extra hero | slot-0x320 arg1 arg2 | movement value, calls | attack speed value, calls
    let rows = hex_rows(include_str!("data/v9/frenzy.txt"));
    assert!(rows.len() >= 300);
    let (mut heroes, mut others) = (0, 0);
    for r in &rows {
        let l = level(r[0], r[1], &[r[2], r[3], r[4]]);
        let hero = r[6] != 0;
        let (dur, mv, atk) = sim::unit_abilities_a::frenzy_values(&l, hero, f(r[5]));
        assert_eq!(r[8], dur.to_bits(), "duration, row {:08x?}", r);
        assert_eq!((r[9], r[10]), (mv.to_bits(), 1), "movement sink, row {:08x?}", r);
        assert_eq!((r[11], r[12]), (atk.to_bits(), 1), "attack speed sink, row {:08x?}", r);
        if hero {
            heroes += 1;
        } else {
            others += 1;
        }
    }
    assert!(heroes > 100 && others > 100);
}
