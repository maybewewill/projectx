//! VM tests on hand-written JASS (and a few hand-assembled modules for opcodes the AST cannot reach).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use jass::bytecode::{Instr, Op};
use jass::compile::{compile, FuncDef, Module, Ty};
use jass::vm::{NativeRegistry, NativeResult, ResumeError, RunStatus, Value, Vm, VmError};
use softfloat::{arith, conv, Tables, F32};

/// The soft-float lookup tables read from the user's own `Game.dll` (`GAME_DLL`), once per run.
/// `None` when `GAME_DLL` is not set. Tests that divide or call table-based natives return early then.
fn tables() -> Option<Tables> {
    static TABLES: OnceLock<Option<Tables>> = OnceLock::new();
    TABLES
        .get_or_init(|| {
            let path = std::env::var("GAME_DLL").ok()?;
            let bytes =
                std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
            Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
        })
        .clone()
}

/// Whether the lookup tables are available; prints a note and returns false otherwise.
fn have_tables() -> bool {
    let ok = tables().is_some();
    if !ok {
        eprintln!("GAME_DLL not set: skipping test that needs the soft-float lookup tables");
    }
    ok
}

/// A real value from an `f32` literal (its bit pattern, which is what the VM stores).
fn real_val(x: f32) -> Value {
    Value::Real(F32::from_bits(x.to_bits()))
}

/// The VM time for an `f32` literal.
fn ft(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

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

"#;

/// A script function of the prelude. It comes after the declarations (types, globals, natives):
/// a global block may not follow a function (harness o22, o27), so test globals go above it.
const PRELUDE_LOG: &str = r#"function Log takes integer id returns nothing
    set gLog[gN] = id
    set gN = gN + 1
endfunction
"#;

/// Splits a test source into its `globals ... endglobals` blocks and the rest. The blocks go above
/// `PRELUDE_LOG`, the rest below it.
fn split_globals(src: &str) -> (String, String) {
    let (mut globals, mut rest) = (String::new(), String::new());
    let mut in_block = false;
    for line in src.lines() {
        let t = line.trim();
        if !in_block && t == "globals" {
            in_block = true;
        }
        // Natives are declarations too: they must precede every function.
        let decl = in_block || t.starts_with("native ") || t.starts_with("constant native ");
        let target = if decl { &mut globals } else { &mut rest };
        target.push_str(line);
        target.push('\n');
        if in_block && t == "endglobals" {
            in_block = false;
        }
    }
    (globals, rest)
}

fn registry() -> NativeRegistry {
    let mut r = NativeRegistry::with_builtins();
    // Handle 0 is the null handle; any other id is a live handle.
    r.register("MakeAgent", |_, a| {
        NativeResult::Return(Value::Handle(a[0].as_integer().unwrap_or(0) as u32))
    });
    r
}

fn build_opt(src: &str, reg: NativeRegistry, tables: Option<Tables>) -> Vm {
    let (globals, rest) = split_globals(src);
    let full = format!("{PRELUDE}\n{globals}\n{PRELUDE_LOG}\n{rest}");
    let prog = jass::parse(&full).unwrap_or_else(|e| panic!("parse failed: {e}\n{full}"));
    let module = compile(&[prog]).unwrap_or_else(|e| panic!("compile failed: {e}"));
    Vm::new(module, reg, tables).unwrap_or_else(|e| panic!("vm creation failed: {e}"))
}

fn build_with(src: &str, reg: NativeRegistry) -> Vm {
    build_opt(src, reg, tables())
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
        vm.step(ft(t));
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
    let src = r#"
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
"#;
    // A zero divisor aborts before any lookup table is read, so this needs no tables.
    let mut vm = build_opt(src, registry(), None);
    assert_eq!(
        vm.call("RDiv", &[real_val(1.0), real_val(0.0)]),
        Err(RunStatus::DivideByZero)
    );
    assert_eq!(
        vm.call("RDiv", &[real_val(1.0), real_val(-0.0)]),
        Err(RunStatus::DivideByZero)
    );
    // Real division with a non-zero divisor reads the reciprocal table: without it the thread stops.
    assert_eq!(
        vm.call("RDiv", &[real_val(1.0), real_val(4.0)]),
        Err(RunStatus::MissingTables)
    );
    // Addition, multiplication, I2R and the ordered compares need no table.
    assert_eq!(vm.call("Widen", &[]), Ok(real_val(3.0)));
    assert_eq!(vm.call("Less", &[]), Ok(Value::Boolean(true)));

    if !have_tables() {
        return;
    }
    // Expected bits come from the softfloat crate itself (the division is not correctly rounded).
    let t = tables().expect("tables");
    let mut vm = build(src);
    let q = arith::div(ft(1.0), ft(4.0), &t);
    assert_eq!(
        vm.call("RDiv", &[real_val(1.0), real_val(4.0)]),
        Ok(Value::Real(q))
    );
    let two = ft(2.0);
    let mix = arith::add(arith::div(conv::i2r(3), two, &t), ft(0.25));
    assert_eq!(vm.call("Mix", &[]), Ok(Value::Real(mix)));
    assert_eq!(
        vm.call("Widen", &[]),
        Ok(Value::Real(arith::mul(conv::i2r(2), ft(1.5))))
    );
    assert_eq!(
        vm.call("RDiv", &[real_val(1.0), real_val(0.0)]),
        Err(RunStatus::DivideByZero)
    );
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
    assert_eq!(vm.call("Trunc", &[real_val(2.9)]), Ok(Value::Integer(2)));
    assert_eq!(vm.call("Trunc", &[real_val(-2.9)]), Ok(Value::Integer(-2)));
}

#[test]
fn a_value_function_without_a_value_return_does_not_compile() {
    // The original rejects the whole script set (error 38 / 41): a function with a return type needs a
    // `return value` somewhere in its body (see `jass::check`).
    let prog = jass::parse(
        "function NoRet takes nothing returns integer\nlocal integer x = 1\nendfunction\n",
    )
    .unwrap();
    assert!(compile(&[prog]).is_err());
}

/// Hand-assembled module with one function `F` returning an integer, for opcodes the AST never emits.
fn hand_module(code: Vec<Instr>, num_slots: usize) -> Module {
    let init = FuncDef {
        name: "<globals>".into(),
        sym: 0,
        params: vec![],
        ret: None,
        num_slots: 1,
        code: vec![Instr::new(Op::Return)],
    };
    let f = FuncDef {
        name: "F".into(),
        sym: 2,
        params: vec![],
        ret: Some(Ty::Integer),
        num_slots,
        code,
    };
    Module {
        functions: vec![f, init],
        function_index: BTreeMap::from([("F".to_string(), 0)]),
        globals: vec![],
        global_index: BTreeMap::new(),
        natives: vec![],
        native_index: BTreeMap::new(),
        symbols: vec![String::new(), String::new(), "f".into()],
        prelude: vec![],
        init: 1,
    }
}

/// Code computing `slot0 OP slot1` for two integer immediates, returning the result.
fn int_binop(op: Op, a: i32, b: i32) -> Vec<Instr> {
    vec![
        Instr::new(Op::LoadConst).left(4).imm(a as u32).dest(0),
        Instr::new(Op::LoadConst).left(4).imm(b as u32).dest(1),
        Instr::new(op).left(0).right(1).dest(2),
        Instr::new(Op::Move).left(2).dest(0),
        Instr::new(Op::Return),
    ]
}

fn eval_int(op: Op, a: i32, b: i32) -> Result<Value, RunStatus> {
    let mut vm = Vm::new(
        hand_module(int_binop(op, a, b), 3),
        NativeRegistry::new(),
        None,
    )
    .expect("valid module");
    vm.call("F", &[])
}

#[test]
fn integer_division_wraps_on_minimum_by_minus_one() {
    // JASS has no `%`; integer division truncates and wraps like the original's i32 division.
    assert_eq!(
        eval_int(Op::Div, i32::MIN, -1),
        Ok(Value::Integer(i32::MIN))
    );
    assert_eq!(eval_int(Op::Div, -7, 2), Ok(Value::Integer(-3)));
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
    assert_eq!(vm.global("gR"), Some(real_val(0.5)));
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
    vm.step(ft(0.0));
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
        vm.step(ft(0.0));
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
    vm.step(ft(0.0));
    assert_eq!(vm.thread_count(), 1);
    assert_eq!(vm.global("gN"), Some(Value::Integer(0)));
    vm.set_budget(300_000);
    vm.step(ft(0.0));
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
    vm.start("Sleeper", &[real_val(1.0), Value::Integer(1)])
        .unwrap();
    vm.start("Sleeper", &[real_val(0.5), Value::Integer(2)])
        .unwrap();
    vm.start("Sleeper", &[real_val(0.0), Value::Integer(3)])
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
            vm.start("Sleeper", &[real_val(0.25 * id as f32), Value::Integer(id)])
                .unwrap();
        }
        let mut t = 0.0f32;
        while vm.thread_count() > 0 && t < 100.0 {
            vm.step(ft(t));
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
    vm.step(ft(0.0));
    assert_eq!(log(&vm), vec![4], "parent continues while the child sleeps");
    assert_eq!(vm.thread_count(), 1);
    vm.step(ft(0.5));
    assert_eq!(log(&vm), vec![4]);
    vm.step(ft(1.0));
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
    vm.step(ft(0.0));
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
    match Vm::new(m, NativeRegistry::new(), None) {
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
    let src = format!("{PRELUDE}\nglobals\n    integer gX = 1 / 0\nendglobals\n{PRELUDE_LOG}\n");
    let prog = jass::parse(&src).unwrap();
    let module = compile(&[prog]).unwrap();
    assert!(matches!(
        Vm::new(module, registry(), tables()),
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
    vm.step(ft(0.0));
    assert_eq!(log(&vm), vec![1]);
    assert_eq!(vm.thread_count(), 1, "a yielded thread stays scheduled");
    vm.step(ft(0.0));
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

#[test]
fn resume_runs_only_the_named_thread() {
    let mut vm = build(
        r#"
function Worker takes integer id returns nothing
    call Log(id)
    call TriggerSleepAction(1.0)
    call Log(id + 10)
endfunction
"#,
    );
    let a = vm.start("Worker", &[Value::Integer(1)]).unwrap();
    let b = vm.start("Worker", &[Value::Integer(2)]).unwrap();
    vm.step(ft(0.0));
    assert_eq!(log(&vm), vec![1, 2]);

    // Resuming b at its wake time runs b alone; a stays asleep.
    assert_eq!(vm.resume(b, ft(1.0)), Ok(()));
    assert_eq!(log(&vm), vec![1, 2, 12]);
    assert_eq!(vm.thread_count(), 1);

    // Before its wake time a sleeping thread is not run.
    assert_eq!(
        vm.resume(a, ft(0.5)),
        Err(ResumeError::NotDue { until: ft(1.0) })
    );
    assert_eq!(log(&vm), vec![1, 2, 12]);

    assert_eq!(vm.resume(a, ft(1.0)), Ok(()));
    assert_eq!(log(&vm), vec![1, 2, 12, 11]);
    assert_eq!(vm.resume(a, ft(2.0)), Err(ResumeError::NoSuchThread(a)));
    assert_eq!(vm.thread_count(), 0);
    assert!(vm.failures().is_empty());
}

#[test]
fn wake_time_is_now_plus_seconds_with_softfloat_add() {
    let mut vm = build(
        r#"
function Napper takes nothing returns nothing
    call TriggerSleepAction(0.1)
    call Log(1)
endfunction
"#,
    );
    let t = vm.start("Napper", &[]).unwrap();
    vm.step(ft(1.0));
    // The wake time is the soft-float sum (no IEEE rounding involved).
    let wake = arith::add(ft(1.0), ft(0.1));
    let one_ulp_early = F32::from_bits(wake.to_bits() - 1);
    assert_eq!(
        vm.resume(t, one_ulp_early),
        Err(ResumeError::NotDue { until: wake })
    );
    assert_eq!(vm.resume(t, wake), Ok(()));
    assert_eq!(log(&vm), vec![1]);
}

#[test]
fn names_are_case_sensitive() {
    // The original compares names exactly (docs/spec/jass-frontend.md, "Real compile path"): `call ABC()`
    // does not call `Abc`. Run-time lookups by name (`ExecuteFunc`, host calls) use the same exact spelling
    // (UNSURE: not probed for the run-time name table).
    let prog = jass::parse(
        "function Abc takes nothing returns nothing\nendfunction\nfunction Run takes nothing returns nothing\ncall ABC()\nendfunction\n",
    )
    .unwrap();
    assert!(compile(&[prog]).is_err());
    let mut vm = build(
        r#"
globals
    integer Gg = 1
endglobals
function Abc takes nothing returns nothing
    set Gg = 7
endfunction
function Run takes nothing returns nothing
    call Abc()
endfunction
"#,
    );
    assert!(vm.call("RUN", &[]).is_err());
    assert_eq!(vm.call("Run", &[]), Ok(Value::Null));
    assert_eq!(vm.global("Gg"), Some(Value::Integer(7)));
    assert_eq!(vm.global("gg"), None);
}
