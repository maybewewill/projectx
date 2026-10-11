//! Replays differential vectors recorded from the original `Game.dll` (see
//! `tools/emu/softfloat_vectors.py`).
//!
//! - `SOFTFLOAT_VECTORS=<path to json>`: replay every case in the file and require exact
//!   equality. Without the variable this test prints a note and does not check the file.
//! - `GAME_DLL=<path to Game.dll 1.26a>`: needed for the cases that use lookup tables
//!   (division, sin/cos/tan, ...). Without it those cases are skipped with a note.
//! - The embedded set (numbers only, `embedded.rs`) always runs.

use softfloat::{arith, conv, math, Tables, F32};
use std::collections::BTreeMap;

mod embedded;
mod json;

/// Helpers that do not need lookup tables.
const NO_TABLE: &[&str] = &[
    "add",
    "sub",
    "mul",
    "i2r",
    "f2i",
    "r2i",
    "lt",
    "le",
    "gt",
    "eq",
    "approx_eq",
    "dist2",
];

fn parse_hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex operand")
}

/// Evaluates one helper. Returns `None` for a function this crate does not provide.
fn eval(name: &str, args: &[u32], tables: Option<&Tables>) -> Option<Result<u32, &'static str>> {
    let a = |i: usize| F32(args[i]);
    let needs_tables = !NO_TABLE.contains(&name);
    if needs_tables && tables.is_none() {
        return Some(Err("needs tables"));
    }
    let t = || tables.expect("checked above");
    let r = match name {
        "add" => arith::add(a(0), a(1)).0,
        "sub" => arith::sub(a(0), a(1)).0,
        "mul" => arith::mul(a(0), a(1)).0,
        "div" => arith::div(a(0), a(1), t()).0,
        "i2r" => conv::i2r(args[0] as i32).0,
        "f2i" => conv::f2i(a(0)) as u32,
        "r2i" => conv::r2i(a(0)) as u32,
        "lt" => arith::lt(a(0), a(1)) as u32,
        "le" => arith::le(a(0), a(1)) as u32,
        "gt" => arith::gt(a(0), a(1)) as u32,
        "eq" => arith::eq(a(0), a(1)) as u32,
        "approx_eq" => arith::approx_eq(a(0), a(1)) as u32,
        "dist2" => arith::dist2(a(0), a(1), a(2), a(3)).0,
        "sqrt" => math::sqrt(a(0), t()).0,
        "sin" => math::sin(a(0), t()).0,
        "cos" => math::cos(a(0), t()).0,
        "tan" => math::tan(a(0), t()).0,
        "asin" => math::asin(a(0), t()).0,
        "acos" => math::acos(a(0), t()).0,
        "atan" => math::atan(a(0), t()).0,
        "atan2" => math::atan2(a(0), a(1), t()).0,
        "pow" => math::pow(a(0), a(1), t()).0,
        "exp" => math::exp(a(0), t()).0,
        "ln" => math::ln(a(0), t()).0,
        "trunc" => math::trunc(a(0)).0,
        "floor_b" => math::floor(a(0)).0,
        "ceil_b" => math::ceil(a(0)).0,
        "round_b" => math::round(a(0)).0,
        _ => return None,
    };
    Some(Ok(r))
}

fn load_tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(path).ok()?;
    Tables::from_game_dll(&bytes).ok()
}

#[derive(Default)]
struct Tally {
    cases: usize,
    mismatches: usize,
    skipped: usize,
    first: Vec<String>,
}

fn check(
    tally: &mut BTreeMap<String, Tally>,
    name: &str,
    args: &[u32],
    want: u32,
    tables: Option<&Tables>,
) {
    let t = tally.entry(name.to_string()).or_default();
    match eval(name, args, tables) {
        None => t.skipped += 1,
        Some(Err(_)) => t.skipped += 1,
        Some(Ok(got)) => {
            t.cases += 1;
            if got != want {
                t.mismatches += 1;
                if t.first.len() < 5 {
                    t.first
                        .push(format!("{args:08x?} want {want:08x} got {got:08x}"));
                }
            }
        }
    }
}

#[test]
fn embedded_cases_always_match() {
    let tables = load_tables();
    let mut tally = BTreeMap::new();
    for (name, cases) in embedded::CASES {
        for (args, want) in cases.iter() {
            check(&mut tally, name, args, *want, tables.as_ref());
        }
    }
    report(&tally, "embedded");
    for (name, t) in &tally {
        if t.cases > 0 {
            assert_eq!(t.mismatches, 0, "{name}: {:?}", t.first);
        }
    }
}

#[test]
fn recorded_vectors_match() {
    let Ok(path) = std::env::var("SOFTFLOAT_VECTORS") else {
        eprintln!("note: SOFTFLOAT_VECTORS not set, recorded vectors skipped");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read vectors file");
    let doc = json::parse(&text).expect("valid JSON");
    let cases = doc
        .get("cases")
        .and_then(|c| c.as_array())
        .expect("\"cases\" array");
    let tables = load_tables();
    if tables.is_none() {
        eprintln!("note: GAME_DLL not set, table-based helpers are skipped");
    }
    let mut tally = BTreeMap::new();
    for case in cases {
        let name = case.get("fn").and_then(|v| v.as_str()).expect("fn");
        let args: Vec<u32> = case
            .get("args")
            .and_then(|v| v.as_array())
            .expect("args")
            .iter()
            .map(|v| parse_hex(v.as_str().expect("hex")))
            .collect();
        let want = parse_hex(case.get("ret").and_then(|v| v.as_str()).expect("ret"));
        check(&mut tally, name, &args, want, tables.as_ref());
    }
    report(&tally, &path);
    for (name, t) in &tally {
        assert_eq!(t.mismatches, 0, "{name}: first mismatches {:?}", t.first);
    }
}

fn report(tally: &BTreeMap<String, Tally>, source: &str) {
    eprintln!("== {source}");
    for (name, t) in tally {
        eprintln!(
            "{name:10} cases {:6}  mismatches {:4}  skipped {:5}",
            t.cases, t.mismatches, t.skipped
        );
    }
}
