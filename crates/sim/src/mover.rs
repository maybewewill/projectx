//! Unit mover kinematics of Game.dll 1.26a (`docs/spec/movement-6.md`).
//!
//! The original does not advance a unit by a fixed step per movement walk. A mover `U` (the unit's movement
//! sub-object of `movement-5.md`) keeps
//!
//! * the position in cells at the last re-base, `U+0x78/+0x7C` (here [`Kin::cx`], [`Kin::cy`]),
//! * a velocity `V = U+0x80/+0x84` in **cells per second**,
//! * a time snapshot `U+0x70` (float) / `U+0x74` (int) of the clock layer it belongs to,
//!
//! and the position at time `t` is **evaluated**, never stepped: `cell + V * v` with `v` the elapsed time since the
//! snapshot (`0x6F483E70`, [`crate::heroes::cell_term`]; CONFIRMED, `unit-position.md`). The 0.03 s movement walk
//! (message `0x8026A`) only steers: it re-bases the position (`0x6F4A6FD0`) and replaces `V` by the new velocity
//! (`0x6F493A40` -> `0x6F4A73B0` -> `0x6F4A70D0`). This module holds the kernels the walk is built from, each
//! checked against the real code in Unicorn (`tools/emu/movement6_emu.py`; the vectors are outputs of the user's
//! own `Game.dll` and are not committed, only a small table-free sample is embedded in the tests).
//!
//! All arithmetic is the original's softfloat. Comparisons that the original makes with the x87 (`fcompp`) are
//! IEEE compares of the `f32` values.

use std::cmp::Ordering;

use softfloat::{arith, math, F32, Tables};

use crate::heroes::{cell_term, GridCell, NormUnit};

/// `FLT_MAX` (`0x6FAAE624`): the "no cap" value.
pub const FLT_MAX: F32 = F32(0x7F7F_FFFF);
/// `2.0e-7` (`0x6FAAE614`): below this the squared velocity is treated as zero.
const EPS_SQ: F32 = F32(0x3456_BF95);
/// `pi` (`0x6FAAE600`) and `2*pi` (`0x6FAAE608`).
const PI: F32 = F32(0x4049_0FDB);
const TWO_PI: F32 = F32(0x40C9_0FDB);
/// `1.0e-5` as a bit pattern (`0x3727C5AC`): the length / angle floor of the heading routine.
const HEADING_FLOOR_BITS: u32 = 0x3727_C5AC;

/// `a <= b` with both operands ordered (the x87 compare reports unordered separately).
fn le_ordered(a: F32, b: F32) -> bool {
    matches!(a.to_f32().partial_cmp(&b.to_f32()), Some(Ordering::Less | Ordering::Equal))
}
fn lt(a: F32, b: F32) -> bool {
    a.to_f32() < b.to_f32()
}
fn gt(a: F32, b: F32) -> bool {
    a.to_f32() > b.to_f32()
}
fn abs(a: F32) -> F32 {
    F32(a.0 & 0x7FFF_FFFF)
}

/// The position state of a mover (`U`). All values are bit patterns of the original's fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kin {
    /// `U+0x70`: the clock layer's time at the last re-base.
    pub snap_t: F32,
    /// `U+0x74`: the clock layer's row at the last re-base.
    pub snap_row: i32,
    /// `U+0x78`, `U+0x7C`: the cell position at the last re-base.
    pub cx: F32,
    pub cy: F32,
    /// `U+0x80`, `U+0x84`: the velocity, cells per second.
    pub vx: F32,
    pub vy: F32,
    /// `U+0x88`: the speed cap, cells per second ([`FLT_MAX`] = none).
    pub cap: F32,
    /// Bit `0x20000000` of the flag word at `U.98+0x40`: set while the velocity is non-zero and the cap positive.
    pub moving: bool,
    /// `U+0x8C`: the facing (radians).
    pub facing: F32,
    /// `U+0xC0`: the speed the walk last set (cells per second).
    pub last_speed: F32,
    /// `U+0xC4`: the wrapped difference `facing - heading` the facing update works toward.
    pub face_error: F32,
}

impl Kin {
    fn norm_unit(&self) -> NormUnit {
        NormUnit {
            grid_word: 0,
            i: self.snap_t,
            j: self.snap_row,
            x: self.cx,
            y: self.cy,
            xv: self.vx,
            yv: self.vy,
        }
    }

    /// The elapsed time `v` since the snapshot on layer `g` (`0x6F483E70`).
    pub fn elapsed(&self, g: &GridCell) -> F32 {
        cell_term(g, &self.norm_unit())
    }

    /// The position in cells at the layer time: `(cx + vx * v, cy + vy * v)` (`0x6F38B0B0`, `0x6F497460`).
    pub fn position(&self, g: &GridCell) -> (F32, F32) {
        let v = self.elapsed(g);
        (
            arith::add(self.cx, arith::mul(self.vx, v)),
            arith::add(self.cy, arith::mul(self.vy, v)),
        )
    }

    /// The re-base `0x6F4A6FD0`: the cell position becomes the evaluated position plus `delta`, and the snapshot
    /// becomes the layer's `(time, row)` (`0x6F497440`).
    pub fn rebase(&mut self, g: &GridCell, delta: (F32, F32)) {
        let (fx, fy) = self.position(g);
        self.cx = arith::add(fx, delta.0);
        self.cy = arith::add(fy, delta.1);
        self.snap_t = g.origin;
        self.snap_row = g.row;
    }

    /// `0x6F4A73B0`: re-base with a zero delta, then add `delta` to the velocity (`0x6F4A70D0`): a tiny result is
    /// zeroed, a result longer than the cap is rescaled to the cap (`0x6F07B870`).
    fn add_velocity(&mut self, g: &GridCell, delta: (F32, F32), t: &Tables) {
        self.rebase(g, (F32::ZERO, F32::ZERO));
        self.vx = arith::add(self.vx, delta.0);
        self.vy = arith::add(self.vy, delta.1);
        let s2 = arith::add(arith::mul(self.vx, self.vx), arith::mul(self.vy, self.vy));
        if lt(abs(s2), EPS_SQ) {
            self.vx = F32::ZERO;
            self.vy = F32::ZERO;
            self.moving = false;
            return;
        }
        let len = math::sqrt(s2, t);
        if lt(self.cap, len) {
            let rs = arith::recip(math::sqrt(s2, t), t);
            let k = arith::mul(self.cap, rs);
            self.vx = arith::mul(self.vx, k);
            self.vy = arith::mul(self.vy, k);
        }
        self.moving = gt(self.cap, F32::ZERO);
    }

    /// `0x6F493A40`: makes `target` the velocity (`delta = target - V`, added by [`Kin::add_velocity`]).
    pub fn set_velocity(&mut self, g: &GridCell, target: (F32, F32), t: &Tables) {
        let delta = (arith::sub(target.0, self.vx), arith::sub(target.1, self.vy));
        self.add_velocity(g, delta, t);
    }

    /// `0x6F494160` (the per-walk velocity writer called from `0x6F492EC0`): `speed` is the speed of the record,
    /// `heading` its heading. Stores the speed and the facing error (`facing - heading`, wrapped by `0x6F496630`).
    /// A positive speed makes `V = speed * (cos heading, sin heading)`; otherwise the unit stops and its facing
    /// becomes the normalised heading (`0x6F493910`).
    pub fn set_speed_heading(&mut self, g: &GridCell, speed: F32, heading: F32, t: &Tables) {
        self.last_speed = speed;
        self.face_error = wrap_error(self.facing, heading);
        if lt(F32::ZERO, speed) {
            let sin = math::sin(heading, t);
            let cos = math::cos(heading, t);
            let target = (arith::mul(speed, cos), arith::mul(speed, sin));
            self.set_velocity(g, target, t);
        } else {
            self.set_velocity(g, (F32::ZERO, F32::ZERO), t);
            self.facing = normalise_angle(heading, t);
        }
    }

    /// `0x6F4A73F0`: stores the cap; a cap below the current speed stops the unit (re-base, zero velocity).
    pub fn set_cap(&mut self, g: &GridCell, cap: F32, t: &Tables) {
        self.cap = cap;
        let s2 = arith::add(arith::mul(self.vy, self.vy), arith::mul(self.vx, self.vx));
        let len = abs(math::sqrt(s2, t));
        if lt(self.cap, len) {
            self.add_velocity(g, (F32::ZERO, F32::ZERO), t);
        }
    }
}

/// The acceleration and turn limiter `0x6F493FA0` (`U+0xB4` acceleration step, `U+0xB8` turn cap, `U+0xBC`
/// propulsion window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SteerParams {
    pub accel: F32,
    pub turn_cap: F32,
    pub window: F32,
}

/// `0x6F493FA0`: `rd` is the record's speed (`R+0x20`), `rh` its heading (`R+0x24`), `delta` the signed heading
/// error. The speed gains `accel` when the error is inside the window (and `force_zero` is not set), otherwise it
/// becomes zero; the heading turns toward the target by at most `turn_cap`.
pub fn steer(p: &SteerParams, rd: &mut F32, rh: &mut F32, delta: F32, force_zero: bool) {
    let a = abs(delta);
    // The window compare is `fcomp`: the speed grows when window > |delta| or the compare is unordered.
    let inside = !le_ordered(p.window, a);
    if !force_zero && inside {
        *rd = arith::add(*rd, p.accel);
    } else {
        *rd = F32::ZERO;
    }
    // A zero error needs no turn (a NaN error still turns).
    if a.to_f32() == 0.0 {
        return;
    }
    let m = if p.turn_cap.to_f32() > a.to_f32() { a } else { p.turn_cap };
    let signed = if delta.to_f32() >= 0.0 { m } else { arith::neg(m) };
    *rh = arith::add(*rh, signed);
}

/// The heading of the vector `(vx, vy)` of length `r` (`0x6F4B2DE0`): `acos(vx / r)`, mirrored to `2*pi - a` when
/// `vy < 0`; zero for a tiny length or a tiny angle (floor `1e-5`).
pub fn heading_of(vx: F32, vy: F32, r: F32, t: &Tables) -> F32 {
    if (r.0 & 0x7FFF_FFFF) <= HEADING_FLOOR_BITS {
        return F32::ZERO;
    }
    let u = arith::div(vx, r, t);
    let a = math::acos(u, t);
    if (a.0 & 0x7FFF_FFFF) <= HEADING_FLOOR_BITS {
        return F32::ZERO;
    }
    if vy.to_f32() < 0.0 {
        arith::sub(TWO_PI, a)
    } else {
        a
    }
}

/// The signed heading error toward the vector `(vx, vy)` from the current heading `cur` (`0x6F493D90` with
/// `0x6F493D20` and the wrap `0x6F496630`): the vector's heading minus `cur`, wrapped into `[-pi, pi]`, and zero
/// when its magnitude is below `2e-7`.
pub fn heading_error(cur: F32, vx: F32, vy: F32, t: &Tables) -> F32 {
    let len = math::sqrt(arith::add(arith::mul(vy, vy), arith::mul(vx, vx)), t);
    let h = heading_of(vx, vy, len, t);
    let wrapped = wrap_error(cur, h);
    if gt(F32(0x3456_BF95), abs(wrapped)) {
        F32::ZERO
    } else {
        wrapped
    }
}

/// `0x6F493910`: an angle folded into `[0, 2*pi)`. A magnitude of `2*pi` or more is reduced by the remainder
/// helper `0x6F6EFE50` ([`fold`]); a negative result gets `2*pi` added.
pub fn normalise_angle(a: F32, t: &Tables) -> F32 {
    let mut x = a;
    if abs(a).to_f32() >= TWO_PI.to_f32() {
        x = fold(a, TWO_PI, t);
    }
    if lt(x, F32::ZERO) {
        arith::add(x, TWO_PI)
    } else {
        x
    }
}

/// The remainder helper `0x6F6EFE50` (`fold(x, m)`): `frac(x / |m|) * |m|` through the reciprocal table, with the
/// two corrections the original applies to a result that is not inside `(-|m|, |m|)`.
pub fn fold(x: F32, m: F32, t: &Tables) -> F32 {
    let am = abs(m);
    let q = arith::mul(x, arith::recip(am, t));
    let fr = frac(q);
    let mut r = arith::mul(fr, am);
    let positive = r.0 & 0x8000_0000 == 0 && (r.0 << 1) != 0;
    if positive {
        // `fcompp`: |m| > r keeps r (also when unordered); otherwise r - |m|
        if le_ordered(am, r) {
            r = arith::sub(r, am);
        }
    } else {
        let nm = F32(am.0 ^ 0x8000_0000);
        // `test ah,1; jne`: keep r when -|m| < r
        if !lt(nm, r) {
            r = arith::add(r, nm);
        }
    }
    r
}

/// The fractional part helper `0x6F6EF2D0`: `x - trunc(x)`; a magnitude below 1 returns `x`, an exponent of 23 or
/// more returns zero.
fn frac(x: F32) -> F32 {
    let e = ((x.0 >> 23) & 0xFF) as i32 - 0x7F;
    if e < 0 {
        return x;
    }
    if e >= 0x17 {
        return F32::ZERO;
    }
    let mask = (0x8000_0000u32 as i32 >> (e as u32 + 8)) as u32;
    arith::sub(x, F32(x.0 & mask))
}

/// `0x6F496630` with `x = cur - h`: the result is `h - cur` wrapped by `2*pi` into the range `[-pi, pi]`.
fn wrap_error(cur: F32, h: F32) -> F32 {
    let x = arith::sub(cur, h);
    if !lt(x, F32::ZERO) {
        // x >= 0 (or NaN): the wrap applies only when pi < x strictly (x87 `fcompp`, `test ah,5`, `jp`)
        if lt(PI, x) {
            arith::sub(TWO_PI, x)
        } else {
            arith::neg(x)
        }
    } else {
        let nx = arith::neg(x);
        if lt(PI, nx) {
            arith::sub(arith::neg(TWO_PI), x)
        } else {
            nx
        }
    }
}

/// The steering inputs of a mover that do not change during a walk (`U+0xB0`, `U+0xB4/B8/BC`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pilot {
    /// Acceleration step, turn cap and propulsion window (`U+0xB4/B8/BC`).
    pub steer: SteerParams,
    /// The arrival radius in cells (`U+0xB0`).
    pub radius: F32,
}

/// What the path follower (`0x6F49E020`) answered for this walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathState {
    /// The path is open: steer toward this waypoint (cells). The speed may grow.
    Open((F32, F32)),
    /// The follower returned a non-zero state (waiting, blocked): the speed is zeroed, the heading still turns
    /// toward the goal.
    Blocked,
}

/// The values the walk computed for the record, for tests: `R+0x20` (speed after the cap) and `R+0x24`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkTrace {
    pub speed: F32,
    pub heading: F32,
    /// The goal was reached and the heading is aligned: the unit stops.
    pub arrived: bool,
}

/// `0.2` (`0x6FAAE4B0`): the heading tolerance of the arrival test, radians.
const ARRIVE_ANGLE: F32 = F32(0x3E4C_CCCD);
/// `4.0e-7` (`0x6FAAE618`): the speed floor of the walk head.
const SPEED_FLOOR: F32 = F32(0x34D6_BF95);

/// One movement walk of a solitary mover record (`0x6F4907F0` with its path step `0x6F495320`, then the speed
/// writer `0x6F492EC0`/`0x6F494160` of `0x6F493010`). `goal` is the record's target (`R+0x18`, cells), `path` the
/// follower's answer. The walk re-bases the position at the layer time of `g` and writes the new velocity.
///
/// Group effects (cluster speed matching, the leader, the `0x200`/`0x800` flags) are not modelled: a record of its own
/// takes `limit = U+0x88`.
pub fn walk(kin: &mut Kin, pilot: &Pilot, g: &GridCell, goal: (F32, F32), path: PathState, t: &Tables) -> WalkTrace {
    let pos = kin.position(g);
    // 0x6F4907F0: the speed is the length of the velocity, the heading its direction (the facing when it is zero)
    let s2 = arith::add(arith::mul(kin.vy, kin.vy), arith::mul(kin.vx, kin.vx));
    let s = math::sqrt(s2, t);
    let mut rd = if !lt(abs(s), SPEED_FLOOR) { s } else { F32::ZERO };
    let mut rh = if rd.to_f32() == 0.0 {
        kin.facing
    } else {
        heading_of(kin.vx, kin.vy, rd, t)
    };
    // 0x6F495320: the arrival test against the goal (0x6F493EA0)
    let gx = arith::sub(goal.0, pos.0);
    let gy = arith::sub(goal.1, pos.1);
    let dist = math::sqrt(arith::add(arith::mul(gy, gy), arith::mul(gx, gx)), t);
    let goal_delta = heading_error(rh, gx, gy, t);
    let within = !lt(pilot.radius, dist) && !pilot.radius.to_f32().is_nan() && !dist.to_f32().is_nan();
    let aligned = !lt(ARRIVE_ANGLE, abs(goal_delta)) && !abs(goal_delta).to_f32().is_nan();
    let arrived = within && aligned;
    if arrived {
        rd = F32::ZERO;
    } else if within {
        steer(&pilot.steer, &mut rd, &mut rh, goal_delta, true);
    } else {
        let (wx, wy, blocked) = match path {
            PathState::Open(w) => (arith::sub(w.0, pos.0), arith::sub(w.1, pos.1), false),
            PathState::Blocked => (gx, gy, true),
        };
        let delta = heading_error(rh, wx, wy, t);
        steer(&pilot.steer, &mut rd, &mut rh, delta, blocked);
    }
    // the cap, then the speed writer: speed = min(limit, speed) with limit = the cap
    if lt(kin.cap, rd) {
        rd = kin.cap;
    }
    let limit = if lt(FLT_MAX, kin.cap) { FLT_MAX } else { kin.cap };
    let speed = if gt(limit, rd) { rd } else { limit };
    kin.set_speed_heading(g, speed, rh, t);
    WalkTrace { speed: rd, heading: rh, arrived }
}

/// The exact divide by 32 the original uses for world-to-cell speeds and radii (`0x6F4747B0`, `0x6F474D40`): 5 is
/// subtracted from the exponent field, and a value whose exponent is below 6 (or whose sign flips under the
/// subtraction) becomes `+0.0`.
pub fn div32(v: F32) -> F32 {
    let a = v.0;
    let keep = !(((a ^ a.wrapping_sub(0x0300_0000)) as i32) >> 31) as u32;
    F32(a.wrapping_sub(0x0280_0000) & keep)
}

/// The speed bound of the world object (`mgr+0x80`, written by the map loader `0x6F46B850` from the cell size 32.0
/// and the walk period 0.03): `32 / 0.03 * 0.49`, which is 522.67 (CONFIRMED arithmetic, Unicorn on the helpers).
pub fn speed_bound(t: &Tables) -> F32 {
    let q = arith::div(F32(0x4200_0000), F32(0x3CF5_C290), t);
    arith::mul(q, F32(0x3EFA_E148))
}

/// The cap (cells per second) a speed (world units per second) gives the mover: `min(speed, bound) / 32`
/// (`Position::vf6`, `0x6F4747B0`; CONFIRMED by disassembly). A speed above the bound is replaced by the bound; a NaN
/// speed passes through (x87 compare).
pub fn speed_to_cap(speed: F32, bound: F32) -> F32 {
    let x = if gt(speed, bound) { bound } else { speed };
    div32(x)
}

/// The arrival radius of a move (cells) from the radius argument of the move issue (`0x6F474D40`): `max(0.49, r / 32)`.
pub fn arrival_radius(radius: F32) -> F32 {
    let x = div32(radius);
    if lt(F32(0x3EFA_E148), x) {
        x
    } else {
        F32(0x3EFA_E148)
    }
}

/// The turn cap of a unit (`0x6F474B30`): the turn speed folded into `[0, 2*pi)` by `0x6F47C420`, not below `0.001`.
pub fn turn_cap(turn_speed: F32) -> F32 {
    let a = wrap_positive(turn_speed);
    if F32(0x3A83_126F).to_f32() >= a.to_f32() {
        F32(0x3A83_126F)
    } else {
        a
    }
}

/// The propulsion window of a unit (`0x6F474BA0`): the value folded into `[0, 2*pi)`.
pub fn prop_window(window: F32) -> F32 {
    wrap_positive(window)
}

/// `0x6F47C420`: `x - t` with `t = trunc(x / 2*pi) * 2*pi`, and `t - 2*pi` for a negative `x` (checked against the
/// original by `tools/emu/movement6_emu.py WP`).
pub fn wrap_positive(x: F32) -> F32 {
    let k = arith::mul(x, F32(0x3E22_F983)); // 1 / (2 pi)
    let mut t = arith::mul(TWO_PI, math::trunc(k));
    if x.to_f32() < 0.0 {
        t = arith::sub(t, TWO_PI);
    }
    arith::sub(x, t)
}
