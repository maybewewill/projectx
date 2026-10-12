//! The HUD minimap (`viewer --hud RACE`): the map image fitted into the ConsoleUI minimap frame,
//! a dot per live simulation unit (player colour, buildings larger) and a white outline of the
//! camera's view on the ground.
//!
//! Presentation only. The world-to-pixel mapping is a plain linear fit of the playable area, so it
//! is not a game rule.

use glam::{Mat4, Vec3, Vec4};

use crate::scene_objects::{player_colour, SimUnit};
use crate::terrain_camera::TerrainCamera;

/// Key of the minimap texture in `UiScene::textures`. Starts with a control character, so no FDF
/// texture name can clash with it.
pub const MINIMAP_KEY: &str = "\u{1}minimap";

/// An axis-aligned world rectangle (playable area or camera footprint), in world units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldRect {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// Where the world rectangle lands inside a `width x height` pixel image. The map keeps its aspect
/// ratio: it is scaled to fit and centred, and the rest of the image is left empty (black).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub world: WorldRect,
    pub width: u32,
    pub height: u32,
    /// Pixels per world unit (the same on both axes).
    pub scale: f32,
    /// Pixel position of the world's top-left corner (min x, max y).
    pub left: f32,
    pub top: f32,
}

impl Fit {
    /// Fits `world` into the image. A degenerate world (zero width or height) is widened to one unit
    /// so the mapping stays finite.
    pub fn new(world: WorldRect, width: u32, height: u32) -> Fit {
        let w = (world.max_x - world.min_x).max(1.0);
        let h = (world.max_y - world.min_y).max(1.0);
        let (iw, ih) = (width.max(1) as f32, height.max(1) as f32);
        let scale = (iw / w).min(ih / h);
        Fit {
            world: WorldRect {
                min_x: world.min_x,
                min_y: world.min_y,
                max_x: world.min_x + w,
                max_y: world.min_y + h,
            },
            width,
            height,
            scale,
            left: (iw - w * scale) * 0.5,
            top: (ih - h * scale) * 0.5,
        }
    }

    /// Pixel position of a world point: x grows to the right, y grows downwards (north is up).
    pub fn world_to_px(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.left + (x - self.world.min_x) * self.scale,
            self.top + (self.world.max_y - y) * self.scale,
        )
    }

    /// The map area inside the image, in pixels: `(left, top, right, bottom)`.
    pub fn map_px(&self) -> (f32, f32, f32, f32) {
        let (w, h) = (self.world.max_x - self.world.min_x, self.world.max_y - self.world.min_y);
        (self.left, self.top, self.left + w * self.scale, self.top + h * self.scale)
    }
}

/// The map picture the minimap draws under the dots: RGBA8, top row = north, covering the whole
/// playable area (see [`Fit`]).
pub struct MinimapBase {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl MinimapBase {
    /// A flat colour base (used when the map has no `war3mapMap.blp`).
    pub fn flat(width: u32, height: u32, rgb: [u8; 3]) -> MinimapBase {
        let (width, height) = (width.max(1), height.max(1));
        let rgba = [rgb[0], rgb[1], rgb[2], 255].repeat((width * height) as usize);
        MinimapBase { width, height, rgba }
    }
}

/// Draws one minimap image of `fit.width x fit.height` pixels, RGBA8, top row first.
///
/// `base` is sampled over the map area of `fit`; units are dots (2 px, buildings 4 px) in their
/// owner's colour; `camera` is a white outline. `is_building` tells buildings from units by type id.
pub fn render(
    base: &MinimapBase,
    fit: &Fit,
    units: &[SimUnit],
    is_building: &dyn Fn(u32) -> bool,
    camera: Option<WorldRect>,
) -> Vec<u8> {
    let (w, h) = (fit.width as usize, fit.height as usize);
    let mut out = vec![0u8; w * h * 4];
    for px in out.chunks_exact_mut(4) {
        px[3] = 255;
    }

    // Base: nearest-neighbour sample of the map picture over the map area.
    let (l, t, r, b) = fit.map_px();
    let x0 = l.floor().max(0.0) as usize;
    let y0 = t.floor().max(0.0) as usize;
    let x1 = (r.ceil().max(0.0) as usize).min(w);
    let y1 = (b.ceil().max(0.0) as usize).min(h);
    let (bw, bh) = (base.width as usize, base.height as usize);
    for py in y0..y1 {
        for px in x0..x1 {
            let u = ((px as f32 + 0.5 - l) / (r - l)).clamp(0.0, 0.999_999);
            let v = ((py as f32 + 0.5 - t) / (b - t)).clamp(0.0, 0.999_999);
            let sx = (u * bw as f32) as usize;
            let sy = (v * bh as f32) as usize;
            let s = (sy * bw + sx) * 4;
            let d = (py * w + px) * 4;
            if let Some(src) = base.rgba.get(s..s + 3) {
                out[d..d + 3].copy_from_slice(src);
            }
        }
    }

    // Units: a square dot per live unit, in the owner's colour.
    for u in units.iter().filter(|u| !u.hidden && !u.dead && !u.removed) {
        let (cx, cy) = fit.world_to_px(u.x, u.y);
        let half = if is_building(u.type_id) { 2.0 } else { 1.0 };
        fill_square(&mut out, w, h, cx, cy, half, player_colour(u.owner));
    }

    // Camera: a white outline of the ground rectangle the view covers.
    if let Some(cam) = camera {
        let (ax, ay) = fit.world_to_px(cam.min_x, cam.max_y);
        let (bx, by) = fit.world_to_px(cam.max_x, cam.min_y);
        outline(&mut out, w, h, (ax, ay), (bx, by), [255, 255, 255]);
    }
    out
}

fn fill_square(out: &mut [u8], w: usize, h: usize, cx: f32, cy: f32, half: f32, rgb: [u8; 3]) {
    let x0 = (cx - half).floor().max(0.0) as usize;
    let y0 = (cy - half).floor().max(0.0) as usize;
    let x1 = ((cx + half).ceil().max(0.0) as usize).min(w);
    let y1 = ((cy + half).ceil().max(0.0) as usize).min(h);
    for py in y0..y1 {
        for px in x0..x1 {
            let d = (py * w + px) * 4;
            out[d..d + 3].copy_from_slice(&rgb);
        }
    }
}

/// One-pixel outline of the rectangle with corners `a` and `b`, clipped to the image.
fn outline(out: &mut [u8], w: usize, h: usize, a: (f32, f32), b: (f32, f32), rgb: [u8; 3]) {
    let x0 = a.0.min(b.0).round() as i64;
    let x1 = a.0.max(b.0).round() as i64;
    let y0 = a.1.min(b.1).round() as i64;
    let y1 = a.1.max(b.1).round() as i64;
    let mut put = |x: i64, y: i64| {
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
            let d = (y as usize * w + x as usize) * 4;
            out[d..d + 3].copy_from_slice(&rgb);
        }
    };
    for x in x0..=x1 {
        put(x, y0);
        put(x, y1);
    }
    for y in y0..=y1 {
        put(x0, y);
        put(x1, y);
    }
}

/// The ground rectangle (on the plane `z = ground_z`) that the camera's view covers: the bounding box
/// of the four screen corners projected onto that plane. `None` when a corner misses the plane in
/// front of the camera.
pub fn camera_footprint(camera: &TerrainCamera, ground_z: f32) -> Option<WorldRect> {
    let inverse = camera.view_projection().inverse();
    let corners = [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let mut rect = WorldRect {
        min_x: f32::INFINITY,
        min_y: f32::INFINITY,
        max_x: f32::NEG_INFINITY,
        max_y: f32::NEG_INFINITY,
    };
    for (nx, ny) in corners {
        let p = ray_to_plane(&inverse, nx, ny, ground_z)?;
        rect.min_x = rect.min_x.min(p.x);
        rect.max_x = rect.max_x.max(p.x);
        rect.min_y = rect.min_y.min(p.y);
        rect.max_y = rect.max_y.max(p.y);
    }
    Some(rect)
}

/// Intersects the ray through NDC point `(nx, ny)` with the plane `z = plane_z`. Depth runs 0..1
/// (wgpu). Returns `None` for a ray parallel to the plane or a hit behind the eye.
fn ray_to_plane(inverse: &Mat4, nx: f32, ny: f32, plane_z: f32) -> Option<Vec3> {
    let near = *inverse * Vec4::new(nx, ny, 0.0, 1.0);
    let far = *inverse * Vec4::new(nx, ny, 1.0, 1.0);
    let near = near.truncate() / near.w;
    let far = far.truncate() / far.w;
    let dir = far - near;
    if dir.z.abs() < 1e-9 {
        return None;
    }
    let t = (plane_z - near.z) / dir.z;
    if t.is_nan() || t <= 0.0 {
        return None;
    }
    Some(near + dir * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> WorldRect {
        WorldRect { min_x: -4000.0, min_y: -2000.0, max_x: 4000.0, max_y: 2000.0 }
    }

    fn unit(type_id: u32, owner: u32, x: f32, y: f32) -> SimUnit {
        SimUnit {
            type_id,
            owner,
            x,
            y,
            facing: 0.0,
            scale: 1.0,
            hidden: false,
            dead: false,
            removed: false,
        }
    }

    #[test]
    fn world_corners_map_to_the_fitted_map_corners() {
        // 800x400 image: the 8000x4000 world fills it exactly (scale 0.1).
        let fit = Fit::new(world(), 800, 400);
        assert!((fit.scale - 0.1).abs() < 1e-6);
        assert_eq!(fit.world_to_px(-4000.0, 2000.0), (0.0, 0.0));
        let (x, y) = fit.world_to_px(4000.0, -2000.0);
        assert!((x - 800.0).abs() < 1e-3 && (y - 400.0).abs() < 1e-3);
        let (cx, cy) = fit.world_to_px(0.0, 0.0);
        assert!((cx - 400.0).abs() < 1e-3 && (cy - 200.0).abs() < 1e-3);
    }

    #[test]
    fn north_is_up_and_east_is_right() {
        let fit = Fit::new(world(), 800, 400);
        let (_, north) = fit.world_to_px(0.0, 1000.0);
        let (_, south) = fit.world_to_px(0.0, -1000.0);
        let (west, _) = fit.world_to_px(-1000.0, 0.0);
        let (east, _) = fit.world_to_px(1000.0, 0.0);
        assert!(north < south);
        assert!(west < east);
    }

    #[test]
    fn a_tall_frame_letterboxes_the_map_and_keeps_its_aspect() {
        // 8000x4000 world in a 200x200 image: scale 0.025, the map is 200 wide and 100 high, centred.
        let fit = Fit::new(world(), 200, 200);
        assert!((fit.scale - 0.025).abs() < 1e-6);
        let (l, t, r, b) = fit.map_px();
        assert_eq!((l, r), (0.0, 200.0));
        assert!((t - 50.0).abs() < 1e-4 && (b - 150.0).abs() < 1e-4);
    }

    #[test]
    fn unit_dot_and_camera_outline_are_drawn_in_place() {
        let fit = Fit::new(world(), 80, 40);
        let base = MinimapBase::flat(4, 4, [10, 20, 30]);
        let units = [unit(0x6862_7261, 1, 0.0, 0.0)];
        let cam = WorldRect { min_x: -1000.0, min_y: -500.0, max_x: 1000.0, max_y: 500.0 };
        let img = render(&base, &fit, &units, &|_| false, Some(cam));
        assert_eq!(img.len(), 80 * 40 * 4);
        // The dot at the world centre (40, 20) is player 1's colour.
        let d = (20 * 80 + 40) * 4;
        assert_eq!(&img[d..d + 3], &player_colour(1));
        // The outline's corner (world -1000, 500) is white.
        let (ax, ay) = fit.world_to_px(-1000.0, 500.0);
        let e = (ay.round() as usize * 80 + ax.round() as usize) * 4;
        assert_eq!(&img[e..e + 3], &[255, 255, 255]);
        // Away from the outline and the dot the base colour shows.
        let far = (2 * 80 + 2) * 4;
        assert_eq!(&img[far..far + 3], &[10, 20, 30]);
    }

    #[test]
    fn buildings_are_drawn_larger_and_dead_units_are_not_drawn() {
        let fit = Fit::new(world(), 80, 40);
        let base = MinimapBase::flat(1, 1, [0, 0, 0]);
        let building = [unit(0x6862_7261, 0, 0.0, 0.0)];
        let img = render(&base, &fit, &building, &|id| id == 0x6862_7261, None);
        // The building is a 4 px square centred on (40, 20): x 38..42. Pixel 41 is inside, 44 is not.
        let inside = (20 * 80 + 41) * 4;
        assert_eq!(&img[inside..inside + 3], &player_colour(0));
        let outside = (20 * 80 + 44) * 4;
        assert_eq!(&img[outside..outside + 3], &[0, 0, 0]);
        let mut dead = unit(0x6862_7261, 0, 0.0, 0.0);
        dead.dead = true;
        let img = render(&base, &fit, &[dead], &|_| false, None);
        assert_eq!(&img[(20 * 80 + 40) * 4..(20 * 80 + 40) * 4 + 3], &[0, 0, 0]);
    }

    #[test]
    fn camera_footprint_of_a_top_down_view_is_centred_on_the_target() {
        let bounds = crate::scene::Aabb {
            min: Vec3::new(-4000.0, -2000.0, 0.0),
            max: Vec3::new(4000.0, 2000.0, 0.0),
        };
        let mut cam = TerrainCamera::fit(&bounds, 16.0 / 9.0);
        cam.target = Vec3::new(500.0, -300.0, 0.0);
        cam.from_vertical = 0.001;
        cam.yaw = 0.0;
        let rect = camera_footprint(&cam, 0.0).expect("the view hits the ground");
        let cx = (rect.min_x + rect.max_x) * 0.5;
        let cy = (rect.min_y + rect.max_y) * 0.5;
        assert!((cx - 500.0).abs() < 5.0, "centre x {cx}");
        assert!((cy + 300.0).abs() < 5.0, "centre y {cy}");
        assert!(rect.max_x > rect.min_x && rect.max_y > rect.min_y);
    }
}
