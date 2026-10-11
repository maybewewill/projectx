//! Minimal model viewer for MDX models.
//!
//! * [`scene`]: GPU-independent. MDX to mesh buffers, bounding box, orbit camera, BLP to RGBA.
//!   Unit-tested.
//! * [`terrain`], [`terrain_camera`]: map terrain from `war3map.w3e` (mesh, ground atlas,
//!   camera). GPU-independent and unit-tested, like [`scene`].
//! * [`renderer`] and [`app`]: wgpu and winit, native and wasm32 (WebGPU, or WebGL2 fallback).
//!   [`terrain_gpu`] holds the terrain pipelines.
//! * [`web`]: wasm-bindgen entry points (wasm32 only).

pub mod app;
pub mod model;
pub mod renderer;
pub mod scene;
pub mod terrain;
pub mod terrain_camera;
pub mod terrain_gpu;
#[cfg(target_arch = "wasm32")]
mod web;

pub use app::{run, run_model, run_terrain};
