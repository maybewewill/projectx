//! Real <-> text conversions of Game.dll 1.26a: R2S / R2SW (format a real), S2R (parse a
//! real) and the JASS scanner's real-literal action. Checked against the original in Unicorn
//! (`tools/emu/text_vectors.py`).
//!
//! - [`r2s`] / [`r2sw`]: the helper `0x6F6F0360`. R2S calls it with width 0 and precision 3;
//!   R2SW passes its width and precision arguments. Output is built with the C runtime's
//!   `%d` and `%*d.%0*d` conversions, after the value has been split with the soft-float
//!   operations (truncation, rounding through `floor(x + 0.5)`).
//! - [`s2r`]: the helper `0x6F6F0520`. Reads an optional sign, up to nine significant
//!   digits into an integer, counts the decimal exponent, converts with I2R and scales by
//!   a power of ten with soft MUL or DIV.
//! - [`real_literal`]: the scanner's real action `0x6F466690`, which uses the same helpers.
//!
//! Some paths divide with the soft DIV, which uses the reciprocal table
//! ([`crate::arith::div`]). The table is read from the user's `Game.dll`. Each function takes
//! `Option<&Tables>` and returns `None` only when a path needs the table and none was given.
//! Everything else (R2S / R2SW of values below `2^31`, and literals without a dot) works
//! without it.
//!
//! Output that the original would write past its 256-byte native buffer is still produced,
//! but the width and the zero runs are capped at [`MAX_RUN`] characters.

use alloc::string::{String, ToString};

use crate::{arith, conv, math, Tables, F32};

/// Longest run of padding or zeros produced for a width or precision argument. The original
/// writes into a 256-byte stack buffer, so anything longer is undefined there. This cap only
/// keeps the port from allocating gigabytes for an absurd argument.
pub const MAX_RUN: i32 = 4096;

/// `10.0` (`0x41200000`), the divisor and base of the scaling steps.
const TEN: F32 = F32(0x4120_0000);
/// Bit pattern of `FLT_MAX`; the helper prints it as `inf` (quirk of its compare constant).
const FLT_MAX_BITS: u32 = 0x7F7F_FFFF;
/// Bit pattern of `+inf`.
const INF_BITS: u32 = 0x7F80_0000;
/// Smallest exponent field handled by the large-value path (`2^31`).
const BIG_EXP: u32 = 0x4F00_0000;

/// Formats a real as `R2S` does: width 0, precision 3, e.g. `1.5 -> "1.500"`.
///
/// Returns `None` only for `|x| >= 2^31` (and NaN) when `tables` is `None`.
pub fn r2s(tables: Option<&Tables>, x: F32) -> Option<String> {
    r2sw(tables, x, 0, 3)
}

/// Formats a real as `R2SW` does (`0x6F6F0360` with the native's width and precision).
///
/// - A minus sign is written for negative values, except `-0.0` (the helper tests `2x != 0`).
///   It is written before the width padding, so the padding of a negative value sits after
///   the sign.
/// - `|x| >= 2^31`, and NaN: the value is divided by ten (soft DIV) until it drops below
///   `2^31`, the integer part is printed, one zero is printed per division, then a dot and
///   `precision` zeros (at least one; no cap below [`MAX_RUN`]). `+inf` and `|x| == FLT_MAX`
///   print as `inf`. This path needs `tables`.
/// - Otherwise the precision is `precision` (a negative value means 6, with trailing zeros
///   removed but one digit kept) and is capped at 9. The fraction is rounded half up to that
///   many digits, carrying into the integer part. The integer part is right-justified in
///   `width - precision` columns (at least one, the sign is not counted).
pub fn r2sw(tables: Option<&Tables>, x: F32, width: i32, precision: i32) -> Option<String> {
    let v = x.0;
    let mut out = String::new();
    if v & 0x8000_0000 != 0 && v != 0x8000_0000 {
        out.push('-');
    }
    let a = v & 0x7FFF_FFFF;
    if (a & 0x7F80_0000) >= BIG_EXP {
        if a == INF_BITS || a == FLT_MAX_BITS {
            out.push_str("inf");
        } else {
            big_value(tables?, a, precision, &mut out);
        }
        return Some(out);
    }
    small_value(F32(a), width, precision, &mut out);
    Some(out)
}

/// Large-value branch of R2SW (`|x| >= 2^31`, or NaN).
fn big_value(tables: &Tables, mut a: u32, precision: i32, out: &mut String) {
    let mut digits_before_dot: i32 = 0;
    loop {
        digits_before_dot += 1;
        a = arith::div(F32(a), TEN, tables).0;
        if a < BIG_EXP {
            break;
        }
    }
    let int_part = conv::f2i(F32(a));
    out.push_str(&int_part.to_string());
    push_zeros(out, digits_before_dot.min(MAX_RUN));
    out.push('.');
    let decimals = if precision > 0 { precision } else { 1 };
    push_zeros(out, decimals.min(MAX_RUN));
}

/// Small-value branch of R2SW (`|x| < 2^31`). Uses no lookup table.
fn small_value(a: F32, width: i32, precision: i32, out: &mut String) {
    let negative_precision = precision < 0;
    let mut decimals = if negative_precision {
        6
    } else {
        precision.min(9)
    };
    let mut int_part = conv::f2i(a);
    let scale_int = 10i32.pow(decimals as u32);
    let scale = conv::i2r(scale_int);
    let frac = fraction(a);
    let scaled = arith::mul(frac, scale);
    let rounded = math::round(scaled);
    let mut frac_int = conv::f2i(rounded);
    if frac_int >= scale_int {
        int_part = int_part.wrapping_add(1);
        frac_int -= scale_int;
    }
    if negative_precision {
        while decimals > 1 && frac_int % 10 == 0 {
            frac_int /= 10;
            decimals -= 1;
        }
    }
    let pad = width.wrapping_sub(decimals).clamp(1, MAX_RUN);
    push_right(out, &int_part.to_string(), pad);
    out.push('.');
    push_zero_padded(out, frac_int, decimals.min(MAX_RUN));
}

/// Fractional part `a - trunc(a)` of a non-negative real, `0x6F6EF2D0`.
///
/// Values below 1 are returned unchanged, values at or above `2^23` have no fraction, and
/// for the rest the mantissa bits below the integer part are masked off and the result is
/// a soft subtraction.
fn fraction(a: F32) -> F32 {
    let e = ((a.0 >> 23) & 0xFF) as i32 - 127;
    if e < 0 {
        return a;
    }
    if e >= 0x17 {
        return F32::ZERO;
    }
    let mask = ((0x8000_0000u32 as i32) >> (e + 8)) as u32;
    arith::sub(a, F32(a.0 & mask))
}

fn push_zeros(out: &mut String, n: i32) {
    for _ in 0..n.max(0) {
        out.push('0');
    }
}

/// `%*d`: right-justified in `width` columns (spaces).
fn push_right(out: &mut String, text: &str, width: i32) {
    let w = width.max(0) as usize;
    for _ in text.len()..w {
        out.push(' ');
    }
    out.push_str(text);
}

/// `%0*d`: zero-padded to `width` characters (a minus sign counts toward the width).
fn push_zero_padded(out: &mut String, value: i32, width: i32) {
    let text = value.to_string();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    let w = width.max(0) as usize;
    out.push_str(sign);
    for _ in (sign.len() + digits.len())..w {
        out.push('0');
    }
    out.push_str(digits);
}

/// Value of a JASS real literal token, the scanner's real action (`0x6F466690`).
///
/// The digits before the dot form an integer (wrapping 32-bit); the digits after it form a
/// second integer and a power of ten `10^k` for `k` digits (wrapping). The value is
/// `I2R(int) + DIV(I2R(frac), I2R(10^k))` with the soft ADD and DIV, so `0.1` is `0x3DCCCCCE`,
/// not the correctly rounded `0x3DCCCCCD`. Without a dot the text is only `I2R(int)`, which
/// needs no table.
///
/// Returns `None` when the text has a dot and `tables` is `None`.
pub fn real_literal(tables: Option<&Tables>, text: &[u8]) -> Option<F32> {
    let mut int_part: i32 = 0;
    let mut dot = None;
    for (k, &c) in text.iter().enumerate() {
        if c == b'.' {
            dot = Some(k);
            break;
        }
        int_part = (c as i32 - 48).wrapping_add(int_part.wrapping_mul(10));
    }
    let Some(dot) = dot else {
        return Some(conv::i2r(int_part));
    };
    let mut frac: i32 = 0;
    let mut scale: i32 = 1;
    for &c in &text[dot + 1..] {
        scale = scale.wrapping_mul(10);
        frac = (c as i32 - 48).wrapping_add(frac.wrapping_mul(10));
    }
    let fraction = arith::div(conv::i2r(frac), conv::i2r(scale), tables?);
    Some(arith::add(conv::i2r(int_part), fraction))
}

/// Parses a real from text as the S2R helper does (`0x6F6F0520`).
///
/// - An optional `+` or `-` is accepted (once).
/// - Digits are read until a space, tab, `;`, NUL, a second dot, or any other character.
///   Leading zeros do not count as significant. The first nine significant digits go into
///   an integer; later integer digits scale the result up by ten each, and later fraction
///   digits are dropped.
/// - The value is `I2R(digits) / 10^k` for `k` fraction digits (soft DIV, not correctly
///   rounded), or `I2R(digits) * 10^k` when the exponent is negative (soft MUL). Text with no
///   digits gives `+0`.
///
/// Returns `None` when a division is needed and `tables` is `None` (every non-negative
/// exponent, including text without a fraction, divides).
pub fn s2r(tables: Option<&Tables>, text: &[u8]) -> Option<F32> {
    let text = match text.iter().position(|&b| b == 0) {
        Some(end) => &text[..end],
        None => text,
    };
    let mut i = 0;
    let mut sign: i32 = 1;
    match text.first() {
        Some(b'-') => {
            sign = -1;
            i = 1;
        }
        Some(b'+') => i = 1,
        _ => {}
    }
    let mut mantissa: i32 = 0;
    let mut significant: i32 = 0;
    let mut exponent: i32 = 0;
    let mut scale_step: i32 = 0;
    let mut started = false;
    let mut seen_dot = false;
    while let Some(&c) = text.get(i) {
        if c == b' ' || c == b';' || c == b'\t' {
            break;
        }
        if c.is_ascii_digit() {
            if c != b'0' {
                started = true;
            }
            significant += started as i32;
            if significant <= 9 {
                exponent = exponent.wrapping_add(scale_step);
                mantissa = (c as i32 - 48).wrapping_add(mantissa.wrapping_mul(10));
            } else {
                if significant == 10 {
                    scale_step -= 1;
                }
                exponent = exponent.wrapping_add(scale_step);
            }
        } else if c == b'.' {
            if seen_dot {
                break;
            }
            scale_step += 1;
            seen_dot = true;
        } else {
            break;
        }
        i += 1;
    }
    let value = conv::i2r(mantissa.wrapping_mul(sign));
    if exponent < 0 {
        let p = math::pow_int(TEN, exponent.wrapping_neg());
        Some(arith::mul(value, p))
    } else {
        let p = math::pow_int(TEN, exponent);
        Some(arith::div(value, p, tables?))
    }
}
