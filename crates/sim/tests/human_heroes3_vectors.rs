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

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

#[test]
fn blizzard_wave_value_and_marker_rule_match_the_original() {
    use sim::human_heroes::{blizzard_buff_replaces, blizzard_unit_damage, BlizzardParams};
    let Some(dll) = game_dll() else {
        return;
    };
    let tables = softfloat::Tables::from_game_dll(&dll).expect("tables");
    let data = hex_rows(include_str!("data/human_heroes/wave_unit.txt"));
    assert!(data.len() >= 300);
    let (mut capped, mut buffs, mut updates, mut struct_like) = (0, 0, 0, 0);
    for r in &data {
        let f = |i: usize| F32::from_bits(r[i]);
        let n = r[0] as i32;
        let p = BlizzardParams {
            waves: 1,
            interval: F32::ONE,
            damage: f(2),
            cap: f(1),
            shards: 1,
            building: f(3),
            data_e: F32::ZERO,
            dur: F32::ZERO,
            hero_dur: F32::ZERO,
            area: F32::ZERO,
        };
        let structure_like = r[4] != 0 || r[5] != 0;
        let got = blizzard_unit_damage(&p, n, structure_like, r[6] != 0, Some(&tables));
        assert_eq!(got.to_bits(), r[9], "n {n} cap {:08x} d {:08x} building {:08x} row {:08x?}", r[1], r[2], r[3], r);
        assert_eq!(r[11], 1, "the effect's slot 0x84 is called once");
        let has_buff = r[7] != 0;
        let replaced = has_buff && blizzard_buff_replaces(f(8), got);
        assert_eq!(u32::from(replaced), r[13], "refresh flag, row {:08x?}", r);
        let stored = if replaced { got.to_bits() } else { r[8] };
        if has_buff {
            assert_eq!(stored, r[12], "stored value, row {:08x?}", r);
            buffs += 1;
            updates += u32::from(replaced);
        }
        capped += u32::from(got.to_bits() != (if r[6] != 0 { r[2] ^ 0x8000_0000 } else { r[2] }) && !structure_like);
        struct_like += u32::from(structure_like);
    }
    assert!(capped > 20 && buffs > 100 && updates > 20 && struct_like > 50, "{capped} {buffs} {updates} {struct_like}");
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
