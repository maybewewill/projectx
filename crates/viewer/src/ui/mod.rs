//! FDF UI rendering: loads frame definitions from the game archives, builds a DrawList, and renders it to a texture.
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

const FRAMEDEF_TOC: &str = "UI\\FrameDef\\FrameDef.toc";

impl UiScene {
    /// Loads a frame from the game's FDF files and builds the draw list.
    ///
    /// Loads all FDF files listed in UI\FrameDef\FrameDef.toc, finds the frame by name,
    /// builds the scene, solves the layout, and loads textures.
    ///
    /// # Arguments
    /// * `frame_name` - The root frame name (e.g., "ConsoleUI", "MainMenu")
    /// * `archive_sources` - MPQ archive byte slices, searched last-to-first
    ///
    /// Returns an error if the TOC cannot be found, frame is not found, or layout fails.
    pub fn from_mpq(
        frame_name: &str,
        archive_sources: &[&[u8]],
    ) -> Result<Self, String> {
        // Load the FrameDef.toc file from MPQ
        let toc_bytes = load_from_mpq(FRAMEDEF_TOC, archive_sources)?;
        let toc_text = String::from_utf8(toc_bytes).map_err(|e| e.to_string())?;

        // Parse TOC to get the list of FDF files to load
        let fdf_files = fdf::parse_toc(&toc_text);
        eprintln!("ui: TOC lists {} FDF files", fdf_files.len());

        // Build library by loading all FDF files
        let mut library = fdf::Library::new();
        let mut load_errors = 0;

        // Create a loader that can load files from MPQ
        let mut loader = |path: &str| -> Option<String> {
            let bytes = load_from_mpq(path, archive_sources).ok()?;
            String::from_utf8(bytes).ok()
        };

        for fdf_file in &fdf_files {
            match library.load(fdf_file, &mut loader) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("ui: load error {}: {}", fdf_file, e);
                    load_errors += 1;
                }
            }
        }
        eprintln!("ui: loaded {} files, {} errors", fdf_files.len(), load_errors);

        // Find the requested frame
        if !library.frame_names().any(|name| name == frame_name) {
            let available: Vec<_> = library.frame_names().collect();
            return Err(format!(
                "Frame '{}' not found. Available frames: {}",
                frame_name,
                available.join(", ")
            ));
        }

        // Build scene
        let roots = vec![frame_name];
        let scene = fdf::Scene::build(&library, &roots)
            .map_err(|e| format!("Failed to build scene for '{}': {e:?}", frame_name))?;
        eprintln!("ui: scene has {} nodes", scene.nodes.len());

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
        eprintln!("ui: draw list has {} items", draw_list.items.len());

        // Collect texture paths from draw list and load them
        let mut texture_paths = std::collections::HashSet::new();
        for item in &draw_list.items {
            if let fdf::DrawItem::Quad(quad) = item {
                texture_paths.insert(quad.texture.clone());
            }
        }
        eprintln!("ui: {} textures referenced", texture_paths.len());

        let mut textures = HashMap::new();
        let mut missing_textures = Vec::new();
        for path in texture_paths {
            match load_texture(&path, archive_sources) {
                Ok(texture) => {
                    textures.insert(path, texture);
                }
                Err(e) => {
                    eprintln!("ui: texture missing {}: {}", path, e);
                    missing_textures.push(path);
                }
            }
        }
        if !missing_textures.is_empty() {
            eprintln!(
                "ui: {} textures missing: {}",
                missing_textures.len(),
                missing_textures.join(", ")
            );
        }
        eprintln!("ui: loaded {} textures", textures.len());

        Ok(UiScene {
            draw_list,
            textures,
            screen,
        })
    }
}

/// Loads a file from the MPQ archives (last archive wins if file is in multiple).
fn load_from_mpq(file_name: &str, archives: &[&[u8]]) -> Result<Vec<u8>, String> {
    // Normalize path separators to backslashes (MPQ standard)
    let normalized = file_name.replace('/', "\\");

    for archive_bytes in archives.iter().rev() {
        if let Ok(mpq_arch) = mpq::Archive::open(archive_bytes) {
            // Try to read the file directly first
            if let Ok(data) = mpq_arch.read(&normalized) {
                return Ok(data);
            }

            // If not found, try case-insensitive search via listfile
            if let Ok(entries) = mpq_arch.listfile() {
                for entry in entries {
                    if entry.eq_ignore_ascii_case(&normalized) {
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
