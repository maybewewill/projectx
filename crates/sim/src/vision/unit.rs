//! The unit sight writer of the fog grid (`0x6f409e00`, docs/spec/vision.md section 14) and the pieces around it:
//! the sight shape of a radius (`0x6f409120`, cached per radius by `0x6f409d00`), the elevation layer
//! (`0x6f406660`), the radius conversion (`0x6f406ab0`) and the unit's effective sight value (`0x6f29f040`).
//!
//! In one paragraph. A unit writes its sight into the fog grid every refresh with a radius `k` in fog cells
//! (`unit+0x10C`). For `k = 1` and `k = 2` the shape is the small rectangle / plus of the circle writer. For `k >= 3`
//! the shape is a *cliff-aware sight cast*: the grid keeps an elevation word per fog cell (the maximum terrain cliff
//! level of its 4 by 4 pathing cells, times 128). The unit has a level word `L` (its own cell's cliff level rounded to
//! 128, or a fixed value for ships). A cell inside the circle is written unless a line of cells towards the unit is
//! *blocked*: a cell blocks when its elevation word is above `L`, and a cell is blocked when either of its two parent
//! cells (the neighbours one step closer to the unit) is blocked or it is itself higher than `L`. The blocking cell
//! itself is still written (you see the cliff face, not behind it). The circle is the span table of
//! [`span_table`], swept in eight octants from the centre. Flying units (`F`) ignore the elevation and write the whole
//! span table.
//!
//! Everything here is checked against the original in Unicorn (`tools/emu/vision_unit_sight_emu.py`): the shape for
//! every radius from 3 to 70, 30 elevation layers and 1,398 writer cases over all paths (docs/spec/vision.md 14.6).

use std::sync::Arc;

use softfloat::{arith, conv, math, F32, Tables};

use super::{span_table, VisionGrid, FOG_VISIBLE};

/// `3.0` (`0x6FAAE50C`): the seconds a sight change (the night switch) takes. The unit's sight property gets a
/// modifier of rate `delta / 3` for 3 seconds (`0x6f27a4e0`, CONFIRMED by disassembly, value from Unicorn).
pub const SIGHT_RAMP_SECONDS: F32 = F32::from_bits(0x4040_0000);
/// `1800.0` (`0x6FAB44B8`): the largest sight the unit's value accepts (`0x6f29f040`, CONFIRMED value).
pub const MAX_SIGHT: F32 = F32::from_bits(0x44E1_0000);
/// `0.001` (`0x6FAAE47C`).
const EPSILON: f32 = 0.001;
/// `50.0` (`0x6FAAE568`) and `250.0` (`0x6FAAE588`): the terms of the capped sight (see [`capped_sight`]).
const CAP_BASE: F32 = F32::from_bits(0x4248_0000);
const CAP_DEFAULT: F32 = F32::from_bits(0x437A_0000);

const NONE: usize = usize::MAX;

/// One cell of a [`SightShape`] beyond the central 3 by 3 block: its offset from the unit's cell and the two cells (as
/// record indices) one step closer to the centre, whose blocking it inherits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SightRecord {
    pub dx: i32,
    pub dy: i32,
    /// Parent record indices; both [`usize::MAX`] for the central block.
    pub a: usize,
    pub b: usize,
}

/// The sight shape of a radius (`0x6f409120`): the records in the order the original keeps them and writes them (the
/// central 3 by 3 block first, then the four quadrants scanned outwards, so that a parent always comes before its
/// children), and the span table of the radius.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SightShape {
    pub k: i32,
    /// The number of records of the central block (9).
    pub n0: usize,
    pub records: Vec<SightRecord>,
    /// The span table of radius `k` (`2k + 1` rows of `(lo, hi)`), used by the flying path and the edge clamp.
    pub spans: Vec<(i32, i32)>,
}

/// The eight sweeps of the first phase: the step per unit of distance and the step per secondary offset.
const SWEEPS: [((i32, i32), (i32, i32)); 8] = [
    ((0, -1), (1, 0)),
    ((0, 1), (-1, 0)),
    ((1, 0), (0, 1)),
    ((-1, 0), (0, -1)),
    ((0, -1), (-1, 0)),
    ((0, 1), (1, 0)),
    ((1, 0), (0, -1)),
    ((-1, 0), (0, 1)),
];
/// The four scan quadrants of the second phase: start column offset, start row offset, column step, row step.
const QUADRANTS: [(i32, i32, i32, i32); 4] = [(0, 0, 1, -1), (-1, 0, -1, -1), (0, 1, 1, 1), (-1, 1, -1, 1)];

impl SightShape {
    /// Builds the shape of radius `k >= 3`. Checked record by record against the original for `k = 3..=70`.
    ///
    /// Phase 1 numbers the cells: the central block first (raster order), then for each distance `d = 1..=k` and each
    /// of the eight sweeps the cells `d * primary + s * secondary` for `s` from 0 (sweeps 0 to 3) or 1 (sweeps 4 to 7)
    /// up to `min(d, hi_d)` (minus 1 for sweeps 4 to 7), where `hi_d` is the half width of the span row at distance
    /// `d`. A cell already numbered is skipped. Its parents are the cell one step back along the primary axis and
    /// (when it is neither on the axis nor on the diagonal) the cell one step back along both. Phase 2 renumbers the
    /// cells beyond the central block in the order of a scan of the four quadrants.
    pub fn build(k: i32) -> SightShape {
        assert!(k >= 3, "the shape exists for radii of 3 and up");
        let spans = span_table(k);
        let u = 2 * k + 1;
        let center = k * (u + 1);
        let mut m = vec![NONE; (u * u) as usize];
        // phase-1 records: (dx, dy, parent a map index, parent b map index)
        let mut old: Vec<(i32, i32, usize, usize)> = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                m[(center + dy * u + dx) as usize] = old.len();
                old.push((dx, dy, NONE, NONE));
            }
        }
        let n0 = old.len();
        for d in 1..=k {
            let hi = spans[(k - d) as usize].1;
            for (t, &(p, s)) in SWEEPS.iter().enumerate() {
                let mut lim = d.min(hi);
                let mut start = 0;
                if t >= 4 {
                    start = 1;
                    lim -= 1;
                }
                for si in start..=lim {
                    let dx = d * p.0 + si * s.0;
                    let dy = d * p.1 + si * s.1;
                    let idx = center + dy * u + dx;
                    if m[idx as usize] != NONE {
                        continue;
                    }
                    let back = idx - (p.1 * u + p.0);
                    let back2 = back - (s.1 * u + s.0);
                    let (a, b) = if si == 0 {
                        (back, back)
                    } else if si == d {
                        (back2, back2)
                    } else {
                        (back, back2)
                    };
                    m[idx as usize] = old.len();
                    old.push((dx, dy, a as usize, b as usize));
                }
            }
        }
        // phase 2: scan the four quadrants and renumber the cells beyond the central block
        let mut newmap = vec![NONE; (u * u) as usize];
        for (i, &v) in m.iter().enumerate() {
            if v != NONE && v < n0 {
                newmap[i] = v;
            }
        }
        let mut records: Vec<SightRecord> = old[..n0]
            .iter()
            .map(|&(dx, dy, _, _)| SightRecord { dx, dy, a: NONE, b: NONE })
            .collect();
        for &(x0, y0, sx, sy) in &QUADRANTS {
            let rows = k + i32::from(y0 == 0);
            let cols = k + i32::from(x0 == 0);
            let mut rowstart = center + y0 * u + x0;
            for _ in 0..rows {
                let mut i = rowstart;
                for _ in 0..cols {
                    let v = m[i as usize];
                    if v != NONE && v >= n0 {
                        let (dx, dy, a, b) = old[v];
                        let na = newmap[a];
                        let nb = newmap[b];
                        newmap[i as usize] = records.len();
                        records.push(SightRecord { dx, dy, a: na, b: nb });
                    }
                    i += sx;
                }
                rowstart += sy * u;
            }
        }
        SightShape { k, n0, records, spans }
    }
}

/// What the unit writer reads about the unit (the arguments and the unit fields of `0x6f409e00`).
#[derive(Debug, Clone, Copy)]
pub struct UnitSightInput {
    /// The centre of the unit's footprint in pathing cells (`0x6f474460`): the fog cell is this `>> 2`. Both
    /// coordinates must be non-negative (a negative half takes the float slow path of the centre function, not modelled).
    pub path_cell: (i32, i32),
    /// The radius in fog cells (`unit+0x10C`, [`VisionGrid::sight_cells`]).
    pub radius: u16,
    /// The player mask; the gate tests the low 12 bits, the words use the low 16.
    pub mask: u32,
    /// The owner slot (read only when the low 12 bits of the mask are zero: owners of 12 and up write nothing then).
    pub owner: u32,
    /// `F`: the unit ignores the elevation (`unit+0x5C & 0x20000000` with `unit+0x200 <= 0`; flying units, LIKELY).
    pub ignore_cliffs: bool,
    /// The movement word is exactly `0x10` (ships): the level comes from `float_height` instead of the terrain.
    pub float_move: bool,
    /// `f2i` of the terrain height function `0x6f00f600` at the unit's cell (a per-tile array that is empty in a fresh
    /// game, so 0; UNSURE) for ships.
    pub float_height: i32,
    /// The terrain cliff level of the unit's pathing cell (`0x6f0128c0`) and whether a ramp object covered it.
    pub terrain_level: i32,
    pub ramp_found: bool,
    /// Argument 3: use the radius 4 instead of the unit's.
    pub use_radius_four: bool,
    /// Argument 4: the cells are explored but not made visible (no visible bits are set).
    pub explore_only: bool,
}

impl VisionGrid {
    /// The cached shape of radius `k >= 3` (`0x6f409d00`).
    pub fn sight_shape(&mut self, k: i32) -> Arc<SightShape> {
        self.shapes.entry(k).or_insert_with(|| Arc::new(SightShape::build(k))).clone()
    }

    /// The elevation word of the fog cell with flat index `i`; 0 outside the layer (the original reads whatever memory
    /// follows; UNSURE, cells outside the grid only occur for units outside the map).
    fn elevation_at(&self, i: i64) -> i16 {
        if i >= 0 && (i as usize) < self.elev.len() {
            self.elev[i as usize]
        } else {
            0
        }
    }

    /// The elevation layer (`0x6f406660`, CONFIRMED by Unicorn, 30 grids): for every fog cell the maximum, as a signed
    /// 16-bit word, of `level << 7` over its 4 by 4 pathing cells, starting from -1. `level(x, y)` is the terrain cliff
    /// level of the pathing cell (`0x6f0128c0`, -1 off the map).
    pub fn set_elevation_layer(&mut self, level: impl Fn(i32, i32) -> i32) {
        for row in 0..self.height {
            for col in 0..self.width {
                let mut best: i16 = -1;
                for i in 0..4 {
                    for j in 0..4 {
                        let v = ((level(col * 4 + i, row * 4 + j) << 7) & 0xFFFF) as u16 as i16;
                        if v > best {
                            best = v;
                        }
                    }
                }
                let at = ((row as i64) << self.shift) + col as i64;
                self.elev[at as usize] = best;
            }
        }
    }

    /// The elevation word of a fog cell (diagnostics and tests).
    pub fn elevation(&self, cell: (i32, i32)) -> i16 {
        self.elevation_at(((cell.1 as i64) << self.shift) + cell.0 as i64)
    }

    /// The radius in fog cells of a sight value (`0x6f406ab0`, CONFIRMED by Unicorn): `0` unless `value * scale_x` is
    /// above zero, else `f2i(round(value * scale_x))`, kept to 16 bits as the unit stores it.
    pub fn sight_cells(&self, value: F32) -> u16 {
        let t = arith::mul(value, self.scale.0);
        if arith::lt(F32::ZERO, t) {
            conv::f2i(math::round(t)) as u16
        } else {
            0
        }
    }

    /// Replaces the three word arrays (`pitch * height` entries each). For tests and hosts that restore a grid.
    pub fn set_raw_words(&mut self, vis: Vec<u16>, msk: Vec<u16>, elev: Vec<i16>) {
        assert!(vis.len() == self.vis.len() && msk.len() == self.msk.len() && elev.len() == self.elev.len());
        self.vis = vis;
        self.msk = msk;
        self.elev = elev;
    }

    /// The visible and mask word arrays, flat (`(row << shift) + col`).
    pub fn raw_words(&self) -> (&[u16], &[u16]) {
        (&self.vis, &self.msk)
    }

    fn write_words(&mut self, i: usize, and_mask: u16, or_mask: u16) {
        self.msk[i] &= and_mask;
        self.vis[i] |= or_mask;
    }

    /// Writes the sight of one unit (`0x6f409e00`). See the module comment for the rule; the paths are, in the order of
    /// the original:
    ///
    /// * radius 0 nothing; radius 1 and 2 the rectangle / plus of the circle writer;
    /// * radius 3 and up, the cast fits the grid and the unit ignores cliffs: the span table rows;
    /// * radius 3 and up, the cast fits the grid: the central block, then the records with blocking;
    /// * radius 3 and up near the edge: the same with every cell checked against the grid (cells outside, and the
    ///   cells behind them, are dropped). The central block is written sequentially from the clamped rows, so its
    ///   blocking flags can be attached to the wrong records when the block is cut by the edge (a quirk of the
    ///   original, reproduced).
    pub fn write_unit_sight_cells(&mut self, u: &UnitSightInput) {
        if u.mask & 0xFFF == 0 && u.owner >= 12 {
            return;
        }
        let (cx, cy) = (u.path_cell.0 >> 2, u.path_cell.1 >> 2);
        let k = if u.use_radius_four { 4 } else { i32::from(u.radius) };
        let mask = (u.mask & 0xFFFF) as u16;
        if k == 0 {
            return;
        }
        if k <= 2 {
            let cells = self.circle_shape_cells(cx, cy, k);
            self.write_indices(FOG_VISIBLE, mask, &cells);
            return;
        }
        let (w, h) = (self.width, self.height);
        let clip_x = !(cx >= k && cx + k < w);
        let clip_y = !(cy >= k && cy + k < h);
        let shape = self.sight_shape(k);
        let pitch = 1i32 << self.shift;
        let c0 = ((cy as i64) << self.shift) + cx as i64;
        // the unit's level word
        let mut ax: u16;
        let mut found = false;
        if u.float_move {
            ax = (u.float_height.wrapping_add(0x100)) as u16;
        } else {
            ax = ((u.terrain_level << 7) & 0xFFFF) as u16;
            found = u.ramp_found;
        }
        if found {
            let centre = self.elevation_at(c0) as u16;
            if (ax as i16) <= (centre as i16) {
                ax = centre;
            }
        }
        let level = (((ax as u32 + 0x2040) & 0xFFFF_FF80).wrapping_sub(0x2000) & 0xFFFF) as u16 as i16;
        let and_mask = !mask;
        let or_mask = if u.explore_only { 0 } else { mask };
        let mut flags = vec![0u8; shape.records.len()];

        if clip_x || clip_y {
            if u.ignore_cliffs {
                let left = (cx - k).max(0);
                let right = (cx + k).min(w - 1);
                let top = (cy - k).max(0);
                let bottom = (cy + k).min(h - 1);
                if top > bottom {
                    return;
                }
                let j0 = if cy - k < 0 { (k - cy) as usize } else { 0 };
                for (j, row) in (j0..).zip(top..=bottom) {
                    let (lo, hi) = shape.spans[j];
                    let x0 = (lo + cx).max(left);
                    let x1 = (hi + cx).min(right);
                    self.write_span(row, x0, x1, mask);
                }
                return;
            }
            // the cast near the edge
            for f in flags.iter_mut().take(shape.n0) {
                *f = 2;
            }
            let left = if cx - 1 <= 0 { 0 } else { cx - 1 };
            let right = (cx + 1).min(w - 1);
            let top = if cy - 1 <= 0 { 0 } else { cy - 1 };
            let bottom = (cy + 1).min(h - 1);
            let mut n = 0usize;
            if top <= bottom {
                for row in top..=bottom {
                    let x0 = (cx - 1).max(left);
                    let x1 = (cx + 1).min(right);
                    for x in x0..=x1 {
                        let i = ((row as i64) << self.shift) + x as i64;
                        flags[n] = u8::from(self.elevation_at(i) > level);
                        n += 1;
                        if i >= 0 && (i as usize) < self.vis.len() {
                            self.write_words(i as usize, and_mask, or_mask);
                        }
                    }
                }
            }
            for r in shape.n0..shape.records.len() {
                let rec = shape.records[r];
                let f = flags[rec.b] | flags[rec.a];
                flags[r] = f;
                if f & 2 != 0 {
                    continue;
                }
                let x = rec.dx + cx;
                if !(0..w).contains(&x) {
                    flags[r] = f | 2;
                    continue;
                }
                let y = rec.dy + cy;
                if !(0..h).contains(&y) {
                    flags[r] = f | 2;
                    continue;
                }
                let i = c0 + i64::from(rec.dx) + i64::from(rec.dy) * i64::from(pitch);
                if f & 1 == 0 && i >= 0 && (i as usize) < self.vis.len() {
                    self.write_words(i as usize, and_mask, or_mask);
                }
                flags[r] = f | u8::from(self.elevation_at(i) > level);
            }
            return;
        }
        if u.ignore_cliffs {
            for (j, row) in (cy - k..=cy + k).enumerate() {
                let (lo, hi) = shape.spans[j];
                self.write_span(row, lo + cx, hi + cx, mask);
            }
            return;
        }
        // the central block (the constant 3 by 3 row list of the grid), then the records
        let mut n = 0usize;
        for row in cy - 1..=cy + 1 {
            for x in cx - 1..=cx + 1 {
                let i = ((row as i64) << self.shift) + x as i64;
                flags[n] = u8::from(self.elevation_at(i) > level);
                n += 1;
                self.write_words(i as usize, and_mask, or_mask);
            }
        }
        for r in shape.n0..shape.records.len() {
            let rec = shape.records[r];
            let f = flags[rec.b] | flags[rec.a];
            let i = c0 + i64::from(rec.dx) + i64::from(rec.dy) * i64::from(pitch);
            if f & 1 == 0 {
                self.write_words(i as usize, and_mask, or_mask);
            }
            flags[r] = f | u8::from(self.elevation_at(i) > level);
        }
    }

    /// The span writer in visible mode with the clamps of `0x6f406850` (see [`VisionGrid::span_indices`]).
    fn write_span(&mut self, row: i32, x0: i32, x1: i32, mask: u16) {
        let mut set = std::collections::BTreeSet::new();
        self.span_indices(row, x0, x1, &mut set);
        self.write_indices(FOG_VISIBLE, mask, &set);
    }
}

/// The unit's sight value as `0x6f29f040` returns it: the property value, or `1800.0` when the property is not
/// strictly below it (by at least `0.001`). CONFIRMED by disassembly.
pub fn clamp_sight(prop: F32) -> F32 {
    let p = prop.to_f32();
    let max = MAX_SIGHT.to_f32();
    if p < max && f32::from_bits(arith::sub(prop, MAX_SIGHT).to_bits() & 0x7FFF_FFFF) >= EPSILON {
        prop
    } else {
        MAX_SIGHT
    }
}

/// The cap that `0x6f29f040` applies for the units of a player with the object flag `+0x20 & 2` (unit flag
/// `+0x20 & 0x400` set): `50 + acquire` when the acquire range is not zero, else `250`. The player flag's meaning is not
/// decoded (UNSURE), so the simulation never applies it.
pub fn capped_sight(sight: F32, acquire: F32) -> F32 {
    let cap = if f32::from_bits(arith::sub(acquire, F32::ZERO).to_bits() & 0x7FFF_FFFF) >= EPSILON {
        arith::add(CAP_BASE, acquire)
    } else {
        CAP_DEFAULT
    };
    if sight.to_f32() > cap.to_f32() {
        cap
    } else {
        sight
    }
}

/// The reveal radius of a dying unit (`unit+0x5C & 0x20`): the sight is cut to the Misc value `DyingRevealRadius`.
pub fn dying_sight(sight: F32, dying_reveal_radius: F32) -> F32 {
    if sight.to_f32() > dying_reveal_radius.to_f32() {
        dying_reveal_radius
    } else {
        sight
    }
}

/// The rate of the sight modifier for a change of `delta` (`0x6f27a4e0`): `delta / 3.0`, over 3 seconds.
pub fn sight_ramp_rate(delta: F32, tables: &Tables) -> F32 {
    super::quotient(delta, SIGHT_RAMP_SECONDS, tables)
}
