//! Sequence playback state: which sequence, how far into it, paused or not. Pure logic.

use mdx::Model;

/// A viewer command, from a key or a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    TogglePause,
    Next,
    Previous,
    /// Select the first sequence matching this name (see `Model::find_sequence`).
    Select(&'static str),
}

impl Command {
    /// Key bindings: `p` or space pauses, `]` and `[` step through sequences, and `s`, `w`,
    /// `a`, `d` pick stand, walk, attack and death.
    pub fn from_key(key: &str) -> Option<Command> {
        match key {
            "p" | " " => Some(Command::TogglePause),
            "]" | "n" => Some(Command::Next),
            "[" => Some(Command::Previous),
            "s" => Some(Command::Select("stand")),
            "w" => Some(Command::Select("walk")),
            "a" => Some(Command::Select("attack")),
            "d" => Some(Command::Select("death")),
            _ => None,
        }
    }
}

/// Playback position within the model's sequences.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    sequence: Option<usize>,
    elapsed_ms: f32,
    paused: bool,
}

impl Playback {
    /// Starts on "Stand" if the model has it, else on the first sequence, else at the bind pose.
    pub fn new(model: &Model) -> Playback {
        let sequence = model
            .find_sequence("stand")
            .or(if model.sequences.is_empty() { None } else { Some(0) });
        Playback {
            sequence,
            elapsed_ms: 0.0,
            paused: false,
        }
    }

    pub fn sequence(&self) -> Option<usize> {
        self.sequence
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Milliseconds since the current sequence started.
    pub fn elapsed_ms(&self) -> f32 {
        self.elapsed_ms
    }

    /// Advances time unless paused. Looping and clamping are done by `Sequence::time_at`.
    pub fn advance(&mut self, dt_ms: f32) {
        if !self.paused && dt_ms.is_finite() && dt_ms > 0.0 {
            self.elapsed_ms += dt_ms;
        }
    }

    /// Absolute animation time (ms) of the current position, or `None` at the bind pose.
    pub fn time(&self, model: &Model) -> Option<u32> {
        let seq = model.sequences.get(self.sequence?)?;
        Some(seq.time_at(self.elapsed_ms))
    }

    /// Selects a sequence by index and restarts it. Out-of-range indices are ignored.
    pub fn select(&mut self, model: &Model, index: usize) -> bool {
        if index < model.sequences.len() {
            self.sequence = Some(index);
            self.elapsed_ms = 0.0;
            true
        } else {
            false
        }
    }

    /// Selects a sequence by name. Returns false when nothing matches.
    pub fn select_named(&mut self, model: &Model, query: &str) -> bool {
        match model.find_sequence(query) {
            Some(index) => self.select(model, index),
            None => false,
        }
    }

    /// Moves to the next or previous sequence, wrapping around.
    pub fn step(&mut self, model: &Model, delta: isize) {
        let count = model.sequences.len();
        if count == 0 {
            return;
        }
        let current = self.sequence.unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(count as isize) as usize;
        self.select(model, next);
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// Applies a command. Returns true when the sequence changed.
    pub fn apply(&mut self, model: &Model, command: Command) -> bool {
        match command {
            Command::TogglePause => {
                self.toggle_pause();
                false
            }
            Command::Next => {
                self.step(model, 1);
                true
            }
            Command::Previous => {
                self.step(model, -1);
                true
            }
            Command::Select(name) => self.select_named(model, name),
        }
    }

    /// Name of the current sequence, or "bind pose".
    pub fn name<'a>(&self, model: &'a Model) -> &'a str {
        self.sequence
            .and_then(|i| model.sequences.get(i))
            .map_or("bind pose", |s| s.name.as_str())
    }
}

/// Frame timing. Native uses the real clock; the browser build runs at a fixed 60 Hz step
/// because `std::time::Instant` is not available on wasm32-unknown-unknown.
#[derive(Debug, Default)]
pub struct Clock {
    #[cfg(not(target_arch = "wasm32"))]
    last: Option<std::time::Instant>,
}

impl Clock {
    /// Milliseconds since the previous call. The first call returns 0.
    pub fn tick(&mut self) -> f32 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = std::time::Instant::now();
            let dt = self
                .last
                .map_or(0.0, |last| now.duration_since(last).as_secs_f32() * 1000.0);
            self.last = Some(now);
            // Long pauses (a window dragged, a debugger) must not skip whole sequences.
            dt.min(250.0)
        }
        #[cfg(target_arch = "wasm32")]
        {
            1000.0 / 60.0
        }
    }
}
