//! Doodads and pre-placed units of a map, drawn offscreen in their bind pose.
//!
//! Pipeline: `war3map.doo` and `war3mapUnits.doo` (from the map) give placements; the type id
//! of each placement resolves to a model path through `Doodads\Doodads.slk`,
//! `Units\destructableData.slk` and `Units\UnitUI.slk` (last archive wins, as in the game);
//! the MDX models are read and their geosets drawn with a software rasterizer into a PNG.
//! Presentation only: no textures are sampled, no animation is applied, and the output is not
//! bit-exact with the game.
//!
//! CLI: `viewer --map MAP --mpq ARCHIVE [--mpq ARCHIVE ...] --objects-png OUT.png`
//! (archives in priority order: later ones override earlier ones).

pub mod png;
pub mod raster;

use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::path::PathBuf;

use gamedata::Slk;
use mdx::Model;

use crate::cliff_render::{lambert, PlacedTriangle};
use crate::model::draw::BlendMode;
use crate::props::{PropBlend, PropBuilder};
use crate::terrain_objects::ground_at;
use raster::{Canvas, View, CLASS_DOODAD, CLASS_UNIT};

/// Archives in priority order: a later archive overrides an earlier one. Names are matched
/// exactly first, then case-insensitively through each archive's `(listfile)` (the game
/// matches names case-insensitively, and SLK paths differ in case from the archive names).
/// The archives are opened once and borrow the bytes they were read from.
pub struct Archives<'a> {
    /// Each opened archive with its lower-cased name to the archive's own spelling.
    archives: Vec<(mpq::Archive<'a>, HashMap<String, String>)>,
}

impl<'a> Archives<'a> {
    /// `files` are the archive bytes in priority order (later ones win). Archives that do not
    /// open are skipped.
    pub fn new(files: &[&'a [u8]]) -> Self {
        let archives = files
            .iter()
            .filter_map(|bytes| mpq::Archive::open(bytes).ok())
            .map(|archive| {
                let names = archive
                    .listfile()
                    .map(|list| list.into_iter().map(|n| (n.to_ascii_lowercase(), n)).collect())
                    .unwrap_or_default();
                (archive, names)
            })
            .collect();
        Self { archives }
    }

    /// Reads a file from the last archive that has it.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let lower = name.to_ascii_lowercase();
        self.archives.iter().rev().find_map(|(archive, names)| {
            archive.read(name).ok().or_else(|| {
                let spelled = names.get(&lower)?;
                archive.read(spelled).ok()
            })
        })
    }
}

/// The model of one type id, from its SLK row.
///
/// Doodads and destructables name a model family, not a file: the file of a placement is
/// `file` + its variation index + `.mdx` (`Doodads\Ruins\Plants\VinyPlant\VinyPlant` with
/// variation 2 is `VinyPlant2.mdx`). The `numVar` column bounds the index; like the game, an
/// index past the last variation uses the last one. Unit types name the file itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelType {
    /// The `file` column, with backslashes and no extension.
    pub file: String,
    /// The `numVar` column (0 when absent: the file is used as it is).
    pub num_var: u32,
    /// The `texFile` column of destructables: the texture of replaceable texture ids 31 and up.
    pub tex_file: Option<String>,
}

impl ModelType {
    /// The archive path of the model for a placement's variation index.
    pub fn path_for(&self, variation: u32) -> String {
        if self.num_var == 0 {
            return self.file.clone();
        }
        let index = variation.min(self.num_var - 1);
        format!("{}{index}", self.file)
    }
}

/// Type id to model, from the SLK tables.
#[derive(Default)]
pub struct Catalog {
    pub doodads: HashMap<[u8; 4], ModelType>,
    pub units: HashMap<[u8; 4], ModelType>,
    pub notes: Vec<String>,
}

fn raw_code(key: &str) -> Option<[u8; 4]> {
    let b = key.as_bytes();
    (b.len() == 4).then(|| [b[0], b[1], b[2], b[3]])
}

fn clean_path(text: &str) -> String {
    text.trim().trim_matches('"').replace('/', "\\")
}

/// Reads the `file` column (and `numVar`, `texFile`) of one SLK table into a type-id map;
/// missing files are notes.
fn load_table(archives: &Archives<'_>, name: &str, out: &mut HashMap<[u8; 4], ModelType>, notes: &mut Vec<String>) {
    let Some(bytes) = archives.read(name) else {
        notes.push(format!("{name} not found in the archives"));
        return;
    };
    match Slk::parse_bytes(&bytes) {
        Ok(slk) => {
            for row in slk.rows() {
                let Some(code) = raw_code(row.key()) else { continue };
                let Some(file) = row.raw("file") else { continue };
                let file = clean_path(file);
                if file.is_empty() {
                    continue;
                }
                let num_var = row.raw("numVar").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
                let tex_file = row.raw("texFile").map(clean_path).filter(|t| !t.is_empty());
                out.insert(code, ModelType { file, num_var, tex_file });
            }
        }
        Err(e) => notes.push(format!("{name}: {e}")),
    }
}

impl Catalog {
    pub fn load(archives: &Archives<'_>) -> Self {
        let mut c = Catalog::default();
        let mut notes = Vec::new();
        load_table(archives, "Doodads\\Doodads.slk", &mut c.doodads, &mut notes);
        load_table(archives, "Units\\destructableData.slk", &mut c.doodads, &mut notes);
        load_table(archives, "Units\\UnitUI.slk", &mut c.units, &mut notes);
        c.notes = notes;
        c
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Doodad,
    Unit,
}

/// One placed object with a resolved model path.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub kind: Kind,
    pub model: String,
    pub position: [f32; 3],
    /// Rotation about the vertical axis, radians.
    pub angle: f32,
    pub scale: [f32; 3],
    /// Owner player slot (units only).
    pub player: Option<u32>,
    /// Texture of replaceable texture ids 31 and up (destructables: their `texFile`).
    pub texture: Option<String>,
    /// Animation sequence name (live sim units only); empty for static bind pose.
    pub sequence_name: String,
    /// Animation time in milliseconds (live sim units only).
    pub animation_time_ms: Option<u32>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stats {
    pub doodads_parsed: usize,
    pub doodads_with_model: usize,
    pub units_parsed: usize,
    pub units_with_model: usize,
    /// Type ids that have no model path in the SLK tables, with their counts (top 12).
    pub unmodelled: Vec<(String, usize)>,
    /// Parse failures and other notes.
    pub notes: Vec<String>,
}

/// Placements of one map, resolved through the catalog.
pub fn placements(map: &[u8], catalog: &Catalog) -> (Vec<Instance>, Stats) {
    let map_archive = mpq::Archive::open(map).ok();
    let read = |name: &str| map_archive.as_ref().and_then(|a| a.read(name).ok());
    let mut instances = Vec::new();
    let mut stats = Stats::default();
    let mut unmodelled: HashMap<String, usize> = HashMap::new();

    if let Some(bytes) = read("war3map.doo") {
        match w3map::Doodads::parse(&bytes) {
            Ok(doo) => {
                stats.doodads_parsed = doo.doodads.len();
                for d in &doo.doodads {
                    match catalog.doodads.get(&d.id) {
                        Some(ty) => {
                            stats.doodads_with_model += 1;
                            instances.push(Instance {
                                kind: Kind::Doodad,
                                model: ty.path_for(d.variation),
                                position: d.position,
                                angle: d.angle,
                                scale: d.scale,
                                player: None,
                                texture: ty.tex_file.clone(),
                                sequence_name: String::new(),
                                animation_time_ms: None,
                            });
                        }
                        None => *unmodelled.entry(code_text(&d.id)).or_insert(0) += 1,
                    }
                }
            }
            Err(e) => stats.notes.push(format!("war3map.doo: {e}")),
        }
    }
    if let Some(bytes) = read("war3mapUnits.doo") {
        match w3map::PlacedUnits::parse(&bytes) {
            Ok(units) => {
                stats.units_parsed = units.units.len();
                for u in &units.units {
                    match catalog.units.get(&u.id) {
                        Some(ty) => {
                            stats.units_with_model += 1;
                            instances.push(Instance {
                                kind: Kind::Unit,
                                model: ty.path_for(u.variation),
                                position: u.position,
                                angle: u.angle,
                                scale: u.scale,
                                player: Some(u.player),
                                texture: None,
                                sequence_name: String::new(),
                                animation_time_ms: None,
                            });
                        }
                        None => *unmodelled.entry(code_text(&u.id)).or_insert(0) += 1,
                    }
                }
            }
            Err(e) => stats.notes.push(format!("war3mapUnits.doo: {e}")),
        }
    }
    let mut top: Vec<(String, usize)> = unmodelled.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(12);
    stats.unmodelled.extend(top);
    (instances, stats)
}

/// Unit animation state for sequence selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimUnitSequenceState {
    /// Unit is moving: play "Walk" sequence or Stand if unavailable.
    Moving,
    /// Unit is attacking or under attack: play "Attack" sequence.
    Attacking,
    /// Unit is under construction: play "Birth" sequence.
    Building,
    /// Unit is dead (life reaches zero): try "Death" then "Decay".
    Dead,
    /// Unit idle (default): play "Stand" sequence.
    Standing,
}

/// A unit of the live simulation, as the viewer draws it (copied out of `sim::Game` by
/// `crate::sim_view`). `type_id` is the raw code as a big-endian integer, as `GetUnitTypeId` gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimUnit {
    pub type_id: u32,
    pub owner: u32,
    pub x: f32,
    pub y: f32,
    /// Facing in radians (the mover's heading).
    pub facing: f32,
    /// Uniform scale; a non-positive value is drawn at 1.0.
    pub scale: f32,
    pub hidden: bool,
    pub dead: bool,
    pub removed: bool,
    /// Unit handle id for deterministic phase offset (sequence time = (game_time * 1000 + handle * 137) % length).
    pub handle: u32,
    /// Sequence name base: "Walk", "Attack", "Birth", "Death", "Decay", "Stand", or empty string.
    pub sequence_state: SimUnitSequenceState,
}

/// What [`sim_unit_instances`] did with the live units.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SimUnitStats {
    /// Units that became draw instances.
    pub drawn: usize,
    /// Hidden, dead or removed units (not drawn).
    pub not_visible: usize,
    /// Type codes with no model in the unit tables, with their counts (sorted by count, then name).
    pub unmodelled: Vec<(String, usize)>,
}

/// Draw instances for the live simulation's units, in the same form as the map's unit placements:
/// the model comes from the unit table by type code (the variation is 0: the simulation has none),
/// the position and facing are the unit's, and the tint is the owner's player colour.
pub fn sim_unit_instances(units: &[SimUnit], catalog: &Catalog, game_time_ms: u32) -> (Vec<Instance>, SimUnitStats) {
    let mut instances = Vec::new();
    let mut stats = SimUnitStats::default();
    let mut unmodelled: HashMap<String, usize> = HashMap::new();
    for u in units {
        if u.hidden || u.dead || u.removed {
            stats.not_visible += 1;
            continue;
        }
        let code = u.type_id.to_be_bytes();
        match catalog.units.get(&code) {
            Some(ty) => {
                let scale = if u.scale > 0.0 { u.scale } else { 1.0 };

                // Choose sequence name based on unit state
                let sequence_name = match u.sequence_state {
                    SimUnitSequenceState::Moving => "Walk",
                    SimUnitSequenceState::Attacking => "Attack",
                    SimUnitSequenceState::Building => "Birth",
                    SimUnitSequenceState::Dead => "Death", // fallback to "Decay" is in the renderer
                    SimUnitSequenceState::Standing => "Stand",
                };

                // Deterministic animation time: (game_time_ms * 1000 + handle * 137) mod sequence_length
                // We pass the game time and handle so the renderer can compute the actual sequence time
                let animation_time_offset = (game_time_ms as u32).wrapping_mul(1000).wrapping_add(u.handle.wrapping_mul(137));

                instances.push(Instance {
                    kind: Kind::Unit,
                    model: ty.path_for(0),
                    position: [u.x, u.y, 0.0],
                    angle: u.facing,
                    scale: [scale; 3],
                    player: Some(u.owner),
                    texture: None,
                    sequence_name: sequence_name.to_string(),
                    animation_time_ms: Some(animation_time_offset),
                });
                stats.drawn += 1;
            }
            None => *unmodelled.entry(code_text(&code)).or_insert(0) += 1,
        }
    }
    let mut top: Vec<(String, usize)> = unmodelled.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    stats.unmodelled = top;
    (instances, stats)
}

fn code_text(id: &[u8; 4]) -> String {
    String::from_utf8_lossy(id).into_owned()
}

#[cfg(test)]
mod sim_unit_tests {
    use super::*;

    fn catalog_with(code: &[u8; 4], file: &str) -> Catalog {
        let mut c = Catalog::default();
        c.units.insert(*code, ModelType { file: file.to_string(), num_var: 0, tex_file: None });
        c
    }

    fn unit(type_id: u32) -> SimUnit {
        SimUnit {
            type_id,
            owner: 2,
            x: 100.0,
            y: -200.5,
            facing: 1.5,
            scale: 1.25,
            hidden: false,
            dead: false,
            removed: false,
            handle: 0x100000,
            sequence_state: SimUnitSequenceState::Standing,
        }
    }

    #[test]
    fn sim_unit_maps_to_a_draw_instance() {
        let hpea = u32::from_be_bytes(*b"hpea");
        let catalog = catalog_with(b"hpea", "units\\human\\Peasant\\Peasant");
        let (inst, stats) = sim_unit_instances(&[unit(hpea)], &catalog, 1000);
        assert_eq!(stats.drawn, 1);
        assert!(stats.unmodelled.is_empty());
        assert_eq!(inst.len(), 1);
        let i = &inst[0];
        assert_eq!(i.kind, Kind::Unit);
        assert_eq!(i.model, "units\\human\\Peasant\\Peasant");
        assert_eq!(i.position, [100.0, -200.5, 0.0]);
        assert_eq!(i.angle, 1.5);
        assert_eq!(i.scale, [1.25, 1.25, 1.25]);
        assert_eq!(i.player, Some(2));
        assert_eq!(i.sequence_name, "Stand");
    }

    #[test]
    fn hidden_dead_removed_units_are_not_drawn() {
        let hpea = u32::from_be_bytes(*b"hpea");
        let catalog = catalog_with(b"hpea", "x");
        let mut hidden = unit(hpea);
        hidden.hidden = true;
        let mut dead = unit(hpea);
        dead.dead = true;
        let mut removed = unit(hpea);
        removed.removed = true;
        let (inst, stats) = sim_unit_instances(&[hidden, dead, removed], &catalog, 1000);
        assert!(inst.is_empty());
        assert_eq!(stats.not_visible, 3);
        assert_eq!(stats.drawn, 0);
    }

    #[test]
    fn unknown_type_is_counted_and_non_positive_scale_becomes_one() {
        let catalog = catalog_with(b"hpea", "x");
        let mut zero_scale = unit(u32::from_be_bytes(*b"hpea"));
        zero_scale.scale = 0.0;
        let other = unit(u32::from_be_bytes(*b"ngol"));
        let (inst, stats) = sim_unit_instances(&[zero_scale, other, other], &catalog, 1000);
        assert_eq!(inst.len(), 1);
        assert_eq!(inst[0].scale, [1.0, 1.0, 1.0]);
        assert_eq!(stats.unmodelled, vec![("ngol".to_string(), 2)]);
    }
}

/// Triangle list of one model, grouped per geoset.
struct Part {
    verts: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tris: Vec<[u16; 3]>,
    tint: [u8; 3],
}

struct Mesh {
    parts: Vec<Part>,
}

const DOODAD_TINTS: [[u8; 3]; 4] = [[92, 128, 70], [120, 104, 74], [70, 110, 96], [140, 124, 96]];

/// The standard colour of player slot `p` (the slots wrap every 12).
pub fn player_colour(p: u32) -> [u8; 3] {
    PLAYER_COLOURS[(p as usize) % PLAYER_COLOURS.len()]
}

/// Standard player colours (slots 0..11).
const PLAYER_COLOURS: [[u8; 3]; 12] = [
    [255, 3, 3],
    [0, 66, 255],
    [28, 230, 185],
    [84, 0, 129],
    [255, 252, 1],
    [254, 138, 14],
    [32, 192, 0],
    [229, 91, 176],
    [149, 150, 151],
    [126, 191, 241],
    [16, 98, 80],
    [78, 42, 4],
];

fn mesh_of(model: &Model, tint_base: Option<[u8; 3]>) -> Mesh {
    let visible = model.idle_geoset_visibility();
    let parts = model
        .geosets
        .iter()
        .enumerate()
        .filter(|(i, _)| visible.get(*i).copied().unwrap_or(true))
        .map(|(i, g)| Part {
            verts: g.vertices.clone(),
            normals: if g.normals.len() == g.vertices.len() {
                g.normals.clone()
            } else {
                vec![[0.0, 0.0, 1.0]; g.vertices.len()]
            },
            tris: g.triangles(),
            tint: tint_base.unwrap_or(DOODAD_TINTS[i % DOODAD_TINTS.len()]),
        })
        .collect();
    Mesh { parts }
}

/// SLK model paths have no extension or a `.mdl` one; the archives store `.mdx` files.
/// Loads and parses a model; the error names the first candidate that failed to parse, or
/// says that no candidate exists in the archives.
fn load_model(archives: &Archives<'_>, path: &str) -> Result<Model, String> {
    let stem = [".mdl", ".mdx", ".MDL", ".MDX"]
        .iter()
        .find_map(|ext| path.strip_suffix(ext))
        .unwrap_or(path);
    let candidates = [format!("{stem}.mdx"), format!("{stem}.mdl")];
    let mut parse_error = None;
    for p in &candidates {
        let Some(bytes) = archives.read(p) else { continue };
        match Model::parse(&bytes) {
            Ok(m) => return Ok(m),
            Err(e) => parse_error = parse_error.or(Some(format!("{p}: {e}"))),
        }
    }
    Err(parse_error.unwrap_or_else(|| "not in the archives".to_string()))
}

/// Rotation about z, then scale, then translation.
fn to_world(inst: &Instance, p: [f32; 3]) -> [f32; 3] {
    let (s, c) = inst.angle.sin_cos();
    let x = p[0] * inst.scale[0];
    let y = p[1] * inst.scale[1];
    let z = p[2] * inst.scale[2];
    [
        inst.position[0] + c * x - s * y,
        inst.position[1] + s * x + c * y,
        inst.position[2] + z,
    ]
}

fn normal_to_world(inst: &Instance, n: [f32; 3]) -> [f32; 3] {
    let (s, c) = inst.angle.sin_cos();
    [c * n[0] - s * n[1], s * n[0] + c * n[1], n[2]]
}

/// Report of a rendered image.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderReport {
    pub width: usize,
    pub height: usize,
    pub instances_drawn: usize,
    /// Instances whose model did not load; they are drawn as square footprint markers.
    pub instances_skipped: usize,
    pub markers_drawn: usize,
    pub models_loaded: usize,
    pub models_missing: Vec<String>,
    pub triangles: usize,
    pub class_counts: Vec<(&'static str, usize)>,
    /// Most common colours (quantized to 16 levels per channel) and pixel counts.
    pub top_colours: Vec<([u8; 3], usize)>,
}

/// Extends a bounding box (min, max) to contain a point.
fn grow(bounds: &mut Option<([f32; 3], [f32; 3])>, w: [f32; 3]) {
    let b = bounds.get_or_insert((w, w));
    for (lo, p) in b.0.iter_mut().zip(w) {
        *lo = lo.min(p);
    }
    for (hi, p) in b.1.iter_mut().zip(w) {
        *hi = hi.max(p);
    }
}

/// Counts of [`place_on_terrain`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PlaceStats {
    /// Instances placed on the terrain (their model was read).
    pub placed: usize,
    /// Instances whose model could not be read (missing from the archives or not parsed).
    pub unplaced: usize,
    /// Distinct model paths of the placed instances.
    pub models_loaded: usize,
    /// Distinct model paths of the unplaced instances, sorted.
    pub missing_paths: Vec<String>,
    /// Flat footprint squares drawn for unplaced instances (see `push_marker`).
    pub markers: usize,
    /// Triangles added to the props (markers included).
    pub triangles: usize,
    /// Of those, triangles that sample a texture (the rest are flat tinted).
    pub textured_triangles: usize,
}

/// The first layer of a geoset's material: the texture and filter mode the geoset draws with.
/// All layers of a geoset's material that can show (static alpha 0 without a track is skipped);
/// one empty entry when the geoset has no material, so it still draws flat.
fn geoset_layers(model: &Model, geoset: &mdx::Geoset) -> Vec<Option<mdx::Layer>> {
    let layers: Vec<Option<mdx::Layer>> = model
        .materials
        .get(geoset.material_id as usize)
        .map(|m| {
            m.layers
                .iter()
                .filter(|l| l.alpha > 0.0 || l.alpha_track.is_some())
                .cloned()
                .map(Some)
                .collect()
        })
        .unwrap_or_default();
    if layers.is_empty() {
        vec![None]
    } else {
        layers
    }
}

/// Texture path of a geoset's first layer. Replaceable ids: 1 is the team colour and 2 the team
/// glow (`ReplaceableTextures\TeamColor\TeamColorNN.blp` and `...\TeamGlow\TeamGlowNN.blp`, NN the
/// player slot, 0 for objects without an owner); 31 and up are the instance's own texture (a
/// destructable's `texFile`). Other replaceable ids give `None`.
fn geoset_texture(model: &Model, layer: &mdx::Layer, inst: &Instance) -> Option<String> {
    let texture = model.textures.get(layer.texture_id as usize)?;
    if texture.replaceable_id == 0 {
        return (!texture.path.is_empty()).then(|| texture.path.clone());
    }
    replaceable_path(texture.replaceable_id, inst.player.unwrap_or(0), inst.texture.as_deref())
}

/// The archive path that replaceable texture `id` stands for, for the player slot `player`
/// (team colour and glow) or the instance's own texture (`instance_texture`, ids 31 and up).
fn replaceable_path(id: u32, player: u32, instance_texture: Option<&str>) -> Option<String> {
    let slot = player.min(11);
    match id {
        1 => Some(format!("ReplaceableTextures\\TeamColor\\TeamColor{slot:02}.blp")),
        2 => Some(format!("ReplaceableTextures\\TeamGlow\\TeamGlow{slot:02}.blp")),
        31.. => instance_texture.map(str::to_string),
        _ => None,
    }
}

/// Places every instance on the terrain: its triangles go to `props`, lifted onto the ground
/// height at its position (the `z` of the placement is an offset above that ground). Textured
/// geosets sample their first layer's texture; the others are flat in the doodad tints (units:
/// the player colour). Presentation only: no animation, team colour or transparency sorting.
pub fn place_on_terrain(
    instances: &[Instance],
    terrain: &w3map::Terrain,
    props: &mut PropBuilder<'_, '_>,
) -> PlaceStats {
    let mut stats = PlaceStats::default();
    let mut used = BTreeSet::new();
    let mut missing = BTreeSet::new();
    for inst in instances {
        // The placement lifted onto the ground under it; its z is then the height of the model.
        let mut lifted = inst.clone();
        lifted.position[2] += ground_at(terrain, inst.position[0], inst.position[1]);
        // The triangles of every geoset as (texture path, world-space triangles, tint).
        let geosets: Option<Vec<PlacedGeoset>> = props.model(&inst.model).map(|model| {
            // Death-only and upgrade-only geosets are hidden by their GEOA alpha in the idle pose.
            let visible = model.idle_geoset_visibility();
            model
                .geosets
                .iter()
                .enumerate()
                .filter(|(i, _)| visible.get(*i).copied().unwrap_or(true))
                .flat_map(|(i, g)| {
                    let uvs = g.uv_sets.first().map(Vec::as_slice).unwrap_or(&[]);
                    let tris = g
                        .triangles()
                        .iter()
                        .filter_map(|t| {
                            let [a, b, c] = t.map(usize::from);
                            let v = |i: usize| g.vertices.get(i).map(|p| to_world(&lifted, *p));
                            let uv = |i: usize| uvs.get(i).copied().unwrap_or([0.0, 0.0]);
                            Some(([v(a)?, v(b)?, v(c)?], [uv(a), uv(b), uv(c)]))
                        })
                        .collect();
                    let tint = match inst.kind {
                        Kind::Doodad => DOODAD_TINTS[i % DOODAD_TINTS.len()],
                        Kind::Unit => inst.player.map_or([200, 200, 200], |p| PLAYER_COLOURS[(p as usize) % PLAYER_COLOURS.len()]),
                    };
                    let tint = tint.map(|c| f32::from(c) / 255.0);
                    // Every layer is drawn in order (team colour base, then the texture on top).
                    let layers = geoset_layers(model, g);
                    let tris: Vec<PlacedTriangle> = tris;
                    layers
                        .into_iter()
                        .map(|layer| {
                            let blend = layer.as_ref().map_or(PropBlend::Opaque, |l| PropBlend::from_mode(BlendMode::from_raw(l.filter_mode)));
                            let texture = layer.as_ref().and_then(|l| geoset_texture(model, l, inst));
                            (texture, blend, tris.clone(), tint)
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        });
        let Some(geosets) = geosets else {
            stats.unplaced += 1;
            missing.insert(inst.model.clone());
            push_marker(props, &lifted, inst.kind, inst.player, &mut stats);
            continue;
        };
        used.insert(inst.model.clone());
        stats.placed += 1;
        for (path, blend, tris, tint) in geosets {
            let texture = path.as_deref().and_then(|p| props.texture(p));
            for (positions, uvs) in tris {
                let k = lambert(&positions);
                let colour = if texture.is_some() {
                    [k, k, k, 1.0]
                } else {
                    [tint[0] * k, tint[1] * k, tint[2] * k, 1.0]
                };
                let vertex = |i: usize| crate::terrain::TerrainVertex {
                    position: positions[i],
                    uv: uvs[i],
                    cell: [0.0, 0.0, 1.0, 1.0],
                    color: colour,
                };
                props.push_blend(texture, blend, [vertex(0), vertex(1), vertex(2)]);
                stats.triangles += 1;
                if texture.is_some() {
                    stats.textured_triangles += 1;
                }
            }
        }
    }
    stats.models_loaded = used.len();
    stats.missing_paths = missing.into_iter().collect();
    stats
}

/// A flat square on the ground marking an instance whose model is missing: half a unit-size
/// footprint (48 units, as the software renderer uses), in the flat doodad or player tint.
fn push_marker(
    props: &mut PropBuilder<'_, '_>,
    lifted: &Instance,
    kind: Kind,
    player: Option<u32>,
    stats: &mut PlaceStats,
) {
    let half = 48.0 * lifted.scale[0].max(0.25);
    let [x, y, z] = lifted.position;
    let corners = [[x - half, y - half, z], [x + half, y - half, z], [x + half, y + half, z], [x - half, y + half, z]];
    let tint = match kind {
        Kind::Doodad => [200u8, 170, 60],
        Kind::Unit => player.map_or([255, 255, 255], |p| PLAYER_COLOURS[(p as usize) % PLAYER_COLOURS.len()]),
    };
    let colour = tint.map(|c| f32::from(c) / 255.0);
    let vertex = |p: [f32; 3]| crate::terrain::TerrainVertex {
        position: p,
        uv: [0.0, 0.0],
        cell: [0.0; 4],
        color: [colour[0], colour[1], colour[2], 1.0],
    };
    for tri in [[0usize, 1, 2], [0, 2, 3]] {
        props.push(None, tri.map(|i| vertex(corners[i])));
        stats.triangles += 1;
    }
    stats.markers += 1;
}

/// One geoset of a placed instance: texture path, draw mode, world-space triangles and flat tint.
type PlacedGeoset = (Option<String>, PropBlend, Vec<PlacedTriangle>, [f32; 3]);

/// Renders the instances into an RGB canvas. Returns the canvas and a report.
pub fn render(instances: &[Instance], archives: &Archives<'_>, width: usize, height: usize) -> (Canvas, RenderReport) {
    let mut cache: HashMap<String, Option<Mesh>> = HashMap::new();
    let mut missing = Vec::new();
    let mut models_loaded = 0;
    for inst in instances {
        if cache.contains_key(&inst.model) {
            continue;
        }
        let mesh = match load_model(archives, &inst.model) {
            Ok(m) => {
                models_loaded += 1;
                Some(mesh_of(&m, None))
            }
            Err(reason) => {
                missing.push(format!("{} ({reason})", inst.model));
                None
            }
        };
        cache.insert(inst.model.clone(), mesh);
    }

    let pitch = 55f32.to_radians();
    let mut drawn = 0;
    let mut skipped = 0;
    let mut triangles = 0;
    let mut bounds = None::<([f32; 3], [f32; 3])>;
    for inst in instances {
        match cache.get(&inst.model).and_then(Option::as_ref) {
            Some(mesh) => {
                drawn += 1;
                for part in &mesh.parts {
                    for v in &part.verts {
                        grow(&mut bounds, to_world(inst, *v));
                    }
                }
            }
            None => {
                skipped += 1;
                let half = 48.0 * inst.scale[0].max(0.25);
                let p = inst.position;
                grow(&mut bounds, [p[0] - half, p[1] - half, p[2]]);
                grow(&mut bounds, [p[0] + half, p[1] + half, p[2]]);
            }
        }
    }
    let (lo, hi) = bounds.unwrap_or(([-512.0; 3], [512.0; 3]));
    let view = View::fit(width, height, pitch, [lo, hi].into_iter());
    let mut canvas = Canvas::new(width, height);
    canvas.fill_ground(&view, [lo[0] - 256.0, lo[1] - 256.0, lo[2]], [hi[0] + 256.0, hi[1] + 256.0, lo[2]], [70, 98, 56]);

    let light = normalize([-0.35, -0.55, 0.76]);
    let mut markers = 0;
    for inst in instances {
        let Some(mesh) = cache.get(&inst.model).and_then(Option::as_ref) else {
            // No model: a square footprint marks the placement (unit size scaled by the instance).
            let half = 48.0 * inst.scale[0].max(0.25);
            let [x, y, z] = inst.position;
            let quad = [[x - half, y - half, z], [x + half, y - half, z], [x + half, y + half, z], [x - half, y + half, z]];
            let colour = match inst.kind {
                Kind::Doodad => [200, 170, 60],
                Kind::Unit => inst.player.map_or([255, 255, 255], |p| PLAYER_COLOURS[(p as usize) % PLAYER_COLOURS.len()]),
            };
            let class = match inst.kind {
                Kind::Doodad => CLASS_DOODAD,
                Kind::Unit => CLASS_UNIT,
            };
            canvas.triangle(&view, [quad[0], quad[1], quad[2]], colour, class);
            canvas.triangle(&view, [quad[0], quad[2], quad[3]], colour, class);
            markers += 1;
            continue;
        };
        let (class, player_tint) = match inst.kind {
            Kind::Doodad => (CLASS_DOODAD, None),
            Kind::Unit => (CLASS_UNIT, inst.player.map(|p| PLAYER_COLOURS[(p as usize) % PLAYER_COLOURS.len()])),
        };
        for part in &mesh.parts {
            let tint = player_tint.unwrap_or(part.tint);
            for tri in &part.tris {
                let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                if i0 >= part.verts.len() || i1 >= part.verts.len() || i2 >= part.verts.len() {
                    continue;
                }
                let world = [to_world(inst, part.verts[i0]), to_world(inst, part.verts[i1]), to_world(inst, part.verts[i2])];
                let n = normalize(normal_to_world(
                    inst,
                    [
                        part.normals[i0][0] + part.normals[i1][0] + part.normals[i2][0],
                        part.normals[i0][1] + part.normals[i1][1] + part.normals[i2][1],
                        part.normals[i0][2] + part.normals[i1][2] + part.normals[i2][2],
                    ],
                ));
                let lambert = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).abs();
                let shade = 0.35 + 0.65 * lambert;
                let colour = tint.map(|c| (f32::from(c) * shade).min(255.0) as u8);
                canvas.triangle(&view, world, colour, class);
                triangles += 1;
            }
        }
    }

    let (class_counts, top_colours) = canvas.histogram(8);
    let report = RenderReport {
        width,
        height,
        instances_drawn: drawn,
        instances_skipped: skipped,
        markers_drawn: markers,
        models_loaded,
        models_missing: missing,
        triangles,
        class_counts,
        top_colours,
    };
    (canvas, report)
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(model: &str, position: [f32; 3], angle: f32, scale: [f32; 3]) -> Instance {
        Instance {
            kind: Kind::Doodad,
            model: model.to_string(),
            position,
            angle,
            scale,
            player: None,
            texture: None,
            sequence_name: String::new(),
            animation_time_ms: None,
        }
    }

    #[test]
    fn replaceable_textures_resolve_per_player_and_per_instance() {
        assert_eq!(
            replaceable_path(1, 3, None).as_deref(),
            Some("ReplaceableTextures\\TeamColor\\TeamColor03.blp")
        );
        assert_eq!(
            replaceable_path(2, 11, None).as_deref(),
            Some("ReplaceableTextures\\TeamGlow\\TeamGlow11.blp")
        );
        assert_eq!(
            replaceable_path(1, 40, None).as_deref(),
            Some("ReplaceableTextures\\TeamColor\\TeamColor11.blp"),
            "slot clamped"
        );
        assert_eq!(replaceable_path(31, 0, Some("Textures\\Tree.blp")).as_deref(), Some("Textures\\Tree.blp"));
        assert_eq!(replaceable_path(31, 0, None), None, "no destructable texture");
        assert_eq!(replaceable_path(7, 0, Some("x")), None);
    }

    #[test]
    fn doodad_model_path_takes_the_placement_variation() {
        // Doodads.slk names the family; the placement's variation picks the file (numVar bounds it).
        let rock = ModelType {
            file: "Doodads\\Ruins\\Rocks\\Ruins_Rock\\Ruins_Rock".into(),
            num_var: 6,
            tex_file: None,
        };
        assert_eq!(rock.path_for(0), "Doodads\\Ruins\\Rocks\\Ruins_Rock\\Ruins_Rock0");
        assert_eq!(rock.path_for(5), "Doodads\\Ruins\\Rocks\\Ruins_Rock\\Ruins_Rock5");
        assert_eq!(rock.path_for(9), "Doodads\\Ruins\\Rocks\\Ruins_Rock\\Ruins_Rock5", "past numVar uses the last");
        // Without numVar the file is used as it is (unit types name their file).
        let unit = ModelType {
            file: "Units\\Human\\Footman\\Footman.mdl".into(),
            num_var: 0,
            tex_file: None,
        };
        assert_eq!(unit.path_for(3), "Units\\Human\\Footman\\Footman.mdl");
    }

    #[test]
    fn slk_rows_give_file_numvar_and_texture() {
        let text = "ID;PWXL;N;E\nC;X1;Y1;K\"doodID\"\nC;X2;Y1;K\"file\"\nC;X3;Y1;K\"numVar\"\nC;X4;Y1;K\"texFile\"\n\
C;X1;Y2;K\"VinP\"\nC;X2;Y2;K\"Doodads\\Ruins\\Plants\\VinyPlant\\VinyPlant\"\nC;X3;Y2;K4\n\
E\n";
        let slk = Slk::parse_bytes(text.as_bytes()).expect("slk");
        let row = slk.rows().next().expect("row");
        assert_eq!(row.key(), "VinP");
        assert_eq!(row.raw("numVar").map(str::trim), Some("4"));
        assert_eq!(clean_path(row.raw("file").expect("file")), "Doodads\\Ruins\\Plants\\VinyPlant\\VinyPlant");
    }

    #[test]
    fn object_transform_scales_then_turns_then_moves() {
        // A quarter turn maps the scaled x axis onto y.
        let inst = instance("m", [10.0, 20.0, 5.0], std::f32::consts::FRAC_PI_2, [2.0, 1.0, 1.0]);
        let p = to_world(&inst, [1.0, 0.0, 0.0]);
        assert!((p[0] - 10.0).abs() < 1e-5, "x {p:?}");
        assert!((p[1] - 22.0).abs() < 1e-5, "y {p:?}");
        assert!((p[2] - 5.0).abs() < 1e-5, "z {p:?}");
        let identity = instance("m", [0.0; 3], 0.0, [1.0; 3]);
        assert_eq!(to_world(&identity, [3.0, 4.0, 5.0]), [3.0, 4.0, 5.0]);
    }

    #[test]
    fn instances_without_a_readable_model_become_markers() {
        let archives = Archives::new(&[]);
        let mut props = PropBuilder::new(&archives);
        let flat_point = w3map::TilePoint {
            ground_height: 8192,
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
            layer_height: 0,
        };
        let terrain = w3map::Terrain {
            main_tileset: 0,
            custom_tileset: false,
            ground_tilesets: vec![],
            cliff_tilesets: vec![],
            width: 2,
            height: 2,
            center: [0.0, 0.0],
            points: vec![flat_point; 4],
        };
        let instances = [
            instance("Doodads\\Missing\\A", [10.0, 10.0, 0.0], 0.0, [1.0; 3]),
            instance("Doodads\\Missing\\A", [20.0, 10.0, 0.0], 0.0, [1.0; 3]),
        ];
        let stats = place_on_terrain(&instances, &terrain, &mut props);
        assert_eq!((stats.placed, stats.unplaced, stats.markers), (0, 2, 2));
        assert_eq!(stats.missing_paths, vec!["Doodads\\Missing\\A".to_string()]);
        assert_eq!(stats.triangles, 4, "two squares of two triangles");
        assert_eq!(stats.textured_triangles, 0);
    }
}

/// `viewer --map MAP --mpq ARCHIVE [--mpq ARCHIVE ...] --objects-png OUT.png`
pub fn cli(args: &[String]) -> Result<(), Box<dyn Error>> {
    let mut map = None::<PathBuf>;
    let mut mpqs = Vec::new();
    let mut out = None::<PathBuf>;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--map" => map = Some(PathBuf::from(it.next().ok_or("--map needs a path")?)),
            "--mpq" => mpqs.push(PathBuf::from(it.next().ok_or("--mpq needs a path")?)),
            "--objects-png" => out = Some(PathBuf::from(it.next().ok_or("--objects-png needs a path")?)),
            other => return Err(format!("unknown option for --objects-png: {other}").into()),
        }
    }
    let (Some(map), Some(out)) = (map, out) else {
        return Err("usage: viewer --map MAP --mpq ARCHIVE [--mpq ...] --objects-png OUT.png".into());
    };
    let archive_bytes: Vec<Vec<u8>> = mpqs.iter().map(std::fs::read).collect::<Result<_, _>>()?;
    let sources: Vec<&[u8]> = archive_bytes.iter().map(Vec::as_slice).collect();
    let archives = Archives::new(&sources);
    let map_bytes = std::fs::read(&map)?;

    let catalog = Catalog::load(&archives);
    for note in &catalog.notes {
        eprintln!("objects: {note}");
    }
    let (instances, stats) = placements(&map_bytes, &catalog);
    println!(
        "objects: doodads parsed {} (with model {}), units parsed {} (with model {}), instances {}",
        stats.doodads_parsed,
        stats.doodads_with_model,
        stats.units_parsed,
        stats.units_with_model,
        instances.len()
    );
    for note in &stats.notes {
        println!("objects: note: {note}");
    }
    for (id, n) in &stats.unmodelled {
        println!("objects: no model for type {id}: {n} placements");
    }

    let (canvas, report) = render(&instances, &archives, 1600, 1200);
    let rgb = canvas.rgb.clone();
    std::fs::write(&out, png::encode_rgb(report.width as u32, report.height as u32, &rgb))?;
    println!(
        "objects: rendered {} instances as models, {} as markers (model failed), {} models loaded, {} triangles -> {}",
        report.instances_drawn,
        report.markers_drawn,
        report.models_loaded,
        report.triangles,
        out.display()
    );
    for m in report.models_missing.iter().take(12) {
        println!("objects: model not found: {m}");
    }
    println!("objects: pixels by class:");
    for (name, n) in &report.class_counts {
        println!("  {name:>10} {n}");
    }
    println!("objects: top colours (16 levels per channel):");
    for (c, n) in &report.top_colours {
        println!("  #{:02x}{:02x}{:02x} {n}", c[0], c[1], c[2]);
    }
    Ok(())
}
