//! Scalar text helpers shared by the profile and SLK readers: quote stripping, real and integer literals.
//!
//! Real text is parsed with the original's S2R rules (`softfloat::text::s2r`, `docs/spec/softfloat.md`) when the
//! Game.dll tables are given. Whether the original's Misc and SLK readers use S2R is UNSURE (the readers are not
//! in the decompile export); S2R is the text-to-real routine the game exposes, so it is the choice here.
//! Without the tables the text is parsed as `f64` and rounded to `f32`, which is not bit-exact.
//!
//! Unit SLK real cells do not use S2R: they go through a text-to-double import of Storm.dll (ordinal 574), which is
//! modelled here as C `atof` ([`atof_f32`]). LIKELY, not checked (Storm.dll is not available); see
//! `docs/spec/unit-types.md` section 4.4.

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

/// Whether `b` is a space of the C locale (`isspace`): space, tab, newline, vertical tab, form feed, carriage return.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// The longest prefix of `text` that the C library's `strtod` would convert as a decimal number, with the leading
/// white space removed; `None` when there is no digit in the prefix.
///
/// The grammar is: optional white space, an optional sign, digits with an optional point (at least one digit in the
/// integer or fraction part), then an optional exponent (`e` or `E`, optional sign, at least one digit; an exponent
/// without digits is not part of the number). Anything after the prefix is ignored. Hexadecimal floats, `inf` and
/// `nan` are NOT accepted (UNSURE, see [`atof`]).
pub fn atof_prefix(text: &str) -> Option<&str> {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n && is_c_space(b[i]) {
        i += 1;
    }
    let start = i;
    if i < n && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let mut digits = 0usize;
    while i < n && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < n && b[i] == b'.' {
        i += 1;
        while i < n && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < n && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < n && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < n && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    // Only ASCII bytes were consumed, so the slice ends on a character boundary.
    Some(&text[start..i])
}

/// The C library's `atof` (`strtod` with no end pointer, C locale) as a correctly rounded `f64`.
///
/// Text without a number prefix gives `None` (C gives 0.0; the caller maps `None` to 0.0 where that is the rule).
/// A prefix too large for `f64` gives infinity, the value `strtod` returns with `ERANGE` (CHOICE, UNSURE for the
/// Storm import). A prefix too small gives the nearest value, which may be a subnormal or zero.
///
/// The conversion is Rust's own decimal parser on the extracted prefix: it is correctly rounded, does not read the
/// locale and does not depend on the platform C library.
pub fn atof(text: &str) -> Option<f64> {
    let prefix = atof_prefix(text)?;
    prefix.parse::<f64>().ok()
}

/// The unit-cell real rule (LIKELY, `docs/spec/unit-types.md` section 4.4): [`atof`] rounded to the nearest `f32`
/// (round-half-even, a second rounding after the `f64` value). `None` when the text has no number prefix.
pub fn atof_f32(text: &str) -> Option<f32> {
    atof(text).map(|v| v as f32)
}
