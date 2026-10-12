//! The timing of a basic attack: when the cooldown timer, the damage-point (execute) timer and the backswing timer fire,
//! and how an attack speed bonus scales them. Spec: `docs/spec/attack-timing.md`.
//!
//! Everything here is CONFIRMED by Unicorn against the original (`tools/emu/attack_timing_emu.py`, 500 random cases per
//! routine, 0 mismatches; vectors in `tests/attack_timing/`). The routines are pure: they take the attack object's
//! values and return the timer commands the original issues, so the caller (`combat.rs`) only has to execute them.
//!
//! | Routine | Original | What it decides |
//! |---|---|---|
//! | [`multiplier`] | `0x6F0C6EB0` | the attack speed multiplier `M`, clamped to `[0.2, 5.0]` |
//! | [`cycle_delay`] | `0x6F0C8D70` | weapon cooldown value divided by `M(1)` |
//! | [`schedule`] | `0x6F0CB9E0` | timer `0xD01B0` (cooldown end) and `0xD01B1` (damage point) |
//! | [`backswing`] | `0x6F0C8DA0` | timer `0xD01B2` (backswing end) |
//! | [`add_bonus`] | `0x6F0C9090` | the stored bonus value and the rescaling of the three pending timers |

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN semantics follow the original's x87 compares

use softfloat::{arith, Tables, F32};

/// Message ids of the three attack timers (CONFIRMED immediates in the routines above).
pub const MSG_COOLDOWN_END: u32 = 0x000D_01B0;
pub const MSG_DAMAGE_POINT: u32 = 0x000D_01B1;
pub const MSG_BACKSWING_END: u32 = 0x000D_01B2;

/// `0x6FAAE480` after start-up: `0.01`, the smallest step between the timers of one swing.
pub const MIN_STEP: F32 = F32::from_bits(0x3C23_D70A);
/// `0x6FAAE484` after start-up: `0.02`, the least cooldown time left when the backswing starts.
pub const MIN_COOLDOWN_LEFT: F32 = F32::from_bits(0x3CA3_D70A);
/// `0x6FAAE47C`: the tolerance `0.001` used to snap the stored bonus.
pub const TOLERANCE: F32 = F32::from_bits(0x3A83_126F);
/// `0x6FAAE614`: `2e-7`, below which a ratio counts as 1.0 and a time counts as 0.
pub const RATIO_EPS: F32 = F32::from_bits(0x3456_BF95);
/// `0x6FAB0078` and `0x6FAB0074`: the clamp of the multiplier, `0.2` and `5.0`.
pub const MULT_MIN: F32 = F32::from_bits(0x3E4C_CCCE);
pub const MULT_MAX: F32 = F32::from_bits(0x40A0_0000);
/// `0x6FAAE624` / `0x6FAAE628`: the infinities of the division helper (`+FLT_MAX`, `-FLT_MAX`).
const FLT_MAX: F32 = F32::from_bits(0x7F7F_FFFF);
const NEG_FLT_MAX: F32 = F32::from_bits(0xFF7F_FFFF);

/// Which of the three timers of the attack object a command is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    /// `A+0x1D8`, message `0xD01B0`: the cooldown (the gate of the next swing).
    Cooldown,
    /// `A+0x1EC`, message `0xD01B1`: the damage point.
    DamagePoint,
    /// `A+0x200`, message `0xD01B2`: the end of the backswing.
    Backswing,
}

/// A command to the timer queue: start the timer with this many seconds (a restart replaces the pending one), or stop it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    Start { timer: Timer, seconds: F32 },
    Stop { timer: Timer },
}

/// The two stored values of the attack speed: `base` (`A+0x1B0`, starts at 1.0) and `slow` (`A+0x1C0`, starts at 0.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Speed {
    pub base: F32,
    pub slow: F32,
}

impl Default for Speed {
    fn default() -> Self {
        Speed { base: F32::ONE, slow: F32::ZERO }
    }
}

/// Whether the weapon mode word (`A+0x218+4w`) is one that ignores the multiplier (`0x6F089050`): 1, `0x40`, `0x80`
/// and `0x100` make the multiplier 1.0.
pub fn mode_ignores_speed(mode: u32) -> bool {
    matches!(mode, 1 | 0x40 | 0x80 | 0x100)
}

fn lt(a: F32, b: F32) -> bool {
    a.to_f32() < b.to_f32()
}

/// `|a - b|` through the soft subtract with the sign bit cleared, the form every snap/compare in these routines uses.
fn abs_diff(a: F32, b: F32) -> F32 {
    F32::from_bits(arith::sub(a, b).to_bits() & 0x7FFF_FFFF)
}

/// `0x6F01FED0`: `|a - b| >= 2e-7` (also true for NaN).
fn differs(a: F32, b: F32) -> bool {
    !(abs_diff(a, b).to_f32() < RATIO_EPS.to_f32())
}

/// `0x6F07C350` on the slow property: true when the stored value is negative and at least `0.001` away from zero.
fn slow_active(p: F32) -> bool {
    let pf = p.to_f32();
    if !(pf < 0.0) {
        return false;
    }
    !(TOLERANCE.to_f32() > abs_diff(p, F32::ZERO).to_f32())
}

/// The attack speed multiplier (`0x6F0C6EB0`). `mode_ignored` is true when a weapon index is given and its mode word
/// is one of [`mode_ignores_speed`]; `flag` selects the cooldown form, which adds a negative `slow` value.
pub fn multiplier(speed: Speed, mode_ignored: bool, flag: bool) -> F32 {
    if mode_ignored {
        return F32::ONE;
    }
    let mut m = speed.base;
    if flag && slow_active(speed.slow) {
        m = arith::add(speed.base, speed.slow);
    }
    // x87 compare order of the original: below the minimum gives the minimum, above the maximum the maximum, NaN stays.
    if MULT_MIN.to_f32() > m.to_f32() {
        return MULT_MIN;
    }
    if MULT_MAX.to_f32() < m.to_f32() {
        return MULT_MAX;
    }
    m
}

/// `0x6F0223C0`: a stored time `value` divided by the multiplier `s`. A zero multiplier gives 1.0 for a zero value and
/// plus or minus the largest float otherwise.
pub fn scaled(value: F32, s: F32, tables: &Tables) -> F32 {
    if s.to_f32() != 0.0 || s.to_f32().is_nan() {
        return arith::div(value, s, tables);
    }
    let v = value.to_f32();
    if v == 0.0 {
        F32::ONE
    } else if v < 0.0 {
        NEG_FLT_MAX
    } else {
        FLT_MAX
    }
}

/// The delay of the whole cycle (`0x6F0C8D70`): the weapon cooldown value `x1` over the multiplier of the cooldown form.
pub fn cycle_delay(speed: Speed, mode_ignored: bool, x1: F32, tables: &Tables) -> F32 {
    scaled(x1, multiplier(speed, mode_ignored, true), tables)
}

/// `schedule(delay, flag)` (`0x6F0CB9E0`). `remaining` is the time left on the cooldown timer, `x2` the weapon's damage
/// point value. The cooldown timer is only touched when its remaining time is below `delay`: it is started with `delay`
/// (or stopped when `delay` is negative). With `flag` the damage point timer is started with the damage point over the
/// plain multiplier, at most `delay - 0.01`.
pub fn schedule(
    speed: Speed,
    mode_ignored: bool,
    x2: F32,
    delay: F32,
    flag: bool,
    remaining: F32,
    tables: &Tables,
) -> Vec<Cmd> {
    let mut out = Vec::new();
    if !lt(remaining, delay) {
        return out;
    }
    if delay.to_f32() < 0.0 {
        out.push(Cmd::Stop { timer: Timer::Cooldown });
    } else {
        out.push(Cmd::Start { timer: Timer::Cooldown, seconds: delay });
    }
    if flag {
        let mut e = scaled(x2, multiplier(speed, mode_ignored, false), tables);
        let limit = arith::sub(delay, MIN_STEP);
        if lt(limit, e) {
            e = limit;
        }
        out.push(Cmd::Start { timer: Timer::DamagePoint, seconds: e });
    }
    out
}

/// The backswing timer (`0x6F0C8DA0`), armed when the damage point fires. `x3` is the weapon's backswing value and
/// `remaining` the time left on the cooldown timer. The backswing is at least `0.01`; a cooldown with less than `0.02`
/// left is restarted with `0.02`; the backswing ends at least `0.01` before the cooldown does.
pub fn backswing(speed: Speed, mode_ignored: bool, x3: F32, remaining: F32, tables: &Tables) -> Vec<Cmd> {
    let mut out = Vec::new();
    let mut v = scaled(x3, multiplier(speed, mode_ignored, false), tables);
    if MIN_STEP.to_f32() > v.to_f32() {
        v = MIN_STEP;
    }
    let mut left = remaining;
    if MIN_COOLDOWN_LEFT.to_f32() > left.to_f32() {
        left = MIN_COOLDOWN_LEFT;
        out.push(Cmd::Start { timer: Timer::Cooldown, seconds: MIN_COOLDOWN_LEFT });
    }
    let limit = arith::sub(left, MIN_STEP);
    if lt(limit, v) {
        v = limit;
    }
    out.push(Cmd::Start { timer: Timer::Backswing, seconds: v });
    out
}

/// The pending time left on each timer, as the getters report it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pending {
    pub cooldown: F32,
    pub damage_point: F32,
    pub backswing: F32,
}

/// An attack speed bonus (`0x6F0C9090`): `delta` is added to the stored value (`slow` when `flag`, else `base`), the sum
/// snapped to 0.0 (`slow`) or 1.0 (`base`) when within `0.001` of it. The pending timers are then restarted with their
/// remaining time divided by `new / old` of the multiplier, when that ratio differs from 1 by at least `2e-7` and the
/// remaining time is not zero. The damage point and backswing timers are only rescaled for the plain form (`flag` clear).
pub fn add_bonus(
    speed: Speed,
    delta: F32,
    flag: bool,
    pending: Pending,
    tables: &Tables,
) -> (Speed, Vec<Cmd>) {
    let old_m = multiplier(speed, false, flag);
    let mut new = speed;
    if flag {
        let sum = arith::add(speed.slow, delta);
        new.slow = if TOLERANCE.to_f32() > abs_diff(sum, F32::ZERO).to_f32() { F32::ZERO } else { sum };
    } else {
        let sum = arith::add(speed.base, delta);
        new.base = if TOLERANCE.to_f32() > abs_diff(sum, F32::ONE).to_f32() { F32::ONE } else { sum };
    }
    let new_m = multiplier(new, false, flag);
    let ratio = arith::div(new_m, old_m, tables);
    let mut out = Vec::new();
    if !differs(ratio, F32::ONE) {
        return (new, out);
    }
    if differs(pending.cooldown, F32::ZERO) {
        out.push(Cmd::Start { timer: Timer::Cooldown, seconds: arith::div(pending.cooldown, ratio, tables) });
    }
    if !flag {
        if differs(pending.damage_point, F32::ZERO) {
            out.push(Cmd::Start {
                timer: Timer::DamagePoint,
                seconds: arith::div(pending.damage_point, ratio, tables),
            });
        }
        if differs(pending.backswing, F32::ZERO) {
            out.push(Cmd::Start {
                timer: Timer::Backswing,
                seconds: arith::div(pending.backswing, ratio, tables),
            });
        }
    }
    (new, out)
}
