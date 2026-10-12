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

/// Where the top bar (ResourceBarFrame) goes. The game places it in code at the top-right of the
/// 4:3 safe area; its size is the FDF's (`Width 0.33828125`, `Height 0.0218`).
pub const RESOURCE_BAR_WIDTH: f32 = 0.33828125;
pub const RESOURCE_BAR_HEIGHT: f32 = 0.0218;

/// Width and height of the upper menu bar (UpperButtonBar.fdf: `UpperButtonBarFrame`, 0.34 x 0.022).
pub const UPPER_BAR_WIDTH: f32 = 0.34;
pub const UPPER_BAR_HEIGHT: f32 = 0.022;

/// The host placements of the root frames: the console (ConsoleUI) fills the 4:3 safe area, so its
/// art pieces, anchored to its left and right edges, tile without a gap whatever the window's aspect.
/// The top bar (ResourceBarFrame) sits at the safe area's TOPRIGHT corner and the upper menu bar
/// (UpperButtonBarFrame) at its TOPLEFT corner.
pub fn host_placements(safe: fdf::layout::Rect) -> fdf::layout::ExternalFrames {
    let mut placed = fdf::layout::ExternalFrames::new();
    placed.insert("consoleui".to_string(), safe);
    placed.insert(
        "resourcebarframe".to_string(),
        fdf::layout::Rect::new(safe.right - RESOURCE_BAR_WIDTH, safe.top - RESOURCE_BAR_HEIGHT, safe.right, safe.top),
    );
    placed.insert(
        "upperbuttonbarframe".to_string(),
        fdf::layout::Rect::new(safe.left, safe.top - UPPER_BAR_HEIGHT, safe.left + UPPER_BAR_WIDTH, safe.top),
    );
    placed
}

/// The gap in the top console art, as offsets from the safe area's left edge: ConsoleTexture02 is
/// drawn at the strip's two ends only (left piece 0.256 + 0.087 wide, right piece ends 0.288 from the
/// right edge and is 0.053 wide), so the strip is open from 0.343 to 0.459.
pub const TOP_GAP: (f32, f32) = (0.256 + 0.087, 0.8 - 0.288 - 0.053);

/// Fills the top console gap with the middle of ConsoleTexture02 (TexCoord 0.33984375 .. 0.79296875,
/// the part ConsoleUI.fdf never draws; the texture is 0.256 wide at the strip's scale). Drawn first, so
/// the upper menu bar and the resource bar paint over it. Returns `false` when the texture is missing.
pub fn install_top_fill(scene: &mut UiScene) -> bool {
    let safe = scene.screen.safe;
    if !scene.textures.contains_key("ConsoleTexture02") {
        return false;
    }
    let (left, right) = TOP_GAP;
    let rect = Rect::new(safe.left + left, safe.top - 0.032, safe.left + right, safe.top);
    scene.draw_list.items.insert(
        0,
        DrawItem::Quad(Quad {
            node: 0,
            kind: QuadKind::Texture,
            rect,
            texture: "ConsoleTexture02".to_string(),
            uv: [0.33984375, 0.79296875, 0.0, 0.125],
            alpha: fdf::props::AlphaMode::AlphaKey,
            tile: false,
        }),
    );
    true
}

/// Key of the 1x1 black texture the empty command slots are drawn with.
pub const EMPTY_SLOT_KEY: &str = "hud-empty-slot";

/// Side of an empty command button, in UI units (the game's command button is 0.039 wide).
pub const COMMAND_SLOT_SIZE: f32 = 0.039;

/// Centres of the command card's 4 columns and 3 rows, in UI units. Measured on the console art at
/// 4:3 (1600 px per UI unit): the wooden bars of the card's wire grid are at x 1052, 1122 and 1192
/// px, the card spans x 985 .. 1260 px, and its rows are split at y 812 and 882 px (top beam to
/// bottom beam, 745 .. 950 px).
pub const COMMAND_COLUMNS: [f32; 4] = [0.63672, 0.67969, 0.72344, 0.76641];
pub const COMMAND_ROWS: [f32; 3] = [0.11325, 0.07031, 0.02731];

/// The empty slot picture: a 64 x 64 square, near black inside with a 3-texel brown bevel (about 3 px
/// on screen), generated here so no game file is needed. The card's interior is already black in the
/// art, so the bevel is what makes the slot visible.
pub fn empty_slot_texture() -> Result<TextureData, String> {
    const SIZE: usize = 64;
    const BEVEL: usize = 3;
    let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let edge = x < BEVEL || y < BEVEL || x >= SIZE - BEVEL || y >= SIZE - BEVEL;
            let px: [u8; 4] = if edge { [92, 70, 44, 255] } else { [14, 12, 10, 255] };
            rgba.extend_from_slice(&px);
        }
    }
    TextureData::from_rgba(SIZE as u32, SIZE as u32, rgba).map_err(|e| e.to_string())
}

/// Draws the twelve empty command slots (4 x 3, row-major from the top left) on the card's wire grid.
pub fn install_command_slots(scene: &mut UiScene) -> Result<(), String> {
    scene.textures.insert(EMPTY_SLOT_KEY.to_string(), empty_slot_texture()?);
    let half = COMMAND_SLOT_SIZE / 2.0;
    for &row in &COMMAND_ROWS {
        for &col in &COMMAND_COLUMNS {
            scene.draw_list.items.push(DrawItem::Quad(Quad {
                node: 0,
                kind: QuadKind::Texture,
                rect: Rect::new(col - half, row - half, col + half, row + half),
                texture: EMPTY_SLOT_KEY.to_string(),
                uv: [0.0, 1.0, 0.0, 1.0],
                alpha: fdf::props::AlphaMode::Opaque,
                tile: false,
            }));
        }
    }
    Ok(())
}

/// Finds the minimap and draws its quad under the console art (first in the draw list), so the
/// console's own border and transparent window frame the picture. The minimap frame ("Minimap" in its
/// name) is used when the FDF files have one. The shipped ConsoleUI.fdf has no such frame: its
/// minimap sits in the bottom-left art block (the texture anchored BOTTOMLEFT at the window corner).
pub fn install_minimap(scene: &mut UiScene) -> Option<MinimapSlot> {
    let (node, rect) = match scene.find_frame("Minimap") {
        Some((node, _name, rect)) => (node, rect),
        None => {
            let (node, art) = bottom_left_art(scene)?;
            (node, minimap_window(art))
        }
    };
    let aspect = if rect.height() > 0.0 { rect.width() / rect.height() } else { 1.0 };
    let width = ((MINIMAP_HEIGHT_PX as f32 * aspect).round() as u32).clamp(16, 1024);
    let slot = MinimapSlot { node, width, height: MINIMAP_HEIGHT_PX };
    scene.draw_list.items.insert(
        0,
        DrawItem::Quad(Quad {
            node,
            kind: QuadKind::Texture,
            rect,
            texture: MINIMAP_KEY.to_string(),
            uv: [0.0, 1.0, 0.0, 1.0],
            alpha: fdf::props::AlphaMode::Opaque,
            tile: false,
        }),
    );
    Some(slot)
}

/// The part of the window where the 3D world is visible: between the top bar's bottom edge and the
/// top of the bottom console (the console art's own rectangles). `None` when the console art is not
/// in the scene.
pub fn world_region(scene: &UiScene) -> Option<fdf::layout::Rect> {
    let window = scene.screen.window;
    let eps = 1e-4;
    let mut console_top: Option<f32> = None;
    let mut top_bar_bottom: Option<f32> = None;
    for item in &scene.draw_list.items {
        let DrawItem::Quad(q) = item else { continue };
        if q.kind != QuadKind::Texture {
            continue;
        }
        if (q.rect.bottom - window.bottom).abs() < eps {
            console_top = Some(console_top.map_or(q.rect.top, |t: f32| t.max(q.rect.top)));
        }
        if (q.rect.top - window.top).abs() < eps {
            top_bar_bottom = Some(top_bar_bottom.map_or(q.rect.bottom, |b: f32| b.min(q.rect.bottom)));
        }
    }
    let (bottom, top) = (console_top?, top_bar_bottom?);
    Some(fdf::layout::Rect::new(window.left, bottom, window.right, top))
}

/// The scissor rectangle `(x, y, width, height)` in pixels (y down) of a UI-unit rectangle in a
/// `width` x `height` image of the window.
pub fn scissor_px(ui: fdf::layout::Rect, window: fdf::layout::Rect, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let (w, h) = (width as f32, height as f32);
    let sx = |u: f32| ((u - window.left) / window.width() * w).round().clamp(0.0, w);
    let sy = |u: f32| ((window.top - u) / window.height() * h).round().clamp(0.0, h);
    let (x0, x1) = (sx(ui.left), sx(ui.right));
    let (y0, y1) = (sy(ui.top), sy(ui.bottom));
    (x0 as u32, y0 as u32, (x1 - x0).max(0.0) as u32, (y1 - y0).max(0.0) as u32)
}

/// The minimap's window inside the bottom-left art block, as fractions of the block's width and height
/// (left, right, bottom, top), measured on the shipped ConsoleTexture01 art: the dark window is
/// inset from the stone and wood border on all four sides.
pub const MINIMAP_WINDOW: (f32, f32, f32, f32) = (0.04, 0.69, 0.045, 0.86);

/// The minimap window of the bottom-left art block `art` (see [`MINIMAP_WINDOW`]).
pub fn minimap_window(art: fdf::layout::Rect) -> fdf::layout::Rect {
    let (l, r, b, t) = MINIMAP_WINDOW;
    let (w, h) = (art.width(), art.height());
    fdf::layout::Rect::new(art.left + l * w, art.bottom + b * h, art.left + r * w, art.bottom + t * h)
}

/// The texture quad whose corner sits at the bottom-left corner of the window: the console's
/// bottom-left art block. Returns its scene node and rectangle.
fn bottom_left_art(scene: &UiScene) -> Option<(usize, Rect)> {
    let safe = scene.screen.safe;
    scene.draw_list.items.iter().find_map(|item| match item {
        DrawItem::Quad(q)
            if q.kind == QuadKind::Texture
                && (q.rect.left - safe.left).abs() < 1e-4
                && (q.rect.bottom - safe.bottom).abs() < 1e-4 =>
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
        assert!(matches!(scene.draw_list.items.first(), Some(DrawItem::Quad(q)) if q.texture == MINIMAP_KEY));
    }

    #[test]
    fn resource_bar_sits_at_the_safe_area_top_right() {
        let safe = fdf::layout::Rect::new(0.0, 0.0, 0.8, 0.6);
        let placed = host_placements(safe);
        let r = placed["resourcebarframe"];
        assert!((r.right - 0.8).abs() < 1e-6 && (r.top - 0.6).abs() < 1e-6);
        assert!((r.width() - RESOURCE_BAR_WIDTH).abs() < 1e-6);
        assert!((r.height() - RESOURCE_BAR_HEIGHT).abs() < 1e-6);
    }

    #[test]
    fn world_region_is_between_the_top_bar_and_the_bottom_console() {
        let mut scene = empty_scene(&["ConsoleUI"]);
        let w = scene.screen.window;
        let quad = |rect: Rect| {
            DrawItem::Quad(Quad {
                node: 0,
                kind: QuadKind::Texture,
                rect,
                texture: "x".into(),
                uv: [0.0, 1.0, 0.0, 1.0],
                alpha: fdf::props::AlphaMode::Opaque,
                tile: false,
            })
        };
        // Top bar 0.032 high, bottom console 0.176 high (the tallest bottom piece), as in ConsoleUI.fdf.
        scene.draw_list.items.push(quad(Rect::new(w.left, w.top - 0.032, w.left + 0.256, w.top)));
        scene.draw_list.items.push(quad(Rect::new(w.left, w.bottom, w.left + 0.256, w.bottom + 0.176)));
        scene.draw_list.items.push(quad(Rect::new(w.left + 0.256, w.bottom, w.left + 0.5, w.bottom + 0.15)));
        let r = world_region(&scene).expect("console art");
        assert!((r.bottom - (w.bottom + 0.176)).abs() < 1e-6);
        assert!((r.top - (w.top - 0.032)).abs() < 1e-6);
        assert_eq!((r.left, r.right), (w.left, w.right));
    }

    #[test]
    fn scissor_maps_ui_units_to_pixels_with_y_down() {
        let window = Rect::new(0.0, 0.0, 0.8, 0.6);
        // The full window is the full image.
        assert_eq!(scissor_px(window, window, 800, 600), (0, 0, 800, 600));
        // The top half of the window, in y-down pixels.
        assert_eq!(scissor_px(Rect::new(0.0, 0.3, 0.8, 0.6), window, 800, 600), (0, 0, 800, 300));
    }

    #[test]
    fn console_is_placed_on_the_safe_area_so_its_pieces_meet() {
        let safe = fdf::layout::Rect::new(0.0, 0.0, 0.8, 0.6);
        let placed = host_placements(safe);
        assert_eq!(placed["consoleui"], safe);
    }

    #[test]
    fn command_card_has_twelve_slots_inside_the_wire_grid() {
        let mut scene = empty_scene(&["ConsoleUI"]);
        install_command_slots(&mut scene).unwrap();
        let slots: Vec<Rect> = scene
            .draw_list
            .items
            .iter()
            .filter_map(|i| match i {
                DrawItem::Quad(q) if q.texture == EMPTY_SLOT_KEY => Some(q.rect),
                _ => None,
            })
            .collect();
        assert_eq!(slots.len(), 12);
        // Every slot lies inside the card (x 0.615 .. 0.79, y 0.0 .. 0.15) and no two overlap.
        for r in &slots {
            assert!(r.left > 0.615 && r.right < 0.79 && r.bottom > 0.0 && r.top < 0.15, "{r:?}");
        }
        for (i, a) in slots.iter().enumerate() {
            for b in &slots[i + 1..] {
                let overlap = a.left < b.right && b.left < a.right && a.bottom < b.top && b.bottom < a.top;
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn no_minimap_frame_means_no_minimap() {
        let mut scene = empty_scene(&["ConsoleUI"]);
        assert!(install_minimap(&mut scene).is_none());
        assert!(scene.draw_list.items.is_empty());
    }
}
