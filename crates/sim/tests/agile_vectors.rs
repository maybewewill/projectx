//! Scenario vectors of the agile property engine, recorded from the original routines in Unicorn
//! (`tools/emu/agile_engine_emu.py gen`, docs/spec/agile.md). Each scenario creates a property, applies modifiers /
//! listeners / value sets at given ticks and runs 0.005 s ticks; the firing of every listener (tick, queue time, row)
//! and the value at checkpoints must match bit for bit. Needs `GAME_DLL` for the reciprocal table.

use sim::agile::{Agile, LisId};
use softfloat::{Tables, F32};

const DATA: &str = include_str!("vectors/agile_scenarios.txt");
const TICK: u32 = 0x3BA3_D70A;

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[derive(Debug)]
enum Op {
    Mod(u32, u32),
    Lis(u32, u32, u32),
    Set(u32),
}

#[derive(Default)]
struct Scenario {
    id: usize,
    now: u32,
    row: u32,
    base: u32,
    lo: u32,
    hi: u32,
    ticks: usize,
    ops: Vec<(usize, Op)>,
    events: Vec<Event>,
    values: Vec<(usize, u32)>,
}

fn parse() -> Vec<Scenario> {
    let mut out = Vec::new();
    let mut cur: Option<Scenario> = None;
    for line in DATA.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(' ').collect();
        match f[0] {
            "SCN" => {
                cur = Some(Scenario {
                    id: f[1].parse().unwrap(),
                    now: hex(f[2]),
                    row: f[3].parse().unwrap(),
                    base: hex(f[4]),
                    lo: hex(f[5]),
                    hi: hex(f[6]),
                    ticks: f[7].parse().unwrap(),
                    ..Default::default()
                })
            }
            "OP" => {
                let s = cur.as_mut().unwrap();
                let t: usize = f[1].parse().unwrap();
                let op = match f[2] {
                    "M" => Op::Mod(hex(f[3]), hex(f[4])),
                    "L" => Op::Lis(f[3].parse().unwrap(), hex(f[4]), hex(f[5])),
                    "S" => Op::Set(hex(f[3])),
                    _ => panic!("op {line}"),
                };
                s.ops.push((t, op));
            }
            "E" => cur.as_mut().unwrap().events.push((
                f[1].parse().unwrap(),
                hex(f[2]),
                f[3].parse().unwrap(),
                f[4].parse().unwrap(),
            )),
            "V" => cur.as_mut().unwrap().values.push((f[1].parse().unwrap(), hex(f[2]))),
            "END" => out.push(cur.take().unwrap()),
            _ => panic!("line {line}"),
        }
    }
    out
}

type Event = (usize, u32, u32, usize);

fn run(s: &Scenario, t: &Tables) -> (Vec<Event>, Vec<(usize, u32)>) {
    let mut w = Agile::new();
    w.set_clock(F32(s.now), s.row);
    let p = w.new_property(F32(s.base), F32(s.lo), F32(s.hi));
    let mut lis: Vec<LisId> = Vec::new();
    let mut events = Vec::new();
    let mut values = Vec::new();
    let index = |lis: &Vec<LisId>, l: LisId| lis.iter().position(|&x| x == l).unwrap();
    for tick in 0..s.ticks {
        for (_, op) in s.ops.iter().filter(|(t0, _)| *t0 == tick) {
            match *op {
                Op::Mod(rate, hor) => {
                    w.add_modifier(p, F32(rate), F32(hor), t);
                }
                Op::Lis(mode, thr, msg) => {
                    let l = w.add_listener(p, mode, F32(thr), msg, t);
                    lis.push(l);
                }
                Op::Set(x) => w.set_value(p, F32(x), t),
            }
            for e in w.take_events() {
                events.push((tick, e.at.0, e.row, index(&lis, e.listener)));
            }
        }
        for e in w.tick(F32(TICK), t) {
            events.push((tick, e.at.0, e.row, index(&lis, e.listener)));
        }
        if tick % 250 == 0 || tick == s.ticks - 1 {
            values.push((tick, w.value(p).0));
        }
    }
    (events, values)
}

#[test]
fn scenarios_match_the_original() {
    let Some(t) = tables() else { return };
    let all = parse();
    let mut fired = 0;
    for s in &all {
        let (events, values) = run(s, &t);
        assert_eq!(events, s.events, "scenario {} events (start {:08x} row {})", s.id, s.now, s.row);
        assert_eq!(values, s.values, "scenario {} values", s.id);
        fired += events.len();
    }
    assert!(all.len() >= 500, "{} scenarios", all.len());
    assert!(fired >= 200, "{fired} firings");
}
