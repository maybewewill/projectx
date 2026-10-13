//! Vectors from the original `Game.dll` for the channel lifecycle and the Blizzard channel time
//! (`tools/emu/human_heroes3_emu.py`, 400 cases per check, 0 mismatches). Only derived results are stored.

use sim::cast::{prop_above, prop_differs, SpellLevel};
use sim::human_heroes::blizzard_channel_time;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

#[test]
fn blizzard_channel_time_matches_the_original() {
    let data = hex_rows(include_str!("data/human_heroes/channel_time.txt"));
    assert!(data.len() >= 300);
    for r in &data {
        let level = SpellLevel {
            cast: F32::from_bits(r[0]),
            data: [F32::from_bits(r[1]), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        assert_eq!(blizzard_channel_time(&level).to_bits(), r[2], "cast {:08x} DataA {:08x}", r[0], r[1]);
    }
}

#[test]
fn channel_property_compares_match_the_original() {
    let text = include_str!("data/human_heroes/prop_compare.txt");
    let mut n = 0;
    let (mut above, mut differs) = (0, 0);
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let w: Vec<&str> = l.split_whitespace().collect();
        let p = F32::from_bits(u32::from_str_radix(w[0], 16).unwrap());
        let c = F32::from_bits(u32::from_str_radix(w[1], 16).unwrap());
        let a: u32 = w[2].parse().unwrap();
        let b: u32 = w[3].parse().unwrap();
        assert_eq!(u32::from(prop_above(p, c)), a, "above {:08x} {:08x}", p.to_bits(), c.to_bits());
        assert_eq!(u32::from(prop_differs(p, c)), b, "differs {:08x} {:08x}", p.to_bits(), c.to_bits());
        above += a;
        differs += b;
        n += 1;
    }
    assert!(n >= 300);
    assert!(above > 20 && above < n && differs > 20 && differs < n + 1);
}
