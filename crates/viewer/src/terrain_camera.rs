//! WC3-like terrain camera: a target on the ground, a yaw around +Z, an angle from the vertical,
//! and a distance. Left drag rotates, right drag pans, the wheel zooms.
//!
//! Defaults follow the editor's default view as far as known: yaw 304 degrees, 34 degrees from
//! vertical, 70 degrees vertical field of view. The 34 degree value is the task's
//! specification and is an assumption (the editor's own number may differ). GPU-free, unit
//! tested.

use glam::{camera, Mat4, Vec3};

use crate::scene::Aabb;

/// Vertical field of view in degrees.
pub const FOV_Y_DEG: f32 = 70.0;
/// Default yaw (rotation around +Z) in degrees.
pub const DEFAULT_YAW_DEG: f32 = 304.0;
/// Default angle between the view direction and the vertical, in degrees.
pub const DEFAULT_FROM_VERTICAL_DEG: f32 = 34.0;
/// Smallest and largest allowed angle from vertical, in degrees.
const MIN_FROM_VERTICAL_DEG: f32 = 1.0;
const MAX_FROM_VERTICAL_DEG: f32 = 85.0;
/// Radians of yaw or angle per pixel of drag.
const ROTATE_SPEED: f32 = 0.005;
/// Pan distance per pixel, as a fraction of the camera distance.
const PAN_SPEED: f32 = 0.0015;
/// Zoom rate per wheel line (exponential).
const ZOOM_RATE: f32 = 0.1;

/// The terrain camera. `eye = target + distance * (sin(a) cos(yaw), sin(a) sin(yaw), cos(a))`
/// where `a` is `from_vertical`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainCamera {
    pub target: Vec3,
    pub distance: f32,
    /// Radians around +Z.
    pub yaw: f32,
    /// Radians between the view direction and +Z.
    pub from_vertical: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl TerrainCamera {
    /// A camera at the default angles that frames the whole box.
    pub fn fit(bounds: &Aabb, aspect: f32) -> TerrainCamera {
        let radius = bounds.radius().max(1.0);
        // The sphere must fit in the narrower of the vertical and horizontal half-angles.
        let half_v = FOV_Y_DEG.to_radians() * 0.5;
        let half_h = (half_v.tan() * aspect.min(1.0)).atan();
        let distance = radius / half_v.min(half_h).sin() * 1.05;
        TerrainCamera {
            target: bounds.center(),
            distance,
            yaw: DEFAULT_YAW_DEG.to_radians(),
            from_vertical: DEFAULT_FROM_VERTICAL_DEG.to_radians(),
            aspect,
            near: distance * 0.02,
            far: distance + radius * 4.0,
            min_distance: radius * 0.05,
            max_distance: distance * 4.0,
        }
    }

    /// Eye position in world space.
    pub fn eye(&self) -> Vec3 {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_v, cos_v) = self.from_vertical.sin_cos();
        self.target + self.distance * Vec3::new(sin_v * cos_yaw, sin_v * sin_yaw, cos_v)
    }

    pub fn view(&self) -> Mat4 {
        camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Z)
    }

    /// Projection with depth in `[0, 1]`, as wgpu expects.
    pub fn projection(&self) -> Mat4 {
        camera::rh::proj::directx::perspective(
            FOV_Y_DEG.to_radians(),
            self.aspect,
            self.near,
            self.far,
        )
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection() * self.view()
    }

    /// Left drag: `dx` yaws, `dy` changes the angle from vertical.
    pub fn rotate(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * ROTATE_SPEED;
        let min = MIN_FROM_VERTICAL_DEG.to_radians();
        let max = MAX_FROM_VERTICAL_DEG.to_radians();
        self.from_vertical = (self.from_vertical + dy * ROTATE_SPEED).clamp(min, max);
    }

    /// Right drag, in pixels. The ground under the cursor follows the mouse: dragging right
    /// moves the camera left, dragging down moves it forward.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        // Ground-plane right vector of the view, and the ground-plane direction the camera
        // looks along.
        let right = Vec3::new(-sin_yaw, cos_yaw, 0.0);
        let forward = Vec3::new(-cos_yaw, -sin_yaw, 0.0);
        let scale = self.distance * PAN_SPEED;
        self.target += right * (-dx * scale) + forward * (dy * scale);
    }

    /// Wheel zoom. Positive `lines` moves closer to the target.
    pub fn zoom(&mut self, lines: f32) {
        let factor = (-ZOOM_RATE * lines).exp();
        self.distance = (self.distance * factor).clamp(self.min_distance, self.max_distance);
    }

    pub fn set_aspect(&mut self, aspect: f32) {
        self.aspect = aspect;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Aabb;

    const EPS: f32 = 1e-3;

    fn map_bounds() -> Aabb {
        Aabb {
            min: Vec3::new(-512.0, -512.0, -40.0),
            max: Vec3::new(512.0, 512.0, 300.0),
        }
    }

    fn project(vp: Mat4, p: Vec3) -> Option<Vec3> {
        let clip = vp * glam::Vec4::from((p, 1.0));
        (clip.w > 0.0).then(|| clip.truncate() / clip.w)
    }

    #[test]
    fn defaults_are_the_wc3_angles() {
        let cam = TerrainCamera::fit(&map_bounds(), 16.0 / 9.0);
        assert!((cam.yaw.to_degrees() - 304.0).abs() < 1e-3);
        assert!((cam.from_vertical.to_degrees() - 34.0).abs() < 1e-3);
        assert!((FOV_Y_DEG - 70.0).abs() < f32::EPSILON);
        // Looking down: the eye is above the target.
        assert!(cam.eye().z > cam.target.z);
        assert!(((cam.eye() - cam.target).length() - cam.distance).abs() < EPS);
    }

    #[test]
    fn fitted_camera_contains_the_whole_box() {
        let bounds = map_bounds();
        for aspect in [0.5_f32, 1.0, 16.0 / 9.0] {
            let cam = TerrainCamera::fit(&bounds, aspect);
            for corner in [
                Vec3::new(bounds.min.x, bounds.min.y, bounds.min.z),
                Vec3::new(bounds.max.x, bounds.min.y, bounds.min.z),
                Vec3::new(bounds.min.x, bounds.max.y, bounds.min.z),
                Vec3::new(bounds.max.x, bounds.max.y, bounds.max.z),
            ] {
                let ndc = project(cam.view_projection(), corner)
                    .expect("corner must be in front of the eye");
                assert!(ndc.z >= -EPS && ndc.z <= 1.0 + EPS, "z {ndc:?}");
                assert!(ndc.x.abs() <= 1.0 + EPS && ndc.y.abs() <= 1.0 + EPS, "ndc {ndc:?}");
            }
        }
    }

    #[test]
    fn rotate_clamps_the_angle_and_wraps_yaw() {
        let mut cam = TerrainCamera::fit(&map_bounds(), 1.0);
        cam.rotate(0.0, 1.0e6);
        assert!((cam.from_vertical.to_degrees() - 85.0).abs() < 1e-3);
        cam.rotate(0.0, -1.0e6);
        assert!((cam.from_vertical.to_degrees() - 1.0).abs() < 1e-3);
        let yaw = cam.yaw;
        cam.rotate(100.0, 0.0);
        assert!(cam.yaw > yaw, "yaw is not clamped");
    }

    #[test]
    fn pan_moves_on_the_ground_plane_only() {
        let mut cam = TerrainCamera::fit(&map_bounds(), 1.0);
        let before = cam.target;
        cam.pan(40.0, -25.0);
        assert!((cam.target.z - before.z).abs() < EPS);
        assert!((cam.target - before).length() > 1.0);
    }

    #[test]
    fn pan_right_moves_the_camera_left() {
        let mut cam = TerrainCamera::fit(&map_bounds(), 1.0);
        cam.yaw = 0.0;
        let before = cam.target;
        cam.pan(10.0, 0.0);
        // With yaw 0 the view's right vector is +Y, so dragging right moves the target to -Y.
        assert!(cam.target.y < before.y);
        assert!((cam.target.x - before.x).abs() < EPS);
    }

    #[test]
    fn zoom_is_clamped_and_direction_correct() {
        let mut cam = TerrainCamera::fit(&map_bounds(), 1.0);
        let d = cam.distance;
        cam.zoom(5.0);
        assert!(cam.distance < d, "positive lines zoom in");
        cam.zoom(-1.0e6);
        assert!((cam.distance - cam.max_distance).abs() < EPS);
        cam.zoom(1.0e6);
        assert!((cam.distance - cam.min_distance).abs() < EPS);
    }

    #[test]
    fn depth_runs_from_zero_at_near_to_one_at_far() {
        let cam = TerrainCamera::fit(&map_bounds(), 1.0);
        let forward = (cam.target - cam.eye()).normalize();
        let at_near = cam.eye() + forward * cam.near;
        let at_far = cam.eye() + forward * cam.far;
        let vp = cam.view_projection();
        assert!(project(vp, at_near).unwrap().z.abs() < EPS);
        assert!((project(vp, at_far).unwrap().z - 1.0).abs() < EPS);
    }
}
