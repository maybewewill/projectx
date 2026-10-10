//! Square root and transcendental helpers.

use crate::arith::mul;
use crate::conv;
use crate::tables::Tables;
use crate::F32;

/// Floor of the square root of a 32-bit unsigned value, computed two bits per step
/// (restoring method). Original: `0x6F6EED10`.
fn isqrt32(x: u32) -> u32 {
    let mut x = x;
    let mut rem: u32 = 0;
    let mut root: u32 = 0;
    for _ in 0..16 {
        let top = x >> 30;
        rem = (rem << 2) | top;
        x <<= 2;
        root <<= 1;
        if root < rem {
            rem = rem.wrapping_sub(root).wrapping_sub(1);
            root = root.wrapping_add(2);
        }
    }
    root >> 1
}

/// Square root, `0x6F6EF750`.
///
/// Zero, negative numbers and `-0` give `+0`. For positive inputs the exponent is
/// halved (odd exponents are scaled by `sqrt(2)`, mantissa `0x3504F3`), the mantissa
/// comes from a linear fit to the integer square root of the 32-bit value
/// `0x80000000 | frac << 8 | frac >> 15`, and the two factors are multiplied with
/// [`mul`]. Not correctly rounded.
pub fn sqrt(a: F32, _t: &Tables) -> F32 {
    let x = a.0;
    if x == 0 || x & 0x8000_0000 != 0 {
        return F32::ZERO;
    }
    let frac = x & 0x007f_ffff;
    let e = ((x >> 23) & 0xff) as i32 - 0x7f;
    let v = 0x8000_0000 | (frac << 8) | (frac >> 15);
    let r = isqrt32(v);
    let m0 = 0x3f80_0000 | (r.wrapping_sub(0xb504).wrapping_mul(0xb505) >> 8);
    let odd = e & 1;
    let mask = (-odd) as u32;
    let h1 = e - odd;
    let half = h1 / 2;
    let s2 = (((half + 0x7f) as u32) << 23) | (mask & 0x0035_04f3);
    mul(F32(s2), F32(m0))
}

/// One table-driven quarter-wave evaluation shared by sin, cos and tan.
///
/// `x` is scaled by `4/pi * 2^17` (`0x4822F983`, a soft multiply) and truncated to an
/// integer `k`. Bits 18-19 of `k` give the quadrant (plus `quad_add`), bits 8-17 index
/// the sine table (1024 segments of a quarter wave, 2^31 scale), and the low byte is the
/// interpolation weight, replicated to 32 bits. Odd quadrants walk the table backwards.
/// The interpolated value is negated in the upper half-wave, converted with [`i2r`] and
/// then divided by 2^31 by subtracting 31 from the exponent field (unless it is zero).
fn trig_core(x: F32, t: &Tables, quad_add: i32) -> F32 {
    let scaled = mul(x, F32(0x4822_f983));
    let k = conv::f2i(scaled);
    let weight = ((k & 0xff) as u32) * 0x0101_0101;
    let q = (((k >> 18) + quad_add) & 3) as u32;
    let idx = ((k >> 8) & 0x3ff) as usize;
    let (base, d) = if q & 1 == 0 {
        let t0 = t.sin_at(idx);
        (t0, t.sin_at(idx + 1).wrapping_sub(t0))
    } else {
        let t0 = t.sin_at(1024 - idx);
        (t0, t0.wrapping_sub(t.sin_at(1023 - idx)))
    };
    let hi = ((d as u64 * weight as u64) >> 32) as u32;
    let mut v = if q & 1 == 0 {
        base.wrapping_add(hi)
    } else {
        base.wrapping_sub(hi)
    };
    if q & 2 != 0 {
        v = v.wrapping_neg();
    }
    let r = conv::i2r(v as i32).0;
    let r = if r & 0x7f80_0000 != 0 {
        r.wrapping_sub(0x0f80_0000)
    } else {
        r
    };
    F32(r)
}

/// Sine (`0x6F6EF820`). Not correctly rounded; a table approximation (see [`trig_core`]).
pub fn sin(a: F32, t: &Tables) -> F32 {
    trig_core(a, t, 0)
}

/// Cosine (`0x6F6EF8D0`): the same evaluation one quadrant ahead of [`sin`].
pub fn cos(a: F32, t: &Tables) -> F32 {
    trig_core(a, t, 1)
}

/// Tangent (`0x6F6F0140`): `sin(x)` divided by `cos(x)` with the soft division.
pub fn tan(a: F32, t: &Tables) -> F32 {
    crate::arith::div(sin(a, t), cos(a, t), t)
}

/// `-pi/2`, `+pi/2` and `pi` as stored by the original's constant pool
/// (`0x6FAAE5E4`, `0x6FAAE5FC`, `0x6FAAE600`).
const NEG_HALF_PI: u32 = 0xbfc9_0fdb;
const HALF_PI: u32 = 0x3fc9_0fdb;
const PI: u32 = 0x4049_0fdb;
/// Constant subtracted from the table interpolation of the small-argument path of asin
/// (acos adds nothing).
const ARC_BIAS: u32 = 0x3243_f6a8;

/// Small-argument arc path shared by asin and acos (`|x| <= 0.9921875`, `x * 2^30`
/// truncated, clamped, table interpolation, scaled by `2^-29`). `bias` is added to the
/// result before conversion, and `negate` selects `m = -k` (asin) or `m = k` (acos).
fn arc_small(x: u32, t: &Tables, bias: u32, negate: bool) -> F32 {
    let k0 = conv::f2i(F32(x.wrapping_add(0x0f00_0000)));
    let biased = (k0 as u32).wrapping_add(0x3fff_ffff);
    let k = if biased > 0x7fff_fffe {
        if (biased as i32) < 0 {
            -0x3fff_ffff
        } else {
            0x3fff_ffff
        }
    } else {
        k0
    };
    let m = if negate { k.wrapping_neg() } else { k };
    let weight = ((m as u32) << 12) | (((m as u32) << 12) >> 20);
    let idx = ((m >> 20) & 0x3ff) as usize;
    let v = if m >= 0 {
        let base = t.asin_at(idx);
        let d = base.wrapping_sub(t.asin_at(idx + 1));
        let hi = ((d as u64 * weight as u64) >> 32) as u32;
        hi.wrapping_neg().wrapping_add(base).wrapping_add(bias)
    } else {
        let b = t.asin_at(1024 - idx);
        let d = t.asin_at(1023 - idx).wrapping_sub(b);
        let hi = ((d as u64 * weight as u64) >> 32) as u32;
        hi.wrapping_neg()
            .wrapping_add(0x6487_ed51u32.wrapping_sub(b))
            .wrapping_add(bias)
    };
    let r = conv::i2r(v as i32).0;
    let r = if r & 0x7f80_0000 != 0 {
        r.wrapping_sub(0x0e80_0000)
    } else {
        r
    };
    F32(r)
}

/// Large-argument arc magnitude shared by asin and acos: `|x|` clamped to `1.0`, the
/// integer `1.0 - |x|` normalised, second table, scaled by `2^-34`.
fn arc_large(x: u32, t: &Tables) -> F32 {
    let abs = x & 0x7fff_ffff;
    let mag = if abs <= 0x3f80_0000 { abs } else { 0x3f80_0000 };
    let esi = 0x3f80_0000u32 - mag;
    let lz = esi.leading_zeros();
    let shifted = if lz < 32 { (!esi) << lz } else { 0 };
    let index = (lz.wrapping_mul(8).wrapping_sub(0x78)) | (shifted >> 28);
    let base = t.asin2_at(index as usize);
    let d = base.wrapping_sub(t.asin2_at(index as usize + 1));
    let hi = ((d as u64 * ((shifted << 4) as u64)) >> 32) as u32;
    let v = base.wrapping_sub(hi);
    let r = conv::i2r(v as i32).0;
    let r = if r & 0x7f80_0000 != 0 {
        r.wrapping_sub(0x1100_0000)
    } else {
        r
    };
    F32(r)
}

/// Arc sine (`0x6F6EFC70`).
///
/// For `|x| <= 0.9921875` (bits up to `0x3F7E8000`) the small-argument table path is
/// used (see [`arc_small`] with bias `-0x3243F6A8`). Otherwise the magnitude
/// (`pi/2 - value`, see [`arc_large`]) is combined with `+-pi/2` by the sign of `x`.
pub fn asin(a: F32, t: &Tables) -> F32 {
    let x = a.0;
    if x & 0x7fff_ffff <= 0x3f7e_8000 {
        return arc_small(x, t, 0u32.wrapping_sub(ARC_BIAS), true);
    }
    let r = arc_large(x, t);
    if x & 0x8000_0000 != 0 {
        crate::arith::add(F32(NEG_HALF_PI), r)
    } else {
        crate::arith::sub(F32(HALF_PI), r)
    }
}

/// Arc cosine (`0x6F6EFB00`): the same two paths as [`asin`] with no bias in the small path.
/// For negative large arguments the result is `pi - value`.
pub fn acos(a: F32, t: &Tables) -> F32 {
    let x = a.0;
    if x & 0x7fff_ffff <= 0x3f7e_8000 {
        return arc_small(x, t, 0, false);
    }
    let r = arc_large(x, t);
    if x & 0x8000_0000 != 0 {
        crate::arith::sub(F32(PI), r)
    } else {
        r
    }
}

/// Arc tangent (`0x6F6F0170`).
///
/// Works on `|x|`: values above 1 are inverted (`1/|x|`, flagged, `pi/2 - result` at the
/// end). Values above `2 - sqrt(3)` are reduced with `(v - 1/sqrt(3)) / (1 + v/sqrt(3))`
/// (flagged, `+ pi/6` at the end). The core is the odd rational approximation
/// `v * (c0 + c1*s) / (1 + c2*s)` with `s = v*v` and the coefficients of the constant pool
/// (`0x6FADA8F0..0x6FADA8F8`). The sign of `x` is applied last (not for `-0`).
pub fn atan(a: F32, t: &Tables) -> F32 {
    let x = a.0;
    let mut v = F32(x & 0x7fff_ffff);
    let inverted = crate::arith::lt(F32::ONE, v);
    if inverted {
        v = crate::arith::recip(v, t);
    }
    let reduced = crate::arith::lt(F32(0x3e89_30a3), v);
    if reduced {
        let t1 = mul(v, F32(0x3f13_cd3a));
        let den = crate::arith::add(F32::ONE, t1);
        let num = crate::arith::add(v, F32(0xbf13_cd3a));
        v = crate::arith::div(num, den, t);
    }
    let s = mul(v, v);
    let t1 = mul(s, F32(0x3f17_592e));
    let t2 = crate::arith::add(F32::ONE, t1);
    let t3 = mul(s, F32(0x3e84_15a6));
    let t4 = crate::arith::add(F32(0x3f7f_fff0), t3);
    let t5 = mul(v, t4);
    let mut r = crate::arith::div(t5, t2, t);
    if reduced {
        r = crate::arith::add(r, F32(0x3f06_0a92));
    }
    if inverted {
        r = crate::arith::sub(F32(HALF_PI), r);
    }
    if x & 0x8000_0000 != 0 && x << 1 != 0 {
        r = crate::arith::neg(r);
    }
    r
}

/// Two-argument arc tangent (`0x6F6F02D0`), standard `atan2(y, x)` convention.
///
/// The original takes `y` in EDX and `x` on the stack. If `x` has a zero exponent the
/// result starts as `pi/2`; otherwise it is `atan(|y / x|)`. A negative (non-zero) `x`
/// gives `pi - r`, a negative (non-zero) `y` negates the result. The division is the
/// soft [`crate::arith::div`].
pub fn atan2(y: F32, x: F32, t: &Tables) -> F32 {
    let mut r = if x.0 & 0x7f80_0000 == 0 {
        F32(HALF_PI)
    } else {
        let q = crate::arith::div(y, x, t);
        atan(F32(q.0 & 0x7fff_ffff), t)
    };
    if x.0 & 0x8000_0000 != 0 && x.0 << 1 != 0 {
        r = crate::arith::sub(F32(PI), r);
    }
    if y.0 & 0x8000_0000 != 0 && y.0 << 1 != 0 {
        r = crate::arith::neg(r);
    }
    r
}

/// Truncation toward zero to an integral real, `0x6F6EF290`. Values with a biased exponent
/// below 127 become `+0`; exponents from 23 up return the input; in between the low
/// fraction bits are cleared with a shifted sign mask.
pub fn trunc(a: F32) -> F32 {
    let v = a.0;
    let e = ((v >> 23) & 0xff) as i32 - 0x7f;
    if e < 0 {
        return F32::ZERO;
    }
    if e >= 0x17 {
        return a;
    }
    let mask = (0x8000_0000u32 as i32 >> (e as u32 + 8)) as u32;
    F32(v & mask)
}

/// Integer power `base ** n` by binary exponentiation, `0x6F6F00E0`. Starts at `1.0`;
/// every set bit multiplies the result, the base is squared after each bit.
///
/// The original shifts the count arithmetically, so a negative `n` never reaches zero
/// and the original does not return. This port shifts logically instead (at most 32
/// steps). Callers in the helpers only pass `n >= 0`, where both agree.
pub fn pow_int(base: F32, n: i32) -> F32 {
    let mut result = F32::ONE;
    let mut b = base;
    let mut n = n as u32;
    while n != 0 {
        if n & 1 != 0 {
            result = mul(result, b);
        }
        b = mul(b, b);
        n >>= 1;
    }
    result
}

/// Exponential core, `0x6F6F0690`: `exp(x)` for the value of `x`.
///
/// `x` is first scaled by 4 (exponent + 2) when it is not zero or denormal; the integer
/// part `k` (through [`trunc`]) and the fraction `f` are split. The fraction, divided by 4
/// (exponent - 2), gives `g`, unless the test on `f - 0x01800000` fails, in which case `g`
/// is zero. `exp(g)` is evaluated with a Horner chain of the Taylor coefficients
/// (1/2, 1/6, 1/24, 1/120 from the constant pool), and the result is multiplied by
/// `e^(1/4)` raised to `k` (`0x3FA45AF2`, see [`pow_int`]).
fn exp_core(v: u32, _t: &Tables) -> F32 {
    let x = v.wrapping_add(if v & 0x7f80_0000 != 0 { 0x0100_0000 } else { 0 });
    let k = trunc(F32(x));
    let f = crate::arith::sub(F32(x), k);
    let fb = f.0;
    let keep = ((fb.wrapping_add(0xfe80_0000) ^ fb) & 0x8000_0000) == 0;
    let g = if keep {
        fb.wrapping_sub(0x0100_0000)
    } else {
        0
    };
    let g = F32(g);
    let mut y = mul(F32(0x3c1a_534c), g);
    y = crate::arith::add(y, F32(0x3d29_6ec9));
    y = mul(y, g);
    y = crate::arith::add(y, F32(0x3e2a_b479));
    y = mul(y, g);
    y = crate::arith::add(y, F32(0x3eff_ffbd));
    y = mul(y, g);
    y = crate::arith::add(y, F32::ONE);
    y = mul(y, g);
    y = crate::arith::add(y, F32::ONE);
    let n = conv::f2i(k);
    let p = pow_int(F32(0x3fa4_5af2), n);
    mul(y, p)
}

/// Exponential, `0x6F6F07B0`: `exp(x)`. A negative `x` (non-zero) gives `1 / exp(-x)`
/// with the table reciprocal; otherwise [`exp_core`].
pub fn exp(a: F32, t: &Tables) -> F32 {
    let x = a.0;
    if x & 0x8000_0000 != 0 && x << 1 != 0 {
        crate::arith::recip(exp_core(x ^ 0x8000_0000, t), t)
    } else {
        exp_core(x, t)
    }
}

/// Series core of the logarithm, `0x6F6EFF20`, for `v` in `[1, 2)`:
/// `q = (v - 1) / (v + 1)`, then the Pade form `2 q (1 + c1 s) / (1 + c2 s)` with
/// `s = q * q`, `c1 = -4/15` (`0xBE88D424`) and `c2 = -0.6` (`0xBF19BF59`).
fn ln_series(v: F32, t: &Tables) -> F32 {
    let a = crate::arith::add(v, F32::ONE);
    let b = crate::arith::add(v, F32(0xbf80_0000));
    let q = crate::arith::div(b, a, t);
    let s = mul(q, q);
    let u = mul(s, F32(0xbe88_d424));
    let w = crate::arith::add(F32::ONE, u);
    let x = mul(q, w);
    // doubling: add one to the exponent field when it is not zero
    let x2 = if x.0 & 0x7f80_0000 != 0 {
        F32(x.0.wrapping_add(0x0080_0000))
    } else {
        x
    };
    let z = mul(s, F32(0xbf19_bf59));
    let y = crate::arith::add(F32::ONE, z);
    crate::arith::div(x2, y, t)
}

/// Natural logarithm, `0x6F6F0060`: `ln(x) = ln2 * (e + log2(e) * series(m))` with
/// `m = 1.f` in `[1, 2)` and `e` the unbiased exponent.
pub fn ln(a: F32, t: &Tables) -> F32 {
    let x = a.0;
    let m = F32((x & 0x007f_ffff) | 0x3f80_0000);
    let l = ln_series(m, t);
    let tt = mul(l, F32(0x3fb8_aa3b));
    let e = ((x >> 23) & 0xff) as i32 - 0x7f;
    let ee = conv::i2r(e);
    let sum = crate::arith::add(ee, tt);
    mul(sum, F32(0x3f31_7218))
}

/// Floor, `0x6F6EF170`, for `|x| < 2^23`: the integer part, minus one for negative
/// non-integers. `+-0` gives `+0`. Infinities, NaNs and values with `|x| >= 2^23` (already
/// integral) are returned unchanged.
pub fn floor(a: F32) -> F32 {
    let x = a.0;
    if x & 0x7fff_ffff == 0 {
        return F32::ZERO;
    }
    if x & 0x7f80_0000 == 0x7f80_0000 || (x & 0x7f80_0000) >= 0x4b00_0000 {
        return a;
    }
    let n = conv::f2i(a);
    let n = if (x as i32) < 0 && conv::i2r(n).0 != x {
        n - 1
    } else {
        n
    };
    conv::i2r(n)
}

/// Ceiling, `0x6F6EF200`, for `|x| < 2^23`: the integer part, plus one for positive
/// non-integers. Unlike the IEEE function, `ceil(+-0)` gives `1.0` (observed in the
/// original). Infinities, NaNs and values with `|x| >= 2^23` are returned unchanged.
pub fn ceil(a: F32) -> F32 {
    let x = a.0;
    if x & 0x7fff_ffff == 0 {
        return F32::ONE;
    }
    if x & 0x7f80_0000 == 0x7f80_0000 || (x & 0x7f80_0000) >= 0x4b00_0000 {
        return a;
    }
    let n = conv::f2i(a);
    let n = if (x as i32) >= 0 && conv::i2r(n).0 != x {
        n + 1
    } else {
        n
    };
    conv::i2r(n)
}

/// Round half up, `0x6F6EF330`: `floor(x + 0.5)` with the soft add and [`floor`].
pub fn round(a: F32) -> F32 {
    floor(crate::arith::add(a, F32(0x3f00_0000)))
}

/// Power, `0x6F6F0810`.
///
/// If `exp` is an integer that is not negative (`-0` included), the result is
/// `base ** exp` by [`pow_int`] with the integer `f2i(exp)`. Otherwise a zero base
/// gives `+0` and anything else gives `exp(exp * ln(base))`. Negative bases with
/// non-integer exponents are not special-cased (the logarithm of a negative bit
/// pattern is whatever the formula produces).
pub fn pow(base: F32, e: F32, t: &Tables) -> F32 {
    let integral = crate::arith::eq(e, trunc(e));
    if integral && (e.0 & 0x8000_0000 == 0 || e.0 << 1 == 0) {
        return pow_int(base, conv::f2i(e));
    }
    if base.0 & 0x7f80_0000 == 0 {
        return F32::ZERO;
    }
    let l = ln(base, t);
    exp(mul(e, l), t)
}
