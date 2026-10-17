//! Gameplay random generator of the 1.26a engine (see `docs/spec/rng.md`).
//!
//! The generator is an 8-byte state `(a, b)`. One step reads four bytes of `b`,
//! uses them as offsets into a 256-byte permutation table, and mixes four
//! table words into `a`. The table is part of the original binary, so it is
//! never stored in this repository: [`RngTable::from_game_dll`] reads it from
//! the user's own `Game.dll`.

use crate::pe;
use softfloat::{arith, F32};

/// Number of independent streams (combat, creep drops, ...).
pub const STREAM_COUNT: usize = 45;

/// Game-setup seed used when the "fixed seed" flag (0x8000) is set.
pub const FIXED_GAME_SEED: u32 = 0x7761_7233;

/// Seed the engine gives the global generator at start-up. Game setup reseeds it.
pub const ENGINE_PLACEHOLDER_SEED: u32 = 0x6970_7365;

/// Virtual address of the 256-byte permutation table in `Game.dll` 1.26a.
const TABLE_VA: u32 = 0x6F95_3650;

/// Errors when loading the permutation table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RngError {
    /// The file is not a PE32 image, or the table address is not inside its file data.
    Image(&'static str),
    /// The bytes at the table address are not a permutation of `0..=255`.
    NotPermutation,
}

impl std::fmt::Display for RngError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RngError::Image(why) => write!(f, "cannot read RNG table from Game.dll: {why}"),
            RngError::NotPermutation => write!(f, "RNG table in Game.dll is not a permutation"),
        }
    }
}

impl std::error::Error for RngError {}

/// The 256-byte permutation table used by [`Rng::next_u32`].
#[derive(Clone, PartialEq, Eq)]
pub struct RngTable([u8; 256]);

impl std::fmt::Debug for RngTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The table is data from the user's Game.dll; print its size only.
        f.write_str("RngTable(256 bytes)")
    }
}

impl RngTable {
    /// Wraps table bytes, checking that they form a permutation of `0..=255`.
    pub fn from_bytes(bytes: [u8; 256]) -> Result<Self, RngError> {
        let mut seen = [false; 256];
        for &v in &bytes {
            if std::mem::replace(&mut seen[usize::from(v)], true) {
                return Err(RngError::NotPermutation);
            }
        }
        Ok(RngTable(bytes))
    }

    /// Reads the table from the bytes of the user's `Game.dll` (1.26a).
    pub fn from_game_dll(dll: &[u8]) -> Result<Self, RngError> {
        let raw = pe::read_va(dll, TABLE_VA, 256).map_err(RngError::Image)?;
        let mut bytes = [0u8; 256];
        bytes.copy_from_slice(raw);
        Self::from_bytes(bytes)
    }

    /// Little-endian 32-bit word at byte offset `off` (`off + 3 <= 255`).
    fn word(&self, off: usize) -> u32 {
        u32::from_le_bytes([
            self.0[off],
            self.0[off + 1],
            self.0[off + 2],
            self.0[off + 3],
        ])
    }
}

/// One generator state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rng {
    /// Accumulator; each step returns it after adding the mix.
    pub a: u32,
    /// Packed table offsets for the next step.
    pub b: u32,
}

/// Index for one byte of `b`: subtract `offset`, wrapping by `modulus` if negative.
fn index(byte: u8, offset: u8, modulus: u8) -> u8 {
    if byte >= offset {
        byte - offset
    } else {
        byte.wrapping_sub(offset).wrapping_add(modulus)
    }
}

impl Rng {
    /// A generator seeded with `seed`.
    pub fn seeded(seed: u32) -> Self {
        let mut rng = Rng::default();
        rng.seed(seed);
        rng
    }

    /// Initialises the state from a 32-bit seed.
    pub fn seed(&mut self, s: u32) {
        let b = ((s % 59) << 10)
            | ((s % 61) << 2)
            | ((s % 53) << 18)
            | ((s / 47).wrapping_mul(17).wrapping_add(s) & 0x3F) << 26;
        self.a = s;
        self.b = b;
    }

    /// Advances one step and returns the new accumulator value.
    pub fn next_u32(&mut self, table: &RngTable) -> u32 {
        trace::record(self as *const Rng as usize, self.a, self.b);
        let [b3, b2, b1, b0] = self.b.to_be_bytes();
        let p = index(b3, 4, 188);
        let q = index(b2, 12, 212);
        let r = index(b1, 24, 236);
        let s = index(b0, 28, 244);
        let mix = table.word(usize::from(r)).rotate_left(3)
            ^ table.word(usize::from(q)).rotate_left(2)
            ^ table.word(usize::from(s))
            ^ table.word(usize::from(p)).rotate_left(1);
        self.a = self.a.wrapping_add(mix);
        self.b = u32::from(p) << 24 | u32::from(q) << 16 | u32::from(r) << 8 | u32::from(s);
        self.a
    }

    /// Integer in the inclusive range between the two bounds, as `GetRandomInt`.
    ///
    /// Equal bounds return `lo` without advancing the state. When `lo > hi`
    /// the range runs upward from `lo` over `lo - hi + 1` values; the bounds are
    /// not swapped.
    ///
    /// The size is computed in 32 bits, as in the original. The full range
    /// `(i32::MIN, i32::MAX)` (and its reverse) therefore has size 0 and returns
    /// `lo`, while still advancing the state once.
    pub fn random_int(&mut self, table: &RngTable, lo: i32, hi: i32) -> i32 {
        if lo == hi {
            return lo;
        }
        let size = if lo < hi {
            (hi as u32).wrapping_sub(lo as u32).wrapping_add(1)
        } else {
            (lo as u32).wrapping_sub(hi as u32).wrapping_add(1)
        };
        let r = self.next_u32(table);
        let offset = (u64::from(r) * u64::from(size)) >> 32;
        lo.wrapping_add(offset as u32 as i32)
    }

    /// Real number from the two float32 bounds, as `GetRandomReal`.
    ///
    /// Always advances the state by one step, even when `a == b`. The range is
    /// `|b - a|` (computed with `softfloat::arith`), the draw `U` is in `[0, 1)`, and
    /// the result is `a + range * U`, all in the engine's soft-float arithmetic.
    pub fn random_real(&mut self, table: &RngTable, a: f32, b: f32) -> f32 {
        self.random_real_soft(
            table,
            F32::from_bits(a.to_bits()),
            F32::from_bits(b.to_bits()),
        )
        .to_f32()
    }

    /// [`Rng::random_real`] on soft-float values (the form the natives use).
    pub fn random_real_soft(&mut self, table: &RngTable, a: F32, b: F32) -> F32 {
        // 0x6F3B35A0 first computes d = |sub(a, b)| (sign bit cleared) and compares it with the float
        // at 0x6FAAE614, which the start-up initialisers set to 2.0e-7 (0x3456BF95; zero in the file
        // image). If 2.0e-7 > d (x87 ordered compare) it returns `a` WITHOUT drawing. A NaN is
        // unordered, so it draws. Verified in Unicorn with the start-up value in place.
        let d = F32::from_bits(arith::sub(a, b).to_bits() & 0x7FFF_FFFF);
        if F32::from_bits(EQUAL_BOUNDS_EPS).to_f32() > d.to_f32() {
            return a;
        }
        let range = if b.to_f32() < a.to_f32() {
            arith::sub(a, b)
        } else {
            arith::sub(b, a)
        };
        let u = self.random_fraction(table);
        let t = arith::mul(range, u);
        arith::add(a, t)
    }

    /// The fraction `U` that `GetRandomReal` scales: one step, `(low 23 bits of the output | 1.0) - 1.0`, a float32
    /// in `[0, 1)`, in soft-float. The pool selection (`natives/pool.rs`) multiplies it by the total weight.
    pub fn random_fraction(&mut self, table: &RngTable) -> F32 {
        let r = self.next_u32(table);
        arith::add(
            F32::from_bits((r & 0x007F_FFFF) | 0x3F80_0000),
            F32::from_bits(0xBF80_0000),
        )
    }
}

/// The float at 0x6FAAE614 after start-up: 2.0e-7.
const EQUAL_BOUNDS_EPS: u32 = 0x3456_BF95;

/// The global generator and the 45 streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Streams {
    /// Global generator used by `GetRandomInt` and `GetRandomReal`.
    pub global: Rng,
    streams: [Rng; STREAM_COUNT],
}

impl Default for Streams {
    fn default() -> Self {
        Streams {
            global: Rng::default(),
            streams: [Rng::default(); STREAM_COUNT],
        }
    }
}

impl Streams {
    /// State of stream `k`.
    pub fn stream(&self, k: usize) -> Rng {
        self.streams[k]
    }

    /// Mutable access to stream `k`, for draws on that stream.
    pub fn stream_mut(&mut self, k: usize) -> &mut Rng {
        &mut self.streams[k]
    }

    /// Fills all 45 streams from `x`: a local generator seeded with `x` is
    /// stepped once per stream, and stream `k` is seeded with the k-th output.
    /// The global generator is not touched.
    pub fn init_streams(&mut self, table: &RngTable, x: u32) {
        let mut local = Rng::seeded(x);
        for stream in &mut self.streams {
            let out = local.next_u32(table);
            stream.seed(out);
        }
    }

    /// `SetRandomSeed(s)`: the global generator is seeded with `s` and stepped
    /// once; the streams are then initialised from that output.
    pub fn set_random_seed(&mut self, table: &RngTable, s: u32) {
        self.global.seed(s);
        let t = self.global.next_u32(table);
        self.init_streams(table, t);
    }

    /// Game-start seeding once the seed `s` is known (the seed value itself
    /// comes from game setup, see `docs/spec/rng.md` section 4.1). The global
    /// generator is seeded with `s` directly, with no step, and the streams are
    /// initialised from `s`.
    pub fn start_game(&mut self, table: &RngTable, s: u32) {
        self.global.seed(s);
        self.init_streams(table, s);
    }
}

/// One step of the game-start slot fold: `S := rotl(S xor X, 3)`.
///
/// Machine code `0x6F5C3A4C..0x6F5C3A5B` (`mov eax,[esp+0x30]; xor eax,[esp+0x44];
/// rol eax,3; mov [esp+0x44],eax`), checked against the original under Unicorn
/// (`tools/emu/game_start_emu.py`, 300 cases). See `docs/spec/game-start.md`.
pub const fn slot_fold_step(s: u32, x: u32) -> u32 {
    (s ^ x).rotate_left(3)
}

/// Folds `values` from `S = 0` with [`slot_fold_step`]. What the values are is not
/// yet established (see `docs/spec/game-start.md`, section 3); this only performs the fold.
pub fn slot_fold<I: IntoIterator<Item = u32>>(values: I) -> u32 {
    values.into_iter().fold(0, slot_fold_step)
}

/// The game-start seed `S` the original passes to the generator and the streams, given the
/// settings flag word and the fold value.
///
/// If bit `0x8000` of the settings flags (the "fixed seed" flag, settings object `+0x38`)
/// is set, the seed is the constant [`FIXED_GAME_SEED`]; otherwise it is `fold` (CONFIRMED
/// in `0x6F5C37A0` at `0x6F5C3F79..0x6F5C3F97`, checked under Unicorn).
pub const fn game_start_seed(settings_flags: u32, fold: u32) -> u32 {
    if settings_flags & 0x8000 != 0 {
        FIXED_GAME_SEED
    } else {
        fold
    }
}

/// Debug trace of every generator step (global and streams), used to attribute the draws of the global generator
/// (docs/spec/lockstep.md section 21). Off by default; costs one thread-local read per step.
pub mod trace {
    use std::cell::RefCell;

    /// One recorded step: the state before the step and the context label at that time.
    #[derive(Debug, Clone)]
    pub struct Entry {
        pub ptr: usize,
        pub a: u32,
        pub b: u32,
        pub ctx: String,
    }

    thread_local! {
        static LOG: RefCell<Option<Vec<Entry>>> = const { RefCell::new(None) };
        static CTX: RefCell<String> = const { RefCell::new(String::new()) };
        static NOTE: RefCell<String> = const { RefCell::new(String::new()) };
    }

    /// A one-shot label appended to the next recorded step (the caller, e.g. a native with its arguments).
    pub fn note(s: impl FnOnce() -> String) {
        if enabled() {
            NOTE.with(|n| *n.borrow_mut() = s());
        }
    }

    /// Starts recording.
    pub fn enable() {
        LOG.with(|l| *l.borrow_mut() = Some(Vec::new()));
    }

    /// True while recording.
    pub fn enabled() -> bool {
        LOG.with(|l| l.borrow().is_some())
    }

    /// Sets the label stored with the following steps.
    pub fn set_ctx(s: &str) {
        if enabled() {
            CTX.with(|c| {
                let mut c = c.borrow_mut();
                c.clear();
                c.push_str(s);
            });
        }
    }

    pub(super) fn record(ptr: usize, a: u32, b: u32) {
        LOG.with(|l| {
            if let Some(v) = l.borrow_mut().as_mut() {
                let mut ctx = CTX.with(|c| c.borrow().clone());
                NOTE.with(|n| {
                    let mut n = n.borrow_mut();
                    if !n.is_empty() {
                        ctx.push_str(" | ");
                        ctx.push_str(&n);
                        n.clear();
                    }
                });
                v.push(Entry { ptr, a, b, ctx });
            }
        });
    }

    /// Takes the recorded entries (recording continues, empty).
    pub fn take() -> Vec<Entry> {
        LOG.with(|l| l.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
    }
}
