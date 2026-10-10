use jass::check::{check, Diagnostic, TypeEnv};

/// Hand-written declarations shared by the snippets below (not taken from any game script).
const PRELUDE: &str = r#"
type agent extends handle
type unit extends agent
type item extends handle

globals
    integer gInt = 0
    real gReal = 1.5
    string gStr = "x"
    boolean gFlag = true
    constant integer gConst = 3
    integer array gArr
    unit gUnit = null
endglobals

native Print takes string s returns nothing
native Half takes integer n returns real
native Count takes nothing returns integer
native MakeUnit takes integer id returns unit
native Run takes code c returns nothing
native Noop takes nothing returns nothing

function Cb takes nothing returns nothing
endfunction

function CbArg takes integer n returns nothing
endfunction
"#;

fn parse(src: &str) -> jass::ast::Program {
    jass::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}\n{src}"))
}

fn run(src: &str) -> Vec<Diagnostic> {
    check(&[parse(src)])
}

/// `PRELUDE` plus a function `Test` (no parameters, returns nothing) with the given locals and statements.
fn in_fn(locals: &str, body: &str) -> String {
    format!(
        "{PRELUDE}\nfunction Test takes nothing returns nothing\n{locals}\n{body}\nendfunction\n"
    )
}

fn fn_ok(locals: &str, body: &str) {
    let d = run(&in_fn(locals, body));
    assert!(d.is_empty(), "expected no errors, got {d:#?}");
}

fn fn_err(locals: &str, body: &str, needle: &str) {
    expect_err(&run(&in_fn(locals, body)), needle);
}

fn expect_err(d: &[Diagnostic], needle: &str) {
    assert!(
        d.iter().any(|x| x.message.contains(needle)),
        "no diagnostic containing {needle:?} in {d:#?}"
    );
}

/// 1-based line of the first line of `src` containing `needle`.
fn line_of(src: &str, needle: &str) -> u32 {
    let idx = src
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} not in source"));
    idx as u32 + 1
}

// ---- declarations and the type environment ----

#[test]
fn prelude_is_clean() {
    let d = run(PRELUDE);
    assert!(d.is_empty(), "{d:#?}");
}

#[test]
fn subtype_relation() {
    let (env, d) = TypeEnv::build(&[parse(PRELUDE)]);
    assert!(d.is_empty(), "{d:#?}");
    assert!(env.is_subtype("unit", "unit"));
    assert!(env.is_subtype("unit", "agent"));
    assert!(env.is_subtype("unit", "handle"));
    assert!(env.is_subtype("item", "handle"));
    assert!(!env.is_subtype("agent", "unit"));
    assert!(!env.is_subtype("unit", "item"));
    assert!(!env.is_subtype("handle", "unit"));
    // Widening is assignability, not subtyping.
    assert!(!env.is_subtype("integer", "real"));
    assert!(!env.is_subtype("nosuch", "handle"));
    assert!(env.is_handle_derived("unit"));
    assert!(!env.is_handle_derived("integer"));
}

#[test]
fn type_cycles_are_reported_and_terminate() {
    // a -> b -> a is one cycle (reported once); c -> c is a self cycle.
    let d = run("type a extends b\ntype b extends a\ntype c extends c\n");
    assert_eq!(d.len(), 2, "{d:#?}");
    expect_err(&d, "cyclic type declaration 'a'");
    expect_err(&d, "cyclic type declaration 'c'");
}

#[test]
fn cycle_repair_does_not_cascade() {
    // After the repair `b` derives from handle, so `a` and `b` are usable and only one error is reported.
    let (env, d) = TypeEnv::build(&[parse("type a extends b\ntype b extends a\n")]);
    assert_eq!(d.len(), 1, "{d:#?}");
    assert!(env.is_handle_derived("a"));
    assert!(env.is_handle_derived("b"));
}

#[test]
fn unknown_parent_is_reported_once() {
    let d = run("type x extends nosuch\ntype y extends x\n");
    assert_eq!(d.len(), 1, "{d:#?}");
    expect_err(&d, "type 'x' extends unknown type 'nosuch'");
    assert_eq!(d[0].line, 1);
}

#[test]
fn user_type_must_derive_from_handle() {
    // UNVERIFIED vs retail: the check itself. Here `integer` is the builtin parent being rejected.
    let d = run("type y extends integer\n");
    assert_eq!(d.len(), 1, "{d:#?}");
    expect_err(&d, "must extend a handle type, not 'integer'");
}

#[test]
fn duplicate_type_is_reported() {
    let src = format!("{PRELUDE}\ntype unit extends item\n");
    let d = run(&src);
    assert_eq!(d.len(), 1, "{d:#?}");
    expect_err(&d, "duplicate type 'unit'");
    assert_eq!(d[0].line, line_of(&src, "type unit extends item"));
}

#[test]
fn forward_type_reference_is_allowed() {
    // UNVERIFIED vs retail: a type may extend a type declared later in the file.
    let src = format!("{PRELUDE}\ntype leaf extends mid\ntype mid extends unit\n");
    let d = run(&src);
    assert!(d.is_empty(), "{d:#?}");
}

// ---- assignability ----

#[test]
fn handle_subtypes_assign_upwards_only() {
    fn_ok(
        "local unit u\nlocal handle h\nlocal agent a",
        "set u = MakeUnit(1)\nset h = MakeUnit(1)\nset a = u\nset h = a",
    );
    fn_err(
        "local handle h",
        "set gUnit = h",
        "cannot convert handle to unit",
    );
    fn_err(
        "local item it",
        "set it = MakeUnit(1)",
        "cannot convert unit to item",
    );
}

#[test]
fn null_assigns_to_handles_string_and_code_only() {
    fn_ok(
        "local unit u\nlocal string s\nlocal code c",
        "set u = null\nset s = null\nset c = null",
    );
    fn_err(
        "local integer i",
        "set i = null",
        "cannot convert null to integer",
    );
    fn_err(
        "local boolean b",
        "set b = null",
        "cannot convert null to boolean",
    );
}

#[test]
fn integer_widens_to_real_not_back() {
    fn_ok("local real r", "set r = 1\nset r = gInt\nset gReal = 2");
    fn_err(
        "local integer i",
        "set i = 1.5",
        "cannot convert real to integer",
    );
    fn_err(
        "local integer i",
        "set i = gReal",
        "cannot convert real to integer",
    );
}

#[test]
fn call_arguments_are_checked_for_type_and_count() {
    fn_ok("", "call Half(1)\ncall Half(gInt)");
    fn_err(
        "",
        "call Half(1.5)",
        "argument 1 of 'Half': cannot convert real to integer",
    );
    fn_err(
        "",
        "call Print(3)",
        "argument 1 of 'Print': cannot convert integer to string",
    );
    fn_err(
        "local real r",
        "set r = Half()",
        "'Half' takes 1 argument(s), got 0",
    );
    fn_err("", "call Half(1, 2)", "'Half' takes 1 argument(s), got 2");
}

// ---- expressions ----

#[test]
fn arithmetic_and_promotion() {
    fn_ok("local integer i", "set i = 1 + 2 * 3 - 4 / 2");
    fn_ok("local real r", "set r = 1 + 2.0 * gInt");
    fn_err(
        "local integer i",
        "set i = 1 + 2.0",
        "cannot convert real to integer",
    );
    fn_err(
        "local integer i",
        "set i = true - 1",
        "arithmetic needs numbers",
    );
}

#[test]
fn string_concatenation() {
    fn_ok("local string s", "set s = \"a\" + gStr");
    // UNVERIFIED vs retail: `string + integer` is rejected here.
    fn_err(
        "local string s",
        "set s = \"a\" + 1",
        "invalid operands to '+': string and integer",
    );
}

#[test]
fn comparisons() {
    fn_ok(
        "local boolean b",
        "set b = 1 < 2.5 and gInt >= 0 or 3 <= 4.0 and 5 > 1",
    );
    fn_err(
        "local boolean b",
        "set b = \"a\" < \"b\"",
        "ordering needs numbers",
    );
    fn_err(
        "local boolean b",
        "set b = 1 == \"a\"",
        "cannot compare integer and string",
    );
    fn_ok("local boolean b", "set b = gInt == 1.0");
    fn_ok("local boolean b", "set b = gUnit == null or gUnit != null");
    fn_ok("local boolean b\nlocal agent a", "set b = gUnit == a");
    fn_err(
        "local boolean b\nlocal item it",
        "set b = gUnit == it",
        "cannot compare unit and item",
    );
}

#[test]
fn boolean_operators() {
    fn_ok("local boolean b", "set b = not gFlag and (1 < 2) or false");
    fn_err(
        "local boolean b",
        "set b = 1 and true",
        "'and'/'or' need boolean operands",
    );
    fn_err("local boolean b", "set b = not 1", "'not' needs a boolean");
    fn_err(
        "local boolean b",
        "set b = gInt or gFlag",
        "'and'/'or' need boolean operands",
    );
}

#[test]
fn negation() {
    fn_ok(
        "local real r\nlocal integer i",
        "set r = -gReal\nset i = -(gInt)",
    );
    fn_err("local string s", "set s = -gStr", "cannot negate string");
}

#[test]
fn arrays_need_integer_index_and_array_variables() {
    fn_ok("local integer i", "set gArr[0] = 5\nset i = gArr[gInt]");
    fn_ok(
        "local integer array la\nlocal integer i",
        "set la[1] = 2\nset i = la[1]",
    );
    fn_err(
        "local integer i",
        "set i = gArr[1.0]",
        "array index must be integer, found real",
    );
    fn_err(
        "local integer i",
        "set i = gInt[0]",
        "'gInt' is not an array",
    );
    fn_err(
        "",
        "set gArr[0] = \"x\"",
        "cannot convert string to integer",
    );
    fn_err(
        "local integer i",
        "set i = gArr",
        "array 'gArr' used without an index",
    );
}

#[test]
fn calls_as_values() {
    fn_ok(
        "local integer i\nlocal real r",
        "set i = Count()\nset r = Half(Count())",
    );
    fn_err(
        "local integer i",
        "set i = Noop()",
        "'Noop' returns nothing and has no value",
    );
    fn_err("", "call Nope()", "undefined function 'Nope'");
    // A variable is not callable.
    fn_err("", "call gInt()", "undefined function 'gInt'");
}

#[test]
fn call_statement_discards_result() {
    fn_ok("", "call Half(2)\ncall Noop()\ncall Count()");
}

#[test]
fn undefined_variable() {
    fn_err(
        "local integer i",
        "set i = nosuch + 1",
        "undefined variable 'nosuch'",
    );
}

#[test]
fn function_reference_needs_no_parameters() {
    fn_ok("local code c", "set c = function Cb\ncall Run(function Cb)");
    fn_err(
        "local code c",
        "set c = function CbArg",
        "function 'CbArg' takes parameters and cannot be used as code",
    );
    fn_err(
        "local code c",
        "set c = function Missing",
        "undefined function 'Missing'",
    );
}

// ---- statements ----

#[test]
fn set_rules() {
    fn_err("", "set gConst = 4", "cannot assign to constant 'gConst'");
    fn_err("", "set gArr = 1", "array 'gArr' assigned without an index");
    fn_err(
        "local integer array la",
        "set la = 1",
        "array 'la' assigned without an index",
    );
    fn_err("", "set gInt[0] = 1", "'gInt' is not an array");
    fn_err("", "set gInt = \"s\"", "cannot convert string to integer");
    fn_err("", "set nosuch = 1", "undefined variable 'nosuch'");
}

#[test]
fn exitwhen_only_inside_loop() {
    fn_ok(
        "local integer i",
        "loop\nexitwhen i > 3\nset i = i + 1\nendloop",
    );
    fn_ok(
        "local integer i",
        "loop\nif i > 3 then\nexitwhen true\nendif\nendloop",
    );
    fn_err("", "exitwhen true", "'exitwhen' outside of a loop");
    fn_err(
        "",
        "if true then\nexitwhen true\nendif",
        "'exitwhen' outside of a loop",
    );
    fn_err(
        "",
        "loop\nexitwhen 1\nendloop",
        "'exitwhen' condition needs a boolean",
    );
}

#[test]
fn if_conditions_must_be_boolean() {
    fn_ok(
        "",
        "if gFlag then\ncall Noop()\nelseif 1 < 2 then\nelse\ncall Noop()\nendif",
    );
    fn_err(
        "",
        "if gInt then\nendif",
        "'if'/'elseif' condition needs a boolean",
    );
    fn_err(
        "",
        "if true then\nelseif 2 then\nendif",
        "'if'/'elseif' condition needs a boolean",
    );
}

#[test]
fn debug_statements_are_checked() {
    fn_ok("", "debug call Print(\"ok\")");
    fn_err(
        "",
        "debug call Print(3)",
        "argument 1 of 'Print': cannot convert integer to string",
    );
}

#[test]
fn return_type_must_match_signature() {
    let int_fn = |body: &str| {
        format!("{PRELUDE}\nfunction F takes nothing returns integer\n{body}\nendfunction\n")
    };
    let real_fn = |body: &str| {
        format!("{PRELUDE}\nfunction F takes nothing returns real\n{body}\nendfunction\n")
    };
    let nothing_fn = |body: &str| {
        format!("{PRELUDE}\nfunction F takes nothing returns nothing\n{body}\nendfunction\n")
    };

    assert!(run(&int_fn("return 1")).is_empty());
    // Integer widens to a real return type.
    assert!(run(&real_fn("return 1")).is_empty());
    expect_err(
        &run(&int_fn("return \"x\"")),
        "cannot convert string to integer",
    );
    expect_err(
        &run(&int_fn("return")),
        "'return' without a value, expected integer",
    );
    expect_err(
        &run(&nothing_fn("return 1")),
        "'return' with a value in a function that returns nothing",
    );
    assert!(run(&nothing_fn("return")).is_empty());
}

// ---- symbols and scopes ----

#[test]
fn duplicate_declarations_are_reported() {
    let d = run(&format!(
        "{PRELUDE}\nglobals\ninteger gInt = 1\nendglobals\n"
    ));
    expect_err(&d, "duplicate global 'gInt'");

    let d = run(&format!(
        "{PRELUDE}\nfunction Cb takes nothing returns nothing\nendfunction\n"
    ));
    expect_err(&d, "duplicate function or native 'Cb'");

    // Natives and functions share one namespace.
    let d = run(&format!(
        "{PRELUDE}\nnative Noop takes nothing returns nothing\n"
    ));
    expect_err(&d, "duplicate function or native 'Noop'");

    fn_err("local integer x\nlocal real x", "", "duplicate local 'x'");
    let d = run("function P takes integer a, integer a returns nothing\nendfunction\n");
    expect_err(&d, "duplicate parameter 'a'");
    let d = run("function P takes integer n returns nothing\nlocal integer n\nendfunction\n");
    expect_err(&d, "duplicate local 'n'");
}

#[test]
fn locals_and_params_may_shadow_globals() {
    fn_ok("local integer gInt = 5", "set gInt = gInt + 1");
    let src = format!(
        "{PRELUDE}\nfunction Q takes string gStr returns nothing\ncall Print(gStr)\nendfunction\n"
    );
    assert!(run(&src).is_empty());
}

#[test]
fn functions_may_call_functions_declared_later() {
    // UNVERIFIED vs retail: no forward declaration is required.
    let src = format!(
        "{PRELUDE}\nfunction Test takes nothing returns nothing\ncall Later()\nendfunction\nfunction Later takes nothing returns nothing\nendfunction\n"
    );
    assert!(run(&src).is_empty());
}

#[test]
fn global_initializers_see_only_earlier_globals() {
    // UNVERIFIED vs retail: a global initializer may refer only to earlier globals.
    let d = run("globals\ninteger a = b\ninteger b = 1\nendglobals\n");
    expect_err(&d, "undefined variable 'b'");
    assert!(run("globals\ninteger a = 1\ninteger b = a + 1\nendglobals\n").is_empty());
    expect_err(
        &run("globals\ninteger a = \"x\"\nendglobals\n"),
        "cannot convert string to integer",
    );
}

#[test]
fn constants_and_array_initializers() {
    expect_err(
        &run("globals\nconstant integer c\nendglobals\n"),
        "constant 'c' needs an initializer",
    );
    expect_err(
        &run("globals\ninteger array arr = 1\nendglobals\n"),
        "array 'arr' cannot have an initializer",
    );
    fn_err(
        "local integer array la = 1",
        "",
        "array 'la' cannot have an initializer",
    );
}

#[test]
fn unknown_and_invalid_types() {
    expect_err(&run("globals\nfoo x\nendglobals\n"), "unknown type 'foo'");
    fn_err(
        "local integer i = \"s\"",
        "",
        "cannot convert string to integer",
    );
    let d = run("function R takes nothing returns foo\nendfunction\n");
    assert_eq!(d.len(), 1, "{d:#?}");
    expect_err(&d, "unknown type 'foo'");
}

#[test]
fn unknown_type_does_not_cascade() {
    let d = run(&in_fn("local foo x", "set x = 1 + 2\ncall Print(x)"));
    assert_eq!(d.len(), 1, "{d:#?}");
    expect_err(&d, "unknown type 'foo'");
}

// ---- several programs in load order ----

const COMMON: &str = "type agent extends handle\ntype unit extends agent\nnative Print takes string s returns nothing\nnative GetUnit takes nothing returns unit\n";
const BLIZZARD: &str = "globals\nunit gHero = null\nendglobals\nfunction Init takes nothing returns nothing\nset gHero = GetUnit()\ncall Helper()\nendfunction\n";
const WAR3MAP: &str =
    "function Helper takes nothing returns nothing\ncall Print(\"hi\")\nendfunction\n";

#[test]
fn programs_share_one_namespace_in_load_order() {
    let progs = [parse(COMMON), parse(BLIZZARD), parse(WAR3MAP)];
    let d = check(&progs);
    assert!(d.is_empty(), "{d:#?}");
}

#[test]
fn duplicate_across_programs_names_the_program() {
    let dup = "globals\nunit gHero = null\nendglobals\n";
    let progs = [parse(COMMON), parse(BLIZZARD), parse(WAR3MAP), parse(dup)];
    let d = check(&progs);
    assert_eq!(d.len(), 1, "{d:#?}");
    assert_eq!(d[0].message, "program 4: duplicate global 'gHero'");
}

#[test]
fn later_program_cannot_be_used_before_it_loads_types() {
    // `unit` comes from COMMON, so loading BLIZZARD alone is an error.
    let d = check(&[parse(BLIZZARD)]);
    expect_err(&d, "unknown type 'unit'");
    expect_err(&d, "undefined function 'GetUnit'");
}

// ---- reporting ----

#[test]
fn reports_every_error_in_source_order() {
    let body = "function F takes nothing returns nothing\nlocal integer i\nset i = \"a\"\ncall Print(1)\nset gConst = 2\nendfunction\n";
    let src = format!("{PRELUDE}\n{body}");
    let d = run(&src);
    assert_eq!(d.len(), 3, "{d:#?}");
    assert_eq!(d[0].line, line_of(&src, "set i = \"a\""));
    assert_eq!(d[1].line, line_of(&src, "call Print(1)"));
    assert_eq!(d[2].line, line_of(&src, "set gConst = 2"));
    assert!(d[0].message.contains("cannot convert string to integer"));
    assert!(d[2].message.contains("constant"));
}

#[test]
fn output_is_deterministic() {
    let src = in_fn(
        "local integer i\nlocal boolean b",
        "set i = \"a\"\nset b = 1\nexitwhen gFlag",
    );
    let a = run(&src);
    let b = run(&src);
    assert_eq!(a, b);
    assert_eq!(a.len(), 3, "{a:#?}");
}
