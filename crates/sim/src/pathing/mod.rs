//! Pathing primitives of the 1.26a rewrite (specs: `docs/spec/pathing.md`, `pathing-2.md`,
//! `pathing-3.md`).
//!
//! * [`isqrt`]: the approximate integer square root behind every search cost (verified
//!   against the original).
//! * [`heap`]: the 1-based min-heap of the open list (verified against the original).
//! * [`grid`]: 32-unit cells, the cell word layout and world-to-cell conversion.
//! * [`pyramid`]: the four-level pyramid of states and the lazy region labeller (verified
//!   against the original).
//! * [`search`]: the request setup, the A* loop and the path fallbacks (verified against the
//!   original for mode 1; the block expander is verified on the generated cases).

pub mod grid;
pub mod heap;
pub mod isqrt;
pub mod pyramid;
pub mod search;

pub use grid::{cell_coordinate, path_type_mask, scale_by_1_32, Grid, NO_OCCUPANT};
pub use heap::Heap;
pub use isqrt::isqrt;
pub use pyramid::{Labels, Level, Node, Pyramid, Record, BLOCKED, REFINE};
pub use search::{FlagLink, Path, Request, Search, Status, POP_BUDGET};
