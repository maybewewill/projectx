//! The production model on the agile engine (docs/spec/agile.md): training, building, research and upgrades complete
//! when their countdown property reaches 0, on the tick the original does it. The expected ticks come from
//! `tools/emu/agile_engine_emu.py cdvec` (the real routines in Unicorn): a countdown of `bldtm` seconds started `t_start`
//! ticks into the game completes `steps` ticks later, with the clock bits given. Needs `GAME_DLL`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::ai;
use sim::natives::ai::register_main;
use sim::natives::game::{Game, ObjKey, UnitType};
use sim::production::{BuildGrid, ProdEvent, TypeTech, UpgradeType};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const HALL: u32 = u32::from_be_bytes(*b"ohal");
const PEON: u32 = u32::from_be_bytes(*b"opeo");
const BARRACKS: u32 = u32::from_be_bytes(*b"obar");
const TECH: u32 = u32::from_be_bytes(*b"Rome");
const GRUNT: u32 = u32::from_be_bytes(*b"ogru");

const MAIN: &str = r#"
type player extends handle
type unit extends handle
native Player takes integer number returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native SetPlayerState takes player p, integer s, integer v returns nothing
function main takes nothing returns nothing
    call CreateUnit(Player(1), 'ohal', 0.0, 0.0, 0.0)
    call CreateUnit(Player(1), 'opeo', 0.0, 70.0, 0.0)
    call SetPlayerState(Player(1), 1, 100000)
    call SetPlayerState(Player(1), 2, 100000)
endfunction
"#;

/// `bldtm t_start steps clock_bits queue_now_bits`.
const EXPECTED: &str = include_str!("vectors/agile_countdowns.txt");

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn ty(name: &str, gold: i32, wood: i32, food_used: i32, food_made: i32, class: u32) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: F32::from_bits(0x4348_0000),
        gold_cost: gold,
        wood_cost: wood,
        food_used,
        food_made,
        classification: class,
        ..Default::default()
    }
}

struct Rig {
    host: ScriptHost,
    game: Rc<RefCell<Game>>,
    hall: ObjKey,
    peon: ObjKey,
}

fn rig(bldtm: i32, tables: &Tables) -> Rig {
    rig2(bldtm, bldtm, tables)
}

fn rig2(bldtm: i32, bldtm2: i32, tables: &Tables) -> Rig {
    let game = Rc::new(RefCell::new(Game::new()));
    {
        let mut g = game.borrow_mut();
        g.unit_types_mut().insert(HALL, ty("hall", 0, 0, 0, 10, 0x40));
        g.unit_types_mut().set_building(HALL, true);
        g.unit_types_mut().insert(PEON, ty("peon", 75, 0, 1, 0, 0));
        g.unit_types_mut().insert(GRUNT, ty("grunt", 100, 0, 1, 0, 0));
        g.unit_types_mut().insert(BARRACKS, ty("barracks", 200, 50, 0, 0, 0));
        g.unit_types_mut().set_building(BARRACKS, true);
        let p = g.production_mut();
        p.build_seconds.insert(PEON, bldtm);
        p.build_seconds.insert(GRUNT, bldtm2);
        p.build_seconds.insert(BARRACKS, bldtm);
        p.tech.insert(HALL, TypeTech { trains: vec![PEON, GRUNT], researches: vec![TECH], ..Default::default() });
        p.tech.insert(PEON, TypeTech { builds: vec![BARRACKS], ..Default::default() });
        p.upgrades.insert(
            TECH,
            UpgradeType { gold_base: 100, gold_mod: 50, lumber_base: 50, lumber_mod: 0, time_base: bldtm, time_mod: 0, max_level: 3 },
        );
        p.grid = Some(BuildGrid { width: 128, height: 128, origin_x: -2048.0, origin_y: -2048.0, types: vec![0; 128 * 128] });
    }
    let shared = ai::new_shared();
    let mut reg = NativeRegistry::with_builtins();
    register_main(&mut reg, &game, &shared);
    let module = compile(&[jass::parse(MAIN).expect("parse")]).expect("compile");
    let mut host = ScriptHost::with_game(module, reg, Some(tables.clone()), Rc::clone(&game)).expect("vm");
    host.start("main", &[]).expect("start");
    let (hall, peon) = {
        let g = game.borrow();
        let hall = g.unit_objects().find(|(_, u)| u.type_id == HALL).map(|(k, _)| k).expect("hall");
        let peon = g.unit_objects().find(|(_, u)| u.type_id == PEON).map(|(k, _)| k).expect("peon");
        (hall, peon)
    };
    Rig { host, game, hall, peon }
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn rows() -> Vec<(i32, u64, u64, u32, u32)> {
    EXPECTED
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f[0].parse().unwrap(), f[1].parse().unwrap(), f[2].parse().unwrap(), hex(f[3]), hex(f[4]))
        })
        .collect()
}

/// Steps until `done` says so; returns the ticks stepped and the clock bits after the last one.
fn run_until(r: &mut Rig, limit: u64, done: impl Fn(&Game) -> bool) -> (u64, u32) {
    for n in 1..=limit {
        r.host.step_ticks(1);
        if done(&r.game.borrow()) {
            return (n, r.host.clock().bits());
        }
    }
    panic!("not done after {limit} ticks");
}

#[test]
fn training_completes_on_the_original_tick() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    for (bldtm, t_start, steps, clock, _) in rows() {
        let mut r = rig(bldtm, &t);
        r.host.step_ticks(t_start);
        r.game.borrow_mut().train_unit(r.hall, PEON).expect("train");
        let (got, bits) = run_until(&mut r, steps + 10, |g| {
            g.production().events.iter().any(|e| matches!(e, ProdEvent::Trained { .. }))
        });
        assert_eq!((got, bits), (steps, clock), "bldtm {bldtm} started at tick {t_start}");
        n += 1;
    }
    assert!(n >= 30);
}

/// The countdown on the bare engine: ticks after the start until the listener fires.
fn standalone(bldtm: i32, t_start: u64, t: &Tables) -> u64 {
    let mut w = sim::agile::Agile::new();
    let inc = F32::from_bits(sim::lockstep::TICK_INCREMENT_BITS);
    for _ in 0..t_start {
        w.tick_idle(inc);
    }
    w.start_countdown(softfloat::conv::i2r(bldtm), F32::ONE, false, 0xD01C4, t);
    for n in 1..200_000u64 {
        if !w.tick(inc, t).is_empty() {
            return n;
        }
    }
    panic!("no completion");
}

#[test]
fn the_bare_engine_agrees_with_the_unicorn_table() {
    let Some(t) = tables() else { return };
    for (bldtm, t_start, steps, _, _) in rows() {
        assert_eq!(standalone(bldtm, t_start, &t), steps, "bldtm {bldtm} start {t_start}");
    }
}

#[test]
fn construction_completes_when_its_countdown_does() {
    let Some(t) = tables() else { return };
    for bldtm in [15, 30, 60] {
        let mut r = rig(bldtm, &t);
        r.game.borrow_mut().start_build_job(r.peon, BARRACKS, 600.0, 600.0).expect("build job");
        // The worker walks to the site first; the countdown starts when the construction does.
        let mut ticks = 0u64;
        loop {
            r.host.step_ticks(1);
            ticks += 1;
            assert!(ticks < 30_000, "the construction never starts");
            if r.game.borrow().production().events.iter().any(|e| matches!(e, ProdEvent::BuildStarted { .. })) {
                break;
            }
        }
        let (got, _) = run_until(&mut r, 100_000, |g| {
            g.production().events.iter().any(|e| matches!(e, ProdEvent::Built { .. }))
        });
        assert_eq!(got, standalone(bldtm, ticks, &t), "bldtm {bldtm} started at tick {ticks}");
    }
}

#[test]
fn research_and_upgrade_countdowns_run() {
    let Some(t) = tables() else { return };
    let mut r = rig(20, &t);
    r.game.borrow_mut().start_research(r.hall, TECH).expect("research");
    let (got, _) = run_until(&mut r, 5000, |g| g.tech_level(1, TECH) >= 1);
    assert_eq!(got, 4001, "20 s research completes at tick 4001 (Unicorn: cdvec bldtm 20)");
}

/// `bldtm1 bldtm2 t_start steps1 steps2 clock_bits2 queue_now2`: two heads in a row. The second countdown starts inside
/// the handler of the first (queue time = the firing time), so it can end one tick before a fresh start would.
const CHAIN: &str = include_str!("vectors/agile_chain.txt");

#[test]
fn a_second_head_starts_inside_the_first_completion() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    for l in CHAIN.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = l.split(' ').collect();
        let (b1, b2): (i32, i32) = (f[0].parse().unwrap(), f[1].parse().unwrap());
        let (t_start, s1, s2): (u64, u64, u64) = (f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap());
        let clock2 = hex(f[5]);
        let mut r = rig2(b1, b2, &t);
        r.host.step_ticks(t_start);
        r.game.borrow_mut().train_unit(r.hall, PEON).expect("train 1");
        r.game.borrow_mut().train_unit(r.hall, GRUNT).expect("train 2");
        let trained = |g: &Game, k: usize| {
            g.production().events.iter().filter(|e| matches!(e, ProdEvent::Trained { .. })).count() >= k
        };
        let (first, _) = run_until(&mut r, s1 + 10, |g| trained(g, 1));
        assert_eq!(first, s1, "first head {b1} {b2} start {t_start}");
        let (second, bits) = run_until(&mut r, s2 + 10, |g| trained(g, 2));
        assert_eq!((second, bits), (s2, clock2), "second head {b1} {b2} start {t_start}");
        n += 1;
    }
    assert!(n >= 30);
}

/// `length hours scale start ticks rate`, then `W tick clock` per wrap and `V tick value` checkpoints, `END`.
const TOD: &str = include_str!("vectors/agile_tod.txt");

#[test]
fn the_time_of_day_wraps_on_the_original_ticks() {
    let Some(t) = tables() else { return };
    let mut lines = TOD.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).peekable();
    let mut cases = 0;
    while let Some(head) = lines.next() {
        let f: Vec<&str> = head.split(' ').collect();
        let (length, hours, scale, start): (f32, f32, f32, f32) =
            (f[0].parse().unwrap(), f[1].parse().unwrap(), f[2].parse().unwrap(), f[3].parse().unwrap());
        let ticks: u64 = f[4].parse().unwrap();
        let mut waves = Vec::new();
        let mut values = Vec::new();
        for l in lines.by_ref() {
            if l == "END" {
                break;
            }
            let g: Vec<&str> = l.split(' ').collect();
            match g[0] {
                "W" => waves.push(g[1].parse::<u64>().unwrap()),
                "V" => values.push((g[1].parse::<u64>().unwrap(), hex(g[2]))),
                _ => panic!("{l}"),
            }
        }
        let game = Rc::new(RefCell::new(Game::new()));
        let shared = ai::new_shared();
        let mut reg = NativeRegistry::with_builtins();
        register_main(&mut reg, &game, &shared);
        let module = compile(&[jass::parse("function main takes nothing returns nothing
endfunction").unwrap()]).unwrap();
        let mut host = ScriptHost::with_game(module, reg, Some(t.clone()), Rc::clone(&game)).expect("vm");
        {
            let mut g = game.borrow_mut();
            g.set_day_cycle(sim::daynight::DayCycle {
                day_length: F32::from_bits(length.to_bits()),
                day_hours: F32::from_bits(hours.to_bits()),
                dawn: F32::from_bits(6.0f32.to_bits()),
                dusk: F32::from_bits(18.0f32.to_bits()),
            });
            g.set_time_of_day(F32::from_bits(start.to_bits()));
            g.start_time_of_day(F32::from_bits(scale.to_bits()));
        }
        assert_eq!(hex(f[5]), game.borrow().day_cycle().rate(F32::from_bits(scale.to_bits()), &t).0);
        let mut got_waves = Vec::new();
        let mut got_values = Vec::new();
        let mut wraps = 0;
        for tick in 0..ticks {
            host.step_ticks(1);
            let g = game.borrow();
            if g.time_of_day_wraps() != wraps {
                wraps = g.time_of_day_wraps();
                got_waves.push(tick);
            }
            if tick % 4000 == 0 || tick == ticks - 1 {
                got_values.push((tick, g.time_of_day().0));
            }
        }
        assert_eq!(got_waves, waves, "wraps for {head}");
        assert_eq!(got_values, values, "values for {head}");
        cases += 1;
    }
    assert!(cases >= 7);
}
