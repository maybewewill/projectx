//! Minimal model viewer for MDX models.
//!
//! * [`scene`]: GPU-independent. MDX to mesh buffers, bounding box, orbit camera, BLP to RGBA.
//!   Unit-tested.
//! * [`renderer`] and [`app`]: wgpu and winit, native and wasm32 (WebGPU, or WebGL2 fallback).
//! * [`web`]: wasm-bindgen entry points (wasm32 only).

pub mod app;
pub mod model;
pub mod renderer;
pub mod scene;
#[cfg(target_arch = "wasm32")]
mod web;

pub use app::{run, run_model};
