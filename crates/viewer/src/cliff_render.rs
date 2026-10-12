//! Cliff models: find cliff tiles, load their MDX models and place their triangles in the terrain.
//!
//! A tile is a cliff tile when its four corners do not share one cliff layer. Its cliff type
//! comes from the map's cliff tileset list and `TerrainArt\CliffTypes.slk` (see
//! [`crate::terrain_objects::tile_cliff`]), so a CityCliffs map gets the `CityCliffs` models.
//! The model name is chosen from the corner levels relative to the lowest corner (so a
//! flat-bottomed tile at layers 5,5,5,6 is `CliffsAAAB0`), and the model is placed at the tile
//! centre on the lowest corner's height. Its triangles keep the model's UVs and sample the
//! cliff texture of the type when that texture can be read; otherwise they are flat-shaded.
//! Placement is presentation only: the positions are not verified against the game.

use std::collections::BTreeSet;

use w3map::Terrain as W3eTerrain;

use crate::props::PropBuilder;
use crate::terrain::{ground_height, Mesh, TerrainVertex, CLIFF_COLOR, TILE_SIZE};
use crate::terrain_objects::{cliff_model_path, tile_cliff, CliffTypes};

/// Counts from one cliff scan.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CliffStatistics {
    /// Tiles whose corners do not share one cliff layer.
    pub cliff_tiles_found: usize,
    /// Cliff tiles that were drawn with a model.
    pub tiles_modeled: usize,
    /// Cliff tiles whose model triangles sample the cliff texture of their type.
    pub tiles_textured: usize,
    /// Distinct model paths that were read, parsed and placed.
    pub models_loaded: usize,
    /// Distinct model paths that were wanted by a tile but not found (or did not parse), after
    /// the variation-0 fallback. Tiles that have a model through the fallback do not count.
    pub models_missing: usize,
    /// The paths counted in `models_missing`, sorted.
    pub missing_paths: Vec<String>,
    /// Cliff tiles whose cliff id is not in `CliffTypes.slk`: drawn with the default folder.
    pub tiles_default_type: usize,
}

/// The result of scanning a terrain for cliffs.
#[derive(Debug, Clone, PartialEq)]
pub struct CliffPlan {
    pub stats: CliffStatistics,
    /// One flag per tile (`tiles_x * tiles_y`, row by row): true where a model was placed. The
    /// flat cliff walls on the edges of these tiles are not drawn.
    pub modeled: Vec<bool>,
}

/// Corner levels relative to the lowest corner, in the order TL, TR, BR, BL.
pub fn relative_corner_layers(layers: [u8; 4]) -> [u8; 4] {
    let low = layers.iter().copied().min().unwrap_or(0);
    layers.map(|l| l - low)
}

/// A triangle of a placed model: world positions and the UV of each corner.
pub type PlacedTriangle = ([[f32; 3]; 3], [[f32; 2]; 3]);

/// Places a model's triangles at `origin`. Vertices are model-local and are only translated:
/// model units are world units. `uvs` is one UV per vertex; a vertex without one gets `[0, 0]`.
/// Triangles that index past the vertex list are skipped.
pub fn place_triangles(
    vertices: &[[f32; 3]],
    uvs: &[[f32; 2]],
    triangles: &[[u16; 3]],
    origin: [f32; 3],
) -> Vec<PlacedTriangle> {
    triangles
        .iter()
        .filter_map(|t| {
            let [a, b, c] = t.map(usize::from);
            let (va, vb, vc) = (vertices.get(a)?, vertices.get(b)?, vertices.get(c)?);
            let move_by = |v: &[f32; 3]| [v[0] + origin[0], v[1] + origin[1], v[2] + origin[2]];
            let uv = |i: usize| uvs.get(i).copied().unwrap_or([0.0, 0.0]);
            Some(([move_by(va), move_by(vb), move_by(vc)], [uv(a), uv(b), uv(c)]))
        })
        .collect()
}

/// Lighting factor of a triangle: 1 for a face that points straight up, 0.55 for a vertical one.
pub fn lambert(tri: &[[f32; 3]; 3]) -> f32 {
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
    0.55 + 0.45 * up
}

/// Plans the cliffs of `terrain`: picks each tile's cliff type from `types`, places the model
/// of that type and adds its triangles to `props`. Textured triangles go to the prop batch of
/// the type's texture; the others to the flat mesh of `props`, in flat cliff colour.
pub fn plan_cliffs(terrain: &W3eTerrain, types: &CliffTypes, props: &mut PropBuilder<'_, '_>) -> CliffPlan {
    let width = terrain.width as usize;
    let (tiles_x, tiles_y) = (width.saturating_sub(1), (terrain.height as usize).saturating_sub(1));
    let mut plan = CliffPlan {
        stats: CliffStatistics::default(),
        modeled: vec![false; tiles_x * tiles_y],
    };
    // Distinct model paths that were placed, and the candidates of each tile that found none.
    let mut used = BTreeSet::new();
    let mut missing = BTreeSet::new();

    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let corner_index = [ty * width + tx, ty * width + tx + 1, (ty + 1) * width + tx + 1, (ty + 1) * width + tx];
            let corners = corner_index.map(|i| &terrain.points[i]);
            let layers = corners.map(|p| p.layer_height);
            if layers.iter().all(|&l| l == layers[0]) {
                continue;
            }
            plan.stats.cliff_tiles_found += 1;

            let tile = tile_cliff(corners, &terrain.cliff_tilesets, types);
            if !tile.from_table {
                plan.stats.tiles_default_type += 1;
            }
            // The tile's variation first, then variation 0, which every cliff type has.
            let relative = relative_corner_layers(layers);
            let candidates: Vec<String> = [tile.variation, 0]
                .iter()
                .map(|&variation| cliff_model_path(&tile.model_dir, relative, u32::from(variation)))
                .collect();
            let Some(path) = candidates.iter().find(|p| props.model(p).is_some()).cloned() else {
                missing.extend(candidates);
                continue;
            };
            used.insert(path.clone());

            // Centre of the tile, on the height of its lowest corner (cliff layer included).
            let origin = [
                terrain.center[0] + (tx as f32 + 0.5) * TILE_SIZE,
                terrain.center[1] + (ty as f32 + 0.5) * TILE_SIZE,
                corners
                    .iter()
                    .map(|p| ground_height(p))
                    .fold(f32::INFINITY, f32::min),
            ];
            let texture = tile.texture.as_deref().and_then(|t| props.texture(t));
            let placed: Vec<PlacedTriangle> = match props.model(&path) {
                Some(model) => model
                    .geosets
                    .iter()
                    .flat_map(|geoset| {
                        let uvs = geoset.uv_sets.first().map(Vec::as_slice).unwrap_or(&[]);
                        place_triangles(&geoset.vertices, uvs, &geoset.triangles(), origin)
                    })
                    .collect(),
                None => continue,
            };
            for (positions, uvs) in placed {
                let k = lambert(&positions);
                match texture {
                    Some(_) => {
                        let color = [k, k, k, 1.0];
                        push_vertices(props, texture, positions, uvs, color);
                    }
                    None => {
                        let color = [CLIFF_COLOR[0] * k, CLIFF_COLOR[1] * k, CLIFF_COLOR[2] * k, 1.0];
                        push_vertices(props, None, positions, [[0.0; 2]; 3], color);
                    }
                }
            }
            if texture.is_some() {
                plan.stats.tiles_textured += 1;
            }
            plan.modeled[ty * tiles_x + tx] = true;
            plan.stats.tiles_modeled += 1;
        }
    }
    plan.stats.models_loaded = used.len();
    plan.stats.models_missing = missing.len();
    plan.stats.missing_paths = missing.into_iter().collect();
    plan
}

/// Adds one placed triangle to the props. A textured triangle samples its texture with the
/// model UVs (`cell` covers the whole texture); a flat one takes the colour.
fn push_vertices(
    props: &mut PropBuilder<'_, '_>,
    texture: Option<usize>,
    positions: [[f32; 3]; 3],
    uvs: [[f32; 2]; 3],
    color: [f32; 4],
) {
    let vertex = |i: usize| TerrainVertex {
        position: positions[i],
        uv: uvs[i],
        cell: [0.0, 0.0, 1.0, 1.0],
        color,
    };
    props.push(texture, [vertex(0), vertex(1), vertex(2)]);
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
        let uvs = [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]];
        let triangles = [[0, 1, 2], [0, 1, 7]];
        let placed = place_triangles(&vertices, &uvs, &triangles, [100.0, -20.0, 10.0]);
        assert_eq!(placed.len(), 1, "the triangle with index 7 is dropped");
        assert_eq!(
            placed[0].0,
            [[101.0, -18.0, 13.0], [104.0, -15.0, 16.0], [99.0, -20.0, 10.5]]
        );
        assert_eq!(placed[0].1, [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]]);
    }

    #[test]
    fn missing_uvs_default_to_the_origin() {
        let vertices = [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let placed = place_triangles(&vertices, &[[0.5, 0.5]], &[[0, 1, 2]], [0.0; 3]);
        assert_eq!(placed[0].1, [[0.5, 0.5], [0.0, 0.0], [0.0, 0.0]]);
    }

    #[test]
    fn lambert_is_full_for_a_flat_face_and_dim_for_a_wall() {
        let flat = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let wall = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        assert!((lambert(&flat) - 1.0).abs() < 1e-6);
        assert!((lambert(&wall) - 0.55).abs() < 1e-6);
    }
}
