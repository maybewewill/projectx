//! Unit life / mana regeneration against the original (`tools/emu/regen_emu.py gen`, docs/spec/agile.md section 9).
//! Each case builds a unit with a life and a mana property, applies the operations of the vector file (the refreshes
//! `0x6F282FC0` / `0x6F283F60`, the regeneration flags, rate additions, death and revival, maximum / value setters, ticks of
//! 0.005 s) and compares a snapshot after every operation: values, slopes, modifier state, bases and rates, bit for bit.
//! Needs `GAME_DLL` for the reciprocal table.

use sim::agile::Agile;
use sim::regen::UnitStats;
use softfloat::{Tables, F32};

const DATA: &str = include_str!("vectors/regen_scenarios.txt");
const TICK: u32 = 0x3BA3_D70A;

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

struct Case {
    id: usize,
    header: Vec<u32>,
    steps: Vec<(String, String)>,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    let mut pending_op: Option<String> = None;
    for line in DATA.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "CASE" => {
                let f: Vec<&str> = rest.split(' ').collect();
                cur = Some(Case {
                    id: f[0].parse().unwrap(),
                    header: vec![
                        hex(f[1]),
                        f[2].parse().unwrap(),
                        hex(f[3]),
                        hex(f[4]),
                        hex(f[5]),
                        hex(f[6]),
                        hex(f[7]),
                        hex(f[8]),
                        hex(f[9]),
                        hex(f[10]),
                        hex(f[11]),
                        hex(f[12]),
                    ],
                    steps: Vec::new(),
                });
            }
            "OP" => pending_op = Some(rest.to_string()),
            "S" => cur.as_mut().unwrap().steps.push((pending_op.take().unwrap(), rest.to_string())),
            "END" => out.push(cur.take().unwrap()),
            _ => panic!("line {line}"),
        }
    }
    out
}

fn snapshot(ag: &Agile, st: &UnitStats) -> String {
    let modstate = |m: Option<sim::agile::ModId>| match m {
        None => "-".to_string(),
        Some(m) => format!("{:08x}", ag.modifier_rate(m).0),
    };
    format!(
        "{:08x} {:08x} {:08x} {:08x} {} {} {:08x} {:08x} {:08x} {:08x}",
        ag.value(st.life).0,
        ag.value(st.mana).0,
        ag.slope(st.life).0,
        ag.slope(st.mana).0,
        modstate(st.life_modifier()),
        modstate(st.mana_modifier()),
        ag.property_state(st.life).0 .0,
        ag.property_state(st.mana).0 .0,
        st.life_rate.0,
        st.mana_rate.0,
    )
}

fn run(c: &Case, t: &Tables) {
    let h = &c.header;
    let mut ag = Agile::new();
    ag.set_clock(F32(h[0]), h[1]);
    let life = ag.new_property(F32(h[2]), F32(h[3]), F32(h[4]));
    let mana = ag.new_property(F32(h[5]), F32(h[6]), F32(h[7]));
    let mut st = UnitStats::new(life, mana, F32(h[8]), F32(h[9]), h[10], h[11] & 0x100 != 0);
    for (i, (op, expect)) in c.steps.iter().enumerate() {
        let f: Vec<&str> = op.split(' ').collect();
        match f[0] {
            "I" => st.refresh_both(&mut ag, t),
            "LON" => st.set_life_regen(true, &mut ag, t),
            "LOFF" => st.set_life_regen(false, &mut ag, t),
            "LDIS" => st.set_life_disabled(true, &mut ag, t),
            "LEN" => st.set_life_disabled(false, &mut ag, t),
            "MDIS" => st.set_mana_disabled(true, &mut ag, t),
            "MEN" => st.set_mana_disabled(false, &mut ag, t),
            "LADD" => st.add_life_rate(F32(hex(f[1])), &mut ag, t),
            "MADD" => st.add_mana_rate(F32(hex(f[1])), &mut ag, t),
            "DEAD" => st.set_dead(true, &mut ag, t),
            "ALIVE" => st.set_dead(false, &mut ag, t),
            "MAXL" => {
                let (lo, _) = ag.limits(life);
                ag.set_limits(life, lo, F32(hex(f[1])), t);
            }
            "MAXM" => {
                let (lo, _) = ag.limits(mana);
                ag.set_limits(mana, lo, F32(hex(f[1])), t);
            }
            "SETL" => ag.set_value(life, F32(hex(f[1])), t),
            "SETM" => ag.set_value(mana, F32(hex(f[1])), t),
            "TICK" => {
                for _ in 0..f[1].parse::<usize>().unwrap() {
                    let _ = ag.tick(F32(TICK), t);
                }
            }
            other => panic!("op {other}"),
        }
        assert_eq!(&snapshot(&ag, &st), expect, "case {} step {} ({op})", c.id, i);
    }
}

#[test]
fn regeneration_matches_the_original() {
    let Some(t) = tables() else { return };
    let all = parse();
    assert!(all.len() >= 700, "{} cases", all.len());
    let ops: usize = all.iter().map(|c| c.steps.len()).sum();
    assert!(ops >= 5000, "{ops} operations");
    for c in &all {
        run(c, &t);
    }
}
