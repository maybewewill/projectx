//! Pathing data at boot: `war3map.wpm` → cell words → pyramid → [`sim::orders::PathingMap`].
//!
//! The cell grid is the wpm grid (one cell is 32 world units, CONFIRMED). The grid covers the whole
//! map (on a real melee map the wpm is 4 cells per tile of playable size plus margins), so its world
//! origin is the whole-map corner (the `w3e` center offset). LIKELY, not confirmed: the source of
//! the bounds the original passes to the wpm reader is still open (`docs/spec/pathing-build.md`
//! section 1.1). A size mismatch between the grid and the w3i map size is reported.

use sim::orders::PathingMap;
use sim::pathing::build::{build_pyramid, cell_words};
use sim::pathing::{FlagLink, Search};
use softfloat::F32;
use w3map::PathMap;

/// Pathing cells per terrain tile side (a tile is 128 units).
const CELLS_PER_TILE: u32 = 4;
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
/// keeps its straight-line movement. `origin` is the playable area's minimum corner in world units,
/// and `tiles` is the whole map size in terrain tiles from the w3i data (playable size plus margins).
pub fn install(
    game: &mut sim::natives::Game,
    wpm: &[u8],
    origin: (F32, F32),
    tiles: (u32, u32),
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
    // The buildable-area grid of the production model: the pathing type byte of every cell.
    game.production_mut().grid = Some(sim::production::BuildGrid {
        width: map.width as i32,
        height: map.height as i32,
        origin_x: origin.0.to_f32(),
        origin_y: origin.1.to_f32(),
        types: words.iter().map(|&w| (w >> 24) as u8).collect(),
    });
    let pyramid = match build_pyramid(&words, map.width, map.height) {
        Ok(pyramid) => pyramid,
        Err(e) => {
            notes.push(format!("pathing pyramid not built: {e}"));
            return None;
        }
    };
    let (tw, th) = tiles;
    if tw * CELLS_PER_TILE != map.width || th * CELLS_PER_TILE != map.height {
        notes.push(format!(
            "war3map.wpm is {} x {} cells but the w3i map size is {tw} x {th} tiles \
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
