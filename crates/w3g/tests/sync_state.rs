//! Structural test of the 0x22 sync stream (docs/spec/lockstep.md section 13.6).
//!
//! For fixed registry fold A, world words X and Y, the value is GF(2)-affine in the tick clock T:
//! `sync(T) ^ sync(T') = rol(T ^ T', 12)`. So for two consecutive records with unchanged hidden state,
//! `rotr(csum_k ^ csum_k+1, 12) = T_k ^ T_k+1`, a small number (T grows by about 110 per slot).
//! The synthetic case always runs and checks that this signature is detected. `W3G_REPLAY=/path/file.w3g`
//! runs the same count on the user's real 1.26a file (never committed): the test documents that no pair of
//! consecutive records shows the signature, i.e. the hidden state (A, X, Y) changes between slots.

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
    let (a, x, y) = (0x1234_5678u32, 0x0BAD_F00D, 0x0F0F_1234);
    let clock: Vec<u32> = (0..200u32).map(|k| 110 * k).collect();
    let csum: Vec<u32> = clock.iter().map(|&t| sync_value(t, a, x, y)).collect();
    assert_eq!(small_pairs(&clock, &csum), clock.len() - 1);
}

#[test]
fn changing_state_hides_the_signature() {
    let clock: Vec<u32> = (0..200u32).map(|k| 110 * k).collect();
    let csum: Vec<u32> = clock
        .iter()
        .enumerate()
        .map(|(k, &t)| sync_value(t, (k as u32).wrapping_mul(0x9E37_79B9), 0x55, 0xAA))
        .collect();
    // Random-like state: only a handful of pairs can fall below 2^16 by chance (expected about 0.4).
    assert!(small_pairs(&clock, &csum) < 5);
}

fn real_replay() -> Option<Vec<u8>> {
    let path = std::env::var("W3G_REPLAY").ok()?;
    Some(std::fs::read(path).expect("read W3G_REPLAY"))
}

#[test]
fn real_replay_has_no_constant_state_pairs() {
    let Some(bytes) = real_replay() else {
        return;
    };
    let r = Replay::parse(&bytes).expect("real replay parses");
    // T = game milliseconds before each slot (the tick clock at the record, LIKELY; see section 13.5).
    let mut clock = Vec::new();
    let mut csum = Vec::new();
    let mut time = 0u32;
    let mut pending_slot = false;
    for b in &r.blocks {
        match b {
            Block::TimeSlot { increment_ms, .. } => {
                clock.push(time);
                time = time.wrapping_add(*increment_ms as u32);
                pending_slot = true;
            }
            Block::Checksum { csum: c } if pending_slot => {
                csum.push(*c);
                pending_slot = false;
            }
            _ => {}
        }
    }
    assert_eq!(csum.len(), clock.len(), "one record per time slot");
    let n = small_pairs(&clock, &csum);
    assert_eq!(n, 0, "consecutive records must not share one hidden state");
}
