//! Per-frame data: the matrix palette and the ordered list of draw calls. Pure CPU, unit
//! tested. The GPU side (`gpu.rs`) only uploads and replays it.

use mdx::glam::Mat4;
use mdx::skin;
use mdx::Pose;

use super::asset::ModelAsset;

/// Layer shading flag: bit 0 means unshaded (**assumption** from public notes).
pub const UNSHADED_FLAG: u32 = 0x1;

/// How a layer combines with what is already drawn. Raw values come from MDX layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// Raw 0 (none) and anything unknown: opaque.
    Opaque,
    /// Raw 1 (transparent): cut out texels below alpha 0.75.
    AlphaTest,
    /// Raw 2 (blend): source alpha over the background.
    Alpha,
    /// Raw 3 (additive): source alpha times source, added.
    Additive,
    /// Raw 4 (add alpha): source added without its alpha.
    AddAlpha,
    /// Raw 5 (modulate): multiply by the background.
    Modulate,
    /// Raw 6 (modulate 2x): multiply by twice the background (approximated).
    Modulate2x,
}

impl BlendMode {
    /// Every mode, in discriminant order (`index` matches this order).
    pub const ALL: [BlendMode; 7] = [
        BlendMode::Opaque,
        BlendMode::AlphaTest,
        BlendMode::Alpha,
        BlendMode::Additive,
        BlendMode::AddAlpha,
        BlendMode::Modulate,
        BlendMode::Modulate2x,
    ];

    pub fn from_raw(raw: u32) -> BlendMode {
        match raw {
            1 => BlendMode::AlphaTest,
            2 => BlendMode::Alpha,
            3 => BlendMode::Additive,
            4 => BlendMode::AddAlpha,
            5 => BlendMode::Modulate,
            6 => BlendMode::Modulate2x,
            _ => BlendMode::Opaque,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Opaque and cut-out layers write depth. Blended ones only read it.
    pub fn writes_depth(self) -> bool {
        matches!(self, BlendMode::Opaque | BlendMode::AlphaTest)
    }

    /// Texels with alpha below this are discarded.
    pub fn alpha_threshold(self) -> f32 {
        match self {
            BlendMode::AlphaTest => 0.75,
            _ => 0.0,
        }
    }
}

/// One draw: a geoset's mesh with one layer of its material.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawCall {
    /// Index into `ModelAsset::meshes`.
    pub mesh: usize,
    /// Index into `ModelAsset::textures`.
    pub texture: usize,
    pub mode: BlendMode,
    /// Multiplies the texel: RGB from the geoset color, A from geoset and layer alpha.
    pub color: [f32; 4],
    pub unshaded: bool,
    /// Material priority plane. Higher draws later within its group.
    pub priority: i32,
    /// Layer index inside the material (for stable ordering).
    pub layer: usize,
}

/// Everything the GPU needs for one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelFrame {
    /// Averaged matrix per palette entry.
    pub palette: Vec<Mat4>,
    /// Sorted: depth-writing draws first, then by priority plane, mesh, and layer.
    pub draws: Vec<DrawCall>,
}

/// Builds the frame for `pose`. The palette averages the skin matrices of each group's bones.
/// A bone missing from the pose counts as identity, so the average keeps the group's bone count.
pub fn build_frame(asset: &ModelAsset, pose: &Pose) -> ModelFrame {
    let palette = asset
        .layout
        .palette
        .iter()
        .map(|bones| {
            let mats: Vec<Mat4> = bones
                .iter()
                .map(|&id| pose.skin_matrix(id).unwrap_or(Mat4::IDENTITY))
                .collect();
            skin::average(&mats)
        })
        .collect();

    let model = &asset.model;
    let mut draws = Vec::new();
    for (mesh_index, mesh) in asset.meshes.iter().enumerate() {
        let geoset = &model.geosets[mesh.geoset];
        let (geo_alpha, geo_color) = match pose.geoset_state(mesh.geoset) {
            Some(state) => (state.alpha, state.color),
            None => (1.0, [1.0, 1.0, 1.0]),
        };
        let geo_alpha = geo_alpha.clamp(0.0, 1.0);
        if geo_alpha <= 0.0 {
            continue;
        }
        let material = model.materials.get(geoset.material_id as usize);
        let material_index = geoset.material_id as usize;
        match material {
            Some(mat) if !mat.layers.is_empty() => {
                for (li, layer) in mat.layers.iter().enumerate() {
                    let lp = pose
                        .materials
                        .get(material_index)
                        .and_then(|m| m.get(li))
                        .copied();
                    let (layer_alpha, texture_id) = match lp {
                        Some(lp) => (lp.alpha, lp.texture_id),
                        None => (layer.alpha, layer.texture_id),
                    };
                    let alpha = geo_alpha * layer_alpha.clamp(0.0, 1.0);
                    if alpha <= 0.0 {
                        continue;
                    }
                    draws.push(DrawCall {
                        mesh: mesh_index,
                        texture: asset.texture_index(texture_id),
                        mode: BlendMode::from_raw(layer.filter_mode),
                        color: [geo_color[0], geo_color[1], geo_color[2], alpha],
                        unshaded: layer.shading_flags & UNSHADED_FLAG != 0,
                        priority: mat.priority_plane,
                        layer: li,
                    });
                }
            }
            // No material or no layers: draw the mesh plainly with the fallback texture.
            _ => draws.push(DrawCall {
                mesh: mesh_index,
                texture: asset.fallback_index(),
                mode: BlendMode::Opaque,
                color: [geo_color[0], geo_color[1], geo_color[2], geo_alpha],
                unshaded: false,
                priority: 0,
                layer: 0,
            }),
        }
    }
    draws.sort_by_key(|d| (!d.mode.writes_depth(), d.priority, d.mesh, d.layer));
    ModelFrame { palette, draws }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_filter_modes_map_to_blend_modes() {
        let expected = [
            (0, BlendMode::Opaque),
            (1, BlendMode::AlphaTest),
            (2, BlendMode::Alpha),
            (3, BlendMode::Additive),
            (4, BlendMode::AddAlpha),
            (5, BlendMode::Modulate),
            (6, BlendMode::Modulate2x),
            (99, BlendMode::Opaque),
        ];
        for (raw, mode) in expected {
            assert_eq!(BlendMode::from_raw(raw), mode, "raw {raw}");
        }
    }

    #[test]
    fn index_follows_all_order() {
        for (i, mode) in BlendMode::ALL.iter().enumerate() {
            assert_eq!(mode.index(), i);
        }
    }

    #[test]
    fn only_opaque_and_cutout_write_depth_and_cutout_has_threshold() {
        for mode in BlendMode::ALL {
            let writes = matches!(mode, BlendMode::Opaque | BlendMode::AlphaTest);
            assert_eq!(mode.writes_depth(), writes);
        }
        assert_eq!(BlendMode::AlphaTest.alpha_threshold(), 0.75);
        assert_eq!(BlendMode::Alpha.alpha_threshold(), 0.0);
    }
}
