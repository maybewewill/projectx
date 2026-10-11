//! The kill experience of Game.dll 1.26a (`docs/spec/heroes.md`, section 8) against the original, run in Unicorn by
//! `tools/emu/hero_kill_xp_emu.py`. The vector file is written by that script to a path the caller chooses; it holds
//! synthetic inputs and the outputs of the original, and is not committed.
//!
//! Mode `A` lines check `sim::heroes::experience_engine` (the XP engine `0x6F20B080`: plain and walk gains, handicap,
//! upgrade bonus, the skill-point cap). Mode `B` lines check `kill_amount` and `distribute_kill` (the amount
//! `0x6F2A0230`, the split `0x6F2A3190` and the engine calls for every recipient).
//!
//! Run: `GAME_DLL=/path/to/Game.dll KILL_XP_VECTORS=/path/to/vectors.txt cargo test -p sim --test kill_xp_vectors`.
//! Without either variable the test returns early (and says so).

use std::collections::HashMap;

use sim::heroes::{
    distribute_kill, experience_engine, kill_amount, BonusEnv, EngineEnv, Gain, KillEnv, KillVictim, XpTables,
};
use sim::natives::game::HeroObj;
use softfloat::{Tables, F32};

/// One candidate of a split line: owner, in-range flag, eligible flag, xp, sp, counter, suspended, and the outputs.
type Cand = (usize, bool, bool, i32, i32, i32, bool, i32, i32, i32);

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn fields(line: &str) -> HashMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|tok| tok.split_once('='))
        .collect()
}

fn get<'a>(f: &HashMap<&str, &'a str>, key: &str) -> &'a str {
    f.get(key).copied().unwrap_or_else(|| panic!("missing field {key}"))
}

fn ints(s: &str) -> Vec<i32> {
    if s == "-" {
        return Vec::new();
    }
    s.split(',').map(|x| x.parse::<i32>().expect("int")).collect()
}

fn hex_u32(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn floats(s: &str) -> Vec<F32> {
    if s == "-" {
        return Vec::new();
    }
    s.split(',').map(|x| F32(hex_u32(x))).collect()
}

fn levels_map(s: &str) -> HashMap<u32, i32> {
    let mut m = HashMap::new();
    if s == "-" {
        return m;
    }
    for entry in s.split(',') {
        let (code, k) = entry.split_once(':').expect("code:levels");
        m.insert(hex_u32(code), k.parse().expect("levels"));
    }
    m
}

fn values_map(s: &str) -> HashMap<u32, Vec<F32>> {
    let mut m = HashMap::new();
    if s == "-" {
        return m;
    }
    for entry in s.split(';') {
        let (code, vals) = entry.split_once(':').expect("code:values");
        m.insert(hex_u32(code), floats(vals));
    }
    m
}

fn caps_arr(s: &str) -> [(i32, i32); 5] {
    let mut caps = [(0, 0); 5];
    if s == "-" {
        return caps;
    }
    for entry in s.split(',') {
        let mut it = entry.split(':');
        let handle = hex_u32(it.next().expect("handle"));
        let lo: i32 = it.next().expect("min").parse().expect("min");
        let hi: i32 = it.next().expect("max").parse().expect("max");
        caps[(handle - 0x1000) as usize] = (lo, hi);
    }
    caps
}

fn codes9(s: &str) -> [u32; 9] {
    let v: Vec<u32> = s.split(',').map(|x| x.parse().expect("code")).collect();
    let mut out = [0u32; 9];
    out.copy_from_slice(&v);
    out
}

fn hero_at(level_minus_one: i32, xp: i32, sp: i32, susp: bool) -> HeroObj {
    HeroObj {
        level: level_minus_one + 1,
        xp,
        skill_points: sp,
        xp_suspended: susp,
        ..HeroObj::default()
    }
}

fn check_a(line: &str, tb: &Tables) -> Result<(), String> {
    let f = fields(line);
    let need = ints(get(&f, "NEED"));
    let factor = floats(get(&f, "FACT"));
    let t = XpTables::from_columns(need, factor, Vec::new(), Vec::new());
    let levels = levels_map(get(&f, "LEV"));
    let values = values_map(get(&f, "VALS"));
    let lv = move |code: u32| levels.get(&code).copied().unwrap_or(0);
    let vf = move |code: u32, idx: i32| {
        values
            .get(&code)
            .and_then(|v| v.get(idx as usize).copied())
            .unwrap_or(F32::ZERO)
    };
    let codes = codes9(get(&f, "CODES"));
    let env = EngineEnv {
        handicap: F32(hex_u32(get(&f, "HCAP"))),
        bonus: BonusEnv {
            b1: get(&f, "B1").parse().expect("B1"),
            slots: get(&f, "SLOTS").parse().expect("SLOTS"),
            codes,
            levels: &lv,
            value: &vf,
        },
        caps: caps_arr(get(&f, "CAPS")),
    };
    let flag = |k: &str| get(&f, k) != "0";
    let c: i32 = get(&f, "C").parse().expect("C");
    let xp: i32 = get(&f, "XP").parse::<u32>().expect("XP") as i32;
    let sp: i32 = get(&f, "SP").parse().expect("SP");
    let mut hero = hero_at(c, xp, sp, flag("SUSP"));
    let gain = Gain {
        handicap: flag("P5"),
        walk: flag("P6"),
        range: flag("P7"),
    };
    experience_engine(&t, tb, &mut hero, F32(hex_u32(get(&f, "V"))), gain, &env);
    let want_xp: i32 = get(&f, "OUT_XP").parse::<u32>().expect("OUT_XP") as i32;
    let want_sp: i32 = get(&f, "OUT_SP").parse().expect("OUT_SP");
    let want_c: i32 = get(&f, "OUT_C").parse().expect("OUT_C");
    let got = (hero.xp, hero.skill_points, hero.level - 1);
    if got != (want_xp, want_sp, want_c) {
        return Err(format!("engine: got {got:?}, want {:?}", (want_xp, want_sp, want_c)));
    }
    Ok(())
}

fn check_b(line: &str, tb: &Tables) -> Result<(), String> {
    let f = fields(line);
    let need = ints(get(&f, "NEED"));
    let factor = floats(get(&f, "FACT"));
    let grant_hero = ints(get(&f, "GH"));
    let grant_unit: Vec<u32> = ints(get(&f, "GU")).into_iter().map(|g| g as u32).collect();
    let t = XpTables::from_columns(need, factor, grant_hero, grant_unit);
    let levels = levels_map(get(&f, "LEV"));
    let values = values_map(get(&f, "VALS"));
    let lv = move |code: u32| levels.get(&code).copied().unwrap_or(0);
    let vf = move |code: u32, idx: i32| {
        values
            .get(&code)
            .and_then(|v| v.get(idx as usize).copied())
            .unwrap_or(F32::ZERO)
    };
    let codes = codes9(get(&f, "CODES"));
    let slots: i32 = get(&f, "SLOTS").parse().expect("SLOTS");
    let caps = caps_arr(get(&f, "CAPS"));
    let b1s = ints(get(&f, "B1"));
    let hcaps = floats(get(&f, "HCAP"));

    let vtype: u8 = get(&f, "VTYPE").parse().expect("VTYPE");
    let vflags: u32 = get(&f, "VFLAGS").parse().expect("VFLAGS");
    let vhero = get(&f, "VHERO") == "1";
    let victim = KillVictim {
        type_upper: vtype.is_ascii_uppercase(),
        illusion: vflags & 0x4000_0000 != 0,
        summoned: get(&f, "VSUM") != "0",
        owner: get(&f, "VOWN").parse().expect("VOWN"),
        level: if vhero {
            get(&f, "VCOUNT").parse::<i32>().expect("VCOUNT") + 1
        } else {
            get(&f, "VLEV").parse().expect("VLEV")
        },
        ability: match get(&f, "ABIL") {
            "-" => None,
            s => {
                let (flag, over) = s.split_once(':').expect("ABIL");
                Some((flag == "1", over.parse().expect("ABIL level")))
            }
        },
    };
    let kenv = KillEnv {
        max_unit: get(&f, "MU").parse().expect("MU"),
        summon_factor: F32(hex_u32(get(&f, "SUMFACT"))),
    };
    let want_k: i32 = get(&f, "K").parse().expect("K");

    // candidate: owner, in, elig, xp, sp, c, susp, out xp, out sp, out c, reserved
    let mut cands: Vec<Cand> = Vec::new();
    let cand_text = get(&f, "CAND");
    if cand_text != "-" {
        for item in cand_text.split(';') {
            let v: Vec<&str> = item.split('.').collect();
            let p = |i: usize| v[i].parse::<i64>().expect("cand field");
            cands.push((
                p(0) as usize,
                p(1) != 0,
                p(2) != 0,
                p(3) as u32 as i32,
                p(4) as i32,
                p(5) as i32,
                p(6) != 0,
                p(7) as u32 as i32,
                p(8) as i32,
                p(9) as i32,
            ));
        }
    }
    let gl = get(&f, "GL") == "1";
    let in_list: Vec<usize> = (0..cands.len()).filter(|&i| cands[i].2 && cands[i].1).collect();
    let gl_list: Vec<usize> = if gl {
        (0..cands.len()).filter(|&i| cands[i].2).collect()
    } else {
        Vec::new()
    };
    let chosen = if in_list.is_empty() { gl_list } else { in_list };

    let amount = kill_amount(&t, &victim, &kenv);
    if amount != want_k {
        return Err(format!("amount: got {amount}, want {want_k}"));
    }

    let mut recipients: Vec<(HeroObj, EngineEnv)> = chosen
        .iter()
        .map(|&i| {
            let c = &cands[i];
            let owner = c.0;
            (
                hero_at(c.5, c.3, c.4, c.6),
                EngineEnv {
                    handicap: hcaps[owner],
                    bonus: BonusEnv {
                        b1: b1s[owner],
                        slots,
                        codes,
                        levels: &lv,
                        value: &vf,
                    },
                    caps,
                },
            )
        })
        .collect();
    let k = distribute_kill(&t, tb, &victim, &kenv, &mut recipients);
    if k != want_k {
        return Err(format!("distribute amount: got {k}, want {want_k}"));
    }
    let mut results: Vec<(i32, i32, i32)> = cands.iter().map(|c| (c.3, c.4, c.5)).collect();
    for (pos, &i) in chosen.iter().enumerate() {
        let h = &recipients[pos].0;
        results[i] = (h.xp, h.skill_points, h.level - 1);
    }
    for (i, c) in cands.iter().enumerate() {
        let got = results[i];
        let want = (c.7, c.8, c.9);
        if got != want {
            return Err(format!("candidate {i} (chosen {}): got {got:?}, want {want:?}", chosen.contains(&i)));
        }
    }
    Ok(())
}

#[test]
fn kill_xp_vectors() {
    let Some(tb) = tables() else {
        eprintln!("GAME_DLL not set: skipping the kill experience vectors");
        return;
    };
    let Ok(path) = std::env::var("KILL_XP_VECTORS") else {
        eprintln!("KILL_XP_VECTORS not set: skipping the kill experience vectors");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let (mut a, mut b) = (0usize, 0usize);
    let mut fails = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let result = if line.starts_with("A ") {
            a += 1;
            check_a(line, &tb)
        } else if line.starts_with("B ") {
            b += 1;
            check_b(line, &tb)
        } else {
            Err("unknown line kind".to_string())
        };
        if let Err(e) = result {
            fails.push(format!("{e}\n  {}", &line[..line.len().min(160)]));
        }
    }
    eprintln!("kill XP vectors: {a} engine cases, {b} split cases, {} mismatches", fails.len());
    assert!(fails.is_empty(), "{} mismatches:\n{}", fails.len(), fails.iter().take(5).cloned().collect::<Vec<_>>().join("\n"));
}
