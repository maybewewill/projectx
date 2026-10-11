//! Unit position storage in cell coordinates (`docs/spec/unit-position.md`).
//!
//! The original keeps a unit's position as cell coordinates (unit+0x78 for x, unit+0x7C for y), not as world
//! coordinates. Writes go through the cell setter `0x6F474250` (CONFIRMED): the world origin is subtracted (x with
//! the world float at `W+0x6C`, y with `W+0x70`, CONFIRMED by the normaliser), the difference is multiplied by 1/32
//! by subtracting 5 from its exponent, and a result whose exponent is below 6 becomes `+0.0` (CONFIRMED bit rule).
//! Reads (`GetUnitX/Y`, the normaliser `0x6F4743A0`, CONFIRMED) multiply the cell coordinate by 32 (add 5 to a
//! non-zero exponent) and add the origin.
//!
//! The normaliser also has a cell term `v` (unit+0x70/+0x74 against the grid `G+0x40/+0x44`, scaled by the step
//! vector unit+0x80/+0x84). Every position write refreshes the term from the grid it uses (`0x6F497440`,
//! CONFIRMED callers), so at read time `v` is `+0.0` for a unit on grid A (unit+0x14 bit 31 clear), whatever the
//! grid values are (the grid values and the step vector drop out). The model therefore has no cell term: it holds
//! the cell coordinates and the cached world coordinates `Unit.x/y`, which are always the normaliser's output for
//! the current cells. Grid B (bit 31 set) and a stale snapshot are not modelled (UNSURE, see the spec).
//!
//! Model-level rules (LIKELY, each checked in the spec): SetUnitX/SetUnitY send both coordinates to the setter, the
//! other one as read back by the normaliser (`0x6F4743C0` pair, CONFIRMED in the harness), so that coordinate is
//! re-rounded too. Creation stores the placement and then runs the init's read-and-write-back of the same position
//! (`0x6F2A0E30` reads through `vf46` and writes with `0x6F474250`, CONFIRMED).
//!
//! Arithmetic: softfloat (`softfloat::arith`), the same truncating operations as the original.

use softfloat::{arith, F32};

use crate::natives::game::{Game, ObjKey, Object};

/// The cell coordinate of a world coordinate `v` for the origin `origin` (the setter's arithmetic, CONFIRMED).
pub fn world_to_cell(v: F32, origin: F32) -> F32 {
    let d = arith::sub(v, origin);
    // The original adds 0xFD000000 and tests the sign bit. That is "magnitude < 6 << 23", i.e. exponent < 6: the
    // result becomes +0.0 (the mask yields zero). Otherwise the exponent drops by 5 (an exact multiply by 1/32).
    if d.0 & 0x7FFF_FFFF < 0x0300_0000 {
        F32::ZERO
    } else {
        F32(d.0.wrapping_add(0xFD80_0000))
    }
}

/// The stored cell coordinate after a write of world coordinate `v` over the stored cell `old` (CONFIRMED by the
/// Unicorn vectors, `docs/spec/unit-position.md` 3): the setter `0x6F4737D0` forms `delta = P - F` with `P` the new
/// cell (`world_to_cell`) and `F = old + step*v` (`0x6F38B0B0`), and the core writer `0x6F4A6FD0` stores
/// `(old + step*v) + delta`. All steps are softfloat, so the old magnitude sets the rounding.
pub fn stored_cell(old: F32, v: F32, origin: F32) -> F32 {
    // F = old + step * cell term; the step term is +0 at rest (the zero vector is written as +0.0), so F = old + 0.
    // The add keeps the original's zero-sign behaviour (a zero first operand returns the second one).
    let f = arith::add(old, F32::ZERO);
    let p = world_to_cell(v, origin);
    arith::add(f, arith::sub(p, f))
}

/// The world coordinate of a cell coordinate `c` for the origin `origin` (the normaliser with a zero cell term,
/// CONFIRMED): `32 * c + origin`, where the multiply adds 5 to a non-zero exponent.
pub fn cell_to_world(c: F32, origin: F32) -> F32 {
    let scaled = if c.0 & 0x7F80_0000 != 0 {
        F32(c.0.wrapping_add(0x0280_0000))
    } else {
        c
    };
    arith::add(scaled, origin)
}

impl Game {
    /// The world origin of the cell storage: `(min x, min y)` of the playable area, or zero before a host sets one
    /// (the original's `W+0x6C` / `W+0x70`; the value is the playable minimum, see `docs/spec/unit-position.md` 2).
    pub fn world_origin(&self) -> (F32, F32) {
        self.playable()
            .map_or((F32::ZERO, F32::ZERO), |p| (p.min_x, p.min_y))
    }

    /// Stores a world position of unit `k` through the cell setter: the cells are kept and the cached world
    /// coordinates become the normaliser's output for them. Does nothing for a non-unit key.
    pub fn store_unit_position(&mut self, k: ObjKey, x: F32, y: F32) {
        let (ox, oy) = self.world_origin();
        let u = self.unit_mut(k);
        let (cx, cy) = (stored_cell(u.cell_x, x, ox), stored_cell(u.cell_y, y, oy));
        u.cell_x = cx;
        u.cell_y = cy;
        u.x = cell_to_world(cx, ox);
        u.y = cell_to_world(cy, oy);
    }

    /// Recomputes every unit's cached world position from its cells (after the origin changes).
    pub fn refresh_unit_positions(&mut self) {
        let (ox, oy) = self.world_origin();
        for i in 0..self.object_count() {
            let k = ObjKey(i as u32);
            if let Object::Unit(u) = self.object_mut(k) {
                u.x = cell_to_world(u.cell_x, ox);
                u.y = cell_to_world(u.cell_y, oy);
            }
        }
    }

    /// The creation placement of unit `k` (LIKELY the setter, see the module notes): the placement is stored, then
    /// the init reads the position back through the normaliser and writes it again (CONFIRMED structure of
    /// `0x6F2A0E30`).
    pub fn place_new_unit(&mut self, k: ObjKey, x: F32, y: F32) {
        self.store_unit_position(k, x, y);
        let (x, y) = {
            let u = self.unit(k);
            (u.x, u.y)
        };
        self.store_unit_position(k, x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32(v.to_bits())
    }

    #[test]
    fn a_zero_origin_round_trip_is_exact_for_normal_values() {
        // With origin 0 the subtraction and the addition are exact, and the 1/32 and 32 scalings are exact.
        for v in [1.0f32, -3.5, 4096.0, 0.25, 1234.5678, -0.001, 1.0e-20] {
            let c = world_to_cell(f(v), F32::ZERO);
            assert_eq!(cell_to_world(c, F32::ZERO), f(v), "value {v}");
        }
    }

    #[test]
    fn small_cells_are_flushed_to_positive_zero() {
        // An exponent below 6 after the subtraction gives +0.0 (the magnitude test of the setter).
        assert_eq!(world_to_cell(f(1.0e-38), F32::ZERO), F32::ZERO);
        assert_eq!(world_to_cell(f(-1.0e-38), F32::ZERO), F32::ZERO);
        assert_eq!(world_to_cell(f(0.0), F32::ZERO), F32::ZERO);
        assert_eq!(world_to_cell(F32(0x8000_0000), F32::ZERO), F32::ZERO);
        // Exponent 6 is the smallest kept value; it is scaled down by 5 to exponent 1.
        assert_eq!(world_to_cell(F32(0x0300_0000), F32::ZERO), F32(0x0080_0000));
        assert_eq!(world_to_cell(F32(0x82FF_FFFF), F32::ZERO), F32::ZERO);
    }

    #[test]
    fn a_far_origin_loses_low_bits_as_the_original_does() {
        // (0.1 - 4096) is rounded by the truncating subtraction, so the read-back is not 0.1 (the ulp of 4096 is
        // 2^-12), while the 1/32 and 32 scalings are exact.
        let origin = f(4096.0);
        let x = f(0.1);
        let back = cell_to_world(world_to_cell(x, origin), origin);
        assert_ne!(back, x);
        assert!((back.0 as i64 - x.0 as i64).abs() > 0);
        let err = (f32::from_bits(back.0) - 0.1).abs();
        assert!(err <= 2.0f32.powi(-12), "error {err}");
    }
}
