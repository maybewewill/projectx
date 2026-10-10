//! Native viewer: `viewer [model.mdx [texture.blp]]`. Without arguments it shows a built-in cube.
//! On wasm32 the browser entry in the library runs instead, so this `main` does nothing there.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::error::Error;

    use viewer::scene::{Scene, TextureData};

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
            _ => Err("usage: viewer [model.mdx [texture.blp]]".into()),
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let scene = load(&args)?;
    viewer::run(scene)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
