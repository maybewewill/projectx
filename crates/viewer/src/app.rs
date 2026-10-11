//! Window, input and frame loop, shared by the native binary and the browser build.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::renderer::Renderer;
use crate::scene::{OrbitCamera, Scene};
use crate::terrain::TerrainScene;
use crate::terrain_camera::TerrainCamera;

/// Pixels of wheel delta that count as one line of zoom (for touchpads that report pixels).
const PIXELS_PER_LINE: f64 = 40.0;

/// Opens a window and runs the viewer until it is closed.
pub fn run(scene: Scene) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    run_app(event_loop, App::new(scene))
}

/// Opens a window that shows a map's terrain instead of a model until it is closed.
pub fn run_terrain(terrain: TerrainScene) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    run_app(event_loop, App::new_terrain(terrain))
}

/// Terrain mode: the scene and its camera. The model scene is not drawn in this mode.
struct TerrainView {
    scene: TerrainScene,
    camera: TerrainCamera,
    uploaded: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn run_app(event_loop: EventLoop<()>, mut app: App) -> Result<(), winit::error::EventLoopError> {
    event_loop.run_app(&mut app)
}

#[cfg(target_arch = "wasm32")]
fn run_app(event_loop: EventLoop<()>, app: App) -> Result<(), winit::error::EventLoopError> {
    // winit's web backend keeps the handler alive after `run_app` returns, so the app must
    // outlive this function. Leaking it is the intended lifetime for a page-long viewer.
    event_loop.run_app(Box::leak(Box::new(app)))
}

struct App {
    scene: Scene,
    needs_upload: bool,
    camera: OrbitCamera,
    aspect: f32,
    window: Option<Arc<Window>>,
    // Filled by `Renderer::new`. On the web this happens later, asynchronously.
    renderer: Rc<RefCell<Option<Renderer>>>,
    /// Button held down during a drag: left orbits (or rotates the terrain), right pans.
    drag: Option<MouseButton>,
    last_cursor: Option<(f64, f64)>,
    terrain: Option<TerrainView>,
}

impl App {
    fn new(scene: Scene) -> App {
        let aspect = 1.0;
        App {
            camera: OrbitCamera::fit(&scene.bounds, aspect),
            scene,
            needs_upload: true,
            aspect,
            window: None,
            renderer: Rc::new(RefCell::new(None)),
            drag: None,
            last_cursor: None,
            terrain: None,
        }
    }

    fn new_terrain(terrain: TerrainScene) -> App {
        let aspect = 1.0;
        let camera = TerrainCamera::fit(&terrain.bounds, aspect);
        let mut app = App::new(Scene::builtin_cube());
        app.terrain = Some(TerrainView {
            scene: terrain,
            camera,
            uploaded: false,
        });
        app
    }

    #[cfg(target_arch = "wasm32")]
    fn set_scene(&mut self, scene: Scene) {
        self.camera = OrbitCamera::fit(&scene.bounds, self.aspect);
        self.scene = scene;
        self.needs_upload = true;
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.aspect = size.width as f32 / size.height as f32;
        self.camera.set_aspect(self.aspect);
        if let Some(terrain) = self.terrain.as_mut() {
            terrain.camera.set_aspect(self.aspect);
        }
        if let Some(renderer) = self.renderer.borrow_mut().as_mut() {
            renderer.resize(size.width, size.height);
        }
    }

    fn start_gpu(&self, window: Arc<Window>) {
        let slot = self.renderer.clone();
        let task = async move {
            match Renderer::new(window).await {
                Ok(renderer) => *slot.borrow_mut() = Some(renderer),
                Err(message) => log_error(&message),
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        pollster::block_on(task);
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(task);
    }

    fn frame(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(scene) = crate::web::take_pending_scene() {
            self.set_scene(scene);
        }
        let mut slot = self.renderer.borrow_mut();
        let Some(renderer) = slot.as_mut() else {
            return;
        };
        if self.needs_upload {
            renderer.upload_scene(&self.scene);
            self.needs_upload = false;
        }
        match self.terrain.as_mut() {
            Some(terrain) => {
                if !terrain.uploaded {
                    renderer.upload_terrain(&terrain.scene.mesh, &terrain.scene.atlas);
                    terrain.uploaded = true;
                }
                renderer.render(terrain.camera.view_projection(), false);
            }
            None => renderer.render(self.camera.view_projection(), true),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("projectx viewer")
            .with_inner_size(PhysicalSize::new(1024, 768));
        #[cfg(target_arch = "wasm32")]
        let attributes = {
            use winit::platform::web::WindowAttributesExtWebSys;
            attributes.with_append(true)
        };
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                log_error(&format!("cannot create window: {err}"));
                event_loop.exit();
                return;
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;
            if let Some(canvas) = window.canvas() {
                // Keep touch drags from scrolling the page instead of orbiting.
                let _ = canvas.style().set_property("touch-action", "none");
            }
        }
        let size = window.inner_size();
        self.resize(size);
        self.start_gpu(window.clone());
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resize(size),
            WindowEvent::MouseInput { state, button, .. } => {
                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    self.drag = (state == ElementState::Pressed).then_some(button);
                    self.last_cursor = None;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let current = (position.x, position.y);
                if let Some((last_x, last_y)) = self.last_cursor {
                    let dx = (current.0 - last_x) as f32;
                    let dy = (current.1 - last_y) as f32;
                    match (self.terrain.as_mut(), self.drag) {
                        (Some(terrain), Some(MouseButton::Left)) => terrain.camera.rotate(dx, dy),
                        (Some(terrain), Some(MouseButton::Right)) => terrain.camera.pan(dx, dy),
                        (None, Some(MouseButton::Left)) => self.camera.orbit(dx, dy),
                        _ => {}
                    }
                }
                self.last_cursor = Some(current);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / PIXELS_PER_LINE) as f32,
                };
                match self.terrain.as_mut() {
                    Some(terrain) => terrain.camera.zoom(lines),
                    None => self.camera.zoom(lines),
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Redraw continuously: the frame is vsync-limited, and this also picks up scenes that
        // arrive from JavaScript.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn log_error(message: &str) {
    eprintln!("viewer: {message}");
}

#[cfg(target_arch = "wasm32")]
fn log_error(message: &str) {
    web_sys::console::error_1(&message.into());
}
