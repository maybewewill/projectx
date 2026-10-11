//! JASS timer natives through `ScriptHost` stepping (0.1 s per step). Expected values come from the
//! agile clock and the soft-float helpers, so they follow the rules in `natives/timer.rs`.

use jass::compile::compile;
use jass::vm::{NativeRegistry, Value};
use sim::lockstep::{TICKS_PER_BATCH, TICK_INCREMENT_BITS};
use sim::{AgileClock, ScriptHost};
use softfloat::{arith, F32};

/// Type and native declarations shared by the scripts of this file.
const PRELUDE: &str = r#"
type timer extends handle
type trigger extends handle
type boolexpr extends handle

native CreateTimer takes nothing returns timer
native DestroyTimer takes timer t returns nothing
native TimerStart takes timer t, real timeout, boolean periodic, code handlerFunc returns nothing
native TimerGetTimeout takes timer t returns real
native TimerGetElapsed takes timer t returns real
native TimerGetRemaining takes timer t returns real
native PauseTimer takes timer t returns nothing
native ResumeTimer takes timer t returns nothing
native GetExpiredTimer takes nothing returns timer
native GetHandleId takes handle h returns integer
native TriggerSleepAction takes real t returns nothing
"#;

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm")
}

fn real(host: &ScriptHost, name: &str) -> u32 {
    match host.vm().global(name) {
        Some(Value::Real(r)) => r.to_bits(),
        other => panic!("global {name} is {other:?}, not a real"),
    }
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn add(a: f32, b: f32) -> f32 {
    arith::add(F32::from_bits(a.to_bits()), F32::from_bits(b.to_bits())).to_f32()
}

fn sub(a: f32, b: f32) -> f32 {
    arith::sub(F32::from_bits(a.to_bits()), F32::from_bits(b.to_bits())).to_f32()
}

/// The agile clock after `ticks` game ticks: one soft-float add of the tick increment per tick
/// (`sim::lockstep::TICK_INCREMENT_BITS`, 0.005 s).
fn clock_at_tick(ticks: u64) -> f32 {
    let mut c = AgileClock::new();
    for _ in 0..ticks {
        c.advance(TICK_INCREMENT_BITS);
    }
    c.now()
}

/// The clock at the end of batch `k` (`TICKS_PER_BATCH` ticks each). An entry fires by the end of
/// batch `k` exactly when its expiry is at most this clock, because the clock only grows.
fn clock_after(k: usize) -> f32 {
    clock_at_tick(TICKS_PER_BATCH * k as u64)
}

/// The start time: Main runs in batch 1, at the boundary tick 20, so timers start from that clock.
fn start() -> f32 {
    clock_at_tick(TICKS_PER_BATCH)
}

/// The first batch (1-based) whose end clock reaches `expiry`. That is the batch that fires it.
fn step_of(expiry: f32) -> usize {
    (1..).find(|&k| clock_after(k) >= expiry).expect("expiry is reached")
}


#[test]
fn one_shot_timer_fires_at_the_step_its_expiry_reaches() {
    let src = r#"
globals
    integer gFired = 0
    integer gSame = 0
    timer gT = null
endglobals

function OnExpire takes nothing returns nothing
    set gFired = gFired + 1
    if GetExpiredTimer() == gT then
        set gSame = 1
    endif
endfunction

function Main takes nothing returns nothing
    set gT = CreateTimer()
    call TimerStart(gT, 0.25, false, function OnExpire)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    let expiry = add(start(), 0.25);
    let fire_step = step_of(expiry);
    for k in 1..=10 {
        host.step();
        let expect = if k >= fire_step { 1 } else { 0 };
        assert_eq!(int(&host, "gFired"), expect, "after step {k}");
    }
    // GetExpiredTimer inside the handler is the timer itself.
    assert_eq!(int(&host, "gSame"), 1);
}

#[test]
fn periodic_timer_rearms_at_expiry_plus_period() {
    let src = r#"
globals
    integer gTicks = 0
endglobals

function OnTick takes nothing returns nothing
    set gTicks = gTicks + 1
endfunction

function Main takes nothing returns nothing
    local timer t = CreateTimer()
    call TimerStart(t, 0.2, true, function OnTick)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    // Expiries: e0 = now + period, then e_{n+1} = e_n + period (soft-float adds).
    let period = 0.2f32;
    let mut expiries = Vec::new();
    let mut e = add(start(), period);
    for _ in 0..20 {
        expiries.push(e);
        e = add(e, period);
    }
    for k in 1..=40 {
        host.step();
        let clock = clock_after(k);
        let expect = expiries.iter().filter(|&&x| x <= clock).count() as i32;
        assert_eq!(int(&host, "gTicks"), expect, "step {k}");
    }
    assert!(int(&host, "gTicks") >= 15);
}

#[test]
fn elapsed_and_remaining_are_softfloat_differences() {
    // Main starts a 1.0 s timer, sleeps 0.5 s, then reads the getters at the wake time.
    let src = r#"
globals
    real gElapsed = 0.0
    real gRemaining = 0.0
    real gTimeout = 0.0
endglobals

function Main takes nothing returns nothing
    local timer t = CreateTimer()
    call TimerStart(t, 1.0, false, null)
    call TriggerSleepAction(0.5)
    set gElapsed = TimerGetElapsed(t)
    set gRemaining = TimerGetRemaining(t)
    set gTimeout = TimerGetTimeout(t)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    for _ in 0..10 {
        host.step();
    }
    let expiry = add(start(), 1.0);
    let wake = add(start(), 0.5);
    let remaining = sub(expiry, wake);
    let elapsed = sub(1.0, remaining);
    assert_eq!(real(&host, "gTimeout"), 1.0f32.to_bits());
    assert_eq!(real(&host, "gRemaining"), remaining.to_bits());
    assert_eq!(real(&host, "gElapsed"), elapsed.to_bits());
}

#[test]
fn pause_freezes_remaining_and_resume_restarts_it() {
    let src = r#"
globals
    integer gFired = 0
    real gPaused = 0.0
    real gAfterPause = 0.0
    real gResumed = 0.0
    timer gT = null
endglobals

function OnExpire takes nothing returns nothing
    set gFired = gFired + 1
endfunction

function Main takes nothing returns nothing
    set gT = CreateTimer()
    call TimerStart(gT, 1.0, false, function OnExpire)
    call TriggerSleepAction(0.3)
    call PauseTimer(gT)
    set gPaused = TimerGetRemaining(gT)
    call TriggerSleepAction(1.0)
    set gAfterPause = TimerGetRemaining(gT)
    call ResumeTimer(gT)
    set gResumed = TimerGetRemaining(gT)
    call TriggerSleepAction(5.0)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    let expiry0 = add(start(), 1.0);
    let wake1 = add(start(), 0.3);
    let remaining1 = sub(expiry0, wake1);
    let wake2 = add(wake1, 1.0);
    // After the resume at wake2 the timer expires at wake2 + remaining1.
    let expiry1 = add(wake2, remaining1);
    let fire_step = step_of(expiry1);
    let mut first_fire = None;
    for k in 1..=80 {
        host.step();
        if int(&host, "gFired") == 1 && first_fire.is_none() {
            first_fire = Some(k);
        }
    }
    assert_eq!(real(&host, "gPaused"), remaining1.to_bits());
    assert_eq!(real(&host, "gAfterPause"), remaining1.to_bits());
    assert_eq!(real(&host, "gResumed"), sub(expiry1, wake2).to_bits());
    assert_eq!(first_fire, Some(fire_step));
    assert_eq!(int(&host, "gFired"), 1);
}

#[test]
fn destroy_releases_ids_lifo_and_destroyed_handles_are_inert() {
    let src = r#"
globals
    integer gIdA = 0
    integer gIdB = 0
    integer gIdC = 0
    integer gIdD = 0
    integer gIdE = 0
    integer gTicks = 0
    real gTimeoutAfter = 9.0
endglobals

function OnTick takes nothing returns nothing
    set gTicks = gTicks + 1
endfunction

function Main takes nothing returns nothing
    local timer a = CreateTimer()
    local timer b = CreateTimer()
    local timer c
    local timer d
    local timer e
    set gIdA = GetHandleId(a)
    set gIdB = GetHandleId(b)
    call TimerStart(a, 0.1, false, function OnTick)
    call DestroyTimer(a)
    call DestroyTimer(b)
    set gTimeoutAfter = TimerGetTimeout(a)
    call TimerStart(a, 0.1, false, function OnTick)
    set c = CreateTimer()
    set gIdC = GetHandleId(c)
    set d = CreateTimer()
    set gIdD = GetHandleId(d)
    set e = CreateTimer()
    set gIdE = GetHandleId(e)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    for _ in 0..10 {
        host.step();
    }
    // The first timer is 0x100001, the next fresh id is 0x100002 and so on. Released ids are
    // reused most recently released first: b (0x100002) was released last, so it comes back first.
    assert_eq!(int(&host, "gIdA"), 0x100001);
    assert_eq!(int(&host, "gIdB"), 0x100002);
    assert_eq!(int(&host, "gIdC"), 0x100002);
    assert_eq!(int(&host, "gIdD"), 0x100001);
    assert_eq!(int(&host, "gIdE"), 0x100003);
    // A destroyed timer: TimerStart did nothing, so its handler never ran; its getters read 0.
    assert_eq!(int(&host, "gTicks"), 0);
    assert_eq!(real(&host, "gTimeoutAfter"), 0.0f32.to_bits());
}

#[test]
fn a_timer_handle_started_twice_runs_the_latest_timeout_only() {
    let src = r#"
globals
    integer gFired = 0
endglobals

function OnExpire takes nothing returns nothing
    set gFired = gFired + 1
endfunction

function Main takes nothing returns nothing
    local timer t = CreateTimer()
    call TimerStart(t, 0.1, false, function OnExpire)
    call TimerStart(t, 0.5, false, function OnExpire)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    let fire_step = step_of(add(start(), 0.5));
    for k in 1..=10 {
        host.step();
        let expect = i32::from(k >= fire_step);
        assert_eq!(int(&host, "gFired"), expect, "step {k}");
    }
}

#[test]
fn negative_timeout_is_clamped_to_zero_and_fires_at_once() {
    let src = r#"
globals
    integer gFired = 0
endglobals

function OnExpire takes nothing returns nothing
    set gFired = gFired + 1
endfunction

function Main takes nothing returns nothing
    local timer t = CreateTimer()
    call TimerStart(t, -2.0, false, function OnExpire)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    // Main runs in step 1, after that step's timer pump. The zero-length timer is due at once, so
    // it fires in step 2 (the first pump after it was started).
    host.step();
    assert_eq!(int(&host, "gFired"), 0);
    host.step();
    assert_eq!(int(&host, "gFired"), 1);
}
