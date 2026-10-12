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

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

use gamedata::Slk;
use mdx::Model;
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

/// Type id to model path, from the SLK tables.
#[derive(Default)]
pub struct Catalog {
    pub doodads: HashMap<[u8; 4], String>,
    pub units: HashMap<[u8; 4], String>,
    pub notes: Vec<String>,
}

fn raw_code(key: &str) -> Option<[u8; 4]> {
    let b = key.as_bytes();
    (b.len() == 4).then(|| [b[0], b[1], b[2], b[3]])
}

fn clean_path(text: &str) -> String {
    text.trim().trim_matches('"').replace('/', "\\")
}

/// Reads the `file` column of one SLK table into a type-id map; missing files are notes.
fn load_table(archives: &Archives<'_>, name: &str, out: &mut HashMap<[u8; 4], String>, notes: &mut Vec<String>) {
    let Some(bytes) = archives.read(name) else {
        notes.push(format!("{name} not found in the archives"));
        return;
    };
    match Slk::parse_bytes(&bytes) {
        Ok(slk) => {
            for row in slk.rows() {
                let Some(code) = raw_code(row.key()) else { continue };
                let Some(file) = row.raw("file") else { continue };
                let path = clean_path(file);
                if !path.is_empty() {
                    out.insert(code, path);
                }
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
                        Some(model) => {
                            stats.doodads_with_model += 1;
                            instances.push(Instance {
                                kind: Kind::Doodad,
                                model: model.clone(),
                                position: d.position,
                                angle: d.angle,
                                scale: d.scale,
                                player: None,
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
                        Some(model) => {
                            stats.units_with_model += 1;
                            instances.push(Instance {
                                kind: Kind::Unit,
                                model: model.clone(),
                                position: u.position,
                                angle: u.angle,
                                scale: u.scale,
                                player: Some(u.player),
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

fn code_text(id: &[u8; 4]) -> String {
    String::from_utf8_lossy(id).into_owned()
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
    let parts = model
        .geosets
        .iter()
        .enumerate()
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
