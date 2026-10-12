//! Cliff models: find cliff tiles, load their MDX models and place their triangles in the terrain.
//!
//! A tile is a cliff tile when its four corners do not share one cliff layer. The model name is
//! chosen from the corner levels relative to the lowest corner (so a flat-bottomed tile at layers
//! 5,5,5,6 is `CliffsAAAB0`), and the model is placed at the tile centre on the lowest corner's
//! height. Placement is presentation only: the positions are not verified against the game.

use std::collections::HashMap;

use w3map::Terrain as W3eTerrain;

use crate::terrain::{ground_height, Mesh, TerrainVertex, CLIFF_COLOR, TILE_SIZE};
use crate::terrain_objects::cliff_model_path;

/// Counts from one cliff scan.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CliffStatistics {
    /// Tiles whose corners do not share one cliff layer.
    pub cliff_tiles_found: usize,
    /// Cliff tiles that were drawn with a model.
    pub tiles_modeled: usize,
    /// Distinct model paths that were read and parsed.
    pub models_loaded: usize,
    /// Distinct model paths that were not found or did not parse.
    pub models_missing: usize,
}

/// The result of scanning a terrain for cliffs.
#[derive(Debug, Clone, PartialEq)]
pub struct CliffPlan {
    pub stats: CliffStatistics,
    /// One flag per tile (`tiles_x * tiles_y`, row by row): true where a model was placed. The
    /// flat cliff walls on the edges of these tiles are not drawn.
    pub modeled: Vec<bool>,
    /// Triangles of every placed model, in world space, with flat shading.
    pub mesh: Mesh,
}

/// Corner levels relative to the lowest corner, in the order TL, TR, BR, BL.
pub fn relative_corner_layers(layers: [u8; 4]) -> [u8; 4] {
    let low = layers.iter().copied().min().unwrap_or(0);
    layers.map(|l| l - low)
}

/// Places a model's triangles at `origin`. Vertices are model-local and are only translated:
/// model units are world units. Triangles that index past the vertex list are skipped.
pub fn place_triangles(
    vertices: &[[f32; 3]],
    triangles: &[[u16; 3]],
    origin: [f32; 3],
) -> Vec<[[f32; 3]; 3]> {
    triangles
        .iter()
        .filter_map(|t| {
            let [a, b, c] = t.map(usize::from);
            let (va, vb, vc) = (vertices.get(a)?, vertices.get(b)?, vertices.get(c)?);
            let move_by = |v: &[f32; 3]| [v[0] + origin[0], v[1] + origin[1], v[2] + origin[2]];
            Some([move_by(va), move_by(vb), move_by(vc)])
        })
        .collect()
}

/// Lit flat colour of a triangle: the cliff colour, darker where the face turns sideways.
fn shade(tri: &[[f32; 3]; 3]) -> [f32; 4] {
    let e1 = [tri[1][0] - tri[0][0], tri[1][1] - tri[0][1], tri[1][2] - tri[0][2]];
    let e2 = [tri[2][0] - tri[0][0], tri[2][1] - tri[0][1], tri[2][2] - tri[0][2]];
    // Face normal = e1 x e2; how much it points up (z) sets the light.
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let up = if len > 0.0 { n[2].abs() / len } else { 1.0 };
    let k = 0.55 + 0.45 * up;
    [CLIFF_COLOR[0] * k, CLIFF_COLOR[1] * k, CLIFF_COLOR[2] * k, 1.0]
}

fn push_triangle(mesh: &mut Mesh, tri: [[f32; 3]; 3]) {
    let color = shade(&tri);
    mesh.triangle(tri.map(|position| TerrainVertex {
        position,
        uv: [0.0, 0.0],
        cell: [0.0; 4],
        color,
    }));
}

/// Model cache over the archives: each path is read once, later archives override earlier ones.
struct ModelCache<'a, 'b> {
    archives: &'a [mpq::Archive<'b>],
    models: HashMap<String, Option<mdx::Model>>,
}

impl ModelCache<'_, '_> {
    /// Returns the model at `path`, reading it on first use. Counts the first outcome per path.
    fn get(&mut self, path: &str, stats: &mut CliffStatistics) -> Option<&mdx::Model> {
        if !self.models.contains_key(path) {
            let model = self.archives.iter().rev().find_map(|archive| {
                let bytes = archive.read(path).ok()?;
                mdx::Model::parse(&bytes).ok()
            });
            if model.is_some() {
                stats.models_loaded += 1;
            } else {
                stats.models_missing += 1;
            }
            self.models.insert(path.to_string(), model);
        }
        self.models.get(path).and_then(Option::as_ref)
    }
}

/// Scans `terrain` for cliff tiles of `cliff_tileset` and places their models.
///
/// `archives` are the game archives in load order (later ones override earlier ones).
pub fn plan_cliffs(terrain: &W3eTerrain, cliff_tileset: &str, archives: &[&[u8]]) -> CliffPlan {
    let opened: Vec<mpq::Archive<'_>> = archives
        .iter()
        .filter_map(|bytes| mpq::Archive::open(bytes).ok())
        .collect();
    let mut cache = ModelCache {
        archives: &opened,
        models: HashMap::new(),
    };

    let width = terrain.width as usize;
    let (tiles_x, tiles_y) = (width.saturating_sub(1), (terrain.height as usize).saturating_sub(1));
    let mut plan = CliffPlan {
        stats: CliffStatistics::default(),
        modeled: vec![false; tiles_x * tiles_y],
        mesh: Mesh::default(),
    };

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let corner_index = [ty * width + tx, ty * width + tx + 1, (ty + 1) * width + tx + 1, (ty + 1) * width + tx];
            let corners = corner_index.map(|i| &terrain.points[i]);
            let layers = corners.map(|p| p.layer_height);
            if layers.iter().all(|&l| l == layers[0]) {
                continue;
            }
            plan.stats.cliff_tiles_found += 1;

            let path = cliff_model_path(cliff_tileset, relative_corner_layers(layers), 0);
            let Some(model) = cache.get(&path, &mut plan.stats) else {
                continue;
            };

            // Centre of the tile, on the height of its lowest corner (cliff layer included).
            let origin = [
                terrain.center[0] + (tx as f32 + 0.5) * TILE_SIZE,
                terrain.center[1] + (ty as f32 + 0.5) * TILE_SIZE,
                corners
                    .iter()
                    .map(|p| ground_height(p))
                    .fold(f32::INFINITY, f32::min),
            ];
            for geoset in &model.geosets {
                for tri in place_triangles(&geoset.vertices, &geoset.triangles(), origin) {
                    push_triangle(&mut plan.mesh, tri);
                }
            }
            plan.modeled[ty * tiles_x + tx] = true;
            plan.stats.tiles_modeled += 1;
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_levels_are_relative_to_the_lowest_corner() {
        assert_eq!(relative_corner_layers([5, 5, 5, 6]), [0, 0, 0, 1]);
        assert_eq!(relative_corner_layers([3, 3, 4, 3]), [0, 0, 1, 0]);
        assert_eq!(relative_corner_layers([2, 2, 2, 2]), [0, 0, 0, 0]);
    }

    #[test]
    fn vertex_transform_translates_by_origin_and_skips_bad_indices() {
        let vertices = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [-1.0, 0.0, 0.5]];
        let triangles = [[0, 1, 2], [0, 1, 7]];
        let placed = place_triangles(&vertices, &triangles, [100.0, -20.0, 10.0]);
        assert_eq!(placed.len(), 1, "the triangle with index 7 is dropped");
        assert_eq!(
            placed[0],
            [[101.0, -18.0, 13.0], [104.0, -15.0, 16.0], [99.0, -20.0, 10.5]]
        );
    }
}
