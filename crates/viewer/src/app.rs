//! Window, input and frame loop, shared by the native binary and the browser build.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use mdx::glam::Vec3;
use mdx::BillboardCamera;
use winit::keyboard::{Key, NamedKey};

use crate::model::gpu::ModelGpu;
use crate::model::{Clock, Command, ModelAsset, ModelView};
use crate::renderer::Renderer;
use crate::scene::{OrbitCamera, Scene};

/// Pixels of wheel delta that count as one line of zoom (for touchpads that report pixels).
const PIXELS_PER_LINE: f64 = 40.0;

/// Opens a window and runs the viewer until it is closed.
pub fn run(scene: Scene) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    run_app(event_loop, App::new(scene))
}

/// Opens a window showing an animated model, starting on the sequence named `initial` (or
/// "Stand"). Keys: space or `p` pause, `[` and `]` step, `s` `w` `a` `d` pick stand, walk,
/// attack and death.
pub fn run_model(
    asset: ModelAsset,
    initial: Option<&str>,
) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(Scene::builtin_cube());
    app.set_model(asset, initial);
    run_app(event_loop, app)
}

/// An animated model on screen, with the GPU objects once a device exists.
struct ModelState {
    view: ModelView,
    gpu: Option<ModelGpu>,
    clock: Clock,
}

/// Text of a key press, for the viewer's key bindings.
fn key_text(key: &Key) -> Option<&str> {
    match key {
        Key::Named(NamedKey::Space) => Some(" "),
        Key::Character(text) => Some(text.as_str()),
        _ => None,
    }
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
    dragging: bool,
    last_cursor: Option<(f64, f64)>,
    /// Set by `run_model`: the animated model replaces the static scene.
    model: Option<ModelState>,
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
            dragging: false,
            last_cursor: None,
            model: None,
        }
    }

    fn set_model(&mut self, asset: ModelAsset, initial: Option<&str>) {
        self.camera = OrbitCamera::fit(&asset.bounds, self.aspect);
        self.model = Some(ModelState {
            view: ModelView::new(asset, initial),
            gpu: None,
            clock: Clock::default(),
        });
    }

    fn model_frame(&mut self) {
        let slot = self.renderer.borrow();
        let Some(renderer) = slot.as_ref() else {
            return;
        };
        let Some(state) = self.model.as_mut() else {
            return;
        };
        if state.gpu.is_none() {
            state.gpu = Some(ModelGpu::new(
                renderer.device(),
                renderer.queue(),
                renderer.format(),
                &state.view.asset,
            ));
        }
        let forward = self.camera.target - self.camera.eye();
        let billboard = BillboardCamera::from_view(forward, Vec3::Z);
        let dt = state.clock.tick();
        let frame = state.view.frame(dt, billboard.as_ref());
        if let Some(gpu) = &state.gpu {
            renderer.render_model(gpu, self.camera.view_projection(), &frame);
        }
    }

    fn set_model_title(&self) {
        if let (Some(window), Some(state)) = (&self.window, &self.model) {
            window.set_title(&format!("projectx viewer - {}", state.view.name()));
        }
    }

    fn key(&mut self, key: &Key) {
        let Some(text) = key_text(key) else {
            return;
        };
        let Some(command) = Command::from_key(&text.to_ascii_lowercase()) else {
            return;
        };
        let Some(state) = self.model.as_mut() else {
            return;
        };
        if state.view.apply(command) {
            self.set_model_title();
        }
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
        if self.model.is_some() {
            self.model_frame();
            return;
        }
        let mut slot = self.renderer.borrow_mut();
        let Some(renderer) = slot.as_mut() else {
            return;
        };
        if self.needs_upload {
            renderer.upload_scene(&self.scene);
            self.needs_upload = false;
        }
        renderer.render(self.camera.view_projection());
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
                if button == MouseButton::Left {
                    self.dragging = state == ElementState::Pressed;
                    self.last_cursor = None;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let current = (position.x, position.y);
                if self.dragging {
                    if let Some((last_x, last_y)) = self.last_cursor {
                        self.camera
                            .orbit((current.0 - last_x) as f32, (current.1 - last_y) as f32);
                    }
                }
                self.last_cursor = Some(current);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / PIXELS_PER_LINE) as f32,
                };
                self.camera.zoom(lines);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat {
                    self.key(&event.logical_key);
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
