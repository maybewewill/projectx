//! The movement walk's cadence (`docs/spec/lockstep.md` section 11.2): the walk is a repeating timer of
//! period `MOVE_PERIOD_BITS` (0.03 s of agile clock), re-armed at soft-float `expiry + period`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::lockstep::{MOVE_PERIOD_BITS, TICK_INCREMENT_BITS};
use sim::{AgileClock, ScriptHost};
use softfloat::{arith, text, Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn host() -> ScriptHost {
    let prog =
        jass::parse("function Main takes nothing returns nothing\nendfunction\n").expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm")
}

fn sf(bits: u32) -> F32 {
    F32::from_bits(bits)
}

/// The first `n` movement expiries: `e_1 = 0 + period`, `e_{k+1} = e_k + period`, all soft-float adds.
fn expiries(n: usize) -> Vec<f32> {
    let period = sf(MOVE_PERIOD_BITS);
    let mut out = Vec::with_capacity(n);
    let mut e = arith::add(F32::ZERO, period);
    for _ in 0..n {
        out.push(e.to_f32());
        e = arith::add(e, period);
    }
    out
}

#[test]
fn movement_walk_fires_at_each_soft_float_expiry_of_the_period() {
    let mut h = host();
    let exp = expiries(200);
    let mut clock = AgileClock::new();
    for tick in 1..=600u64 {
        clock.advance(TICK_INCREMENT_BITS);
        h.step_ticks(1);
        let now = clock.now();
        let due = exp.iter().take_while(|&&e| e <= now).count();
        assert_eq!(
            h.game().movement_calls,
            due as u64,
            "walk count at tick {tick} (clock {now})"
        );
    }
}

#[test]
fn movement_walk_runs_about_every_six_ticks() {
    let mut h = host();
    h.step_ticks(600);
    // 600 ticks are 3.0 s of agile clock: one walk per 0.03 s, so about 100 walks.
    let calls = h.game().movement_calls;
    assert!((99..=101).contains(&calls), "walks in 600 ticks: {calls}");
}

#[test]
fn movement_is_armed_once_per_game() {
    // A second host on the same game does not arm a second movement timer.
    let src = "function Main takes nothing returns nothing\nendfunction\n";
    let module = compile(&[jass::parse(src).expect("parse")]).expect("compile");
    let first = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    let module2 = compile(&[jass::parse(src).expect("parse")]).expect("compile");
    let second = ScriptHost::with_game(
        module2,
        NativeRegistry::with_builtins(),
        tables(),
        first.game_handle(),
    )
    .expect("vm");
    drop(second);
    let mut first = first;
    first.step_ticks(600);
    let calls = first.game().movement_calls;
    assert!(
        (99..=101).contains(&calls),
        "one timer: {calls} walks in 600 ticks"
    );
}

#[test]
fn movement_period_is_the_text_0_03_in_s2r() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: skipping the S2R check of the movement period");
        return;
    };
    // The static initializer 0x6F85F650 converts the literal at 0x6F942DD8 ("0.03") with S2R.
    let v = text::s2r(Some(&t), b"0.03").expect("s2r");
    assert_eq!(
        v.to_bits(),
        MOVE_PERIOD_BITS,
        "S2R(\"0.03\") is the movement period"
    );
    assert_eq!(MOVE_PERIOD_BITS, 0x3CF5_C290);
}
