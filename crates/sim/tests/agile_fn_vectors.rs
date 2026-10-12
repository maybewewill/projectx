//! Per-function vectors of the agile engine, recorded from the original routines in Unicorn
//! (`tools/emu/agile_engine_emu.py fnvec`, docs/spec/agile.md): property value `0x6F4773A0`, slope add `0x6F4A9320`,
//! listener value decision `0x6F4A9C70`, jump decision `0x6F4A9CF0` (with the crossing detector `0x6F4A9940`) and
//! crossing time `0x6F4A8FD0`. Needs `GAME_DLL` for the reciprocal table.

use sim::agile::{Agile, PropId};
use softfloat::{Tables, F32};

const EVAL: &str = include_str!("vectors/agile_fn_eval.tsv");
const SLOPE: &str = include_str!("vectors/agile_fn_slope.tsv");
const DECIDE: &str = include_str!("vectors/agile_fn_decide.tsv");
const JUMP: &str = include_str!("vectors/agile_fn_jump.tsv");
const CROSS: &str = include_str!("vectors/agile_fn_cross.tsv");
const MODIFIER: &str = include_str!("vectors/agile_fn_mod.tsv");

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

/// The common prefix: clock, row, t0, t0 row, base, slope, lo, hi.
fn world(f: &[&str]) -> (Agile, PropId) {
    let mut w = Agile::new();
    w.set_clock(F32(hex(f[0])), f[1].parse().unwrap());
    let p = w.property_raw(F32(hex(f[4])), F32(hex(f[5])), F32(hex(f[6])), F32(hex(f[7])), F32(hex(f[2])), f[3].parse().unwrap());
    (w, p)
}

#[test]
fn value_matches_the_original() {
    let Some(_) = tables() else { return };
    let mut n = 0;
    for line in EVAL.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (w, p) = world(&f);
        assert_eq!(w.value(p).0, hex(f[8]), "{line}");
        n += 1;
    }
    assert!(n >= 500);
}

#[test]
fn slope_add_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    for line in SLOPE.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (mut w, p) = world(&f);
        w.apply_rate(p, F32(hex(f[8])), &t);
        let (b, s, t0, row) = w.property_state(p);
        assert_eq!((b.0, s.0, t0.0, row), (hex(f[9]), hex(f[10]), hex(f[11]), f[12].parse().unwrap()), "{line}");
        n += 1;
    }
    assert!(n >= 500);
}

/// Checks the result columns of the listener decision vectors starting at `f[r]`:
/// fired count, fire time, timer (expiry, seq, flags, period) or "-", stand-in cancelled.
fn check_result(w: &mut Agile, l: sim::agile::LisId, f: &[&str], r: usize, line: &str) {
    let events = w.take_events();
    assert_eq!(events.len().to_string(), f[r], "fired: {line}");
    if let Some(e) = events.first() {
        assert_eq!(e.at.0, hex(f[r + 1]), "fire time: {line}");
    }
    match w.listener_timer(l) {
        Some((exp, seq, periodic, period)) => {
            assert_eq!(exp.0, hex(f[r + 2]), "expiry: {line}");
            assert_eq!(seq.to_string(), f[r + 3], "seq: {line}");
            assert_eq!(if periodic { "20001" } else { "20000" }, f[r + 4], "flags: {line}");
            assert_eq!(period.0, hex(f[r + 5]), "period: {line}");
        }
        None => assert_eq!(f[r + 2], "-", "no timer expected: {line}"),
    }
    assert_eq!(if w.raw_timer_cancelled(l) { "1" } else { "0" }, f[r + 6], "stand-in: {line}");
}

#[test]
fn listener_value_decision_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    let mut fired = 0;
    let mut armed = 0;
    for line in DECIDE.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (mut w, p) = world(&f);
        let l = w.listener_raw(p, f[9].parse().unwrap(), F32(hex(f[8])), 0xD01C4, f[11] == "1");
        w.evaluate(l, F32(hex(f[10])), &t);
        if f[12] != "0" {
            fired += 1;
        }
        if f[14] != "-" {
            armed += 1;
        }
        check_result(&mut w, l, &f, 12, line);
        n += 1;
    }
    assert!(n >= 500 && fired >= 30 && armed >= 30, "{n} {fired} {armed}");
}

#[test]
fn listener_jump_decision_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    let mut fired = 0;
    let mut armed = 0;
    for line in JUMP.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (mut w, p) = world(&f);
        let l = w.listener_raw(p, f[9].parse().unwrap(), F32(hex(f[8])), 0xD01C4, f[12] == "1");
        w.jump(l, F32(hex(f[11])), F32(hex(f[10])), &t);
        // columns: thr, mode, new, old, has_timer, result...
        if f[13] != "0" {
            fired += 1;
        }
        if f[15] != "-" {
            armed += 1;
        }
        check_result(&mut w, l, &f, 13, line);
        n += 1;
    }
    assert!(n >= 500 && fired >= 30 && armed >= 30, "{n} {fired} {armed}");
}

#[test]
fn crossing_time_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    for line in CROSS.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (w, p) = world(&f);
        assert_eq!(w.crossing_time(p, F32(hex(f[8])), &t).0, hex(f[9]), "{line}");
        n += 1;
    }
    assert!(n >= 500);
}

#[test]
fn modifier_creation_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut n = 0;
    let mut timers = 0;
    for line in MODIFIER.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (mut w, p) = world(&f);
        let m = w.add_modifier(p, F32(hex(f[8])), F32(hex(f[9])), &t);
        let (b, s, t0, row) = w.property_state(p);
        assert_eq!(
            (b.0, s.0, t0.0, row),
            (hex(f[10]), hex(f[11]), hex(f[12]), f[13].parse().unwrap()),
            "{line}"
        );
        match w.modifier_timer(m) {
            Some((exp, seq, periodic, period)) => {
                timers += 1;
                assert_eq!(exp.0, hex(f[14]), "expiry: {line}");
                assert_eq!(seq.to_string(), f[15], "seq: {line}");
                assert_eq!(if periodic { "20001" } else { "20000" }, f[16], "flags: {line}");
                assert_eq!(period.0, hex(f[17]), "period: {line}");
            }
            None => assert_eq!(f[14], "-", "{line}"),
        }
        n += 1;
    }
    assert!(n >= 500 && timers >= 400, "{n} {timers}");
}
