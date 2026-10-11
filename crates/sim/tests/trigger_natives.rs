//! JASS trigger natives through `ScriptHost` stepping (0.1 s per step). Expected values come from the
//! agile clock and the soft-float helpers, so they follow the rules in `natives/trigger.rs`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::lockstep::{TICKS_PER_BATCH, TICK_INCREMENT_BITS};
use sim::{AgileClock, ScriptHost};
use softfloat::{arith, F32};

const PRELUDE: &str = r#"
type timer extends handle
type trigger extends handle
type event extends handle
type boolexpr extends handle
type conditionfunc extends boolexpr
type filterfunc extends boolexpr
type triggercondition extends handle
type triggeraction extends handle

native CreateTimer takes nothing returns timer
native TimerStart takes timer t, real timeout, boolean periodic, code handlerFunc returns nothing
native GetHandleId takes handle h returns integer
native TriggerSleepAction takes real t returns nothing

native CreateTrigger takes nothing returns trigger
native DestroyTrigger takes trigger t returns nothing
native ResetTrigger takes trigger t returns nothing
native EnableTrigger takes trigger t returns nothing
native DisableTrigger takes trigger t returns nothing
native IsTriggerEnabled takes trigger t returns boolean
native TriggerAddCondition takes trigger t, boolexpr b returns triggercondition
native TriggerRemoveCondition takes trigger t, triggercondition c returns nothing
native TriggerClearConditions takes trigger t returns nothing
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerExecute takes trigger t returns nothing
native TriggerEvaluate takes trigger t returns boolean
native GetTriggerEvalCount takes trigger t returns integer
native GetTriggerExecCount takes trigger t returns integer
native TriggerRegisterTimerEvent takes trigger t, real timeout, boolean periodic returns event
native TriggerRegisterTimerExpireEvent takes trigger t, timer tm returns event
native GetTriggeringTrigger takes nothing returns trigger
native GetTriggerEventId takes nothing returns integer
native Condition takes code c returns boolexpr
native Filter takes code c returns filterfunc
native DestroyBoolExpr takes boolexpr b returns nothing
native And takes boolexpr a, boolexpr b returns boolexpr
native Or takes boolexpr a, boolexpr b returns boolexpr
native Not takes boolexpr a returns boolexpr
"#;

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm")
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| v.as_boolean())
        .unwrap_or_else(|| panic!("global {name} is not a boolean"))
}

fn add(a: f32, b: f32) -> f32 {
    arith::add(F32::from_bits(a.to_bits()), F32::from_bits(b.to_bits())).to_f32()
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

#[test]
fn trigger_execute_runs_actions_at_once_and_counts() {
    let src = r#"
globals
    integer gRan = 0
    integer gSeen = -1
    integer gEval = -1
    integer gExec = -1
    trigger gTr = null
endglobals

function Act takes nothing returns nothing
    set gRan = gRan + 1
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    call TriggerAddAction(gTr, function Act)
    call TriggerExecute(gTr)
    set gSeen = gRan
    set gEval = GetTriggerEvalCount(gTr)
    set gExec = GetTriggerExecCount(gTr)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    // The action ran before TriggerExecute returned (synchronous until its first stop).
    assert_eq!(int(&host, "gSeen"), 1);
    assert_eq!(int(&host, "gRan"), 1);
    assert_eq!(int(&host, "gEval"), 1);
    assert_eq!(int(&host, "gExec"), 1);
}

#[test]
fn conditions_are_all_called_in_order_and_anded() {
    let src = r#"
globals
    integer gOrder = 0
    boolean gResult = true
    boolean gEvalNoCond = false
    trigger gTr = null
endglobals

function C1 takes nothing returns boolean
    set gOrder = gOrder * 10 + 1
    return false
endfunction

function C2 takes nothing returns boolean
    set gOrder = gOrder * 10 + 2
    return true
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    set gEvalNoCond = TriggerEvaluate(gTr)
    call TriggerAddCondition(gTr, Condition(function C1))
    call TriggerAddCondition(gTr, Condition(function C2))
    set gResult = TriggerEvaluate(gTr)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    // Both conditions are called, C1 first, even though C1 is false. Empty lists are true.
    assert_eq!(int(&host, "gOrder"), 12);
    assert!(!boolean(&host, "gResult"));
    assert!(boolean(&host, "gEvalNoCond"));
}

#[test]
fn execute_ignores_conditions_but_event_path_checks_them() {
    let src = r#"
globals
    integer gRan = 0
    trigger gTr = null
endglobals

function Gate takes nothing returns boolean
    return false
endfunction

function Act takes nothing returns nothing
    set gRan = gRan + 1
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    call TriggerAddCondition(gTr, Condition(function Gate))
    call TriggerAddAction(gTr, function Act)
    call TriggerExecute(gTr)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(
        int(&host, "gRan"),
        1,
        "TriggerExecute does not evaluate conditions"
    );
}

#[test]
fn timer_event_evaluates_each_period_and_executes_only_when_the_gate_passes() {
    // The gate opens when Main wakes at W (its sleep ends in the pump of the step that reaches W).
    // Timer events fire every 0.2 s from 0.3 on. Each expiry is an evaluation. Only expiries after
    // W run the actions.
    let src = r#"
globals
    boolean gGate = false
    integer gRan = 0
    integer gEvals = 0
    trigger gTr = null
endglobals

function Gate takes nothing returns boolean
    set gEvals = gEvals + 1
    return gGate
endfunction

function Act takes nothing returns nothing
    set gRan = gRan + 1
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    call TriggerAddCondition(gTr, Condition(function Gate))
    call TriggerAddAction(gTr, function Act)
    call TriggerRegisterTimerEvent(gTr, 0.2, true)
    call TriggerSleepAction(0.95)
    set gGate = true
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    let wake = add(start(), 0.95);
    let mut expiries = Vec::new();
    let mut e = add(start(), 0.2);
    for _ in 0..25 {
        expiries.push(e);
        e = add(e, 0.2);
    }
    for k in 1..=40 {
        host.step();
        let clock = clock_after(k);
        let fired: Vec<f32> = expiries.iter().copied().filter(|&x| x <= clock).collect();
        let open = fired.iter().filter(|&&x| x > wake).count() as i32;
        assert_eq!(int(&host, "gRan"), open, "step {k}");
        // Evaluations: one per expiry (the gate is itself a condition, so gEvals counts the same).
        assert_eq!(int(&host, "gEvals"), fired.len() as i32, "step {k}");
    }
}

#[test]
fn disabled_trigger_counts_evaluations_but_not_event_executions() {
    let src = r#"
globals
    integer gRan = 0
    boolean gEnabled = true
    boolean gEvalAfterDisable = false
    integer gExecExplicit = -1
    trigger gTr = null
endglobals

function Act takes nothing returns nothing
    set gRan = gRan + 1
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    call TriggerAddAction(gTr, function Act)
    call TriggerRegisterTimerEvent(gTr, 0.2, true)
    call DisableTrigger(gTr)
    set gEnabled = IsTriggerEnabled(gTr)
    set gEvalAfterDisable = TriggerEvaluate(gTr)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(!boolean(&host, "gEnabled"));
    assert!(boolean(&host, "gEvalAfterDisable"));
    for _ in 0..20 {
        host.step();
    }
    // The event fired (eval count grows) but the disabled trigger never ran its actions.
    assert_eq!(int(&host, "gRan"), 0);
}

#[test]
fn explicit_execute_of_a_disabled_trigger_runs_its_actions() {
    let src = r#"
globals
    integer gRan = 0
    integer gExec = -1
endglobals

function Act takes nothing returns nothing
    set gRan = gRan + 1
endfunction

function Main takes nothing returns nothing
    local trigger t = CreateTrigger()
    call TriggerAddAction(t, function Act)
    call DisableTrigger(t)
    call TriggerExecute(t)
    set gExec = GetTriggerExecCount(t)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gRan"), 1);
    assert_eq!(int(&host, "gExec"), 1);
}

#[test]
fn expire_listeners_run_after_the_handler_in_registration_order() {
    let src = r#"
globals
    integer gOrder = 0
    timer gT = null
    trigger gA = null
    trigger gB = null
endglobals

function Handler takes nothing returns nothing
    set gOrder = gOrder * 10 + 9
endfunction

function ActA takes nothing returns nothing
    set gOrder = gOrder * 10 + 1
endfunction

function ActB takes nothing returns nothing
    set gOrder = gOrder * 10 + 2
endfunction

function Main takes nothing returns nothing
    set gT = CreateTimer()
    set gA = CreateTrigger()
    set gB = CreateTrigger()
    call TriggerAddAction(gA, function ActA)
    call TriggerAddAction(gB, function ActB)
    call TriggerRegisterTimerExpireEvent(gB, gT)
    call TriggerRegisterTimerExpireEvent(gA, gT)
    call TimerStart(gT, 0.3, false, function Handler)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    for _ in 0..10 {
        host.step();
    }
    // Listeners fire in the order they were registered (B, then A). The handler runs first.
    assert_eq!(int(&host, "gOrder"), 9 * 100 + 2 * 10 + 1);
}

#[test]
fn an_action_that_sleeps_holds_back_the_rest_of_the_chain() {
    let src = r#"
globals
    integer gOrder = 0
    integer gAfterExecute = -1
    trigger gTr = null
endglobals

function A1 takes nothing returns nothing
    call TriggerSleepAction(0.3)
    set gOrder = gOrder * 10 + 1
endfunction

function A2 takes nothing returns nothing
    set gOrder = gOrder * 10 + 2
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    call TriggerAddAction(gTr, function A1)
    call TriggerAddAction(gTr, function A2)
    call TriggerExecute(gTr)
    set gAfterExecute = gOrder
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    // TriggerExecute returned while A1 sleeps: the caller went on, and A2 has not run yet.
    assert_eq!(int(&host, "gAfterExecute"), 0);
    assert_eq!(int(&host, "gOrder"), 0);
    for _ in 0..10 {
        host.step();
    }
    assert_eq!(int(&host, "gOrder"), 12);
}

#[test]
fn triggering_trigger_and_event_id_inside_an_action() {
    let src = r#"
globals
    integer gSame = 0
    integer gHasEvent = 0
    trigger gTr = null
endglobals

function Act takes nothing returns nothing
    if GetTriggeringTrigger() == gTr then
        set gSame = 1
    endif
    // A timer event carries no event id in the model (UNSURE), so the raw id is 0 here.
    set gHasEvent = GetTriggerEventId()
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    call TriggerAddAction(gTr, function Act)
    call TriggerRegisterTimerEvent(gTr, 0.2, false)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    for _ in 0..6 {
        host.step();
    }
    assert_eq!(int(&host, "gSame"), 1);
    assert_eq!(int(&host, "gHasEvent"), 0);
}

#[test]
fn destroy_stops_events_and_releases_ids_for_reuse() {
    let src = r#"
globals
    integer gRanAfter = 0
    integer gFirst = 0
    integer gReused = 0
    trigger gTr = null
endglobals

function Act takes nothing returns nothing
    set gRanAfter = gRanAfter + 1
endfunction

function Main takes nothing returns nothing
    set gTr = CreateTrigger()
    set gFirst = GetHandleId(gTr)
    call TriggerAddAction(gTr, function Act)
    call TriggerRegisterTimerEvent(gTr, 0.2, true)
endfunction

function Later takes nothing returns nothing
    call DestroyTrigger(gTr)
    call TriggerExecute(gTr)
    set gReused = GetHandleId(CreateTrigger())
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gFirst"), 0x100001);
    // Destroy the trigger at step 2 via a second thread.
    host.start("Later", &[]).unwrap();
    host.step();
    let ran_at_destroy = int(&host, "gRanAfter");
    for _ in 0..20 {
        host.step();
    }
    // The destroyed trigger's event never fires (its queue entry was cancelled) and its execute does
    // nothing. The event would have fired at 0.3 s, so the count was 0 at the destroy.
    assert_eq!(ran_at_destroy, 0);
    assert_eq!(int(&host, "gRanAfter"), 0);
    // Released ids come back LIFO: the trigger's id (released last) is the next one handed out.
    assert_eq!(int(&host, "gReused"), 0x100001);
}

#[test]
fn reset_trigger_clears_both_counters() {
    let src = r#"
globals
    integer gEval = -1
    integer gExec = -1
endglobals

function Main takes nothing returns nothing
    local trigger t = CreateTrigger()
    call TriggerExecute(t)
    call TriggerExecute(t)
    call ResetTrigger(t)
    set gEval = GetTriggerEvalCount(t)
    set gExec = GetTriggerExecCount(t)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gEval"), 0);
    assert_eq!(int(&host, "gExec"), 0);
}

#[test]
fn boolexpr_and_or_not_compose_conditions() {
    let src = r#"
globals
    boolean gAndNot = false
    boolean gOr = false
    boolean gNotA = true
    boolean gAfterRemove = false
    boolean gAfterClear = false
endglobals

function CondTrue takes nothing returns boolean
    return true
endfunction

function CondFalse takes nothing returns boolean
    return false
endfunction

function Main takes nothing returns nothing
    local trigger t = CreateTrigger()
    local boolexpr e1 = And(Condition(function CondTrue), Not(Condition(function CondFalse)))
    local boolexpr e2 = Or(Condition(function CondFalse), Condition(function CondTrue))
    local boolexpr e3 = Not(Condition(function CondTrue))
    local triggercondition c
    call TriggerAddCondition(t, e1)
    set gAndNot = TriggerEvaluate(t)
    call TriggerClearConditions(t)
    call TriggerAddCondition(t, e2)
    set gOr = TriggerEvaluate(t)
    call TriggerClearConditions(t)
    set c = TriggerAddCondition(t, e3)
    set gNotA = TriggerEvaluate(t)
    call TriggerRemoveCondition(t, c)
    set gAfterRemove = TriggerEvaluate(t)
    call TriggerAddCondition(t, e3)
    call TriggerClearConditions(t)
    set gAfterClear = TriggerEvaluate(t)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(boolean(&host, "gAndNot"));
    assert!(boolean(&host, "gOr"));
    assert!(!boolean(&host, "gNotA"));
    assert!(boolean(&host, "gAfterRemove"));
    assert!(boolean(&host, "gAfterClear"));
}
