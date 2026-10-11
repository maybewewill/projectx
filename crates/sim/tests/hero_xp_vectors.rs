//! The experience-table builder (`sim::heroes::build`) against the original `Game.dll` builder, run in Unicorn by
//! `tools/emu/hero_xp_emu.py`. The vector file is written by that script to a path the caller chooses; it holds
//! synthetic Misc content and the builder's output, and is not committed.
//!
//! Each line also carries the need lookup of the original for levels around 1..N (the clamp).
//!
//! Run: `GAME_DLL=/path/to/Game.dll HERO_XP_VECTORS=/path/to/vectors.txt cargo test -p sim --test hero_xp_vectors`.
//! Without either variable the test returns early (and says so).

use sim::heroes::{build, XpInputs};
use softfloat::{Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

/// The tokens of one `KEY v v v` field (a `-` means no values).
fn values<'a>(field: &'a str, key: &str) -> Vec<&'a str> {
    let rest = field
        .strip_prefix(key)
        .unwrap_or_else(|| panic!("field {field:?} does not start with {key:?}"));
    let rest = rest.trim();
    if rest == "-" || rest.is_empty() {
        Vec::new()
    } else {
        rest.split_whitespace().collect()
    }
}

fn ints(field: &str, key: &str) -> Vec<i32> {
    values(field, key)
        .into_iter()
        .map(|t| t.parse::<i32>().expect("integer"))
        .collect()
}

fn bits(field: &str, key: &str) -> Vec<u32> {
    values(field, key)
        .into_iter()
        .map(|t| t.parse::<u32>().expect("unsigned"))
        .collect()
}

fn hex_reals(field: &str, key: &str) -> [F32; 3] {
    let v: Vec<u32> = values(field, key)
        .into_iter()
        .map(|t| u32::from_str_radix(t, 16).expect("hex"))
        .collect();
    assert_eq!(v.len(), 3, "three formula constants in {field:?}");
    [F32(v[0]), F32(v[1]), F32(v[2])]
}

#[test]
fn builder_matches_the_original_in_unicorn() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: skipping the hero experience vectors");
        return;
    };
    let Ok(path) = std::env::var("HERO_XP_VECTORS") else {
        eprintln!("HERO_XP_VECTORS not set: skipping the hero experience vectors");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let mut cases = 0usize;
    let mut mismatches = 0usize;
    let mut values_checked = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split(" | ").collect();
        assert_eq!(f.len(), 14, "field count in {line:?}");
        let n = ints(f[0], "N")[0];
        let inputs = XpInputs {
            max_hero_level: n,
            need: ints(f[1], "NEED"),
            need_formula: hex_reals(f[2], "NEEDF"),
            grant: ints(f[3], "GRANT"),
            grant_formula: hex_reals(f[4], "GRANTF"),
            factor: ints(f[5], "FACTOR"),
            max_unit_level: ints(f[6], "N2")[0],
            normal: ints(f[7], "NORMAL"),
            normal_formula: hex_reals(f[8], "NORMALF"),
        };
        let out = build(&inputs, &t);
        let want_need: Vec<u32> = bits(f[9], "OUTNEED");
        let want_grant: Vec<u32> = bits(f[10], "OUTGRANT");
        let want_factor: Vec<u32> = bits(f[11], "OUTFACTOR");
        let want_normal: Vec<u32> = bits(f[12], "OUTNORMAL");
        cases += 1;

        // The need lookup of the original (the level clamp), `level:result` pairs.
        for pair in values(f[13], "CLAMP") {
            let (level, want_entry) = pair.split_once(':').expect("level:result");
            let level: i32 = level.parse().expect("level");
            let want: u32 = want_entry.parse().expect("result");
            values_checked += 1;
            if out.need_xp(level) as u32 != want {
                mismatches += 1;
                eprintln!("MISMATCH clamp level {level}: got {} want {want}", out.need_xp(level));
            }
        }

        let got_need: Vec<u32> = (0..n as usize).map(|i| out_need(&out, i) as u32).collect();
        let got_grant: Vec<u32> = (0..n as usize).map(|i| out_grant(&out, i) as u32).collect();
        let got_factor: Vec<u32> = (0..n as usize).map(|i| out_factor(&out, i)).collect();
        let got_normal: Vec<u32> = (0..inputs.max_unit_level as usize)
            .map(|i| out.grant_normal_xp(i).expect("normal entry"))
            .collect();
        values_checked += got_need.len() + got_grant.len() + got_factor.len() + got_normal.len();
        if got_need != want_need || got_grant != want_grant || got_factor != want_factor || got_normal != want_normal {
            mismatches += 1;
            eprintln!("MISMATCH: {line}");
            eprintln!(" got need {got_need:?} grant {got_grant:?} factor {got_factor:x?} normal {got_normal:?}");
        }
    }
    println!("hero XP builder: {cases} cases, {values_checked} values, {mismatches} mismatches");
    assert_eq!(mismatches, 0);
}

fn out_need(t: &sim::heroes::XpTables, i: usize) -> i32 {
    t.hero_need(i).expect("hero entry")
}

fn out_grant(t: &sim::heroes::XpTables, i: usize) -> i32 {
    t.grant_hero_xp(i).expect("hero entry")
}

fn out_factor(t: &sim::heroes::XpTables, i: usize) -> u32 {
    t.hero_factor(i).expect("factor entry").0
}
