//! Deterministic simulation primitives for the Warcraft III 1.26a rewrite.
//!
//! * [`rng`]: the gameplay random generator (one global generator plus 45
//!   streams), with the permutation table read from the user's own `Game.dll`.
//! * [`timer`]: the agile (game) clock and the timer queue.
//! * [`script`]: the JASS host that runs a `jass::vm::Vm` on that clock.
//! * [`natives`]: the timer, trigger and sleep natives, with their behaviour notes (timers and
//!   triggers are game objects with handles, and their expiries share the timer queue).
//! * [`pathing`]: the integer square root, the search heap, the pathing grid and the
//!   region-level A* (the A* is unverified against the original).
//! * [`handles`]: JASS handle ids (allocation order, LIFO reuse, reference counts).
//!
//! Real arithmetic uses the bit-exact helpers of the `softfloat` crate. Nothing here reads the
//! system clock, uses hash maps with random seeds, or depends on the host floating point unit.

pub mod abilities;
pub mod attack_timing;
pub mod ai;
pub mod ai_manager;
pub mod ai_site;
pub mod ai_towns;
pub mod agile;
pub mod captains;
pub mod cast;
pub mod auras;
pub mod combat;
pub mod commands;
pub mod bounce;
pub mod construction;
pub mod damage;
pub mod daynight;
pub mod economy;
pub mod handles;
pub mod heroes;
pub mod human_heroes;
pub mod kill_xp;
pub mod hashmap;
pub mod lockstep;
pub mod missiles;
pub mod mover;
pub mod movement;
pub mod natives;
pub mod occupancy;
pub mod orc;
pub mod orders;
pub mod pathing;
pub mod push;
pub mod production;
pub mod regen;
pub mod revive;
pub mod rng;
pub mod script;
pub mod shop;
pub mod sync;
pub mod splash;
pub mod target_accept;
pub mod terrain;
pub mod timer;
pub mod unit_counts;
pub mod undead_ne;
pub mod unitclass;
pub mod unit_position;
mod unit_stats;
pub mod unitz;
pub mod vision;

mod pe;

pub use handles::{HandleTable, HANDLE_BASE};
pub use natives::{Game, HandleKind, Payload, Playable};
pub use rng::{Rng, RngError, RngTable, Streams, FIXED_GAME_SEED, STREAM_COUNT};
pub use script::ScriptHost;
pub use timer::{AgileClock, Fired, TimerId, TimerQueue, STEP, STEP_BITS};
