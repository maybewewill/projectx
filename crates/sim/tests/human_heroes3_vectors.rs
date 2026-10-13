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
fn target_flag_normaliser_matches_the_original() {
    use sim::human_heroes::targs_normalise;
    let data = hex_rows(include_str!("data/human_heroes/targs_normalise.txt"));
    assert!(data.len() >= 300);
    for r in &data {
        assert_eq!(targs_normalise(r[0]), r[1], "word {:08x}", r[0]);
    }
    // The two words the Human heroes use: Blizzard's `_` and Mass Teleport's group filter.
    assert_eq!(targs_normalise(0), 0xFDD1_F00E);
    assert_eq!(targs_normalise(0x4030_3006), 0x7DF0_3006);
}

#[test]
fn dispel_walker_selection_matches_the_original() {
    use sim::human_heroes::{dispel_selects, DispelArgs, DispelBuff, MAGIC_IMMUNITY_DISPEL};
    let text = include_str!("data/human_heroes/dispel.txt");
    let (mut rows, mut removed_total, mut ds_rows) = (0, 0, 0);
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Vec<u32> = l.split_whitespace().map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
        let a = DispelArgs {
            physical: v[0] != 0,
            magic: v[1] != 0,
            undispellable_ok: v[2] != 0,
            aura_ok: v[3] != 0,
            typed: v[4] != 0,
            positive: v[5] != 0,
            negative: v[6] != 0,
            type_arg: v[7] != 0,
            timed_ok: v[8] != 0,
        };
        if a == MAGIC_IMMUNITY_DISPEL {
            ds_rows += 1;
        }
        let n = v[9] as usize;
        let mut mask = 0u32;
        for i in 0..n {
            let b = v[10 + i];
            let buff = DispelBuff {
                magic: b & 1 != 0,
                physical: b & 2 != 0,
                negative: b & 4 != 0,
                positive: b & 8 != 0,
                aura: b & 0x10 != 0,
                undispellable: b & 0x20 != 0,
                typed: b & 0x40 != 0,
                timed: b & 0x80 != 0,
            };
            if dispel_selects(&a, &buff) {
                mask |= 1 << i;
            }
        }
        assert_eq!(mask, v[15], "removed mask, row {l}");
        assert_eq!(mask.count_ones(), v[16], "returned count, row {l}");
        rows += 1;
        removed_total += mask.count_ones();
    }
    assert!(rows >= 300 && ds_rows > 40 && removed_total > 50, "{rows} {ds_rows} {removed_total}");
}

#[test]
fn banish_values_match_the_original() {
    let data = hex_rows(include_str!("data/human_heroes/banish.txt"));
    assert!(data.len() >= 300);
    let (mut heroes, mut others) = (0, 0);
    for r in &data {
        let level = SpellLevel {
            dur: F32::from_bits(r[0]),
            hero_dur: F32::from_bits(r[1]),
            data: [F32::from_bits(r[2]), F32::from_bits(r[3]), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let (dur, mv, atk) = sim::human_heroes::banish_values(&level, r[5] != 0, F32::from_bits(r[4]));
        assert_eq!([dur.to_bits(), mv.to_bits(), atk.to_bits()], [r[6], r[7], r[8]], "row {:08x?}", r);
        if r[5] != 0 {
            heroes += 1;
        } else {
            others += 1;
        }
    }
    assert!(heroes > 100 && others > 100);
}

#[test]
fn thunder_clap_slow_values_match_the_original() {
    let data = hex_rows(include_str!("data/human_heroes/thunder_slow.txt"));
    assert!(data.len() >= 300);
    let (mut applied, mut refused) = (0, 0);
    for r in &data {
        let level = SpellLevel {
            dur: F32::from_bits(r[0]),
            hero_dur: F32::from_bits(r[1]),
            data: [F32::ZERO, F32::ZERO, F32::from_bits(r[2]), F32::from_bits(r[3]), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let got = sim::human_heroes::thunder_clap_slow(&level, r[5] != 0, F32::from_bits(r[4]));
        assert_eq!(u32::from(got.is_some()), r[6], "applied, row {:08x?}", r);
        if let Some((d, m, a)) = got {
            assert_eq!([d.to_bits(), m.to_bits(), a.to_bits()], [r[7], r[8], r[9]], "row {:08x?}", r);
            applied += 1;
        } else {
            refused += 1;
        }
    }
    assert!(applied > 200 && refused > 20);
}

#[test]
fn corpse_distance_matches_the_original() {
    use softfloat::arith;
    let data = hex_rows(include_str!("data/human_heroes/corpse_distance.txt"));
    assert!(data.len() >= 300);
    let f = |x: u32| F32::from_bits(x);
    for r in &data {
        // out = A - B per component (`0x6F020210`), then x*x + y*y (`0x6F020000`), the corpse record's real `+8`.
        let d: Vec<F32> = (0..3).map(|i| arith::sub(f(r[i]), f(r[3 + i]))).collect();
        for i in 0..3 {
            assert_eq!(d[i].to_bits(), r[6 + i], "component {i}, row {:08x?}", r);
        }
        let sq = arith::add(arith::mul(d[0], d[0]), arith::mul(d[1], d[1]));
        assert_eq!(sq.to_bits(), r[9], "squared length, row {:08x?}", r);
    }
}

#[test]
fn mass_teleport_channel_time_is_data_b() {
    let data = hex_rows(include_str!("data/human_heroes/mt_init.txt"));
    assert!(data.len() >= 300);
    for r in &data {
        let level = SpellLevel {
            data: [F32::ZERO, F32::from_bits(r[0]), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let t = sim::human_heroes::channel_time_override(sim::human_heroes::MASS_TELEPORT, &level);
        assert_eq!(t.map(F32::to_bits), Some(r[1]), "DataB {:08x}", r[0]);
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
