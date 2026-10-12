//! The hero skill learning model (`sim::heroes::{required_level, can_learn, learn, learned_count, slot_max}`) against
//! the original routines `0x6F207B60`, `0x6F208A00`, `0x6F2095D0`, `0x6F207C80`, `0x6F207CC0`, run in Unicorn by
//! `tools/emu/hero_learn_emu.py`. The vector files hold synthetic values and the original's results and are not
//! committed.
//!
//! Run: `HERO_CAN_VECTORS=can.txt HERO_LEARN_VECTORS=learn.txt cargo test -p sim --test hero_learn_vectors -- --nocapture`.
//! Without the variables the test returns early.

use std::collections::BTreeMap;

use sim::heroes::{
    can_learn, learn, learned_count, required_level, skill_point_cap, slot_max, LearnEffect, LearnEnv,
};
use sim::natives::game::HeroSlot;

struct Case {
    slots: [HeroSlot; 5],
    learned: BTreeMap<u32, i32>,
    skip: BTreeMap<u32, i32>,
    points: i32,
    avail: bool,
    deps_ok: bool,
    deps_flag: bool,
    counter: i32,
    cheat: bool,
    misc: i32,
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("hex {s:?}"))
}

fn map(s: &str) -> BTreeMap<u32, i32> {
    let mut m = BTreeMap::new();
    if s != "-" {
        for kv in s.split(',') {
            let (k, v) = kv.split_once(':').expect("code:value");
            m.insert(hex(k), v.parse().expect("value"));
        }
    }
    m
}

fn ints(s: &str) -> Vec<i32> {
    s.split(',').map(|x| x.parse().expect("int")).collect()
}

fn parse_case(text: &str) -> Case {
    let f: Vec<&str> = text.split_whitespace().collect();
    assert_eq!(f.len(), 12, "case fields in {text:?}");
    let codes: Vec<u32> = f[0].split(',').map(hex).collect();
    let (maxl, req) = (ints(f[1]), ints(f[2]));
    let mut slots = [HeroSlot::default(); 5];
    for i in 0..5 {
        slots[i] = HeroSlot { code: codes[i], max_level: maxl[i], req_level: req[i] };
    }
    Case {
        slots,
        learned: map(f[3]),
        skip: map(f[4]),
        points: f[5].parse().unwrap(),
        avail: f[6] == "1",
        deps_ok: f[7] == "1",
        deps_flag: f[8] == "1",
        counter: f[9].parse().unwrap(),
        cheat: hex(f[10]) & 0x100 != 0,
        misc: f[11].parse().unwrap(),
    }
}

fn with_env<R>(c: &Case, f: impl FnOnce(&LearnEnv) -> R) -> R {
    let learned = |code: u32| c.learned.get(&code).copied();
    let skip = |code: u32| c.skip.get(&code).copied().unwrap_or(0);
    let env = LearnEnv {
        learned: &learned,
        level_skip: &skip,
        misc_skip: c.misc,
        available: c.avail,
        deps: (c.deps_ok, c.deps_flag),
        level_counter: c.counter,
        cheat: c.cheat,
    };
    f(&env)
}

fn effects_text(e: &[LearnEffect]) -> String {
    if e.is_empty() {
        return "-".to_string();
    }
    e.iter()
        .map(|x| match x {
            LearnEffect::Error(k) => format!("E {k}"),
            LearnEffect::LevelUp(c) => format!("V {c}"),
            LearnEffect::Add(c) => format!("A {c}"),
            LearnEffect::SetLevel(c, l) => format!("S {c} {l}"),
            LearnEffect::NoPointsLeft => "D".to_string(),
            LearnEffect::Refresh => "U".to_string(),
            LearnEffect::Notify => "N".to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}

#[test]
fn can_learn_required_level_and_caps_match_the_original() {
    let Ok(path) = std::env::var("HERO_CAN_VECTORS") else {
        eprintln!("HERO_CAN_VECTORS not set: skipping");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let (mut cases, mut mismatches, mut learnable) = (0usize, 0usize, 0usize);
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        cases += 1;
        let ok = if let Some(rest) = line.strip_prefix("REQ ") {
            let parts: Vec<&str> = rest.split('|').collect();
            let c = parse_case(parts[0]);
            let sl: Vec<i32> = parts[1].split_whitespace().map(|x| x.parse().unwrap()).collect();
            let want: i32 = parts[2].trim().parse().unwrap();
            let code = c.slots[sl[0] as usize].code;
            let skip = c.skip.get(&code).copied().unwrap_or(0);
            required_level(c.slots[sl[0] as usize].req_level, skip, c.misc, sl[1]) == want
        } else if let Some(rest) = line.strip_prefix("CAP ") {
            let parts: Vec<&str> = rest.split('|').collect();
            let c = parse_case(parts[0]);
            let probe = hex(parts[1].trim());
            let w: Vec<i32> = parts[2].split_whitespace().map(|x| x.parse().unwrap()).collect();
            learned_count(c.learned.get(&probe).copied()) == w[0] && slot_max(&c.slots, probe) == w[1]
        } else {
            let parts: Vec<&str> = line.split('|').collect();
            let c = parse_case(parts[0]);
            let slot: usize = parts[1].trim().parse().unwrap();
            let w: Vec<i32> = parts[2].split_whitespace().map(|x| x.parse().unwrap()).collect();
            let (got, flag) = with_env(&c, |env| can_learn(&c.slots, slot, env));
            if got {
                learnable += 1;
            }
            (got as i32, flag as i32) == (w[0], w[1])
        };
        if !ok {
            mismatches += 1;
            eprintln!("MISMATCH {line}");
        }
    }
    println!("hero can-learn / required level / caps: {cases} cases ({learnable} learnable), {mismatches} mismatches");
    assert!(learnable > 20, "the vectors exercise the positive path");
    assert_eq!(mismatches, 0);
}

#[test]
fn the_learn_matches_the_original() {
    let Ok(path) = std::env::var("HERO_LEARN_VECTORS") else {
        eprintln!("HERO_LEARN_VECTORS not set: skipping");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let (mut cases, mut mismatches, mut learned_cases) = (0usize, 0usize, 0usize);
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        cases += 1;
        let parts: Vec<&str> = line.split('|').collect();
        let c = parse_case(parts[0]);
        let a: Vec<&str> = parts[1].split_whitespace().collect();
        let (code, force) = (hex(a[0]), a[1] == "1");
        let want_events = parts[2].trim();
        let want_points: i32 = parts[3].trim().parse().unwrap();
        let mut points = c.points;
        let effects = with_env(&c, |env| learn(&mut points, &c.slots, code, force, env));
        if effects.iter().any(|e| matches!(e, LearnEffect::SetLevel(..))) {
            learned_cases += 1;
        }
        if effects_text(&effects) != want_events || points != want_points {
            mismatches += 1;
            eprintln!("MISMATCH {line}: got {} points {points}", effects_text(&effects));
        }
    }
    println!("hero learn: {cases} cases ({learned_cases} learned a level), {mismatches} mismatches");
    assert!(learned_cases > 20, "the vectors exercise the learn path");
    assert_eq!(mismatches, 0);
}

#[test]
fn the_cap_is_the_sum_of_the_room_of_the_slots() {
    let slots = [
        HeroSlot { code: 1, max_level: 3, req_level: 1 },
        HeroSlot { code: 2, max_level: 3, req_level: 1 },
        HeroSlot { code: 3, max_level: 3, req_level: 1 },
        HeroSlot { code: 4, max_level: 1, req_level: 6 },
        HeroSlot::default(),
    ];
    assert_eq!(skill_point_cap(&slots, &|_| None), 10);
    // Slot 1 at index 1 (2 levels learned), slot 4 at index 0 (1 learned = full).
    let cap = skill_point_cap(&slots, &|c| match c {
        2 => Some(1),
        4 => Some(0),
        _ => None,
    });
    assert_eq!(cap, 7);
}
