//! Animated MDX models: the CPU asset (skinning layout, textures, bind-pose meshes), sequence
//! playback, per-frame data (palette and sorted draw calls), and the wgpu path that draws them.
//!
//! The CPU parts are unit tested without a GPU. `gpu` needs a device and is only compiled
//! into the normal build; it has not been run in the environment where this was written.

pub mod asset;
pub mod draw;
pub mod gpu;
pub mod playback;

pub use asset::{AssetError, ModelAsset, TextureSource};
pub use draw::{build_frame, BlendMode, DrawCall, ModelFrame};
pub use playback::{Clock, Command, Playback};

use mdx::BillboardCamera;

/// The model on screen: its asset and where playback is. The app creates the GPU objects
/// (`gpu::ModelGpu`) once a device exists.
pub struct ModelView {
    pub asset: ModelAsset,
    pub playback: Playback,
}

impl ModelView {
    /// Starts playback on `initial` if it names a sequence, else on "Stand".
    pub fn new(asset: ModelAsset, initial: Option<&str>) -> ModelView {
        let mut playback = Playback::new(&asset.model);
        if let Some(name) = initial {
            playback.select_named(&asset.model, name);
        }
        ModelView { asset, playback }
    }

    /// Applies a command. Returns true when the sequence changed.
    pub fn apply(&mut self, command: Command) -> bool {
        self.playback.apply(&self.asset.model, command)
    }

    /// Name of the current sequence.
    pub fn name(&self) -> &str {
        self.playback.name(&self.asset.model)
    }

    /// Advances playback by `dt_ms` and builds the frame for the current time. `billboard`
    /// is the camera basis for billboarded nodes.
    pub fn frame(&mut self, dt_ms: f32, billboard: Option<&BillboardCamera>) -> ModelFrame {
        self.playback.advance(dt_ms);
        let time = self.playback.time(&self.asset.model);
        let pose = self
            .asset
            .skeleton
            .evaluate_at(&self.asset.model, time, billboard);
        build_frame(&self.asset, &pose)
    }
}

#[cfg(test)]
mod tests {
    use glam::{Mat4, Vec3};
    use mdx::{
        Extent, Geoset, Interpolation, Key, Layer, Material, Model, ModelInfo, Node, Primitive,
        Sequence, Track,
    };

    use super::*;
    use crate::scene::TextureData;

    fn extent() -> Extent {
        Extent {
            radius: 0.0,
            min: [0.0; 3],
            max: [0.0; 3],
        }
    }

    fn layer(filter_mode: u32, texture_id: u32, alpha_track: Option<Track<f32>>) -> Layer {
        Layer {
            filter_mode,
            shading_flags: 0,
            texture_id,
            texture_anim_id: None,
            coord_id: 0,
            alpha: 1.0,
            alpha_track,
            texture_track: None,
        }
    }

    fn bone(object_id: i32, translation: Option<Track<[f32; 3]>>) -> mdx::Bone {
        mdx::Bone {
            node: Node {
                name: format!("bone{object_id}"),
                object_id,
                parent_id: -1,
                flags: 0x100,
                translation,
                rotation: None,
                scaling: None,
                visibility: None,
            },
            geoset_id: -1,
            geoset_anim_id: -1,
        }
    }

    /// One triangle skinned to two bones: bone 1 moves +2 in x, bone 2 stays. The palette
    /// entry for both is their average, which moves +1 in x.
    fn sample_model() -> Model {
        let moving = Track {
            interpolation: Interpolation::Linear,
            global_sequence: None,
            keys: vec![Key {
                time: 0,
                value: [2.0, 0.0, 0.0],
                in_tangent: None,
                out_tangent: None,
            }],
        };
        let kmta = Track {
            interpolation: Interpolation::Linear,
            global_sequence: None,
            keys: vec![Key {
                time: 0,
                value: 0.5f32,
                in_tangent: None,
                out_tangent: None,
            }],
        };
        Model {
            version: 800,
            info: ModelInfo {
                name: "t".into(),
                extent: extent(),
                blend_time: 150,
            },
            sequences: vec![Sequence {
                name: "Stand".into(),
                start: 0,
                end: 1000,
                move_speed: 0.0,
                flags: 0,
                rarity: 0.0,
                sync_point: 0,
                extent: extent(),
            }],
            global_sequences: Vec::new(),
            textures: vec![
                mdx::Texture {
                    replaceable_id: 0,
                    path: "a.blp".into(),
                    unknown: 0,
                    flags: 0,
                },
                mdx::Texture {
                    replaceable_id: 0,
                    path: "b.blp".into(),
                    unknown: 0,
                    flags: 0,
                },
            ],
            materials: vec![Material {
                priority_plane: 0,
                flags: 0,
                layers: vec![
                    layer(0, 0, None),
                    layer(3, 1, Some(kmta)),
                ],
            }],
            geosets: vec![Geoset {
                vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: Vec::new(),
                primitives: vec![Primitive {
                    kind: mdx::PRIMITIVE_TRIANGLES,
                    indices: vec![0, 1, 2],
                }],
                vertex_groups: vec![0, 0, 0],
                matrix_groups: vec![2],
                matrices: vec![1, 2],
                material_id: 0,
                selection_group: 0,
                selection_flags: 0,
                extent: extent(),
                sequence_extents: Vec::new(),
                uv_sets: Vec::new(),
            }],
            bones: vec![bone(1, Some(moving)), bone(2, None)],
            helpers: Vec::new(),
            pivots: vec![[0.0; 3], [0.0; 3]],
            geoset_anims: Vec::new(),
            unknown_chunks: Vec::new(),
        }
    }

    fn asset() -> ModelAsset {
        ModelAsset::new(sample_model(), |path| {
            Some(TextureData {
                width: 1,
                height: 1,
                rgba: vec![path.len() as u8, 0, 0, 255],
            })
        })
        .unwrap()
    }

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }

    #[test]
    fn palette_averages_the_group_bones() {
        let mut view = ModelView::new(asset(), None);
        let frame = view.frame(0.0, None);
        assert_eq!(frame.palette.len(), 1);
        let p = frame.palette[0].transform_point3(Vec3::ZERO);
        close(p.x, 1.0);
        close(p.y, 0.0);
        close(p.z, 0.0);
    }

    #[test]
    fn bind_pose_frame_has_identity_palette() {
        let mut a = asset();
        a.model.sequences.clear();
        let mut view = ModelView::new(a, None);
        let frame = view.frame(0.0, None);
        assert_eq!(frame.palette[0], Mat4::IDENTITY);
    }

    #[test]
    fn draws_are_sorted_opaque_first_and_carry_layer_alpha() {
        let mut view = ModelView::new(asset(), None);
        let frame = view.frame(0.0, None);
        assert_eq!(frame.draws.len(), 2);
        assert_eq!(frame.draws[0].mode, BlendMode::Opaque);
        assert_eq!(frame.draws[0].texture, 0);
        assert_eq!(frame.draws[1].mode, BlendMode::Additive);
        assert_eq!(frame.draws[1].texture, 1);
        close(frame.draws[0].color[3], 1.0);
        close(frame.draws[1].color[3], 0.5);
    }

    #[test]
    fn unknown_texture_index_uses_the_fallback_slot() {
        let a = asset();
        assert_eq!(a.texture_index(99), a.fallback_index());
        assert_eq!(a.texture_index(1), 1);
    }

    #[test]
    fn missing_texture_is_marked_and_replaceable_ids_get_placeholders() {
        let mut m = sample_model();
        m.textures.push(mdx::Texture {
            replaceable_id: 1,
            path: String::new(),
            unknown: 0,
            flags: 0,
        });
        let a = ModelAsset::new(m, |_| None).unwrap();
        assert_eq!(a.textures[0].source, TextureSource::Missing);
        assert_eq!(a.textures[2].source, TextureSource::TeamColor);
        assert_eq!(a.textures[3].source, TextureSource::Fallback);
    }

    #[test]
    fn rigid_geosets_use_the_bone_that_names_them() {
        let mut m = sample_model();
        m.geosets[0].matrix_groups.clear();
        m.geosets[0].matrices.clear();
        m.bones[1].geoset_id = 0;
        let a = ModelAsset::new(m, |_| None).unwrap();
        assert_eq!(a.layout.palette, vec![vec![2]]);
    }

    #[test]
    fn playback_steps_wraps_and_pauses() {
        let mut m = sample_model();
        m.sequences.push(Sequence {
            name: "Walk".into(),
            start: 0,
            end: 500,
            move_speed: 0.0,
            flags: 0,
            rarity: 0.0,
            sync_point: 0,
            extent: extent(),
        });
        let mut p = Playback::new(&m);
        assert_eq!(p.sequence(), Some(0));
        assert!(p.apply(&m, Command::Next));
        assert_eq!(p.sequence(), Some(1));
        assert!(p.apply(&m, Command::Next));
        assert_eq!(p.sequence(), Some(0));
        assert!(p.apply(&m, Command::Select("walk")));
        assert_eq!(p.name(&m), "Walk");
        assert!(!p.apply(&m, Command::Select("attack")));
        p.advance(100.0);
        assert_eq!(p.time(&m), Some(100));
        p.apply(&m, Command::TogglePause);
        p.advance(100.0);
        assert_eq!(p.time(&m), Some(100));
        assert!(p.is_paused());
    }

    #[test]
    fn key_bindings_map_to_commands() {
        assert_eq!(Command::from_key("p"), Some(Command::TogglePause));
        assert_eq!(Command::from_key(" "), Some(Command::TogglePause));
        assert_eq!(Command::from_key("["), Some(Command::Previous));
        assert_eq!(Command::from_key("w"), Some(Command::Select("walk")));
        assert_eq!(Command::from_key("x"), None);
    }

    #[test]
    fn empty_model_plays_at_the_bind_pose() {
        let mut m = sample_model();
        m.sequences.clear();
        let p = Playback::new(&m);
        assert_eq!(p.sequence(), None);
        assert_eq!(p.name(&m), "bind pose");
        assert_eq!(p.time(&m), None);
    }
}
