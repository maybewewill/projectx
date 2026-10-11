//! Turns (replay or network time slots) to simulation steps, as in Game.dll 1.26a.
//!
//! The model follows `docs/spec/lockstep.md`:
//!
//! * A time slot carries an increment in milliseconds. It is added to a buffer when the slot is
//!   applied (CNetData +0x284, CONFIRMED structure).
//! * The buffer is consumed in 25 ms units (`0x6F553470`, CONFIRMED). Each unit dispatches
//!   `SPEED_TICKS[speed]` game-tick events (CONFIRMED values 3/4/5 at `0x6FA89154`).
//! * One simulation step per 100 ms of consumed time, with the remainder carried in integer
//!   milliseconds (LIKELY, not confirmed; see the contradiction in spec section 2.3).
//! * The commands of a turn are handed back before the steps of that turn (LIKELY).
//!
//! Everything here is integer arithmetic. The wall-clock pacing of the original (the 4000 ms cap,
//! the 200 ms freeze rule) is not modelled: replays run as fast as the caller pushes turns.

/// Milliseconds per consumed unit (`0x19` in `0x6F553470`).
pub const UNIT_MS: u64 = 25;

/// Milliseconds of consumed time per simulation step (the agile clock step, 0.1 s).
pub const STEP_MS: u64 = 100;

/// Game-tick dispatches per 25 ms unit for slow, normal and fast (`0x6FA89154`, int32 table).
pub const SPEED_TICKS: [u64; 3] = [3, 4, 5];

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

    /// Game-tick dispatches per 25 ms unit at this speed.
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
    /// Game-tick dispatches this turn produced (`units * ticks_per_unit`).
    pub game_ticks: u64,
    /// Milliseconds of game time consumed by this turn (a multiple of 25).
    pub consumed_ms: u64,
}

/// Converts time slots into simulation steps. Deterministic and integer-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnScheduler {
    speed: GameSpeed,
    /// Milliseconds received but not yet consumed (below 25 after every push).
    buffer_ms: u64,
    /// Consumed milliseconds not yet forming a full step (below 100 after every push).
    step_carry_ms: u64,
    total_steps: u64,
    total_ticks: u64,
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
            buffer_ms: 0,
            step_carry_ms: 0,
            total_steps: 0,
            total_ticks: 0,
        }
    }

    /// Changes the speed. Takes effect from the next consumed unit.
    pub fn set_speed(&mut self, speed: GameSpeed) {
        self.speed = speed;
    }

    pub fn speed(&self) -> GameSpeed {
        self.speed
    }

    /// Milliseconds received but not yet consumed.
    pub fn buffered_ms(&self) -> u64 {
        self.buffer_ms
    }

    /// Total simulation steps produced since creation.
    pub fn total_steps(&self) -> u64 {
        self.total_steps
    }

    /// Total game-tick dispatches produced since creation.
    pub fn total_ticks(&self) -> u64 {
        self.total_ticks
    }

    /// Applies one turn: adds its time increment to the buffer, consumes whole 25 ms units, and
    /// returns the commands together with the steps and ticks the consumed time produces.
    pub fn push_turn<A>(&mut self, elapsed_ms: u64, actions: Vec<(u8, A)>) -> TurnReport<A> {
        self.buffer_ms = self.buffer_ms.saturating_add(elapsed_ms);
        let units = self.buffer_ms / UNIT_MS;
        self.buffer_ms -= units * UNIT_MS;

        let consumed_ms = units * UNIT_MS;
        let game_ticks = units * self.speed.ticks_per_unit();

        // u128 so that a saturated buffer cannot overflow the carry sum.
        let carry = u128::from(self.step_carry_ms) + u128::from(consumed_ms);
        let sim_steps = (carry / u128::from(STEP_MS)) as u64;
        self.step_carry_ms = (carry % u128::from(STEP_MS)) as u64;

        self.total_steps += sim_steps;
        self.total_ticks += game_ticks;

        TurnReport {
            actions,
            sim_steps,
            game_ticks,
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
    fn one_hundred_ms_is_one_step_and_four_units_at_normal_speed() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        let r = s.push_turn(100, no_actions());
        assert_eq!(r.consumed_ms, 100);
        assert_eq!(r.sim_steps, 1);
        assert_eq!(r.game_ticks, 16); // 4 units * 4 ticks
        assert_eq!(s.buffered_ms(), 0);
    }

    #[test]
    fn remainder_below_a_unit_is_buffered_and_below_a_step_is_carried() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        let r = s.push_turn(30, no_actions());
        assert_eq!((r.consumed_ms, r.sim_steps, r.game_ticks), (25, 0, 4));
        assert_eq!(s.buffered_ms(), 5);

        let r = s.push_turn(70, no_actions()); // buffer 75: three units
        assert_eq!((r.consumed_ms, r.sim_steps), (75, 1));
        assert_eq!(s.buffered_ms(), 0);
    }

    #[test]
    fn carry_accumulates_across_turns_without_drift() {
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        // 130 ms: five units (125 consumed), one step, carry 25.
        let r = s.push_turn(130, no_actions());
        assert_eq!((r.consumed_ms, r.sim_steps, s.buffered_ms()), (125, 1, 5));
        // 70 ms: buffer 75, three units, carry 25 + 75 = 100, one more step.
        let r = s.push_turn(70, no_actions());
        assert_eq!((r.consumed_ms, r.sim_steps), (75, 1));
        assert_eq!(s.total_steps(), 2);
    }

    #[test]
    fn zero_increment_produces_nothing() {
        let mut s = TurnScheduler::new(GameSpeed::Fast);
        let r = s.push_turn(0, vec![(1u8, 7u32)]);
        assert_eq!((r.sim_steps, r.game_ticks, r.consumed_ms), (0, 0, 0));
        assert_eq!(r.actions, vec![(1, 7)]);
    }

    #[test]
    fn long_run_steps_equal_consumed_ms_div_100() {
        // A deterministic pseudo-random sequence of increments (LCG, no system state).
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut s = TurnScheduler::new(GameSpeed::Normal);
        let mut received = 0u64;
        let mut consumed_total = 0u64;
        let mut steps_total = 0u64;
        for _ in 0..10_000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let inc = (seed >> 33) % 400; // 0..399 ms
            let r = s.push_turn(inc, no_actions());
            received += inc;
            consumed_total += r.consumed_ms;
            steps_total += r.sim_steps;
        }
        // Nothing is created or lost: received = consumed + buffered.
        assert_eq!(received, consumed_total + s.buffered_ms());
        // Steps are exactly the consumed time in 100 ms units, minus the carry.
        assert_eq!(steps_total, consumed_total / STEP_MS);
        assert_eq!(s.total_steps(), steps_total);
    }

    #[test]
    fn speed_tick_counts_follow_the_table() {
        assert_eq!(GameSpeed::Slow.ticks_per_unit(), 3);
        assert_eq!(GameSpeed::Normal.ticks_per_unit(), 4);
        assert_eq!(GameSpeed::Fast.ticks_per_unit(), 5);
        // 100 ms at each speed: 4 units times the table value.
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
        let mut s = TurnScheduler::new(GameSpeed::Normal);
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
}
