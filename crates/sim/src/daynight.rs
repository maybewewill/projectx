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

/// The message of the time-of-day listener (`0x80262`, `0x6F429770`): the cycle is complete.
pub const WRAP_MESSAGE: u32 = 0x80262;

/// The time of day on the agile engine (`0x6F429770`, docs/spec/agile.md section 6): float game state 2 is a property
/// with a forever modifier of rate `scale * DayHours / DayLength` and a rising listener at `DayHours`; the listener's
/// message resets the value to 0.0 (`0x6F42FC9F`).
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeOfDay {
    pub(crate) prop: Option<crate::agile::PropId>,
    pub(crate) modifier: Option<crate::agile::ModId>,
    pub(crate) listener: Option<crate::agile::LisId>,
    /// How many times the cycle wrapped.
    pub(crate) wraps: u32,
}

impl crate::natives::Game {
    /// Starts the time of day on the agile engine (what the game does when the time-of-day suspension ends at the start
    /// of a map): needs the softfloat tables. The current game state 2 is the property's starting value. `scale` is the
    /// time-of-day scale (1.0 by default).
    pub fn start_time_of_day(&mut self, scale: F32) {
        let Some(tables) = self.orders.tables.as_ref() else { return };
        if self.tod.prop.is_some() {
            return;
        }
        let rate = self.day_cycle.rate(scale, tables);
        let start = self.game_state.float_state;
        let p = self.agile.new_property(start, F32(0xFF7F_FFFF), F32(crate::agile::FOREVER_BITS));
        let m = self.agile.add_modifier(p, rate, F32(0), tables);
        let l = self.agile.add_listener(p, 1, self.day_cycle.day_hours, WRAP_MESSAGE, tables);
        self.tod = TimeOfDay { prop: Some(p), modifier: Some(m), listener: Some(l), wraps: 0 };
    }

    /// Float game state 2 (`GetFloatGameState(GAME_STATE_TIME_OF_DAY)`): the agile property's value once the time of day
    /// runs, the stored value before.
    pub fn time_of_day(&self) -> F32 {
        match self.tod.prop {
            Some(p) => self.agile.value(p),
            None => self.game_state.float_state,
        }
    }

    /// `SetFloatGameState(GAME_STATE_TIME_OF_DAY, x)` (`0x6F429940`: the property is moved to `x`).
    pub fn set_time_of_day(&mut self, x: F32) {
        match (self.tod.prop, self.orders.tables.as_ref()) {
            (Some(p), Some(t)) => self.agile.set_value(p, x, t),
            _ => self.game_state.float_state = x,
        }
    }

    /// `SuspendTimeOfDay(flag)`: suspending ends the modifier (the time stops); resuming creates it again
    /// (`0x6F427E70`, `0x6F429920`; LIKELY the native maps to these).
    pub fn suspend_time_of_day(&mut self, suspend: bool) {
        let Some(t) = self.orders.tables.as_ref() else { return };
        let Some(p) = self.tod.prop else { return };
        if suspend {
            if let Some(m) = self.tod.modifier.take() {
                self.agile.remove_modifier(m, t);
            }
        } else if self.tod.modifier.is_none() {
            let rate = self.day_cycle.rate(F32(0x3F80_0000), t);
            self.tod.modifier = Some(self.agile.add_modifier(p, rate, F32(0), t));
        }
    }

    /// The listener firing of the time of day: the cycle wraps to 0.0.
    pub(crate) fn time_of_day_wrap(&mut self) {
        if let (Some(p), Some(t)) = (self.tod.prop, self.orders.tables.as_ref()) {
            self.agile.set_value(p, F32(0), t);
            self.tod.wraps += 1;
        }
    }

    /// How many times the time of day wrapped since it started.
    pub fn time_of_day_wraps(&self) -> u32 {
        self.tod.wraps
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
