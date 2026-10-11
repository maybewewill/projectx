//! Game-start seed rules (`docs/spec/game-start.md`).
//!
//! The expected values come from the original `Game.dll` 1.26a run under Unicorn
//! (`tools/emu/game_start_emu.py`), not from the model alone.

use sim::rng::{
    game_start_seed, slot_fold, slot_fold_step, Rng, RngTable, Streams, FIXED_GAME_SEED,
};

fn identity_table() -> RngTable {
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    RngTable::from_bytes(bytes).unwrap()
}

/// `(S, X, S')` from the original's fold instructions (`0x6F5C3A4C..0x6F5C3A5B`).
const FOLD_VECTORS: &[(u32, u32, u32)] = &[
    (0x0000_0000, 0x0000_0000, 0x0000_0000),
    (0x0000_0001, 0x0000_0000, 0x0000_0008),
    (0x1234_5678, 0x9ABC_DEF0, 0x4444_4444),
    (0xFFFF_FFFF, 0x0000_0001, 0xFFFF_FFF7),
    (0x8000_0000, 0x8000_0000, 0x0000_0000),
];

#[test]
fn fold_step_matches_original_instructions() {
    for &(s, x, want) in FOLD_VECTORS {
        assert_eq!(slot_fold_step(s, x), want, "s={s:#x} x={x:#x}");
    }
}

#[test]
fn fold_is_left_to_right_from_zero() {
    // Folding one value is one step from zero.
    assert_eq!(slot_fold([0x1234_5678]), slot_fold_step(0, 0x1234_5678));
    // Two values: step applied in list order (rotation makes the order matter).
    let ab = slot_fold([3, 0x100]);
    let ba = slot_fold([0x100, 3]);
    assert_eq!(ab, slot_fold_step(slot_fold_step(0, 3), 0x100));
    assert_ne!(ab, ba);
    assert_eq!(slot_fold(std::iter::empty()), 0);
}

#[test]
fn fixed_seed_flag_selects_constant() {
    assert_eq!(FIXED_GAME_SEED, 0x7761_7233);
    // Flag 0x8000 set: constant, whatever the fold is.
    for flags in [0x8000, 0xFFFF_FFFF, 0x0001_8000] {
        assert_eq!(game_start_seed(flags, 0x1111_1111), 0x7761_7233);
    }
    // Flag clear: the fold value is used unchanged.
    for flags in [0, 0x7FFF, 0xFFFF_7FFF, 0x0010_0000] {
        assert_eq!(game_start_seed(flags, 0x1111_1111), 0x1111_1111);
        assert_eq!(game_start_seed(flags, 0), 0);
    }
}

#[test]
fn start_game_seeds_global_from_s_and_streams_from_s() {
    let t = identity_table();
    for s in [0u32, 1, 0x7761_7233, 0xDEAD_BEEF] {
        let mut streams = Streams::default();
        streams.start_game(&t, s);
        // The global generator is seeded with S directly, with no step.
        assert_eq!(streams.global, Rng::seeded(s));
        // The streams are initialised from S (not from a step of the global generator).
        let mut expected = Streams::default();
        expected.init_streams(&t, s);
        for k in 0..sim::rng::STREAM_COUNT {
            assert_eq!(streams.stream(k), expected.stream(k), "stream {k}, s={s:#x}");
        }
    }
}

#[test]
fn start_game_differs_from_set_random_seed() {
    // SetRandomSeed steps the global generator once before the streams; game start does not.
    let t = identity_table();
    let mut a = Streams::default();
    a.start_game(&t, 0x1234_5678);
    let mut b = Streams::default();
    b.set_random_seed(&t, 0x1234_5678);
    assert_ne!(a.stream(0), b.stream(0));
}
