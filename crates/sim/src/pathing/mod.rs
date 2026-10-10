//! Pathing primitives of the 1.26a rewrite (specs: `docs/spec/pathing.md`, `pathing-2.md`).
//!
//! * [`isqrt`]: the approximate integer square root behind every search cost (verified
//!   against the original).
//! * [`heap`]: the 1-based min-heap of the open list (verified against the original).
//! * [`grid`]: 32-unit cells, the cell word layout and world-to-cell conversion.
//! * [`search`]: the region-level A*. UNVERIFIED against the original; see its module doc.

pub mod grid;
pub mod heap;
pub mod isqrt;
pub mod search;

pub use grid::{cell_coordinate, path_type_mask, scale_by_1_32, Grid, NO_OCCUPANT};
pub use heap::Heap;
pub use isqrt::isqrt;
pub use search::{find_path, Path, RegionMap, Status, NO_REGION, POP_BUDGET};
