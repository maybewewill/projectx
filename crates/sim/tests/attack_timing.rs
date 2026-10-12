//! The attack timing routines (speed multiplier, cycle delay, swing scheduling, backswing, speed bonus rescaling)
//! against the original: 600 Unicorn vectors each (`tools/emu/attack_timing_emu.py`). Needs `GAME_DLL` for the
//! soft-float tables.

#[path = "attack_timing/attack_timing_back.rs"]
mod back;
#[path = "attack_timing/attack_timing_bonus.rs"]
mod bonus;
#[path = "attack_timing/attack_timing_cycle.rs"]
mod cycle;
#[path = "attack_timing/attack_timing_mult.rs"]
mod mult;
#[path = "attack_timing/attack_timing_sched.rs"]
mod sched;

use sim::attack_timing::*;
use softfloat::{Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn b(x: u32) -> F32 {
    F32::from_bits(x)
}

/// The weapon index word `w` (0xFFFFFFFF = none) and its mode word decide whether the multiplier is bypassed.
fn ignored(w: u32, mode: u32) -> bool {
    w != 0xFFFF_FFFF && mode_ignores_speed(mode)
}

fn offset(t: Timer) -> u32 {
    match t {
        Timer::Cooldown => 0x1D8,
        Timer::DamagePoint => 0x1EC,
        Timer::Backswing => 0x200,
    }
}

fn msg(t: Timer) -> u32 {
    match t {
        Timer::Cooldown => MSG_COOLDOWN_END,
        Timer::DamagePoint => MSG_DAMAGE_POINT,
        Timer::Backswing => MSG_BACKSWING_END,
    }
}

/// The event words of the vector: count, then (offset, message or 0xFFFFFFFF, delay bits) x 3.
fn event_words(cmds: &[Cmd]) -> [u32; 10] {
    let mut w = [0u32; 10];
    w[0] = cmds.len() as u32;
    for (i, c) in cmds.iter().enumerate() {
        let (o, m, d) = match *c {
            Cmd::Start { timer, seconds } => (offset(timer), msg(timer), seconds.to_bits()),
            Cmd::Stop { timer } => (offset(timer), 0xFFFF_FFFF, 0),
        };
        w[1 + 3 * i] = o;
        w[2 + 3 * i] = m;
        w[3 + 3 * i] = d;
    }
    w
}

#[test]
fn multiplier_matches_the_original() {
    let Some(_t) = tables() else { return };
    let (mut clamped_lo, mut clamped_hi, mut bypass, mut slow) = (0, 0, 0, 0);
    for (i, r) in mult::ROWS.iter().enumerate() {
        let speed = Speed { base: b(r[0]), slow: b(r[1]) };
        let ig = ignored(r[3], r[2]);
        let m = multiplier(speed, ig, r[4] != 0);
        assert_eq!(m.to_bits(), r[5], "row {i}: {r:08X?}");
        clamped_lo += (m == MULT_MIN) as usize;
        clamped_hi += (m == MULT_MAX) as usize;
        bypass += ig as usize;
        slow += (r[4] != 0 && b(r[1]).to_f32() < -0.001) as usize;
    }
    assert!(clamped_lo > 10 && clamped_hi > 10 && bypass > 20 && slow > 50);
}

#[test]
fn cycle_delay_matches_the_original() {
    let Some(t) = tables() else { return };
    for (i, r) in cycle::ROWS.iter().enumerate() {
        let speed = Speed { base: b(r[0]), slow: b(r[1]) };
        let d = cycle_delay(speed, ignored(r[3], r[2]), b(r[4]), &t);
        assert_eq!(d.to_bits(), r[5], "row {i}: {r:08X?}");
    }
}

#[test]
fn schedule_matches_the_original() {
    let Some(t) = tables() else { return };
    let (mut none, mut stop, mut both) = (0, 0, 0);
    for (i, r) in sched::ROWS.iter().enumerate() {
        let speed = Speed { base: b(r[0]), slow: b(r[1]) };
        let cmds = schedule(speed, ignored(r[3], r[2]), b(r[4]), b(r[5]), r[6] != 0, b(r[7]), &t);
        assert_eq!(event_words(&cmds)[..], r[8..18], "row {i}: {r:08X?}");
        none += cmds.is_empty() as usize;
        stop += matches!(cmds.first(), Some(Cmd::Stop { .. })) as usize;
        both += (cmds.len() == 2) as usize;
    }
    assert!(none > 20 && both > 100, "none {none} stop {stop} both {both}");
}

#[test]
fn backswing_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut restarted = 0;
    for (i, r) in back::ROWS.iter().enumerate() {
        let speed = Speed { base: b(r[0]), slow: b(r[1]) };
        let cmds = backswing(speed, ignored(r[3], r[2]), b(r[4]), b(r[5]), &t);
        assert_eq!(event_words(&cmds)[..], r[6..16], "row {i}: {r:08X?}");
        restarted += (cmds.len() == 2) as usize;
    }
    assert!(restarted > 20);
}

#[test]
fn speed_bonus_matches_the_original() {
    let Some(t) = tables() else { return };
    let mut rescaled = 0;
    for (i, r) in bonus::ROWS.iter().enumerate() {
        let speed = Speed { base: b(r[0]), slow: b(r[1]) };
        let pending = Pending { cooldown: b(r[4]), damage_point: b(r[5]), backswing: b(r[6]) };
        let (s, cmds) = add_bonus(speed, b(r[2]), r[3] != 0, pending, &t);
        assert_eq!((s.base.to_bits(), s.slow.to_bits()), (r[7], r[8]), "row {i}: {r:08X?}");
        assert_eq!(event_words(&cmds)[..], r[9..19], "row {i}: {r:08X?}");
        rescaled += !cmds.is_empty() as usize;
    }
    assert!(rescaled > 100);
}
