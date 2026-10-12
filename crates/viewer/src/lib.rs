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
#[cfg(not(target_arch = "wasm32"))]
pub mod cliff_render;
#[cfg(not(target_arch = "wasm32"))]
pub mod offscreen;
pub mod hud;
pub mod minimap;
pub mod model;
pub mod props;
pub mod renderer;
pub mod scene;
pub mod scene_objects;
#[cfg(not(target_arch = "wasm32"))]
pub mod sim_view;
pub mod terrain;
pub mod terrain_camera;
pub mod terrain_gpu;
pub mod terrain_objects;
pub mod ui;
#[cfg(target_arch = "wasm32")]
mod web;

pub use app::{run, run_model, run_terrain};
