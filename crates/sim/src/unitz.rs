//! A unit's height `z` (`docs/spec/terrain.md` section 6): the unit vtable slot `0xE4` (`0x6F281F50`).
//!
//! For an ordinary unit the height is `f32(fly + ground')` where `fly` is the unit's fly-height property (slot `0xE8`,
//! `0x6F29E850`) and `ground'` the terrain height at the unit's point, raised to the water surface for a unit whose
//! movement word has a bit of `0x38` (float, hover, amphibious) when the surface lies above the ground and the unit is
//! not (movement exactly `0x20` without the swim flag). CONFIRMED by Unicorn
//! (`tools/emu/unit_z_emu.py`, 1,500 vectors, `tests/unit_z.rs`). The sum is the FPU's (53-bit) add stored as a float,
//! not the soft float. Not modelled: a corpse's sinking interpolation (flag `+0x5C & 0x20000000` with `+0x200 <= 0`)
//! and the special branch of flag `+0x280 & 8` (a unit carried by something); neither is reachable for a shooter or a
//! live target.

use softfloat::F32;

use crate::natives::game::{Game, ObjKey};

/// The movement word bits that make a unit follow the water surface (`test [unit+0x1FC], 0x38`).
pub const MOVE_WATER_BITS: u32 = 0x38;
/// The movement word of the swimming type (`cmp [unit+0x1FC], 0x20`).
pub const MOVE_SWIM: u32 = 0x20;
/// The movement word of flying units (`cmp [unit+0x1FC], 2` in the fly-height slot).
pub const MOVE_FLY: u32 = 0x2;

/// The height of an ordinary unit from its parts (see the module notes). `swim_flag` is `unit+0x280 & 0x20`.
pub fn unit_height(ground: F32, water: Option<F32>, fly: F32, movement: u32, swim_flag: bool) -> F32 {
    let mut g = ground;
    if movement & MOVE_WATER_BITS != 0 && !(movement == MOVE_SWIM && !swim_flag) {
        if let Some(w) = water {
            // `fcomp`: the water replaces the ground when ground < water (ordered)
            if g.to_f32() < w.to_f32() {
                g = w;
            }
        }
    }
    F32::from_bits(((f64::from(fly.to_f32()) + f64::from(g.to_f32())) as f32).to_bits())
}

impl Game {
    /// The fly height a unit is at now (the value of its fly-height property, slot `0xE8`, `0x6F29E850`): the
    /// unit's fly height for a flying movement type, zero otherwise. UNSURE: the property climbs at the rate of
    /// `SetUnitFlyHeight` (`docs/spec/terrain.md` section 6.3); this model has no ramp and uses the stored target.
    pub fn current_fly_height(&self, unit: ObjKey) -> F32 {
        let u = self.unit(unit);
        let movement = self.unit_types().get(u.type_id).map_or(0, |t| t.movement);
        if movement == MOVE_FLY {
            u.fly_height
        } else {
            F32::ZERO
        }
    }

    /// The unit's height now: the terrain (and water) under it plus its fly height. Zero ground without a terrain grid.
    pub fn unit_z(&self, unit: ObjKey) -> F32 {
        let u = self.unit(unit);
        let movement = self.unit_types().get(u.type_id).map_or(0, |t| t.movement);
        let (x, y) = (u.x, u.y);
        let (ground, water) = match self.terrain() {
            Some(t) => (
                F32::from_bits(t.ground_height(x.to_f32(), y.to_f32()).to_bits()),
                t.water_at(x.to_f32(), y.to_f32()).map(|w| F32::from_bits(w.to_bits())),
            ),
            None => (F32::ZERO, None),
        };
        unit_height(ground, water, self.current_fly_height(unit), movement, false)
    }
}
