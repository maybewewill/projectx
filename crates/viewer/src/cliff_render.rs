//! Cliff model rendering: load MDX models and count cliff tiles.
//!
//! Scans terrain for tiles with height differences and attempts to load corresponding cliff models.

use std::collections::HashMap;

use crate::terrain_objects::cliff_model_path;
use w3map::Terrain as W3eTerrain;

/// Cliff model cache: path → loaded successfully.
pub struct CliffModelCache {
    loaded: HashMap<String, bool>,
    stats: CliffStatistics,
}

impl CliffModelCache {
    pub fn new() -> Self {
        CliffModelCache {
            loaded: HashMap::new(),
            stats: CliffStatistics::default(),
        }
    }

    /// Try to load a cliff model from MPQ archives (later archives override earlier ones).
    pub fn load_or_cache(&mut self, path: &str, archives: &[&[u8]]) -> bool {
        if let Some(&loaded) = self.loaded.get(path) {
            return loaded;
        }

        // Try to read from the last archive that has it
        let mut found = false;
        for archive_bytes in archives.iter().rev() {
            if let Ok(archive) = mpq::Archive::open(archive_bytes) {
                if let Ok(bytes) = archive.read(path) {
                    // Try to parse as MDX
                    if let Ok(_model) = mdx::Model::parse(&bytes) {
                        found = true;
                        self.stats.models_loaded += 1;
                        break;
                    }
                }
            }
        }

        if !found {
            self.stats.models_missing += 1;
        }

        self.loaded.insert(path.to_string(), found);
        found
    }

    pub fn stats(&self) -> CliffStatistics {
        self.stats
    }
}

/// Scan terrain for cliff tiles and try to load their models.
pub fn scan_cliff_tiles(
    terrain: &W3eTerrain,
    cliff_tileset: &str,
    archives: &[&[u8]],
) -> CliffStatistics {
    let mut cache = CliffModelCache::new();
    let (tile_width, tile_height) = (terrain.width as usize - 1, terrain.height as usize - 1);

    for ty in 0..tile_height {
        for tx in 0..tile_width {
            // Get the four corners of this tile
            let idx_tl = ty * (terrain.width as usize) + tx;
            let idx_tr = idx_tl + 1;
            let idx_br = idx_tl + (terrain.width as usize) + 1;
            let idx_bl = idx_tl + (terrain.width as usize);

            let corners = [
                &terrain.points[idx_tl],
                &terrain.points[idx_tr],
                &terrain.points[idx_br],
                &terrain.points[idx_bl],
            ];

            // Check if this tile has cliff height differences
            let heights = [
                corners[0].layer_height,
                corners[1].layer_height,
                corners[2].layer_height,
                corners[3].layer_height,
            ];

            if heights[0] == heights[1] && heights[1] == heights[2] && heights[2] == heights[3] {
                // No cliff on this tile
                continue;
            }

            cache.stats.cliff_tiles_found += 1;

            // Build the cliff model path
            let path = cliff_model_path(cliff_tileset, heights, 0);

            // Try to load the model
            let _ = cache.load_or_cache(&path, archives);
        }
    }

    cache.stats
}

/// Statistics on cliff tile and model loading.
#[derive(Debug, Default, Clone, Copy)]
pub struct CliffStatistics {
    pub cliff_tiles_found: usize,
    pub models_loaded: usize,
    pub models_missing: usize,
}
