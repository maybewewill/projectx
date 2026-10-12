//! Props: triangles that sample one texture each, for cliff walls and placed model objects.
//!
//! The terrain's own pipelines sample a ground atlas, which models cannot use: their UVs wrap
//! and their textures are separate files. Props keep a batch per (texture, blend mode), drawn
//! with the `fs_model*` stages of `terrain.wgsl`. Triangles whose texture cannot be read go to a
//! flat mesh, drawn with the flat colour pipeline like the rest of the terrain's flat geometry.
//!
//! Blending follows the layer's filter mode ([`PropBlend`]). Opaque and cut-out batches write
//! depth. Blended and additive batches come after them, with depth write off, sorted back to
//! front: triangles within a batch, and batches among themselves by mean depth. The order is
//! computed for one camera direction when the scene is built ([`PropScene::sort_back_to_front`]);
//! it is not recomputed when the camera orbits.
//!
//! Model files and BLP textures are read once per path and cached. Presentation only: the
//! textures are used at their first mip level, without mip-mapping.

use std::collections::{BTreeSet, HashMap};

use crate::model::draw::BlendMode;
use crate::scene::TextureData;
use crate::scene_objects::Archives;
use crate::terrain::{Mesh, TerrainVertex};

/// How a prop batch is drawn, from the filter mode of its layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropBlend {
    /// Filter mode none: opaque, writes depth.
    Opaque,
    /// Filter mode transparent: texels below alpha 0.5 are discarded, writes depth.
    Cutout,
    /// Filter mode blend (modulate is approximated the same way): alpha blended, no depth write.
    Alpha,
    /// Filter mode additive (add alpha too): added to the background, no depth write.
    Additive,
}

impl PropBlend {
    /// The draw mode of a layer's blend mode.
    pub fn from_mode(mode: BlendMode) -> PropBlend {
        match mode {
            BlendMode::Opaque => PropBlend::Opaque,
            BlendMode::AlphaTest => PropBlend::Cutout,
            BlendMode::Alpha | BlendMode::Modulate | BlendMode::Modulate2x => PropBlend::Alpha,
            BlendMode::Additive | BlendMode::AddAlpha => PropBlend::Additive,
        }
    }

    /// Blended batches are drawn after the depth-writing ones, back to front.
    pub fn is_blended(self) -> bool {
        matches!(self, PropBlend::Alpha | PropBlend::Additive)
    }
}

/// One texture's triangles in one blend mode.
#[derive(Debug, Clone, PartialEq)]
pub struct PropBatch {
    /// Index into [`PropScene::textures`].
    pub texture: usize,
    pub blend: PropBlend,
    pub mesh: Mesh,
}

/// Everything drawn with a model texture: the decoded textures and the batches.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PropScene {
    pub textures: Vec<TextureData>,
    pub batches: Vec<PropBatch>,
    /// Texture archive names that were asked for and could not be read.
    pub missing_textures: Vec<String>,
}

impl PropScene {
    pub fn triangle_count(&self) -> usize {
        self.batches.iter().map(|b| b.mesh.indices.len() / 3).sum()
    }

    /// Orders the batches for drawing: the depth-writing ones first (in build order), then the
    /// blended ones. Blended triangles are sorted far to near along `eye_dir`, the unit vector
    /// from the look-at point towards the eye; the blended batches by their mean depth.
    pub fn sort_back_to_front(&mut self, eye_dir: [f32; 3]) {
        let depth = |p: [f32; 3]| p[0] * eye_dir[0] + p[1] * eye_dir[1] + p[2] * eye_dir[2];
        let mut keyed: Vec<(f32, PropBatch)> = Vec::with_capacity(self.batches.len());
        for mut batch in std::mem::take(&mut self.batches) {
            let key = if batch.blend.is_blended() {
                sort_triangles_far_to_near(&mut batch.mesh, &depth);
                let vertices = batch.mesh.vertices.len().max(1) as f32;
                batch.mesh.vertices.iter().map(|v| depth(v.position)).sum::<f32>() / vertices
            } else {
                f32::NEG_INFINITY
            };
            keyed.push((key, batch));
        }
        // Stable sort: opaque batches (key -inf) keep their build order and come first; the
        // blended ones follow, far (small depth) to near.
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.batches = keyed.into_iter().map(|(_, batch)| batch).collect();
    }
}

/// Reorders the triangles of `mesh` by the mean `depth` of their corners, ascending (so the
/// farthest comes first). Vertices stay where they are; only the index list is permuted.
fn sort_triangles_far_to_near(mesh: &mut Mesh, depth: &impl Fn([f32; 3]) -> f32) {
    let mut keyed: Vec<(f32, [u32; 3])> = mesh
        .indices
        .chunks_exact(3)
        .map(|c| {
            let corner = |i: u32| depth(mesh.vertices[i as usize].position);
            ((corner(c[0]) + corner(c[1]) + corner(c[2])) / 3.0, [c[0], c[1], c[2]])
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    mesh.indices = keyed.into_iter().flat_map(|(_, tri)| tri).collect();
}

/// Reads models and textures through the archives, caching each path.
pub struct PropBuilder<'a, 'b> {
    archives: &'a Archives<'b>,
    models: HashMap<String, Option<mdx::Model>>,
    texture_ids: HashMap<String, Option<usize>>,
    textures: Vec<TextureData>,
    /// Index into `batches` of each (texture, blend) pair.
    batch_index: HashMap<(usize, PropBlend), usize>,
    batches: Vec<PropBatch>,
    flat: Mesh,
    /// Texture paths that were asked for and could not be read, sorted.
    pub missing_textures: BTreeSet<String>,
}

/// Archive name of a texture: the MDX files store texture names without an extension in some
/// tables, and BLP is the only format the viewer reads.
pub fn texture_archive_name(path: &str) -> String {
    let path = path.trim().replace('/', "\\");
    if path.to_ascii_lowercase().ends_with(".blp") {
        path
    } else {
        format!("{path}.blp")
    }
}

impl<'a, 'b> PropBuilder<'a, 'b> {
    pub fn new(archives: &'a Archives<'b>) -> Self {
        Self {
            archives,
            models: HashMap::new(),
            texture_ids: HashMap::new(),
            textures: Vec::new(),
            batch_index: HashMap::new(),
            batches: Vec::new(),
            flat: Mesh::default(),
            missing_textures: BTreeSet::new(),
        }
    }

    /// The model at `path` (an `.mdx` or `.mdl` name), read on first use. `None` when it is not
    /// in the archives or does not parse.
    pub fn model(&mut self, path: &str) -> Option<&mdx::Model> {
        if !self.models.contains_key(path) {
            let stem = [".mdl", ".mdx", ".MDL", ".MDX"]
                .iter()
                .find_map(|ext| path.strip_suffix(ext))
                .unwrap_or(path);
            let model = [format!("{stem}.mdx"), format!("{stem}.mdl")]
                .iter()
                .find_map(|p| {
                    let bytes = self.archives.read(p)?;
                    mdx::Model::parse(&bytes).ok()
                });
            self.models.insert(path.to_string(), model);
        }
        self.models.get(path).and_then(Option::as_ref)
    }

    /// Index of the texture at `path` in the scene, decoding it on first use. `None` when the
    /// archives do not have it or it does not decode (the path is then in `missing_textures`).
    pub fn texture(&mut self, path: &str) -> Option<usize> {
        if let Some(id) = self.texture_ids.get(path) {
            return *id;
        }
        let name = texture_archive_name(path);
        let decoded = self
            .archives
            .read(&name)
            .and_then(|bytes| blp::Blp::parse(&bytes).ok())
            .and_then(|blp| TextureData::from_blp(&blp).ok());
        let id = match decoded {
            Some(texture) => {
                self.textures.push(texture);
                Some(self.textures.len() - 1)
            }
            None => {
                self.missing_textures.insert(name);
                None
            }
        };
        self.texture_ids.insert(path.to_string(), id);
        id
    }

    /// Adds one cut-out triangle (the cliff walls). See [`PropBuilder::push_blend`].
    pub fn push(&mut self, texture: Option<usize>, tri: [TerrainVertex; 3]) {
        self.push_blend(texture, PropBlend::Cutout, tri);
    }

    /// Adds one triangle. With a texture it goes to the batch of that texture and blend mode
    /// (vertex UVs sampled from it); without one it goes to the flat mesh, drawn with the vertex
    /// colour.
    pub fn push_blend(&mut self, texture: Option<usize>, blend: PropBlend, tri: [TerrainVertex; 3]) {
        match texture {
            Some(t) => {
                let index = match self.batch_index.get(&(t, blend)) {
                    Some(&index) => index,
                    None => {
                        self.batches.push(PropBatch {
                            texture: t,
                            blend,
                            mesh: Mesh::default(),
                        });
                        self.batch_index.insert((t, blend), self.batches.len() - 1);
                        self.batches.len() - 1
                    }
                };
                self.batches[index].mesh.triangle(tri);
            }
            None => self.flat.triangle(tri),
        }
    }

    /// The flat mesh (triangles without a texture so far).
    pub fn flat_mesh_mut(&mut self) -> &mut Mesh {
        &mut self.flat
    }

    /// Splits the builder into the textured scene and the flat mesh.
    pub fn finish(self) -> (PropScene, Mesh) {
        let batches = self
            .batches
            .into_iter()
            .filter(|batch| !batch.mesh.is_empty())
            .collect();
        (
            PropScene {
                textures: self.textures,
                batches,
                missing_textures: self.missing_textures.into_iter().collect(),
            },
            self.flat,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(position: [f32; 3]) -> TerrainVertex {
        TerrainVertex {
            position,
            uv: [0.0, 0.0],
            cell: [0.0; 4],
            color: [1.0; 4],
        }
    }

    fn tri_at(z: f32) -> [TerrainVertex; 3] {
        [vertex([0.0, 0.0, z]), vertex([1.0, 0.0, z]), vertex([0.0, 1.0, z])]
    }

    #[test]
    fn texture_names_get_the_blp_extension_once() {
        assert_eq!(
            texture_archive_name("ReplaceableTextures\\Cliff\\Cliff0"),
            "ReplaceableTextures\\Cliff\\Cliff0.blp"
        );
        assert_eq!(texture_archive_name("Textures/Rock.BLP"), "Textures\\Rock.BLP");
    }

    #[test]
    fn filter_modes_map_to_draw_modes() {
        assert_eq!(PropBlend::from_mode(BlendMode::Opaque), PropBlend::Opaque);
        assert_eq!(PropBlend::from_mode(BlendMode::AlphaTest), PropBlend::Cutout);
        assert_eq!(PropBlend::from_mode(BlendMode::Alpha), PropBlend::Alpha);
        assert_eq!(PropBlend::from_mode(BlendMode::Additive), PropBlend::Additive);
        assert_eq!(PropBlend::from_mode(BlendMode::AddAlpha), PropBlend::Additive);
        assert!(!PropBlend::Cutout.is_blended() && PropBlend::Alpha.is_blended());
    }

    #[test]
    fn batches_are_keyed_by_texture_and_blend() {
        let archives = Archives::new(&[]);
        let mut props = PropBuilder::new(&archives);
        props.push_blend(Some(0), PropBlend::Alpha, tri_at(0.0));
        props.push_blend(Some(0), PropBlend::Opaque, tri_at(0.0));
        props.push_blend(Some(0), PropBlend::Alpha, tri_at(1.0));
        props.push(None, tri_at(0.0));
        let (scene, flat) = props.finish();
        assert_eq!(scene.batches.len(), 2);
        assert_eq!(scene.batches[0].blend, PropBlend::Alpha);
        assert_eq!(scene.batches[0].mesh.indices.len(), 6);
        assert_eq!(scene.batches[1].blend, PropBlend::Opaque);
        assert_eq!(flat.indices.len(), 3);
    }

    #[test]
    fn blended_batches_come_last_and_far_to_near() {
        // Eye on the +z side: larger z is nearer.
        let archives = Archives::new(&[]);
        let mut props = PropBuilder::new(&archives);
        props.push_blend(Some(0), PropBlend::Alpha, tri_at(5.0));
        props.push_blend(Some(0), PropBlend::Alpha, tri_at(-5.0));
        props.push_blend(Some(0), PropBlend::Opaque, tri_at(0.0));
        let (mut scene, _) = props.finish();
        scene.sort_back_to_front([0.0, 0.0, 1.0]);
        assert_eq!(scene.batches[0].blend, PropBlend::Opaque);
        // Both blended triangles share one batch: the index order is far (z = -5) then near (z = 5).
        let mesh = &scene.batches[1].mesh;
        let z_of_first_triangle = mesh.vertices[mesh.indices[0] as usize].position[2];
        let z_of_second_triangle = mesh.vertices[mesh.indices[3] as usize].position[2];
        assert_eq!((z_of_first_triangle, z_of_second_triangle), (-5.0, 5.0));
        assert_eq!(scene.batches.len(), 2, "one opaque and one blended batch");
    }

    #[test]
    fn missing_texture_is_recorded_and_not_scened() {
        let archives = Archives::new(&[]);
        let mut props = PropBuilder::new(&archives);
        assert_eq!(props.texture("ReplaceableTextures\\Cliff\\Cliff0"), None);
        assert_eq!(props.texture("ReplaceableTextures\\Cliff\\Cliff0"), None);
        assert_eq!(
            props.missing_textures.iter().collect::<Vec<_>>(),
            vec!["ReplaceableTextures\\Cliff\\Cliff0.blp"]
        );
        let (scene, flat) = props.finish();
        assert!(scene.batches.is_empty() && scene.textures.is_empty());
        assert!(flat.is_empty());
    }
}
