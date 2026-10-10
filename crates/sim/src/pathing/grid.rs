//! The pathing grid: 32-unit cells, one 32-bit word per cell (pathing.md 1.2 to 1.5).
//!
//! Cell word:
//! * bits 24..31: pathing-type bits; a set bit means blocked for that type.
//! * bits 0..23: head index of the cell's occupant list, `NO_OCCUPANT` (0xFFFFFF) when empty.
//!
//! Cells are stored row by row: index = `y * width + x`.
//!
//! World to cell (pathing.md 1.3, CONFIRMED arithmetic): subtract the map origin with the
//! soft-float subtract, multiply by 1/32 by editing the exponent of the float bits, take
//! the soft-float floor, then truncate to an integer with the soft-float f2i. The 1/32
//! step flushes a value to +0 when its magnitude is below 2^-121 (bits below 0x03000000).
//! Which origin belongs to x and which to y is LIKELY (x with the field at +0x6C); here the
//! caller passes both explicitly.

use softfloat::{arith, conv, math, F32};

/// World units per cell is `1 << CELL_SHIFT` (32).
pub const CELL_SHIFT: u32 = 5;
/// Cell word value with no occupant (low 24 bits all set).
pub const NO_OCCUPANT: u32 = 0x00FF_FFFF;

/// Pathing-type bit for "any" (every bit).
pub const PATH_ANY: u8 = 0xFF;
/// Pathing-type bit for unwalkable.
pub const PATH_UNWALKABLE: u8 = 0x02;
/// Pathing-type bit for unflyable.
pub const PATH_UNFLYABLE: u8 = 0x04;
/// Pathing-type bit for unbuildable.
pub const PATH_UNBUILDABLE: u8 = 0x08;
/// Pathing-type bit for blighted.
pub const PATH_BLIGHTED: u8 = 0x20;
/// Pathing-type bit for unfloat.
pub const PATH_UNFLOAT: u8 = 0x40;
/// Pathing-type bit for unamphibious.
pub const PATH_UNAMPH: u8 = 0x80;

/// Bit mask of a native pathing-type index (table of `FUN_6F3B4280`, pathing.md 1.5).
/// Unknown indices give 0.
pub fn path_type_mask(index: u32) -> u8 {
    match index {
        0 => PATH_ANY,
        1 => 0x02,
        2 => 0x04,
        3 => 0x08,
        4 => 0x10,
        5 => 0x20,
        6 => 0x40,
        7 => 0x80,
        _ => 0,
    }
}

/// Top byte of a cell word (the pathing-type bits).
pub fn cell_type(word: u32) -> u8 {
    (word >> 24) as u8
}

/// Low 24 bits of a cell word (the occupant list head).
pub fn cell_occupant(word: u32) -> u32 {
    word & NO_OCCUPANT
}

/// Scales a float by 1/32 the way the original does it: the exponent field is reduced by 5
/// through integer addition on the bit pattern. A value whose magnitude is below 2^-121
/// (bits below 0x03000000, zero included) comes out as +0.
///
/// The test is the sign bit of `(bits - 0x03000000) ^ bits`: it is set exactly when the
/// subtraction crosses the sign boundary, which is the flush condition.
pub fn scale_by_1_32(v: F32) -> F32 {
    let bits = v.to_bits();
    if (bits.wrapping_add(0xFD00_0000) ^ bits) & 0x8000_0000 != 0 {
        F32::ZERO
    } else {
        F32::from_bits(bits.wrapping_add(0xFD80_0000))
    }
}

/// One coordinate of the world to cell conversion: `f2i(floor(scale(v - origin)))`.
pub fn cell_coordinate(v: F32, origin: F32) -> i32 {
    let rel = arith::sub(v, origin);
    conv::f2i(math::floor(scale_by_1_32(rel)))
}

/// A rectangular grid of pathing cells with its world origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    width: u32,
    height: u32,
    origin_x: F32,
    origin_y: F32,
    cells: Vec<u32>,
}

impl Grid {
    /// A `width` by `height` grid with no blocked cells and no occupants.
    ///
    /// Panics if the cell count does not fit in `usize` (cannot happen for real maps).
    pub fn new(width: u32, height: u32, origin_x: F32, origin_y: F32) -> Self {
        let count = (width as usize)
            .checked_mul(height as usize)
            .expect("grid cell count overflows usize");
        Grid {
            width,
            height,
            origin_x,
            origin_y,
            cells: vec![NO_OCCUPANT; count],
        }
    }

    /// Width in cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row-major index of a cell, or `None` when it is outside the grid.
    pub fn index(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return None;
        }
        Some(y as usize * self.width as usize + x as usize)
    }

    /// The cell word at `(x, y)`, or `None` outside the grid.
    pub fn word(&self, x: i32, y: i32) -> Option<u32> {
        self.index(x, y).map(|i| self.cells[i])
    }

    /// Sets the pathing-type bits in `mask` (OR into the top byte).
    pub fn set_type_bits(&mut self, x: i32, y: i32, mask: u8) {
        if let Some(i) = self.index(x, y) {
            self.cells[i] |= u32::from(mask) << 24;
        }
    }

    /// Clears the pathing-type bits in `mask` (AND-NOT on the top byte).
    pub fn clear_type_bits(&mut self, x: i32, y: i32, mask: u8) {
        if let Some(i) = self.index(x, y) {
            self.cells[i] &= !(u32::from(mask) << 24);
        }
    }

    /// Replaces the occupant list head (only the low 24 bits are kept).
    pub fn set_occupant(&mut self, x: i32, y: i32, head: u32) {
        if let Some(i) = self.index(x, y) {
            self.cells[i] = (self.cells[i] & !NO_OCCUPANT) | (head & NO_OCCUPANT);
        }
    }

    /// True when the cell is blocked for any type bit in `mask`. Outside the grid: `None`.
    pub fn is_blocked(&self, x: i32, y: i32, mask: u8) -> Option<bool> {
        self.word(x, y).map(|w| cell_type(w) & mask != 0)
    }

    /// Cell coordinates of a world point. The result may be outside the grid.
    pub fn to_cell(&self, x: F32, y: F32) -> (i32, i32) {
        (
            cell_coordinate(x, self.origin_x),
            cell_coordinate(y, self.origin_y),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    #[test]
    fn cell_word_fields() {
        let w = (0x42u32 << 24) | 0x0012_3456;
        assert_eq!(cell_type(w), 0x42);
        assert_eq!(cell_occupant(w), 0x0012_3456);
        assert_eq!(cell_occupant(NO_OCCUPANT), NO_OCCUPANT);
    }

    #[test]
    fn type_bits_and_occupant_edit_the_right_fields() {
        let mut g = Grid::new(3, 2, F32::ZERO, F32::ZERO);
        assert_eq!(g.word(0, 0), Some(NO_OCCUPANT));
        g.set_type_bits(1, 1, PATH_UNWALKABLE | PATH_BLIGHTED);
        assert_eq!(cell_type(g.word(1, 1).unwrap()), 0x22);
        assert_eq!(g.is_blocked(1, 1, PATH_UNWALKABLE), Some(true));
        assert_eq!(g.is_blocked(1, 1, PATH_UNFLYABLE), Some(false));
        g.set_occupant(1, 1, 0xFFF_FFFF);
        assert_eq!(cell_occupant(g.word(1, 1).unwrap()), NO_OCCUPANT);
        g.clear_type_bits(1, 1, PATH_UNWALKABLE);
        assert_eq!(cell_type(g.word(1, 1).unwrap()), 0x20);
        assert_eq!(g.is_blocked(3, 0, PATH_ANY), None);
    }

    #[test]
    fn path_type_masks_follow_the_table() {
        assert_eq!(path_type_mask(0), 0xFF);
        assert_eq!(path_type_mask(1), 0x02);
        assert_eq!(path_type_mask(7), 0x80);
        assert_eq!(path_type_mask(8), 0);
    }

    #[test]
    fn scale_is_exact_divide_by_32_for_normal_values() {
        for v in [32.0f32, 1.0, -1.0, 0.5, 96.0, -33.0, 1.0e6] {
            let s = scale_by_1_32(f(v)).to_f32();
            assert_eq!(s, v / 32.0, "v = {v}");
        }
    }

    #[test]
    fn scale_flushes_tiny_values_to_zero() {
        assert_eq!(scale_by_1_32(F32::ZERO), F32::ZERO);
        assert_eq!(scale_by_1_32(F32::from_bits(0x8000_0000)), F32::ZERO);
        assert_eq!(scale_by_1_32(F32::from_bits(0x0200_0000)), F32::ZERO);
        assert_eq!(scale_by_1_32(F32::from_bits(0x8200_0000)), F32::ZERO);
    }

    #[test]
    fn world_to_cell_uses_floor_and_truncation() {
        let g = Grid::new(4, 4, F32::ZERO, F32::ZERO);
        assert_eq!(g.to_cell(f(0.0), f(0.0)), (0, 0));
        assert_eq!(g.to_cell(f(31.999), f(32.0)), (0, 1));
        assert_eq!(g.to_cell(f(-0.0), f(64.0)), (0, 2));
        assert_eq!(g.to_cell(f(-1.0), f(-32.0)), (-1, -1));
        assert_eq!(g.to_cell(f(-0.5), f(127.0)), (-1, 3));
    }

    #[test]
    fn world_to_cell_subtracts_the_origin_first() {
        let g = Grid::new(4, 4, f(100.0), f(-50.0));
        assert_eq!(g.to_cell(f(132.0), f(-18.0)), (1, 1));
        assert_eq!(g.to_cell(f(99.0), f(-50.0)), (-1, 0));
    }
}
