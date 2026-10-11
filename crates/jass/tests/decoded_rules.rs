//! One test per rule decoded from the original compile path (docs/spec/jass-frontend.md section 8). The
//! same rules are covered in bulk by `original_probes.rs`; these tests name them. Every program here
//! is our own text; "the original" means the compiler of Game.dll 1.26a run in Unicorn.

use jass::check::{check_with, CheckOptions, NativeRegistry};

const TYPES: &str =
    "type widget extends handle\ntype unit extends widget\ntype item extends widget\n";

fn errors(srcs: &[&str], opts: &CheckOptions) -> Vec<String> {
    let mut programs = Vec::new();
    for s in srcs {
        match jass::parse(s) {
            Ok(p) => programs.push(p),
            Err(e) => return vec![format!("parse: {e}")],
        }
    }
    check_with(&programs, opts)
        .into_iter()
        .map(|d| d.message)
        .collect()
}

fn ok(src: &str) -> bool {
    errors(&[src], &CheckOptions::default()).is_empty()
}

fn lenient_ok(src: &str) -> bool {
    let o = CheckOptions {
        lenient_returns: true,
        ..CheckOptions::default()
    };
    errors(&[src], &o).is_empty()
}

fn func(ret: &str, body: &str) -> String {
    format!("{TYPES}function f takes nothing returns nothing\nendfunction\nfunction g takes nothing returns {ret}\nlocal handle vh\nlocal widget vw\nlocal unit vu\nlocal item vi\n{body}\nendfunction\n")
}

// ---- case rules (8.3) ----

#[test]
fn rect_the_native_and_rect_the_type_are_different_names() {
    // Retail common.j: `type rect` and `native Rect`.
    assert!(ok(
        "type rect extends handle\nnative Rect takes real a returns rect\n"
    ));
    assert!(!ok(
        "type rect extends handle\nnative rect takes real a returns rect\n"
    ));
}

#[test]
fn keywords_are_lower_case_only() {
    assert!(jass::parse("FUNCTION f takes nothing returns nothing\nendfunction\n").is_err());
    assert!(jass::parse(
        "function f takes nothing returns nothing\nIF true THEN\nENDIF\nendfunction\n"
    )
    .is_err());
}

// ---- the return analysis (8.6) ----

#[test]
fn a_lone_return_null_in_an_integer_function_is_an_error() {
    assert!(!ok(&func("integer", "return null")));
}

#[test]
fn bomber_command_shape_nested_null_and_a_final_value_is_accepted() {
    // (6)BomberCommand: `if not valid then return null endif` followed by `return array[i]`.
    assert!(ok(&func(
        "integer",
        "if true then\nreturn null\nendif\nreturn 1"
    )));
    // The order does not matter for null: the null return after a value return is fine too.
    assert!(ok(&func(
        "integer",
        "if true then\nreturn 1\nendif\nreturn null"
    )));
    assert!(ok(&func("integer", "return null\nreturn 1")));
}

#[test]
fn null_returns_take_the_declared_type_when_it_can_be_null() {
    for ty in ["string", "code", "handle", "unit"] {
        assert!(ok(&func(ty, "return null")), "{ty}");
    }
    for ty in ["boolean", "real", "integer"] {
        assert!(!ok(&func(ty, "return null")), "{ty}");
    }
}

#[test]
fn a_value_return_anywhere_satisfies_the_function_not_every_path() {
    assert!(ok(&func("integer", "if true then\nreturn 1\nendif")));
    assert!(ok(&func("integer", "return 1\ncall f()")));
    assert!(!ok(&func("integer", "call f()")));
    assert!(!ok(&func("integer", "local integer x")));
}

#[test]
fn void_and_value_returns_conflict() {
    assert!(!ok(&func(
        "integer",
        "if true then\nreturn\nendif\nreturn 1"
    )));
    assert!(ok(
        "function g takes nothing returns nothing\nreturn\nendfunction\n"
    ));
    assert!(!ok(
        "function g takes nothing returns nothing\nreturn 1\nendfunction\n"
    ));
}

#[test]
fn different_return_types_conflict_but_integer_and_real_mix() {
    assert!(!ok(&func(
        "integer",
        "if true then\nreturn \"a\"\nendif\nreturn 1"
    )));
    assert!(!ok(&func("integer", "return 1\nreturn \"a\"")));
    assert!(ok(&func(
        "real",
        "if true then\nreturn 1\nendif\nreturn 1.5"
    )));
    assert!(ok(&func("integer", "return 1.5")));
}

#[test]
fn two_handle_types_in_one_block_conflict_even_in_a_handle_function() {
    assert!(!ok(&func(
        "widget",
        "if true then\nreturn vu\nendif\nreturn vi"
    )));
    assert!(ok(&func(
        "widget",
        "if true then\nreturn vu\nendif\nreturn vu"
    )));
}

#[test]
fn a_returned_handle_must_be_a_subtype_of_the_declared_one() {
    assert!(ok(&func("handle", "return vu")));
    assert!(ok(&func("widget", "return vu")));
    assert!(!ok(&func("unit", "return vh")));
    assert!(!ok(&func("unit", "return vw")));
}

#[test]
fn the_branches_of_an_if_may_not_return_different_types() {
    assert!(!ok(&func(
        "integer",
        "if true then\nreturn 1\nelse\nreturn \"a\"\nendif"
    )));
    assert!(!ok(&func(
        "integer",
        "if true then\nreturn 1\nelseif true then\nreturn true\nendif\nreturn 1"
    )));
    assert!(ok(&func(
        "integer",
        "if true then\nreturn 1\nelse\nreturn 2\nendif"
    )));
    // A null branch takes the other branch's type.
    assert!(ok(&func(
        "string",
        "if true then\nreturn null\nelse\nreturn \"a\"\nendif"
    )));
}

#[test]
fn lenient_mode_checks_only_the_last_statement() {
    // The mode the game uses for 23 whitelisted legacy maps (8.6).
    let src = func("integer", "if true then\nreturn \"a\"\nendif\nreturn 1");
    assert!(!ok(&src));
    assert!(lenient_ok(&src));
    // Without a final return the lenient mode asks for one at the end.
    let src = func("integer", "if true then\nreturn 1\nendif");
    assert!(ok(&src));
    assert!(!lenient_ok(&src));
}

// ---- names (8.4) ----

#[test]
fn one_namespace_for_globals_functions_natives_and_types() {
    let g = "globals\ninteger Gx\nendglobals\n";
    assert!(!ok(&format!(
        "{g}function Gx takes nothing returns nothing\nendfunction\n"
    )));
    assert!(ok(&format!(
        "{g}function gx takes nothing returns nothing\nendfunction\n"
    )));
    assert!(!ok(
        "type t extends handle\nglobals\ninteger t\nendglobals\n"
    ));
    assert!(!ok("globals\ninteger integer\nendglobals\n"));
    assert!(jass::parse("globals\ninteger function\nendglobals\n").is_err());
}

#[test]
fn locals_and_parameters_name_rules() {
    let f = "function zz takes nothing returns nothing\nendfunction\n";
    // A local may not take the name of an earlier function, nor of the function itself.
    assert!(!ok(&format!(
        "{f}function g takes nothing returns nothing\nlocal integer zz\nendfunction\n"
    )));
    assert!(!ok(
        "function g takes nothing returns nothing\nlocal integer g\nendfunction\n"
    ));
    // A parameter may take the function's own name, not an earlier function's.
    assert!(ok(
        "function g takes integer g returns nothing\nendfunction\n"
    ));
    assert!(!ok(&format!(
        "{f}function g takes integer zz returns nothing\nendfunction\n"
    )));
    // Both may shadow globals and repeat.
    assert!(ok("globals\ninteger a\nendglobals\nfunction g takes integer a, integer a returns nothing\nlocal real a\nendfunction\n"));
    // A local may use the name of a later function.
    assert!(ok(&format!(
        "function g takes nothing returns nothing\nlocal integer zz\nendfunction\n{f}"
    )));
    // A type name is never a variable name.
    assert!(!ok("type t extends handle\nfunction g takes nothing returns nothing\nlocal integer t\nendfunction\n"));
}

#[test]
fn declarations_may_follow_functions_only_in_a_later_program() {
    let f = "function f takes nothing returns nothing\nendfunction\n";
    assert!(jass::parse(&format!("{f}globals\nendglobals\n")).is_err());
    assert!(jass::parse(&format!("{f}native N takes nothing returns nothing\n")).is_err());
    assert!(jass::parse(&format!("{f}type t extends handle\n")).is_err());
    assert!(errors(
        &[f, "globals\ninteger a\nendglobals\n"],
        &CheckOptions::default()
    )
    .is_empty());
    // The functions of the first program are callable from the second, not the other way round.
    assert!(errors(
        &[
            f,
            "function g takes nothing returns nothing\ncall f()\nendfunction\n"
        ],
        &CheckOptions::default()
    )
    .is_empty());
    assert!(!errors(
        &[
            "function g takes nothing returns nothing\ncall h()\nendfunction\n",
            "function h takes nothing returns nothing\nendfunction\n"
        ],
        &CheckOptions::default()
    )
    .is_empty());
}

#[test]
fn constant_prefixes_natives_and_functions_only() {
    assert!(ok(
        "constant function f takes nothing returns integer\nreturn 1\nendfunction\n"
    ));
    assert!(jass::parse("constant type t extends handle\n").is_err());
}

// ---- natives (8.2) ----

#[test]
fn natives_are_checked_against_the_registry() {
    let mut reg = NativeRegistry::new();
    reg.insert("Sin", "(R)R");
    reg.insert("GetTournamentFinishNowRule", "()I;");
    reg.insert("GetUnitX", "(Hunit;)R");
    let o = CheckOptions {
        natives: Some(reg),
        ..CheckOptions::default()
    };
    let t = "type unit extends handle\n";
    assert!(errors(
        &[&format!(
            "{t}native Sin takes real r returns real\nnative GetUnitX takes unit u returns real\n"
        )],
        &o
    )
    .is_empty());
    // Unknown (also in another case) and mismatching natives.
    assert!(!errors(&["native Zork takes nothing returns nothing\n"], &o).is_empty());
    assert!(!errors(&["native sin takes real r returns real\n"], &o).is_empty());
    assert!(!errors(&["native Sin takes integer r returns real\n"], &o).is_empty());
    assert!(!errors(&["native Sin takes real r returns integer\n"], &o).is_empty());
    assert!(!errors(&["native Sin takes real r, real s returns real\n"], &o).is_empty());
    // Handle types compare by exact name, not by subtyping.
    let t2 = "type widget extends handle\ntype unit extends widget\n";
    assert!(!errors(
        &[&format!(
            "{t2}native GetUnitX takes widget u returns real\n"
        )],
        &o
    )
    .is_empty());
    // A stray `;` after the registered return letter is ignored.
    assert!(errors(
        &["native GetTournamentFinishNowRule takes nothing returns integer\n"],
        &o
    )
    .is_empty());
}

#[test]
fn the_native_table_of_the_game_accepts_the_stray_semicolons() {
    let reg = NativeRegistry::from_tsv(
        include_str!("../../../docs/spec/natives-1.26a.tsv"),
        &["common"],
    );
    assert!(reg.len() > 1100);
    let o = CheckOptions {
        natives: Some(reg),
        ..CheckOptions::default()
    };
    let src = "type hashtable extends handle\nnative LoadInteger takes hashtable table, integer parentKey, integer childKey returns integer\n";
    assert!(errors(&[src], &o).is_empty());
}

// ---- expressions (8.5) ----

#[test]
fn null_is_an_argument_for_every_parameter_type_but_not_an_assignment_to_all() {
    let g = "function g takes integer i, boolean b returns nothing\nendfunction\n";
    assert!(ok(&format!(
        "{g}function h takes nothing returns nothing\ncall g(null, null)\nendfunction\n"
    )));
    assert!(!ok(
        "function h takes nothing returns nothing\nlocal integer i = null\nendfunction\n"
    ));
    assert!(ok("function h takes nothing returns nothing\nlocal string s = null\nlocal code c = null\nendfunction\n"));
}

#[test]
fn comparisons_follow_the_matrix() {
    let t = "type player extends handle\ntype unit extends handle\n";
    let body = |e: &str| {
        format!("{t}function h takes nothing returns nothing\nlocal unit u\nlocal player p\nlocal boolean b = {e}\nendfunction\n")
    };
    assert!(ok(&body("u == p")));
    assert!(ok(&body("1 == null")));
    assert!(ok(&body("null < 1")));
    assert!(!ok(&body("null < null")));
    assert!(!ok(&body("u < p")));
    assert!(!ok(&body("u == 1")));
    assert!(!ok(&body("\"a\" < \"b\"")));
}

#[test]
fn function_references_accept_any_declared_function() {
    let f = "function takes_args takes integer i returns integer\nreturn i\nendfunction\n";
    assert!(ok(&format!("{f}function h takes nothing returns nothing\nlocal code c = function takes_args\nendfunction\n")));
    assert!(!ok("function h takes nothing returns nothing\nlocal code c = function later\nendfunction\nfunction later takes nothing returns nothing\nendfunction\n"));
}

// ---- lexical rules (8.4) ----

#[test]
fn lexical_forms() {
    let wrap = |e: &str| {
        format!("function h takes nothing returns nothing\nlocal string s = {e}\nendfunction\n")
    };
    let int = |e: &str| {
        format!("function h takes nothing returns nothing\nlocal integer i = {e}\nendfunction\n")
    };
    assert!(jass::parse(&wrap("\"a\\qb\"")).is_err());
    assert!(jass::parse(&wrap("\"a\\\\b\\n\\\"\\'\"")).is_ok());
    assert!(jass::parse(&int("'\\n'")).is_ok());
    assert!(jass::parse(&int("'ab'")).is_err());
    assert!(jass::parse(&int("'a\\nbc'")).is_err());
    assert!(jass::parse(&int("089")).is_err());
    assert!(jass::parse(&int("0XFF")).is_ok());
    assert!(jass::parse(
        "function h takes nothing returns nothing\nlocal integer _a\nendfunction\n"
    )
    .is_err());
    assert!(jass::parse(
        "function h takes nothing returns nothing\nlocal integer a_1\nendfunction\n"
    )
    .is_ok());
}
