//! VM tests on hand-written JASS (and a few hand-assembled modules for opcodes the AST cannot reach).

use std::collections::BTreeMap;

use jass::bytecode::{Instr, Op};
use jass::compile::{compile, FuncDef, Module, Ty};
use jass::vm::{NativeRegistry, NativeResult, RunStatus, Value, Vm, VmError};

/// Declarations shared by the snippets. Natives are implemented by `NativeRegistry::with_builtins`
/// plus `MakeAgent` (registered by `build`).
const PRELUDE: &str = r#"
type agent extends handle

globals
    integer gN = 0
    integer array gLog
endglobals

native I2R takes integer i returns real
native R2I takes real r returns integer
native I2S takes integer i returns string
native S2I takes string s returns integer
native TriggerSleepAction takes real t returns nothing
native ExecuteFunc takes string name returns nothing
native MakeAgent takes integer id returns agent

function Log takes integer id returns nothing
    set gLog[gN] = id
    set gN = gN + 1
endfunction
"#;

fn registry() -> NativeRegistry {
    let mut r = NativeRegistry::with_builtins();
    // Handle 0 is the null handle; any other id is a live handle.
    r.register("MakeAgent", |_, a| {
        NativeResult::Return(Value::Handle(a[0].as_integer().unwrap_or(0) as u32))
    });
    r
}

fn build_with(src: &str, reg: NativeRegistry) -> Vm {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).unwrap_or_else(|e| panic!("parse failed: {e}\n{full}"));
    let module = compile(&[prog]).unwrap_or_else(|e| panic!("compile failed: {e}"));
    Vm::new(module, reg).unwrap_or_else(|e| panic!("vm creation failed: {e}"))
}

fn build(src: &str) -> Vm {
    build_with(src, registry())
}

fn int(v: Value) -> i32 {
    v.as_integer()
        .unwrap_or_else(|| panic!("expected integer, got {v:?}"))
}

/// Log contents: the first `gN` entries of `gLog`.
fn log(vm: &Vm) -> Vec<i32> {
    let n = int(vm.global("gN").expect("gN"));
    (0..n as usize)
        .map(|i| int(vm.global_element("gLog", i).expect("gLog")))
        .collect()
}

/// Runs scheduler passes at the given game times.
fn steps(vm: &mut Vm, times: &[f32]) {
    for &t in times {
        vm.step(t);
    }
}

// ------------------------------------------------------------------------------------------
// Arithmetic
// ------------------------------------------------------------------------------------------

#[test]
fn integer_arithmetic_wraps() {
    let mut vm = build(
        r#"
function Wrap takes nothing returns integer
    return 2147483647 + 1
endfunction
function WrapMul takes nothing returns integer
    return 65536 * 65536
endfunction
function WrapSub takes integer a, integer b returns integer
    return a - b
endfunction
function Neg takes integer a returns integer
    return -a
endfunction
function Prec takes nothing returns integer
    return 2 + 3 * 4 - 10 / 2
endfunction
"#,
    );
    assert_eq!(vm.call("Wrap", &[]), Ok(Value::Integer(i32::MIN)));
    assert_eq!(vm.call("WrapMul", &[]), Ok(Value::Integer(0)));
    assert_eq!(
        vm.call("WrapSub", &[Value::Integer(i32::MIN), Value::Integer(1)]),
        Ok(Value::Integer(i32::MAX))
    );
    assert_eq!(
        vm.call("Neg", &[Value::Integer(i32::MIN)]),
        Ok(Value::Integer(i32::MIN))
    );
    assert_eq!(vm.call("Prec", &[]), Ok(Value::Integer(9)));
}

#[test]
fn integer_division_truncates_and_divide_by_zero_fails_the_call() {
    let mut vm = build(
        r#"
function Div takes integer a, integer b returns integer
    return a / b
endfunction
"#,
    );
    assert_eq!(
        vm.call("Div", &[Value::Integer(-7), Value::Integer(2)]),
        Ok(Value::Integer(-3))
    );
    assert_eq!(
        vm.call("Div", &[Value::Integer(1), Value::Integer(0)]),
        Err(RunStatus::DivideByZero)
    );
}

#[test]
fn real_arithmetic_and_divide_by_zero() {
    let mut vm = build(
        r#"
function RDiv takes real a, real b returns real
    return a / b
endfunction
function Mix takes nothing returns real
    local integer i = 3
    return I2R(i) / 2.0 + 0.25
endfunction
function Widen takes nothing returns real
    local integer i = 2
    return i * 1.5
endfunction
function Less takes nothing returns boolean
    return 1 < 1.5
endfunction
"#,
    );
    assert_eq!(
        vm.call("RDiv", &[Value::Real(1.0), Value::Real(4.0)]),
        Ok(Value::Real(0.25))
    );
    assert_eq!(
        vm.call("RDiv", &[Value::Real(1.0), Value::Real(0.0)]),
        Err(RunStatus::DivideByZero)
    );
    assert_eq!(
        vm.call("RDiv", &[Value::Real(1.0), Value::Real(-0.0)]),
        Err(RunStatus::DivideByZero)
    );
    assert_eq!(vm.call("Mix", &[]), Ok(Value::Real(1.75)));
    assert_eq!(vm.call("Widen", &[]), Ok(Value::Real(3.0)));
    assert_eq!(vm.call("Less", &[]), Ok(Value::Boolean(true)));
}

#[test]
fn real_to_integer_truncates_toward_zero() {
    // UNVERIFIED vs retail: R2I truncation. These cases pin our implementation, not the original.
    let mut vm = build(
        r#"
function Trunc takes real r returns integer
    return R2I(r)
endfunction
"#,
    );
    assert_eq!(vm.call("Trunc", &[Value::Real(2.9)]), Ok(Value::Integer(2)));
    assert_eq!(
        vm.call("Trunc", &[Value::Real(-2.9)]),
        Ok(Value::Integer(-2))
    );
}

#[test]
fn falling_off_the_end_returns_the_default_value() {
    let mut vm = build(
        r#"
function NoRet takes nothing returns integer
    local integer x = 1
endfunction
"#,
    );
    assert_eq!(vm.call("NoRet", &[]), Ok(Value::Integer(0)));
}

/// Hand-assembled module with one function `F` returning an integer, for opcodes the AST never emits.
fn hand_module(code: Vec<Instr>, num_slots: usize) -> Module {
    let init = FuncDef {
        name: "<globals>".into(),
        params: vec![],
        ret: None,
        num_slots: 0,
        locals: vec![],
        local_arrays: vec![],
        code: vec![Instr::new(Op::Return)],
    };
    let f = FuncDef {
        name: "F".into(),
        params: vec![],
        ret: Some(Ty::Integer),
        num_slots,
        locals: vec![],
        local_arrays: vec![],
        code,
    };
    Module {
        functions: vec![f, init],
        function_index: BTreeMap::from([("F".to_string(), 0)]),
        globals: vec![],
        global_index: BTreeMap::new(),
        natives: vec![],
        native_index: BTreeMap::new(),
        strings: vec![String::new()],
        init: 1,
    }
}

/// Code computing `slot0 OP slot1` for two integer immediates, returning the result.
fn int_binop(op: Op, a: i32, b: i32) -> Vec<Instr> {
    vec![
        Instr::new(Op::LoadInt).imm(a as u32).dest(0),
        Instr::new(Op::LoadInt).imm(b as u32).dest(1),
        Instr::new(op).left(0).right(1).dest(2),
        Instr::new(Op::Return).right(2),
    ]
}

fn eval_int(op: Op, a: i32, b: i32) -> Result<Value, RunStatus> {
    let mut vm =
        Vm::new(hand_module(int_binop(op, a, b), 3), NativeRegistry::new()).expect("valid module");
    vm.call("F", &[])
}

#[test]
fn modulo_by_zero_is_zero_and_sign_follows_dividend() {
    assert_eq!(eval_int(Op::ModI, 7, 0), Ok(Value::Integer(0)));
    assert_eq!(eval_int(Op::ModI, 7, 3), Ok(Value::Integer(1)));
    assert_eq!(eval_int(Op::ModI, -7, 3), Ok(Value::Integer(-1)));
    assert_eq!(eval_int(Op::ModI, 7, -3), Ok(Value::Integer(1)));
    assert_eq!(eval_int(Op::ModI, i32::MIN, -1), Ok(Value::Integer(0)));
    assert_eq!(
        eval_int(Op::DivI, i32::MIN, -1),
        Ok(Value::Integer(i32::MIN))
    );
}

// ------------------------------------------------------------------------------------------
// Control flow, recursion, globals and arrays
// ------------------------------------------------------------------------------------------

#[test]
fn loops_and_exitwhen() {
    let mut vm = build(
        r#"
function SumTo takes integer n returns integer
    local integer i = 1
    local integer s = 0
    loop
        exitwhen i > n
        set s = s + i
        set i = i + 1
    endloop
    return s
endfunction
function Nested takes nothing returns integer
    local integer i = 0
    local integer hits = 0
    loop
        set i = i + 1
        exitwhen i > 4
        loop
            exitwhen true
            set hits = hits + 1
        endloop
        set hits = hits + 100
    endloop
    return hits
endfunction
"#,
    );
    assert_eq!(
        vm.call("SumTo", &[Value::Integer(10)]),
        Ok(Value::Integer(55))
    );
    assert_eq!(
        vm.call("SumTo", &[Value::Integer(0)]),
        Ok(Value::Integer(0))
    );
    assert_eq!(vm.call("Nested", &[]), Ok(Value::Integer(400)));
}

#[test]
fn if_elseif_else_and_boolean_operators() {
    let mut vm = build(
        r#"
function Classify takes integer n returns integer
    if n < 0 then
        return -1
    elseif n == 0 then
        return 0
    elseif n < 10 and not (n == 5) then
        return 1
    else
        return 2
    endif
endfunction
function OrAnd takes nothing returns boolean
    return (1 == 2) or (3 >= 3) and (2 <= 1) == false
endfunction
"#,
    );
    let mut c = |n| vm.call("Classify", &[Value::Integer(n)]);
    assert_eq!(c(-4), Ok(Value::Integer(-1)));
    assert_eq!(c(0), Ok(Value::Integer(0)));
    assert_eq!(c(3), Ok(Value::Integer(1)));
    assert_eq!(c(5), Ok(Value::Integer(2)));
    assert_eq!(c(50), Ok(Value::Integer(2)));
    assert_eq!(vm.call("OrAnd", &[]), Ok(Value::Boolean(true)));
}

#[test]
fn recursion_fibonacci() {
    let mut vm = build(
        r#"
function Fib takes integer n returns integer
    if n < 2 then
        return n
    endif
    return Fib(n - 1) + Fib(n - 2)
endfunction
"#,
    );
    assert_eq!(
        vm.call("Fib", &[Value::Integer(15)]),
        Ok(Value::Integer(610))
    );
    assert_eq!(vm.call("Fib", &[Value::Integer(0)]), Ok(Value::Integer(0)));
}

#[test]
fn globals_with_initializers_and_constants() {
    let mut vm = build(
        r#"
globals
    integer gA = 5
    integer gB = gA + 1
    constant integer gC = 10
    real gR = 0.5
endglobals
function Read takes nothing returns integer
    set gA = gA * 2
    return gA + gB + gC
endfunction
"#,
    );
    assert_eq!(vm.global("gB"), Some(Value::Integer(6)));
    assert_eq!(vm.global("gR"), Some(Value::Real(0.5)));
    assert_eq!(vm.call("Read", &[]), Ok(Value::Integer(10 + 6 + 10)));
    assert_eq!(vm.global("gA"), Some(Value::Integer(10)));
}

#[test]
fn global_and_local_arrays() {
    let mut vm = build(
        r#"
globals
    integer array gArr
endglobals
function Arr takes nothing returns integer
    local integer array la
    set gArr[5] = 7
    set la[3] = 4
    return gArr[5] * 10 + la[3] + gArr[6]
endfunction
function Read takes integer i returns integer
    return gArr[i]
endfunction
"#,
    );
    assert_eq!(vm.call("Arr", &[]), Ok(Value::Integer(74)));
    assert_eq!(vm.global_element("gArr", 8191), Some(Value::Integer(0)));
    assert_eq!(vm.global_element("gArr", 5), Some(Value::Integer(7)));
    assert_eq!(
        vm.call("Read", &[Value::Integer(8192)]),
        Err(RunStatus::IndexOutOfRange)
    );
    assert_eq!(
        vm.call("Read", &[Value::Integer(-1)]),
        Err(RunStatus::IndexOutOfRange)
    );
}

#[test]
fn local_array_is_fresh_on_each_call() {
    let mut vm = build(
        r#"
function Fresh takes integer v returns integer
    local integer array la
    local integer old = la[0]
    set la[0] = v
    return old
endfunction
"#,
    );
    assert_eq!(
        vm.call("Fresh", &[Value::Integer(9)]),
        Ok(Value::Integer(0))
    );
    assert_eq!(
        vm.call("Fresh", &[Value::Integer(9)]),
        Ok(Value::Integer(0))
    );
}

// ------------------------------------------------------------------------------------------
// Strings and handles
// ------------------------------------------------------------------------------------------

#[test]
fn strings_are_interned_and_concatenated() {
    let mut vm = build(
        r#"
function Concat takes nothing returns boolean
    local string s = "ab" + "cd"
    return s == "abcd"
endfunction
function Num takes nothing returns integer
    return S2I(I2S(42) + "0")
endfunction
function Bad takes nothing returns integer
    return S2I("abc")
endfunction
function Same takes nothing returns boolean
    return I2S(7) == I2S(7)
endfunction
"#,
    );
    assert_eq!(vm.call("Concat", &[]), Ok(Value::Boolean(true)));
    assert_eq!(vm.call("Num", &[]), Ok(Value::Integer(420)));
    assert_eq!(vm.call("Bad", &[]), Ok(Value::Integer(0)));
    assert_eq!(vm.call("Same", &[]), Ok(Value::Boolean(true)));
    // Interning: "abcd" was created by the concatenation above; running it again adds nothing.
    let found = vm.strings().find("abcd").expect("interned by Concat");
    let before = vm.strings().len();
    assert_eq!(vm.call("Concat", &[]), Ok(Value::Boolean(true)));
    assert_eq!(vm.strings().len(), before);
    assert_eq!(vm.strings().find("abcd"), Some(found));
    assert_eq!(vm.strings().get(found), "abcd");
}

#[test]
fn null_handles_compare_equal_to_null() {
    let mut vm = build(
        r#"
function IsNull takes integer id returns boolean
    local agent a = MakeAgent(id)
    return a == null
endfunction
function Same takes nothing returns boolean
    local agent a = MakeAgent(5)
    local agent b = MakeAgent(5)
    return a == b and a != null
endfunction
"#,
    );
    assert_eq!(
        vm.call("IsNull", &[Value::Integer(0)]),
        Ok(Value::Boolean(true))
    );
    assert_eq!(
        vm.call("IsNull", &[Value::Integer(3)]),
        Ok(Value::Boolean(false))
    );
    assert_eq!(vm.call("Same", &[]), Ok(Value::Boolean(true)));
}

#[test]
fn native_signature_is_checked_at_call_time() {
    // The checker rejects bad calls, so a native that returns the wrong kind must be caught by the VM.
    let mut reg = NativeRegistry::with_builtins();
    reg.register("MakeAgent", |_, _| {
        NativeResult::Return(Value::Boolean(true))
    });
    let mut vm = build_with(
        r#"
function Use takes nothing returns nothing
    local agent a = MakeAgent(1)
endfunction
"#,
        reg,
    );
    assert_eq!(vm.call("Use", &[]), Err(RunStatus::TypeError));
}

// ------------------------------------------------------------------------------------------
// Threads, budget and the scheduler
// ------------------------------------------------------------------------------------------

#[test]
fn division_by_zero_aborts_only_that_thread() {
    let mut vm = build(
        r#"
function Bad takes nothing returns nothing
    call Log(1)
    set gN = gN + 1 / 0
    call Log(99)
endfunction
function Good takes nothing returns nothing
    call Log(2)
endfunction
"#,
    );
    vm.start("Bad", &[]).unwrap();
    vm.start("Good", &[]).unwrap();
    vm.step(0.0);
    assert_eq!(vm.failures().len(), 1);
    assert_eq!(vm.failures()[0].1, RunStatus::DivideByZero);
    assert_eq!(vm.thread_count(), 0);
    assert_eq!(log(&vm), vec![1, 2]);
    assert_eq!(vm.global_element("gLog", 2), Some(Value::Integer(0)));
}

#[test]
fn budget_exhaustion_resumes_exactly() {
    let src = r#"
function Count takes integer n returns nothing
    local integer i = 0
    loop
        exitwhen i >= n
        set i = i + 1
    endloop
    set gN = i
endfunction
"#;
    // Same result with a tiny budget: a thread that runs out of budget stays runnable.
    let mut vm = build(src);
    vm.set_budget(3);
    vm.start("Count", &[Value::Integer(100)]).unwrap();
    let mut passes = 0;
    while vm.thread_count() > 0 {
        vm.step(0.0);
        passes += 1;
        assert!(passes < 100_000, "thread never finished");
    }
    assert!(
        passes > 50,
        "tiny budget should need many passes, got {passes}"
    );
    assert_eq!(vm.global("gN"), Some(Value::Integer(100)));
    assert!(vm.failures().is_empty());

    // Synchronous calls resume transparently.
    let mut vm = build(
        r#"
function Fib takes integer n returns integer
    if n < 2 then
        return n
    endif
    return Fib(n - 1) + Fib(n - 2)
endfunction
"#,
    );
    vm.set_budget(1);
    assert_eq!(
        vm.call("Fib", &[Value::Integer(10)]),
        Ok(Value::Integer(55))
    );
}

#[test]
fn budget_is_respected_per_run() {
    // With budget 0 nothing executes and the thread stays put.
    let mut vm = build(
        r#"
function Once takes nothing returns nothing
    call Log(1)
endfunction
"#,
    );
    vm.set_budget(0);
    vm.start("Once", &[]).unwrap();
    vm.step(0.0);
    assert_eq!(vm.thread_count(), 1);
    assert_eq!(vm.global("gN"), Some(Value::Integer(0)));
    vm.set_budget(300_000);
    vm.step(0.0);
    assert_eq!(vm.thread_count(), 0);
    assert_eq!(log(&vm), vec![1]);
}

#[test]
fn sleep_and_scheduler_ordering() {
    let mut vm = build(
        r#"
function Sleeper takes real t, integer id returns nothing
    call Log(id)
    call TriggerSleepAction(t)
    call Log(id + 10)
endfunction
"#,
    );
    vm.start("Sleeper", &[Value::Real(1.0), Value::Integer(1)])
        .unwrap();
    vm.start("Sleeper", &[Value::Real(0.5), Value::Integer(2)])
        .unwrap();
    vm.start("Sleeper", &[Value::Real(0.0), Value::Integer(3)])
        .unwrap();

    steps(&mut vm, &[0.0]);
    assert_eq!(log(&vm), vec![1, 2, 3]);
    assert_eq!(
        vm.thread_count(),
        3,
        "all three are still alive, asleep or queued"
    );

    steps(&mut vm, &[0.25]);
    assert_eq!(
        log(&vm),
        vec![1, 2, 3, 13],
        "C wakes at 0 s, B still asleep until 0.5"
    );

    steps(&mut vm, &[0.5]);
    assert_eq!(log(&vm), vec![1, 2, 3, 13, 12]);

    steps(&mut vm, &[0.99]);
    assert_eq!(
        log(&vm),
        vec![1, 2, 3, 13, 12],
        "A must not wake before its time"
    );

    steps(&mut vm, &[1.0]);
    assert_eq!(log(&vm), vec![1, 2, 3, 13, 12, 11]);
    assert_eq!(vm.thread_count(), 0);
    assert!(vm.failures().is_empty());
}

#[test]
fn scheduler_is_deterministic() {
    let src = r#"
function Sleeper takes real t, integer id returns nothing
    local integer k = 0
    loop
        exitwhen k >= id
        call Log(id * 10 + k)
        call TriggerSleepAction(t)
        set k = k + 1
    endloop
endfunction
"#;
    let run = |budget: u32| {
        let mut vm = build(src);
        vm.set_budget(budget);
        for id in 1..=4 {
            vm.start(
                "Sleeper",
                &[Value::Real(0.25 * id as f32), Value::Integer(id)],
            )
            .unwrap();
        }
        let mut t = 0.0f32;
        while vm.thread_count() > 0 && t < 100.0 {
            vm.step(t);
            t += 0.125;
        }
        assert!(vm.failures().is_empty());
        log(&vm)
    };
    // The same inputs give the same log, including with a budget that preempts threads mid-call.
    assert_eq!(run(7), run(7));
    // Preemption inside Log's read-modify-write of gN can lose an increment (threads really do
    // interleave at the op limit), so only the default budget is expected to log all 10 entries.
    assert_eq!(run(jass::vm::DEFAULT_BUDGET).len(), 1 + 2 + 3 + 4);
}

#[test]
fn execute_func_runs_child_synchronously() {
    let mut vm = build(
        r#"
function Child takes nothing returns nothing
    call Log(2)
endfunction
function Parent takes nothing returns nothing
    call Log(1)
    call ExecuteFunc("Child")
    call Log(3)
endfunction
"#,
    );
    assert_eq!(vm.call("Parent", &[]), Ok(Value::Null));
    assert_eq!(log(&vm), vec![1, 2, 3]);
}

#[test]
fn execute_func_child_that_sleeps_is_scheduled() {
    let mut vm = build(
        r#"
function Child takes nothing returns nothing
    call TriggerSleepAction(1.0)
    call Log(20)
endfunction
function Parent takes nothing returns nothing
    call ExecuteFunc("Child")
    call Log(4)
endfunction
"#,
    );
    vm.start("Parent", &[]).unwrap();
    vm.step(0.0);
    assert_eq!(log(&vm), vec![4], "parent continues while the child sleeps");
    assert_eq!(vm.thread_count(), 1);
    vm.step(0.5);
    assert_eq!(log(&vm), vec![4]);
    vm.step(1.0);
    assert_eq!(log(&vm), vec![4, 20]);
    assert_eq!(vm.thread_count(), 0);
}

#[test]
fn execute_func_unknown_name_aborts_the_caller() {
    let mut vm = build(
        r#"
function Bad takes nothing returns nothing
    call ExecuteFunc("NoSuchFunction")
    call Log(5)
endfunction
"#,
    );
    assert_eq!(vm.call("Bad", &[]), Err(RunStatus::FunctionNotFound));
    assert_eq!(log(&vm), Vec::<i32>::new());
}

#[test]
fn start_checks_name_and_arguments() {
    let mut vm = build(
        r#"
function Two takes integer a, integer b returns nothing
    call Log(a + b)
endfunction
"#,
    );
    assert_eq!(vm.start("Missing", &[]), Err(RunStatus::FunctionNotFound));
    assert_eq!(
        vm.start("Two", &[Value::Integer(1)]),
        Err(RunStatus::TypeError)
    );
    vm.start("Two", &[Value::Integer(1), Value::Integer(2)])
        .unwrap();
    vm.step(0.0);
    assert_eq!(log(&vm), vec![3]);
}

#[test]
fn unbound_native_is_function_not_found() {
    let mut vm = build_with(
        r#"
function Use takes nothing returns nothing
    call TriggerSleepAction(0.0)
endfunction
"#,
        NativeRegistry::new(),
    );
    assert_eq!(vm.call("Use", &[]), Err(RunStatus::FunctionNotFound));
}

#[test]
fn invalid_bytecode_is_rejected_at_creation() {
    let m = hand_module(
        vec![Instr::new(Op::Jump).imm(99), Instr::new(Op::Return)],
        1,
    );
    match Vm::new(m, NativeRegistry::new()) {
        Err(VmError::Invalid { function, pc, .. }) => {
            assert_eq!(function, "F");
            assert_eq!(pc, 0);
        }
        Err(other) => panic!("expected invalid bytecode, got {other}"),
        Ok(_) => panic!("invalid bytecode was accepted"),
    }
}

#[test]
fn global_initializer_failure_is_reported() {
    let src = format!("{PRELUDE}\nglobals\n    integer gX = 1 / 0\nendglobals\n");
    let prog = jass::parse(&src).unwrap();
    let module = compile(&[prog]).unwrap();
    assert!(matches!(
        Vm::new(module, registry()),
        Err(VmError::InitFailed(RunStatus::DivideByZero))
    ));
}

#[test]
fn compile_errors_block_the_vm() {
    let prog =
        jass::parse("function F takes nothing returns nothing\nset x = 1\nendfunction").unwrap();
    assert!(compile(&[prog]).is_err());
}

#[test]
fn yield_pauses_the_pass_and_resumes_after_the_call() {
    let mut reg = registry();
    reg.register("Pause", |_, _| NativeResult::Yield);
    let mut vm = build_with(
        r#"
native Pause takes nothing returns nothing

function Worker takes nothing returns nothing
    call Log(1)
    call Pause()
    call Log(2)
endfunction
"#,
        reg,
    );
    vm.start("Worker", &[]).unwrap();
    vm.step(0.0);
    assert_eq!(log(&vm), vec![1]);
    assert_eq!(vm.thread_count(), 1, "a yielded thread stays scheduled");
    vm.step(0.0);
    assert_eq!(log(&vm), vec![1, 2]);
    assert_eq!(vm.thread_count(), 0);
}

#[test]
fn code_values_refer_to_script_functions() {
    use std::cell::Cell;
    use std::rc::Rc;

    let seen: Rc<Cell<Option<Value>>> = Rc::new(Cell::new(None));
    let mut reg = registry();
    let sink = Rc::clone(&seen);
    reg.register("Remember", move |_, a| {
        sink.set(Some(a[0]));
        NativeResult::Return(Value::Null)
    });
    let mut vm = build_with(
        r#"
native Remember takes code c returns nothing

function Target takes nothing returns nothing
endfunction

function Give takes nothing returns nothing
    call Remember(function Target)
endfunction
"#,
        reg,
    );
    let target = vm.module().function_index["Target"];
    assert_eq!(vm.call("Give", &[]), Ok(Value::Null));
    assert_eq!(seen.get(), Some(Value::Code(target)));
}
