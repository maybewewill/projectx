//! Property tests that do not need the Game.dll table.
//!
//! Where a generator is needed, a stand-in permutation is used. The properties
//! checked here hold for any valid table, so the stand-in is enough.

use softfloat::{arith, F32};

/// Soft-float add on raw bit patterns (the `softfloat` crate's `arith::add`).
fn sfadd(x: u32, y: u32) -> u32 {
    arith::add(F32::from_bits(x), F32::from_bits(y)).to_bits()
}

/// Soft-float subtract on raw bit patterns.
fn sfsub(x: u32, y: u32) -> u32 {
    arith::sub(F32::from_bits(x), F32::from_bits(y)).to_bits()
}

/// Soft-float multiply on raw bit patterns.
fn sfmul(x: u32, y: u32) -> u32 {
    arith::mul(F32::from_bits(x), F32::from_bits(y)).to_bits()
}
use sim::{Rng, RngError, RngTable, Streams, STREAM_COUNT};

/// Identity permutation: a valid table that is not the game's data.
fn stand_in_table() -> RngTable {
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    RngTable::from_bytes(bytes).expect("identity is a permutation")
}

/// Deterministic pseudo-random 32-bit values (xorshift), for sampling inputs.
struct Sampler(u32);

impl Sampler {
    fn word(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn state(&mut self) -> Rng {
        Rng {
            a: self.word(),
            b: self.word(),
        }
    }

    /// Finite normal float32 bit pattern with a moderate exponent (2^-27 .. 2^32).
    fn normal_bits(&mut self) -> u32 {
        let sign = self.word() & 0x8000_0000;
        let exp = 100 + self.word() % 60;
        let mant = self.word() & 0x007F_FFFF;
        sign | (exp << 23) | mant
    }
}

const INT_BOUNDS: &[i32] = &[
    0,
    1,
    -1,
    7,
    -7,
    1000,
    -1000,
    i32::MAX,
    i32::MIN,
    i32::MAX - 1,
    i32::MIN + 1,
];

#[test]
fn random_int_equal_bounds_returns_lo_without_drawing() {
    let t = stand_in_table();
    let mut s = Sampler(7);
    for &lo in INT_BOUNDS {
        for _ in 0..20 {
            let mut rng = s.state();
            let before = rng;
            assert_eq!(rng.random_int(&t, lo, lo), lo);
            assert_eq!(rng, before, "state must not change when lo == hi");
        }
    }
}

#[test]
fn random_int_stays_in_range_and_draws_once() {
    let t = stand_in_table();
    let mut s = Sampler(11);
    for &lo in INT_BOUNDS {
        for &hi in INT_BOUNDS {
            if lo == hi {
                continue;
            }
            for _ in 0..10 {
                let mut rng = s.state();
                let mut one_step = rng;
                let r = one_step.next_u32(&t);
                let got = rng.random_int(&t, lo, hi);
                assert_eq!(rng, one_step, "exactly one step for ({lo}, {hi})");

                // The result is lo plus an offset in 0..size, where size = |lo - hi| + 1
                // (the range runs from lo toward hi, or upward from lo when lo > hi).
                let size = (i64::from(lo) - i64::from(hi)).unsigned_abs() + 1;
                let offset =
                    (u64::from(got as u32).wrapping_sub(u64::from(lo as u32))) & 0xFFFF_FFFF;
                assert!(offset < size, "{got} outside the range of ({lo}, {hi})");

                // The offset is the high 32 bits of r * size (size taken in 32 bits, see rng.rs).
                let size32 = size & 0xFFFF_FFFF;
                let want = (u64::from(r) * size32) >> 32;
                assert_eq!(
                    got,
                    lo.wrapping_add(want as u32 as i32),
                    "offset rule for ({lo}, {hi})"
                );
            }
        }
    }
}

#[test]
fn full_range_returns_lo_because_size_wraps_to_zero() {
    let t = stand_in_table();
    let mut rng = Rng { a: 1, b: 2 };
    let mut one_step = rng;
    one_step.next_u32(&t);
    assert_eq!(rng.random_int(&t, i32::MIN, i32::MAX), i32::MIN);
    assert_eq!(rng, one_step, "the step is still taken");

    let mut rng = Rng { a: 5, b: 9 };
    assert_eq!(rng.random_int(&t, i32::MAX, i32::MIN), i32::MAX);
}

#[test]
fn random_real_equal_bounds_returns_bound_and_draws_once() {
    let t = stand_in_table();
    let mut s = Sampler(3);
    for _ in 0..200 {
        let x = f32::from_bits(s.normal_bits());
        let mut rng = s.state();
        let mut one_step = rng;
        one_step.next_u32(&t);
        let got = rng.random_real(&t, x, x);
        assert_eq!(
            got.to_bits(),
            x.to_bits(),
            "GetRandomReal(x, x) == x for {x:e}"
        );
        assert_eq!(rng, one_step, "one step even when the bounds are equal");
    }
}

#[test]
fn random_real_stays_between_bound_and_bound_plus_range() {
    let t = stand_in_table();
    let mut s = Sampler(5);
    for _ in 0..2000 {
        let a = f32::from_bits(s.normal_bits());
        let b = f32::from_bits(s.normal_bits());
        let mut rng = s.state();
        let r = rng.random_real(&t, a, b);
        assert!(r.is_finite(), "finite result for ({a:e}, {b:e})");
        // The result runs from a upward (toward b when b > a, and away from a
        // when b < a), and never passes a + |b - a|. Allow a small slack for
        // the truncating soft-float arithmetic.
        let (a64, r64) = (f64::from(a), f64::from(r));
        let range = (f64::from(b) - a64).abs();
        let slack = 1e-6 * (a64.abs() + range + 1.0);
        assert!(
            r64 >= a64 - slack,
            "{r:e} below {a:e} (bounds {a:e}, {b:e})"
        );
        assert!(
            r64 <= a64 + range + slack,
            "{r:e} beyond a + |b - a| (bounds {a:e}, {b:e})"
        );
    }
}

#[test]
fn seed_is_deterministic_and_fields_are_packed_as_specified() {
    let mut s = Sampler(9);
    for _ in 0..500 {
        let seed = s.word();
        let r1 = Rng::seeded(seed);
        assert_eq!(r1, Rng::seeded(seed));
        assert_eq!(r1.a, seed);
        let b = u64::from(r1.b);
        let s64 = u64::from(seed);
        assert_eq!((b >> 10) & 0x3F, s64 % 59);
        assert_eq!((b >> 2) & 0x3F, s64 % 61);
        assert_eq!((b >> 18) & 0x3F, s64 % 53);
        assert_eq!(b >> 26, ((s64 / 47) * 17 + s64) & 0x3F);
    }
}

#[test]
fn set_random_seed_is_seed_then_one_step_then_streams() {
    let t = stand_in_table();
    let mut s = Sampler(21);
    for _ in 0..50 {
        let seed = s.word();
        let mut streams = Streams::default();
        streams.set_random_seed(&t, seed);

        let mut g = Rng::seeded(seed);
        let out = g.next_u32(&t);
        assert_eq!(streams.global, g);

        let mut expect = Streams::default();
        expect.init_streams(&t, out);
        for k in 0..STREAM_COUNT {
            assert_eq!(streams.stream(k), expect.stream(k), "stream {k}");
        }
    }
}

#[test]
fn start_game_seeds_global_without_a_step() {
    let t = stand_in_table();
    let mut streams = Streams::default();
    streams.start_game(&t, 0x1234_5678);
    assert_eq!(streams.global, Rng::seeded(0x1234_5678));

    let mut expect = Streams::default();
    expect.init_streams(&t, 0x1234_5678);
    for k in 0..STREAM_COUNT {
        assert_eq!(streams.stream(k), expect.stream(k), "stream {k}");
    }
}

#[test]
fn stream_mut_draws_only_that_stream() {
    let t = stand_in_table();
    let mut streams = Streams::default();
    streams.init_streams(&t, 42);
    let mut expect = streams.clone();

    let before = streams.stream(7);
    streams.stream_mut(7).next_u32(&t);
    assert_ne!(streams.stream(7), before);
    expect.stream_mut(7).next_u32(&t);
    assert_eq!(streams, expect);
    assert_eq!(streams.global, Rng::default());
}

#[test]
fn soft_add_and_mul_are_commutative_for_normal_values() {
    let mut s = Sampler(31);
    for _ in 0..5000 {
        let (x, y) = (s.normal_bits(), s.normal_bits());
        assert_eq!(sfadd(x, y), sfadd(y, x), "add({x:#010x}, {y:#010x})");
        assert_eq!(sfmul(x, y), sfmul(y, x), "mul({x:#010x}, {y:#010x})");
    }
}

#[test]
fn soft_identities_for_normal_values() {
    let mut s = Sampler(41);
    for _ in 0..5000 {
        let x = s.normal_bits();
        let y = s.normal_bits();
        assert_eq!(sfadd(x, 0), x, "x + 0 == x");
        assert_eq!(sfsub(x, x), 0, "x - x == +0");
        assert_eq!(sfmul(x, 0x3F80_0000), x, "x * 1.0 == x");
        assert_eq!(
            sfmul(x, 0x4000_0000),
            x + (1 << 23),
            "x * 2.0 doubles the exponent"
        );
        assert_eq!(
            sfsub(x, y),
            sfadd(x, y ^ 0x8000_0000),
            "sub is add of the negation"
        );
    }
}

#[test]
fn soft_ops_match_ieee_for_exact_results() {
    let cases: &[(f32, f32)] = &[
        (1.5, 1.5),
        (3.0, 0.5),
        (-2.5, 4.0),
        (0.75, -8.0),
        (1.25, 1.75),
    ];
    for &(x, y) in cases {
        let (xb, yb) = (x.to_bits(), y.to_bits());
        assert_eq!(sfmul(xb, yb), (x * y).to_bits(), "mul {x} * {y}");
        assert_eq!(sfadd(xb, yb), (x + y).to_bits(), "add {x} + {y}");
        assert_eq!(sfsub(xb, yb), (x - y).to_bits(), "sub {x} - {y}");
    }
}

#[test]
fn table_must_be_a_permutation() {
    assert_eq!(
        RngTable::from_bytes([0u8; 256]).err(),
        Some(RngError::NotPermutation)
    );
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    bytes[3] = 4; // 4 twice, 3 missing
    assert_eq!(
        RngTable::from_bytes(bytes).err(),
        Some(RngError::NotPermutation)
    );
}

#[test]
fn table_from_non_pe_is_rejected() {
    let err = RngTable::from_game_dll(b"plain text, not a DLL").err();
    assert!(matches!(err, Some(RngError::Image(_))), "{err:?}");
    assert!(RngTable::from_game_dll(&[]).is_err());
}
