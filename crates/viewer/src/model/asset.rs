//! CPU side of an animated model: the parsed file, its skeleton, the skinning palette layout,
//! textures for every TEXS entry, and bind-pose meshes. No wgpu here.

use std::collections::BTreeMap;

use mdx::{skin, Model, Skeleton, SkeletonError};

use crate::scene::{Aabb, TextureData};

/// Most matrices one frame can hold. A uniform block of 256 mat4 is 16 KiB, the WebGL2 minimum.
pub const MAX_PALETTE: usize = 256;

/// Errors while preparing an animated model.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("skeleton: {0}")]
    Skeleton(#[from] SkeletonError),
    #[error("model has no triangles")]
    NoGeometry,
    #[error("model needs {0} distinct matrix groups; at most {MAX_PALETTE} are supported")]
    TooManyMatrices(usize),
}

/// Where a texture slot's pixels came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureSource {
    /// Loaded from the file the TEXS path names.
    File,
    /// Team color placeholder (replaceable id 1).
    TeamColor,
    /// Team glow placeholder (replaceable id 2).
    TeamGlow,
    /// Any other replaceable id: neutral gray placeholder.
    OtherReplaceable,
    /// A file path that could not be loaded: magenta checker, so it is visible.
    Missing,
    /// Used when a layer names a texture the model does not have.
    Fallback,
}

/// One texture of the model, parallel to the TEXS chunk, followed by the fallback slot.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSlot {
    pub data: TextureData,
    pub source: TextureSource,
}

/// Skinning layout. A "palette entry" is one averaged matrix: the average of the skin matrices
/// of its bones. Identical bone lists share one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinLayout {
    /// Bone object ids of each palette entry.
    pub palette: Vec<Vec<i32>>,
    /// For each geoset, the palette index of each of its matrix groups (indexed by GNDX).
    pub geoset_groups: Vec<Vec<u32>>,
}

impl SkinLayout {
    /// Builds the layout from MTGC/MATS. A geoset without matrix groups is rigid: it follows the
    /// first bone whose `geoset_id` names it, or stays at the identity if there is none.
    pub fn build(model: &Model) -> Result<SkinLayout, AssetError> {
        let mut interned: BTreeMap<Vec<i32>, u32> = BTreeMap::new();
        let mut palette: Vec<Vec<i32>> = Vec::new();
        let mut geoset_groups = Vec::with_capacity(model.geosets.len());
        for (gi, geoset) in model.geosets.iter().enumerate() {
            let groups: Vec<Vec<i32>> = if geoset.matrix_groups.is_empty() {
                let bone = model
                    .bones
                    .iter()
                    .find(|b| b.geoset_id == gi as i32)
                    .map(|b| b.node.object_id);
                vec![bone.into_iter().collect()]
            } else {
                skin::matrix_groups(geoset)
                    .into_iter()
                    .map(|bones| bones.into_iter().map(|id| id as i32).collect())
                    .collect()
            };
            let mapped = groups
                .into_iter()
                .map(|bones| {
                    if let Some(&index) = interned.get(&bones) {
                        return index;
                    }
                    let index = palette.len() as u32;
                    interned.insert(bones.clone(), index);
                    palette.push(bones);
                    index
                })
                .collect();
            geoset_groups.push(mapped);
        }
        if palette.len() > MAX_PALETTE {
            return Err(AssetError::TooManyMatrices(palette.len()));
        }
        Ok(SkinLayout {
            palette,
            geoset_groups,
        })
    }
}

/// Bind-pose mesh of one geoset with triangles. `groups` holds the palette index of each
/// vertex, already resolved from GNDX.
#[derive(Debug, Clone, PartialEq)]
pub struct GeosetMesh {
    /// Index into `Model::geosets`.
    pub geoset: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub groups: Vec<u32>,
    pub indices: Vec<u16>,
}

/// Everything the viewer needs for an animated model except GPU objects.
pub struct ModelAsset {
    pub model: Model,
    pub skeleton: Skeleton,
    pub layout: SkinLayout,
    pub textures: Vec<TextureSlot>,
    pub meshes: Vec<GeosetMesh>,
    /// Bounds of the bind pose.
    pub bounds: Aabb,
    fallback: usize,
}

impl ModelAsset {
    /// Prepares `model`. `load_texture` is called once per TEXS file path (backslashes as in
    /// the file) and returns decoded pixels, or `None` to show the missing-texture checker.
    pub fn new(
        model: Model,
        mut load_texture: impl FnMut(&str) -> Option<TextureData>,
    ) -> Result<ModelAsset, AssetError> {
        let skeleton = Skeleton::new(&model)?;
        let layout = SkinLayout::build(&model)?;
        let meshes = build_meshes(&model, &layout);
        let bounds = Aabb::from_points(meshes.iter().flat_map(|m| m.positions.iter().copied()))
            .ok_or(AssetError::NoGeometry)?;

        let mut textures: Vec<TextureSlot> = model
            .textures
            .iter()
            .map(|t| {
                if t.replaceable_id != 0 {
                    replaceable_slot(t.replaceable_id)
                } else {
                    match load_texture(&t.path) {
                        Some(data) => TextureSlot {
                            data,
                            source: TextureSource::File,
                        },
                        None => TextureSlot {
                            data: missing_texture(),
                            source: TextureSource::Missing,
                        },
                    }
                }
            })
            .collect();
        textures.push(TextureSlot {
            data: TextureData {
                width: 1,
                height: 1,
                rgba: vec![255, 255, 255, 255],
            },
            source: TextureSource::Fallback,
        });
        let fallback = textures.len() - 1;

        Ok(ModelAsset {
            model,
            skeleton,
            layout,
            textures,
            meshes,
            bounds,
            fallback,
        })
    }

    /// Texture slot for a TEXS index. Out-of-range indices use the fallback slot.
    pub fn texture_index(&self, texture_id: u32) -> usize {
        let i = texture_id as usize;
        if i < self.fallback {
            i
        } else {
            self.fallback
        }
    }

    /// Index of the fallback texture slot.
    pub fn fallback_index(&self) -> usize {
        self.fallback
    }
}

fn build_meshes(model: &Model, layout: &SkinLayout) -> Vec<GeosetMesh> {
    let mut out = Vec::new();
    for (gi, geoset) in model.geosets.iter().enumerate() {
        let indices: Vec<u16> = geoset.triangles().into_iter().flatten().collect();
        if indices.is_empty() {
            continue;
        }
        let n = geoset.vertices.len();
        let normals = if geoset.normals.len() == n {
            geoset.normals.clone()
        } else {
            vec![[0.0, 0.0, 1.0]; n]
        };
        let uvs = match geoset.uv_sets.first() {
            Some(set) if set.len() == n => set.clone(),
            _ => vec![[0.0, 0.0]; n],
        };
        let rigid = geoset.matrix_groups.is_empty();
        let table = layout.geoset_groups.get(gi);
        let groups = (0..n)
            .map(|i| {
                if rigid {
                    return 0;
                }
                let gndx = geoset.vertex_groups.get(i).copied().map_or(0, usize::from);
                table.and_then(|t| t.get(gndx).copied()).unwrap_or(0)
            })
            .collect();
        out.push(GeosetMesh {
            geoset: gi,
            positions: geoset.vertices.clone(),
            normals,
            uvs,
            groups,
            indices,
        });
    }
    out
}

fn replaceable_slot(id: u32) -> TextureSlot {
    let (rgba, source) = match id {
        1 => (vec![230, 40, 40, 255], TextureSource::TeamColor),
        2 => (vec![255, 255, 255, 255], TextureSource::TeamGlow),
        _ => (vec![128, 128, 128, 255], TextureSource::OtherReplaceable),
    };
    TextureSlot {
        data: TextureData {
            width: 1,
            height: 1,
            rgba,
        },
        source,
    }
}

fn missing_texture() -> TextureData {
    TextureData {
        width: 2,
        height: 2,
        rgba: vec![
            255, 0, 255, 255, 0, 0, 0, 255, //
            0, 0, 0, 255, 255, 0, 255, 255,
        ],
    }
}
