//! Vectors from the original `Game.dll` for Blizzard and Mass Teleport (`tools/emu/human_heroes2_emu.py`, 400 cases per
//! check, 0 mismatches). Only derived results are stored. The shard placement needs the soft-float and RNG tables, so
//! it runs with `GAME_DLL` and returns early without it.

use sim::cast::SpellLevel;
use sim::human_heroes::{
    blizzard_params, blizzard_raw_counts, blizzard_shards, resurrection_compare, teleport_compare, teleport_select,
    ResurrectionRec, TeleportRec,
};
use sim::natives::game::ObjKey;
use sim::rng::{Rng, RngTable};
use softfloat::{Tables, F32};

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

#[test]
fn blizzard_arguments_match_the_original() {
    let data = hex_rows(include_str!("data/human_heroes/blizzard_args.txt"));
    assert!(data.len() >= 300);
    for r in &data {
        let f = |i: usize| F32::from_bits(r[i]);
        let level = SpellLevel {
            cast: f(0),
            dur: f(1),
            hero_dur: f(2),
            area: f(9),
            data: [f(3), f(4), f(5), f(6), f(7), f(8), F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let p = blizzard_params(&level);
        let (waves, shards) = blizzard_raw_counts(&level);
        let got = [
            p.area.to_bits(),
            waves as u32,
            p.interval.to_bits(),
            p.damage.to_bits(),
            p.cap.to_bits(),
            shards as u32,
            p.building.to_bits(),
            p.data_e.to_bits(),
            p.dur.to_bits(),
            p.hero_dur.to_bits(),
        ];
        assert_eq!(&got[..], &r[10..20], "level {:08x?}", &r[..10]);
        assert_eq!(p.waves, waves.max(1));
        assert_eq!(p.shards, shards.clamp(1, 32));
    }
}

#[test]
fn blizzard_shard_placement_matches_the_original() {
    let Some(dll) = game_dll() else {
        return;
    };
    let tables = Tables::from_game_dll(&dll).expect("tables");
    let table = RngTable::from_game_dll(&dll).expect("RNG table");
    let data = hex_rows(include_str!("data/human_heroes/blizzard_place.txt"));
    assert!(data.len() >= 300);
    let mut big = 0;
    for r in &data {
        let n = r[4] as usize;
        let mut global = Rng { a: r[0], b: r[1] };
        let mut s4 = Rng { a: r[2], b: r[3] };
        let wave = blizzard_shards(n, F32::from_bits(r[5]), &mut global, &mut s4, &table, &tables);
        assert_eq!((global.a, global.b, s4.a, s4.b), (r[6], r[7], r[8], r[9]), "final states, n {n}");
        let base = 10;
        let perm: Vec<u32> = r[base..base + n].to_vec();
        assert_eq!(wave.perm, perm, "permutation, n {n}");
        for i in 0..n {
            let x = r[base + n + i];
            let y = r[base + 2 * n + i];
            assert_eq!((wave.offsets[i].0.to_bits(), wave.offsets[i].1.to_bits()), (x, y), "shard {i} of {n}");
        }
        if n > 5 {
            big += 1;
        }
    }
    assert!(big > 100);
}

#[test]
fn teleport_comparator_matches_the_original() {
    let data = include_str!("data/human_heroes/mt_compare.txt");
    let mut signs = [0usize; 3];
    let mut n = 0;
    for l in data.lines().filter(|l| !l.trim().is_empty()) {
        let v: Vec<i32> = l.split_whitespace().map(|w| w.parse().unwrap()).collect();
        let rec = |o: usize| TeleportRec {
            dist: v[o],
            key: v[o + 1],
            hero: v[o + 2],
            own: v[o + 3],
            id: v[o + 4],
        };
        let got = teleport_compare(&rec(0), &rec(5)).signum();
        assert_eq!(got, v[10], "{v:?}");
        signs[(got + 1) as usize] += 1;
        n += 1;
    }
    assert!(n >= 300);
    assert!(signs[0] > 50 && signs[2] > 50, "both orders occur {signs:?}");
}

#[test]
fn teleport_selection_keeps_own_heroes_then_nearest() {
    let rec = |dist, key, hero, own, id| TeleportRec { dist, key, hero, own, id };
    let list = vec![
        (ObjKey(1), rec(10, 0, 0, 0, 1)),
        (ObjKey(2), rec(50, 0, 0, 1, 2)),
        (ObjKey(3), rec(90, 5, 1, 1, 3)),
        (ObjKey(4), rec(20, 0, 0, 1, 4)),
    ];
    // DataA = 3: the limit is 2; own units first (3 is a hero), then the nearest own unit.
    let keep = teleport_select(list.clone(), 3);
    assert_eq!(keep.iter().map(|(k, _)| k.0).collect::<Vec<_>>(), vec![3, 4]);
    // DataA = 10: nothing is trimmed and the order is untouched.
    let all = teleport_select(list.clone(), 10);
    assert_eq!(all.iter().map(|(k, _)| k.0).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    // DataA = 0: the unsigned limit never trims (quirk of `0x6F069D60`).
    assert_eq!(teleport_select(list, 0).len(), 4);
}

#[test]
fn resurrection_comparator_matches_the_original() {
    let text = include_str!("data/human_heroes/res_compare.txt");
    let data: Vec<Vec<u32>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut w: Vec<&str> = l.split_whitespace().collect();
            let sign: i32 = w.pop().unwrap().parse().unwrap();
            let mut v: Vec<u32> = w.iter().map(|x| u32::from_str_radix(x, 16).unwrap()).collect();
            v.push(sign as u32);
            v
        })
        .collect();
    assert!(data.len() >= 300);
    let rec = |r: &[u32], o: usize| ResurrectionRec {
        owner: r[o] as i32,
        key: r[o + 1] as i32,
        real: F32::from_bits(r[o + 2]),
        flag: r[o + 3] as i32,
        player: r[o + 4] as i32,
        id: r[o + 5] as i32,
    };
    for r in &data {
        let want = r[12] as i32;
        let got = resurrection_compare(&rec(r, 0), &rec(r, 6)).signum();
        assert_eq!(got, want, "{r:08x?}");
    }
}
