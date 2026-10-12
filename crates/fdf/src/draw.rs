//! A GPU-free draw list: textured quads and text runs in paint order.
//!
//! The list is plain data. A renderer (crates/viewer, later) turns quads into
//! triangles and text runs into glyphs. Texture paths are kept as written in
//! the FDF (single backslashes) after StringList decoration.
//!
//! Paint order: a depth-first walk of the frame tree. Inside each frame the
//! backdrop comes first, then its Texture/String blocks sorted by layer
//! (BACKGROUND < BORDER < ARTWORK < ARTWORK_OVERLAY < HIGHLIGHT < OVERLAY),
//! then its child frames. Blocks in the same layer keep declaration order.

use crate::ast::Property;
use crate::layout::{Layout, Rect};
use crate::library::Library;
use crate::props::{self, AlphaMode, FontSpec, JustifyH, JustifyV, PropError};
use crate::scene::{NodeKind, Scene};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuadKind {
    /// A `Texture` block's `File`.
    Texture,
    /// A frame's `BackdropBackground` (fills the frame).
    BackdropBackground,
    /// A frame's `BackdropEdgeFile` (border; the renderer tiles it).
    BackdropEdge,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    /// Index of the owning node in the scene.
    pub node: usize,
    pub kind: QuadKind,
    pub rect: Rect,
    pub texture: String,
    /// `[left, right, top, bottom]` in texture fractions.
    pub uv: [f32; 4],
    pub alpha: AlphaMode,
    /// `BackdropTileBackground` was set (the renderer should tile).
    pub tile: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub node: usize,
    pub rect: Rect,
    pub text: String,
    pub font: Option<FontSpec>,
    /// RGBA, 0..1.
    pub color: [f32; 4],
    pub justify_h: JustifyH,
    pub justify_v: JustifyV,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DrawItem {
    Quad(Quad),
    Text(TextRun),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawError {
    pub node: String,
    pub error: PropError,
}

impl std::fmt::Display for DrawError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "\"{}\": {} {}",
            self.node, self.error.key, self.error.message
        )
    }
}

impl std::error::Error for DrawError {}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawList {
    pub items: Vec<DrawItem>,
}

/// Layer rank for paint order. Unknown levels paint as ARTWORK.
pub fn layer_rank(layer: Option<&str>) -> u8 {
    match layer.map(str::to_ascii_uppercase).as_deref() {
        Some("BACKGROUND") => 0,
        Some("BORDER") => 1,
        Some("ARTWORK") | None => 2,
        Some("ARTWORK_OVERLAY") => 3,
        Some("HIGHLIGHT") => 4,
        Some("OVERLAY") => 5,
        Some(_) => 2,
    }
}

const FULL_UV: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

impl DrawList {
    /// Builds the draw list for every root of the scene. Nodes without a
    /// rectangle (unpositioned) are skipped together with their own items.
    pub fn build(scene: &Scene, layout: &Layout, lib: &Library) -> Result<DrawList, DrawError> {
        let mut list = DrawList::default();
        for &r in &scene.roots {
            emit(scene, layout, lib, r, &mut list)?;
        }
        Ok(list)
    }

    pub fn quads(&self) -> impl Iterator<Item = &Quad> {
        self.items.iter().filter_map(|i| match i {
            DrawItem::Quad(q) => Some(q),
            DrawItem::Text(_) => None,
        })
    }

    pub fn texts(&self) -> impl Iterator<Item = &TextRun> {
        self.items.iter().filter_map(|i| match i {
            DrawItem::Text(t) => Some(t),
            DrawItem::Quad(_) => None,
        })
    }
}

fn derr(node: &str, error: PropError) -> DrawError {
    DrawError {
        node: node.to_string(),
        error,
    }
}

fn emit(
    scene: &Scene,
    layout: &Layout,
    lib: &Library,
    i: usize,
    out: &mut DrawList,
) -> Result<(), DrawError> {
    let node = &scene.nodes[i];
    if let Some(rect) = layout.rect(i) {
        match node.kind {
            NodeKind::Frame => emit_frame(node, i, rect, lib, out)?,
            NodeKind::Texture => {
                if let Some(file) = props::text(&node.props, "File") {
                    let alpha = props::alpha_mode(&node.props).map_err(|e| derr(&node.name, e))?;
                    let uv = props::tex_coord(&node.props).map_err(|e| derr(&node.name, e))?;
                    out.items.push(DrawItem::Quad(Quad {
                        node: i,
                        kind: QuadKind::Texture,
                        rect,
                        texture: lib.decorate(node.decorate, &file),
                        uv,
                        alpha,
                        tile: false,
                    }));
                }
            }
            NodeKind::String => {
                if let Some(run) = text_run(node, i, rect, lib)? {
                    out.items.push(DrawItem::Text(run));
                }
            }
        }
    }

    // Blocks first (by layer), then child frames.
    let mut blocks: Vec<usize> = node
        .children
        .iter()
        .copied()
        .filter(|&c| scene.nodes[c].kind != NodeKind::Frame)
        .collect();
    blocks.sort_by_key(|&c| layer_rank(scene.nodes[c].layer.as_deref()));
    for c in blocks {
        emit(scene, layout, lib, c, out)?;
    }
    for &c in &node.children {
        if scene.nodes[c].kind == NodeKind::Frame {
            emit(scene, layout, lib, c, out)?;
        }
    }
    Ok(())
}

fn emit_frame(
    node: &crate::scene::Node,
    i: usize,
    rect: Rect,
    lib: &Library,
    out: &mut DrawList,
) -> Result<(), DrawError> {
    let p: &[Property] = &node.props;
    let alpha = if props::has(p, "BackdropBlendAll") {
        AlphaMode::Blend
    } else {
        AlphaMode::Opaque
    };
    let tile = props::has(p, "BackdropTileBackground");
    if let Some(bg) = props::text(p, "BackdropBackground") {
        out.items.push(DrawItem::Quad(Quad {
            node: i,
            kind: QuadKind::BackdropBackground,
            rect,
            texture: lib.decorate(node.decorate, &bg),
            uv: FULL_UV,
            alpha,
            tile,
        }));
    }
    // A button's NormalTexture names a top-level Texture block (its File and TexCoord are the art).
    if let Some(name) = props::text(p, "NormalTexture") {
        if let Ok(template) = lib.block(&name) {
            if let Some(file) = props::text(&template.props, "File") {
                out.items.push(DrawItem::Quad(Quad {
                    node: i,
                    kind: QuadKind::Texture,
                    rect,
                    texture: lib.decorate(node.decorate, &file),
                    uv: props::tex_coord(&template.props).map_err(|e| derr(&node.name, e))?,
                    alpha: props::alpha_mode(&template.props).map_err(|e| derr(&node.name, e))?,
                    tile: false,
                }));
            }
        }
    }
    if let Some(edge) = props::text(p, "BackdropEdgeFile") {
        out.items.push(DrawItem::Quad(Quad {
            node: i,
            kind: QuadKind::BackdropEdge,
            rect,
            texture: lib.decorate(node.decorate, &edge),
            uv: FULL_UV,
            alpha,
            tile: true,
        }));
    }
    if let Some(run) = text_run(node, i, rect, lib)? {
        out.items.push(DrawItem::Text(run));
    }
    Ok(())
}

/// A text run for a node with a `Text` property. Frame text uses
/// `FrameFont`, String blocks use `Font`.
fn text_run(
    node: &crate::scene::Node,
    i: usize,
    rect: Rect,
    lib: &Library,
) -> Result<Option<TextRun>, DrawError> {
    // A String without `Text` is still emitted, with empty text: the game sets its text from code
    // (for example the resource bar numbers), so the run must exist for the host to fill it.
    let raw = match props::text(&node.props, "Text") {
        Some(raw) => raw,
        None if node.kind == NodeKind::String => String::new(),
        None => return Ok(None),
    };
    let font_key = if node.kind == NodeKind::String {
        "Font"
    } else {
        "FrameFont"
    };
    let font = props::font(&node.props, font_key).map_err(|e| derr(&node.name, e))?;
    let color = props::color(&node.props, "FontColor")
        .map_err(|e| derr(&node.name, e))?
        .unwrap_or([1.0; 4]);
    Ok(Some(TextRun {
        node: i,
        rect,
        text: lib.decorate(node.decorate, &raw),
        font,
        color,
        justify_h: props::justify_h(&node.props).map_err(|e| derr(&node.name, e))?,
        justify_v: props::justify_v(&node.props).map_err(|e| derr(&node.name, e))?,
    }))
}
