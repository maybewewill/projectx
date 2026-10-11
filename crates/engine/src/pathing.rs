//! Pathing data at boot: `war3map.wpm` → cell words → pyramid → [`sim::orders::PathingMap`].
//!
//! The cell grid is the wpm grid (one cell is 32 world units, CONFIRMED). The world origin of the
//! grid is the minimum corner of the playable area. UNSURE: the original takes the origin from its
//! playable bounds (`docs/spec/pathing-build.md` section 1.1), and the w3i camera bounds used by
//! [`crate::playable_area`] are only an approximation of those, so the origin is the least certain
//! part of the boot. A size mismatch between the grid and the w3i playable size is reported.

use sim::orders::PathingMap;
use sim::pathing::build::{build_pyramid, cell_words};
use sim::pathing::{FlagLink, Search};
use softfloat::F32;
use w3map::PathMap;

/// Length of a pathing cell in world units.
const CELL_UNITS: u32 = 32;
/// Growth step and initial capacity of the search's node list (UNSURE: the original's values).
const LIST_GROW: usize = 8;
const LIST_CAPACITY: usize = 0;

/// What the boot installed for the pathing data (the report shows it).
#[derive(Debug, Clone, PartialEq)]
pub struct PathingSummary {
    /// Width of the cell grid.
    pub width: u32,
    /// Height of the cell grid.
    pub height: u32,
    /// World position of cell `(0, 0)`.
    pub origin: (f32, f32),
}

/// Builds the pathing data from the wpm bytes and installs it in the game.
///
/// Returns `Ok(None)` when the grid cannot be used (the reason is pushed to `notes`); the game then
/// keeps its straight-line movement. `playable` is the playable rectangle `(min_x, min_y, width,
/// height)` in world units, from the w3i data.
pub fn install(
    game: &mut sim::natives::Game,
    wpm: &[u8],
    origin: (F32, F32),
    playable: (u32, u32),
    notes: &mut Vec<String>,
) -> Option<PathingSummary> {
    let map = match PathMap::parse(wpm) {
        Ok(map) => map,
        Err(e) => {
            notes.push(format!("war3map.wpm not used: {e}"));
            return None;
        }
    };
    let words = match cell_words(map.width, map.height, &map.cells) {
        Ok(words) => words,
        Err(e) => {
            notes.push(format!("war3map.wpm not used: {e}"));
            return None;
        }
    };
    let pyramid = match build_pyramid(&words, map.width, map.height) {
        Ok(pyramid) => pyramid,
        Err(e) => {
            notes.push(format!("pathing pyramid not built: {e}"));
            return None;
        }
    };
    let (pw, ph) = playable;
    if pw != map.width * CELL_UNITS || ph != map.height * CELL_UNITS {
        notes.push(format!(
            "war3map.wpm is {} x {} cells but the w3i playable size is {pw} x {ph} units \
             (UNSURE which one the origin and the grid follow)",
            map.width, map.height
        ));
    }
    let search = Search::new(
        pyramid,
        vec![FlagLink::default(); 256],
        LIST_GROW,
        LIST_CAPACITY,
    );
    game.set_pathing(Some(PathingMap { search, origin }));
    Some(PathingSummary {
        width: map.width,
        height: map.height,
        origin: (origin.0.to_f32(), origin.1.to_f32()),
    })
}
