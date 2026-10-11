//! Native viewer:
//! * `viewer [model.mdx [texture.blp]]`: a model. Without arguments it shows a built-in cube.
//! * `viewer --map map.w3x`: the terrain of a map. Needs the map archive; ground textures are
//!   best effort (see `viewer::terrain`), otherwise procedural sheets are used.
//!
//! On wasm32 the browser entry in the library runs instead, so this `main` does nothing there.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::error::Error;

    use viewer::scene::{Scene, TextureData};
    use viewer::terrain::TerrainScene;

    fn load(args: &[String]) -> Result<Scene, Box<dyn Error>> {
        match args {
            [] => Ok(Scene::builtin_cube()),
            [model] | [model, _] => {
                let model = mdx::Model::parse(&std::fs::read(model)?)?;
                let texture = match args.get(1) {
                    Some(path) => {
                        let blp = blp::Blp::parse(&std::fs::read(path)?)?;
                        Some(TextureData::from_blp(&blp)?)
                    }
                    None => None,
                };
                Ok(Scene::from_model(&model, texture)?)
            }
            _ => Err("usage: viewer [model.mdx [texture.blp]] | viewer --map map.w3x".into()),
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let scene = load(&args)?;
    viewer::run(scene)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
