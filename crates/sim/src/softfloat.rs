//! Single-precision helpers that reproduce the engine's own float routines.
//!
//! The 1.26a engine does not use the FPU or IEEE rounding for the random real
//! numbers. It calls three small routines (add, subtract, multiply) that work
//! on the raw float32 bit patterns with integer arithmetic. Their results differ
//! from correctly rounded IEEE results in the last bit fairly often, so the
//! simulation must use these functions to stay in lockstep.
//!
//! Behaviour, in the terms that matter here:
//!
//! * Operands are taken as bit patterns. Mantissas are truncated, not rounded.
//! * A zero exponent field is treated as "zero": `add` returns the other operand
//!   unchanged, and `mul` returns +0 unless both mantissas are non-zero.
//! * Denormal inputs with a non-zero mantissa are treated as normal numbers with
//!   an implicit leading one (the exponent is not checked).
//! * Exponent differences of 24 or more drop the smaller operand.
//! * Results with an exponent below the normal range are flushed to +0. Results
//!   at or above 2^31 in the internal exponent field can also come out as +0.
//!   No infinity or NaN is produced.
//!
//! Verified bit for bit against the original machine code (`tools/emu/gen_vectors.py`).

const EXP_MASK: u32 = 0x7F80_0000;
const MANT_MASK: u32 = 0x007F_FFFF;
const IMPLICIT_ONE: u32 = 0x0080_0000;
const SIGN_BIT: u32 = 0x8000_0000;
/// Largest exponent-field difference that still lets the smaller operand
/// contribute (23 exponent steps, minus one unit of the field).
const NEAR_LIMIT: i32 = 0x0B7F_FFFF;

/// Mantissa with the implicit one, doubled, as a two's complement value whose
/// sign comes from `bits`. The doubling keeps one extra bit below the leading one.
fn signed_scaled_mantissa(bits: u32) -> i32 {
    let mag = ((bits & MANT_MASK) | IMPLICIT_ONE) << 1;
    let neg = ((bits as i32) >> 31) as u32; // 0 or all ones
    ((mag ^ neg).wrapping_sub(neg)) as i32
}

/// Normalises a non-zero magnitude so its leading one lands on bit 23, adjusting
/// the exponent field by the same amount. Bits shifted out are discarded.
fn normalize(magnitude: u32, sign: u32, exponent_field: u32) -> u32 {
    let clz = magnitude.leading_zeros() as i32;
    let shift = 8 - clz;
    let frac = if shift < 0 {
        magnitude << ((-shift) as u32 & 0x1F)
    } else {
        ((magnitude as i32) >> (shift as u32 & 0x1F)) as u32
    };
    let exp = exponent_field.wrapping_add(((7 - clz) as u32).wrapping_mul(IMPLICIT_ONE));
    exp | (frac & MANT_MASK) | sign
}

/// `x + y` on float32 bit patterns.
pub fn add(x: u32, y: u32) -> u32 {
    let ex = x & EXP_MASK;
    if ex == 0 {
        return y;
    }
    let ey = y & EXP_MASK;
    if ey == 0 {
        return x;
    }
    let mut sx = signed_scaled_mantissa(x);
    let mut sy = signed_scaled_mantissa(y);
    let diff = ey.wrapping_sub(ex) as i32;
    let mut exp = ex;
    if diff <= 0 {
        if diff < -NEAR_LIMIT {
            return x;
        }
        sy >>= (ex.wrapping_sub(ey) >> 23) & 0x1F;
    } else {
        if diff > NEAR_LIMIT {
            return y;
        }
        sx >>= ((diff as u32) >> 23) & 0x1F;
        exp = ey;
    }
    let sum = sx.wrapping_add(sy) as u32;
    if sum == 0 {
        return 0;
    }
    let sign = sum & SIGN_BIT;
    let magnitude = if (sum as i32) < 0 {
        sum.wrapping_neg()
    } else {
        sum
    };
    normalize(magnitude, sign, exp)
}

/// `x - y` on float32 bit patterns. Same as `add(x, -y)`.
pub fn sub(x: u32, y: u32) -> u32 {
    add(x, y ^ SIGN_BIT)
}

/// `x * y` on float32 bit patterns.
pub fn mul(x: u32, y: u32) -> u32 {
    let mx = x & MANT_MASK;
    let my = y & MANT_MASK;
    let sign = (x ^ y) & SIGN_BIT;
    let ex = x & EXP_MASK;
    let ey = y & EXP_MASK;
    let exp_sum = ey.wrapping_add(ex).wrapping_add(0xC080_0000);
    let underflow = (exp_sum.wrapping_sub(IMPLICIT_ONE) as i32) < 0;
    if mx != 0 && my != 0 {
        // Both mantissas with the implicit one, as 32-bit fixed point with the
        // leading one at bit 31; the 64-bit product has its leading one at bit 62 or 63.
        let a = u64::from((my | 0xFF80_0000) << 8);
        let b = u64::from((mx | 0xFF80_0000) << 8);
        let product = a * b;
        let hi = (product >> 32) as u32;
        let overflow = (product >> 63) as u32;
        let mant = ((hi >> overflow) >> 7) & MANT_MASK;
        let carry = if (hi as i32) < 0 { IMPLICIT_ONE } else { 0 };
        if underflow {
            return 0;
        }
        mant | carry.wrapping_add(exp_sum) | sign
    } else if ex != 0 && ey != 0 {
        if underflow {
            return 0;
        }
        exp_sum | my | mx | sign
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: u32 = 0x3F80_0000;
    const MINUS_ONE: u32 = 0xBF80_0000;
    const TWO: u32 = 0x4000_0000;

    fn f(bits: u32) -> f32 {
        f32::from_bits(bits)
    }

    #[test]
    fn exact_small_cases() {
        assert_eq!(add(ONE, ONE), TWO);
        assert_eq!(sub(TWO, ONE), ONE);
        assert_eq!(mul(TWO, TWO), 0x4080_0000); // 4.0
        assert_eq!(add(ONE, MINUS_ONE), 0);
        assert_eq!(mul(ONE, MINUS_ONE), MINUS_ONE);
        assert_eq!(f(sub(0x4040_0000, 0x3F00_0000)), 2.5); // 3.0 - 0.5
    }

    #[test]
    fn zero_operands_follow_original_rules() {
        assert_eq!(add(0, ONE), ONE);
        assert_eq!(add(ONE, 0), ONE);
        assert_eq!(mul(0, ONE), 0);
        assert_eq!(mul(ONE, 0x8000_0000), 0); // -0 times 1.0 is +0 here
    }

    #[test]
    fn results_are_truncated_not_rounded() {
        // Operands 23 or more exponent steps apart: the smaller one is dropped.
        assert_eq!(add(ONE, 0x3400_0000), ONE); // 2^-23
        assert_eq!(add(MINUS_ONE, 0xB380_0000), MINUS_ONE); // -2^-24
                                                            // Inside the window, bits shifted out are truncated, not rounded.
                                                            // 1.0 + (1 + 3 * 2^-23) = 2 + 1.5 * 2^-22: IEEE rounds to nearest-even
                                                            // (0x40000002), the engine truncates to 0x40000001.
        assert_eq!(add(ONE, 0x3F80_0003), 0x4000_0001);
    }
}
