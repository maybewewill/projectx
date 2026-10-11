//! Day and night of Game.dll 1.26a (`docs/spec/vision.md`, section 13).
//!
//! Time of day is float game state 2. The original builds a modifier on it when the time-of-day timers are set up
//! (`0x6f429770`) with the rate `scale * DayHours / DayLength` (CONFIRMED: Unicorn, `tools/emu/time_of_day_emu.py`,
//! 4 vectors, 0 mismatches; the operands are softfloat MUL(scale, DayHours) then DIV by DayLength, and the DIV is the
//! reciprocal product of softfloat). A listener at `DayHours` resets the value to 0.0 (`0x6f42fc9f`, LIKELY the
//! wrap of one cycle, see section 13).
//!
//! The day rule of the switch `0x6f42c210` is CONFIRMED by Unicorn (13 vectors, 0 mismatches): day iff
//! `Dawn <= t < Dusk`, with f32 comparisons (a NaN is night).
//!
//! NOT modelled here: the per-tick evolution of the modifier (the agile evaluation of `0x6f4a6b70` and `0x6f4a9390`
//! is not run, UNSURE), so this module gives the rate and the day rule but does not advance the clock.

use softfloat::{arith, Tables, F32};

/// A real from a native `f32` value (the bit pattern is kept, no conversion).
fn real(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// The inputs of the day cycle: the `[Misc]` keys `DayLength`, `DayHours`, `Dawn` and `Dusk`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayCycle {
    /// `DayLength` (seconds of one cycle).
    pub day_length: F32,
    /// `DayHours` (hours covered by one cycle).
    pub day_hours: F32,
    /// `Dawn` (inclusive start of day, in hours).
    pub dawn: F32,
    /// `Dusk` (exclusive start of night, in hours).
    pub dusk: F32,
}

impl Default for DayCycle {
    /// The cycle used when no `[Misc]` data is loaded: 240 seconds for 24 hours, day from 6 to 18. These are the usual
    /// values, not read from the original (UNSURE), and they keep the previous model's day rule.
    fn default() -> Self {
        DayCycle {
            day_length: real(240.0),
            day_hours: real(24.0),
            dawn: real(6.0),
            dusk: real(18.0),
        }
    }
}

impl DayCycle {
    /// The rate of time of day (game state 2) per second for a time scale `scale`:
    /// `DIV(MUL(scale, DayHours), DayLength)` in softfloat, the operand order of `0x6f429770`.
    pub fn rate(&self, scale: F32, tables: &Tables) -> F32 {
        arith::div(arith::mul(scale, self.day_hours), self.day_length, tables)
    }

    /// Whether the time `t` (game state 2) is day: `Dawn <= t < Dusk` (`0x6f42c210`, CONFIRMED).
    pub fn is_day(&self, t: F32) -> bool {
        arith::le(self.dawn, t) && arith::lt(t, self.dusk)
    }

    /// Whether the time `t` is night (the negation of [`DayCycle::is_day`]).
    pub fn is_night(&self, t: F32) -> bool {
        !self.is_day(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(x: f32) -> F32 {
        real(x)
    }

    #[test]
    fn day_is_dawn_inclusive_dusk_exclusive() {
        let c = DayCycle::default();
        assert!(!c.is_day(f(0.0)));
        assert!(!c.is_day(f(5.99)));
        assert!(c.is_day(f(6.0)));
        assert!(c.is_day(f(17.99)));
        assert!(!c.is_day(f(18.0)));
    }

    #[test]
    fn nan_is_night() {
        let c = DayCycle::default();
        assert!(c.is_night(f(f32::NAN)));
    }

    #[test]
    fn custom_dawn_and_dusk_replace_six_and_eighteen() {
        let c = DayCycle {
            dawn: f(4.5),
            dusk: f(20.0),
            ..DayCycle::default()
        };
        assert!(c.is_night(f(4.49)));
        assert!(c.is_day(f(4.5)));
        assert!(c.is_day(f(19.99)));
        assert!(c.is_night(f(20.0)));
    }

    #[test]
    fn dawn_after_dusk_is_never_day_for_a_half_open_range() {
        let c = DayCycle {
            dawn: f(20.0),
            dusk: f(4.0),
            ..DayCycle::default()
        };
        assert!(c.is_night(f(5.0)));
        assert!(c.is_night(f(21.0)));
    }
}
