//! Bounce attacks (`weapTp` mbounce, `CMissileBounce`) and the splash launch defaults (`docs/spec/weapon-splash.md`).
//!
//! Plain models of routines of Game.dll 1.26a. The candidate callback is pinned by Unicorn
//! (`tools/emu/splash_emu.py`, `crates/sim/tests/bounce.rs`); the arrival rules are read from the machine code.

use softfloat::{arith, F32};

/// The launch defaults of the splash factors (`0x6F2C4680`): a factor of exactly zero (either sign) becomes 0.5 for
/// `Hfact` (constant `0x6FAAE4D4`) and 0.25 for `Qfact` (`0x6FAAE4B8`). CONFIRMED by reading (the compare is an x87
/// equality with 0.0).
pub fn launch_factors(hfact: F32, qfact: F32) -> (F32, F32) {
    let zero = |v: F32| f32::from_bits(v.to_bits()) == 0.0;
    (
        if zero(hfact) { F32::from_bits(0x3F00_0000) } else { hfact },
        if zero(qfact) { F32::from_bits(0x3E80_0000) } else { qfact },
    )
}

/// The initial best distance of the bounce target search (`0x6FAAE624`, the largest finite float).
pub const BEST_INIT: F32 = F32::from_bits(0x7F7F_FFFF);

/// The bounce candidate callback (`0x6F2BBAF0`) over a list of `(accepted, distance)` candidates in enumeration order:
/// an accepted candidate replaces the best one when its distance is **strictly** smaller (an x87 compare of the raw
/// floats, so NaN and ties keep the earlier one). Returns the index of the best candidate and its distance.
/// CONFIRMED (500 Unicorn vectors).
pub fn pick_nearest(candidates: &[(bool, F32)]) -> (Option<usize>, F32) {
    let mut best = BEST_INIT;
    let mut which = None;
    for (i, &(accepted, dist)) in candidates.iter().enumerate() {
        if !accepted {
            continue;
        }
        if f32::from_bits(dist.to_bits()) < f32::from_bits(best.to_bits()) {
            best = dist;
            which = Some(i);
        }
    }
    (which, best)
}

/// What a `CMissileBounce` arrival does to its damage after a hit (`0x6F2C5ED0`, CONFIRMED structure, not run in
/// Unicorn): the hit counter drops by one and the missile ends when it reaches zero; then, when the loss differs from
/// zero by at least 0.001, the amount is multiplied by `1 - loss` and the missile ends when the new amount is within
/// 0.001 of zero. Returns the new amount, or `None` when the missile ends.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn after_hit(counter: &mut i32, amount: F32, loss: F32) -> Option<F32> {
    *counter = counter.wrapping_sub(1);
    if *counter == 0 {
        return None;
    }
    let d = arith::sub(loss, F32::ZERO).to_bits() & 0x7FFF_FFFF;
    let differs = !(f32::from_bits(d) < f32::from_bits(0x3A83_126F));
    if !differs {
        return Some(amount);
    }
    let next = arith::mul(amount, arith::sub(F32::ONE, loss));
    if arith::approx_eq(next, F32::ZERO) {
        return None;
    }
    Some(next)
}

/// The target flag word of a bounce: the weapon's splash flags with the relation bits cleared and `enemy` set
/// (`0x6F0CFC9D`..: `flags & 0xFFFF0FFF | 0x10000`; a weapon without a splash object uses 1, which keeps the `none`
/// bit and therefore matches nothing).
pub fn bounce_targs(splash_targs: Option<u32>) -> u32 {
    (splash_targs.unwrap_or(1) & 0xFFFF_0FFF) | 0x1_0000
}
