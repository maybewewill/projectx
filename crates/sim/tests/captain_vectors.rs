//! Replays the Unicorn vectors of the captain routines of Game.dll 1.26a against `sim::captains`:
//! the assault request control flow (`0x6F2D6EA0`, `tools/emu/captain_assault_emu.py`) and the creep-camp chooser
//! (`0x6F2E3980`, `tools/emu/creep_camp_emu.py`). The vectors are synthetic states with the original's answers only.

use sim::captains::{assault_flow, choose_camp, readiness_with, CampView, Pass, ReadyUnit};

#[test]
fn assault_request_matches_the_original() {
    let text = include_str!("vectors/captain_assault_seed1.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let typ = u32::from_str_radix(f[0], 16).unwrap();
        let qty: i32 = f[1].parse().unwrap();
        let flags: u32 = f[2].parse().unwrap();
        let mut script: Vec<i32> = f[3].split(',').map(|x| x.parse().unwrap()).collect();
        script.reverse();
        let mut calls: Vec<String> = Vec::new();
        let ok = assault_flow(typ, qty, |pass, t, q| {
            let (kind, flag) = match pass {
                Pass::Group => ("A", 0),
                Pass::Recruit(first) => ("B", u32::from(first)),
            };
            calls.push(format!("{kind}:{t:X}:{flag}"));
            *q = q.wrapping_sub(script.pop().unwrap_or(0));
        });
        let out_flags = if ok { flags } else { flags & !1 };
        let got = format!("{}|{}|{}", u8::from(ok), out_flags, calls.join(";"));
        assert_eq!(got, f[4], "line: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}

#[test]
fn creep_camp_choice_matches_the_original() {
    let text = include_str!("vectors/creep_camp_seed1.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (min, max): (i32, i32) = (f[0].parse().unwrap(), f[1].parse().unwrap());
        let fly = f[2] == "1";
        let bound: u32 = f[3].parse().unwrap();
        let camps: Vec<CampView> = if f[4] == "-" {
            Vec::new()
        } else {
            f[4].split(';')
                .map(|c| {
                    let p: Vec<i32> = c.split('/').map(|x| x.parse().unwrap()).collect();
                    CampView { count: p[0], skip: p[1] != 0, level: p[2], flyers: p[3] != 0 }
                })
                .collect()
        };
        let mut costs: Vec<u32> = if f[5] == "-" { Vec::new() } else { f[5].split(',').map(|x| x.parse().unwrap()).collect() };
        costs.reverse();
        let got = choose_camp(&camps, min, max, fly, bound, |_, _| costs.pop().unwrap());
        let want: i32 = f[6].parse().unwrap();
        assert_eq!(got.map_or(-1, |i| i as i32), want, "line: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}

#[test]
fn captain_readiness_matches_the_original() {
    // `0x6F2E91A0` (tools/emu/captain_ready_emu.py): 600 synthetic groups. The percentages divide in soft float, which
    // needs the reciprocal table of GAME_DLL.
    let Ok(path) = std::env::var("GAME_DLL") else {
        eprintln!("GAME_DLL not set: skipping");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let tables = softfloat::Tables::from_game_dll(&bytes).expect("tables");
    let text = include_str!("vectors/captain_ready_seed1.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let life = f[0] == "1";
        let (b8, bc): (i32, i32) = (f[2].parse().unwrap(), f[3].parse().unwrap());
        let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
        let units: Vec<ReadyUnit> = if f[4] == "-" {
            Vec::new()
        } else {
            f[4].split(';')
                .map(|u| {
                    let p: Vec<u32> = u.split('/').map(hex).collect();
                    let fl = |i: usize| softfloat::F32::from_bits(p[i]);
                    ReadyUnit { type_id: p[0], flags: p[1], life: fl(2), max_life: fl(3), mana: fl(4), max_mana: fl(5) }
                })
                .collect()
        };
        let got = readiness_with(|a, b| softfloat::arith::div(a, b, &tables), b8 - bc, &units, life);
        let want: i32 = f[5].parse::<i64>().unwrap() as i32;
        assert_eq!(got, want, "line: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}

#[test]
fn camp_assignment_matches_the_original() {
    // `0x6F2E38A0` (tools/emu/camp_assign_emu.py): 600 synthetic camp lists.
    let text = include_str!("vectors/camp_assign_seed1.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let d: i32 = f[0].parse().unwrap();
        let cache = softfloat::F32::from_bits(u32::from_str_radix(f[1], 16).unwrap());
        let camps: Vec<sim::captains::CampSlot> = if f[2] == "-" {
            Vec::new()
        } else {
            f[2].split(';')
                .map(|c| {
                    let p: Vec<&str> = c.split('/').collect();
                    sim::captains::CampSlot {
                        count: p[0].parse().unwrap(),
                        dist: (p[1] == "1").then(|| softfloat::F32::from_bits(u32::from_str_radix(p[2], 16).unwrap())),
                    }
                })
                .collect()
        };
        let got = sim::captains::assign_camp(d, Some(cache), &camps);
        assert_eq!(got, f[3].parse::<i32>().unwrap(), "line: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}

#[test]
fn captain_position_test_matches_the_original() {
    // `0x6F2E76E0` (tools/emu/captain_goal_emu.py): 600 cases, radius / captain / point bits and the original's answer.
    let text = include_str!("vectors/captain_goal_seed1.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let b = |i: usize| f32::from_bits(u32::from_str_radix(f[i], 16).unwrap());
        let got = sim::captains::within_goal((b(1), b(2)), (b(3), b(4)), b(0));
        assert_eq!(u8::from(got).to_string(), f[5], "line: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}
