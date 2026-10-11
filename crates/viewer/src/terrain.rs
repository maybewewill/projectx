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

impl SheetLayout {
    /// Grid of a ground texture with the given pixel size. Ground textures have 4 rows of
    /// square variation cells, and the column count follows the aspect ratio: 4 columns for a
    /// square texture (256x256), 8 for 512x256 (the common WC3 size). Clamped to 1..=8.
    pub fn for_size(width: u32, height: u32) -> SheetLayout {
        let height = height.max(1);
        let cols = ((4.0 * width as f32 / height as f32).round() as u32).clamp(1, 8);
        SheetLayout { cols, rows: 4 }
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

/// Where one ground slot lives in the atlas: the top-left `width x height` pixels of its
/// `sheet_px` square, cut into `sheet` cells. A 512x256 texture uses 256x128 of its slot, so its
/// cells stay square.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotLayout {
    pub sheet: SheetLayout,
    pub width: u32,
    pub height: u32,
}

/// The atlas is a square of `SHEET_GRID x SHEET_GRID` slots, each `sheet_px` pixels square.
/// Every slot has its own layout, so textures of different shapes can share one atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasLayout {
    pub sheet_px: u32,
    pub slots: [SlotLayout; GROUND_SLOTS],
}

impl AtlasLayout {
    /// Layout for `sheet_px` slots with the default 4x4 square cell grid in every slot (what
    /// the procedural sheets use).
    pub fn square(sheet_px: u32) -> AtlasLayout {
        AtlasLayout {
            sheet_px,
            slots: [SlotLayout {
                sheet: SheetLayout::GRID_4X4,
                width: sheet_px,
                height: sheet_px,
            }; GROUND_SLOTS],
        }
    }

    /// Layout that fits each supplied texture into its slot, keeping its aspect ratio. Slots
    /// without a texture keep the procedural square layout.
    pub fn for_textures(sheet_px: u32, sources: &[Option<&TextureData>]) -> AtlasLayout {
        let mut layout = AtlasLayout::square(sheet_px);
        for (slot, source) in sources.iter().take(GROUND_SLOTS).enumerate() {
            let Some(texture) = source else {
                continue;
            };
            let (w, h) = (texture.width.max(1), texture.height.max(1));
            let (width, height) = if w >= h {
                (sheet_px, (sheet_px as u64 * u64::from(h) / u64::from(w)).max(1) as u32)
            } else {
                ((sheet_px as u64 * u64::from(w) / u64::from(h)).max(1) as u32, sheet_px)
            };
            layout.slots[slot] = SlotLayout {
                sheet: SheetLayout::for_size(w, h),
                width,
                height,
            };
        }
        layout
    }

    /// Side of the atlas in pixels.
    pub fn size(self) -> u32 {
        self.sheet_px * SHEET_GRID
    }

    /// UV rectangle of one variation of one ground texture.
    pub fn cell_uv(self, texture: u8, variation: u8) -> UvRect {
        let slot = u32::from(texture) % (SHEET_GRID * SHEET_GRID);
        let (slot_x, slot_y) = (slot % SHEET_GRID, slot / SHEET_GRID);
        let layout = self.slots[slot as usize];
        let (cell_x, cell_y) = layout.sheet.cell(variation);
        let cell_w = layout.width as f32 / layout.sheet.cols as f32;
        let cell_h = layout.height as f32 / layout.sheet.rows as f32;
        let sheet_px = self.sheet_px as f32;
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
        AtlasLayout::square(256)
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
pub fn build_atlas(layout: AtlasLayout, sources: &[Option<&TextureData>]) -> Atlas {
    let size = layout.size();
    let s = layout.sheet_px;
    let mut rgba = vec![0u8; (size as usize) * (size as usize) * 4];
    for slot in 0..GROUND_SLOTS as u32 {
        let (slot_x, slot_y) = (slot % SHEET_GRID, slot / SHEET_GRID);
        let source = sources.get(slot as usize).copied().flatten();
        let slot_layout = layout.slots[slot as usize];
        let fill = source.map(opaque_average);
        for py in 0..slot_layout.height {
            for px in 0..slot_layout.width {
                let color = match (source, fill) {
                    (Some(texture), Some(fill)) => {
                        bake_alpha(sample_area(texture, px, py, slot_layout), fill)
                    }
                    _ => procedural_texel(slot, slot_layout.sheet, s, px, py),
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

/// One texel of a slot filled from `texture`: the average of the source texels that cover it
/// when shrinking (box filter, so a 512-pixel texture does not alias into a 256 slot), and the
/// nearest texel when enlarging.
fn sample_area(texture: &TextureData, px: u32, py: u32, slot: SlotLayout) -> [u8; 4] {
    let (tw, th) = (texture.width.max(1), texture.height.max(1));
    let x0 = u64::from(px) * u64::from(tw) / u64::from(slot.width);
    let x1 = ((u64::from(px) + 1) * u64::from(tw) / u64::from(slot.width)).max(x0 + 1);
    let y0 = u64::from(py) * u64::from(th) / u64::from(slot.height);
    let y1 = ((u64::from(py) + 1) * u64::from(th) / u64::from(slot.height)).max(y0 + 1);
    let mut sum = [0u64; 4];
    let mut count = 0u64;
    for sy in y0..y1.min(u64::from(th)) {
        for sx in x0..x1.min(u64::from(tw)) {
            let index = (sy as usize * tw as usize + sx as usize) * 4;
            for (c, total) in sum.iter_mut().enumerate() {
                *total += u64::from(texture.rgba[index + c]);
            }
            count += 1;
        }
    }
    let count = count.max(1);
    std::array::from_fn(|c| ((sum[c] + count / 2) / count) as u8)
}

/// Mean colour of the opaque texels (alpha at least 128) of a texture; the mean of all texels
/// when none is opaque. Used to fill the transparent parts of a texture.
fn opaque_average(texture: &TextureData) -> [u8; 3] {
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    let mut all = [0u64; 3];
    let mut all_count = 0u64;
    for px in texture.rgba.chunks_exact(4) {
        for c in 0..3 {
            all[c] += u64::from(px[c]);
        }
        all_count += 1;
        if px[3] >= 128 {
            for c in 0..3 {
                sum[c] += u64::from(px[c]);
            }
            count += 1;
        }
    }
    let (sum, count) = if count > 0 { (sum, count) } else { (all, all_count.max(1)) };
    sum.map(|s| ((s + count / 2) / count.max(1)) as u8)
}

/// Blends a texel over the texture's fill colour by its alpha, and keeps the alpha.
///
/// Ground textures carry transition masks: texels with alpha 0 hold black, and the original
/// draws them over the neighbouring ground. The opaque ground pass has nothing to show through,
/// so those texels take the fill colour instead. The overlay pass keeps the alpha, so the
/// transition still fades in over the ground.
fn bake_alpha(texel: [u8; 4], fill: [u8; 3]) -> [u8; 4] {
    let a = u32::from(texel[3]);
    let mix = |c: u8, f: u8| ((u32::from(c) * a + u32::from(f) * (255 - a) + 127) / 255) as u8;
    [
        mix(texel[0], fill[0]),
        mix(texel[1], fill[1]),
        mix(texel[2], fill[2]),
        texel[3],
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
                .cell_uv(base, variations[base_corner])
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
                    .cell_uv(texture, variations[corner])
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
    /// Meshes a parsed terrain. `textures[slot]` is the texture for ground slot `slot`, if known.
    /// Each texture keeps its own cell grid (see [`AtlasLayout::for_textures`]).
    pub fn from_terrain(
        terrain: &W3eTerrain,
        textures: &[Option<TextureData>],
    ) -> Result<TerrainScene, MeshError> {
        let refs: Vec<Option<&TextureData>> = textures.iter().map(Option::as_ref).collect();
        let options = MeshOptions {
            atlas: AtlasLayout::for_textures(DEFAULT_SHEET_PX, &refs),
        };
        let mesh = build_mesh(terrain, options)?;
        let atlas = build_atlas(options.atlas, &refs);
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

    /// Reads `war3map.w3e` from a map archive (`.w3x`) and meshes it, with procedural ground
    /// sheets.
    pub fn from_map(map: &[u8]) -> Result<TerrainScene, MapError> {
        TerrainScene::from_map_with_sources(map, &[])
    }

    /// Like [`TerrainScene::from_map`], with ground textures read from `sources`: the game
    /// archives, in load order (later archives override earlier ones, as in the game). The
    /// tileset table `TerrainArt\Terrain.slk` is merged over all of them, then each ground
    /// slot's `dir\file.blp` is looked up from the last archive that has it. A slot that cannot
    /// be read falls back to its procedural sheet.
    pub fn from_map_with_sources(
        map: &[u8],
        sources: &[&[u8]],
    ) -> Result<TerrainScene, MapError> {
        let archive = mpq::Archive::open(map)?;
        let terrain = W3eTerrain::parse(&archive.read("war3map.w3e")?)?;
        let archives: Vec<mpq::Archive<'_>> = sources
            .iter()
            .filter_map(|bytes| mpq::Archive::open(bytes).ok())
            .collect();
        let textures = load_ground_textures(&archives, &terrain);
        Ok(TerrainScene::from_terrain(&terrain, &textures)?)
    }
}

/// Atlas sheet size in pixels, per slot.
pub const DEFAULT_SHEET_PX: u32 = 256;

/// Reads `name` from the last archive that has it.
fn read_from_archives(archives: &[mpq::Archive<'_>], name: &str) -> Option<Vec<u8>> {
    archives.iter().rev().find_map(|archive| archive.read(name).ok())
}

/// `TerrainArt\Terrain.slk` merged over every archive, in load order. Later archives replace
/// the cells they define; the expansion and patch files carry the Ruins and Fall tilesets with
/// their own column layout, which the column names account for.
fn merged_terrain_slk(archives: &[mpq::Archive<'_>]) -> Option<gamedata::Slk> {
    let mut merged: Option<gamedata::Slk> = None;
    for archive in archives {
        let Ok(bytes) = archive.read(TERRAIN_SLK) else {
            continue;
        };
        let Ok(slk) = gamedata::Slk::parse_bytes(&bytes) else {
            continue;
        };
        match merged.as_mut() {
            Some(base) => base.merge(&slk),
            None => merged = Some(slk),
        }
    }
    merged
}

/// Best effort: `TerrainArt\Terrain.slk` row of each ground tileset id, then its `dir` and `file`
/// columns as `dir\file.blp`. Missing or unreadable entries give `None` (procedural slot).
fn load_ground_textures(
    archives: &[mpq::Archive<'_>],
    terrain: &W3eTerrain,
) -> Vec<Option<TextureData>> {
    let slk = merged_terrain_slk(archives);
    (0..GROUND_SLOTS)
        .map(|slot| {
            let id = terrain.ground_tilesets.get(slot)?;
            let key = String::from_utf8_lossy(id)
                .trim_end_matches('\0')
                .to_string();
            let slk = slk.as_ref()?;
            // Text cells are stored quoted in the SLK; `as_str` gives the bare text.
            let dir = slk.get(&key, "dir")?.as_str()?;
            let file = slk.get(&key, "file")?.as_str()?;
            let bytes = read_from_archives(archives, &format!("{dir}\\{file}.blp"))?;
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
        let base = options.atlas.cell_uv(3, 0).to_array();
        assert_eq!(mesh.ground.vertices[0].cell, base);
        let overlay_cell = options.atlas.cell_uv(5, 0).to_array();
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
        let expected = options.atlas.cell_uv(1, 5).to_array();
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
        let layout = AtlasLayout::square(64);
        let mut seen = Vec::new();
        for texture in 0..16u8 {
            for variation in 0..16u8 {
                let r = layout.cell_uv(texture, variation);
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
        let layout = AtlasLayout::square(32);
        let atlas = build_atlas(layout, &[]);
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
        // A 2x2 red texture fills slot 2 completely.
        let red = TextureData::from_rgba(2, 2, [255, 0, 0, 255].repeat(4)).unwrap();
        let mut sources: Vec<Option<&TextureData>> = vec![None; GROUND_SLOTS];
        sources[2] = Some(&red);
        let layout = AtlasLayout::for_textures(4, &sources);
        let atlas = build_atlas(layout, &sources);
        // Slot 2 is at sheet column 2, row 0: pixels x in 8..12, y in 0..4.
        let i = ((16 + 9) * 4) as usize;
        assert_eq!(&atlas.rgba[i..i + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn wide_texture_keeps_square_cells_and_its_colours() {
        // A 512x256 texture: left half blue, right half green, 8x4 cells of 64 px.
        let (w, h) = (512u32, 256u32);
        let mut rgba = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(if x < w / 2 { &[0, 0, 255, 255] } else { &[0, 255, 0, 255] });
            }
        }
        let wide = TextureData::from_rgba(w, h, rgba).unwrap();
        let mut sources: Vec<Option<&TextureData>> = vec![None; GROUND_SLOTS];
        sources[0] = Some(&wide);
        let layout = AtlasLayout::for_textures(64, &sources);
        assert_eq!(layout.slots[0].sheet, SheetLayout { cols: 8, rows: 4 });
        assert_eq!((layout.slots[0].width, layout.slots[0].height), (64, 32));
        // Cells are 8x8 px in a 64x32 slot: square.
        let cell = layout.cell_uv(0, 0);
        assert!((cell.size[0] - cell.size[1]).abs() < 1e-6);
        let atlas = build_atlas(layout, &sources);
        // Left-most texel is blue, right-most of the slot is green, and the lower half of the
        // slot's square is unused (transparent).
        assert_eq!(&atlas.rgba[0..4], &[0, 0, 255, 255]);
        let right = (60 * 4) as usize;
        assert_eq!(&atlas.rgba[right..right + 4], &[0, 255, 0, 255]);
        let below = (((40 * atlas.size) + 2) * 4) as usize;
        assert_eq!(&atlas.rgba[below..below + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn transparent_texels_take_the_fill_colour_and_keep_their_alpha() {
        // Opaque texels are green, transparent ones black (the transition-mask convention).
        let mut rgba = Vec::new();
        for i in 0..4 {
            if i % 2 == 0 {
                rgba.extend_from_slice(&[0, 200, 0, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
        let texture = TextureData::from_rgba(4, 1, rgba).unwrap();
        assert_eq!(opaque_average(&texture), [0, 200, 0]);
        // Fully transparent: fill colour, alpha 0. Opaque: unchanged.
        assert_eq!(bake_alpha([0, 0, 0, 0], [0, 200, 0]), [0, 200, 0, 0]);
        assert_eq!(bake_alpha([10, 20, 30, 255], [0, 200, 0]), [10, 20, 30, 255]);
        // Half transparent: halfway between the texel and the fill colour.
        let half = bake_alpha([0, 0, 0, 128], [200, 200, 200]);
        assert!((i32::from(half[0]) - 100).abs() <= 1 && half[3] == 128);
    }

    #[test]
    fn overlay_layers_keep_the_texel_alpha_for_blending() {
        // A transparent texel in a blended cell must not paint black: the atlas bakes the fill
        // colour into it, and the alpha stays 0 for the overlay pass.
        let texture = TextureData::from_rgba(2, 2, vec![0, 0, 0, 0, 90, 90, 90, 255, 90, 90, 90, 255, 90, 90, 90, 255]).unwrap();
        let mut sources: Vec<Option<&TextureData>> = vec![None; GROUND_SLOTS];
        sources[0] = Some(&texture);
        let layout = AtlasLayout::for_textures(4, &sources);
        let atlas = build_atlas(layout, &sources);
        assert_eq!(&atlas.rgba[0..4], &[90, 90, 90, 0]);
        // Atlas column 2 maps to texel column 1, which is opaque.
        assert_eq!(&atlas.rgba[2 * 4..2 * 4 + 4], &[90, 90, 90, 255]);
    }

    #[test]
    fn box_filter_averages_when_shrinking() {
        // 4x1 texture black, white, black, white into a 2x1 slot: each texel is a 2-pixel average.
        let rgba = [0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255].to_vec();
        let texture = TextureData::from_rgba(4, 1, rgba).unwrap();
        let slot = SlotLayout {
            sheet: SheetLayout::GRID_4X4,
            width: 2,
            height: 1,
        };
        assert_eq!(sample_area(&texture, 0, 0, slot), [128, 128, 128, 255]);
        assert_eq!(sample_area(&texture, 1, 0, slot), [128, 128, 128, 255]);
    }

    #[test]
    fn terrain_scene_from_parsed_bytes() {
        let t = terrain(2, 2, &flat(2, 2));
        let scene = TerrainScene::from_terrain(&t, &[]).unwrap();
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

    #[test]
    fn terrain_slk_text_cells_are_read_unquoted() {
        // Two layouts of the same table, as the base and the expansion files store them: the
        // `dir` column is X2 in one and X3 in the other. Columns are found by name.
        let base = b"ID;PWXL;N;E\nC;X1;K\"tileID\"\nC;X2;K\"dir\"\nC;X3;K\"file\"\nC;X1;Y2;K\"Ldrt\"\nC;X2;K\"TerrainArt\\LordaeronSummer\"\nC;X3;K\"Lords_Dirt\"\nE\n";
        let patch = b"ID;PWXL;N;E\nC;X1;Y1;K\"tileID\"\nC;X3;K\"dir\"\nC;X4;K\"file\"\nC;X1;Y100;K\"Zdrt\"\nC;X2;K-1\nC;X3;K\"TerrainArt\\Ruins\"\nC;X4;K\"Ruins_Dirt\"\nE\n";
        let mut slk = gamedata::Slk::parse_bytes(base).unwrap();
        slk.merge(&gamedata::Slk::parse_bytes(patch).unwrap());
        let text = |key: &str, column: &str| {
            slk.get(key, column)
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        };
        assert_eq!(text("Ldrt", "dir").as_deref(), Some("TerrainArt\\LordaeronSummer"));
        assert_eq!(text("Ldrt", "file").as_deref(), Some("Lords_Dirt"));
        assert_eq!(text("Zdrt", "dir").as_deref(), Some("TerrainArt\\Ruins"));
        assert_eq!(text("Zdrt", "file").as_deref(), Some("Ruins_Dirt"));
    }
}
