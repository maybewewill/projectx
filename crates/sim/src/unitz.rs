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

use softfloat::{arith, F32};

use crate::natives::game::{Game, ObjKey};

/// The movement word bits that make a unit follow the water surface (`test [unit+0x1FC], 0x38`).
pub const MOVE_WATER_BITS: u32 = 0x38;
/// The movement word of the swimming type (`cmp [unit+0x1FC], 0x20`).
pub const MOVE_SWIM: u32 = 0x20;
/// The movement word of flying units (`cmp [unit+0x1FC], 2` in the fly-height slot).
pub const MOVE_FLY: u32 = 0x2;
/// The movement word of hovering units (`cmp [unit+0x1FC], 8` in the fly-height slot).
pub const MOVE_HOVER: u32 = 0x8;

/// The height of an ordinary unit from its parts (see the module notes). `swim_flag` is `unit+0x280 & 0x20`.
pub fn unit_height(ground: F32, water: Option<F32>, fly: F32, movement: u32, swim_flag: bool) -> F32 {
    let mut g = ground;
    let follows_water = movement & MOVE_WATER_BITS != 0 && (movement != MOVE_SWIM || swim_flag);
    if let (true, Some(w)) = (follows_water, water) {
        // `fcomp`: the water replaces the ground when ground < water (ordered)
        if g.to_f32() < w.to_f32() {
            g = w;
        }
    }
    F32::from_bits(((f64::from(fly.to_f32()) + f64::from(g.to_f32())) as f32).to_bits())
}

/// `1e-4` (`0x6FAAE478`): a time difference below it is zero.
const SNAP: f32 = 1.0e-4;
const FLOAT_MAX: F32 = F32::from_bits(0x7F7F_FFFF);

impl Game {
    /// The value of the unit's fly-height property now (`unit+0x214`, an agile property: `clamp(base + slope * w)` with
    /// `w` the time since the base was set, `docs/spec/agile.md` section 2). The slope lasts `fly_for` seconds (the
    /// modifier's horizon), then the value stays.
    pub fn fly_property(&self, unit: ObjKey) -> F32 {
        let u = self.unit(unit);
        let mut w = arith::sub(self.game_time(), u.fly_t0);
        if w.to_f32().abs() < SNAP {
            w = F32::ZERO;
        }
        if u.fly_for.to_f32() < w.to_f32() {
            w = u.fly_for;
        }
        let v = arith::add(u.fly_from, arith::mul(u.fly_slope, w));
        if u.fly_lo.to_f32() > v.to_f32() {
            u.fly_lo
        } else if u.fly_hi.to_f32() < v.to_f32() {
            u.fly_hi
        } else {
            v
        }
    }

    /// `SetUnitFlyHeight` (`0x6F3C5C60` -> `0x6F2835E0` -> `0x6F2783F0`, read from the decompile, not run): the target
    /// field (`unit+0x208`, what `GetUnitFlyHeight` returns) takes `target` at once. The property then moves from its
    /// value now to the target: its limits become `[min, max]` of the two, a zero `rate` sets the value at once, any
    /// other rate becomes the slope (negated when descending) for `|target - now| / rate + 1.0` seconds. LIKELY: the
    /// limit order and the horizon; the type-dependent presentation offsets of the routine are not modelled.
    pub fn set_fly_height(&mut self, unit: ObjKey, target: F32, rate: F32) {
        let now = self.game_time();
        let cur = self.fly_property(unit);
        let horizon = |gm: &Game, diff: F32, rate: F32| -> F32 {
            match gm.order_tables() {
                Some(t) => arith::add(arith::div(diff, rate, t), F32::ONE),
                None => FLOAT_MAX,
            }
        };
        let (slope, last) = if target.to_f32() == cur.to_f32() || rate.to_f32() == 0.0 {
            (F32::ZERO, FLOAT_MAX)
        } else {
            let descending = cur.to_f32() >= target.to_f32();
            let signed = if descending {
                F32::from_bits(rate.to_bits() ^ 0x8000_0000)
            } else {
                rate
            };
            let diff = arith::sub(target, cur);
            let h = horizon(self, diff, signed);
            let h = if h.to_f32() < SNAP || h.to_f32().is_nan() { F32::from_bits(SNAP.to_bits()) } else { h };
            (signed, h)
        };
        let (lo, hi) = if cur.to_f32() >= target.to_f32() { (target, cur) } else { (cur, target) };
        let u = self.unit_mut(unit);
        u.fly_height = target;
        u.fly_t0 = now;
        u.fly_slope = slope;
        u.fly_for = last;
        if target.to_f32() == cur.to_f32() {
            // already there: the old modifier is cancelled and the value kept, the limits stay (`0x6F2783F0` returns early)
            u.fly_from = cur;
        } else if rate.to_f32() == 0.0 {
            u.fly_from = target;
            u.fly_lo = lo;
            u.fly_hi = hi;
        } else {
            u.fly_from = cur;
            u.fly_lo = lo;
            u.fly_hi = hi;
        }
    }

    /// The fly height a unit is at now (slot `0xE8`, `0x6F29E850`, read from the decompile): the property for a flying
    /// movement type, the type's `moveHeight` for a hovering one (LIKELY), zero otherwise. (A unit with the flag
    /// `+0x20 & 0x800` also reads the property; no such flag is modelled.)
    pub fn current_fly_height(&self, unit: ObjKey) -> F32 {
        let u = self.unit(unit);
        let ty = self.unit_types().get(u.type_id);
        match ty.map_or(0, |t| t.movement) {
            MOVE_FLY => self.fly_property(unit),
            MOVE_HOVER => ty.map_or(F32::ZERO, |t| t.fly_height),
            _ => F32::ZERO,
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
