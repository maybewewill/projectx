//! Tests for the agile clock, the timer heap and the JASS sleep bridge (`docs/spec/game-loop-2.md`).

use jass::compile::compile;
use jass::vm::{NativeRegistry, Value};
use softfloat::{arith, Tables, F32};

/// Soft-float add on raw bit patterns (the `softfloat` crate's `arith::add`).
fn sfadd(x: u32, y: u32) -> u32 {
    arith::add(F32::from_bits(x), F32::from_bits(y)).to_bits()
}

use sim::lockstep::{TICKS_PER_BATCH, TICK_INCREMENT_BITS};
use sim::{AgileClock, ScriptHost, TimerQueue, STEP, STEP_BITS};

/// Pump with a closure that records `(payload, expiry bits, now during callback bits)`.
fn fire_all(q: &mut TimerQueue<u32>, target: f32) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    q.pump(target, |q, ev| {
        out.push((ev.payload, ev.expiry.to_bits(), q.now().to_bits()));
    });
    out
}

#[test]
fn clock_accumulation_is_pinned() {
    // Values from the truncating soft-float add (UNVERIFIED against the original clock, which is
    // only known to be 0.1 s per step).
    let mut c = AgileClock::new();
    let expected = [
        0x3DCC_CCCD,
        0x3E4C_CCCD,
        0x3E99_9999,
        0x3ECC_CCCC,
        0x3EFF_FFFF,
        0x3F19_9999,
    ];
    for bits in expected {
        c.advance_step();
        assert_eq!(c.bits(), bits);
    }
    // Drift is downward: after 10 steps the clock is slightly under 1.0.
    for _ in 0..4 {
        c.advance_step();
    }
    assert!(c.now() < 1.0 && c.now() > 0.999);
}

#[test]
fn step_constant_is_f32_point_one() {
    assert_eq!(STEP_BITS, 0x3DCC_CCCD);
    assert_eq!(STEP, 0.1f32);
}

#[test]
fn ordering_is_by_expiry_then_fifo() {
    let mut q = TimerQueue::new();
    q.start(0.5, false, 1u32); // A
    q.start(0.2, false, 2); // B
    q.start(0.5, false, 3); // C, same expiry as A, started later
    q.start(0.2, false, 4); // D, same expiry as B, started later
    q.start(0.0, false, 5); // E
    let order: Vec<u32> = fire_all(&mut q, 1.0).iter().map(|e| e.0).collect();
    assert_eq!(order, vec![5, 2, 4, 1, 3]);
    assert!(q.is_empty());
}

#[test]
fn fifo_holds_for_many_equal_expiries() {
    let mut q = TimerQueue::new();
    for i in 0..200u32 {
        q.start(0.3, false, i);
    }
    let order: Vec<u32> = fire_all(&mut q, 0.3).iter().map(|e| e.0).collect();
    assert_eq!(order, (0..200).collect::<Vec<_>>());
}

#[test]
fn short_timeouts_are_clamped_to_the_minimum() {
    // `0x6F477F80`: timeouts strictly below the runtime float at 0x6FAAE478 (1e-4) become 1e-4.
    let min = f32::from_bits(sim::timer::MIN_TIMEOUT_BITS);
    let mut q = TimerQueue::new();
    q.start(-2.5, false, 7u32);
    q.start(f32::NAN, false, 8);
    q.start(0.0, false, 9);
    assert_eq!(q.next_expiry(), Some(min));
    assert!(fire_all(&mut q, 0.0).is_empty());
    let fired = fire_all(&mut q, min);
    assert_eq!(fired.len(), 3);
    for f in &fired {
        assert_eq!(f.1, sim::timer::MIN_TIMEOUT_BITS);
    }
}

#[test]
fn now_is_the_expiry_during_the_callback_and_target_after() {
    let mut q = TimerQueue::new();
    q.start(0.3, false, 1u32);
    q.start(0.7, false, 2);
    let fired = fire_all(&mut q, 1.0);
    assert_eq!(fired[0].2, 0.3f32.to_bits());
    assert_eq!(fired[1].2, 0.7f32.to_bits());
    assert_eq!(q.now(), 1.0);
}

#[test]
fn timers_started_in_a_callback_are_relative_to_its_expiry() {
    let mut q: TimerQueue<u32> = TimerQueue::new();
    q.start(0.3, false, 1);
    let mut seen = Vec::new();
    q.pump(1.0, |q, ev| {
        seen.push((ev.payload, q.now().to_bits()));
        if ev.payload == 1 {
            // Relative to the callback time 0.3, not to the pump target 1.0.
            q.start(0.1, false, 2);
        }
    });
    // Callback of 2 runs at expiry 0.3 + 0.1 in soft-float, which is about 0.4.
    let expected = sfadd(0.3f32.to_bits(), 0.1f32.to_bits());
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1], (2, expected));
    assert!((f32::from_bits(expected) - 0.4).abs() < 1e-6);
}

#[test]
fn pump_before_now_is_a_no_op() {
    let mut q = TimerQueue::new();
    q.start(0.5, false, 1u32);
    fire_all(&mut q, 0.5);
    assert_eq!(q.now(), 0.5);
    q.start(0.5, false, 2);
    assert!(fire_all(&mut q, 0.2).is_empty());
    assert_eq!(q.now(), 0.5);
    assert_eq!(q.len(), 1);
}

#[test]
fn periodic_timer_rearms_at_expiry_plus_period() {
    let mut q = TimerQueue::new();
    q.start(0.25, true, 9u32);
    let fired = fire_all(&mut q, 1.0);
    let expiries: Vec<f32> = fired.iter().map(|e| f32::from_bits(e.1)).collect();
    assert_eq!(expiries, vec![0.25, 0.5, 0.75, 1.0]);
    assert_eq!(q.len(), 1, "still armed for 1.25");
    assert_eq!(q.next_expiry(), Some(1.25));
}

#[test]
fn periodic_timer_can_be_cancelled_from_its_callback() {
    let mut q: TimerQueue<u32> = TimerQueue::new();
    let id = q.start(0.1, true, 3);
    let mut count = 0;
    q.pump(1.0, |q, _| {
        count += 1;
        if count == 2 {
            assert!(q.cancel(id));
        }
    });
    assert_eq!(count, 2);
    assert!(q.is_empty());
}

#[test]
fn zero_period_periodic_timer_runs_at_the_minimum_period() {
    // A zero period is clamped to 1e-4, so the timer fires about 10,000 times per second
    // of clock and a pump always terminates.
    let mut q: TimerQueue<u32> = TimerQueue::new();
    q.start(0.0, true, 4);
    let mut count = 0;
    q.pump(0.5, |_, _| count += 1);
    assert!((4_000..=6_000).contains(&count), "{count}");
    let first = count;
    q.pump(0.5, |_, _| count += 1);
    assert_eq!(count, first);
}

#[test]
fn cancel_removes_only_the_named_timer() {
    let mut q: TimerQueue<u32> = TimerQueue::new();
    let a = q.start(0.2, false, 1);
    let _b = q.start(0.1, false, 2);
    let c = q.start(0.3, false, 3);
    assert!(q.cancel(a));
    assert!(!q.cancel(a), "second cancel finds nothing");
    assert!(q.cancel(c));
    let order: Vec<u32> = fire_all(&mut q, 1.0).iter().map(|e| e.0).collect();
    assert_eq!(order, vec![2]);
}

/// Pseudo-random sequence (xorshift32) for building inputs. Deterministic.
fn xorshift(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn run_many(seed: u32) -> Vec<(u32, u32)> {
    let mut rng = seed;
    let mut q: TimerQueue<u32> = TimerQueue::new();
    for i in 0..3000u32 {
        // Quantised timeouts (multiples of 0.1 up to 5 s) give many equal expiries.
        let t = (xorshift(&mut rng) % 50) as f32 * 0.1;
        let periodic = xorshift(&mut rng).is_multiple_of(7);
        if periodic {
            q.start(t.max(0.05), true, i);
        } else {
            q.start(t, false, i);
        }
    }
    let mut out = Vec::new();
    for target in [0.5f32, 1.3, 2.0, 3.7, 6.0] {
        q.pump(target, |_, ev| out.push((ev.payload, ev.expiry.to_bits())));
    }
    out
}

#[test]
fn many_timers_fire_in_the_same_order_every_run() {
    let a = run_many(0x1234_5678);
    let b = run_many(0x1234_5678);
    assert_eq!(a, b);
    assert!(a.len() > 1000);
    // Expiries never go backwards across the whole run (each pump is ordered and targets increase).
    for w in a.windows(2) {
        assert!(f32::from_bits(w[0].1) <= f32::from_bits(w[1].1));
    }
}

#[test]
fn many_one_shot_timers_fire_sorted_by_expiry_then_insertion() {
    let mut rng = 0xDEAD_BEEFu32;
    let mut q: TimerQueue<u32> = TimerQueue::new();
    let mut expect = Vec::new();
    for i in 0..2000u32 {
        let t = (xorshift(&mut rng) % 40) as f32 * 0.1;
        q.start(t, false, i);
        let t = t.max(f32::from_bits(sim::timer::MIN_TIMEOUT_BITS));
        expect.push((t.to_bits(), i));
    }
    expect.sort_by(|x, y| {
        f32::from_bits(x.0)
            .partial_cmp(&f32::from_bits(y.0))
            .unwrap()
            .then(x.1.cmp(&y.1))
    });
    let got: Vec<(u32, u32)> = fire_all(&mut q, 10.0).iter().map(|e| (e.1, e.0)).collect();
    assert_eq!(got, expect);
}

// ---------------------------------------------------------------------------------------------
// JASS: TriggerSleepAction through ScriptHost
// ---------------------------------------------------------------------------------------------

const SLEEP_SCRIPT: &str = r#"
globals
    integer gDone = 0
endglobals

native TriggerSleepAction takes real t returns nothing

function Main takes nothing returns nothing
    call TriggerSleepAction(0.25)
    set gDone = 1
endfunction
"#;

/// The soft-float lookup tables of the user's `Game.dll` (`GAME_DLL`), or `None` when it is unset.
fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

/// A real value from an `f32` literal (its bit pattern, as the VM stores it).
fn real_val(x: f32) -> Value {
    Value::Real(F32::from_bits(x.to_bits()))
}

fn host_for(src: &str) -> ScriptHost {
    let prog = jass::parse(src).expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm")
}

/// The agile clock after `ticks` game ticks: one soft-float add of the tick increment per tick.
fn clock_at_tick(ticks: u64) -> f32 {
    let mut c = AgileClock::new();
    for _ in 0..ticks {
        c.advance(TICK_INCREMENT_BITS);
    }
    c.now()
}

#[test]
fn trigger_sleep_resumes_at_the_expected_step() {
    // Main starts at once, at tick 0 (clock 0.0): `start` no longer waits for a batch (lockstep.md
    // section 12.1). Its sleep of 0.25 s is due at the soft-float `0 + 0.25`; the thread resumes in the
    // pump of the first tick whose clock reaches that value, not at a batch boundary.
    let mut host = host_for(SLEEP_SCRIPT);
    host.start("Main", &[]).expect("start");
    let until = f32::from_bits(sfadd(clock_at_tick(0).to_bits(), 0.25f32.to_bits()));
    let want_tick = (1..)
        .find(|&t| clock_at_tick(t) >= until)
        .expect("resume within the search range");
    assert!(clock_at_tick(want_tick - 1) < until, "the tick before does not reach it");

    let mut resumed_tick = None;
    for _ in 0..200u64 {
        let fired = host.step_ticks(1);
        let done = host.vm().global("gDone").expect("gDone");
        if host.ticks() == 1 {
            assert_eq!(fired, 0, "no timer before the first sleep is due");
        }
        if done == Value::Integer(1) {
            resumed_tick = Some((host.ticks(), fired));
            break;
        }
    }
    let (tick, fired) = resumed_tick.expect("thread resumed");
    assert_eq!(tick, want_tick, "resumes on the first tick that reaches the wake time");
    assert_eq!(fired, 1, "one sleep entry fires in that tick");
    // The thread ran at the expiry: the VM read the wake time as the game time.
    assert!(until <= clock_at_tick(tick));
}

#[test]
fn per_tick_clock_is_twenty_soft_adds_per_batch() {
    // One batch is 20 ticks of 0.005 s. The clock is the fold of soft-float adds, never 0.1 in one add.
    let mut host = host_for(SLEEP_SCRIPT);
    host.step_ticks(1);
    assert_eq!(host.ticks(), 1);
    assert_eq!(host.clock().bits(), TICK_INCREMENT_BITS);
    host.step_ticks(19);
    assert_eq!(host.ticks(), 20);
    assert!(host.at_batch_boundary());
    let mut bits = 0u32;
    for _ in 0..20 {
        bits = sfadd(bits, TICK_INCREMENT_BITS);
    }
    assert_eq!(host.clock().bits(), bits);
    assert_ne!(host.clock().bits(), 0x3DCC_CCCD, "not the 0.1 s step");
    // The batch ran at tick 20: the VM reads the batch-end clock.
    assert_eq!(host.vm().now().to_bits(), bits);
}

#[test]
fn point_one_second_timer_fires_on_the_first_tick_whose_clock_reaches_expiry() {
    let src = r#"
type timer extends handle
native CreateTimer takes nothing returns timer
native TimerStart takes timer t, real timeout, boolean periodic, code handlerFunc returns nothing

globals
    integer gFired = 0
endglobals

function OnExpire takes nothing returns nothing
    set gFired = gFired + 1
endfunction

function Main takes nothing returns nothing
    local timer t = CreateTimer()
    call TimerStart(t, 0.1, false, function OnExpire)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    // Main starts at tick 0 (clock 0.0), so the expiry is the soft-float `0 + 0.1`.
    let expiry = f32::from_bits(sfadd(clock_at_tick(0).to_bits(), 0.1f32.to_bits()));
    // The first tick whose clock reaches the expiry.
    let want = (1..)
        .find(|&t| clock_at_tick(t) >= expiry)
        .expect("expiry reached");
    assert!(clock_at_tick(want - 1) < expiry, "the tick before does not reach it");

    let mut fired_at = None;
    for _ in 1..=60u64 {
        host.step_ticks(1);
        if host.vm().global("gFired") == Some(Value::Integer(1)) {
            fired_at = Some(host.ticks());
            break;
        }
    }
    assert_eq!(fired_at, Some(want), "fires on the first tick that reaches the expiry");
    // 0.1 s is 20 ticks of 0.005 s, give or take the soft-float drift: the expiry is at tick 20 or 21,
    // not at the end of the first batch's wait (the old model added one batch of delay).
    assert!((TICKS_PER_BATCH..=TICKS_PER_BATCH + 1).contains(&want), "want = {want}");
}

#[test]
fn sleeping_threads_resume_in_expiry_order() {
    let src = r#"
globals
    integer gN = 0
    integer array gOrder
endglobals

native TriggerSleepAction takes real t returns nothing

function Waiter takes integer id, real t returns nothing
    call TriggerSleepAction(t)
    set gOrder[gN] = id
    set gN = gN + 1
endfunction
"#;
    let mut host = host_for(src);
    host.start("Waiter", &[Value::Integer(1), real_val(0.5)])
        .unwrap();
    host.start("Waiter", &[Value::Integer(2), real_val(0.2)])
        .unwrap();
    host.start("Waiter", &[Value::Integer(3), real_val(0.2)])
        .unwrap();
    for _ in 0..10 {
        host.step();
    }
    let n = host.vm().global("gN").unwrap().as_integer().unwrap();
    assert_eq!(n, 3);
    let order: Vec<i32> = (0..3)
        .map(|i| {
            host.vm()
                .global_element("gOrder", i)
                .unwrap()
                .as_integer()
                .unwrap()
        })
        .collect();
    assert_eq!(order, vec![2, 3, 1]);
}

#[test]
fn negative_sleep_is_not_clamped_by_the_host() {
    // The native clamps a negative sleep to 0 (builtin behaviour). With the host the wake time is
    // now + 0, so the thread resumes at the next step after it sleeps.
    let src = r#"
globals
    integer gDone = 0
endglobals
native TriggerSleepAction takes real t returns nothing
function Main takes nothing returns nothing
    call TriggerSleepAction(-1.0)
    set gDone = 1
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    host.step();
    assert_eq!(host.vm().global("gDone"), Some(Value::Integer(1)));
}

/// Runs `steps` host steps with a spinning thread (budget 50) and three sleepers that sleep
/// `sleep` seconds. Returns, per step, the number of timers that fired and the spinner's counter.
fn spin_trace(sleep: f32, steps: usize) -> Vec<(usize, i32)> {
    let src = r#"
globals
    integer gSpin = 0
endglobals

native TriggerSleepAction takes real t returns nothing

function Spinner takes nothing returns nothing
    loop
        set gSpin = gSpin + 1
    endloop
endfunction

function Sleeper takes real t returns nothing
    call TriggerSleepAction(t)
endfunction
"#;
    let mut host = host_for(src);
    host.vm_mut().set_budget(50);
    host.start("Spinner", &[]).expect("spinner");
    for _ in 0..3 {
        host.start("Sleeper", &[real_val(sleep)]).expect("sleeper");
    }
    (0..steps)
        .map(|_| {
            let fired = host.step();
            let spin = host
                .vm()
                .global("gSpin")
                .and_then(|v| v.as_integer())
                .unwrap_or(-1);
            (fired, spin)
        })
        .collect()
}

#[test]
fn fired_timers_do_not_give_runnable_threads_extra_slices() {
    // Same spinner in both runs. With three sleepers that wake inside the window, three timers fire
    // in one host step; with sleepers that do not wake, none does. The spinner's counter must follow
    // the same trace either way: each host step gives it exactly one budget slice.
    let waking = spin_trace(0.1, 8);
    let sleeping = spin_trace(1000.0, 8);
    assert!(
        waking.iter().any(|&(fired, _)| fired == 3),
        "three timers fire in one step: {waking:?}"
    );
    assert!(sleeping.iter().all(|&(fired, _)| fired == 0));
    let spin_waking: Vec<i32> = waking.iter().map(|&(_, spin)| spin).collect();
    let spin_sleeping: Vec<i32> = sleeping.iter().map(|&(_, spin)| spin).collect();
    assert_eq!(spin_waking, spin_sleeping);
}

// ---------------------------------------------------------------------------------------------
// The 300 s clock restart (docs/spec/lockstep.md section 14)
// ---------------------------------------------------------------------------------------------

const WAKE_SCRIPT: &str = r#"
globals
    integer gWake = 0
endglobals

native TriggerSleepAction takes real t returns nothing

function Main takes nothing returns nothing
    loop
        call TriggerSleepAction(100.0)
        set gWake = gWake + 1
    endloop
endfunction
"#;

const LONG_SLEEP_SCRIPT: &str = r#"
globals
    integer gDone = 0
endglobals

native TriggerSleepAction takes real t returns nothing

function Main takes nothing returns nothing
    call TriggerSleepAction(1000.0)
    set gDone = 1
endfunction
"#;

#[test]
fn sleeping_thread_keeps_waking_every_100_seconds_across_clock_restarts() {
    // 150,000 ticks pass two restarts of the agile clock. A sleep started before a restart is shifted with the queue
    // (both the timer expiry and the thread's wake time lose 300.0); without the shift the wake would be 300 s late.
    let mut host = host_for(WAKE_SCRIPT);
    host.start("Main", &[]).expect("start");
    host.step_ticks(150_000);
    assert_eq!(host.clock().row(), 2);
    assert!(host.misfires().is_empty(), "{:?}", host.misfires());
    let seconds = host.clock().elapsed_seconds();
    assert!((700.0..760.0).contains(&seconds), "{seconds}");
    assert_eq!(host.vm().global("gWake"), Some(Value::Integer(7)), "wakes at 100, 200, ... 700 s of game time");
}

#[test]
fn sleep_of_1000_seconds_wakes_at_299_999() {
    // The agent timer a sleep starts cannot hold 300.0 or more: the start cuts the timeout to horizon - 0.001
    // (Unicorn-checked on 0x6F477FF0). So the thread wakes after 299.999 s, before the first restart.
    let mut host = host_for(LONG_SLEEP_SCRIPT);
    host.start("Main", &[]).expect("start");
    let mut ticks = 0u64;
    while host.vm().global("gDone") != Some(Value::Integer(1)) {
        host.step_ticks(1);
        ticks += 1;
        assert!(ticks < 200_000, "never woke");
    }
    assert_eq!(host.clock().row(), 0);
    let now = host.clock().now();
    assert!((299.999..300.0).contains(&now), "{now}");
}
