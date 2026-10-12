//! Props: triangles that sample one texture each, for cliff walls and placed model objects.
//!
//! The terrain's own pipelines sample a ground atlas, which models cannot use: their UVs wrap
//! and their textures are separate files. Props keep a per-texture batch instead, drawn with
//! the `fs_model` stage of `terrain.wgsl`. Triangles whose texture cannot be read go to a flat
//! mesh, drawn with the flat colour pipeline like the rest of the terrain's flat geometry.
//!
//! Model files and BLP textures are read once per path and cached. Presentation only: the
//! textures are used at their first mip level, without mip-mapping, and alpha is a cut-out.

use std::collections::{BTreeSet, HashMap};

use crate::scene::TextureData;
use crate::scene_objects::Archives;
use crate::terrain::{Mesh, TerrainVertex};

/// One texture's triangles.
#[derive(Debug, Clone, PartialEq)]
pub struct PropBatch {
    /// Index into [`PropScene::textures`].
    pub texture: usize,
    pub mesh: Mesh,
}

/// Everything drawn with a model texture: the decoded textures and one batch per texture.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PropScene {
    pub textures: Vec<TextureData>,
    pub batches: Vec<PropBatch>,
}

impl PropScene {
    pub fn triangle_count(&self) -> usize {
        self.batches.iter().map(|b| b.mesh.indices.len() / 3).sum()
    }
}

/// Reads models and textures through the archives, caching each path.
pub struct PropBuilder<'a, 'b> {
    archives: &'a Archives<'b>,
    models: HashMap<String, Option<mdx::Model>>,
    texture_ids: HashMap<String, Option<usize>>,
    textures: Vec<TextureData>,
    batch_meshes: Vec<Mesh>,
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
            batch_meshes: Vec::new(),
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
                self.batch_meshes.push(Mesh::default());
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

    /// Adds one triangle. With a texture it goes to that texture's batch (vertex UVs sampled
    /// from it); without one it goes to the flat mesh, drawn with the vertex colour.
    pub fn push(&mut self, texture: Option<usize>, tri: [TerrainVertex; 3]) {
        match texture {
            Some(t) => self.batch_meshes[t].triangle(tri),
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
            .batch_meshes
            .into_iter()
            .enumerate()
            .filter(|(_, mesh)| !mesh.is_empty())
            .map(|(texture, mesh)| PropBatch { texture, mesh })
            .collect();
        (
            PropScene {
                textures: self.textures,
                batches,
            },
            self.flat,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_names_get_the_blp_extension_once() {
        assert_eq!(
            texture_archive_name("ReplaceableTextures\\Cliff\\Cliff0"),
            "ReplaceableTextures\\Cliff\\Cliff0.blp"
        );
        assert_eq!(texture_archive_name("Textures/Rock.BLP"), "Textures\\Rock.BLP");
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
