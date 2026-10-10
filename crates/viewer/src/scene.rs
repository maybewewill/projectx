//! GPU-independent scene preparation.
//!
//! Converts an [`mdx::Model`] into CPU mesh buffers, computes the bounding box, builds an
//! orbit camera around it, and turns a [`blp::Blp`] texture (mip 0) into RGBA8 pixels. Nothing
//! here touches wgpu, so all of it is unit-tested.
//!
//! Coordinates are used as stored in the file. WC3 models are Z-up, so the orbit camera uses
//! +Z as its up axis.

use blp::Blp;
use glam::{camera, Mat4, Vec3, Vec4};
use mdx::Model;

/// Normal used for vertices whose geoset has no normal for them.
pub const DEFAULT_NORMAL: [f32; 3] = [0.0, 0.0, 1.0];

/// Errors while preparing a scene.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error("model has no triangles")]
    NoGeometry,
    #[error("triangle index {index} is outside a geoset with {vertices} vertices")]
    IndexOutOfRange { index: u16, vertices: usize },
    #[error("texture size {width}x{height} is zero or does not match {len} bytes")]
    TextureSize { width: u32, height: u32, len: usize },
    #[error("texture: {0}")]
    Blp(#[from] blp::Error),
}

/// One geoset as flat CPU buffers. `positions`, `normals` and `uvs` have the same length;
/// `indices` index into them and are always below that length.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
    /// Index into the model's MTLS chunk. Informational only for now.
    pub material_id: u32,
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// Box around a set of points, or `None` when the set is empty.
    pub fn from_points(points: impl IntoIterator<Item = [f32; 3]>) -> Option<Aabb> {
        let mut iter = points.into_iter().map(Vec3::from_array);
        let first = iter.next()?;
        let (min, max) = iter.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Some(Aabb { min, max })
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Radius of the sphere around the center that contains the box.
    pub fn radius(&self) -> f32 {
        (self.max - self.min).length() * 0.5
    }

    /// The eight corners of the box.
    pub fn corners(&self) -> [Vec3; 8] {
        let (lo, hi) = (self.min, self.max);
        [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
            Vec3::new(hi.x, hi.y, hi.z),
        ]
    }
}

/// RGBA8 texture, tightly packed, row-major, `rgba.len() == width * height * 4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl TextureData {
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, SceneError> {
        let expected = (width as usize) * (height as usize) * 4;
        if width == 0 || height == 0 || rgba.len() != expected {
            return Err(SceneError::TextureSize {
                width,
                height,
                len: rgba.len(),
            });
        }
        Ok(TextureData {
            width,
            height,
            rgba,
        })
    }

    /// Decodes mip level 0 of a BLP.
    pub fn from_blp(blp: &Blp) -> Result<Self, SceneError> {
        let (width, height, rgba) = blp.mip_rgba(0)?;
        Self::from_rgba(width, height, rgba)
    }

    /// The built-in 2x2 texture: four solid colors, so every face of the cube shows all of them.
    pub fn builtin() -> Self {
        let rgba = vec![
            230, 60, 60, 255, // red
            60, 200, 80, 255, // green
            60, 110, 230, 255, // blue
            240, 210, 60, 255, // yellow
        ];
        TextureData {
            width: 2,
            height: 2,
            rgba,
        }
    }
}

/// Everything the renderer needs: meshes, one texture, and the bounds used to frame the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub meshes: Vec<Mesh>,
    pub texture: TextureData,
    pub bounds: Aabb,
}

impl Scene {
    /// Builds a scene from a parsed model. Without a texture the built-in one is used.
    pub fn from_model(model: &Model, texture: Option<TextureData>) -> Result<Scene, SceneError> {
        let meshes = meshes_from_model(model)?;
        let bounds = Aabb::from_points(meshes.iter().flat_map(|m| m.positions.iter().copied()))
            .ok_or(SceneError::NoGeometry)?;
        Ok(Scene {
            meshes,
            texture: texture.unwrap_or_else(TextureData::builtin),
            bounds,
        })
    }

    /// A unit cube centered on the origin, with the built-in texture on every face.
    pub fn builtin_cube() -> Scene {
        Scene {
            meshes: vec![cube_mesh()],
            texture: TextureData::builtin(),
            bounds: Aabb {
                min: Vec3::splat(-0.5),
                max: Vec3::splat(0.5),
            },
        }
    }
}

/// Converts every geoset that has triangles into a [`Mesh`]. Geosets without triangles are
/// skipped. Missing normals become [`DEFAULT_NORMAL`]; missing UVs become zero. Only the first
/// UV set is used.
pub fn meshes_from_model(model: &Model) -> Result<Vec<Mesh>, SceneError> {
    let mut out = Vec::new();
    for geoset in &model.geosets {
        let indices: Vec<u16> = geoset.triangles().into_iter().flatten().collect();
        if indices.is_empty() {
            continue;
        }
        let n = geoset.vertices.len();
        if let Some(&index) = indices.iter().find(|&&i| usize::from(i) >= n) {
            return Err(SceneError::IndexOutOfRange { index, vertices: n });
        }
        let normals = if geoset.normals.len() == n {
            geoset.normals.clone()
        } else {
            vec![DEFAULT_NORMAL; n]
        };
        let uvs = match geoset.uv_sets.first() {
            Some(set) if set.len() == n => set.clone(),
            _ => vec![[0.0, 0.0]; n],
        };
        out.push(Mesh {
            positions: geoset.vertices.clone(),
            normals,
            uvs,
            indices,
            material_id: geoset.material_id,
        });
    }
    Ok(out)
}

fn cube_mesh() -> Mesh {
    // (normal, four corners counter-clockwise seen from outside).
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
                [0.5, -0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, 0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, -0.5],
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, -0.5, -0.5],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
            ],
        ),
    ];
    const FACE_UVS: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

    let mut mesh = Mesh {
        positions: Vec::with_capacity(24),
        normals: Vec::with_capacity(24),
        uvs: Vec::with_capacity(24),
        indices: Vec::with_capacity(36),
        material_id: 0,
    };
    for (face, (normal, corners)) in FACES.iter().enumerate() {
        let base = (face * 4) as u16;
        for (corner, uv) in corners.iter().zip(FACE_UVS) {
            mesh.positions.push(*corner);
            mesh.normals.push(*normal);
            mesh.uvs.push(uv);
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh
}

/// Field of view (vertical), 45 degrees.
const FOV_Y: f32 = std::f32::consts::FRAC_PI_4;
/// Pitch limit, just under 90 degrees, so the view never degenerates.
const MAX_PITCH: f32 = 1.5;
/// Radians per pixel of mouse drag.
const ORBIT_SPEED: f32 = 0.01;
/// Zoom factor per wheel line: distance is multiplied by `exp(-ZOOM_RATE * lines)`.
const ZOOM_RATE: f32 = 0.1;

/// Orbit camera around a target point. Up is +Z.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub distance: f32,
    /// Rotation around +Z, in radians.
    pub yaw: f32,
    /// Elevation above the XY plane, in radians, within `[-MAX_PITCH, MAX_PITCH]`.
    pub pitch: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl OrbitCamera {
    /// A camera that frames the bounding sphere of `bounds` with a small margin.
    pub fn fit(bounds: &Aabb, aspect: f32) -> OrbitCamera {
        let radius = bounds.radius().max(1e-3);
        // The sphere must fit in the narrower of the two half-angles (vertical and horizontal).
        let half_angle = ((FOV_Y * 0.5).tan() * aspect.min(1.0)).atan();
        let distance = radius / half_angle.sin() * 1.1;
        OrbitCamera {
            target: bounds.center(),
            distance,
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: 0.3,
            aspect,
            near: distance * 0.01,
            far: distance + radius * 2.0,
            min_distance: radius * 0.1,
            max_distance: distance * 10.0,
        }
    }

    /// Eye position in world space.
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + self.distance * Vec3::new(cp * cy, cp * sy, sp)
    }

    pub fn view(&self) -> Mat4 {
        camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Z)
    }

    /// Projection with depth in `[0, 1]`, as wgpu expects (glam's `directx` convention).
    pub fn projection(&self) -> Mat4 {
        camera::rh::proj::directx::perspective(FOV_Y, self.aspect, self.near, self.far)
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection() * self.view()
    }

    /// Mouse drag in pixels. Positive `dx` yaws right, positive `dy` raises the camera.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * ORBIT_SPEED;
        self.pitch = (self.pitch + dy * ORBIT_SPEED).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Wheel zoom. Positive `lines` moves closer to the target.
    pub fn zoom(&mut self, lines: f32) {
        let factor = (-ZOOM_RATE * lines).exp();
        self.distance = (self.distance * factor).clamp(self.min_distance, self.max_distance);
    }

    /// Updates the aspect ratio, keeping the clip planes.
    pub fn set_aspect(&mut self, aspect: f32) {
        self.aspect = aspect;
    }
}

/// Projects a world point to normalized device coordinates (x and y in `[-1, 1]` on screen,
/// z in `[0, 1]`). Returns `None` when the point is behind the eye.
pub fn project_ndc(view_projection: Mat4, point: Vec3) -> Option<Vec3> {
    let clip = view_projection * Vec4::from((point, 1.0));
    if clip.w <= 0.0 {
        return None;
    }
    Some(clip.truncate() / clip.w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdx::{Extent, Geoset, ModelInfo, Primitive, PRIMITIVE_TRIANGLES};

    const EPS: f32 = 1e-4;

    fn assert_close(a: f32, b: f32) {
        assert!((a - b).abs() < EPS, "{a} != {b}");
    }

    fn geoset(vertices: Vec<[f32; 3]>, primitives: Vec<Primitive>) -> Geoset {
        let extent = Extent {
            radius: 0.0,
            min: [0.0; 3],
            max: [0.0; 3],
        };
        Geoset {
            vertices,
            normals: Vec::new(),
            primitives,
            vertex_groups: Vec::new(),
            matrix_groups: Vec::new(),
            matrices: Vec::new(),
            material_id: 3,
            selection_group: 0,
            selection_flags: 0,
            extent,
            sequence_extents: Vec::new(),
            uv_sets: Vec::new(),
        }
    }

    fn triangles(indices: &[u16]) -> Primitive {
        Primitive {
            kind: PRIMITIVE_TRIANGLES,
            indices: indices.to_vec(),
        }
    }

    /// A model with two geosets: a quad (two triangles) with normals and UVs, and a second
    /// geoset whose only primitive is a non-triangle kind (must be skipped).
    fn sample_model() -> Model {
        let mut quad = geoset(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 2.0, 0.0],
                [0.0, 2.0, 0.0],
            ],
            vec![triangles(&[0, 1, 2, 0, 2, 3])],
        );
        quad.normals = vec![[0.0, 0.0, 1.0]; 4];
        quad.uv_sets = vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]];
        quad.material_id = 7;

        let lines = geoset(
            vec![[5.0, 5.0, 5.0], [6.0, 5.0, 5.0]],
            vec![Primitive {
                kind: 1,
                indices: vec![0, 1],
            }],
        );

        Model {
            version: 800,
            info: ModelInfo {
                name: "test".to_owned(),
                extent: Extent {
                    radius: 0.0,
                    min: [0.0; 3],
                    max: [0.0; 3],
                },
                blend_time: 150,
            },
            sequences: Vec::new(),
            global_sequences: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            geosets: vec![quad, lines],
            bones: Vec::new(),
            helpers: Vec::new(),
            pivots: Vec::new(),
            unknown_chunks: Vec::new(),
        }
    }

    #[test]
    fn meshes_keep_geosets_with_triangles_only() {
        let meshes = meshes_from_model(&sample_model()).unwrap();
        assert_eq!(meshes.len(), 1);
        let m = &meshes[0];
        assert_eq!(m.positions.len(), 4);
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(m.material_id, 7);
    }

    #[test]
    fn mesh_normals_and_uvs_pass_through() {
        let m = &meshes_from_model(&sample_model()).unwrap()[0];
        assert_eq!(m.normals, vec![[0.0, 0.0, 1.0]; 4]);
        assert_eq!(m.uvs[2], [1.0, 1.0]);
        assert_eq!(m.uvs.len(), m.positions.len());
    }

    #[test]
    fn missing_normals_and_uvs_get_defaults() {
        let g = geoset(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![triangles(&[0, 1, 2])],
        );
        let model = Model {
            geosets: vec![g],
            ..sample_model()
        };
        let m = &meshes_from_model(&model).unwrap()[0];
        assert_eq!(m.normals, vec![DEFAULT_NORMAL; 3]);
        assert_eq!(m.uvs, vec![[0.0, 0.0]; 3]);
    }

    #[test]
    fn index_outside_vertex_range_is_an_error() {
        let g = geoset(vec![[0.0; 3]; 2], vec![triangles(&[0, 1, 2])]);
        let model = Model {
            geosets: vec![g],
            ..sample_model()
        };
        let err = meshes_from_model(&model).unwrap_err();
        assert!(matches!(
            err,
            SceneError::IndexOutOfRange {
                index: 2,
                vertices: 2
            }
        ));
    }

    #[test]
    fn aabb_from_points() {
        let b = Aabb::from_points([[1.0, -2.0, 3.0], [-1.0, 4.0, 0.0], [0.5, 0.0, 1.0]]).unwrap();
        assert_eq!(b.min, Vec3::new(-1.0, -2.0, 0.0));
        assert_eq!(b.max, Vec3::new(1.0, 4.0, 3.0));
        assert_eq!(b.center(), Vec3::new(0.0, 1.0, 1.5));
        assert_close(b.radius(), Vec3::new(2.0, 6.0, 3.0).length() * 0.5);
        assert!(Aabb::from_points(std::iter::empty()).is_none());
    }

    #[test]
    fn scene_bounds_cover_all_meshes() {
        let scene = Scene::from_model(&sample_model(), None).unwrap();
        assert_eq!(scene.bounds.min, Vec3::ZERO);
        assert_eq!(scene.bounds.max, Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(scene.texture, TextureData::builtin());
    }

    #[test]
    fn model_without_triangles_has_no_geometry() {
        let model = Model {
            geosets: vec![geoset(vec![[0.0; 3]], Vec::new())],
            ..sample_model()
        };
        assert!(matches!(
            Scene::from_model(&model, None),
            Err(SceneError::NoGeometry)
        ));
    }

    #[test]
    fn builtin_cube_is_a_closed_unit_cube() {
        let cube = Scene::builtin_cube();
        let m = &cube.meshes[0];
        assert_eq!(m.positions.len(), 24);
        assert_eq!(m.indices.len(), 36);
        assert!(m.indices.iter().all(|&i| usize::from(i) < 24));
        assert!(m.positions.iter().all(|p| p.iter().all(|c| c.abs() == 0.5)));
        assert_eq!(cube.bounds.min, Vec3::splat(-0.5));
        assert_eq!(cube.bounds.max, Vec3::splat(0.5));
    }

    #[test]
    fn texture_from_rgba_checks_length() {
        assert!(TextureData::from_rgba(2, 2, vec![0; 16]).is_ok());
        assert!(matches!(
            TextureData::from_rgba(2, 2, vec![0; 15]),
            Err(SceneError::TextureSize { .. })
        ));
        assert!(matches!(
            TextureData::from_rgba(0, 4, Vec::new()),
            Err(SceneError::TextureSize { .. })
        ));
    }

    /// A 2x2 paletted BLP without alpha and without mip-maps, using four palette entries.
    fn paletted_blp() -> Vec<u8> {
        const HEADER_LEN: usize = 0x9C;
        const PALETTE_LEN: usize = 256 * 4;
        let mut file = vec![0u8; HEADER_LEN + PALETTE_LEN];
        file[0..4].copy_from_slice(b"BLP1");
        file[4..8].copy_from_slice(&1u32.to_le_bytes()); // paletted
        file[8..12].copy_from_slice(&0u32.to_le_bytes()); // no alpha
        file[12..16].copy_from_slice(&2u32.to_le_bytes());
        file[16..20].copy_from_slice(&2u32.to_le_bytes());
        let mip_offset = (HEADER_LEN + PALETTE_LEN) as u32;
        file[0x1C..0x20].copy_from_slice(&mip_offset.to_le_bytes());
        file[0x5C..0x60].copy_from_slice(&4u32.to_le_bytes());
        // BGRA palette: index 0 = red, 1 = green, 2 = blue, 3 = white.
        let palette: [[u8; 4]; 4] = [
            [0, 0, 255, 255],
            [0, 255, 0, 255],
            [255, 0, 0, 255],
            [255, 255, 255, 255],
        ];
        for (i, bgra) in palette.iter().enumerate() {
            let at = HEADER_LEN + i * 4;
            file[at..at + 4].copy_from_slice(bgra);
        }
        file.extend_from_slice(&[0, 1, 2, 3]);
        file
    }

    #[test]
    fn blp_mip0_becomes_rgba() {
        let blp = Blp::parse(&paletted_blp()).unwrap();
        let tex = TextureData::from_blp(&blp).unwrap();
        assert_eq!((tex.width, tex.height), (2, 2));
        assert_eq!(
            tex.rgba,
            vec![
                255, 0, 0, 255, // index 0: red
                0, 255, 0, 255, // index 1: green
                0, 0, 255, 255, // index 2: blue
                255, 255, 255, 255, // index 3: white
            ]
        );
    }

    #[test]
    fn center_projects_to_screen_center() {
        let bounds = Aabb {
            min: Vec3::new(-3.0, 1.0, 2.0),
            max: Vec3::new(5.0, 4.0, 9.0),
        };
        let cam = OrbitCamera::fit(&bounds, 16.0 / 9.0);
        let ndc = project_ndc(cam.view_projection(), bounds.center()).unwrap();
        assert_close(ndc.x, 0.0);
        assert_close(ndc.y, 0.0);
    }

    #[test]
    fn fitted_camera_contains_the_whole_box() {
        let bounds = Aabb {
            min: Vec3::new(-10.0, -4.0, 0.0),
            max: Vec3::new(10.0, 4.0, 30.0),
        };
        for aspect in [0.5_f32, 1.0, 16.0 / 9.0, 3.0] {
            let cam = OrbitCamera::fit(&bounds, aspect);
            for yaw in [0.0_f32, 1.0, 2.5, 4.0] {
                for pitch in [-1.0_f32, 0.0, 0.3, 1.2] {
                    let mut c = cam;
                    c.yaw = yaw;
                    c.pitch = pitch;
                    for corner in bounds.corners() {
                        let ndc = project_ndc(c.view_projection(), corner)
                            .expect("corner must be in front of the eye");
                        assert!(ndc.x.abs() <= 1.0 + EPS, "x {ndc:?} aspect {aspect}");
                        assert!(ndc.y.abs() <= 1.0 + EPS, "y {ndc:?} aspect {aspect}");
                        assert!((-EPS..=1.0 + EPS).contains(&ndc.z), "z {ndc:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn depth_runs_from_zero_at_near_to_one_at_far() {
        let bounds = Aabb {
            min: Vec3::splat(-1.0),
            max: Vec3::splat(1.0),
        };
        let cam = OrbitCamera::fit(&bounds, 1.0);
        let forward = (cam.target - cam.eye()).normalize();
        let at_near = cam.eye() + forward * cam.near;
        let at_far = cam.eye() + forward * cam.far;
        assert_close(project_ndc(cam.view_projection(), at_near).unwrap().z, 0.0);
        assert_close(project_ndc(cam.view_projection(), at_far).unwrap().z, 1.0);
    }

    #[test]
    fn eye_is_at_distance_from_target() {
        let mut cam = OrbitCamera::fit(&Aabb::from_points([[0.0; 3], [2.0; 3]]).unwrap(), 1.0);
        cam.yaw = 0.7;
        cam.pitch = -0.4;
        assert_close((cam.eye() - cam.target).length(), cam.distance);
    }

    #[test]
    fn orbit_clamps_pitch_and_wraps_yaw_freely() {
        let mut cam = OrbitCamera::fit(&Aabb::from_points([[0.0; 3], [1.0; 3]]).unwrap(), 1.0);
        cam.orbit(0.0, 1.0e6);
        assert_close(cam.pitch, MAX_PITCH);
        cam.orbit(0.0, -1.0e6);
        assert_close(cam.pitch, -MAX_PITCH);
        let yaw = cam.yaw;
        cam.orbit(100.0, 0.0);
        assert_close(cam.yaw, yaw + 100.0 * ORBIT_SPEED);
    }

    #[test]
    fn zoom_moves_closer_and_clamps() {
        let mut cam = OrbitCamera::fit(&Aabb::from_points([[0.0; 3], [1.0; 3]]).unwrap(), 1.0);
        let start = cam.distance;
        cam.zoom(1.0);
        assert!(cam.distance < start);
        cam.zoom(-1.0);
        assert_close(cam.distance, start);
        cam.zoom(1.0e6);
        assert_close(cam.distance, cam.min_distance);
        cam.zoom(-1.0e6);
        assert_close(cam.distance, cam.max_distance);
    }

    #[test]
    fn point_behind_eye_is_not_projected() {
        let cam = OrbitCamera::fit(&Aabb::from_points([[0.0; 3], [1.0; 3]]).unwrap(), 1.0);
        let behind = cam.eye() + (cam.eye() - cam.target).normalize() * 1.0;
        assert!(project_ndc(cam.view_projection(), behind).is_none());
    }
}
