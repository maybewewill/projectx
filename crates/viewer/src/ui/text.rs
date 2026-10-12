//! CPU text for the FDF UI. Glyphs are rasterised with fontdue into one atlas (rgb = 1, alpha =
//! coverage), so the existing textured-quad pipeline draws them and the vertex colour tints them.
//!
//! Positions are computed in window pixels (y down). A font size given in UI units becomes
//! `size * viewport_height / window_height`, where the window height is the UI-space height (0.6).

use std::collections::HashMap;

use fdf::layout::Rect;
use fdf::props::{JustifyH, JustifyV};
use fdf::TextRun;

/// Atlas width in texels. Glyphs are shelf-packed into rows of this width.
pub const ATLAS_WIDTH: u32 = 1024;
/// Empty texels added right and below each glyph so linear filtering never bleeds neighbours.
const GLYPH_GAP: u32 = 1;

/// One glyph quad in window pixels (y down) with its atlas rectangle and tint.
#[derive(Debug, Clone, PartialEq)]
pub struct TextQuad {
    /// left, top, right, bottom in pixels (y down).
    pub rect_px: [f32; 4],
    /// u0, v0, u1, v1 in atlas fractions (v down, matching the texture rows).
    pub uv: [f32; 4],
    pub color: [f32; 4],
}

/// Counts for the report line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TextStats {
    /// Runs that produced at least one glyph quad.
    pub runs_drawn: usize,
    /// Runs without a loaded font (or with an unusable size).
    pub runs_skipped: usize,
    /// Glyph quads emitted.
    pub glyphs: usize,
    /// Distinct glyph bitmaps in the atlas.
    pub distinct_glyphs: usize,
    /// Text lines laid out (after `|n` and word wrap).
    pub lines: usize,
}

/// The glyph atlas plus one quad list per text run (same order as the runs passed in).
pub struct TextAtlas {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, rgb = 255, alpha = glyph coverage.
    pub rgba: Vec<u8>,
    pub runs: Vec<Vec<TextQuad>>,
    pub stats: TextStats,
}

/// Font size in pixels for a size in UI units.
pub fn size_px(ui_size: f32, window_height_ui: f32, viewport_height_px: f32) -> f32 {
    ui_size * viewport_height_px / window_height_ui
}

/// Shelf-packs rectangles of the given `(width, height)` into rows `width` texels wide, in order.
/// Returns the top-left corner of each rectangle and the total height used. A rectangle wider
/// than `width` is placed at x = 0 and overflows; callers skip such glyphs.
pub fn pack_shelves(sizes: &[(u32, u32)], width: u32) -> (Vec<(u32, u32)>, u32) {
    let mut out = Vec::with_capacity(sizes.len());
    let (mut x, mut y, mut shelf_h) = (0u32, 0u32, 0u32);
    for &(w, h) in sizes {
        let (w_gap, h_gap) = (w + GLYPH_GAP, h + GLYPH_GAP);
        if x > 0 && x + w_gap > width {
            x = 0;
            y += shelf_h;
            shelf_h = 0;
        }
        out.push((x, y));
        x += w_gap;
        shelf_h = shelf_h.max(h_gap);
    }
    (out, y + shelf_h)
}

/// Strips WC3 inline markup: `|cAARRGGBB` colour starts, `|r` resets, `||` is a bar, `|n` is a
/// line break. Colours are not applied; the run colour is used.
pub fn clean_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '|' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('n') | Some('N') => {
                chars.next();
                out.push('\n');
            }
            Some('r') | Some('R') => {
                chars.next();
            }
            Some('c') | Some('C') => {
                chars.next();
                for _ in 0..8 {
                    chars.next();
                }
            }
            Some('|') => {
                chars.next();
                out.push('|');
            }
            _ => out.push('|'),
        }
    }
    out
}

/// Greedy word wrap of one paragraph. `max_width <= 0` disables wrapping. A single word wider
/// than the limit stays on its own line.
pub fn wrap_line(paragraph: &str, max_width: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    if max_width <= 0.0 || measure(paragraph) <= max_width {
        return vec![paragraph.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in paragraph.split(' ') {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if current.is_empty() || measure(&candidate) <= max_width {
            current = candidate;
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    lines.push(current);
    lines
}

/// Key of one rasterised glyph: font name, size in hundredths of a pixel, character.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: String,
    px_hundredths: u32,
    ch: char,
}

/// A placed glyph before packing: which bitmap, and where its pen origin and baseline are.
struct Placement {
    run: usize,
    key: GlyphKey,
    pen_x: f32,
    baseline: f32,
}

struct Bitmap {
    width: u32,
    height: u32,
    xmin: i32,
    ymin: i32,
    coverage: Vec<u8>,
}

/// Lays out every run and packs the glyphs they use into one atlas.
///
/// `runs` are in draw-list order; `fonts` maps the FDF font name (for example `MasterFont`) to
/// TTF bytes. Runs whose font is missing are skipped (empty quad list).
pub fn build_atlas(
    runs: &[&TextRun],
    fonts: &HashMap<String, Vec<u8>>,
    window: Rect,
    viewport: (u32, u32),
) -> TextAtlas {
    let (vw, vh) = (viewport.0 as f32, viewport.1 as f32);
    let window_h = window.height();
    let px_x = |x: f32| (x - window.left) / window.width() * vw;
    let px_y = |y: f32| (window.top - y) / window_h * vh;

    let mut stats = TextStats::default();
    let mut parsed: HashMap<&str, Option<fontdue::Font>> = HashMap::new();
    let mut placements: Vec<Placement> = Vec::new();

    for (index, run) in runs.iter().enumerate() {
        let Some(spec) = run.font.as_ref() else {
            stats.runs_skipped += 1;
            continue;
        };
        let font = parsed
            .entry(spec.name.as_str())
            .or_insert_with(|| {
                fonts
                    .get(&spec.name)
                    .and_then(|bytes| fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default()).ok())
            });
        let Some(font) = font.as_ref() else {
            stats.runs_skipped += 1;
            continue;
        };
        let px_raw = size_px(spec.size, window_h, vh);
        if !(px_raw.is_finite() && px_raw > 0.0) {
            stats.runs_skipped += 1;
            continue;
        }
        let px = (px_raw * 100.0).round() / 100.0;
        let px_q = (px * 100.0).round() as u32;

        let advance = |s: &str| -> f32 { s.chars().map(|c| font.metrics(c, px).advance_width).sum() };
        let rect = &run.rect;
        let left = px_x(rect.left);
        let right = px_x(rect.right);
        let top = px_y(rect.top);
        let bottom = px_y(rect.bottom);
        let line_metrics = font.horizontal_line_metrics(px);
        let ascent = line_metrics.map_or(px * 0.8, |m| m.ascent);
        let new_line = line_metrics.map_or(px * 1.2, |m| m.new_line_size.max(1.0));

        let mut lines = Vec::new();
        for paragraph in clean_text(&run.text).split('\n') {
            lines.extend(wrap_line(paragraph, right - left, advance));
        }
        if lines.iter().all(|l| l.is_empty()) {
            continue;
        }
        stats.lines += lines.len();

        let block_h = new_line * lines.len() as f32;
        let block_top = match run.justify_v {
            JustifyV::Top => top,
            JustifyV::Middle => top + ((bottom - top) - block_h) * 0.5,
            JustifyV::Bottom => bottom - block_h,
        };
        for (line_index, line) in lines.iter().enumerate() {
            let width = advance(line);
            let start_x = match run.justify_h {
                JustifyH::Left => left,
                JustifyH::Center => left + ((right - left) - width) * 0.5,
                JustifyH::Right => right - width,
            };
            let baseline = block_top + new_line * line_index as f32 + ascent;
            let mut pen_x = start_x;
            for ch in line.chars() {
                let metrics = font.metrics(ch, px);
                placements.push(Placement {
                    run: index,
                    key: GlyphKey {
                        font: spec.name.clone(),
                        px_hundredths: px_q,
                        ch,
                    },
                    pen_x,
                    baseline,
                });
                pen_x += metrics.advance_width;
            }
        }
    }

    // Rasterise each distinct glyph once and pack the visible ones.
    let mut glyph_index: HashMap<GlyphKey, usize> = HashMap::new();
    let mut bitmaps: Vec<Bitmap> = Vec::new();
    for placement in &placements {
        let key = &placement.key;
        if glyph_index.contains_key(key) {
            continue;
        }
        let font = parsed.get(key.font.as_str()).and_then(|f| f.as_ref());
        let Some(font) = font else { continue };
        let (metrics, coverage) = font.rasterize(key.ch, key.px_hundredths as f32 / 100.0);
        let (w, h) = (metrics.width as u32, metrics.height as u32);
        if w == 0 || h == 0 || w > ATLAS_WIDTH {
            glyph_index.insert(key.clone(), usize::MAX);
            continue;
        }
        glyph_index.insert(key.clone(), bitmaps.len());
        bitmaps.push(Bitmap {
            width: w,
            height: h,
            xmin: metrics.xmin,
            ymin: metrics.ymin,
            coverage,
        });
    }
    let sizes: Vec<(u32, u32)> = bitmaps.iter().map(|b| (b.width, b.height)).collect();
    let (positions, used_h) = pack_shelves(&sizes, ATLAS_WIDTH);
    let height = used_h.max(1);
    let width = ATLAS_WIDTH;

    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (bitmap, &(x0, y0)) in bitmaps.iter().zip(&positions) {
        for row in 0..bitmap.height {
            for col in 0..bitmap.width {
                let cov = bitmap.coverage[(row * bitmap.width + col) as usize];
                let at = (((y0 + row) * width + x0 + col) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[255, 255, 255, cov]);
            }
        }
    }
    stats.distinct_glyphs = bitmaps.len();

    let mut quads_per_run: Vec<Vec<TextQuad>> = vec![Vec::new(); runs.len()];
    for placement in &placements {
        let Some(&slot) = glyph_index.get(&placement.key) else { continue };
        if slot == usize::MAX {
            continue;
        }
        let bitmap = &bitmaps[slot];
        let (x0, y0) = positions[slot];
        let left = placement.pen_x + bitmap.xmin as f32;
        let top = placement.baseline - (bitmap.ymin as f32 + bitmap.height as f32);
        let (w, h) = (bitmap.width as f32, bitmap.height as f32);
        let (fw, fh) = (width as f32, height as f32);
        let uv = [
            x0 as f32 / fw,
            y0 as f32 / fh,
            (x0 + bitmap.width) as f32 / fw,
            (y0 + bitmap.height) as f32 / fh,
        ];
        quads_per_run[placement.run].push(TextQuad {
            rect_px: [left, top, left + w, top + h],
            uv,
            color: runs[placement.run].color,
        });
    }
    for quads in &quads_per_run {
        if !quads.is_empty() {
            stats.runs_drawn += 1;
            stats.glyphs += quads.len();
        }
    }

    TextAtlas {
        width,
        height,
        rgba,
        runs: quads_per_run,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_units_convert_to_pixels_by_viewport_height() {
        // 0.011 UI units at a 960 px tall window whose UI height is 0.6.
        let px = size_px(0.011, 0.6, 960.0);
        assert!((px - 17.6).abs() < 1e-4, "got {px}");
        // The 4:3 window of 1280x960 has the same scale as the full viewport height.
        assert!((size_px(0.6, 0.6, 960.0) - 960.0).abs() < 1e-3);
    }

    #[test]
    fn shelf_packing_is_in_bounds_and_disjoint() {
        let sizes = [(10, 12), (300, 20), (400, 8), (600, 30), (5, 5), (1000, 40)];
        let width = 1024;
        let (pos, height) = pack_shelves(&sizes, width);
        assert_eq!(pos.len(), sizes.len());
        for (i, (&(x, y), &(w, h))) in pos.iter().zip(&sizes).enumerate() {
            assert!(x + w <= width, "glyph {i} overflows the row");
            assert!(y + h <= height, "glyph {i} is taller than the atlas");
            for (j, (&(x2, y2), &(w2, h2))) in pos.iter().zip(&sizes).enumerate().skip(i + 1) {
                let disjoint = x + w <= x2 || x2 + w2 <= x || y + h <= y2 || y2 + h2 <= y;
                assert!(disjoint, "glyphs {i} and {j} overlap");
            }
        }
    }

    #[test]
    fn shelf_packing_starts_a_new_row_when_full() {
        let (pos, height) = pack_shelves(&[(600, 10), (600, 10)], 1024);
        assert_eq!(pos[0], (0, 0));
        assert_eq!(pos[1], (0, 11));
        assert_eq!(height, 22);
    }

    #[test]
    fn shelf_packing_empty_is_zero_height() {
        let (pos, height) = pack_shelves(&[], 1024);
        assert!(pos.is_empty());
        assert_eq!(height, 0);
    }

    #[test]
    fn inline_markup_is_removed() {
        assert_eq!(clean_text("|cffFFCC00Gold|r now"), "Gold now");
        assert_eq!(clean_text("a|nb"), "a\nb");
        assert_eq!(clean_text("a||b"), "a|b");
    }

    #[test]
    fn wrapping_breaks_at_spaces_within_the_limit() {
        // Every character is 1 unit wide.
        let measure = |s: &str| s.chars().count() as f32;
        let lines = wrap_line("aaa bbb ccc", 7.0, measure);
        assert_eq!(lines, vec!["aaa bbb".to_string(), "ccc".to_string()]);
        // No limit means no wrapping.
        assert_eq!(wrap_line("aaa bbb ccc", 0.0, measure), vec!["aaa bbb ccc".to_string()]);
    }
}
