//! CPU rasterizer for placed models: orthographic view from the front and above, flat-shaded
//! triangles with a depth buffer. Presentation only; no texture sampling.

pub const CLASS_BACKGROUND: u8 = 0;
pub const CLASS_GROUND: u8 = 1;
pub const CLASS_DOODAD: u8 = 2;
pub const CLASS_UNIT: u8 = 3;
pub const CLASS_NAMES: [&str; 4] = ["background", "ground", "doodad", "unit"];
/// A quantized colour and its pixel count.
pub type ColourCount = ([u8; 3], usize);

/// Orthographic camera: looks from the front and above (`pitch` radians above the ground).
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub pitch: f32,
    /// World x/y minimum and scale (pixels per world unit), set by [`View::fit`].
    pub min: [f32; 2],
    pub scale: f32,
    pub width: usize,
    pub height: usize,
}

impl View {
    /// Screen position (x right, y up) and depth (larger is nearer) of a world point.
    pub fn project(&self, p: [f32; 3]) -> [f32; 3] {
        let (s, c) = self.pitch.sin_cos();
        [p[0], p[1] * s + p[2] * c, -p[1] * c + p[2] * s]
    }

    /// Chooses the scale so that the projected box `[min, max]` (screen units) fills the image
    /// with a small margin.
    pub fn fit(width: usize, height: usize, pitch: f32, points: impl Iterator<Item = [f32; 3]>) -> Self {
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        let probe = View { pitch, min: [0.0; 2], scale: 1.0, width, height };
        for p in points {
            let s = probe.project(p);
            for k in 0..2 {
                lo[k] = lo[k].min(s[k]);
                hi[k] = hi[k].max(s[k]);
            }
        }
        if lo[0] > hi[0] {
            lo = [0.0; 2];
            hi = [1.0; 2];
        }
        let span = [(hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0)];
        let margin = 0.04;
        let scale = (width as f32 * (1.0 - 2.0 * margin) / span[0])
            .min(height as f32 * (1.0 - 2.0 * margin) / span[1]);
        // Centre the projected box: the visible window is width/scale by height/scale world units.
        let centre = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        let min = [
            centre[0] - width as f32 / (2.0 * scale),
            centre[1] - height as f32 / (2.0 * scale),
        ];
        View { pitch, min, scale, width, height }
    }

    /// Pixel position (x, y with y down) of a projected screen point.
    fn pixel(&self, s: [f32; 3]) -> [f32; 2] {
        [(s[0] - self.min[0]) * self.scale, self.height as f32 - (s[1] - self.min[1]) * self.scale]
    }
}

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
    pub class: Vec<u8>,
    depth: Vec<f32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgb: vec![0; width * height * 3],
            class: vec![CLASS_BACKGROUND; width * height],
            depth: vec![f32::MIN; width * height],
        }
    }

    /// Fills the rectangle covered by a ground quad with the ground colour (no depth write).
    pub fn fill_ground(&mut self, view: &View, lo: [f32; 3], hi: [f32; 3], color: [u8; 3]) {
        let corners = [
            view.pixel(view.project([lo[0], lo[1], lo[2]])),
            view.pixel(view.project([hi[0], lo[1], lo[2]])),
            view.pixel(view.project([hi[0], hi[1], lo[2]])),
            view.pixel(view.project([lo[0], hi[1], lo[2]])),
        ];
        let (x0, x1) = minmax(corners.iter().map(|c| c[0]));
        let (y0, y1) = minmax(corners.iter().map(|c| c[1]));
        for y in y0.max(0)..y1.min(self.height as i32) {
            for x in x0.max(0)..x1.min(self.width as i32) {
                let i = y as usize * self.width + x as usize;
                self.rgb[i * 3..i * 3 + 3].copy_from_slice(&color);
                self.class[i] = CLASS_GROUND;
            }
        }
    }

    /// Draws one triangle given in world space, with a flat colour and class.
    pub fn triangle(&mut self, view: &View, world: [[f32; 3]; 3], color: [u8; 3], class: u8) {
        let s = world.map(|p| view.project(p));
        let p = s.map(|q| view.pixel(q));
        let area = edge(p[0], p[1], p[2]);
        if area.abs() < 1e-6 {
            return;
        }
        let (x0, x1) = minmax(p.iter().map(|q| q[0]));
        let (y0, y1) = minmax(p.iter().map(|q| q[1]));
        for y in y0.max(0)..y1.min(self.height as i32) {
            for x in x0.max(0)..x1.min(self.width as i32) {
                let c = [x as f32 + 0.5, y as f32 + 0.5];
                let w0 = edge(p[1], p[2], c) / area;
                let w1 = edge(p[2], p[0], c) / area;
                let w2 = edge(p[0], p[1], c) / area;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let depth = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let i = y as usize * self.width + x as usize;
                if depth >= self.depth[i] {
                    self.depth[i] = depth;
                    self.rgb[i * 3..i * 3 + 3].copy_from_slice(&color);
                    self.class[i] = class;
                }
            }
        }
    }

    /// Counts pixels per class and the most common quantized colours (top `n`).
    pub fn histogram(&self, n: usize) -> (Vec<(&'static str, usize)>, Vec<ColourCount>) {
        let mut per_class = [0usize; 4];
        let mut colours = std::collections::BTreeMap::<[u8; 3], usize>::new();
        for (i, &c) in self.class.iter().enumerate() {
            per_class[c as usize] += 1;
            if c != CLASS_BACKGROUND {
                let px = &self.rgb[i * 3..i * 3 + 3];
                let q = [px[0] & 0xF0, px[1] & 0xF0, px[2] & 0xF0];
                *colours.entry(q).or_insert(0) += 1;
            }
        }
        let classes = CLASS_NAMES.iter().zip(per_class).map(|(&name, count)| (name, count)).collect();
        let mut top: Vec<([u8; 3], usize)> = colours.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        top.truncate(n);
        (classes, top)
    }
}

fn edge(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
}

fn minmax(values: impl Iterator<Item = f32>) -> (i32, i32) {
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for v in values {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    (lo.floor() as i32, hi.ceil() as i32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_covers_its_interior_and_depth_wins() {
        // Pitch a quarter turn: screen y = world y, depth = world z.
        let view = View {
            pitch: std::f32::consts::FRAC_PI_2,
            min: [0.0, 0.0],
            scale: 1.0,
            width: 10,
            height: 10,
        };
        let mut c = Canvas::new(10, 10);
        // Screen x right, y up: the triangle (0,0)-(10,0)-(0,10) covers the lower-left half.
        c.triangle(&view, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]], [9, 9, 9], CLASS_DOODAD);
        let lower_left = (10 - 1) * 10 + 1; // pixel (1, 9): screen (1.5, 0.5)
        assert_eq!(c.class[lower_left], CLASS_DOODAD);
        let upper_right = 9; // pixel (9, 0): screen (9.5, 9.5)
        assert_eq!(c.class[upper_right], CLASS_BACKGROUND);
        // A nearer triangle over the same pixel replaces the colour, a farther one does not.
        c.triangle(&view, [[0.0, 0.0, 5.0], [10.0, 0.0, 5.0], [0.0, 10.0, 5.0]], [1, 2, 3], CLASS_UNIT);
        assert_eq!(&c.rgb[lower_left * 3..lower_left * 3 + 3], &[1, 2, 3]);
        c.triangle(&view, [[0.0, 0.0, -5.0], [10.0, 0.0, -5.0], [0.0, 10.0, -5.0]], [7, 7, 7], CLASS_DOODAD);
        assert_eq!(&c.rgb[lower_left * 3..lower_left * 3 + 3], &[1, 2, 3]);
    }
}
