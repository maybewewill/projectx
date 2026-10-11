//! Scalar text helpers shared by the profile and SLK readers: quote stripping, real and integer literals.
//!
//! Real text is parsed with the original's S2R rules (`softfloat::text::s2r`, `docs/spec/softfloat.md`) when the
//! Game.dll tables are given. Whether the original's Misc and SLK readers use S2R is UNSURE (the readers are not
//! in the decompile export); S2R is the text-to-real routine the game exposes, so it is the choice here.
//! Without the tables the text is parsed as `f64` and rounded to `f32`, which is not bit-exact.

use softfloat::{text, Tables, F32};

/// The text of a cell or a value with one pair of surrounding double quotes removed and the ends trimmed.
pub fn scalar(raw: &str) -> &str {
    let t = raw.trim();
    t.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(t)
        .trim()
}

/// Whether `t` is a plain decimal real: an optional sign, digits, at most one dot, at least one digit.
pub fn is_real_syntax(t: &str) -> bool {
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let mut digits = 0usize;
    let mut dot = false;
    for c in body.bytes() {
        match c {
            b'0'..=b'9' => digits += 1,
            b'.' if !dot => dot = true,
            _ => return false,
        }
    }
    digits > 0
}

/// Parses a real value (see the module documentation). `None` for text that is not a plain decimal real.
pub fn parse_real(raw: &str, tables: Option<&Tables>) -> Option<F32> {
    let t = scalar(raw);
    if !is_real_syntax(t) {
        return None;
    }
    match tables {
        Some(tables) => text::s2r(Some(tables), t.as_bytes()),
        None => t
            .parse::<f64>()
            .ok()
            .map(|v| F32::from_bits((v as f32).to_bits())),
    }
}

/// Parses a 32-bit signed integer value. `None` for text that is not an integer.
pub fn parse_int(raw: &str) -> Option<i32> {
    scalar(raw).parse::<i32>().ok()
}
