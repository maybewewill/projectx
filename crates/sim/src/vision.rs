//! Fog-of-war grid: the per-cell, per-player visibility words, the writers that change them, the state lookup and
//! the shapes (rectangle, sight circle). The rules come from `docs/spec/vision.md` (decoded from `Game.dll` 1.26a).
//! The constructor, the span table, the circle writer and the cell writers are checked against the original in
//! Unicorn (`tools/emu/vision_grid_emu.py`, 300 grids each, 0 mismatches); the table and the writers are unit-tested
//! here against the same values.
//!
//! Model in one paragraph. The grid covers the playable area: the cell count is `trunc(range / 128)` per axis, and
//! the cell mapping is `scale = cells / range` (so the cells cover the area exactly, about 128 units each). Each
//! cell has two 16-bit words, one bit per player slot: `vis` (the player sees the cell now) and `msk` (the player has
//! not explored the cell: black mask). Three modes change them: [`FOG_MASKED`] sets the `msk` bit and clears the
//! `vis` bit, [`FOG_FOGGED`] clears both, [`FOG_VISIBLE`] sets the `vis` bit and clears the `msk` bit. There are no
//! counts: the last writer wins, and the visible layer is cleared at each refresh before the sources are applied.
//! The state of a cell for a player is then given by a table indexed by the two game flags (`FogMaskEnable`,
//! `FogEnable`).
//!
//! Storage is flat, as in the original: index `(row << shift) + col` with `1 << shift` the row pitch (the smallest
//! power of two at least the width). The single-cell writer reads both coordinates as bytes, so its indices wrap
//! modulo 256; the span and rectangle writers use full integers (both CONFIRMED by disassembly).
//!
//! Units. A unit's sight and its visibility query use the unit fog cell ([`VisionGrid::unit_cell`]): the pathing cell
//! of the unit shifted right by 2, so 128 units per cell whatever the grid scale (CONFIRMED for the centre function by
//! Unicorn, LIKELY for the footprint). The sight shape of a unit is the circle of [`VisionGrid::unit_sight_shape`]:
//! a stand-in, because the original's shape for radii of 3 and up is not the circle (vision.md section 11.9).

use std::collections::BTreeSet;

use softfloat::{arith, conv, F32, Tables};

use crate::pathing::grid::cell_coordinate;

/// The fog states of the JASS constants (CONFIRMED: the values the grid lookup returns).
pub const FOG_MASKED: i32 = 1;
pub const FOG_FOGGED: i32 = 2;
pub const FOG_VISIBLE: i32 = 4;

/// The cell size in world units: the constructor divides the playable size by it (`0x6fadb3e8` = 128.0, set at
/// start-up; CONFIRMED value).
pub const CELL_UNITS: F32 = F32::from_bits(0x4300_0000);

/// The grid used before a map sets its playable area (UNSURE: no game uses this size; it is a fallback).
pub const DEFAULT_CELLS: i32 = 256;

/// The word of an unexplored cell: the constructor fills the mask array with `0xFFFF` (CONFIRMED).
pub const UNEXPLORED: u16 = 0xFFFF;

/// `1/128` as a soft-float value: the default cell scale (the constructor's scale for a 128-unit cell).
const DEFAULT_SCALE: F32 = F32::from_bits(0x3C00_0000);

/// The unit path's radius scale: `1/128` (exact in binary, so the product is exact). The sight radius of a unit is
/// `f2i(radius * 1/128)` cells here (UNSURE: the original reads the shape value `unit+0x10C`, whose writer is not
/// decoded; a 128-unit fog cell is CONFIRMED for the unit path, see [`VisionGrid::unit_cell`]).
pub const UNIT_CELL_SCALE: F32 = F32::from_bits(0x3C00_0000);

/// Cell words of one grid cell: visible bits and mask bits, one bit per player slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellWords {
    pub vis: u16,
    pub msk: u16,
}

impl Default for CellWords {
    fn default() -> Self {
        CellWords { vis: 0, msk: UNEXPLORED }
    }
}

/// The state table of `0x6f00e7a0` (CONFIRMED by disassembly and by 1,000 Unicorn cases per branch).
///
/// `mask_enabled` is the flag at the grid's `+0x10` and `fog_enabled` the flag at `+0x14`. The row is `a + 2b` with
/// `a` and `b` the flags as 0 or 1: row 0 (neither) is always visible; row 1 (mask only) gives visible when the cell
/// is explored and masked otherwise; row 2 (fog only) gives visible when the cell is visible and fogged otherwise;
/// row 3 (both) is the full three-state rule.
pub fn table_state(mask_enabled: bool, fog_enabled: bool, vis: u16, msk: u16, bit: u16) -> i32 {
    let row: [i32; 3] = match (mask_enabled, fog_enabled) {
        (false, false) => [FOG_VISIBLE, FOG_VISIBLE, FOG_VISIBLE],
        (true, false) => [FOG_VISIBLE, FOG_VISIBLE, FOG_MASKED],
        (false, true) => [FOG_VISIBLE, FOG_FOGGED, FOG_FOGGED],
        (true, true) => [FOG_VISIBLE, FOG_FOGGED, FOG_MASKED],
    };
    if vis & bit != 0 {
        row[0]
    } else if bit & !msk & 0x0FFF != 0 {
        row[1]
    } else {
        row[2]
    }
}

/// The cells the per-row writers (`0x6f406920`, `0x6f4069f0`, `0x6f406850`) touch for the span `[x0, x1]` after
/// their clamps (`x0 >= 0`, `x1 <= width-1`). The writer works on pairs of cells (one dword store per pair): it
/// stores the start cell alone when it is odd, then the pairs from the next even cell up to `x1 - 1` (or `x1` when
/// `x1` is odd), then the cell after the pairs alone when `x1` is even. When `x0 > x1` the odd start and the tail
/// still store, so up to two cells outside the span are written (CONFIRMED, Unicorn: 1,000 random cases).
pub fn span_cells(x0: i32, x1: i32) -> Vec<i32> {
    let mut cells = Vec::new();
    let u = x0 & 1;
    if u != 0 {
        cells.push(x0);
    }
    let mut p = x0 + u;
    let e = x1 - if x1 % 2 == 0 { 1 } else { 0 };
    if p < e {
        let count = ((e - p - 1) >> 1) + 1;
        for k in 0..count {
            cells.push(p + 2 * k);
            cells.push(p + 2 * k + 1);
        }
        p += 2 * count;
    }
    if x1 % 2 == 0 {
        cells.push(p);
    }
    cells
}

/// The octant table of `0x6f408800`: the x of every south-east step of the midpoint circle of radius `r`, starting
/// at `(0, r)` (CONFIRMED by disassembly and by the Unicorn span table runs, 158 radii). The first step is east
/// while the decision value is negative.
pub fn octant_steps(r: i32) -> Vec<i32> {
    let mut rec = Vec::new();
    let (mut x, mut y) = (0i32, r);
    let mut d = 3 - 2 * r;
    let mut delta = -4 * r + 10;
    let mut east = 6;
    loop {
        let (cur, next) = if d < 0 {
            (east, delta + 4)
        } else {
            rec.push(x);
            y -= 1;
            (delta, delta + 8)
        };
        d += cur;
        east += 4;
        x += 1;
        delta = next;
        if x > y {
            break;
        }
    }
    rec
}

/// The span table of `0x6f408fd0` for radius `r >= 1`: `2r + 1` pairs `(lo, hi)`; entry `j` is the row `dy = j - r`
/// and its extent is `[cx + lo, cx + hi]` (CONFIRMED by disassembly, 158 radii in Unicorn, 0 mismatches). The octant
/// steps fill the top and bottom entries with their own extent, and the rows between take the extent of the step
/// at which they are reached (`r - k`).
pub fn span_table(r: i32) -> Vec<(i32, i32)> {
    let n = (2 * r + 1) as usize;
    let oct = octant_steps(r);
    let mut t = vec![(0i32, 0i32); n];
    t[r as usize] = (-r, r);
    let mut level = 1i32;
    let (mut lo_k, mut hi_k) = (-r, r);
    let (mut i_lo, mut i_hi) = (r - 1, r + 1);
    for (k, &x) in oct.iter().enumerate() {
        let k = k as i32;
        t[k as usize] = (-x, x);
        t[(2 * r - k) as usize] = (-x, x);
        if level <= x {
            let cnt = x - level + 1;
            level += cnt;
            for _ in 0..cnt {
                t[i_lo as usize] = (lo_k, hi_k);
                t[i_hi as usize] = (lo_k, hi_k);
                i_lo -= 1;
                i_hi += 1;
            }
        }
        lo_k += 1;
        hi_k -= 1;
    }
    t
}

/// Cell index of a world coordinate on one axis: `clamp(f2i((v - origin) * scale), 0, limit - 1)`. This is
/// `0x6f192620` (CONFIRMED by disassembly): a soft-float subtract, a soft-float multiply and a truncating conversion,
/// then the clamp to `[0, limit-1]` written with an unsigned compare.
pub fn cell_index(v: F32, origin: F32, scale: F32, limit: i32) -> i32 {
    let c = conv::f2i(arith::mul(arith::sub(v, origin), scale));
    c.clamp(0, limit - 1)
}

/// The soft-float quotient of `0x6f6efe10` (CONFIRMED by disassembly): equal operands (compared as bits) give `1.0`,
/// otherwise the division.
fn quotient(a: F32, b: F32, t: &Tables) -> F32 {
    if a.to_bits() == b.to_bits() {
        F32::from_bits(0x3F80_0000)
    } else {
        arith::div(a, b, t)
    }
}

/// Row pitch and shift of a grid of `width` columns: the smallest power of two at least `width`, at least 2
/// (`0x6f408d10`, CONFIRMED).
fn pitch_for(width: i32) -> (i32, u32) {
    let mut pitch = 2i32;
    let mut shift = 1u32;
    while pitch < width {
        pitch <<= 1;
        shift += 1;
    }
    (pitch, shift)
}

/// The fog grid of the game: the cell words, the two flags and the cell mapping.
#[derive(Debug, Clone)]
pub struct VisionGrid {
    /// Grid size in cells (`+0x60`, `+0x6c`). Queries and the cell mapping clamp to `[0, width-1] x [0, height-1]`.
    pub width: i32,
    pub height: i32,
    /// Row pitch (`1 << shift`) and shift (`+0x64`, `+0x68`).
    pub pitch: i32,
    pub shift: u32,
    /// Origin of the cell mapping: `(x, y)` = (`+0x48`, `+0x44`), the playable minimum (CONFIRMED).
    pub origin: (F32, F32),
    /// Cells per world unit: `(x, y)` = (`+0x70`, `+0x74`). From a playable area it is `cells / range` per axis.
    pub scale: (F32, F32),
    /// `FogMaskEnable` (grid `+0x10`). Default on.
    pub mask_enabled: bool,
    /// `FogEnable` (grid `+0x14`). Default on.
    pub fog_enabled: bool,
    /// The visible and mask words, `pitch * height` entries each (index `(row << shift) + col`).
    vis: Vec<u16>,
    msk: Vec<u16>,
}

impl Default for VisionGrid {
    /// The fallback grid used before a map sets its playable area (UNSURE: no game uses this size).
    fn default() -> Self {
        VisionGrid::with_cells(DEFAULT_CELLS, DEFAULT_CELLS, (F32::ZERO, F32::ZERO), (DEFAULT_SCALE, DEFAULT_SCALE))
    }
}

impl VisionGrid {
    /// A grid of `width` by `height` cells with the given mapping, all words at their initial values.
    pub fn with_cells(width: i32, height: i32, origin: (F32, F32), scale: (F32, F32)) -> VisionGrid {
        let width = width.max(1);
        let height = height.max(1);
        let (pitch, shift) = pitch_for(width);
        let n = (pitch * height) as usize;
        VisionGrid {
            width,
            height,
            pitch,
            shift,
            origin,
            scale,
            mask_enabled: true,
            fog_enabled: true,
            vis: vec![0; n],
            msk: vec![UNEXPLORED; n],
        }
    }

    /// The grid of a playable area `[min_x, max_x] x [min_y, max_y]` (the constructor `0x6f408d10`, CONFIRMED by
    /// Unicorn on 300 rectangles): `width = trunc(range_x / 128)` (the same for the height), the origin is the
    /// minimum, and the scale is `width / range_x`. A degenerate area (under 128 units on an axis) is kept at one
    /// cell (UNSURE: the original's behaviour for a zero width is not run).
    pub fn from_playable(min_x: F32, min_y: F32, max_x: F32, max_y: F32, tables: &Tables) -> VisionGrid {
        let rx = arith::sub(max_x, min_x);
        let ry = arith::sub(max_y, min_y);
        let w = conv::f2i(quotient(rx, CELL_UNITS, tables));
        let h = conv::f2i(quotient(ry, CELL_UNITS, tables));
        let sx = quotient(conv::i2r(w), rx, tables);
        let sy = quotient(conv::i2r(h), ry, tables);
        VisionGrid::with_cells(w, h, (min_x, min_y), (sx, sy))
    }

    /// The cell `(x, y)` of a world point, clamped to the grid (the mapping of `0x6f192620`).
    pub fn cell_of_point(&self, x: F32, y: F32) -> (i32, i32) {
        (
            cell_index(x, self.origin.0, self.scale.0, self.width),
            cell_index(y, self.origin.1, self.scale.1, self.height),
        )
    }

    /// The flat index of a cell for the single-cell writer: both coordinates are bytes (CONFIRMED). `None` when the
    /// index is outside the arrays.
    fn index_byte(&self, col: i32, row: i32) -> Option<usize> {
        let i = (((row & 0xFF) as i64) << self.shift) + (col & 0xFF) as i64;
        self.checked(i)
    }

    /// The flat index of a cell for the span and rectangle writers: full integers (CONFIRMED). `None` when outside.
    fn index_raw(&self, col: i32, row: i32) -> Option<usize> {
        let i = ((row as i64) << self.shift) + col as i64;
        self.checked(i)
    }

    fn checked(&self, i: i64) -> Option<usize> {
        (i >= 0 && (i as usize) < self.vis.len()).then_some(i as usize)
    }

    /// The words of a cell, or the default words when the index is outside the arrays.
    pub fn words(&self, cell: (i32, i32)) -> CellWords {
        match self.index_raw(cell.0, cell.1) {
            Some(i) => CellWords { vis: self.vis[i], msk: self.msk[i] },
            None => CellWords::default(),
        }
    }

    /// The state of cell `cell` for the player in slot `player` (`0..16`), through [`table_state`]. The cell is
    /// clamped to the grid, as the lookup does.
    pub fn state(&self, cell: (i32, i32), player: u32) -> i32 {
        let cell = (cell.0.clamp(0, self.width - 1), cell.1.clamp(0, self.height - 1));
        let w = self.words(cell);
        table_state(self.mask_enabled, self.fog_enabled, w.vis, w.msk, 1u16 << (player & 15))
    }

    /// The fog cell of a unit at the world point `(x, y)`, on the unit path (`0x6f409e00` and the unit query
    /// `0x6f285080` -> `0x6f3a38f0`, vision.md section 11.9). The original takes the centre of the unit's footprint in
    /// pathing cells (`0x6f474460`, CONFIRMED by Unicorn) and shifts it right by 2: a fog cell is 4 by 4 pathing cells
    /// = 128 world units, whatever the grid's own scale. The footprint is taken as the unit's own pathing cell
    /// (LIKELY: the footprint rectangle of a unit is not decoded). Not clamped: the writers and [`VisionGrid::state`]
    /// clamp it.
    pub fn unit_cell(&self, x: F32, y: F32) -> (i32, i32) {
        (cell_coordinate(x, self.origin.0) >> 2, cell_coordinate(y, self.origin.1) >> 2)
    }

    /// The state of the world point `(x, y)` for the player in slot `player`.
    pub fn state_at(&self, x: F32, y: F32, player: u32) -> i32 {
        self.state(self.cell_of_point(x, y), player)
    }

    /// Applies a writer `mode` with `mask` to one flat index.
    fn apply(&mut self, mode: i32, mask: u16, i: usize) {
        match mode {
            FOG_MASKED => {
                self.msk[i] |= mask;
                self.vis[i] &= !mask;
            }
            FOG_FOGGED => {
                self.msk[i] &= !mask;
                self.vis[i] &= !mask;
            }
            FOG_VISIBLE => {
                self.msk[i] &= !mask;
                self.vis[i] |= mask;
            }
            _ => {}
        }
    }

    /// Writes the given flat indices with one mode and player mask (the modifier and explicit-state writers).
    pub fn write_indices(&mut self, mode: i32, mask: u16, indices: &BTreeSet<usize>) {
        for &i in indices {
            self.apply(mode, mask, i);
        }
    }

    /// Clears the visible layer of every word (the original's memset of `(height << shift)` words, CONFIRMED).
    pub fn clear_visible(&mut self) {
        self.vis.fill(0);
    }

    /// `FogEnable(v)`: stores the flag and, when it is off, clears the visible layer (CONFIRMED, `0x6f407e60`).
    pub fn set_fog_enabled(&mut self, v: bool) {
        self.fog_enabled = v;
        if !v {
            self.clear_visible();
        }
    }

    /// `FogMaskEnable(v)`: stores the flag (the mask clear it may also do is not wired, UNSURE).
    pub fn set_mask_enabled(&mut self, v: bool) {
        self.mask_enabled = v;
    }

    /// The flat indices the span writer touches for row `row` and the span `[x0, x1]`: nothing outside the rows,
    /// `x0` clamped to 0 and `x1` to `width - 1` (CONFIRMED).
    fn span_indices(&self, row: i32, x0: i32, x1: i32, out: &mut BTreeSet<usize>) {
        if !(0..self.height).contains(&row) {
            return;
        }
        let x0 = x0.max(0);
        let x1 = x1.min(self.width - 1);
        for c in span_cells(x0, x1) {
            if let Some(i) = self.index_raw(c, row) {
                out.insert(i);
            }
        }
    }

    /// The flat indices of the rectangle writer `0x6f3b0e90` for `[x0, x1) x [y0, y1)` after its clamps (the start
    /// to 0, the end to `width-1` / `height-1`; the end is exclusive, CONFIRMED).
    fn rect_indices(&self, x0: i32, y0: i32, x1: i32, y1: i32, out: &mut BTreeSet<usize>) {
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width - 1);
        let y1 = y1.min(self.height - 1);
        for r in y0..y1 {
            for c in x0..x1 {
                if let Some(i) = self.index_raw(c, r) {
                    out.insert(i);
                }
            }
        }
    }

    /// The rectangle of world corners `min` and `max`: the cells of both corners (the mapping, clamped), with the end
    /// exclusive (`0x6f3b76e0`, CONFIRMED structure).
    pub fn rect_shape(&self, min: (F32, F32), max: (F32, F32)) -> BTreeSet<usize> {
        let (x0, y0) = self.cell_of_point(min.0, min.1);
        let (x1, y1) = self.cell_of_point(max.0, max.1);
        let mut out = BTreeSet::new();
        self.rect_indices(x0, y0, x1, y1, &mut out);
        out
    }

    /// The flat indices the sight circle writer `0x6f3ba480` touches for a circle of world radius `radius` centred on
    /// `(x, y)`. The radius in cells is `f2i(radius * scale_x)` (CONFIRMED). A radius of 0 writes nothing; 1 writes the
    /// rectangle `[cx-1, cx+1) x [cy-1, cy+1)`; 2 adds the four single cells `(cx±2, cy)` and `(cx, cy±2)` through the
    /// byte-wide cell writer (so they wrap, CONFIRMED). From 3 up the rows `[cy-R, min(cy+R, h))` each take the span of
    /// [`span_table`] (CONFIRMED by Unicorn, 300 circles including grids wider than 256 cells, 0 mismatches).
    pub fn circle_shape(&self, x: F32, y: F32, radius: F32) -> BTreeSet<usize> {
        let r = conv::f2i(arith::mul(radius, self.scale.0));
        let (cx, cy) = self.cell_of_point(x, y);
        self.circle_shape_cells(cx, cy, r)
    }

    /// The sight circle of radius `r` cells around the cell `(cx, cy)`: the shapes of [`VisionGrid::circle_shape`]
    /// after its mapping (`r <= 0` writes nothing).
    pub fn circle_shape_cells(&self, cx: i32, cy: i32, r: i32) -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        if r <= 0 {
            return out;
        }
        if r <= 2 {
            self.rect_indices(cx - 1, cy - 1, cx + 1, cy + 1, &mut out);
            if r == 2 {
                for (c, rw) in [(cx - 2, cy), (cx + 2, cy), (cx, cy - 2), (cx, cy + 2)] {
                    if let Some(i) = self.index_byte(c, rw) {
                        out.insert(i);
                    }
                }
            }
            return out;
        }
        let table = span_table(r);
        let y0 = cy - r;
        let y1 = if self.height <= cy + r { self.height } else { cy + r };
        for row in y0.max(0)..y1 {
            let (lo, hi) = table[(row - y0) as usize];
            let x0 = (cx + lo).max(0);
            let x1 = if self.width <= cx + hi { self.width } else { cx + hi };
            self.span_indices(row, x0, x1, &mut out);
        }
        out
    }

    /// Writes the sight circle of [`VisionGrid::circle_shape`] with one mode and player mask.
    pub fn write_circle(&mut self, mode: i32, mask: u16, x: F32, y: F32, radius: F32) {
        let cells = self.circle_shape(x, y, radius);
        self.write_indices(mode, mask, &cells);
    }

    /// The unit sight circle: the radius in cells is `f2i(radius * 1/128)` ([`UNIT_CELL_SCALE`], UNSURE) around the
    /// unit's fog cell ([`VisionGrid::unit_cell`]). The original's row shape for radii of 3 and up is not the span table
    /// of [`span_table`] (vision.md section 11.9, UNSURE): this is the stand-in shape until it is decoded.
    pub fn unit_sight_shape(&self, x: F32, y: F32, radius: F32) -> BTreeSet<usize> {
        let r = conv::f2i(arith::mul(radius, UNIT_CELL_SCALE));
        let (cx, cy) = self.unit_cell(x, y);
        self.circle_shape_cells(cx, cy, r)
    }

    /// Writes the unit sight circle of [`VisionGrid::unit_sight_shape`] in visible mode with the player mask.
    pub fn write_unit_sight(&mut self, mask: u16, x: F32, y: F32, radius: F32) {
        let cells = self.unit_sight_shape(x, y, radius);
        self.write_indices(FOG_VISIBLE, mask, &cells);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    /// A grid with the default 128-unit mapping and `w` by `h` cells.
    fn grid(w: i32, h: i32) -> VisionGrid {
        VisionGrid::with_cells(w, h, (F32::ZERO, F32::ZERO), (DEFAULT_SCALE, DEFAULT_SCALE))
    }

    fn word(g: &VisionGrid, c: (i32, i32)) -> CellWords {
        g.words(c)
    }

    #[test]
    fn table_rows_follow_the_flags() {
        assert_eq!(table_state(false, false, 0, 0xFFFF, 1), FOG_VISIBLE);
        assert_eq!(table_state(true, false, 0, 0, 1), FOG_VISIBLE);
        assert_eq!(table_state(true, false, 0, 1, 1), FOG_MASKED);
        assert_eq!(table_state(false, true, 1, 0xFFFF, 1), FOG_VISIBLE);
        assert_eq!(table_state(false, true, 0, 0xFFFF, 1), FOG_FOGGED);
        assert_eq!(table_state(true, true, 1, 0xFFFF, 1), FOG_VISIBLE);
        assert_eq!(table_state(true, true, 0, 0, 1), FOG_FOGGED);
        assert_eq!(table_state(true, true, 0, 1, 1), FOG_MASKED);
    }

    #[test]
    fn table_only_reads_the_player_bit() {
        assert_eq!(table_state(true, true, 0b100, 0, 1 << 3), FOG_FOGGED);
        assert_eq!(table_state(true, true, 0b1000, 0, 1 << 3), FOG_VISIBLE);
    }

    #[test]
    fn modes_write_the_two_layers() {
        let mut g = grid(16, 16);
        let i = g.index_raw(5, 5).unwrap();
        let set: BTreeSet<usize> = [i].into_iter().collect();
        g.write_indices(FOG_VISIBLE, 0b01, &set);
        assert_eq!(word(&g, (5, 5)), CellWords { vis: 0b01, msk: 0xFFFE });
        assert_eq!(g.state((5, 5), 0), FOG_VISIBLE);
        g.write_indices(FOG_FOGGED, 0b01, &set);
        assert_eq!(g.state((5, 5), 0), FOG_FOGGED);
        g.write_indices(FOG_MASKED, 0b01, &set);
        assert_eq!(g.state((5, 5), 0), FOG_MASKED);
        assert_eq!(word(&g, (5, 5)).msk & 0b10, 0b10, "other players are untouched");
    }

    #[test]
    fn span_cells_match_the_pairwise_writer() {
        assert_eq!(span_cells(2, 4), vec![2, 3, 4]);
        assert_eq!(span_cells(3, 8), vec![3, 4, 5, 6, 7, 8]);
        assert_eq!(span_cells(4, 4), vec![4]);
        assert_eq!(span_cells(3, 3), vec![3]);
        assert_eq!(span_cells(11, 6), vec![11, 12]);
        assert_eq!(span_cells(10, 6), vec![10]);
    }

    #[test]
    fn span_tables_match_the_original() {
        // Unicorn runs of 0x6f408fd0 (vision_grid_emu.py, 158 radii; the values below are from the same runs).
        assert_eq!(
            span_table(3),
            vec![(-1, 1), (-2, 2), (-3, 3), (-3, 3), (-3, 3), (-2, 2), (-1, 1)]
        );
        assert_eq!(
            span_table(4),
            vec![(-1, 1), (-3, 3), (-3, 3), (-4, 4), (-4, 4), (-4, 4), (-3, 3), (-3, 3), (-1, 1)]
        );
        assert_eq!(
            span_table(5),
            vec![
                (-2, 2),
                (-3, 3),
                (-4, 4),
                (-5, 5),
                (-5, 5),
                (-5, 5),
                (-5, 5),
                (-5, 5),
                (-4, 4),
                (-3, 3),
                (-2, 2)
            ]
        );
    }

    #[test]
    fn octant_steps_match_the_original() {
        // Unicorn: the south-east steps of the midpoint circle (0x6f408800).
        assert_eq!(octant_steps(3), vec![1, 2]);
        assert_eq!(octant_steps(4), vec![1, 3]);
        assert_eq!(octant_steps(5), vec![2, 3]);
        assert_eq!(octant_steps(8), vec![2, 4, 5]);
        assert_eq!(octant_steps(12), vec![3, 5, 7, 8]);
    }

    #[test]
    fn span_table_has_2r_plus_1_rows_and_a_full_centre() {
        for r in 3..=40 {
            let t = span_table(r);
            assert_eq!(t.len(), (2 * r + 1) as usize);
            assert_eq!(t[r as usize], (-r, r));
        }
    }

    #[test]
    fn point_to_cell_clamps_to_the_grid() {
        let g = grid(256, 256);
        assert_eq!(g.cell_of_point(f(0.0), f(0.0)), (0, 0));
        assert_eq!(g.cell_of_point(f(127.9), f(128.0)), (0, 1));
        assert_eq!(g.cell_of_point(f(-500.0), f(1.0e9)), (0, 255));
    }

    #[test]
    fn circle_radius_in_cells_and_small_shapes() {
        let g = grid(256, 256);
        // a radius below one cell writes nothing
        assert!(g.circle_shape(f(1000.0), f(1000.0), f(100.0)).is_empty());
        // one cell: the 2x2 block at the centre cell (1000 is in cell 7, so cells 6..7)
        let r1 = g.circle_shape(f(1000.0), f(1000.0), f(128.0));
        let cells: Vec<(i32, i32)> = r1.iter().map(|&i| (i as i32 & 255, i as i32 >> 8)).collect();
        assert_eq!(cells, vec![(6, 6), (7, 6), (6, 7), (7, 7)]);
        // two cells: the 2x2 block plus the four arms
        assert_eq!(g.circle_shape(f(1000.0), f(1000.0), f(256.0)).len(), 8);
    }

    #[test]
    fn large_circle_rows_are_end_exclusive() {
        let g = grid(256, 256);
        let cells = g.circle_shape(f(128.0 * 50.0), f(128.0 * 50.0), f(128.0 * 4.0));
        let rows: BTreeSet<usize> = cells.iter().map(|&i| i >> 8).collect();
        assert_eq!(rows.iter().next().copied(), Some(46));
        assert_eq!(rows.iter().next_back().copied(), Some(53), "row cy+R is not written");
    }

    #[test]
    fn single_cell_writer_wraps_bytes_and_span_writer_does_not() {
        // The single-cell writer reads its coordinates as bytes (CONFIRMED): a column of -1 is column 255 on the same
        // row. The span and rectangle writers use full integers: the index of column -1 is the previous row's last.
        let g = grid(300, 8);
        assert_eq!(g.index_byte(-1, 2), g.index_raw(255, 2));
        assert_eq!(g.index_raw(-1, 2), Some((1usize << g.shift) * 2 - 1));
    }

    #[test]
    fn clear_and_flags() {
        let mut g = grid(16, 16);
        let set: BTreeSet<usize> = [g.index_raw(3, 3).unwrap()].into_iter().collect();
        g.write_indices(FOG_VISIBLE, 1, &set);
        g.clear_visible();
        assert_eq!(g.words((3, 3)).vis, 0);
        g.write_indices(FOG_VISIBLE, 1, &set);
        g.set_fog_enabled(false);
        assert_eq!(g.words((3, 3)).vis, 0, "FogEnable(false) clears the visible layer");
        assert!(!g.fog_enabled);
    }

    #[test]
    fn pitch_is_the_next_power_of_two() {
        assert_eq!(pitch_for(1), (2, 1));
        assert_eq!(pitch_for(2), (2, 1));
        assert_eq!(pitch_for(3), (4, 2));
        assert_eq!(pitch_for(17), (32, 5));
        assert_eq!(pitch_for(256), (256, 8));
    }

    #[test]
    fn unit_cells_are_128_units_whatever_the_grid_scale() {
        // A 1000-unit playable width: 7 cells with scale 7/1000 (not 1/128). The unit path takes the pathing cell
        // (x / 32) and shifts it by 2: x = 900 is pathing cell 28 and fog cell 7. The grid mapping of the point query
        // gives floor(900 * 0.007) = 6, so the two differ on such maps (the original does both, vision.md section 11.9).
        let g = VisionGrid::with_cells(7, 7, (F32::ZERO, F32::ZERO), (f(0.007), f(0.007)));
        assert_eq!(g.unit_cell(f(900.0), f(900.0)), (7, 7));
        assert_eq!(g.cell_of_point(f(900.0), f(900.0)), (6, 6));
        // 128 units per cell, from the origin
        assert_eq!(g.unit_cell(f(127.9), f(0.0)), (0, 0));
        assert_eq!(g.unit_cell(f(128.0), f(0.0)), (1, 0));
        assert_eq!(g.unit_cell(f(255.9), f(383.9)), (1, 2));
    }

    #[test]
    fn unit_cell_is_relative_to_the_playable_minimum() {
        // The pathing cells are counted from the playable minimum: a minimum of -640 moves the cell by five.
        let g = VisionGrid::with_cells(8, 8, (f(-640.0), f(-640.0)), (f(0.0625), f(0.0625)));
        assert_eq!(g.unit_cell(f(-640.0), f(-640.0)), (0, 0));
        assert_eq!(g.unit_cell(f(-512.0), f(-512.0)), (1, 1));
        assert_eq!(g.unit_cell(f(0.0), f(0.0)), (5, 5));
    }

    #[test]
    fn unit_sight_radius_is_whole_128_unit_cells() {
        let g = grid(16, 16);
        // 512 units is 4 cells (the footman of the tests); the unit centre is the cell of (1000, 1000), cell 7.
        assert_eq!(g.unit_sight_shape(f(1000.0), f(1000.0), f(512.0)), g.circle_shape_cells(7, 7, 4));
        // 500 units is 3 cells (truncation), and a radius below 128 writes nothing
        assert_eq!(g.unit_sight_shape(f(1000.0), f(1000.0), f(500.0)), g.circle_shape_cells(7, 7, 3));
        assert!(g.unit_sight_shape(f(1000.0), f(1000.0), f(100.0)).is_empty());
    }

    #[test]
    fn sight_writes_only_set_visible_so_their_order_does_not_matter() {
        // Two visible writes with overlapping masks give the same words in either order (the sight pass of the refresh
        // only ever writes FOG_VISIBLE; see vision.md section 11.10).
        let mut a = grid(16, 16);
        let mut b = grid(16, 16);
        a.write_unit_sight(0b01, f(1000.0), f(1000.0), f(384.0));
        a.write_unit_sight(0b10, f(1100.0), f(1000.0), f(256.0));
        b.write_unit_sight(0b10, f(1100.0), f(1000.0), f(256.0));
        b.write_unit_sight(0b01, f(1000.0), f(1000.0), f(384.0));
        assert_eq!(a.vis, b.vis);
        assert_eq!(a.msk, b.msk);
    }
}
