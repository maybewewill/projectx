//! Real <-> text conversions (R2S, R2SW, S2R and the scanner's real literal) against the
//! original `Game.dll` 1.26a, recorded with `tools/emu/text_vectors.py`.
//!
//! - The embedded pairs (`text_embedded.rs`) always run. A case that needs the reciprocal table
//!   (division) runs only when `GAME_DLL` is set; without the table it is counted as skipped.
//! - `SOFTFLOAT_TEXT_VECTORS=<path to json>` replays every recorded case and requires exact
//!   equality. Without the variable the test prints a note and checks nothing more.
//! - `GAME_DLL=<path to Game.dll 1.26a>` supplies the table.

use softfloat::{text, Tables, F32};
use std::collections::BTreeMap;

mod json;
mod text_embedded;

/// One call of the ported functions, with its arguments.
enum Call {
    R2s { x: u32 },
    R2sw { x: u32, width: u32, precision: u32 },
    S2r { text: Vec<u8> },
    LexReal { text: Vec<u8> },
}

/// Observed result: the output bytes of a formatter, or the bits of a real.
#[derive(Debug, PartialEq)]
enum Out {
    Text(Vec<u8>),
    Bits(u32),
}

fn hex_bytes(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex byte"))
        .collect()
}

/// Runs one call. `None` when the call needs the lookup table and none is loaded.
fn evaluate(call: &Call, tables: Option<&Tables>) -> Option<Out> {
    match call {
        Call::R2s { x } => text::r2s(tables, F32(*x)).map(|s| Out::Text(s.into_bytes())),
        Call::R2sw {
            x,
            width,
            precision,
        } => text::r2sw(tables, F32(*x), *width as i32, *precision as i32)
            .map(|s| Out::Text(s.into_bytes())),
        Call::S2r { text: t } => text::s2r(tables, t).map(|v| Out::Bits(v.0)),
        Call::LexReal { text: t } => text::real_literal(tables, t).map(|v| Out::Bits(v.0)),
    }
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
    tally: &mut BTreeMap<&'static str, Tally>,
    name: &'static str,
    call: &Call,
    want: &Out,
    tables: Option<&Tables>,
) {
    let t = tally.entry(name).or_default();
    match evaluate(call, tables) {
        None => t.skipped += 1,
        Some(got) => {
            t.cases += 1;
            if &got != want {
                t.mismatches += 1;
                if t.first.len() < 5 {
                    t.first
                        .push(format!("{} want {want:?} got {got:?}", describe(call)));
                }
            }
        }
    }
}

fn describe(call: &Call) -> String {
    match call {
        Call::R2s { x } => format!("r2s({x:08x})"),
        Call::R2sw {
            x,
            width,
            precision,
        } => format!("r2sw({x:08x}, {width}, {precision})"),
        Call::S2r { text } => format!("s2r({:?})", String::from_utf8_lossy(text)),
        Call::LexReal { text } => format!("lex_real({:?})", String::from_utf8_lossy(text)),
    }
}

fn report(tally: &BTreeMap<&'static str, Tally>, label: &str) {
    for (name, t) in tally {
        eprintln!(
            "{label} {name}: {} cases, {} mismatches, {} skipped (no table)",
            t.cases, t.mismatches, t.skipped
        );
        for line in &t.first {
            eprintln!("  {line}");
        }
    }
}

fn embedded_calls() -> Vec<(&'static str, Call, Out)> {
    let mut out = Vec::new();
    for &(x, text) in text_embedded::R2S {
        out.push(("r2s", Call::R2s { x }, Out::Text(hex_bytes(text))));
    }
    for &(x, width, precision, text) in text_embedded::R2SW {
        out.push((
            "r2sw",
            Call::R2sw {
                x,
                width,
                precision,
            },
            Out::Text(hex_bytes(text)),
        ));
    }
    for &(text, bits) in text_embedded::S2R {
        out.push((
            "s2r",
            Call::S2r {
                text: hex_bytes(text),
            },
            Out::Bits(bits),
        ));
    }
    for &(text, bits) in text_embedded::LEX_REAL {
        out.push((
            "lex_real",
            Call::LexReal {
                text: hex_bytes(text),
            },
            Out::Bits(bits),
        ));
    }
    out
}

#[test]
fn embedded_pairs_always_match() {
    let tables = load_tables();
    if tables.is_none() {
        eprintln!("note: GAME_DLL not set, cases that need the reciprocal table are skipped");
    }
    let mut tally = BTreeMap::new();
    for (name, call, want) in embedded_calls() {
        check(&mut tally, name, &call, &want, tables.as_ref());
    }
    report(&tally, "embedded");
    for (name, t) in &tally {
        assert_eq!(t.mismatches, 0, "{name}: {:?}", t.first);
        assert!(t.cases + t.skipped > 0, "{name}: no embedded cases");
    }
}

#[test]
fn small_formats_need_no_table() {
    // |x| < 2^31 never reaches the table: these run with `None`.
    assert_eq!(text::r2s(None, F32(0x3FC0_0000)).as_deref(), Some("1.500"));
    assert_eq!(text::r2s(None, F32(0xC050_0000)).as_deref(), Some("-3.250"));
    assert_eq!(text::r2s(None, F32(0x8000_0000)).as_deref(), Some("0.000"));
    assert_eq!(
        text::r2sw(None, F32(0xBFC0_0000), 10, 2).as_deref(),
        Some("-       1.50")
    );
    assert_eq!(
        text::r2sw(None, F32(0x3F80_0000), 0, 0).as_deref(),
        Some("1.0")
    );
    assert_eq!(
        text::r2sw(None, F32(0x3F00_0000), 0, 0).as_deref(),
        Some("1.0")
    );
    assert_eq!(
        text::r2sw(None, F32(0x4000_0000), 0, -1).as_deref(),
        Some("2.0")
    );
}

#[test]
fn large_and_table_formats_report_missing_table() {
    // 3e9 takes the division loop, and "12" needs the power-of-ten division.
    assert_eq!(text::r2s(None, F32(3e9f32.to_bits())), None);
    assert_eq!(text::s2r(None, b"12"), None);
    assert_eq!(text::real_literal(None, b"0.1"), None);
    // A literal without a dot is only I2R, so it needs no table.
    assert_eq!(
        text::real_literal(None, b"12").map(|v| v.0),
        Some(0x4140_0000)
    );
    // "1234567890": the tenth integer digit makes the exponent negative, so S2R multiplies
    // (no table) and "123" divides by 10^0 (table).
    assert!(text::s2r(None, b"1234567890").is_some());
    assert_eq!(text::s2r(None, b"123"), None);
}

#[test]
fn inf_and_flt_max_print_as_inf() {
    // Table-free: the helper prints these before it reaches the division loop.
    assert_eq!(text::r2s(None, F32(0x7F80_0000)).as_deref(), Some("inf"));
    assert_eq!(text::r2s(None, F32(0xFF80_0000)).as_deref(), Some("-inf"));
    assert_eq!(text::r2s(None, F32(0x7F7F_FFFF)).as_deref(), Some("inf"));
    assert_eq!(text::r2s(None, F32(0xFF7F_FFFF)).as_deref(), Some("-inf"));
}

#[test]
fn recorded_vectors_match() {
    let Ok(path) = std::env::var("SOFTFLOAT_TEXT_VECTORS") else {
        eprintln!("note: SOFTFLOAT_TEXT_VECTORS not set, recorded text vectors skipped");
        return;
    };
    let file = std::fs::read_to_string(&path).expect("read vectors file");
    let doc = json::parse(&file).expect("valid JSON");
    let cases = doc
        .get("cases")
        .and_then(|c| c.as_array())
        .expect("\"cases\" array");
    let tables = load_tables();
    if tables.is_none() {
        eprintln!("note: GAME_DLL not set, cases that need the reciprocal table are skipped");
    }
    let mut tally = BTreeMap::new();
    for case in cases {
        let name = case.get("fn").and_then(|v| v.as_str()).expect("fn");
        let hex = |key: &str| case.get(key).and_then(|v| v.as_str()).expect(key);
        let num = |key: &str| u32::from_str_radix(hex(key), 16).expect("hex u32");
        let (call, label): (Call, &'static str) = match name {
            "r2s" => (Call::R2s { x: num("x") }, "r2s"),
            "r2sw" => (
                Call::R2sw {
                    x: num("x"),
                    width: num("width"),
                    precision: num("precision"),
                },
                "r2sw",
            ),
            "s2r" => (
                Call::S2r {
                    text: hex_bytes(hex("text")),
                },
                "s2r",
            ),
            "lex_real" => (
                Call::LexReal {
                    text: hex_bytes(hex("text")),
                },
                "lex_real",
            ),
            other => panic!("unknown fn {other}"),
        };
        let want = match name {
            "r2s" | "r2sw" => Out::Text(hex_bytes(hex("text"))),
            _ => Out::Bits(num("ret")),
        };
        check(&mut tally, label, &call, &want, tables.as_ref());
    }
    report(&tally, "recorded");
    for (name, t) in &tally {
        assert_eq!(t.mismatches, 0, "{name}: {:?}", t.first);
    }
}
