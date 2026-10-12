//! FDF UI rendering: loads frame definitions from the game archives, builds a DrawList, and renders it to a texture.
//!
//! The UI renderer converts FDF DrawItems (quads and text runs) to GPU vertices and renders
//! them with wgpu. Textures are loaded from BLP files found in the provided MPQ archives.

use crate::scene::TextureData;
use std::collections::{BTreeSet, HashMap};

pub mod gpu;
#[cfg(test)]
mod tests;

/// Scene data for rendering an FDF UI.
pub struct UiScene {
    pub draw_list: fdf::DrawList,
    /// Loaded textures, keyed by their name as written in the FDF.
    pub textures: HashMap<String, TextureData>,
    /// Screen dimensions in UI units.
    pub screen: fdf::Screen,
}

const FRAMEDEF_TOC: &str = "UI\\FrameDef\\FrameDef.toc";
const WAR3_SKINS: &str = "UI\\war3skins.txt";

impl UiScene {
    /// Loads a frame from the game's FDF files and builds the draw list.
    ///
    /// Loads every FDF file listed in UI\FrameDef\FrameDef.toc, finds the frame by name, builds the
    /// scene and the draw list, then loads each texture. A texture name without a path separator is a
    /// skin key: it is resolved through UI\war3skins.txt, race section first, then [Default].
    ///
    /// `race` is one of human, orc, nightelf, undead (other values fall back to [Default] only).
    pub fn from_mpq(frame_name: &str, archive_sources: &[&[u8]], race: &str) -> Result<Self, String> {
        let toc_bytes = load_from_mpq(FRAMEDEF_TOC, archive_sources)?;
        let toc_text = String::from_utf8(toc_bytes).map_err(|e| e.to_string())?;
        let fdf_files = fdf::parse_toc(&toc_text);

        let mut library = fdf::Library::new();
        let mut loader = |path: &str| -> Option<String> {
            let bytes = load_from_mpq(path, archive_sources).ok()?;
            String::from_utf8(bytes).ok()
        };
        let mut load_errors = 0;
        for fdf_file in &fdf_files {
            if let Err(e) = library.load(fdf_file, &mut loader) {
                eprintln!("ui: load error {fdf_file}: {e}");
                load_errors += 1;
            }
        }
        eprintln!(
            "ui: frames: {} FDF files listed, {} load errors, {} top-level frames",
            fdf_files.len(),
            load_errors,
            library.frame_names().count()
        );

        if !library.frame_names().any(|name| name == frame_name) {
            let available: Vec<_> = library.frame_names().collect();
            return Err(format!("Frame '{frame_name}' not found. Available frames: {}", available.join(", ")));
        }

        let scene = fdf::Scene::build(&library, &[frame_name])
            .map_err(|e| format!("Failed to build scene for '{frame_name}': {e:?}"))?;

        // A 16:9 window; the layout pillarboxes the 4:3 UI space inside it.
        let screen = fdf::layout::Screen::for_window(1920, 1080);
        let layout = fdf::layout::solve(&scene, &screen, &std::collections::BTreeMap::new())
            .map_err(|e| format!("Failed to solve layout: {e:?}"))?;
        let draw_list = fdf::DrawList::build(&scene, &layout, &library)
            .map_err(|e| format!("Failed to build draw list: {e}"))?;

        let quads = draw_list.items.iter().filter(|i| matches!(i, fdf::DrawItem::Quad(_))).count();
        eprintln!(
            "ui: laid out {} nodes, {} draw items ({} quads, {} text runs)",
            scene.nodes.len(),
            draw_list.items.len(),
            quads,
            draw_list.items.len() - quads
        );

        let skins = load_skins(archive_sources).unwrap_or_else(|e| {
            eprintln!("ui: {WAR3_SKINS} not loaded: {e}");
            gamedata::Profile::default()
        });

        let names: BTreeSet<String> = draw_list
            .items
            .iter()
            .filter_map(|item| match item {
                fdf::DrawItem::Quad(quad) => Some(quad.texture.clone()),
                _ => None,
            })
            .collect();

        let mut textures = HashMap::new();
        let mut missing = Vec::new();
        for name in &names {
            let resolved = resolve_skin_name(&skins, race, name).unwrap_or_else(|| name.clone());
            match load_texture(&resolved, archive_sources) {
                Ok(texture) => {
                    textures.insert(name.clone(), texture);
                }
                Err(_) => missing.push(format!("{name} -> {resolved}")),
            }
        }
        eprintln!(
            "ui: textures: {} referenced, {} loaded (race {race})",
            names.len(),
            textures.len()
        );
        if !missing.is_empty() {
            eprintln!("ui: textures missing: {}", missing.join("; "));
        }

        Ok(UiScene { draw_list, textures, screen })
    }
}

/// Loads UI\war3skins.txt (the later archive wins) for texture name resolution.
fn load_skins(archives: &[&[u8]]) -> Result<gamedata::Profile, String> {
    let bytes = load_from_mpq(WAR3_SKINS, archives)?;
    Ok(gamedata::Profile::parse_lenient_bytes(&bytes))
}

/// Maps the `--race` argument to its war3skins.txt section name.
fn race_section(race: &str) -> Option<&'static str> {
    match race.to_ascii_lowercase().as_str() {
        "human" => Some("Human"),
        "orc" => Some("Orc"),
        "nightelf" => Some("NightElf"),
        "undead" => Some("Undead"),
        _ => None,
    }
}

/// Resolves a texture name from the FDF. A name with a path separator is already a file path; a bare
/// name is a skin key, looked up in the race section first and then in [Default].
fn resolve_skin_name(skins: &gamedata::Profile, race: &str, name: &str) -> Option<String> {
    if name.contains('\\') || name.contains('/') {
        return Some(name.to_string());
    }
    race_section(race)
        .and_then(|section| skins.get_ci(section, name))
        .or_else(|| skins.get_ci("Default", name))
        .map(str::to_string)
}

/// Loads a file from the MPQ archives (last archive wins if file is in multiple).
fn load_from_mpq(file_name: &str, archives: &[&[u8]]) -> Result<Vec<u8>, String> {
    let normalized = file_name.replace('/', "\\");

    for archive_bytes in archives.iter().rev() {
        if let Ok(mpq_arch) = mpq::Archive::open(archive_bytes) {
            if let Ok(data) = mpq_arch.read(&normalized) {
                return Ok(data);
            }
            // Fall back to a case-insensitive match through the listfile.
            if let Ok(entries) = mpq_arch.listfile() {
                for entry in entries {
                    if entry.eq_ignore_ascii_case(&normalized) {
                        return mpq_arch
                            .read(&entry)
                            .map_err(|e| format!("Cannot read {file_name}: {e}"));
                    }
                }
            }
        }
    }
    Err(format!("File not found in MPQ archives: {file_name}"))
}

/// Loads a BLP texture from the MPQ archives. A name without an extension also tries `.blp`
/// (the skin tables give some tile names without one).
fn load_texture(path: &str, archives: &[&[u8]]) -> Result<TextureData, String> {
    let candidates = [path.to_string(), format!("{path}.blp")];
    for candidate in &candidates {
        if let Ok(bytes) = load_from_mpq(candidate, archives) {
            let blp = blp::Blp::parse(&bytes).map_err(|e| e.to_string())?;
            return TextureData::from_blp(&blp).map_err(|e| e.to_string());
        }
    }
    Err(format!("Texture not found: {path}"))
}
