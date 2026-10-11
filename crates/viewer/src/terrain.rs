//! Terrain from a `war3map.w3e` file: GPU-independent mesh building, the ground texture atlas
//! and UV logic. Nothing here touches wgpu, so all of it is unit-tested with synthetic data.
//!
//! This is presentation code. It has to look and behave like the original terrain, but it is
//! not bit-exact (that is the simulation's job).
//!
//! # Assumptions (not verified against retail files)
//!
//! * Tile points form a `W x H` grid. The `(W-1) x (H-1)` tiles are [`TILE_SIZE`] = 128 world
//!   units wide. Point `(0, 0)` sits at the file's offset (`Terrain::center`), the bottom-left
//!   corner of the map.
//! * Ground height: the raw `i16` minus 8192 (`0x2000`), scaled by 128/512 world units per raw
//!   unit ([`HEIGHT_SCALE`]).
//! * Cliff layer: the point's layer height times [`CLIFF_LAYER_HEIGHT`] (128) is added to its
//!   ground height.
//! * Water level uses the same height formula as the ground (raw minus 8192, scaled).
//! * A tile point's ground texture index selects a tile sheet. The atlas has 4x4 sheet slots
//!   (16 ground textures). Each sheet has `cols x rows` variation cells (4x4 or 8x4). The
//!   variation index wraps inside the sheet.
//! * Ground texture blending: each tile is drawn with its dominant texture (most corners,
//!   ties go to the earliest corner). Each other texture of the tile is drawn on top with a
//!   per-corner alpha that is 1 at corners using it and 0 elsewhere, interpolated across the
//!   tile. This approximates the original's edge blending.
//! * Cliffs: where two neighbouring points have different cliff layers (and neither is a ramp),
//!   a vertical wall of flat colour is drawn on their shared edge. Cliff models are not drawn.
//! * Water: a tile gets a water quad when all four of its corners have the water flag.

use bytemuck::{Pod, Zeroable};
use w3map::{Terrain as W3eTerrain, TilePoint};

use crate::scene::{Aabb, TextureData};

/// World units per tile edge.
pub const TILE_SIZE: f32 = 128.0;
/// Raw ground height that means zero height.
pub const HEIGHT_CENTER: f32 = 8192.0;
/// World units per raw height unit (128 world units per 512 raw units).
pub const HEIGHT_SCALE: f32 = TILE_SIZE / 512.0;
/// World units per cliff layer.
pub const CLIFF_LAYER_HEIGHT: f32 = TILE_SIZE;
/// Ground textures the atlas can hold (4 bits in the file).
pub const GROUND_SLOTS: usize = 16;
/// Sheet slots per atlas row and column.
pub const SHEET_GRID: u32 = 4;
/// Flat colour of cliff walls.
pub const CLIFF_COLOR: [f32; 4] = [0.36, 0.31, 0.25, 1.0];
/// Flat colour of water (alpha is used for blending).
pub const WATER_COLOR: [f32; 4] = [0.12, 0.34, 0.58, 0.6];
/// Tile-local UV of the four tile corners: (0,0), (1,0), (1,1), (0,1).
const CORNER_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
/// Point offsets of the four tile corners, in the same order as [`CORNER_UV`].
const CORNER_OFFSET: [(usize, usize); 4] = [(0, 0), (1, 0), (1, 1), (0, 1)];

/// World height of a ground point, including its cliff layer.
pub fn ground_height(point: &TilePoint) -> f32 {
    (f32::from(point.ground_height) - HEIGHT_CENTER) * HEIGHT_SCALE
        + f32::from(point.layer_height) * CLIFF_LAYER_HEIGHT
}

/// World height of a point's water level.
pub fn water_height(point: &TilePoint) -> f32 {
    (f32::from(point.water_level) - HEIGHT_CENTER) * HEIGHT_SCALE
}

/// Layout of one ground tile sheet: how many variation cells it has, in columns and rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetLayout {
    pub cols: u32,
    pub rows: u32,
}

impl SheetLayout {
    /// 4 columns, 4 rows: 16 variations.
    pub const GRID_4X4: SheetLayout = SheetLayout { cols: 4, rows: 4 };
    /// 8 columns, 4 rows: 32 variations.
    pub const GRID_8X4: SheetLayout = SheetLayout { cols: 8, rows: 4 };

    pub fn cells(self) -> u32 {
        self.cols * self.rows
    }

    /// Column and row of a variation. Variations past the last cell wrap around.
    pub fn cell(self, variation: u8) -> (u32, u32) {
        let index = u32::from(variation) % self.cells();
        (index % self.cols, index / self.cols)
    }
}

impl Default for SheetLayout {
    fn default() -> Self {
        SheetLayout::GRID_4X4
    }
}

/// Rectangle in atlas UV space: origin and size. Already inset by half a texel so that linear
/// filtering never samples a neighbouring cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvRect {
    pub min: [f32; 2],
    pub size: [f32; 2],
}

impl UvRect {
    /// `[min.x, min.y, size.x, size.y]`, the layout the shader reads.
    pub fn to_array(self) -> [f32; 4] {
        [self.min[0], self.min[1], self.size[0], self.size[1]]
    }
}

/// The atlas is a square of `SHEET_GRID x SHEET_GRID` sheets, each `sheet_px` pixels square.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasLayout {
    pub sheet_px: u32,
}

impl AtlasLayout {
    /// Side of the atlas in pixels.
    pub fn size(self) -> u32 {
        self.sheet_px * SHEET_GRID
    }

    /// UV rectangle of one variation of one ground texture.
    pub fn cell_uv(self, texture: u8, variation: u8, sheet: SheetLayout) -> UvRect {
        let slot = u32::from(texture) % (SHEET_GRID * SHEET_GRID);
        let (slot_x, slot_y) = (slot % SHEET_GRID, slot / SHEET_GRID);
        let (cell_x, cell_y) = sheet.cell(variation);
        let sheet_px = self.sheet_px as f32;
        let cell_w = sheet_px / sheet.cols as f32;
        let cell_h = sheet_px / sheet.rows as f32;
        let x0 = slot_x as f32 * sheet_px + cell_x as f32 * cell_w;
        let y0 = slot_y as f32 * sheet_px + cell_y as f32 * cell_h;
        let atlas = self.size() as f32;
        UvRect {
            min: [(x0 + 0.5) / atlas, (y0 + 0.5) / atlas],
            size: [(cell_w - 1.0) / atlas, (cell_h - 1.0) / atlas],
        }
    }
}

impl Default for AtlasLayout {
    fn default() -> Self {
        AtlasLayout { sheet_px: 256 }
    }
}

/// RGBA8 atlas, `size x size` pixels, row-major from the top-left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atlas {
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// Approximate average colours of the 16 ground slots, used when no texture file is available.
const PALETTE: [[f32; 3]; GROUND_SLOTS] = [
    [0.30, 0.52, 0.25],
    [0.44, 0.38, 0.22],
    [0.55, 0.50, 0.36],
    [0.35, 0.33, 0.30],
    [0.26, 0.44, 0.36],
    [0.50, 0.42, 0.28],
    [0.62, 0.58, 0.48],
    [0.28, 0.30, 0.20],
    [0.40, 0.46, 0.22],
    [0.58, 0.46, 0.32],
    [0.24, 0.36, 0.42],
    [0.66, 0.62, 0.52],
    [0.42, 0.30, 0.24],
    [0.34, 0.50, 0.40],
    [0.50, 0.50, 0.44],
    [0.30, 0.28, 0.36],
];

/// Builds the atlas. Slots with a texture get that texture resampled (nearest) to the sheet
/// size. Missing slots get a deterministic procedural sheet: one palette colour per slot with
/// slightly different cells, so the variations are visible without any game files.
pub fn build_atlas(
    layout: AtlasLayout,
    sheet: SheetLayout,
    sources: &[Option<&TextureData>],
) -> Atlas {
    let size = layout.size();
    let s = layout.sheet_px;
    let mut rgba = vec![0u8; (size as usize) * (size as usize) * 4];
    for slot in 0..GROUND_SLOTS as u32 {
        let (slot_x, slot_y) = (slot % SHEET_GRID, slot / SHEET_GRID);
        let source = sources.get(slot as usize).copied().flatten();
        for py in 0..s {
            for px in 0..s {
                let color = match source {
                    Some(texture) => sample_nearest(texture, px, py, s),
                    None => procedural_texel(slot, sheet, s, px, py),
                };
                let dst_x = slot_x * s + px;
                let dst_y = slot_y * s + py;
                let index = ((dst_y * size + dst_x) * 4) as usize;
                rgba[index..index + 4].copy_from_slice(&color);
            }
        }
    }
    Atlas { size, rgba }
}

fn sample_nearest(texture: &TextureData, px: u32, py: u32, sheet_px: u32) -> [u8; 4] {
    let sx = (u64::from(px) * u64::from(texture.width) / u64::from(sheet_px))
        .min(u64::from(texture.width) - 1) as usize;
    let sy = (u64::from(py) * u64::from(texture.height) / u64::from(sheet_px))
        .min(u64::from(texture.height) - 1) as usize;
    let index = (sy * texture.width as usize + sx) * 4;
    [
        texture.rgba[index],
        texture.rgba[index + 1],
        texture.rgba[index + 2],
        texture.rgba[index + 3],
    ]
}

fn procedural_texel(slot: u32, sheet: SheetLayout, sheet_px: u32, px: u32, py: u32) -> [u8; 4] {
    let cell_w = (sheet_px / sheet.cols).max(1);
    let cell_h = (sheet_px / sheet.rows).max(1);
    let cx = (px / cell_w).min(sheet.cols - 1);
    let cy = (py / cell_h).min(sheet.rows - 1);
    let variation = cy * sheet.cols + cx;
    let base = PALETTE[slot as usize];
    let cell_factor = 0.85 + 0.30 * hash3(slot, variation, 1);
    let noise = 0.94 + 0.12 * hash3(slot, px, py);
    let border = if px.is_multiple_of(cell_w) || py.is_multiple_of(cell_h) {
        0.6
    } else {
        1.0
    };
    let mut out = [255u8; 4];
    for (channel, value) in base.iter().enumerate() {
        let v = (value * cell_factor * noise * border).clamp(0.0, 1.0);
        out[channel] = (v * 255.0 + 0.5) as u8;
    }
    out
}

/// Deterministic hash to `[0, 1]`. Used for procedural texture noise only.
fn hash3(a: u32, b: u32, c: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// One vertex of the terrain. The same layout is used by every terrain pipeline.
///
/// * `position`: world space.
/// * `uv`: tile-local UV in `[0, 1]`.
/// * `cell`: atlas cell of the tile's texture (`[min.x, min.y, size.x, size.y]`).
/// * `color`: flat colour (flat pipelines), or `[_, _, _, alpha]` as the blend weight of a
///   textured layer.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct TerrainVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub cell: [f32; 4],
    pub color: [f32; 4],
}

/// Indexed triangles. Quads are 4 vertices and 6 indices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<TerrainVertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    fn quad(&mut self, vertices: [TerrainVertex; 4]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&vertices);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn quad_count(&self) -> usize {
        self.indices.len() / 6
    }
}

/// Terrain split by pipeline. Draw order: `ground`, `cliffs`, `overlays`, `water`.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainMesh {
    /// Opaque dominant texture of every tile.
    pub ground: Mesh,
    /// Blended secondary textures (alpha per corner).
    pub overlays: Mesh,
    /// Vertical cliff walls, flat colour, opaque.
    pub cliffs: Mesh,
    /// Water quads, flat colour, blended.
    pub water: Mesh,
    /// Tiles along x and y.
    pub tiles: (u32, u32),
}

impl TerrainMesh {
    /// Bounding box of all vertices.
    pub fn bounds(&self) -> Option<Aabb> {
        Aabb::from_points(
            [&self.ground, &self.overlays, &self.cliffs, &self.water]
                .into_iter()
                .flat_map(|mesh| mesh.vertices.iter().map(|v| v.position)),
        )
    }
}

/// Settings for [`build_mesh`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MeshOptions {
    pub sheet: SheetLayout,
    pub atlas: AtlasLayout,
}

/// Why a terrain could not be meshed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    #[error("terrain has {width}x{height} points, at least 2x2 are needed")]
    TooSmall { width: u32, height: u32 },
    #[error("terrain is {width}x{height} but has {points} points")]
    PointCount {
        width: u32,
        height: u32,
        points: usize,
    },
}

/// Builds the terrain mesh from parsed `war3map.w3e` data.
pub fn build_mesh(terrain: &W3eTerrain, options: MeshOptions) -> Result<TerrainMesh, MeshError> {
    let (width, height) = (terrain.width as usize, terrain.height as usize);
    if width < 2 || height < 2 {
        return Err(MeshError::TooSmall {
            width: terrain.width,
            height: terrain.height,
        });
    }
    if terrain.points.len() != width * height {
        return Err(MeshError::PointCount {
            width: terrain.width,
            height: terrain.height,
            points: terrain.points.len(),
        });
    }
    let point = |x: usize, y: usize| &terrain.points[y * width + x];
    let position = |x: usize, y: usize| -> [f32; 3] {
        [
            terrain.center[0] + x as f32 * TILE_SIZE,
            terrain.center[1] + y as f32 * TILE_SIZE,
            ground_height(point(x, y)),
        ]
    };

    let mut mesh = TerrainMesh {
        ground: Mesh::default(),
        overlays: Mesh::default(),
        cliffs: Mesh::default(),
        water: Mesh::default(),
        tiles: ((width - 1) as u32, (height - 1) as u32),
    };

    for ty in 0..height - 1 {
        for tx in 0..width - 1 {
            let corners = CORNER_OFFSET.map(|(dx, dy)| (tx + dx, ty + dy));
            let points = corners.map(|(x, y)| point(x, y));
            let positions = corners.map(|(x, y)| position(x, y));
            let textures = points.map(|p| p.ground_texture);
            let variations = points.map(|p| p.ground_variation);

            // Dominant texture: drawn opaque across the whole tile.
            let base = dominant_texture(textures);
            let base_corner = textures.iter().position(|&t| t == base).unwrap_or(0);
            let base_cell = options
                .atlas
                .cell_uv(base, variations[base_corner], options.sheet)
                .to_array();
            mesh.ground
                .quad(tile_vertices(positions, [base_cell; 4], [[1.0; 4]; 4]));

            // Other textures: blended on top, weight 1 at the corners that use them.
            let mut seen = vec![base];
            for (corner, &texture) in textures.iter().enumerate() {
                if seen.contains(&texture) {
                    continue;
                }
                seen.push(texture);
                let cell = options
                    .atlas
                    .cell_uv(texture, variations[corner], options.sheet)
                    .to_array();
                let weights = textures.map(|t| if t == texture { 1.0 } else { 0.0 });
                let colors = weights.map(|w| [1.0, 1.0, 1.0, w]);
                mesh.overlays
                    .quad(tile_vertices(positions, [cell; 4], colors));
            }

            if points.iter().all(|p| p.water) {
                let level = points.iter().map(|p| water_height(p)).sum::<f32>() / 4.0;
                let flat = positions.map(|p| [p[0], p[1], level]);
                mesh.water
                    .quad(tile_vertices(flat, [[0.0; 4]; 4], [WATER_COLOR; 4]));
            }
        }
    }

    for y in 0..height {
        for x in 0..width - 1 {
            add_cliff_wall(
                &mut mesh.cliffs,
                point(x, y),
                point(x + 1, y),
                position(x, y),
                position(x + 1, y),
            );
        }
    }
    for x in 0..width {
        for y in 0..height - 1 {
            add_cliff_wall(
                &mut mesh.cliffs,
                point(x, y),
                point(x, y + 1),
                position(x, y),
                position(x, y + 1),
            );
        }
    }

    Ok(mesh)
}

/// The texture with the most corners. Ties go to the one met first in corner order.
fn dominant_texture(textures: [u8; 4]) -> u8 {
    let mut best = textures[0];
    let mut best_count = 0;
    for &texture in &textures {
        let count = textures.iter().filter(|&&t| t == texture).count();
        if count > best_count {
            best_count = count;
            best = texture;
        }
    }
    best
}

fn tile_vertices(
    positions: [[f32; 3]; 4],
    cells: [[f32; 4]; 4],
    colors: [[f32; 4]; 4],
) -> [TerrainVertex; 4] {
    std::array::from_fn(|k| TerrainVertex {
        position: positions[k],
        uv: CORNER_UV[k],
        cell: cells[k],
        color: colors[k],
    })
}

/// Vertical wall on the edge between two points whose cliff layers differ.
fn add_cliff_wall(walls: &mut Mesh, a: &TilePoint, b: &TilePoint, pa: [f32; 3], pb: [f32; 3]) {
    if a.layer_height == b.layer_height || a.ramp || b.ramp {
        return;
    }
    let low = pa[2].min(pb[2]);
    let high = pa[2].max(pb[2]);
    if high <= low {
        return;
    }
    let corners = [
        [pa[0], pa[1], low],
        [pb[0], pb[1], low],
        [pb[0], pb[1], high],
        [pa[0], pa[1], high],
    ];
    walls.quad(tile_vertices(corners, [[0.0; 4]; 4], [CLIFF_COLOR; 4]));
}

/// Errors while loading a map for the viewer.
#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("map archive: {0}")]
    Archive(#[from] mpq::Error),
    #[error("war3map.w3e: {0}")]
    Terrain(#[from] w3map::Error),
    #[error("terrain: {0}")]
    Mesh(#[from] MeshError),
}

/// Terrain ready to upload: mesh, atlas and bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainScene {
    pub mesh: TerrainMesh,
    pub atlas: Atlas,
    pub bounds: Aabb,
    /// Ground textures that were read from the map's archive (0 means procedural only).
    pub textures_loaded: usize,
}

/// Archive path of the terrain table that names the ground textures.
const TERRAIN_SLK: &str = "TerrainArt\\Terrain.slk";

impl TerrainScene {
    /// Meshes a parsed terrain. `textures[slot]` is the sheet for ground slot `slot`, if known.
    pub fn from_terrain(
        terrain: &W3eTerrain,
        textures: &[Option<TextureData>],
        options: MeshOptions,
    ) -> Result<TerrainScene, MeshError> {
        let mesh = build_mesh(terrain, options)?;
        let refs: Vec<Option<&TextureData>> = textures.iter().map(Option::as_ref).collect();
        let atlas = build_atlas(options.atlas, options.sheet, &refs);
        let bounds = mesh.bounds().ok_or(MeshError::TooSmall {
            width: terrain.width,
            height: terrain.height,
        })?;
        let textures_loaded = textures.iter().filter(|t| t.is_some()).count();
        Ok(TerrainScene {
            mesh,
            atlas,
            bounds,
            textures_loaded,
        })
    }

    /// Reads `war3map.w3e` from a map archive (`.w3x`) and meshes it. Ground textures are looked
    /// up through `TerrainArt\Terrain.slk`; any step that fails falls back to the procedural
    /// sheets.
    pub fn from_map(map: &[u8]) -> Result<TerrainScene, MapError> {
        let archive = mpq::Archive::open(map)?;
        let terrain = W3eTerrain::parse(&archive.read("war3map.w3e")?)?;
        let textures = load_ground_textures(&archive, &terrain);
        Ok(TerrainScene::from_terrain(
            &terrain,
            &textures,
            MeshOptions::default(),
        )?)
    }
}

/// Best effort: `TerrainArt\Terrain.slk` row of each ground tileset id, then its `dir` and `file`
/// columns as `dir\file.blp`. UNVERIFIED against real files (column names are assumed).
fn load_ground_textures(
    archive: &mpq::Archive<'_>,
    terrain: &W3eTerrain,
) -> Vec<Option<TextureData>> {
    let slk = archive
        .read(TERRAIN_SLK)
        .ok()
        .and_then(|bytes| gamedata::Slk::parse_bytes(&bytes).ok());
    (0..GROUND_SLOTS)
        .map(|slot| {
            let id = terrain.ground_tilesets.get(slot)?;
            let key = String::from_utf8_lossy(id)
                .trim_end_matches('\0')
                .to_string();
            let row = slk.as_ref()?.row(&key)?;
            let dir = row.raw("dir")?;
            let file = row.raw("file")?;
            let bytes = archive.read(&format!("{dir}\\{file}.blp")).ok()?;
            let blp = blp::Blp::parse(&bytes).ok()?;
            TextureData::from_blp(&blp).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One tile point for a synthetic `war3map.w3e`.
    #[derive(Clone, Copy)]
    struct P {
        height: u16,
        water_level: u16,
        texture: u8,
        variation: u8,
        layer: u8,
        flags: u8,
    }

    impl Default for P {
        fn default() -> Self {
            P {
                height: 8192,
                water_level: 8192,
                texture: 0,
                variation: 0,
                layer: 0,
                flags: 0,
            }
        }
    }

    /// Encodes a version 11 `war3map.w3e` with one ground tileset "Ldrt" and offset (-256, -128).
    fn w3e_bytes(width: u32, height: u32, points: &[P]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"W3E!");
        out.extend_from_slice(&11u32.to_le_bytes());
        out.push(b'L'); // main tileset
        out.extend_from_slice(&0u32.to_le_bytes()); // custom tileset flag
        out.extend_from_slice(&1u32.to_le_bytes()); // ground tileset count
        out.extend_from_slice(b"Ldrt");
        out.extend_from_slice(&0u32.to_le_bytes()); // cliff tileset count
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&(-256.0f32).to_le_bytes());
        out.extend_from_slice(&(-128.0f32).to_le_bytes());
        for p in points {
            out.extend_from_slice(&p.height.to_le_bytes());
            out.extend_from_slice(&p.water_level.to_le_bytes());
            out.push(p.texture | p.flags);
            out.push(p.variation);
            out.push(p.layer);
        }
        out
    }

    fn terrain(width: u32, height: u32, points: &[P]) -> W3eTerrain {
        W3eTerrain::parse(&w3e_bytes(width, height, points)).expect("synthetic w3e parses")
    }

    fn flat(width: u32, height: u32) -> Vec<P> {
        vec![P::default(); (width * height) as usize]
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn flat_map_has_one_quad_per_tile() {
        let t = terrain(3, 3, &flat(3, 3));
        let mesh = build_mesh(&t, MeshOptions::default()).unwrap();
        assert_eq!(mesh.tiles, (2, 2));
        assert_eq!(mesh.ground.quad_count(), 4);
        assert_eq!(mesh.ground.vertices.len(), 16);
        assert_eq!(mesh.ground.indices.len(), 24);
        assert!(mesh.overlays.is_empty());
        assert!(mesh.cliffs.is_empty());
        assert!(mesh.water.is_empty());
        // Raw 8192 is height zero, and the tiles start at the file offset.
        for v in &mesh.ground.vertices {
            assert!(approx(v.position[2], 0.0));
        }
        assert!(approx(mesh.ground.vertices[0].position[0], -256.0));
        assert!(approx(mesh.ground.vertices[0].position[1], -128.0));
        let far = mesh
            .ground
            .vertices
            .iter()
            .map(|v| v.position[0])
            .fold(f32::MIN, f32::max);
        assert!(approx(far, -256.0 + 2.0 * TILE_SIZE));
    }

    #[test]
    fn ground_height_formula() {
        let point = |raw: u16, layer: u8| TilePoint {
            ground_height: raw as i16,
            water_level: 0,
            map_edge: false,
            ground_texture: 0,
            ramp: false,
            blight: false,
            water: false,
            boundary: false,
            ground_variation: 0,
            cliff_variation: 0,
            cliff_texture: 0,
            layer_height: layer,
        };
        assert!(approx(ground_height(&point(8192, 0)), 0.0));
        assert!(approx(ground_height(&point(8192 + 512, 0)), TILE_SIZE));
        assert!(approx(ground_height(&point(8192 - 512, 0)), -TILE_SIZE));
        assert!(approx(
            ground_height(&point(8192, 2)),
            2.0 * CLIFF_LAYER_HEIGHT
        ));
    }

    #[test]
    fn slope_follows_the_heights() {
        // Height rises by 512 raw (128 world units) per point along x.
        let w = 4;
        let mut points = flat(w, 2);
        for (i, p) in points.iter_mut().enumerate() {
            p.height = (8192 + 512 * (i as u32 % w)) as u16;
        }
        let mesh = build_mesh(&terrain(w, 2, &points), MeshOptions::default()).unwrap();
        // The first tile has corners at x = 0 and x = 1: heights 0, 128, 128, 0.
        let first_tile: Vec<f32> = mesh.ground.vertices[..4]
            .iter()
            .map(|v| v.position[2])
            .collect();
        assert!(approx(first_tile[0], 0.0));
        assert!(approx(first_tile[1], TILE_SIZE));
        assert!(approx(first_tile[2], TILE_SIZE));
        assert!(approx(first_tile[3], 0.0));
        // Heights run 0..=3 tiles along x (four points), so the top is 3 * 128.
        let bounds = mesh.bounds().unwrap();
        assert!(approx(bounds.max.z, 3.0 * TILE_SIZE));
    }

    #[test]
    fn dominant_texture_and_overlay_weights() {
        // Corners in order (0,0), (1,0), (1,1), (0,1): textures 3, 5, 5, 3.
        // Row-major indices: corner (1,1) is index 3 and corner (0,1) is index 2.
        let mut points = flat(2, 2);
        points[0].texture = 3;
        points[1].texture = 5;
        points[3].texture = 5;
        points[2].texture = 3;
        let t = terrain(2, 2, &points);
        let options = MeshOptions::default();
        let mesh = build_mesh(&t, options).unwrap();
        assert_eq!(mesh.ground.quad_count(), 1);
        assert_eq!(mesh.overlays.quad_count(), 1);
        // Tie: texture 3 is met first, so it is the base.
        let base = options.atlas.cell_uv(3, 0, options.sheet).to_array();
        assert_eq!(mesh.ground.vertices[0].cell, base);
        let overlay_cell = options.atlas.cell_uv(5, 0, options.sheet).to_array();
        let weights: Vec<f32> = mesh.overlays.vertices.iter().map(|v| v.color[3]).collect();
        assert_eq!(weights, vec![0.0, 1.0, 1.0, 0.0]);
        assert!(mesh.overlays.vertices.iter().all(|v| v.cell == overlay_cell));
        // Base is opaque everywhere.
        assert!(mesh.ground.vertices.iter().all(|v| v.color[3] == 1.0));
    }

    #[test]
    fn variation_selects_the_cell_of_the_base_corner() {
        let mut points = flat(2, 2);
        for p in &mut points {
            p.texture = 1;
            p.variation = 5;
        }
        let options = MeshOptions::default();
        let mesh = build_mesh(&terrain(2, 2, &points), options).unwrap();
        assert!(mesh.overlays.is_empty());
        let expected = options.atlas.cell_uv(1, 5, options.sheet).to_array();
        assert!(mesh.ground.vertices.iter().all(|v| v.cell == expected));
        // Local UVs cover the whole tile.
        assert_eq!(mesh.ground.vertices[0].uv, [0.0, 0.0]);
        assert_eq!(mesh.ground.vertices[2].uv, [1.0, 1.0]);
    }

    #[test]
    fn water_only_where_all_corners_are_flagged() {
        // 3x2 points. Tile 0 (left) has all four corners flagged; tile 1 has only three.
        let mut points = flat(3, 2);
        for &i in &[0usize, 1, 3, 4, 2, 5] {
            points[i].flags = 0x40;
            points[i].water_level = 8192 + 512; // +128 world units
        }
        points[2].flags = 0;
        let mesh = build_mesh(&terrain(3, 2, &points), MeshOptions::default()).unwrap();
        assert_eq!(mesh.water.quad_count(), 1);
        for v in &mesh.water.vertices {
            assert!(approx(v.position[2], TILE_SIZE));
            assert_eq!(v.color, WATER_COLOR);
        }
    }

    #[test]
    fn cliff_walls_on_layer_edges_only() {
        // 3x2 points; the middle column is one cliff layer higher.
        let mut points = flat(3, 2);
        for y in 0..2 {
            points[y * 3 + 1].layer = 1;
        }
        let mesh = build_mesh(&terrain(3, 2, &points), MeshOptions::default()).unwrap();
        // Horizontal edges across the layer change: 2 on each side = 4 walls.
        assert_eq!(mesh.cliffs.quad_count(), 4);
        for v in &mesh.cliffs.vertices {
            assert_eq!(v.color, CLIFF_COLOR);
        }
        // Each wall spans exactly one layer (128 world units).
        for quad in mesh.cliffs.vertices.chunks(4) {
            let low = quad[0].position[2].min(quad[3].position[2]);
            let high = quad[0].position[2].max(quad[3].position[2]);
            assert!(approx(high - low, CLIFF_LAYER_HEIGHT));
        }
    }

    #[test]
    fn ramps_get_no_cliff_walls() {
        let mut points = flat(3, 2);
        for y in 0..2 {
            points[y * 3 + 1].layer = 1;
            points[y * 3 + 1].flags = 0x10; // ramp
        }
        let mesh = build_mesh(&terrain(3, 2, &points), MeshOptions::default()).unwrap();
        assert!(mesh.cliffs.is_empty());
    }

    #[test]
    fn too_small_is_an_error() {
        let t = terrain(1, 3, &flat(1, 3));
        assert_eq!(
            build_mesh(&t, MeshOptions::default()),
            Err(MeshError::TooSmall {
                width: 1,
                height: 3
            })
        );
    }

    #[test]
    fn sheet_layout_cells_and_wrap() {
        assert_eq!(SheetLayout::GRID_4X4.cell(0), (0, 0));
        assert_eq!(SheetLayout::GRID_4X4.cell(5), (1, 1));
        assert_eq!(SheetLayout::GRID_4X4.cell(17), (1, 0)); // wraps past 16 cells
        assert_eq!(SheetLayout::GRID_8X4.cell(31), (7, 3));
    }

    #[test]
    fn atlas_cells_are_disjoint_and_inside_their_sheet() {
        let layout = AtlasLayout { sheet_px: 64 };
        let sheet = SheetLayout::GRID_4X4;
        let mut seen = Vec::new();
        for texture in 0..16u8 {
            for variation in 0..16u8 {
                let r = layout.cell_uv(texture, variation, sheet);
                assert!(r.min[0] > 0.0 && r.min[1] > 0.0);
                assert!(r.min[0] + r.size[0] < 1.0 && r.min[1] + r.size[1] < 1.0);
                // Inside the sheet of this texture (4x4 sheets in the atlas).
                let sx = (texture % 4) as f32 / 4.0;
                let sy = (texture / 4) as f32 / 4.0;
                assert!(r.min[0] >= sx && r.min[0] + r.size[0] <= sx + 0.25 + 1e-6);
                assert!(r.min[1] >= sy && r.min[1] + r.size[1] <= sy + 0.25 + 1e-6);
                seen.push(r.to_array().map(|v| v.to_bits()));
            }
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 256, "every (texture, variation) has its own cell");
    }

    #[test]
    fn procedural_atlas_has_every_slot_filled() {
        let layout = AtlasLayout { sheet_px: 32 };
        let atlas = build_atlas(layout, SheetLayout::GRID_8X4, &[]);
        assert_eq!(atlas.size, 128);
        assert_eq!(atlas.rgba.len(), 128 * 128 * 4);
        assert!(atlas.rgba.chunks(4).all(|px| px[3] == 255));
        // Different slots have different average colours.
        let average = |slot: u32| {
            let (sx, sy) = (slot % 4, slot / 4);
            let mut sum = [0u64; 3];
            for py in 0..32 {
                for px in 0..32 {
                    let i = (((sy * 32 + py) * 128 + sx * 32 + px) * 4) as usize;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += u64::from(atlas.rgba[i + c]);
                    }
                }
            }
            sum
        };
        assert_ne!(average(0), average(1));
        assert_ne!(average(0), average(15));
    }

    #[test]
    fn supplied_texture_is_resampled_into_its_slot() {
        let layout = AtlasLayout { sheet_px: 4 };
        // A 2x2 red texture fills slot 2 completely.
        let red = TextureData::from_rgba(2, 2, [255, 0, 0, 255].repeat(4)).unwrap();
        let mut sources: Vec<Option<&TextureData>> = vec![None; GROUND_SLOTS];
        sources[2] = Some(&red);
        let atlas = build_atlas(layout, SheetLayout::GRID_4X4, &sources);
        // Slot 2 is at sheet column 2, row 0: pixels x in 8..12, y in 0..4.
        let i = ((16 + 9) * 4) as usize;
        assert_eq!(&atlas.rgba[i..i + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn terrain_scene_from_parsed_bytes() {
        let t = terrain(2, 2, &flat(2, 2));
        let scene = TerrainScene::from_terrain(&t, &[], MeshOptions::default()).unwrap();
        assert_eq!(scene.textures_loaded, 0);
        assert_eq!(scene.atlas.size, 4 * 256);
        assert!(approx(scene.bounds.min.x, -256.0));
        assert!(approx(scene.bounds.max.x, -256.0 + TILE_SIZE));
    }

    #[test]
    fn garbage_map_is_an_error_not_a_panic() {
        assert!(TerrainScene::from_map(&[]).is_err());
        assert!(TerrainScene::from_map(b"not an archive at all, just text").is_err());
    }
}
