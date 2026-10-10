//! Bit-exact model of the software floating-point helpers of Warcraft III 1.26a
//! (`Game.dll`).
//!
//! The original engine does not use the FPU for `real` arithmetic. It calls its
//! own helpers, which work on IEEE binary32 bit patterns stored in memory, with
//! a 24-bit mantissa, truncating (not rounding) results, no denormal support,
//! and wrap-around instead of overflow/NaN handling. See `docs/spec/softfloat.md`
//! for the description of every helper, its address and its edge cases.
//!
//! Layout of the module:
//! - [`F32`]: the bit-pattern newtype used everywhere.
//! - [`arith`]: add, sub, mul, div, compare.
//! - [`conv`]: int -> real (`I2R`), real -> int truncating (`F2I`, the internal helper)
//!   and real -> int saturating (`R2I`, the JASS native).
//! - [`tables`]: lookup tables, read at runtime from the user's own `Game.dll`.
//!   Nothing from the DLL is embedded in this crate.
//! - [`math`]: square root and the trigonometric / inverse / power helpers.
//! - [`text`]: real to text (R2S / R2SW) and text to real (S2R), with the same helpers.
//!
//! The crate is `no_std` (it uses `alloc` only for the table storage).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod arith;
pub mod conv;
pub mod math;
pub mod tables;
pub mod text;

pub use tables::{Tables, TablesError};

/// IEEE binary32 bit pattern, exactly as the original stores a `real`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct F32(pub u32);

impl F32 {
    /// Positive zero.
    pub const ZERO: F32 = F32(0);
    /// `1.0`.
    pub const ONE: F32 = F32(0x3f80_0000);

    /// Wraps a raw bit pattern.
    #[inline]
    pub const fn from_bits(bits: u32) -> F32 {
        F32(bits)
    }

    /// Returns the raw bit pattern.
    #[inline]
    pub const fn to_bits(self) -> u32 {
        self.0
    }

    /// Converts to a native `f32` with the same bits (for display and tests only).
    #[inline]
    pub fn to_f32(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// Counts the leading zero bits of a 32-bit value (32 for zero).
///
/// Original: `0x6F6EEC00`.
#[inline]
pub(crate) const fn clz(x: u32) -> u32 {
    x.leading_zeros()
}
