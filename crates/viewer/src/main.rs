//! Native viewer: `viewer [--seq NAME] [--texture-dir DIR] [model.mdx [texture.blp]]`,
//! or `viewer --map map.w3x` for the terrain of a map (ground textures best effort, see
//! `viewer::terrain`; procedural sheets otherwise).
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
    use viewer::terrain::TerrainScene;

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

    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.iter().any(|a| a == "--objects-png") {
            viewer::scene_objects::cli(&args)?;
            return Ok(());
        }
        if let [flag, path] = args.as_slice() {
            if flag == "--map" {
                let terrain = TerrainScene::from_map(&std::fs::read(path)?)?;
                let tiles = terrain.mesh.tiles;
                eprintln!(
                    "viewer: {path}: {}x{} tiles, {} ground textures from the archive, {} quads",
                    tiles.0,
                    tiles.1,
                    terrain.textures_loaded,
                    terrain.mesh.ground.quad_count() + terrain.mesh.overlays.quad_count()
                );
                viewer::run_terrain(terrain)?;
                return Ok(());
            }
        }
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
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

#[cfg(target_arch = "wasm32")]
fn main() {}
