//! The day / night watch against the original (`tools/emu/daynight_emu.py gen`, docs/spec/agile.md section 9.6). Each case
//! runs the time of day (forever modifier + wrap listener) and the watch `DayWatch` for about two cycles; the sequence of
//! events (watch evaluations, flag changes with the queue clock, messages to the world and the units, wraps) and the listener
//! thresholds after every evaluation must match the recorded run of the real routine 0x6F429460. Needs `GAME_DLL`.

use sim::agile::Agile;
use sim::daynight::{DayCycle, DayWatch, DAY_BEGINS, NIGHT_BEGINS, WRAP_MESSAGE};
use softfloat::{arith, Tables, F32};

const DATA: &str = include_str!("vectors/daynight_scenarios.txt");
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
    head: Vec<u32>,
    lines: Vec<String>,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for line in DATA.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "CASE" => {
                let f: Vec<&str> = rest.split(' ').collect();
                cur = Some(Case {
                    head: vec![
                        hex(f[0]),
                        hex(f[1]),
                        hex(f[2]),
                        hex(f[3]),
                        hex(f[4]),
                        hex(f[5]),
                        f[6].parse().unwrap(),
                        f[7].parse().unwrap(),
                    ],
                    lines: Vec::new(),
                })
            }
            "END" => out.push(cur.take().unwrap()),
            _ => cur.as_mut().unwrap().lines.push(line.to_string()),
        }
    }
    out
}

fn listener_text(w: &DayWatch, ag: &Agile) -> String {
    let f = |x: Option<(u32, u32)>| x.map_or("-".to_string(), |(m, t)| format!("{m}:{t:08x}"));
    let [a, b] = w.listeners(ag);
    format!("{} {}", f(a), f(b))
}

#[allow(clippy::too_many_lines)]
fn run(c: &Case, t: &Tables) -> Vec<String> {
    let h = &c.head;
    let (length, hours, scale, start) = (F32(h[0]), F32(h[1]), F32(h[2]), F32(h[3]));
    let cycle = DayCycle { day_length: length, day_hours: hours, dawn: F32(h[4]), dusk: F32(h[5]) };
    let mut ag = Agile::new();
    let prop = ag.new_property(start, F32(0xFF7F_FFFF), F32(0x7F7F_FFFF));
    let rate = arith::div(arith::mul(scale, hours), length, t);
    ag.add_modifier(prop, rate, F32::ZERO, t);
    let wrap = ag.add_listener(prop, 1, hours, WRAP_MESSAGE, t);
    let _ = ag.take_events();
    let mut watch = DayWatch::default();
    watch.day = h[6] != 0;
    let mut out: Vec<String> = Vec::new();
    let flag_change = |ag: &Agile, tick: usize, day: bool, out: &mut Vec<String>| {
        out.push(format!("C {} {:08x} {} {}", tick, ag.now().0, ag.row(), u32::from(day)));
        let msg = if day { DAY_BEGINS } else { NIGHT_BEGINS };
        out.push(format!("D {tick} {msg:x}"));
        out.push(format!("B {tick} {msg:x}"));
    };
    // start-up: the first evaluation
    if let Some(day) = watch.recompute(&mut ag, t, prop, &cycle) {
        flag_change(&ag, 0, day, &mut out);
    }
    out.push(format!("L 0 {}", listener_text(&watch, &ag)));
    let handle = |ag: &mut Agile, watch: &mut DayWatch, tick: usize, out: &mut Vec<String>, f: sim::agile::Fired| {
        if f.listener == wrap {
            ag.set_value(prop, F32::ZERO, t);
            out.push(format!("W {} {:08x} {}", tick, ag.now().0, ag.row()));
        } else if watch.owns(f.listener) {
            out.push(format!("R {} {:08x} {}", tick, ag.now().0, ag.row()));
            if let Some(day) = watch.recompute(ag, t, prop, &cycle) {
                flag_change(ag, tick, day, out);
            }
            out.push(format!("L {} {}", tick, listener_text(watch, ag)));
        }
    };
    for f in ag.take_events() {
        handle(&mut ag, &mut watch, 0, &mut out, f);
    }
    let ticks = h[7] as usize;
    let mut values = Vec::new();
    for i in 0..ticks {
        ag.begin_tick(F32(TICK));
        while let Some(f) = ag.next_fired(Some(t)) {
            handle(&mut ag, &mut watch, i, &mut out, f);
        }
        if i % 5000 == 0 || i == ticks - 1 {
            values.push(format!("V {} {:08x}", i, ag.value(prop).0));
        }
    }
    out.extend(values);
    out
}

#[test]
fn day_night_watch_matches_the_original() {
    let Some(t) = tables() else { return };
    let all = parse();
    assert!(all.len() >= 60, "{} cases", all.len());
    let mut changes = 0;
    for (n, c) in all.iter().enumerate() {
        let got = run(c, &t);
        // the recorded lines keep the harness order: events in time order, then the value checkpoints
        let (exp_events, exp_vals): (Vec<&String>, Vec<&String>) = c.lines.iter().partition(|l| !l.starts_with("V "));
        let (got_events, got_vals): (Vec<&String>, Vec<&String>) = got.iter().partition(|l| !l.starts_with("V "));
        assert_eq!(got_events, exp_events, "case {n} events");
        assert_eq!(got_vals, exp_vals, "case {n} values");
        changes += exp_events.iter().filter(|l| l.starts_with("C ")).count();
    }
    assert!(changes >= 150, "{changes} flag changes");
}
