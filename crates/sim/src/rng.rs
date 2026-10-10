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
        let range = if b.to_f32() < a.to_f32() {
            arith::sub(a, b)
        } else {
            arith::sub(b, a)
        };
        let r = self.next_u32(table);
        let u = arith::add(
            F32::from_bits((r & 0x007F_FFFF) | 0x3F80_0000),
            F32::from_bits(0xBF80_0000),
        );
        let t = arith::mul(range, u);
        arith::add(a, t)
    }
}

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
