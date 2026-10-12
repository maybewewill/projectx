//! Terrain queries of the simulation: the cliff level of a pathing cell (`0x6F0128C0`) and the ground height behind
//! `GetLocationZ` (`0x6F3C0E50` -> `0x6F24F3E0`). Written from `docs/spec/terrain.md`.
//!
//! The data is the corner grid of `war3map.w3e` as the original keeps it (28-byte records, one per tile corner,
//! row-major, built by the loader `0x6F756CA0`). Only the fields the two queries read are kept: the 14-bit ground
//! height, the 14-bit water level, the "misc" byte (low nibble = layer = cliff level) and the flag word.
//!
//! Arithmetic note: the original does this in x87 with the FPU set to 53-bit precision by `_control87(0x9001F)`, with a
//! store to `float` at some points. The port evaluates in `f64` (exact for these operations) and rounds to `f32` at
//! exactly those stores, so it is not a soft-float path (`crates/softfloat`): the terrain code never calls the soft-float
//! library. Checked against the original in Unicorn (`tools/emu/terrain_emu.py`, `tests/terrain.rs`).
//!
//! Not modelled (all UNSURE, listed in the spec): the "ramp-object" list of bridge-like models (its ray test is only
//! used by `GetLocationZ` when the list is not empty; the cell-level lookup of the list is modelled, see
//! [`RampRect`]), and the terrain deformation list (`+0x790`, craters and ripples; empty at map start).

use crate::combat::CliffLevels;

/// `0x6FAA1760`: the height of one cliff layer (read at start-up, Unicorn).
pub const LEVEL_HEIGHT: f32 = 128.0;
/// `0x6FADB3E4`: the base offset added to every corner (layer 2 is the ground, so `-2 * 128`).
pub const LEVEL_BASE: f32 = -256.0;
/// The initial minimum of the cliff-top test (`terrain + 0x988`): `trunc((MaxHeight + 256) / 128)` with Misc
/// `[Terrain] MaxHeight = 1536` (UI\MiscData.txt of 1.26a), so 14.
pub const RAMP_SCAN_INIT: i32 = 14;

/// The corner flag bits of the record word at `+0xC`.
pub const FLAG_RAMP: u16 = 0x02;
/// Water flag of a corner (`+0xC` bit 4).
pub const FLAG_WATER: u16 = 0x10;

/// One tile corner. `height` and `water` are the 14-bit raw values of the file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Corner {
    /// Raw 14-bit ground height (`(h - 8192) / 4` world units).
    pub height: u16,
    /// Raw 14-bit water level.
    pub water: u16,
    /// The third misc byte of the file: high nibble cliff texture, low nibble layer (cliff level).
    pub misc: u8,
    /// The record flag word: 0x02 ramp, 0x04 blight, 0x10 water, 0x40 boundary, 0x80 map edge.
    pub flags: u16,
}

impl Corner {
    /// The corner of the file values: the first 3 bytes after the height/water words are `flags|texture`,
    /// `variation` and `misc`; the loader `0x6F740610` maps the flag bits 0x10/0x20/0x40/0x80 of the first to
    /// 0x02/0x04/0x10/0x40 and the water word's bit 14 (map edge) to 0x80.
    pub fn from_file(height_word: u16, water_word: u16, flags_tex: u8, misc: u8) -> Self {
        let mut flags = 0u16;
        if water_word & 0x4000 != 0 {
            flags |= 0x80;
        }
        if flags_tex & 0x10 != 0 {
            flags |= 0x02;
        }
        if flags_tex & 0x20 != 0 {
            flags |= 0x04;
        }
        if flags_tex & 0x40 != 0 {
            flags |= 0x10;
        }
        if flags_tex & 0x80 != 0 {
            flags |= 0x40;
        }
        Corner {
            height: height_word & 0x3FFF,
            water: water_word & 0x3FFF,
            misc,
            flags,
        }
    }

    /// The cliff level (layer) of the corner.
    pub fn level(&self) -> i32 {
        (self.misc & 0xF) as i32
    }
}

/// One entry of the original's ramp-object list (`0x6F0118C0`): a rectangle of cells `[x0..=x1] x [y0..=y1]`
/// with one level per cell, row-major (`(x1 - x0 + 1) * (y - y0) + (x - x0)`). The original builds these for
/// bridge-like objects (`0x6F0152E0`); the simulation has none by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RampRect {
    pub y0: i32,
    pub x0: i32,
    pub y1: i32,
    pub x1: i32,
    pub levels: Vec<i32>,
}

/// The corner grid and the water height offset.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainGrid {
    width: u32,
    height: u32,
    /// `w3e` center offset (the world position of corner (0, 0)).
    center: [f32; 2],
    corners: Vec<Corner>,
    /// The water height offset of the tileset (`[water table + 0x108]`); `None` = no water table (`+0x30C == 0`).
    water_offset: Option<f32>,
    ramp_init: i32,
    ramps: Vec<RampRect>,
}

const K: f64 = 0.0078125; // 1/128, the double at 0x6F8763A8
const LO: f32 = -0.001; // 0x6F935D98
const HI: f32 = 1.001; // 0x6F981A8C
const DX: [i64; 4] = [0, 1, 0, 1]; // 0x6F9815E0
const DY: [i64; 4] = [0, 0, 1, 1]; // 0x6F9815F0

/// The float-to-int helper `0x6F7E11E0` on its truncating branch (the flag `0x6FAE759C` is 1 at run time): a
/// conversion toward zero, 0x80000000 for out-of-range and NaN.
fn trunc_i32(v: f64) -> i32 {
    if v.is_nan() || v >= 2147483648.0 || v <= -2147483649.0 {
        i32::MIN
    } else {
        v as i32
    }
}

impl TerrainGrid {
    /// A grid of `width * height` corners (row-major) whose corner (0, 0) is at the world position `center`.
    /// Returns `None` when the count does not match.
    pub fn new(width: u32, height: u32, center: [f32; 2], corners: Vec<Corner>) -> Option<Self> {
        if width == 0 || height == 0 || corners.len() != (width as usize) * (height as usize) {
            return None;
        }
        Some(TerrainGrid {
            width,
            height,
            center,
            corners,
            water_offset: None,
            ramp_init: RAMP_SCAN_INIT,
            ramps: Vec::new(),
        })
    }

    /// Sets the water height offset of the tileset (`None` = no water table).
    pub fn set_water_offset(&mut self, offset: Option<f32>) {
        self.water_offset = offset;
    }

    /// Sets the initial minimum of the cliff-top test (`terrain + 0x988`).
    pub fn set_ramp_scan_init(&mut self, v: i32) {
        self.ramp_init = v;
    }

    /// Adds an entry of the ramp-object list used by the cell level.
    pub fn add_ramp_rect(&mut self, r: RampRect) {
        self.ramps.push(r);
    }

    /// Width in corners.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in corners.
    pub fn height(&self) -> u32 {
        self.height
    }

    fn w1(&self) -> u32 {
        self.width - 1
    }

    fn h1(&self) -> u32 {
        self.height - 1
    }

    fn at(&self, tx: u32, ty: u32) -> &Corner {
        &self.corners[(ty as usize) * (self.width as usize) + tx as usize]
    }

    /// A corner or `None` outside `0..=w-1 x 0..=h-1` (`0x6F743C60`: unsigned compares).
    fn nb(&self, tx: i64, ty: i64) -> Option<&Corner> {
        let (ux, uy) = (tx as u32, ty as u32);
        if tx < i64::from(i32::MIN) || ty < i64::from(i32::MIN) || ux > self.w1() || uy > self.h1() {
            return None;
        }
        Some(self.at(ux, uy))
    }

    /// The 9-bit position index the loader stores for tile index `i` on one axis (`0x6F756CA0`, version 11):
    /// `trunc((f32(i * 128 + off) + 32768) / 128)`, only the low 9 bits survive in the record.
    fn pos_idx(i: u32, off: f32) -> u32 {
        let v13 = (f64::from(i) * 128.0 + f64::from(off)) as f32;
        let v14 = ((f64::from(v13) + 32768.0) * K) as f32;
        (v14 as i64 as u32) & 0x1FF
    }

    /// The world x of corner column `tx` as the original decodes it from the record bits (`0x6F73A960`).
    fn corner_x(&self, tx: u32) -> f32 {
        (128.0 * f64::from(Self::pos_idx(tx, self.center[0])) - 32768.0) as f32
    }

    /// The world y of corner row `ty`.
    fn corner_y(&self, ty: u32) -> f32 {
        (128.0 * f64::from(Self::pos_idx(ty, self.center[1])) - 32768.0) as f32
    }

    /// The map extent `(min y, min x, max y, max x)` from the first and the last record (`0x6F73F430`).
    fn extent(&self) -> (f32, f32, f32, f32) {
        (
            self.corner_y(0),
            self.corner_x(0),
            self.corner_y(self.h1()),
            self.corner_x(self.w1()),
        )
    }

    // ---- cliff level (0x6F0128C0) --------------------------------------------------------------------------------

    /// The ramp-object list lookup `0x6F0118C0`: the largest level of the entries that cover the cell, or `None`.
    fn ramp_lookup(&self, cx: i32, cy: i32) -> Option<i32> {
        let mut best = -1i32;
        let mut found = false;
        for r in &self.ramps {
            if cx < r.x0 || cx > r.x1 || cy < r.y0 || cy > r.y1 {
                continue;
            }
            let idx = (r.x1 - r.x0 + 1) * (cy - r.y0) + cx - r.x0;
            let v = r.levels.get(idx as usize).copied().unwrap_or(0);
            if v > best {
                best = v;
            }
            found = true;
        }
        found.then_some(best)
    }

    /// The terrain level of the pathing cell `(cx, cy)` (32-unit cells, cell 0 at the whole-map minimum corner):
    /// the layer nibble of the nearest tile corner, `(c + 2) / 4` truncated (`0x6F73AFA0`), -1 outside the map
    /// (`0x6F742820`), raised to the ramp-object level when an entry of the list covers the cell.
    pub fn cell_level(&self, cx: i32, cy: i32) -> i32 {
        let base = if (cx as u32) >= 4 * self.w1() || (cy as u32) >= 4 * self.h1() {
            -1
        } else {
            // (c + 2) / 4 with C truncation; c >= 0 here
            let tx = ((cx + 2) / 4) as u32;
            let ty = ((cy + 2) / 4) as u32;
            self.at(tx, ty).level()
        };
        match self.ramp_lookup(cx, cy) {
            Some(r) if r > base => r,
            _ => base,
        }
    }

    // ---- corner height (0x6F7523C0, mode 7) ------------------------------------------------------------------------

    /// Cliff-top test `0x6F747110`: whether corner `(tx, ty)` is the highest of the ramp corners around it, with a
    /// level step among them.
    fn cliff_top(&self, tx: i64, ty: i64) -> bool {
        let mut lo = self.ramp_init;
        let mut hi = 0i32;
        for k in 0..4 {
            let (a, b) = (tx - DX[k], ty - DY[k]);
            if (a as u32) < self.w1() && (b as u32) < self.h1() && a >= i64::from(i32::MIN) && b >= i64::from(i32::MIN) {
                for (ex, ey) in [(0i64, 0i64), (1, 0), (0, 1), (1, 1)] {
                    let c = self.at((a + ex) as u32, (b + ey) as u32);
                    if c.flags & FLAG_RAMP != 0 {
                        let l = c.level();
                        lo = lo.min(l);
                        hi = hi.max(l);
                    }
                }
            }
        }
        lo != hi && self.at(tx as u32, ty as u32).level() == hi
    }

    /// The 3x3 neighbourhood of `0x6F743C60`, x-major: index `(dx + 1) * 3 + (dy + 1)`.
    fn neighbours(&self, tx: i64, ty: i64) -> [Option<&Corner>; 9] {
        let mut out = [None; 9];
        for a in 0..3i64 {
            for b in 0..3i64 {
                out[(a * 3 + b) as usize] = self.nb(tx - 1 + a, ty - 1 + b);
            }
        }
        out
    }

    /// Ramp class `0x6F74B920`: -1 not a ramp corner, 0 cliff top, 1 and 2 ramp kinds, -1 otherwise.
    fn ramp_class(&self, tx: i64, ty: i64) -> i32 {
        let me = self.at(tx as u32, ty as u32);
        if me.flags & FLAG_RAMP == 0 {
            return -1;
        }
        if self.cliff_top(tx, ty) {
            return 0;
        }
        let n = self.neighbours(tx, ty);
        let l0 = me.level();
        let (mut mn, mut mx) = (l0, l0); // v6, v7: all neighbours
        let (mut rmn, mut rmx) = (l0, l0); // v24, v5: ramp neighbours
        let mut all_ramp = true; // v23
        for c in n.iter() {
            match c {
                None => all_ramp = false,
                Some(c) => {
                    let l = c.level();
                    mn = mn.min(l);
                    mx = mx.max(l);
                    if c.flags & FLAG_RAMP == 0 {
                        all_ramp = false;
                    } else {
                        rmn = rmn.min(l);
                        rmx = rmx.max(l);
                    }
                }
            }
        }
        let v12 = rmx - rmn;
        let v13 = mx - mn;
        if (all_ramp && v13 == 0) || v12 > 1 {
            return -1;
        }
        let pair = |a: usize, b: usize| -> bool {
            match (n[a], n[b]) {
                (Some(p), Some(q)) if p.flags & FLAG_RAMP != 0 && q.flags & FLAG_RAMP != 0 => {
                    (p.misc ^ q.misc) & 0xF != 0
                }
                _ => false,
            }
        };
        let v14 = pair(1, 7);
        let v15 = pair(3, 5);
        let step = if v13 <= 1 { 1 } else { -1 }; // 2 * (v13 <= 1) - 1
        if v14 {
            // LABEL_39
            return if v15 {
                if v13 <= 1 {
                    2
                } else {
                    -1
                }
            } else {
                step
            };
        }
        if !all_ramp {
            if !v15 {
                0
            } else {
                step
            }
        } else if !v15 {
            2
        } else {
            step
        }
    }

    /// Corner kind `0x6F74BAE0`: a letter.
    fn corner_kind(&self, tx: i64, ty: i64) -> u8 {
        let me = self.at(tx as u32, ty as u32);
        let top = self.cliff_top(tx, ty);
        if me.flags & FLAG_RAMP == 0 {
            return if top { 66 } else { 65 };
        }
        if top {
            return 72;
        }
        match self.ramp_class(tx, ty) {
            0 => 76,
            1 => 82,
            2 => 83,
            _ => 33,
        }
    }

    /// Ramp variant `0x6F750860` for a class-2 corner: -1, 0, 1 or 2.
    fn ramp_variant(&self, tx: i64, ty: i64) -> i32 {
        if self.ramp_class(tx, ty) != 2 {
            return -1;
        }
        let n = self.neighbours(tx, ty);
        let mut c = [0u8; 9];
        for (k, p) in n.iter().enumerate() {
            let Some(_) = p else { return -1 };
            let (a, b) = (k as i64 / 3, k as i64 % 3);
            c[k] = self.corner_kind(tx - 1 + a, ty - 1 + b);
        }
        // corner positions in the 3x3 block: index = 3 * row + col
        const POS: [(usize, usize); 4] = [(0, 0), (0, 2), (2, 0), (2, 2)];
        let (mut nl, mut nh) = (0, 0);
        let (mut last_l, mut last_h) = (usize::MAX, usize::MAX);
        for (p, &(r, cl)) in POS.iter().enumerate() {
            let k = 3 * r + cl;
            if c[k] == 76 {
                nl += 1;
                last_l = p;
            } else if c[k] == 72 {
                nh += 1;
                last_h = p;
            }
        }
        match nl {
            1 => {
                if nh == 3 {
                    let (row, col) = POS[last_l];
                    let v14 = 3 * row;
                    if c[1 + v14] == 82 && c[3 + col] == 82 && c[7 - v14] == 72 {
                        return if c[5 - col] == 72 { 1 } else { -1 };
                    }
                }
                -1
            }
            2 => {
                if nh != 2 {
                    return -1;
                }
                let (row, col) = POS[last_l];
                if c[8 - 3 * row - col] != 76 || c[1] != 82 || c[7] != 82 || c[3] != 82 || c[5] != 82 {
                    return -1;
                }
                2
            }
            3 => {
                if nh != 1 {
                    return -1;
                }
                let (row, col) = POS[last_h];
                let v16 = 3 * row;
                if c[1 + v16] != 82 || c[3 + col] != 82 || c[7 - v16] != 76 {
                    return -1;
                }
                if c[5 - col] != 76 {
                    -1
                } else {
                    0
                }
            }
            _ => -1,
        }
    }

    /// The corner kind letter and the ramp variant of a corner, for test generators (`0x6F74BAE0`, `0x6F750860`).
    #[doc(hidden)]
    pub fn ramp_debug(&self, tx: u32, ty: u32) -> (u8, i32) {
        (self.corner_kind(i64::from(tx), i64::from(ty)), self.ramp_variant(i64::from(tx), i64::from(ty)))
    }

    /// `0x6F750AB0(tx, ty, 0)`: the extra height of a ramp corner.
    fn ramp_offset(&self, tx: i64, ty: i64) -> f32 {
        match self.corner_kind(tx, ty) {
            82 => (f64::from(LEVEL_HEIGHT) * 0.5) as f32,
            83 => match self.ramp_variant(tx, ty) {
                0 => (f64::from(LEVEL_HEIGHT) * 0.25) as f32,
                1 => (f64::from(LEVEL_HEIGHT) * 0.75) as f32,
                2 => (f64::from(LEVEL_HEIGHT) * 0.5) as f32,
                _ => 0.0,
            },
            _ => 0.0,
        }
    }

    /// The height of corner `(tx, ty)` in mode 7 (`0x6F7523C0`): ground height, layer, ramp offset. The
    /// deformation term (mode bit 4) is not modelled.
    fn corner_z(&self, tx: u32, ty: u32) -> f32 {
        let c = self.at(tx, ty);
        let bits = (u32::from(c.height) | 0x22C000) << 9;
        let mut v9 = (f64::from(f32::from_bits(bits)) - 6144.0) as f32;
        // mode & 1: fild(layer) * 128 + (-256) + v9
        v9 = (f64::from(c.level()) * f64::from(LEVEL_HEIGHT) + f64::from(LEVEL_BASE) + f64::from(v9)) as f32;
        // mode & 2: ramp corners
        if c.flags & FLAG_RAMP != 0 {
            let off = self.ramp_offset(i64::from(tx), i64::from(ty));
            v9 = (f64::from(off) + f64::from(v9)) as f32;
        }
        v9
    }

    // ---- ground height (0x6F763D50, mode 7) -------------------------------------------------------------------------

    /// The interpolation `0x6F741250` of the four corner heights, `(dx, dy)` being the offset from the tile corner.
    fn interpolate(dx: f32, dy: f32, c: [f32; 4]) -> f32 {
        let vx = (f64::from(dx) * K) as f32;
        let vy = (K * f64::from(dy)) as f32;
        // a NaN passes the range tests (the original's flag test lets unordered through), so no `contains`
        #[allow(clippy::manual_range_contains)]
        let outside = vx < LO || HI < vx || vy < LO || HI < vy;
        if outside {
            return 0.0;
        }
        let mut m = c[0];
        if m > c[1] {
            m = c[1];
        }
        if c[2] < m {
            m = c[2];
        }
        if c[3] < m {
            m = c[3];
        }
        let (vx, vy, m64) = (f64::from(vx), f64::from(vy), f64::from(m));
        let u = 1.0 - vy;
        let ux = 1.0 - vx;
        let acc1 = ((((f64::from(c[0]) - m64) * u) * ux) + m64) as f32;
        let acc2 = (f64::from(acc1) + ((f64::from(c[1]) - m64) * u) * vx) as f32;
        let acc3 = (f64::from(acc2) + ((f64::from(c[2]) - m64) * vy) * ux) as f32;
        let s = ((f64::from(c[3]) - m64) * vy) * vx + f64::from(acc3);
        let acc4 = s as f32;
        (f64::from(acc4) + 0.0) as f32
    }

    /// The ground height at `(x, y)`: the piecewise-bilinear surface over the tile corners (`0x6F763F00` with mode -1,
    /// which runs `0x6F763D50` with mode 7: layer, ramps). Zero outside the map.
    pub fn ground_height(&self, x: f32, y: f32) -> f32 {
        let (w1, h1) = (self.w1(), self.h1());
        if w1 == 0 || h1 == 0 {
            return 0.0;
        }
        let ox = self.corner_x(0);
        let oy = self.corner_y(0);
        let mut a = trunc_i32((f64::from(x) - f64::from(ox)) * K) as u32;
        let mut b = trunc_i32((f64::from(y) - f64::from(oy)) * K) as u32;
        if !(a < w1 && b < h1) {
            if a == w1 {
                a = w1 - 1;
            }
            if b == h1 {
                b = h1 - 1;
            }
            if !(a < w1 && b < h1) {
                return 0.0;
            }
        }
        let mut c = [0f32; 4];
        for i in 0..4 {
            c[i] = self.corner_z(a + DX[i] as u32, b + DY[i] as u32);
        }
        let px = self.corner_x(a);
        let py = self.corner_y(b);
        let dx = (f64::from(x) - f64::from(px)) as f32;
        let dy = (f64::from(y) - f64::from(py)) as f32;
        Self::interpolate(dx, dy, c)
    }

    // ---- water and GetLocationZ -----------------------------------------------------------------------------------

    /// `0x6F747450`: the nearest corner's index, or `None`.
    fn nearest_corner(&self, x: f32, y: f32, minx: f32, miny: f32) -> Option<(u32, u32)> {
        let axis = |v: f32, min: f32| -> i32 {
            let a = (((f64::from(v) - f64::from(min)) * K) + 0.5) as f32;
            let s = a.to_bits() >> 31;
            let t = (f64::from(a.abs()) + 65536.0) as f32;
            let r = (((t.to_bits() as i32 >> 7) & 0x7FFF) as u32 ^ s).wrapping_sub(s);
            i32::from(r as u16 as i16)
        };
        let tx = axis(x, minx) as u32;
        let ty = axis(y, miny) as u32;
        if tx > self.w1() || ty > self.h1() {
            return None;
        }
        Some((tx, ty))
    }

    /// The water surface at the point if the nearest corner has water above the ground (`0x6F7527F0`).
    fn water_surface(&self, x: f32, y: f32) -> Option<f32> {
        let off = self.water_offset?;
        let (miny, minx, maxy, maxx) = self.extent();
        // strict inside test of 0x6F73AC90 (a NaN fails)
        if !(minx < x && x < maxx && miny < y && y < maxy) {
            return None;
        }
        let (tx, ty) = self.nearest_corner(x, y, minx, miny)?;
        let c = self.at(tx, ty);
        if c.flags & FLAG_WATER == 0 {
            return None;
        }
        let ground = self.corner_z(tx, ty);
        let bits = 0x4580_0000 | (u32::from(c.water) << 9);
        let wv = (f64::from(f32::from_bits(bits)) - 6144.0) as f32;
        let w = (f64::from(off) + f64::from(wv)) as f32;
        if 0.0 < f64::from(w) - f64::from(ground) {
            Some(w)
        } else {
            None
        }
    }

    /// `GetLocationZ`: the larger of the ground height and the water surface (`0x6F24F3E0`).
    pub fn location_z(&self, x: f32, y: f32) -> f32 {
        let g = self.ground_height(x, y);
        match self.water_surface(x, y) {
            // `W < G` keeps the ground; equal or unordered takes the water
            Some(w) => {
                if w < g {
                    g
                } else {
                    w
                }
            }
            None => g,
        }
    }
}

impl CliffLevels for TerrainGrid {
    fn level(&self, cell_x: i32, cell_y: i32) -> i32 {
        self.cell_level(cell_x, cell_y)
    }
}

/// A shared grid as the cliff-level provider of the combat module.
#[derive(Debug, Clone)]
pub struct SharedTerrain(pub std::sync::Arc<TerrainGrid>);

impl CliffLevels for SharedTerrain {
    fn level(&self, cell_x: i32, cell_y: i32) -> i32 {
        self.0.cell_level(cell_x, cell_y)
    }
}
