//! FDF UI rendering: loads a FrameDef file, builds a DrawList, and renders it to a texture.
//!
//! The UI renderer converts FDF DrawItems (quads and text runs) to GPU vertices and renders
//! them with wgpu. Textures are loaded from BLP files found in the provided MPQ archives.

use crate::scene::TextureData;
use std::collections::HashMap;

pub mod gpu;
#[cfg(test)]
mod tests;

/// Scene data for rendering an FDF UI.
pub struct UiScene {
    pub draw_list: fdf::DrawList,
    /// Loaded textures, keyed by their path (as written in the FDF).
    pub textures: HashMap<String, TextureData>,
    /// Screen dimensions in UI units.
    pub screen: fdf::Screen,
}

impl UiScene {
    /// Loads a FDF file from the MPQ archives and builds the draw list.
    ///
    /// Returns an error if the file cannot be loaded or the FDF is invalid.
    pub fn from_mpq(
        file_name: &str,
        archive_sources: &[&[u8]],
    ) -> Result<Self, String> {
        // Load the FDF file from MPQ
        let fdf_bytes = load_from_mpq(file_name, archive_sources)?;
        let fdf_text = String::from_utf8(fdf_bytes).map_err(|e| e.to_string())?;

        // Build library from the FDF text
        let mut library = fdf::Library::new();
        library
            .load_source("<mpq>", &fdf_text, &mut |_path| None)
            .map_err(|e| format!("Failed to build library: {e:?}"))?;

        // Find the first root frame
        let first_root = library
            .frame_names()
            .next()
            .ok_or_else(|| "No root frames found in FDF".to_string())?
            .to_string();

        // Build scene
        let roots = vec![first_root.as_str()];
        let scene = fdf::Scene::build(&library, &roots)
            .map_err(|e| format!("Failed to build scene: {e:?}"))?;

        // Solve layout with 16:9 aspect ratio
        let width_px = 1920u32;
        let height_px = 1080u32;
        let screen = fdf::layout::Screen::for_window(width_px, height_px);
        let external_frames = std::collections::BTreeMap::new();
        let layout = fdf::layout::solve(&scene, &screen, &external_frames)
            .map_err(|e| format!("Failed to solve layout: {e:?}"))?;

        // Build draw list
        let draw_list = fdf::DrawList::build(&scene, &layout, &library)
            .map_err(|e| format!("Failed to build draw list: {e}"))?;

        // Load textures from MPQ
        let mut textures = HashMap::new();
        for item in &draw_list.items {
            if let fdf::DrawItem::Quad(quad) = item {
                if !textures.contains_key(&quad.texture) {
                    if let Ok(texture) = load_texture(&quad.texture, archive_sources) {
                        textures.insert(quad.texture.clone(), texture);
                    }
                }
            }
        }

        Ok(UiScene {
            draw_list,
            textures,
            screen,
        })
    }
}

/// Loads a file from the MPQ archives (last archive wins if file is in multiple).
fn load_from_mpq(file_name: &str, archives: &[&[u8]]) -> Result<Vec<u8>, String> {
    for archive_bytes in archives.iter().rev() {
        if let Ok(mpq_arch) = mpq::Archive::open(archive_bytes) {
            // Try to read the file directly first
            if let Ok(data) = mpq_arch.read(file_name) {
                return Ok(data);
            }

            // If not found, try case-insensitive search via listfile
            if let Ok(entries) = mpq_arch.listfile() {
                for entry in entries {
                    if entry.eq_ignore_ascii_case(file_name) {
                        return mpq_arch
                            .read(&entry)
                            .map_err(|e| format!("Cannot read {}: {}", file_name, e));
                    }
                }
            }
        }
    }
    Err(format!("File not found in MPQ archives: {}", file_name))
}

/// Loads a BLP texture from the MPQ archives.
fn load_texture(path: &str, archives: &[&[u8]]) -> Result<TextureData, String> {
    // Normalize path separators
    let normalized = path.replace('/', "\\");

    // Try to load from MPQ
    if let Ok(bytes) = load_from_mpq(&normalized, archives) {
        let blp = blp::Blp::parse(&bytes).map_err(|e| e.to_string())?;
        return TextureData::from_blp(&blp).map_err(|e| e.to_string());
    }

    Err(format!("Texture not found: {}", path))
}
