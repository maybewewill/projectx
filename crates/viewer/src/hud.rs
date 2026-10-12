//! The HUD of `viewer --hud RACE`: the ConsoleUI layer drawn over the live 3D frame, with the minimap
//! and the resource texts filled from the simulation.
//!
//! Everything here is presentation. The texts and the minimap are read from the simulation; the
//! console art and its layout come from the game's FDF files, unchanged.

use fdf::{DrawItem, Quad, QuadKind, Rect};
use crate::minimap::{self, Fit, MinimapBase, WorldRect, MINIMAP_KEY};
use crate::scene::TextureData;
use crate::scene_objects::SimUnit;
use crate::ui::UiScene;

/// Height of the minimap image in pixels. Its width follows the minimap frame's aspect ratio.
const MINIMAP_HEIGHT_PX: u32 = 256;

/// The minimap frame in the HUD: where its quad is drawn and how big its image is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapSlot {
    /// Scene node of the minimap frame.
    pub node: usize,
    pub width: u32,
    pub height: u32,
}

/// The resource numbers shown in the top bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resources {
    pub gold: i64,
    pub lumber: i64,
    pub food_used: i32,
    pub food_made: i32,
}

/// Frame names of the resource texts in ConsoleUI (looked up by exact name, case-insensitive).
pub const GOLD_TEXT: &str = "ResourceBarGoldText";
pub const LUMBER_TEXT: &str = "ResourceBarLumberText";
pub const SUPPLY_TEXT: &str = "ResourceBarSupplyText";

/// Finds the minimap and adds its quad on top of the draw list, so the minimap image is painted over
/// the console art. The minimap frame ("Minimap" in its name) is used when the FDF files have one.
/// The shipped ConsoleUI.fdf has no such frame: its minimap sits in the bottom-left art block (the
/// texture anchored BOTTOMLEFT at the window corner), which is used instead.
pub fn install_minimap(scene: &mut UiScene) -> Option<MinimapSlot> {
    let (node, rect) = match scene.find_frame("Minimap") {
        Some((node, _name, rect)) => (node, rect),
        None => bottom_left_art(scene)?,
    };
    let aspect = if rect.height() > 0.0 { rect.width() / rect.height() } else { 1.0 };
    let width = ((MINIMAP_HEIGHT_PX as f32 * aspect).round() as u32).clamp(16, 1024);
    let slot = MinimapSlot { node, width, height: MINIMAP_HEIGHT_PX };
    scene.draw_list.items.push(DrawItem::Quad(Quad {
        node,
        kind: QuadKind::Texture,
        rect,
        texture: MINIMAP_KEY.to_string(),
        uv: [0.0, 1.0, 0.0, 1.0],
        alpha: fdf::props::AlphaMode::Opaque,
        tile: false,
    }));
    Some(slot)
}

/// The texture quad whose corner sits at the bottom-left corner of the window: the console's
/// bottom-left art block. Returns its scene node and rectangle.
fn bottom_left_art(scene: &UiScene) -> Option<(usize, Rect)> {
    let window = scene.screen.window;
    scene.draw_list.items.iter().find_map(|item| match item {
        DrawItem::Quad(q)
            if q.kind == QuadKind::Texture
                && (q.rect.left - window.left).abs() < 1e-4
                && (q.rect.bottom - window.bottom).abs() < 1e-4 =>
        {
            Some((q.node, q.rect))
        }
        _ => None,
    })
}

/// Sets the text of the text run(s) of the frame named `frame`. Returns `false` when the frame or
/// its text run is not in the laid-out scene.
pub fn set_frame_text(scene: &mut UiScene, frame: &str, text: &str) -> bool {
    let Some(node) = scene.node_named(frame) else {
        return false;
    };
    let mut found = false;
    for item in &mut scene.draw_list.items {
        if let DrawItem::Text(run) = item {
            if run.node == node {
                run.text = text.to_string();
                found = true;
            }
        }
    }
    found
}

/// Writes the resource numbers into the top bar. Returns the frames that were set and the ones that
/// were not found (so the report can say which texts are missing).
pub fn set_resource_texts(scene: &mut UiScene, r: &Resources) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut set = Vec::new();
    let mut missing = Vec::new();
    let food = format!("{}/{}", r.food_used, r.food_made);
    for (frame, text) in [
        (GOLD_TEXT, r.gold.to_string()),
        (LUMBER_TEXT, r.lumber.to_string()),
        (SUPPLY_TEXT, food),
    ] {
        if set_frame_text(scene, frame, &text) {
            set.push(frame);
        } else {
            missing.push(frame);
        }
    }
    (set, missing)
}

/// Draws the minimap image and stores it as the minimap texture of the scene.
pub fn update_minimap(
    scene: &mut UiScene,
    slot: MinimapSlot,
    base: &MinimapBase,
    world: WorldRect,
    units: &[SimUnit],
    is_building: &dyn Fn(u32) -> bool,
    camera: Option<WorldRect>,
) -> Result<(), String> {
    let fit = Fit::new(world, slot.width, slot.height);
    let rgba = minimap::render(base, &fit, units, is_building, camera);
    let texture = TextureData::from_rgba(slot.width, slot.height, rgba).map_err(|e| e.to_string())?;
    scene.textures.insert(MINIMAP_KEY.to_string(), texture);
    Ok(())
}

/// The rectangle of the minimap frame (for the report).
pub fn minimap_rect(scene: &UiScene, slot: MinimapSlot) -> Option<Rect> {
    scene.node_rects.get(slot.node).copied().flatten()
}

/// The minimap picture of the map (`war3mapMap.blp` of the map archive), or `None` when the map has
/// none or it does not read.
pub fn map_picture(map: &[u8]) -> Option<MinimapBase> {
    let archive = mpq::Archive::open(map).ok()?;
    let bytes = archive.read("war3mapMap.blp").ok()?;
    let blp = blp::Blp::parse(&bytes).ok()?;
    let texture = TextureData::from_blp(&blp).ok()?;
    Some(MinimapBase { width: texture.width, height: texture.height, rgba: texture.rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiScene;
    use std::collections::HashMap;

    fn empty_scene(names: &[&str]) -> UiScene {
        UiScene {
            draw_list: fdf::DrawList::default(),
            textures: HashMap::new(),
            fonts: HashMap::new(),
            screen: fdf::Screen::for_window(1920, 1080),
            node_names: names.iter().map(|s| s.to_string()).collect(),
            node_rects: names.iter().map(|_| Some(Rect::new(0.0, 0.0, 0.1, 0.1))).collect(),
        }
    }

    fn text_run(node: usize, text: &str) -> DrawItem {
        DrawItem::Text(fdf::TextRun {
            node,
            rect: Rect::new(0.0, 0.0, 0.1, 0.1),
            text: text.to_string(),
            font: None,
            color: [1.0; 4],
            justify_h: fdf::props::JustifyH::Left,
            justify_v: fdf::props::JustifyV::Middle,
        })
    }

    #[test]
    fn resource_texts_go_to_the_named_frames_and_missing_ones_are_reported() {
        let mut scene = empty_scene(&["ConsoleUI", GOLD_TEXT, LUMBER_TEXT]);
        scene.draw_list.items.push(text_run(1, "0"));
        scene.draw_list.items.push(text_run(2, "0"));
        let r = Resources { gold: 500, lumber: 120, food_used: 7, food_made: 10 };
        let (set, missing) = set_resource_texts(&mut scene, &r);
        assert_eq!(set, vec![GOLD_TEXT, LUMBER_TEXT]);
        assert_eq!(missing, vec![SUPPLY_TEXT]);
        let texts: Vec<&str> = scene
            .draw_list
            .items
            .iter()
            .filter_map(|i| match i {
                DrawItem::Text(run) => Some(run.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["500", "120"]);
    }

    #[test]
    fn minimap_frame_is_found_by_name_and_gets_an_aspect_matching_image() {
        let mut scene = empty_scene(&["ConsoleUI", "MinimapFrame"]);
        scene.node_rects[1] = Some(Rect::new(0.0, 0.0, 0.2, 0.1));
        let slot = install_minimap(&mut scene).expect("minimap frame");
        assert_eq!(slot.node, 1);
        assert_eq!((slot.width, slot.height), (512, 256));
        assert!(matches!(scene.draw_list.items.last(), Some(DrawItem::Quad(q)) if q.texture == MINIMAP_KEY));
    }

    #[test]
    fn no_minimap_frame_means_no_minimap() {
        let mut scene = empty_scene(&["ConsoleUI"]);
        assert!(install_minimap(&mut scene).is_none());
        assert!(scene.draw_list.items.is_empty());
    }
}
