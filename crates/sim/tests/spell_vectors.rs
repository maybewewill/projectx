//! The spell cost pieces against Unicorn runs of the original (`tools/emu/spells_emu.py`, docs/spec/abilities-5.md).
//! Rows hold derived results only (inputs and outputs of the original's functions as hex words).

use sim::cast::{is_hero_target, mana_check, pay_mana};
use softfloat::F32;

fn rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.split_whitespace()
                .map(|w| u32::from_str_radix(w, 16).unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn unit_mana_check_matches_the_original() {
    let rows = rows(include_str!("data/spells/unit_check.txt"));
    assert!(rows.len() >= 1000);
    for r in &rows {
        let (mana, cost, _flag, want) = (r[0], r[1], r[2], r[3]);
        let got = mana_check(F32::from_bits(mana), F32::from_bits(cost));
        assert_eq!(u32::from(got), want, "mana {mana:08x} cost {cost:08x}");
    }
}

#[test]
fn mana_payment_matches_the_original() {
    let rows = rows(include_str!("data/spells/payment.txt"));
    assert!(rows.len() >= 1000);
    for r in &rows {
        let (mana, cost, want_out, want_mana) = (r[0], r[1], r[2], r[3]);
        let (out, new_mana) = pay_mana(F32::from_bits(mana), F32::from_bits(cost));
        assert_eq!(out.to_bits(), want_out, "reported: mana {mana:08x} cost {cost:08x}");
        assert_eq!(new_mana.to_bits(), want_mana, "new mana: mana {mana:08x} cost {cost:08x}");
    }
}

#[test]
fn hero_test_matches_the_original() {
    let rows = rows(include_str!("data/spells/hero.txt"));
    assert!(rows.len() >= 1000);
    for r in &rows {
        let (first, h1, h2, want) = (r[0], r[1], r[2], r[3]);
        let id = first << 24;
        assert_eq!(
            u32::from(is_hero_target(id, h1 != 0 || h2 != 0)),
            want,
            "first byte {first:02x} helpers {h1} {h2}"
        );
    }
}
