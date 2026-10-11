//! Pathing primitives of the 1.26a rewrite (specs: `docs/spec/pathing.md`, `pathing-2.md`,
//! `pathing-3.md`).
//!
//! * [`isqrt`]: the approximate integer square root behind every search cost (verified
//!   against the original).
//! * [`heap`]: the 1-based min-heap of the open list (verified against the original).
//! * [`grid`]: 32-unit cells, the cell word layout and world-to-cell conversion.
//! * [`build`]: the cell words from the wpm bytes and the pyramid levels built from them (verified
//!   against the original by `tools/emu/pyramid_build_vectors.py`).
//! * [`pyramid`]: the four-level pyramid of states and the lazy region labeller (verified
//!   against the original).
//! * [`search`]: the request setup, the A* loop, the request dispatcher and the path fallbacks
//!   (verified against the original for modes 1 and 2 on whole requests).
//! * [`request`]: the waypoint output and the cell centre (verified on the waypoint arrays).

pub mod build;
pub mod grid;
pub mod heap;
pub mod isqrt;
pub mod pyramid;
pub mod request;
pub mod search;

pub use build::{build_levels, build_pyramid, cell_words, level_dims, wpm_top_byte, BuildError};
pub use grid::{cell_coordinate, path_type_mask, scale_by_1_32, Grid, NO_OCCUPANT};
pub use heap::Heap;
pub use isqrt::isqrt;
pub use pyramid::{Labels, Level, Node, Pyramid, Record, BLOCKED, REFINE};
pub use search::{Dispatch, FlagLink, Path, Request, Search, Status, POP_BUDGET};
