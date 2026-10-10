//! Browser entry points. The page starts the built-in cube when the module loads; JavaScript
//! can then replace the model with `load_model`.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use crate::scene::Scene;

thread_local! {
    /// A scene parsed by `load_model`, waiting for the render loop to pick it up.
    static PENDING: RefCell<Option<Scene>> = const { RefCell::new(None) };
}

/// Takes the scene queued by the last `load_model` call, if any.
pub(crate) fn take_pending_scene() -> Option<Scene> {
    PENDING.with(|pending| pending.borrow_mut().take())
}

#[wasm_bindgen(start)]
pub fn start() {
    if let Err(err) = crate::run(Scene::builtin_cube()) {
        web_sys::console::error_1(&err.to_string().into());
    }
}

/// Parses an MDX file and shows it in place of the current model. Geometry only: the model's
/// own textures are not loaded, so the built-in texture is used. The change takes effect on
/// the next frame.
#[wasm_bindgen]
pub fn load_model(bytes: &[u8]) -> Result<(), JsError> {
    let model = mdx::Model::parse(bytes).map_err(|e| JsError::new(&e.to_string()))?;
    let scene = Scene::from_model(&model, None).map_err(|e| JsError::new(&e.to_string()))?;
    PENDING.with(|pending| *pending.borrow_mut() = Some(scene));
    Ok(())
}
