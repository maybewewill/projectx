//! Native (engine-provided) functions for the JASS VM.
//!
//! Only natives that need no game state live here (`pure`). Natives that read or change game
//! state belong in later modules and are registered on the same `NativeRegistry`.

pub mod pure;

use crate::vm::NativeRegistry;

/// Registers every native that needs no game state (math, conversions, strings).
/// A later registration with the same name replaces an earlier one.
pub fn register_pure(reg: &mut NativeRegistry) {
    pure::register(reg);
}
