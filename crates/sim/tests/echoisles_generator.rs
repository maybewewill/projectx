//! The generator link of the EchoIsles replay (`docs/spec/lockstep.md` section 19).
//!
//! The replay `echoisles_ai_1.w3g` (not committed) records one 0x22 value per time slot. Its hidden word changes by
//! `rol3(R_a ^ R_b)` in the slots where the global generator is the only thing that moved (R = the byte-hash sum of the
//! generator state, `sim::sync::rng_state_hash`). The table below holds those deltas exactly as read from the file and
//! the draw counts `(a, b)` they identify when the generator is seeded with the fixed seed `0x77617233`. They are
//! evidence for the seed link: the record seed `0x030b2473` explains none of them.
//!
//! Needs the user's `Game.dll` (the 256-byte table); set `GAME_DLL` to run, otherwise the test is skipped.

use sim::rng::{Rng, RngTable, FIXED_GAME_SEED};
use sim::sync::rng_state_hash;

/// `(slot, hidden delta of the file, draws before, draws after)`.
const ANCHORS: &[(u32, u32, u32, u32)] = &[
    (42, 0x03f1_c060, 29, 30),
    (51, 0x0325_2540, 30, 31),
    (102, 0x06c3_5600, 31, 32),
    (150, 0x06c8_ac40, 44, 45),
    (173, 0x0240_8860, 46, 47),
    (384, 0x1efd_9620, 62, 63),
    (404, 0x1f70_7600, 63, 64),
    (414, 0x1ef2_2700, 66, 67),
];

fn table() -> Option<RngTable> {
    let path = std::env::var_os("GAME_DLL")?;
    let bytes = std::fs::read(path).ok()?;
    RngTable::from_game_dll(&bytes).ok()
}

fn hashes(seed: u32, table: &RngTable, n: usize) -> Vec<u32> {
    let mut rng = Rng::seeded(seed);
    let mut out = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        out.push(rng_state_hash(&rng));
        rng.next_u32(table);
    }
    out
}

#[test]
fn recorded_generator_deltas_follow_the_fixed_seed() {
    let Some(table) = table() else {
        eprintln!("GAME_DLL not set: skipped");
        return;
    };
    let h = hashes(FIXED_GAME_SEED, &table, 80);
    for &(slot, delta, a, b) in ANCHORS {
        let want = (h[a as usize] ^ h[b as usize]).rotate_left(3);
        assert_eq!(want, delta, "slot {slot}: draws {a} -> {b}");
    }
}

#[test]
fn the_record_seed_does_not_explain_the_deltas() {
    let Some(table) = table() else {
        eprintln!("GAME_DLL not set: skipped");
        return;
    };
    // Any draw window up to 3000 draws and 30 steps wide: count the anchors the record seed matches by chance.
    let h = hashes(0x030b_2473, &table, 3000);
    let mut matches = 0;
    for &(_, delta, _, _) in ANCHORS {
        let target = delta.rotate_right(3);
        for a in 0..h.len() {
            for k in 1..=30 {
                if a + k < h.len() && h[a] ^ h[a + k] == target {
                    matches += 1;
                }
            }
        }
    }
    assert!(matches <= 2, "the record seed matched {matches} anchors");
}

#[test]
fn the_first_two_draws_resolve_human_and_undead() {
    let Some(table) = table() else {
        eprintln!("GAME_DLL not set: skipped");
        return;
    };
    // 0x6F3A31A0: race = (draw >> 30) + 1 for each random-race player in order; the replay's human is a Human.
    let mut rng = Rng::seeded(FIXED_GAME_SEED);
    assert_eq!((rng.next_u32(&table) >> 30) + 1, 1);
    assert_eq!((rng.next_u32(&table) >> 30) + 1, 3);
}
