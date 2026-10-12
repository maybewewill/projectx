//! Native viewer: `viewer [--seq NAME] [--texture-dir DIR] [model.mdx [texture.blp]]`,
//! or `viewer --map map.w3x [--mpq archive]... [--render-png out.png] [--size WxH]
//! [--camera-at X,Y | --camera-start SLOT] [--zoom Z]` for the terrain of a map. The `--mpq`
//! archives (load order, last wins) supply the ground, cliff and model textures; without them the
//! procedural sheets are used. `--render-png` draws offscreen to a PNG file instead of opening a
//! window (see `viewer::offscreen`). `--camera-at X,Y` looks at world coordinates X, Y;
//! `--camera-start SLOT` looks at that player's start location from war3map.w3i; `--zoom Z`
//! brings the camera Z times closer (default 1).
//!
//! Without a model it shows a built-in cube. With one, the model is animated and plays its
//! "Stand" sequence (or the one named by `--seq`). Textures are matched by file name, ignoring
//! case, in `--texture-dir`; a `texture.blp` argument stands in for any texture that is not
//! found. The files are yours: nothing here ships game data.
//!
//! On wasm32 the browser entry in the library runs instead, so this `main` does nothing there.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::{Path, PathBuf};

    use viewer::model::ModelAsset;
    use viewer::scene::{Scene, TextureData};

    #[derive(Default)]
    struct Options {
        seq: Option<String>,
        texture_dir: Option<PathBuf>,
        model: Option<PathBuf>,
        fallback_texture: Option<PathBuf>,
    }

    fn parse_args(args: &[String]) -> Result<Options, String> {
        let mut opts = Options::default();
        let mut positional = Vec::new();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--seq" => {
                    opts.seq = Some(it.next().ok_or("--seq needs a sequence name")?.clone());
                }
                "--texture-dir" => {
                    let dir = it.next().ok_or("--texture-dir needs a directory")?;
                    opts.texture_dir = Some(PathBuf::from(dir));
                }
                other if other.starts_with("--") => return Err(format!("unknown option {other}")),
                other => positional.push(other),
            }
        }
        match positional.as_slice() {
            [] => {}
            [model] => opts.model = Some(PathBuf::from(model)),
            [model, texture] => {
                opts.model = Some(PathBuf::from(model));
                opts.fallback_texture = Some(PathBuf::from(texture));
            }
            _ => {
                return Err(
                    "usage: viewer [--seq NAME] [--texture-dir DIR] [model.mdx [texture.blp]]"
                        .into(),
                )
            }
        }
        Ok(opts)
    }

    /// Last component of a path as written in an MDX file (which uses backslashes).
    fn file_name(path: &str) -> &str {
        path.rsplit(['\\', '/']).next().unwrap_or(path)
    }

    fn find_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
    }

    fn load_blp(path: &Path) -> Option<TextureData> {
        let result = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| blp::Blp::parse(&bytes).map_err(|e| e.to_string()))
            .and_then(|blp| TextureData::from_blp(&blp).map_err(|e| e.to_string()));
        match result {
            Ok(texture) => Some(texture),
            Err(message) => {
                eprintln!("viewer: {}: {message}", path.display());
                None
            }
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--objects-png") {
        viewer::scene_objects::cli(&args)?;
        return Ok(());
    }
    if args.iter().any(|a| a == "--map") {
        return run_map(&args);
    }
    if args.iter().any(|a| a == "--ui") {
        return run_ui(&args);
    }
    let opts = parse_args(&args)?;
    let Some(model_path) = opts.model.as_deref() else {
        viewer::run(Scene::builtin_cube())?;
        return Ok(());
    };

    let model = mdx::Model::parse(&std::fs::read(model_path)?)?;
    let fallback = opts.fallback_texture.as_deref().and_then(load_blp);
    let asset = ModelAsset::new(model, |path| {
        let name = file_name(path);
        if let Some(dir) = &opts.texture_dir {
            if let Some(texture) = find_in_dir(dir, name).and_then(|p| load_blp(&p)) {
                return Some(texture);
            }
        }
        fallback.clone()
    })?;
    viewer::run_model(asset, opts.seq.as_deref())?;
    Ok(())
}

/// `viewer --map map.w3x [--mpq archive]... [--render-png out.png] [--size WxH]`.
///
/// `--mpq` archives supply the terrain art (`TerrainArt\Terrain.slk` and the BLP textures); give
/// them in load order, the last one wins. With `--render-png` the terrain is drawn offscreen to
/// that file and a short summary of the image is printed; without it the window opens.
#[cfg(not(target_arch = "wasm32"))]
fn run_map(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;

    use viewer::offscreen;
    use viewer::terrain::TerrainScene;

    let mut map: Option<PathBuf> = None;
    let mut mpqs: Vec<PathBuf> = Vec::new();
    let mut png: Option<PathBuf> = None;
    let mut dump_atlas: Option<PathBuf> = None;
    let mut size = (1280u32, 720u32);
    let mut camera_at: Option<(f32, f32)> = None;
    let mut camera_start: Option<usize> = None;
    let mut zoom: f32 = 1.0;
    let mut sim = SimArgs::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--sim-seconds" => {
                sim.seconds = Some(it.next().ok_or("--sim-seconds needs seconds")?.parse()?);
            }
            "--computer" => {
                let text = it.next().ok_or("--computer needs SLOT[:RACE]")?;
                sim.computers.push(parse_computer(text)?);
            }
            "--dll" => sim.dll = Some(it.next().ok_or("--dll needs a file")?.into()),
            "--frames" => sim.frames = Some(it.next().ok_or("--frames needs a count")?.parse()?),
            "--every" => sim.every = Some(it.next().ok_or("--every needs seconds")?.parse()?),
            "--map" => map = Some(it.next().ok_or("--map needs a file")?.into()),
            "--mpq" => mpqs.push(it.next().ok_or("--mpq needs a file")?.into()),
            "--render-png" => png = Some(it.next().ok_or("--render-png needs a file")?.into()),
            "--dump-atlas" => {
                dump_atlas = Some(it.next().ok_or("--dump-atlas needs a file")?.into());
            }
            "--size" => {
                let text = it.next().ok_or("--size needs WxH")?;
                let (w, h) = text.split_once('x').ok_or("--size needs WxH")?;
                size = (w.parse()?, h.parse()?);
            }
            "--camera-at" => {
                let text = it.next().ok_or("--camera-at needs X,Y")?;
                let (x, y) = text.split_once(',').ok_or("--camera-at needs X,Y")?;
                camera_at = Some((x.parse()?, y.parse()?));
            }
            "--camera-start" => {
                camera_start = Some(it.next().ok_or("--camera-start needs a player slot")?.parse()?);
            }
            "--zoom" => zoom = it.next().ok_or("--zoom needs a factor")?.parse()?,
            "--hud" => {
                let race = it.next().ok_or("--hud needs a race (human, orc, undead, nightelf)")?;
                sim.hud = Some(race.clone());
            }
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    if sim.hud.is_some() && sim.seconds.is_none() {
        return Err("--hud needs --sim-seconds (the HUD shows the live simulation)".into());
    }
    // --camera-start PLAYER: the start location of that player slot from war3map.w3i.
    if let (Some(slot), None) = (camera_start, camera_at) {
        let path = map.as_ref().ok_or("--map needs a file")?;
        let bytes = std::fs::read(path)?;
        let archive = mpq::Archive::open(&bytes)?;
        let info = w3map::MapInfo::parse(&archive.read("war3map.w3i")?)?;
        let player = info
            .players
            .iter()
            .find(|p| p.number as usize == slot)
            .ok_or_else(|| format!("no player slot {slot} in war3map.w3i"))?;
        eprintln!("viewer: camera at the start of player {slot}: {}, {}", player.start_x, player.start_y);
        camera_at = Some((player.start_x, player.start_y));
    }
    let map = map.ok_or("usage: viewer --map map.w3x [--mpq archive]... [--render-png out.png]")?;
    if sim.seconds.is_none() && (sim.frames.is_some() || sim.every.is_some() || !sim.computers.is_empty()) {
        return Err("--frames, --every, --computer and --dll need --sim-seconds".into());
    }
    if sim.frames.is_some() && sim.every.is_none() {
        return Err("--frames needs --every SECONDS".into());
    }
    if let Some(seconds) = sim.seconds {
        let out = png.ok_or("--sim-seconds needs --render-png out.png")?;
        let run = SimRun {
            map,
            mpqs,
            out,
            size,
            camera_at,
            zoom,
            seconds,
            args: sim,
        };
        return run_sim(run);
    }

    let map_bytes = std::fs::read(&map)?;
    let archive_bytes = mpqs
        .iter()
        .map(|path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())))
        .collect::<Result<Vec<Vec<u8>>, String>>()?;
    let sources: Vec<&[u8]> = archive_bytes.iter().map(Vec::as_slice).collect();
    let terrain = TerrainScene::from_map_with_sources(&map_bytes, &sources)?;
    let tiles = terrain.mesh.tiles;
    eprintln!(
        "viewer: {}: {}x{} tiles, {} ground textures from {} archives, {} quads",
        map.display(),
        tiles.0,
        tiles.1,
        terrain.textures_loaded,
        mpqs.len(),
        terrain.mesh.ground.quad_count() + terrain.mesh.overlays.quad_count()
    );

    let cliffs = terrain.cliffs.clone();
    eprintln!(
        "viewer: cliffs: {} tiles found, {} drawn with a model ({} with the cliff texture); {} models loaded, {} missing; {} tiles with a cliff id not in CliffTypes.slk",
        cliffs.cliff_tiles_found,
        cliffs.tiles_modeled,
        cliffs.tiles_textured,
        cliffs.models_loaded,
        cliffs.models_missing,
        cliffs.tiles_default_type
    );
    for path in cliffs.missing_paths.iter().take(8) {
        eprintln!("viewer: cliff model not found: {path}");
    }
    let placements = &terrain.object_placements;
    let objects = &terrain.objects;
    eprintln!(
        "viewer: objects: doodads {} parsed ({} with a model type), units {} parsed ({} with a model type); placed {} (models loaded {}), unplaced {} (models missing {}); {} triangles, {} textured",
        placements.doodads_parsed,
        placements.doodads_with_model,
        placements.units_parsed,
        placements.units_with_model,
        objects.placed,
        objects.models_loaded,
        objects.unplaced,
        objects.missing_paths.len(),
        objects.triangles,
        objects.textured_triangles
    );
    for (id, n) in placements.unmodelled.iter().take(8) {
        eprintln!("viewer: objects: no model type for {id}: {n} placements");
    }
    for path in objects.missing_paths.iter().take(8) {
        eprintln!("viewer: object model not found: {path}");
    }
    let mut by_blend = std::collections::BTreeMap::new();
    for batch in &terrain.props.batches {
        *by_blend.entry(format!("{:?}", batch.blend)).or_insert(0usize) += batch.mesh.indices.len() / 3;
    }
    eprintln!("viewer: prop triangles by blend mode: {by_blend:?}");
    eprintln!(
        "viewer: prop textures: {} read, {} missing{}",
        terrain.props.textures.len(),
        terrain.props.missing_textures.len(),
        if terrain.props.missing_textures.is_empty() { String::new() } else { format!(" (first: {})", terrain.props.missing_textures[0]) }
    );
    for note in &placements.notes {
        eprintln!("viewer: objects: note: {note}");
    }
    if let Some(ids) = cliff_tileset_ids(&map_bytes) {
        eprintln!("viewer: cliff tilesets of the map: {}", ids.join(" "));
    }
    if let Some(out) = dump_atlas {
        let size = terrain.atlas.size;
        offscreen::write_png(&out, size, size, &terrain.atlas.rgba)?;
        eprintln!("viewer: wrote the ground atlas to {}", out.display());
    }
    match png {
        Some(out) => {
            let (w, h) = size;
            // The camera looks at the ground under the chosen point (the mesh middle otherwise).
            let target = camera_at.map(|(x, y)| {
                let z = w3e_of(&map_bytes).map_or(terrain.bounds.center().z, |t| {
                    viewer::terrain_objects::ground_at(&t, x, y)
                });
                [x, y, z]
            });
            let rgba = offscreen::render_terrain_at(&terrain, w, h, target, zoom)?;
            offscreen::write_png(&out, w, h, &rgba)?;
            let s = offscreen::summarize(w, h, &rgba);
            eprintln!(
                "viewer: wrote {} ({}x{}): {} distinct colours, mean RGB {:.1} {:.1} {:.1}",
                out.display(),
                w,
                h,
                s.distinct_colors,
                s.mean_rgb[0],
                s.mean_rgb[1],
                s.mean_rgb[2]
            );
            eprintln!("viewer: luma histogram (32-wide bins): {:?}", s.luma_histogram);
        }
        None => viewer::run_terrain(terrain)?,
    }
    Ok(())
}

/// The `--sim-seconds` options (see [`run_sim`]).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct SimArgs {
    seconds: Option<f64>,
    /// `--computer SLOT[:RACE]` (race ids as `engine::cli` takes them).
    computers: Vec<(u32, Option<u32>)>,
    dll: Option<std::path::PathBuf>,
    /// `--frames K --every S`: K pictures, the first at `seconds`, then one every S game seconds.
    frames: Option<usize>,
    every: Option<f64>,
    /// `--hud RACE`: draw the ConsoleUI layer and the minimap over the frame.
    hud: Option<String>,
}

/// The `--hud` state of a run: the console scene, the minimap frame and its picture of the map.
#[cfg(not(target_arch = "wasm32"))]
struct HudState {
    scene: viewer::ui::UiScene,
    slot: Option<viewer::hud::MinimapSlot>,
    base: viewer::minimap::MinimapBase,
    /// The player whose resources are shown: the first computer slot of that race, else slot 0.
    player: u32,
}

/// `--hud` race name to the race id of `--computer` (`None` for an unknown name).
#[cfg(not(target_arch = "wasm32"))]
fn race_id(name: &str) -> Option<u32> {
    match name {
        "human" => Some(1),
        "orc" => Some(2),
        "undead" => Some(3),
        "nightelf" => Some(4),
        _ => None,
    }
}

/// What [`run_sim`] renders.
#[cfg(not(target_arch = "wasm32"))]
struct SimRun {
    map: std::path::PathBuf,
    mpqs: Vec<std::path::PathBuf>,
    out: std::path::PathBuf,
    size: (u32, u32),
    camera_at: Option<(f32, f32)>,
    zoom: f32,
    seconds: f64,
    args: SimArgs,
}

/// `--computer SLOT[:RACE]` with RACE human, orc, undead or nightelf.
#[cfg(not(target_arch = "wasm32"))]
fn parse_computer(text: &str) -> Result<(u32, Option<u32>), String> {
    let (slot, race) = match text.split_once(':') {
        Some((s, r)) => (s, Some(r)),
        None => (text, None),
    };
    let slot = slot.parse::<u32>().map_err(|_| format!("--computer: bad slot {text}"))?;
    let race = match race {
        None => None,
        Some("human") => Some(1),
        Some("orc") => Some(2),
        Some("undead") => Some(3),
        Some("nightelf") => Some(4),
        Some(other) => return Err(format!("--computer: unknown race {other}")),
    };
    Ok((slot, race))
}

/// Game ticks of `seconds` of game time (one tick is 0.005 s, `docs/spec/lockstep.md` section 9).
#[cfg(not(target_arch = "wasm32"))]
fn ticks_for(seconds: f64) -> u64 {
    (seconds.max(0.0) * 200.0).round() as u64
}

/// `out.png` with frame index `k` in the name: `out_000.png`.
#[cfg(not(target_arch = "wasm32"))]
fn frame_path(out: &std::path::Path, k: usize) -> std::path::PathBuf {
    let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let name = match out.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{stem}_{k:03}.{ext}"),
        None => format!("{stem}_{k:03}"),
    };
    out.with_file_name(name)
}

/// `viewer --map ... --sim-seconds N [--computer SLOT:RACE]... [--frames K --every S]`: boots the map
/// with `engine::cli` (the same path as `w3boot`), runs N game seconds, then draws the live units of the
/// simulation in place of the map's unit placements (doodads stay from the map). With `--frames` it
/// writes K pictures, one every S game seconds, named after `--render-png` with the index added.
#[cfg(not(target_arch = "wasm32"))]
fn run_sim(run: SimRun) -> Result<(), Box<dyn std::error::Error>> {
    use viewer::{offscreen, sim_view, terrain::TerrainScene, terrain_objects};

    let boot = engine::cli::BootArgs {
        map: Some(run.map.to_string_lossy().into_owned()),
        dll: run.args.dll.as_ref().map(|p| p.to_string_lossy().into_owned()),
        mpqs: run.mpqs.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        computers: run.args.computers.clone(),
        ..Default::default()
    };
    let mut engine = engine::cli::boot(&boot)?;
    engine.step_ticks(ticks_for(run.seconds));

    let map_bytes = std::fs::read(&run.map)?;
    let archive_bytes = run
        .mpqs
        .iter()
        .map(|path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())))
        .collect::<Result<Vec<Vec<u8>>, String>>()?;
    let sources: Vec<&[u8]> = archive_bytes.iter().map(Vec::as_slice).collect();

    let mut hud = match run.args.hud.as_deref() {
        None => None,
        Some(race) => Some(load_hud(race, &run.args.computers, &map_bytes, &sources, run.size)?),
    };

    let frames = run.args.frames.unwrap_or(1).max(1);
    let (w, h) = run.size;
    for k in 0..frames {
        if k > 0 {
            engine.step_ticks(ticks_for(run.args.every.unwrap_or(0.0)));
        }
        let now = engine.report().game_time;
        let units = sim_view::units(&engine);
        let game_time_ms = (now * 1000.0) as u32;
        let terrain = TerrainScene::from_map_with_sim_units(&map_bytes, &sources, Some(&units), game_time_ms)?;
        let alive = sim_view::alive_per_owner(&units);
        let alive_text: Vec<String> = alive.iter().map(|(p, n)| format!("p{p}={n}")).collect();
        eprintln!(
            "viewer: frame {k} at game time {now:.2}s: units alive per player: {}",
            if alive_text.is_empty() { "none".to_string() } else { alive_text.join(" ") }
        );
        for slot in run.args.computers.iter().map(|c| c.0) {
            eprintln!("viewer:   p{slot}: {}", engine.host().game().production_summary(slot));
        }
        for note in terrain.object_placements.notes.iter().filter(|n| n.starts_with("units from")) {
            eprintln!("viewer:   {note}");
        }
        let target = run.camera_at.map(|(x, y)| {
            let z = w3e_of(&map_bytes).map_or(terrain.bounds.center().z, |t| terrain_objects::ground_at(&t, x, y));
            [x, y, z]
        });
        let rgba = match hud.as_mut() {
            None => offscreen::render_terrain_at(&terrain, w, h, target, run.zoom)?,
            Some(state) => {
                let summary = update_hud(state, &engine, &units, &terrain, target, run.zoom, (w, h))?;
                eprintln!("viewer:   hud: {summary}");
                offscreen::render_terrain_hud(&terrain, w, h, target, run.zoom, Some(&state.scene))?
            }
        };
        let path = if run.args.frames.is_some() { frame_path(&run.out, k) } else { run.out.clone() };
        offscreen::write_png(&path, w, h, &rgba)?;
        let s = offscreen::summarize(w, h, &rgba);
        eprintln!(
            "viewer: wrote {} ({}x{}): {} distinct colours, mean RGB {:.1} {:.1} {:.1}",
            path.display(),
            w,
            h,
            s.distinct_colors,
            s.mean_rgb[0],
            s.mean_rgb[1],
            s.mean_rgb[2]
        );
    }
    Ok(())
}

/// Loads the ConsoleUI layer for `race`, finds the minimap frame and the map picture. The player
/// shown in the top bar is the first computer slot of that race, else slot 0.
#[cfg(not(target_arch = "wasm32"))]
fn load_hud(
    race: &str,
    computers: &[(u32, Option<u32>)],
    map: &[u8],
    sources: &[&[u8]],
    size: (u32, u32),
) -> Result<HudState, Box<dyn std::error::Error>> {
    use viewer::{hud, minimap::MinimapBase, ui::UiScene};

    // The library keys frame names in lower case. ResourceBarFrame is its own FDF in the game (the top
    // bar texts are its children), so it is laid out with the console.
    let placed = hud::host_placements(fdf::layout::Rect::new(0.0, 0.0, 0.8, 0.6));
    let mut scene = UiScene::from_mpq_frames_sized(&["consoleui", "resourcebarframe", "upperbuttonbarframe"], sources, race, &placed, size)?;
    let slot = hud::install_minimap(&mut scene);
    hud::install_top_fill(&mut scene);
    hud::install_command_slots(&mut scene)?;
    let base = match hud::map_picture(map) {
        Some(picture) => {
            eprintln!("viewer: hud: minimap picture war3mapMap.blp {}x{}", picture.width, picture.height);
            picture
        }
        None => {
            eprintln!("viewer: hud: no war3mapMap.blp in the map; the minimap is a flat colour");
            MinimapBase::flat(256, 256, [40, 70, 40])
        }
    };
    let player = race_id(race)
        .and_then(|id| computers.iter().find(|(_, r)| *r == Some(id)).map(|(s, _)| *s))
        .unwrap_or(0);
    match slot {
        Some(s) => eprintln!("viewer: hud: minimap frame at node {} drawn {}x{} px", s.node, s.width, s.height),
        None => eprintln!("viewer: hud: no minimap slot (no *Minimap* frame and no bottom-left art); no minimap"),
    }
    eprintln!("viewer: hud: top bar shows player {player}");
    Ok(HudState { scene, slot, base, player })
}

/// Fills the HUD for one frame: the resource texts from the simulation and the minimap. Returns a
/// one-line summary for the log.
#[cfg(not(target_arch = "wasm32"))]
fn update_hud(
    state: &mut HudState,
    engine: &engine::Engine,
    units: &[viewer::scene_objects::SimUnit],
    terrain: &viewer::terrain::TerrainScene,
    target: Option<[f32; 3]>,
    zoom: f32,
    size: (u32, u32),
) -> Result<String, Box<dyn std::error::Error>> {
    use std::collections::BTreeSet;

    use sim::economy::Res;
    use viewer::{hud, minimap, offscreen, terrain_camera::TerrainCamera};

    let game = engine.host().game();
    let p = state.player;
    let stock = |res: Res| game.player_economy(p).map_or(0, |e| e.stored(res) / 10);
    let resources = hud::Resources {
        gold: i64::from(stock(Res::Gold)),
        lumber: i64::from(stock(Res::Lumber)),
        food_used: game.food_used_of(p),
        food_made: game.food_made_of(p),
    };
    let (set, missing) = hud::set_resource_texts(&mut state.scene, &resources);

    // The minimap shows the playable area; the terrain bounds stand in when the map has none.
    let world = match game.playable() {
        Some(pl) => minimap::WorldRect {
            min_x: pl.min_x.to_f32(),
            min_y: pl.min_y.to_f32(),
            max_x: pl.max_x.to_f32(),
            max_y: pl.max_y.to_f32(),
        },
        None => minimap::WorldRect {
            min_x: terrain.bounds.min.x,
            min_y: terrain.bounds.min.y,
            max_x: terrain.bounds.max.x,
            max_y: terrain.bounds.max.y,
        },
    };

    // The camera exactly as the offscreen render builds it, then its ground footprint.
    let (w, h) = size;
    let mut cam = TerrainCamera::fit(&terrain.bounds, w as f32 / h as f32);
    if let Some([x, y, z]) = target {
        cam.target = glam::Vec3::new(x, y, z);
    }
    let cam = offscreen::zoomed(cam, zoom);
    let footprint = minimap::camera_footprint(&cam, cam.target.z);

    let buildings: BTreeSet<u32> = units
        .iter()
        .map(|u| u.type_id)
        .filter(|id| game.unit_types().is_building(*id))
        .collect();
    let mut summary = format!(
        "player {p}: gold {} lumber {} food {}/{}; top bar set {:?}, missing {:?}",
        resources.gold, resources.lumber, resources.food_used, resources.food_made, set, missing
    );
    if let Some(slot) = state.slot {
        hud::update_minimap(
            &mut state.scene,
            slot,
            &state.base,
            world,
            units,
            &|id| buildings.contains(&id),
            footprint,
        )?;
        summary.push_str(&format!(
            "; minimap {}x{} px, camera box {}",
            slot.width,
            slot.height,
            if footprint.is_some() { "drawn" } else { "not on the ground" }
        ));
    }
    Ok(summary)
}

/// The parsed `war3map.w3e` of a map (`None` if unreadable).
#[cfg(not(target_arch = "wasm32"))]
fn w3e_of(map: &[u8]) -> Option<w3map::Terrain> {
    let archive = mpq::Archive::open(map).ok()?;
    w3map::Terrain::parse(&archive.read("war3map.w3e").ok()?).ok()
}

/// Cliff ids listed in a map's `war3map.w3e`, for the summary line (`None` if unreadable).
#[cfg(not(target_arch = "wasm32"))]
fn cliff_tileset_ids(map: &[u8]) -> Option<Vec<String>> {
    let terrain = w3e_of(map)?;
    Some(
        terrain
            .cliff_tilesets
            .iter()
            .map(|id| String::from_utf8_lossy(id).into_owned())
            .collect(),
    )
}

/// `viewer --ui FDF_NAME [--mpq archive]... [--race human|orc|nightelf|undead] [--offscreen out.png] [--size WxH]`.
///
/// `--mpq` archives supply the FDF and texture files; give them in load order, the last one wins.
/// `--race` selects the race for texture name resolution (default: human).
/// With `--offscreen` the UI is drawn to a PNG file; without it the window opens.
#[cfg(not(target_arch = "wasm32"))]
fn run_ui(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;

    use viewer::offscreen;
    use viewer::ui::UiScene;

    let mut ui_name: Option<String> = None;
    let mut mpqs: Vec<PathBuf> = Vec::new();
    let mut png: Option<PathBuf> = None;
    let mut size = (1280u32, 960u32);
    let mut race = "human".to_string();
    let mut list_frames = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ui" => ui_name = Some(it.next().ok_or("--ui needs a name")?.clone()),
            "--mpq" => mpqs.push(it.next().ok_or("--mpq needs a file")?.into()),
            "--race" => race = it.next().ok_or("--race needs a value")?.clone(),
            "--offscreen" => png = Some(it.next().ok_or("--offscreen needs a file")?.into()),
            "--list-frames" => list_frames = true,
            "--size" => {
                let text = it.next().ok_or("--size needs WxH")?;
                let (w, h) = text.split_once('x').ok_or("--size needs WxH")?;
                size = (w.parse()?, h.parse()?);
            }
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    let ui_name = ui_name.ok_or("usage: viewer --ui FDF_NAME [--mpq archive]... [--race RACE] [--offscreen out.png] [--size WxH]")?;

    let archive_bytes = mpqs
        .iter()
        .map(|path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())))
        .collect::<Result<Vec<Vec<u8>>, String>>()?;
    let sources: Vec<&[u8]> = archive_bytes.iter().map(Vec::as_slice).collect();

    let ui_scene = UiScene::from_mpq(&ui_name, &sources, &race)?;
    eprintln!(
        "viewer: {}: {} items in draw list",
        ui_name,
        ui_scene.draw_list.items.len()
    );
    if list_frames {
        // Every named frame with its laid-out rectangle in UI units (`--list-frames`).
        for (i, name) in ui_scene.node_names.iter().enumerate() {
            if let Some(r) = ui_scene.node_rects.get(i).copied().flatten() {
                eprintln!(
                    "viewer: frame {i} {name}: left {:.3} bottom {:.3} right {:.3} top {:.3}",
                    r.left, r.bottom, r.right, r.top
                );
            }
        }
    }

    match png {
        Some(out) => {
            let (w, h) = size;
            let rgba = offscreen::render_ui(&ui_scene, w, h)?;
            offscreen::write_png(&out, w, h, &rgba)?;
            let s = offscreen::summarize(w, h, &rgba);
            eprintln!(
                "viewer: wrote {} ({}x{}): {} distinct colours, mean RGB {:.1} {:.1} {:.1}",
                out.display(),
                w,
                h,
                s.distinct_colors,
                s.mean_rgb[0],
                s.mean_rgb[1],
                s.mean_rgb[2]
            );
        }
        None => {
            eprintln!("viewer: window rendering not yet implemented for UI");
            return Err("--offscreen is required for now".into());
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
