//! Terrain grid for the simulation: `war3map.w3e` corners to [`sim::terrain::TerrainGrid`], the water height offset
//! from `TerrainArt\Water.slk`, and the install into the game (cliff levels for the missile uphill miss, ground
//! height for `GetLocationZ`). See `docs/spec/terrain.md`.

use std::sync::Arc;

use gamedata::Slk;
use sim::natives::Game;
use sim::terrain::{Corner, TerrainGrid};

/// Builds the corner grid of a parsed `w3e`. The raw file fields are rebuilt from the decoded point and
/// then mapped exactly as the original's loader does (`sim::terrain::Corner::from_file`).
pub fn grid_from_w3e(t: &w3map::Terrain) -> Option<TerrainGrid> {
    let corners: Vec<Corner> = t
        .points
        .iter()
        .map(|p| {
            let water_word = p.water_level | if p.map_edge { 0x4000 } else { 0 };
            let flags_tex = p.ground_texture
                | if p.ramp { 0x10 } else { 0 }
                | if p.blight { 0x20 } else { 0 }
                | if p.water { 0x40 } else { 0 }
                | if p.boundary { 0x80 } else { 0 };
            let misc = (p.cliff_texture << 4) | (p.layer_height & 0xF);
            Corner::from_file(p.ground_height as u16, water_word, flags_tex, misc)
        })
        .collect();
    TerrainGrid::new(t.width, t.height, t.center, corners)
}

/// The water height offset of the map's tileset: the `height` column of the shallow-water row `<tileset>Sha` of
/// `TerrainArt\Water.slk`, times 128 (the editor's known `-89.6` for Lordaeron Summer is `-0.7 * 128`).
/// UNSURE: which row is entry 0 of the original's water table, and the factor (docs/spec/terrain.md).
pub fn water_offset(water_slk: &Slk, tileset: u8) -> Option<f32> {
    let key = format!("{}Sha", tileset as char);
    let height = water_slk.get(&key, "height")?.as_real()?;
    Some((height as f32) * 128.0)
}

/// Reads `TerrainArt\Water.slk` from the game archives (a later archive overrides).
fn read_water_slk(archives: &[&[u8]]) -> Option<Slk> {
    let mut found = None;
    for bytes in archives {
        let Ok(archive) = mpq::Archive::open(bytes) else {
            continue;
        };
        if let Ok(data) = archive.read("TerrainArt\\Water.slk") {
            found = Some(data);
        }
    }
    Slk::parse_bytes(&found?).ok()
}

/// Installs the terrain into the game: `Game::set_terrain` (which also installs the cliff levels of the missile miss
/// gate). Returns a short note for the boot report.
pub fn install(game: &mut Game, terrain: &w3map::Terrain, game_archives: &[&[u8]]) -> String {
    let Some(mut grid) = grid_from_w3e(terrain) else {
        return "terrain grid not built (empty w3e)".to_string();
    };
    let offset = read_water_slk(game_archives).and_then(|s| water_offset(&s, terrain.main_tileset));
    grid.set_water_offset(offset);
    let note = format!(
        "terrain grid {}x{} corners, water offset {}",
        grid.width(),
        grid.height(),
        offset.map_or("none".to_string(), |o| format!("{o}"))
    );
    game.set_terrain(Some(Arc::new(grid)));
    note
}
