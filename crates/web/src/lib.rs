//! Headless wasm32 entry point for the simulation stack (no rendering, no file system).
//!
//! JavaScript passes the bytes it has (the map, the game archives, and optionally the user's
//! `Game.dll`); the crate boots the engine the same way `w3boot` does and exposes the report as text.
//!
//! ```js
//! import init, { boot, step, report } from "./web.js";
//! await init();
//! console.log(boot(mapBytes, [war3Mpq, war3xMpq], gameDllBytes)); // gameDllBytes may be undefined
//! console.log(step(10));   // 10 batches of 20 game ticks
//! console.log(report());
//! ```
//!
//! The engine lives in a thread-local slot: wasm is single-threaded, and the engine holds `Rc` state.
//! Build: `cargo build -p web --target wasm32-unknown-unknown --release` (see docs/wasm.md).

use std::cell::RefCell;

use engine::{Engine, Inputs};
use wasm_bindgen::prelude::*;

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

fn js_err(message: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&message.to_string())
}

/// Boots a map. `mpqs` are the game archives in load order (`War3.mpq`, `War3x.mpq`, ...; the later ones
/// override, so a patch goes last). `game_dll` is optional: without it the soft-float tables and the RNG
/// table are missing, as in `w3boot` without `--dll`. Returns the boot report. A second call replaces the
/// engine.
#[wasm_bindgen]
pub fn boot(map: &[u8], mpqs: js_sys::Array, game_dll: Option<Vec<u8>>) -> Result<String, JsValue> {
    let archives: Vec<engine::FileBytes> = mpqs
        .iter()
        .map(|v| js_sys::Uint8Array::new(&v).to_vec().into())
        .collect();
    let refs: Vec<&[u8]> = archives.iter().map(|a| &a[..]).collect();
    let (common_j, blizzard_j) = engine::scripts_from_archives(&refs).map_err(js_err)?;
    let mut ai = engine::ai_scripts_from_archives(&refs).map_err(js_err)?;
    let inputs = Inputs {
        map: map.to_vec(),
        game_dll,
        common_j,
        blizzard_j,
        rng_seed: 0,
        ai_common: ai.common.take(),
        ai_scripts: ai.scripts,
        game_archives: archives,
        check_natives: false,
    };
    let engine = Engine::boot(inputs).map_err(js_err)?;
    let text = engine.report().to_string();
    ENGINE.with(|slot| *slot.borrow_mut() = Some(engine));
    Ok(text)
}

/// Runs `n` batches (each one is 20 game ticks, see `engine::Engine::step`). Returns a one-line status.
#[wasm_bindgen]
pub fn step(n: u32) -> Result<String, JsValue> {
    ENGINE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let engine = slot.as_mut().ok_or_else(|| js_err("step: no map booted"))?;
        for _ in 0..n {
            engine.step();
        }
        let r = engine.report();
        Ok(format!(
            "steps={} ticks={} timers_fired={} game_time={:.3}",
            r.steps, r.ticks, r.timers_fired, r.game_time
        ))
    })
}

/// The full report of the booted engine.
#[wasm_bindgen]
pub fn report() -> Result<String, JsValue> {
    ENGINE.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|e| e.report().to_string())
            .ok_or_else(|| js_err("report: no map booted"))
    })
}
