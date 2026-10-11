//! Structural test of the 0x22 sync stream (docs/spec/lockstep.md sections 13 and 15).
//!
//! For fixed registry fold A, world words X, Y and RNG hash R, the value is GF(2)-affine in the tick clock T:
//! `sync(T) ^ sync(T') = rol(T ^ T', 12)`. So for two consecutive records with unchanged hidden state,
//! `rotr(csum_k ^ csum_k+1, 12) = T_k ^ T_k+1`, a small number. The synthetic cases always run and check that this
//! signature is detected or hidden. `W3G_REPLAY=/path/file.w3g` checks the stream structure of the user's real
//! 1.26a file (never committed): one record after every time slot. The content of the records is checked in
//! `engine/tests/sync_real.rs`, which has the net checksum.

use w3g::sync::sync_value;
use w3g::{Block, Replay};

fn rotr(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

/// Number of consecutive pairs whose XOR, un-rotated by 12, is below 2^16 (the constant-state signature).
fn small_pairs(clock: &[u32], csum: &[u32]) -> usize {
    (0..csum.len().saturating_sub(1))
        .filter(|&k| rotr(csum[k] ^ csum[k + 1], 12) < (1 << 16) && clock[k] != clock[k + 1])
        .count()
}

#[test]
fn constant_state_produces_the_signature() {
    let (a, x, y, r) = (0x1234_5678u32, 0x0BAD_F00D, 0x0F0F_1234, 0x0001_0203);
    let clock: Vec<u32> = (0..200u32).map(|k| 110 * k).collect();
    let csum: Vec<u32> = clock.iter().map(|&t| sync_value(t, a, x, y, r)).collect();
    assert_eq!(small_pairs(&clock, &csum), clock.len() - 1);
}

#[test]
fn changing_state_hides_the_signature() {
    let clock: Vec<u32> = (0..200u32).map(|k| 110 * k).collect();
    let csum: Vec<u32> = clock
        .iter()
        .enumerate()
        .map(|(k, &t)| sync_value(t, (k as u32).wrapping_mul(0x9E37_79B9), 0x55, 0xAA, 0x33))
        .collect();
    // Random-like state: only a handful of pairs can fall below 2^16 by chance (expected about 0.4).
    assert!(small_pairs(&clock, &csum) < 5);
}

fn real_replay() -> Option<Vec<u8>> {
    let path = std::env::var("W3G_REPLAY").ok()?;
    Some(std::fs::read(path).expect("read W3G_REPLAY"))
}

#[test]
fn real_replay_has_one_record_per_time_slot() {
    let Some(bytes) = real_replay() else {
        return;
    };
    let r = Replay::parse(&bytes).expect("real replay parses");
    let mut slots = 0usize;
    let mut records = 0usize;
    for b in &r.blocks {
        match b {
            Block::TimeSlot { .. } => slots += 1,
            Block::Checksum { .. } => records += 1,
            _ => {}
        }
    }
    // The real file has a record after every slot except possibly the last one(s).
    assert!(records + 64 >= slots && records <= slots, "slots {slots} records {records}");
}
