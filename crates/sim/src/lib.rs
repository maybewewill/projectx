//! Deterministic simulation primitives for the Warcraft III 1.26a rewrite.
//!
//! * [`softfloat`]: the single-precision add, subtract and multiply that the
//!   original engine uses for its random real numbers, on raw IEEE bit patterns.
//! * [`rng`]: the gameplay random generator (one global generator plus 45
//!   streams), with the permutation table read from the user's own `Game.dll`.
//!
//! Nothing here reads the system clock, uses hash maps with random seeds, or
//! depends on the host floating point unit.

pub mod rng;
pub mod softfloat;

mod pe;

pub use rng::{Rng, RngError, RngTable, Streams, FIXED_GAME_SEED, STREAM_COUNT};
