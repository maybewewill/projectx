//! Approximate integer square root used by the region search for every cost and
//! heuristic value (Game.dll 1.26a routine at `0x6F4BABB0`, `__fastcall`: argument in
//! ECX, result in EAX).
//!
//! This is NOT an exact floor square root. It is a Newton iteration on unsigned 32-bit
//! integers, and the search keys depend on the exact value it stops on, so the stopping
//! rule is reproduced as observed:
//!
//! 1. Initial guess `u`: `n / 12 + 1` for `n < 0x100`, `n / 200 + 0x15` for
//!    `n < 0x10000`, and `n / 26743 + 0x1BC` above that.
//! 2. Repeat: `q = n / u` (unsigned), `d = u - q` (signed), `u = (q + u) / 2` (signed,
//!    truncating). Stop when `d / 2 == 0` (truncating), keeping the new `u`.
//! 3. Return `u`.
//!
//! The port is checked against the original by `tools/emu/pathing_vectors.py` for all
//! inputs `0..=65535` and 50,000 random `u32` values (see `tests/pathing`).

/// Approximate `floor(sqrt(n))`, bit for bit as the 1.26a routine returns it.
pub fn isqrt(n: u32) -> u32 {
    let mut u = initial_guess(n);
    loop {
        // `u` is never zero here: it starts at 1 or more, and the update can give zero only
        // when `n == 0` and `u == 1`; then `d == 1`, so the loop ends before another division.
        debug_assert!(u != 0);
        let q = n / u;
        let d = u.wrapping_sub(q) as i32;
        u = ((q.wrapping_add(u) as i32) / 2) as u32;
        if d / 2 == 0 {
            return u;
        }
    }
}

fn initial_guess(n: u32) -> u32 {
    if n < 0x100 {
        n / 12 + 1
    } else if n < 0x1_0000 {
        n / 200 + 0x15
    } else {
        // The original divides by 26743 with a multiply-high by 0x39ACC69D and two shifts.
        let hi = ((u64::from(n) * 0x39AC_C69D) >> 32) as u32;
        ((((n - hi) >> 1) + hi) >> 14) + 0x1BC
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_values() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(2), 1);
        assert_eq!(isqrt(4), 2);
    }

    #[test]
    fn large_guess_matches_division() {
        // The multiply-high guess must equal the plain division by 26743 it stands for.
        let mut x: u64 = 0x1234_5678;
        for _ in 0..200_000 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let n = (x >> 32) as u32;
            let hi = ((u64::from(n) * 0x39AC_C69D) >> 32) as u32;
            let magic = (((n - hi) >> 1) + hi) >> 14;
            assert_eq!(magic, n / 26743, "n = {n}");
        }
        for n in [0x1_0000, 0x6876, 0x6877, 0xFFFF_FFFF, 0x8000_0000] {
            let hi = ((u64::from(n) * 0x39AC_C69D) >> 32) as u32;
            assert_eq!((((n - hi) >> 1) + hi) >> 14, n / 26743, "n = {n}");
        }
    }
}
