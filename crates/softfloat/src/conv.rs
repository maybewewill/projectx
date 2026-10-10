//! Integer <-> real conversions.
//!
//! - [`i2r`]: `0x6F6EF350`, the JASS `I2R` native and the VM's integer-to-real op.
//! - [`f2i`]: `0x6F6EEC20`, the internal truncation used by the trigonometric helpers
//!   and many gameplay paths. It wraps on overflow.
//! - [`r2i`]: `0x6F6EEC70`, the JASS `R2I` native. It saturates for large magnitudes.

use crate::{clz, F32};

/// Integer to real, `0x6F6EF350`.
///
/// Zero maps to `+0.0`. Otherwise the magnitude is shifted so that its leading one
/// sits at bit 23 (shifting right truncates the low bits, so only 24 significant bits
/// survive), the exponent is `158 - leading_zeros`, and the sign is copied from the
/// integer. `I2R(16777219) == 16777218.0`.
pub fn i2r(n: i32) -> F32 {
    if n == 0 {
        return F32::ZERO;
    }
    let sign = (n as u32) & 0x8000_0000;
    let m = n.wrapping_neg() as u32;
    let m = if n < 0 { m } else { n as u32 };
    let lz = clz(m);
    let shifted = if lz >= 8 {
        m << (lz - 8)
    } else {
        ((m as i32) >> (8 - lz)) as u32
    };
    let exp = (0x1f - lz + 0x7f) << 23;
    F32(exp | (shifted & 0x007f_ffff) | sign)
}

/// Real to integer, truncating, wrapping on overflow, `0x6F6EEC20`.
///
/// `|x| < 1` (biased exponent below 127) gives 0. Otherwise the 24-bit mantissa is
/// shifted by `150 - exponent`; for exponents above 150 the shift is to the left and
/// the bits that leave the 32-bit word are lost (x86 shift counts are taken mod 32).
/// The sign is applied last with two's complement.
pub fn f2i(x: F32) -> i32 {
    let v = x.0;
    let e = (v >> 23) & 0xff;
    if e < 0x7f {
        return 0;
    }
    let m = (v & 0x007f_ffff) | 0x0080_0000;
    let sh = 0x96i32 - e as i32;
    let mag = if sh >= 0 {
        m >> ((sh as u32) & 31)
    } else {
        m << (((-sh) as u32) & 31)
    };
    let s = (v as i32) >> 31;
    ((mag as i32) ^ s).wrapping_sub(s)
}

/// Real to integer for the JASS `R2I` native, `0x6F6EEC70`.
///
/// Same as [`f2i`] for biased exponents up to `0x9d`. Above that (including
/// infinities and NaN bit patterns) the result saturates: `i32::MIN` for a negative
/// sign bit, `i32::MAX` otherwise.
pub fn r2i(x: F32) -> i32 {
    let v = x.0;
    let e = (v >> 23) & 0xff;
    if e < 0x7f {
        return 0;
    }
    if e > 0x9d {
        return if v & 0x8000_0000 != 0 {
            i32::MIN
        } else {
            i32::MAX
        };
    }
    f2i(x)
}
