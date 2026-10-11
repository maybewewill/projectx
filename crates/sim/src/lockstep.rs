//! Turns (replay or network time slots) to game ticks and simulation steps, as in Game.dll 1.26a.
//!
//! The model follows `docs/spec/lockstep.md`, section 7 (pass 3):
//!
//! * A time slot carries an increment in milliseconds. It is added to a buffer (CNetData +0x284 /
//!   +0x2250 in the driver). CONFIRMED.
//! * The buffer is consumed in 25 ms units by the frame driver `0x6F553470`. CONFIRMED.
//! * Each unit dispatches `SPEED_TICKS[speed] * multiplier` game ticks, where the speed table is
//!   `{3, 4, 5}` at `0x6FA89154` and the multiplier `m` comes from `0x6F5BE0B0/0xC0`. CONFIRMED
//!   structure; the runtime value of `m` is UNSURE, default 1.
//! * Each tick adds 5 to the counter `+0x1C70` (CONFIRMED). We read that as 5 ms of game time per
//!   tick (LIKELY). The same tick also hands the agile clock the f32 0.005 s increment
//!   ([`TICK_INCREMENT_BITS`], CONFIRMED, section 9).
//! * The speed factor stored by the game is `SPEED / 5` (`DAT_6F957640` = 5.0 in the image,
//!   CONFIRMED, section 8), i.e. game ms per turn ms at 5 ms per tick (LIKELY).
//! * "Steps" below are an accounting value, `game ms / 100`, not an engine event. The original
//!   dispatches no 100 ms event (section 8.5, LIKELY); a 0.1 s period is an ordinary timer on the
//!   agile clock. Steps per 100 ms of turn time are 0.6 / 0.8 / 1.0 for slow / normal / fast at
//!   `m = 1`. This replaces the earlier "one step per 100 ms of turn time" model (section 7.5).
//!
//! Everything here is integer arithmetic, except the agile clock step in
//! [`advance_agile_clock`], which uses the soft-float add. The wall-clock pacing of the original (the 4000 ms cap,
//! the 200 ms freeze rule, the latency factor) is not modelled: replays run as fast as the caller
//! pushes turns.

use crate::timer::AgileClock;

/// Milliseconds per consumed unit (`0x19` in `0x6F553470`).
pub const UNIT_MS: u64 = 25;

/// Milliseconds of game time per simulation step (the agile 0.1 s step, LIKELY).
pub const STEP_MS: u64 = 100;

/// Game-tick dispatches per 25 ms unit for slow, normal and fast (`0x6FA89154`, int32 table).
pub const SPEED_TICKS: [u64; 3] = [3, 4, 5];

/// Increment of the per-tick counter `+0x1C70` (CONFIRMED). Read as 5 ms of game time per tick (LIKELY).
pub const TICK_GAME_MS: u64 = 5;

/// Game ticks per batch of the per-step work of the host: the combat tick of units with orders, the
/// JASS scheduler pass and the AI pass. UNSURE for all three (lockstep.md sections 10.2 and 11.5-11.7):
/// the original runs threads from their events and timers, not from a batch (11.6), and the AI
/// dispatcher's rhythm is not the 100 ms of the old model. Unit movement is NOT batched: it runs from
/// its own timer, [`MOVE_PERIOD_BITS`] (lockstep.md section 11.2).
pub const TICKS_PER_BATCH: u64 = 20;

/// Period of the unit movement walk (message 0x8026A), f32 0.03 (`0x3CF5C290`). CONFIRMED value:
/// the static initializer `0x6F85F650` converts the text `"0.03"` at `0x6F942DD8` with S2R into the
/// f32 at `0x6FAB7370`, which is the timeout of the repeating agent timer that posts 0x8026A
/// (`0x6F426BF0`, `CAgentTimer::vf2`), and Unicorn gives `0x3CF5C290` (lockstep.md section 11.2).
/// The period is the same as the timeout (LIKELY). 0.03 s is six game ticks of 0.005 s.
pub const MOVE_PERIOD_BITS: u32 = 0x3CF5_C290;

/// Agile-clock increment per game tick, f32 0.005 (`0x3BA3D70A`). CONFIRMED: the tick event built by
/// the frame driver carries `[0x6FACC8CC]` at +0x14, the static initializer `0x6F8621C0` stores
/// `I2R(5) / I2R(1000)` there (run in Unicorn), and the listener `0x6F2F4950` adds that float to
/// the agile clock (lockstep.md section 9). The clock advances by one soft-float add per tick.
pub const TICK_INCREMENT_BITS: u32 = 0x3BA3_D70A;

/// Advances the agile clock by `ticks` game ticks: one soft-float add of [`TICK_INCREMENT_BITS`]
/// per tick. Each add rounds, so the work is linear in `ticks` and cannot be shortcut.
///
/// The host ([`crate::ScriptHost::step_ticks`]) calls this one tick at a time and runs the timer
/// pump after each add, as the original does (`0x6F46E520`, lockstep.md section 10).
pub fn advance_agile_clock(clock: &mut AgileClock, ticks: u64) {
    for _ in 0..ticks {
        clock.advance(TICK_INCREMENT_BITS);
    }
}

/// Game speed as set by the game (`0x6F53E890`): 0 = slow, 1 = normal, anything else = fast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameSpeed {
    Slow,
    #[default]
    Normal,
    Fast,
}

impl GameSpeed {
    /// The setting as the native passes it. Values of 2 and up are clamped to fast, as in the
    /// original (`if (1 < param) param = 2`).
    pub fn from_setting(setting: u32) -> Self {
        match setting {
            0 => GameSpeed::Slow,
            1 => GameSpeed::Normal,
            _ => GameSpeed::Fast,
        }
    }

    /// Game-tick dispatches per 25 ms unit at this speed (before the multiplier).
    pub fn ticks_per_unit(self) -> u64 {
        SPEED_TICKS[match self {
            GameSpeed::Slow => 0,
            GameSpeed::Normal => 1,
            GameSpeed::Fast => 2,
        }]
    }
}

/// What one pushed turn produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnReport<A> {
    /// The turn's commands, in the order the stream gave them. Apply them before `sim_steps`.
    pub actions: Vec<(u8, A)>,
    /// Simulation steps this turn produced (possibly zero).
    pub sim_steps: u64,
    /// Game-tick dispatches this turn produced (`units * ticks_per_unit * multiplier`).
    pub game_ticks: u64,
    /// Milliseconds of game time this turn produced (`game_ticks * TICK_GAME_MS`).
    pub game_ms: u64,
    /// Milliseconds of turn time consumed by this turn (a multiple of 25).
    pub consumed_ms: u64,
}

/// Converts time slots into game ticks and simulation steps. Deterministic and integer-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnScheduler {
    speed: GameSpeed,
    /// The multiplier `m` of the driver (`0x6F5BE0C0`), 1 in normal play (UNSURE in general).
    multiplier: u32,
    /// Milliseconds received but not yet consumed (below 25 after every push).
    buffer_ms: u64,
    /// Game milliseconds not yet forming a full step (below 100 after every push).
    step_carry_ms: u64,
    total_steps: u64,
    total_ticks: u64,
    total_game_ms: u64,
}

impl Default for TurnScheduler {
    fn default() -> Self {
        Self::new(GameSpeed::Normal)
    }
}

impl TurnScheduler {
    pub fn new(speed: GameSpeed) -> Self {
        Self {
            speed,
            multiplier: 1,
            buffer_ms: 0,
            step_carry_ms: 0,
            total_steps: 0,
            total_ticks: 0,
            total_game_ms: 0,
        }
    }

    /// Changes the speed. Takes effect from the next consumed unit.
    pub fn set_speed(&mut self, speed: GameSpeed) {
        self.speed = speed;
    }

    pub fn speed(&self) -> GameSpeed {
        self.speed
    }

    /// Sets the driver multiplier `m`. Takes effect from the next consumed unit. A value of 0
    /// stops tick production (the model, not checked against the original).
    pub fn set_multiplier(&mut self, m: u32) {
        self.multiplier = m;
    }

    pub fn multiplier(&self) -> u32 {
        self.multiplier
    }

    /// Milliseconds received but not yet consumed.
    pub fn buffered_ms(&self) -> u64 {
        self.buffer_ms
    }

    /// Total simulation steps produced since creation.
    pub fn total_steps(&self) -> u64 {
        self.total_steps
    }

    /// Total game-tick dispatches produced since creation (saturating).
    pub fn total_ticks(&self) -> u64 {
        self.total_ticks
    }

    /// Total game milliseconds produced since creation (saturating).
    pub fn total_game_ms(&self) -> u64 {
        self.total_game_ms
    }

    /// Applies one turn: adds its time increment to the buffer, consumes whole 25 ms units, and
    /// returns the commands together with the ticks, game time and steps the consumed time produces.
    pub fn push_turn<A>(&mut self, elapsed_ms: u64, actions: Vec<(u8, A)>) -> TurnReport<A> {
        self.buffer_ms = self.buffer_ms.saturating_add(elapsed_ms);
        let units = self.buffer_ms / UNIT_MS;
        self.buffer_ms -= units * UNIT_MS;
        let consumed_ms = units * UNIT_MS;

        // u128 so that a saturated buffer, a large multiplier and the carry cannot overflow.
        let ticks = u128::from(units)
            * u128::from(self.speed.ticks_per_unit())
            * u128::from(self.multiplier);
        let game_ms = ticks * u128::from(TICK_GAME_MS);
        let carry = u128::from(self.step_carry_ms) + game_ms;
        let sim_steps = (carry / u128::from(STEP_MS)) as u64;
        self.step_carry_ms = (carry % u128::from(STEP_MS)) as u64;

        let game_ticks = u64::try_from(ticks).unwrap_or(u64::MAX);
        let game_ms = u64::try_from(game_ms).unwrap_or(u64::MAX);
        self.total_steps = self.total_steps.saturating_add(sim_steps);
        self.total_ticks = self.total_ticks.saturating_add(game_ticks);
        self.total_game_ms = self.total_game_ms.saturating_add(game_ms);

        TurnReport {
            actions,
            sim_steps,
            game_ticks,
            game_ms,
            consumed_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_actions() -> Vec<(u8, u32)> {
        Vec::new()
    }

    #[test]
    fn hundred_ms_at_normal_speed_is_16_ticks_and_80_game_ms() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        let r = s.push_turn(100, no_actions());
        assert_eq!(r.consumed_ms, 100);
        assert_eq!(r.game_ticks, 16); // 4 units * 4 ticks
        assert_eq!(r.game_ms, 80); // 16 * 5 ms
        assert_eq!(r.sim_steps, 0); // 80 ms carried, no full step yet
        assert_eq!(s.buffered_ms(), 0);
        // The next 100 ms completes the first step, carrying 60 ms.
        let r = s.push_turn(100, no_actions());
        assert_eq!(r.sim_steps, 1);
        assert_eq!(s.total_steps(), 1);
    }

    #[test]
    fn steps_per_hundred_turn_ms_are_0_6_0_8_1_0() {
        // Over 1000 ms of turn time: 6, 8 and 10 steps (LIKELY, section 7.5).
        for (speed, want) in [
            (GameSpeed::Slow, 6),
            (GameSpeed::Normal, 8),
            (GameSpeed::Fast, 10),
        ] {
            let mut s = TurnScheduler::new(speed);
            for _ in 0..10 {
                s.push_turn(100, no_actions());
            }
            assert_eq!(s.total_steps(), want, "speed {speed:?}");
            // 600 / 800 / 1000 ms of game time: no remainder.
            assert_eq!(s.total_game_ms(), want * 100);
        }
    }

    #[test]
    fn remainder_below_a_unit_is_buffered_and_game_time_is_carried() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        let r = s.push_turn(30, no_actions());
        assert_eq!((r.consumed_ms, r.game_ticks, r.game_ms), (25, 4, 20));
        assert_eq!(s.buffered_ms(), 5);
        assert_eq!(r.sim_steps, 0);
    }

    #[test]
    fn carry_accumulates_across_turns_without_drift() {
        let mut s = TurnScheduler::new(GameSpeed::Fast);
        // Fast: 25 ms game time per unit. 130 ms = 5 units = 125 ms game time = 1 step, carry 25.
        let r = s.push_turn(130, no_actions());
        assert_eq!((r.consumed_ms, r.game_ms, r.sim_steps), (125, 125, 1));
        // 70 ms: buffer 5 + 70 = 75, three units = 75 ms game time, carry 25 + 75 = 100: one step.
        let r = s.push_turn(70, no_actions());
        assert_eq!((r.consumed_ms, r.sim_steps), (75, 1));
        assert_eq!(s.total_steps(), 2);
    }

    #[test]
    fn zero_increment_produces_nothing() {
        let mut s = TurnScheduler::new(GameSpeed::Fast);
        let r = s.push_turn(0, vec![(1u8, 7u32)]);
        assert_eq!((r.sim_steps, r.game_ticks, r.game_ms, r.consumed_ms), (0, 0, 0, 0));
        assert_eq!(r.actions, vec![(1, 7)]);
    }

    #[test]
    fn long_run_steps_equal_game_ms_div_100() {
        // A deterministic pseudo-random sequence of increments (LCG, no system state).
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        let mut received = 0u64;
        let mut consumed_total = 0u64;
        let mut game_total = 0u64;
        let mut steps_total = 0u64;
        for _ in 0..10_000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let inc = (seed >> 33) % 400; // 0..399 ms
            let r = s.push_turn(inc, no_actions());
            received += inc;
            consumed_total += r.consumed_ms;
            game_total += r.game_ms;
            steps_total += r.sim_steps;
        }
        // Nothing is created or lost: received = consumed + buffered.
        assert_eq!(received, consumed_total + s.buffered_ms());
        // Game time is exactly 5 ms per tick, and steps are the game time in 100 ms units.
        assert_eq!(game_total, s.total_ticks() * TICK_GAME_MS);
        assert_eq!(steps_total, game_total / STEP_MS);
        assert_eq!(s.total_steps(), steps_total);
    }

    #[test]
    fn speed_tick_counts_follow_the_table() {
        assert_eq!(GameSpeed::Slow.ticks_per_unit(), 3);
        assert_eq!(GameSpeed::Normal.ticks_per_unit(), 4);
        assert_eq!(GameSpeed::Fast.ticks_per_unit(), 5);
        for (speed, ticks) in [
            (GameSpeed::Slow, 12),
            (GameSpeed::Normal, 16),
            (GameSpeed::Fast, 20),
        ] {
            let mut s = TurnScheduler::new(speed);
            assert_eq!(s.push_turn(100, no_actions()).game_ticks, ticks);
        }
    }

    #[test]
    fn multiplier_scales_ticks_and_game_time() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        s.set_multiplier(2);
        let r = s.push_turn(100, no_actions());
        assert_eq!((r.game_ticks, r.game_ms, r.sim_steps), (32, 160, 1));
        s.set_multiplier(0);
        let r = s.push_turn(100, no_actions());
        assert_eq!((r.game_ticks, r.game_ms), (0, 0));
    }

    #[test]
    fn speed_setting_is_clamped_like_the_native() {
        assert_eq!(GameSpeed::from_setting(0), GameSpeed::Slow);
        assert_eq!(GameSpeed::from_setting(1), GameSpeed::Normal);
        assert_eq!(GameSpeed::from_setting(2), GameSpeed::Fast);
        assert_eq!(GameSpeed::from_setting(9), GameSpeed::Fast);
    }

    #[test]
    fn commands_come_back_in_stream_order_and_are_not_reordered() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        let actions = vec![(3u8, "c"), (0u8, "a"), (3u8, "d")];
        let r = s.push_turn(100, actions.clone());
        assert_eq!(r.actions, actions);
    }

    #[test]
    fn speed_change_applies_to_the_next_unit() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        s.push_turn(25, no_actions());
        s.set_speed(GameSpeed::Fast);
        let r = s.push_turn(25, no_actions());
        assert_eq!(r.game_ticks, 5);
        assert_eq!(s.total_ticks(), 4 + 5);
    }

    #[test]
    fn huge_increment_does_not_overflow() {
        let mut s = TurnScheduler::new(GameSpeed::Fast);
        s.set_multiplier(u32::MAX);
        let r = s.push_turn(u64::MAX, no_actions());
        assert_eq!(r.consumed_ms % UNIT_MS, 0);
        assert!(s.buffered_ms() < UNIT_MS);
        assert_eq!(r.consumed_ms + s.buffered_ms(), u64::MAX);
    }

    /// Reads the speed table from the user's Game.dll when `GAME_DLL` points at it.
    #[test]
    fn speed_table_matches_the_image() {
        let Ok(path) = std::env::var("GAME_DLL") else {
            return;
        };
        let file = std::fs::read(path).expect("GAME_DLL is readable");
        let bytes = crate::pe::read_va(&file, 0x6FA8_9154, 12).expect("table is in the image");
        let got: Vec<i32> = bytes
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let want: Vec<i32> = SPEED_TICKS.iter().map(|&v| v as i32).collect();
        assert_eq!(got, want);
    }

    /// The divisor `DAT_6F957640` of the speed factor (`0x6F53E890` stores `SPEED / DAT` at
    /// +0x22AC) is the double 5.0 in the image (CONFIRMED, lockstep.md section 8). With 5 ms per
    /// tick, that factor is game milliseconds per turn millisecond: `ticks * 5 / 25`.
    #[test]
    fn speed_factor_divisor_is_five_in_the_image() {
        let Ok(path) = std::env::var("GAME_DLL") else {
            return;
        };
        let file = std::fs::read(path).expect("GAME_DLL is readable");
        let bytes = crate::pe::read_va(&file, 0x6F95_7640, 8).expect("divisor is in the image");
        let mut raw = [0u8; 8];
        raw.copy_from_slice(bytes);
        assert_eq!(f64::from_le_bytes(raw), 5.0);
    }

    /// The tick increment is the f32 0.005, i.e. 5 ms of agile (seconds) time, the same 5 as the
    /// `+0x1C70` counter. Its bits are the value the static initializer computes (section 9).
    #[test]
    fn tick_increment_is_five_milliseconds_as_f32() {
        let v = f32::from_bits(TICK_INCREMENT_BITS);
        assert_eq!(v, 0.005f32);
        assert!((f64::from(v) * 1000.0 - 5.0).abs() < 1e-6);
    }

    /// The initializer `0x6F8621C0` stores `I2R(5) / I2R(1000)` (div with the 5 as operand A).
    /// Checked in Unicorn (section 9.2); this test repeats the division with the soft-float
    /// reciprocal table read from the user's Game.dll.
    #[test]
    fn tick_increment_is_five_over_thousand_in_soft_float() {
        let Ok(path) = std::env::var("GAME_DLL") else {
            return;
        };
        let file = std::fs::read(path).expect("GAME_DLL is readable");
        let tables = softfloat::Tables::from_game_dll(&file).expect("tables are in the image");
        let q = softfloat::arith::div(
            softfloat::conv::i2r(5),
            softfloat::conv::i2r(1000),
            &tables,
        );
        assert_eq!(q.to_bits(), TICK_INCREMENT_BITS);
    }

    #[test]
    fn agile_clock_advances_by_one_soft_float_add_per_tick() {
        let mut c = AgileClock::new();
        advance_agile_clock(&mut c, 1);
        // 0 + x returns x exactly.
        assert_eq!(c.bits(), TICK_INCREMENT_BITS);
        advance_agile_clock(&mut c, 1);
        // 0.005 + 0.005 doubles the exponent with the same mantissa: 0x3C23D70A.
        assert_eq!(c.bits(), 0x3C23_D70A);
        // 200 ticks is one second of agile time, up to the drift of truncating adds.
        advance_agile_clock(&mut c, 198);
        assert!((f64::from(c.now()) - 1.0).abs() < 1e-3, "{}", c.now());
        // Same result as folding the soft-float add by hand over 200 ticks.
        let mut bits = 0u32;
        for _ in 0..200 {
            bits = softfloat::arith::add(
                softfloat::F32::from_bits(bits),
                softfloat::F32::from_bits(TICK_INCREMENT_BITS),
            )
            .to_bits();
        }
        let mut direct = AgileClock::new();
        advance_agile_clock(&mut direct, 200);
        assert_eq!(direct.bits(), bits);
    }

    /// The speed factor is game milliseconds per turn millisecond: at 5 ms per tick and 25 ms per
    /// unit it is `ticks / 5`, i.e. 0.6 / 0.8 / 1.0 (no speed-dependent per-tick increment needed).
    #[test]
    fn speed_factor_is_game_ms_per_turn_ms() {
        for speed in [GameSpeed::Slow, GameSpeed::Normal, GameSpeed::Fast] {
            let ticks = speed.ticks_per_unit();
            let game_ms_per_unit = ticks * TICK_GAME_MS;
            // factor = game_ms_per_unit / UNIT_MS, compared exactly as a ratio of integers.
            assert_eq!(game_ms_per_unit * 5, ticks * 25);
        }
        assert_eq!(GameSpeed::Normal.ticks_per_unit() * TICK_GAME_MS, 20);
    }
}
