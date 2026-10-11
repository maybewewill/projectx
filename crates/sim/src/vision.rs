//! Fog-of-war grid: the per-cell, per-player visibility words, the writers that change them, the state lookup
//! and the shapes (rectangle, sight circle) that the writers take. The rules come from `docs/spec/vision.md`
//! (decoded from `Game.dll` 1.26a); the writers and the state table are checked against the original in
//! Unicorn (`tools/emu/vision_emu.py`).
//!
//! Model in one paragraph. Each grid cell has two 16-bit words, one bit per player slot: `vis` (the player
//! sees the cell now) and `msk` (the player has not explored the cell: black mask). Three writers change
//! them: mode [`FOG_MASKED`] sets the `msk` bit and clears the `vis` bit, mode [`FOG_FOGGED`] clears both,
//! mode [`FOG_VISIBLE`] sets the `vis` bit and clears the `msk` bit. There are no counts: the last writer
//! wins, and the whole visible layer is rebuilt by [`VisionGrid::clear_visible`] followed by re-applying the
//! sources. The state of a cell for a player is then given by a table indexed by the two game flags
//! (`FogMaskEnable`, `FogEnable`).

use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

/// The fog states of the JASS constants (CONFIRMED: the values the grid lookup returns).
pub const FOG_MASKED: i32 = 1;
pub const FOG_FOGGED: i32 = 2;
pub const FOG_VISIBLE: i32 = 4;

/// Default grid size in cells (UNSURE: the map's size is not read into the grid yet).
pub const DEFAULT_CELLS: i32 = 256;

/// The word of an unexplored cell (UNSURE: the initial value of the original's mask layer). Every player bit set.
pub const UNEXPLORED: u16 = 0xFFFF;

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
/// `mask_enabled` is the flag at the grid's `+0x10` and `fog_enabled` the flag at `+0x14`. The row is
/// `a + 2b` with `a` and `b` the flags as 0 or 1: row 0 (neither) is always visible; row 1 (mask only) gives
/// visible when the cell is explored and masked otherwise; row 2 (fog only) gives visible when the cell is
/// visible and fogged otherwise; row 3 (both) is the full three-state rule.
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

/// The cells a per-row writer touches for the span `[x0, x1]` after its clamps (`x0 >= 0`, `x1 <= width-1`),
/// in the order the original stores them (`0x6f406920`, `0x6f4069f0`, `0x6f406850`). The writer works on
/// pairs of cells (one dword store per pair): it stores the start cell alone when it is odd, then the pairs
/// from the next even cell up to `x1 - 1` (or `x1` when `x1` is odd), then the cell after the pairs alone when
/// `x1` is even. When `x0 > x1` the odd start and the tail still store, so up to two cells outside the span are
/// written. This quirk is CONFIRMED by Unicorn (span writers, 1,000 random cases, 0 mismatches).
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

/// Cell index of a world coordinate on one axis: `clamp(f2i((v - origin) * scale), 0, limit - 1)`.
/// This is `0x6f192620` (CONFIRMED by disassembly): a soft-float subtract, a soft-float multiply and a
/// truncating conversion, then the clamp to `[0, limit-1]` written with an unsigned compare.
pub fn cell_index(v: F32, origin: F32, scale: F32, limit: i32) -> i32 {
    let c = conv::f2i(arith::mul(arith::sub(v, origin), scale));
    c.clamp(0, limit - 1)
}

/// The fog grid of the game: the cell words, the two flags and the cell mapping. Cells that were never
/// written hold [`CellWords::default`].
#[derive(Debug, Clone)]
pub struct VisionGrid {
    /// Grid size in cells. Queries and writers clamp to `[0, width-1] x [0, height-1]`.
    pub width: i32,
    pub height: i32,
    /// Origin of the cell mapping (world units) and the scale (cells per world unit).
    /// UNSURE: the original takes both from the grid object; the defaults are `0` and `1/128`.
    pub origin: (F32, F32),
    pub scale: F32,
    /// `FogMaskEnable` (grid `+0x10`). Default on.
    pub mask_enabled: bool,
    /// `FogEnable` (grid `+0x14`). Default on.
    pub fog_enabled: bool,
    cells: BTreeMap<(i32, i32), CellWords>,
}

/// `1/128` as a soft-float value: the cell scale of the fog grid (UNSURE, see [`VisionGrid::scale`]).
const CELL_SCALE: F32 = F32::from_bits(0x3C00_0000);

impl Default for VisionGrid {
    fn default() -> Self {
        VisionGrid {
            width: DEFAULT_CELLS,
            height: DEFAULT_CELLS,
            origin: (F32::from_bits(0), F32::from_bits(0)),
            scale: CELL_SCALE,
            mask_enabled: true,
            fog_enabled: true,
            cells: BTreeMap::new(),
        }
    }
}

impl VisionGrid {
    /// The cell `(x, y)` of a world point, clamped to the grid.
    pub fn cell_of_point(&self, x: F32, y: F32) -> (i32, i32) {
        (
            cell_index(x, self.origin.0, self.scale, self.width),
            cell_index(y, self.origin.1, self.scale, self.height),
        )
    }

    /// The words of a cell (default when never written). The coordinates are not clamped here.
    pub fn words(&self, cell: (i32, i32)) -> CellWords {
        self.cells.get(&cell).copied().unwrap_or_default()
    }

    /// The state of cell `cell` for the player in slot `player` (`0..16`), through [`table_state`].
    pub fn state(&self, cell: (i32, i32), player: u32) -> i32 {
        let cell = (cell.0.clamp(0, self.width - 1), cell.1.clamp(0, self.height - 1));
        let w = self.words(cell);
        table_state(self.mask_enabled, self.fog_enabled, w.vis, w.msk, 1u16 << (player & 15))
    }

    /// The state of the world point `(x, y)` for the player in slot `player`.
    pub fn state_at(&self, x: F32, y: F32, player: u32) -> i32 {
        self.state(self.cell_of_point(x, y), player)
    }

    /// Stores one cell with a writer `mode` (`FOG_MASKED`, `FOG_FOGGED` or `FOG_VISIBLE`) and a player mask.
    /// Any other mode writes nothing (as the original does). `cell` must already be inside the grid.
    pub fn write_cell(&mut self, mode: i32, mask: u16, cell: (i32, i32)) {
        let w = self.cells.entry(cell).or_default();
        match mode {
            FOG_MASKED => {
                w.msk |= mask;
                w.vis &= !mask;
            }
            FOG_FOGGED => {
                w.msk &= !mask;
                w.vis &= !mask;
            }
            FOG_VISIBLE => {
                w.msk &= !mask;
                w.vis |= mask;
            }
            _ => {}
        }
    }

    /// Writes a span of one row with the pair-wise writer (see [`span_cells`]), after the row and end clamps.
    pub fn write_span(&mut self, mode: i32, mask: u16, row: i32, x0: i32, x1: i32) {
        if !(0..self.height).contains(&row) {
            return;
        }
        let x0 = x0.max(0);
        let x1 = x1.min(self.width - 1);
        for c in span_cells(x0, x1) {
            self.write_cell(mode, mask, (c, row));
        }
    }

    /// Writes the rectangle `[x0, x1) x [y0, y1)` (end exclusive) with the rectangle writer `0x6f3b0e90`
    /// (CONFIRMED by Unicorn, 1,000 random cases per mode, 0 mismatches). The start is clamped to 0, the end to
    /// `width-1` and `height-1`.
    pub fn write_rect(&mut self, mode: i32, mask: u16, x0: i32, y0: i32, x1: i32, y1: i32) {
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.width - 1);
        let y1 = y1.min(self.height - 1);
        for r in y0..y1 {
            for c in x0..x1 {
                self.write_cell(mode, mask, (c, r));
            }
        }
    }

    /// Writes a set of cells (the shapes below) for one mode and one player mask.
    pub fn write_cells(&mut self, mode: i32, mask: u16, cells: &BTreeSet<(i32, i32)>) {
        for &cell in cells {
            self.write_cell(mode, mask, cell);
        }
    }

    /// Clears the visible layer of every cell (the start of a rebuild of the sources).
    pub fn clear_visible(&mut self) {
        for w in self.cells.values_mut() {
            w.vis = 0;
        }
    }

    /// The cell rectangle of a world rectangle given by its min and max corners, as the writer receives it:
    /// `[cell(min), cell(max))` on each axis (CONFIRMED by `0x6f3b76e0`: both corners go through `0x6f192620`).
    pub fn rect_shape(&self, min: (F32, F32), max: (F32, F32)) -> BTreeSet<(i32, i32)> {
        let (x0, y0) = self.cell_of_point(min.0, min.1);
        let (x1, y1) = self.cell_of_point(max.0, max.1);
        let mut out = BTreeSet::new();
        for r in y0..y1 {
            for c in x0..x1 {
                out.insert((c, r));
            }
        }
        out
    }

    /// The cells of a sight circle of world radius `radius` centred on `(x, y)`.
    ///
    /// The radius in cells is `f2i(radius * scale)` (CONFIRMED, `0x6f3ba480`). A radius of 0 writes nothing; a
    /// radius of 1 writes the 2x2 block `[cx-1, cx+1) x [cy-1, cy+1)` (the exclusive end of the rectangle writer,
    /// CONFIRMED from the call arguments); a radius of 2 adds the four cells `(cx±2, cy)` and `(cx, cy±2)`.
    /// From 3 up the original writes rows `[cy-R, cy+R)` (end exclusive, CONFIRMED from its loop bounds) with the
    /// span table of `0x6f408fd0`. The row extents of that table are NOT decoded yet: this model uses integer
    /// `x*x + dy*dy <= R*R` (UNSURE, see docs/spec/vision.md section 4).
    pub fn circle_shape(&self, x: F32, y: F32, radius: F32) -> BTreeSet<(i32, i32)> {
        let r = conv::f2i(arith::mul(radius, self.scale));
        let mut out = BTreeSet::new();
        if r <= 0 {
            return out;
        }
        let (cx, cy) = self.cell_of_point(x, y);
        if r <= 2 {
            for dy in -1..1 {
                for dx in -1..1 {
                    out.insert((cx + dx, cy + dy));
                }
            }
            if r == 2 {
                out.insert((cx - 2, cy));
                out.insert((cx + 2, cy));
                out.insert((cx, cy - 2));
                out.insert((cx, cy + 2));
            }
        } else {
            for dy in -r..r {
                let row = cy + dy;
                let mut dx = 0;
                while (dx + 1) * (dx + 1) + dy * dy <= r * r {
                    dx += 1;
                }
                for c in (cx - dx)..=(cx + dx) {
                    out.insert((c, row));
                }
            }
        }
        out.into_iter()
            .filter(|&(c, r)| (0..self.width).contains(&c) && (0..self.height).contains(&r))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    #[test]
    fn table_rows_follow_the_flags() {
        // neither flag: always visible
        assert_eq!(table_state(false, false, 0, 0xFFFF, 1), FOG_VISIBLE);
        // mask only: explored -> visible, unexplored -> masked
        assert_eq!(table_state(true, false, 0, 0, 1), FOG_VISIBLE);
        assert_eq!(table_state(true, false, 0, 1, 1), FOG_MASKED);
        // fog only: visible -> visible, not visible -> fogged (whatever the mask says)
        assert_eq!(table_state(false, true, 1, 0xFFFF, 1), FOG_VISIBLE);
        assert_eq!(table_state(false, true, 0, 0xFFFF, 1), FOG_FOGGED);
        // both: visible, fogged (explored, not visible), masked (unexplored, not visible)
        assert_eq!(table_state(true, true, 1, 0xFFFF, 1), FOG_VISIBLE);
        assert_eq!(table_state(true, true, 0, 0, 1), FOG_FOGGED);
        assert_eq!(table_state(true, true, 0, 1, 1), FOG_MASKED);
    }

    #[test]
    fn table_only_reads_the_player_bit() {
        // the bit of player 3 is the only one tested: a visible bit of player 2 does not show player 3
        assert_eq!(table_state(true, true, 0b100, 0, 1 << 3), FOG_FOGGED);
        assert_eq!(table_state(true, true, 0b1000, 0, 1 << 3), FOG_VISIBLE);
    }

    #[test]
    fn modes_write_the_two_layers() {
        let mut g = VisionGrid::default();
        let c = (5, 5);
        g.write_cell(FOG_VISIBLE, 0b01, c);
        assert_eq!(g.words(c), CellWords { vis: 0b01, msk: 0xFFFE });
        assert_eq!(g.state(c, 0), FOG_VISIBLE);
        g.write_cell(FOG_FOGGED, 0b01, c);
        assert_eq!(g.state(c, 0), FOG_FOGGED);
        g.write_cell(FOG_MASKED, 0b01, c);
        assert_eq!(g.state(c, 0), FOG_MASKED);
        // other players are untouched
        assert_eq!(g.words(c).msk & 0b10, 0b10);
    }

    #[test]
    fn span_cells_match_the_pairwise_writer() {
        assert_eq!(span_cells(2, 4), vec![2, 3, 4]);
        assert_eq!(span_cells(3, 8), vec![3, 4, 5, 6, 7, 8]);
        assert_eq!(span_cells(4, 4), vec![4]);
        assert_eq!(span_cells(3, 3), vec![3]);
        // start above end: the odd start and the tail still store (Unicorn: 0 mismatches)
        assert_eq!(span_cells(11, 6), vec![11, 12]);
        assert_eq!(span_cells(10, 6), vec![10]);
    }

    #[test]
    fn rect_end_is_exclusive_and_clamped() {
        let mut g = VisionGrid { width: 4, height: 4, ..VisionGrid::default() };
        g.write_rect(FOG_VISIBLE, 1, -2, 1, 10, 10);
        // columns 0..3 (end clamped to width-1 = 3, exclusive), rows 1..3
        assert_eq!(g.state((0, 1), 0), FOG_VISIBLE);
        assert_eq!(g.state((2, 2), 0), FOG_VISIBLE);
        assert_eq!(g.words((2, 3)).vis & 1, 0, "row 3 is the exclusive end");
        assert_eq!(g.words((3, 1)).vis & 1, 0, "the last column is not written");
        assert_eq!(g.words((0, 0)).vis & 1, 0, "row 0 is outside the rect");
    }

    #[test]
    fn point_to_cell_clamps_to_the_grid() {
        let g = VisionGrid::default();
        assert_eq!(g.cell_of_point(f(0.0), f(0.0)), (0, 0));
        assert_eq!(g.cell_of_point(f(127.9), f(128.0)), (0, 1));
        assert_eq!(g.cell_of_point(f(-500.0), f(1.0e9)), (0, 255));
    }

    #[test]
    fn circle_radius_in_cells_and_small_shapes() {
        let g = VisionGrid::default();
        // radius below one cell writes nothing
        assert!(g.circle_shape(f(1000.0), f(1000.0), f(100.0)).is_empty());
        // radius 1 cell: the 2x2 block at the centre cell (cell of 1000 is 7, so cells 6..7)
        let r1 = g.circle_shape(f(1000.0), f(1000.0), f(128.0));
        assert_eq!(r1.iter().copied().collect::<Vec<_>>(), vec![(6, 6), (6, 7), (7, 6), (7, 7)]);
        // radius 2 cells adds the four arms
        let r2 = g.circle_shape(f(1000.0), f(1000.0), f(256.0));
        assert_eq!(r2.len(), 8);
        assert!(r2.contains(&(5, 7)) && r2.contains(&(7, 9)));
    }

    #[test]
    fn large_circle_rows_are_end_exclusive() {
        let g = VisionGrid::default();
        let cells = g.circle_shape(f(128.0 * 50.0), f(128.0 * 50.0), f(128.0 * 4.0));
        let rows: BTreeSet<i32> = cells.iter().map(|c| c.1).collect();
        assert_eq!(rows.iter().next().copied(), Some(46));
        assert_eq!(rows.iter().next_back().copied(), Some(53), "row cy+R is not written");
    }
}
