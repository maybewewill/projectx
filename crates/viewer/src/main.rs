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
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
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
            other => return Err(format!("unknown option {other}").into()),
        }
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
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--ui" => ui_name = Some(it.next().ok_or("--ui needs a name")?.clone()),
            "--mpq" => mpqs.push(it.next().ok_or("--mpq needs a file")?.into()),
            "--race" => race = it.next().ok_or("--race needs a value")?.clone(),
            "--offscreen" => png = Some(it.next().ok_or("--offscreen needs a file")?.into()),
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
