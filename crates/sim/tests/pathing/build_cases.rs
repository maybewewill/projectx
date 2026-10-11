//! The pyramid builder against the digests recorded from the original `Game.dll` 1.26a
//! (`build_data.rs`, from `tools/emu/pyramid_build_vectors.py`). The cases are regenerated here
//! with the generator of that script (64-bit LCG, the same draws in the same order).

use sim::pathing::build::{build_levels, level_dims};
use sim::pathing::NO_OCCUPANT;

use super::build_data::BUILD_CASES;

const DIGEST_INIT: u64 = 0xCBF2_9CE4_8422_2325;
const DIGEST_MUL: u64 = 0x0000_0100_0000_01B3;
const FIXED_DIMS: [(u32, u32); 12] = [
    (1, 1),
    (1, 2),
    (2, 1),
    (2, 2),
    (3, 3),
    (1, 40),
    (40, 1),
    (40, 40),
    (17, 9),
    (9, 17),
    (33, 21),
    (7, 7),
];
const DENSITY: [u32; 6] = [0, 5, 20, 50, 80, 100];

/// The generator of the vector script.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    fn u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }
}

/// Case `index`: dimensions, then the density, then one pair of draws per cell (row-major).
fn make_case(index: u32) -> (u32, u32, Vec<u32>) {
    let mut rng = Lcg::new(u64::from(index));
    let (w, h) = if (index as usize) < FIXED_DIMS.len() {
        FIXED_DIMS[index as usize]
    } else {
        (1 + rng.u32() % 40, 1 + rng.u32() % 40)
    };
    let density = DENSITY[(rng.u32() % DENSITY.len() as u32) as usize];
    let words = (0..w * h)
        .map(|_| {
            let r = rng.u32();
            let t = rng.u32() & 0xFF;
            let top = if r % 100 < density { t } else { 0 };
            (top << 24) | NO_OCCUPANT
        })
        .collect();
    (w, h, words)
}

#[test]
fn builder_matches_the_original_on_every_recorded_case() {
    assert_eq!(BUILD_CASES.len(), 300);
    for &(index, w, h, digest) in BUILD_CASES {
        let (gw, gh, words) = make_case(index);
        assert_eq!((gw, gh), (w, h), "case {index}: generator dimensions");
        let levels = build_levels(&words, gw, gh).expect("build");
        let mut dig = DIGEST_INIT;
        for level in &levels {
            for r in level.records() {
                dig = dig
                    .wrapping_mul(DIGEST_MUL)
                    .wrapping_add(u64::from(r.word1));
            }
        }
        assert_eq!(dig, digest, "case {index} ({w}x{h})");
    }
}

#[test]
fn recorded_level_dimensions_are_ceiling_halves() {
    for &(_, w, h, _) in BUILD_CASES {
        let dims = level_dims(w, h);
        assert_eq!(dims[0], (w.div_ceil(2), h.div_ceil(2)));
        assert_eq!(dims[3].0, w.div_ceil(16));
    }
}
