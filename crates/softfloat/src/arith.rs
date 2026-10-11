//! Basic real arithmetic and comparisons.
//!
//! Original entry points (all take their operands by pointer; the Rust versions
//! take them by value):
//! - add `0x6F6EF000`, sub `0x6F6EEEF0`, mul `0x6F6EEE20`, div `0x6F6EFE10`
//!   (div calls the reciprocal helper `0x6F6EF100`).
//! - compares `0x6F00C110`, `0x6F01FD40`, `0x6F01FD60`, `0x6F01FD80`, `0x6F009AE0`
//!   (x87 `fcompp`, IEEE ordered semantics) and `0x6F00C3F0` (approximate equality).

use crate::tables::Tables;
use crate::F32;

const SIGN: u32 = 0x8000_0000;
const EXP_MASK: u32 = 0x7f80_0000;
const FRAC_MASK: u32 = 0x007f_ffff;
const IMPLICIT: u32 = 0x0080_0000;

/// Real addition, `0x6F6EF000`.
///
/// Mantissas are taken as signed `2 * (1.f)` values, the smaller operand is shifted
/// right (arithmetic shift, so it rounds toward minus infinity), the sum is normalised
/// with a left or right shift and the result is truncated. A zero exponent on either
/// side returns the other operand unchanged.
#[inline]
pub fn add(a: F32, b: F32) -> F32 {
    F32(add_bits(a.0, b.0))
}

/// Real subtraction, `0x6F6EEEF0`: `a - b`, implemented as `add(a, -b)`.
///
/// The original flips the sign bit of `b` before running the same routine, so
/// `sub(a, b)` is bit-identical to `add(a, neg(b))`, including the zero-exponent
/// cases (a zero-exponent `a` returns `-b`).
#[inline]
pub fn sub(a: F32, b: F32) -> F32 {
    F32(add_bits(a.0, b.0 ^ SIGN))
}

fn add_bits(a: u32, b: u32) -> u32 {
    let ea = a & EXP_MASK;
    if ea == 0 {
        return b;
    }
    let eb = b & EXP_MASK;
    if eb == 0 {
        return a;
    }
    let ma = signed_double_mantissa(a);
    let mb = signed_double_mantissa(b);
    // exponent difference, as the original computes it (eb - ea, 32-bit)
    let d = eb.wrapping_sub(ea) as i32;
    let (x, y, base) = if d <= 0 {
        if d <= -0x0b80_0000 {
            return a;
        }
        let k = ((ea.wrapping_sub(eb)) >> 23) & 31;
        (ma, mb >> k, ea)
    } else {
        if d >= 0x0b80_0000 {
            return b;
        }
        let k = ((d as u32) >> 23) & 31;
        (ma >> k, mb, eb)
    };
    let mut s = x.wrapping_add(y);
    if s == 0 {
        return 0;
    }
    let sign = (s as u32) & SIGN;
    if s < 0 {
        s = s.wrapping_neg();
    }
    let s = s as u32;
    let lz = s.leading_zeros();
    let norm = if lz > 8 { s << (lz - 8) } else { s >> (8 - lz) };
    let exp_adj = ((7u32.wrapping_sub(lz)) << 23).wrapping_add(base);
    exp_adj | (norm & FRAC_MASK) | sign
}

/// `2 * (implicit-1 mantissa)` with the sign of `v` applied (two's complement).
#[inline]
fn signed_double_mantissa(v: u32) -> i32 {
    let m = (((v & FRAC_MASK) | IMPLICIT) << 1) as i32;
    let s = (v as i32) >> 31;
    (m ^ s).wrapping_sub(s)
}

/// Real multiplication, `0x6F6EEE20`.
///
/// Full path when both fractions are non-zero: the high 32 bits of the product of
/// the two `(f - 2^23) << 8` values, normalised by one bit, truncated to 23 bits.
/// Otherwise (one fraction is zero, i.e. a power of two) the exponents are added and
/// the other fraction is kept. Results whose biased exponent goes below 1 become zero.
pub fn mul(a: F32, b: F32) -> F32 {
    let (a, b) = (a.0, b.0);
    let fa = a & FRAC_MASK;
    let fb = b & FRAC_MASK;
    let ea = a & EXP_MASK;
    let eb = b & EXP_MASK;
    let sign = (a ^ b) & SIGN;
    if fa != 0 && fb != 0 {
        let x = ((fa | 0xff80_0000) << 8) as u64;
        let y = ((fb | 0xff80_0000) << 8) as u64;
        let h = ((x * y) >> 32) as u32;
        let s = h >> 31;
        let e = ea.wrapping_add(eb).wrapping_sub(0x3f80_0000);
        let e2 = e.wrapping_add(s << 23);
        let frac = ((h >> s) >> 7) & FRAC_MASK;
        let keep = (e.wrapping_sub(0x0080_0000) as i32) >= 0;
        if keep {
            F32(frac | e2 | sign)
        } else {
            F32::ZERO
        }
    } else if ea != 0 && eb != 0 {
        let e = ea.wrapping_add(eb).wrapping_sub(0x3f80_0000);
        let keep = (e.wrapping_sub(0x0080_0000) as i32) >= 0;
        if keep {
            F32(e | fa | fb | sign)
        } else {
            F32::ZERO
        }
    } else {
        F32::ZERO
    }
}

/// Fused squared distance, `0x6F6EF3B0`: `(bx - ax)^2 + (by - ay)^2`, with the operands in the
/// original's order: `(ax, ay)` is the point read through the first pointer (EDX), `(bx, by)`
/// the one behind the stack argument.
///
/// The disassembly shows the following steps (same cores as the public helpers):
/// - If both points are bit-identical (`ax == bx` and `ay == by` as patterns, so `+0` vs
///   `-0` is not identical), the result is `+0` at once.
/// - Each difference is the add core applied to `(ax ^ sign, bx)`, i.e. `sub(bx, ax)` with
///   the zero-exponent rules of the add core.
/// - Each square is the multiply core applied to the difference with itself (the sign is
///   positive).
/// - The two squares are summed with the add core (both non-negative).
///
/// Checked in Unicorn on 12,000 cases (`tools/emu/softfloat_vectors.py`, `dist2`): 0
/// mismatches. The composition `mul(sub(bx, ax), ..)` with the same helpers also matches
/// those cases; the reversed order `sub(ax, bx)` does not (about 12% of them differ). The
/// function is kept as its own entry point because that operand order is part of the
/// behaviour.
pub fn dist2(ax: F32, ay: F32, bx: F32, by: F32) -> F32 {
    if ax.0 == bx.0 && ay.0 == by.0 {
        return F32::ZERO;
    }
    let dx = add_bits(ax.0 ^ SIGN, bx.0);
    let dy = add_bits(ay.0 ^ SIGN, by.0);
    let sx = mul(F32(dx), F32(dx));
    let sy = mul(F32(dy), F32(dy));
    F32(add_bits(sx.0, sy.0))
}

/// Real division, `0x6F6EFE10`: `a / b`.
///
/// If the two operands have identical bits the result is `1.0` (so `x / x` is
/// `1.0` even for `x = 0`). Otherwise the result is `a * recip(b)`, where `recip`
/// is the table-driven approximation of `1 / b` (see [`recip`]), so division is
/// **not** correctly rounded.
pub fn div(a: F32, b: F32, tables: &Tables) -> F32 {
    if a.0 == b.0 {
        return F32::ONE;
    }
    mul(a, recip(b, tables))
}

/// Table-driven reciprocal, `0x6F6EF100`.
///
/// The fraction of `b` selects one of 1024 segments (its top 10 bits); the segment
/// is linearly interpolated using the next 13 fraction bits, and the exponent is
/// rebuilt from `253 - e`. Results that would be below the normal range (mantissa
/// field below `2^23` after the exponent is applied) are zero. The sign of `b` is
/// kept.
pub fn recip(b: F32, tables: &Tables) -> F32 {
    let x = b.0;
    let frac = x & FRAC_MASK;
    let idx = (frac >> 13) as usize;
    let t0 = tables.recip_at(idx);
    let t1 = tables.recip_at(idx + 1);
    let low = frac << 19;
    let d = t0.wrapping_sub(t1);
    let f = ((low | (low >> 13)) >> 13) | low;
    let high = ((d as u64 * f as u64) >> 32) as u32;
    let mut r = t0.wrapping_sub(high);
    r = r.wrapping_sub(x & EXP_MASK);
    r = r.wrapping_add(0x7e80_0000);
    if (r.wrapping_sub(0x0080_0000) as i32) < 0 {
        F32::ZERO
    } else {
        F32((x & SIGN) | r)
    }
}

/// Sign flip, the VM's unary minus on reals (`x ^ 0x80000000`, no other checks).
#[inline]
pub fn neg(a: F32) -> F32 {
    F32(a.0 ^ SIGN)
}

/// Ordered `a < b` (IEEE; false when either side is NaN). Original `0x6F00C110`.
#[inline]
pub fn lt(a: F32, b: F32) -> bool {
    a.to_f32() < b.to_f32()
}

/// Ordered `a <= b`. Original `0x6F01FD60`.
#[inline]
pub fn le(a: F32, b: F32) -> bool {
    a.to_f32() <= b.to_f32()
}

/// Ordered `a > b`. Original `0x6F01FD40`.
#[inline]
pub fn gt(a: F32, b: F32) -> bool {
    a.to_f32() > b.to_f32()
}

/// Ordered `a == b` (so `+0 == -0`; false for NaN). Original `0x6F009AE0`.
#[inline]
pub fn eq(a: F32, b: F32) -> bool {
    a.to_f32() == b.to_f32()
}

/// Approximate equality used by the VM for real `==`: `|a - b| < 0.001`, where the
/// difference is computed with [`sub`]. Original `0x6F00C3F0`, constant `0.001`
/// at `0x6FAAE47C`.
#[inline]
pub fn approx_eq(a: F32, b: F32) -> bool {
    let d = sub(a, b);
    d.to_f32().abs() < f32::from_bits(0x3a83_126f)
}
