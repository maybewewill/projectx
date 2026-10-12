//! The 0x22 sync record as a replay oracle (docs/spec/lockstep.md section 15).
//!
//! [`check_replay`] walks the blocks of a replay in file order with a [`sim::sync::SyncState`] (net checksum, tick
//! clock, record rule) and compares with the records stored in the file in two ways:
//!
//! * **Absolute**: the value the tracker predicts for each slot, with the hidden inputs at their start values
//!   (empty registry besides cheat flags and the net slot, `X = Y = R = 0`). This matches only if the simulation
//!   supplies the hidden words (the unit/pathing counters X and Y, the generator hash R, the data-table slots);
//!   the report gives the first mismatching slot.
//! * **Differential**: for two consecutive computed records the XOR of the stored values minus the contribution
//!   of the clock and of the net checksum ([`sim::sync::residual`]). A zero residual means the hidden words did
//!   not change and the pair is predicted exactly from the replay stream alone; a residual that is a small
//!   multiple of 64 is a change of the birth-tag counter `Y` (`rol6`).
//!
//! The report is what `w3boot --replay FILE.w3g --check-sync` prints.

use std::fmt::Write as _;

use sim::sync::{
    participant_rotation, residual, slot, tick_clock_at_slot, y_chain_feasible, RecordPoint,
    SyncState,
};
use w3g::{Block, Replay};

/// Window size (computed records) of the per-window exact-pair table.
pub const WINDOW: usize = 5000;

/// Result of [`check_replay`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Time slots in the file.
    pub time_slots: usize,
    /// 0x22 records that directly follow a time slot.
    pub records: usize,
    /// Records of slots with a nonzero increment (the ones the original computes).
    pub computed: usize,
    /// Records whose value equals the tracker's prediction with start-state hidden inputs.
    pub absolute_matches: usize,
    /// Index (in slots) of the first computed record that differs from the prediction.
    pub first_absolute_mismatch: Option<usize>,
    /// Consecutive computed-record pairs examined.
    pub pairs: usize,
    /// Pairs with residual 0: predicted exactly from the stream.
    pub exact_pairs: usize,
    /// Pairs whose residual is a nonzero change of at most 16 bits in the `rol6` (birth tag) position.
    pub tag_like_pairs: usize,
    /// All other pairs.
    pub other_pairs: usize,
    /// Pair index of the first nonzero residual.
    pub first_inexact_pair: Option<usize>,
    /// Pair index of the first exact pair.
    pub first_exact_pair: Option<usize>,
    /// Longest run of consecutive exact pairs.
    pub longest_exact_run: usize,
    /// Exact pairs per window of [`WINDOW`] pairs.
    pub exact_per_window: Vec<usize>,
    /// Runs (of at least 2) of consecutive birth-tag-only pairs.
    pub y_runs: usize,
    /// Of those, runs that one monotone counter `Y` can produce (steps up to 4096, runs cut at 12 pairs).
    pub y_chain_runs: usize,
    /// Pairs inside the chain-consistent runs.
    pub y_chain_pairs: usize,
    /// Leave blocks folded into the net checksum.
    pub leaves: usize,
    /// The net checksum at the end of the file.
    pub final_net: u32,
    /// The rotation used for the net slot (21 with the cheat and net slots present).
    pub net_rotation: u32,
}

impl SyncReport {
    /// The text `w3boot --check-sync` prints.
    pub fn render(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "sync check (docs/spec/lockstep.md section 15)");
        let _ = writeln!(
            s,
            "  time slots {}  records {}  computed {} (nonzero increment)",
            self.time_slots, self.records, self.computed
        );
        let _ = writeln!(
            s,
            "  net checksum rotation {}  final net checksum {:#010x}  leave blocks {}",
            self.net_rotation, self.final_net, self.leaves
        );
        let _ = writeln!(
            s,
            "  absolute: {} of {} computed records equal the start-state prediction; first mismatch: {}",
            self.absolute_matches,
            self.computed,
            self.first_absolute_mismatch
                .map_or("none".to_string(), |i| format!("slot {i}")),
        );
        let _ = writeln!(
            s,
            "  differential: {} pairs, {} exact (clock + net explain everything), {} birth-tag-like, {} other",
            self.pairs, self.exact_pairs, self.tag_like_pairs, self.other_pairs
        );
        let _ = writeln!(
            s,
            "  first inexact pair: {}  first exact pair: {}  longest exact run: {}",
            self.first_inexact_pair
                .map_or("none".to_string(), |i| i.to_string()),
            self.first_exact_pair
                .map_or("none".to_string(), |i| i.to_string()),
            self.longest_exact_run
        );
        let _ = writeln!(
            s,
            "  birth-tag runs (>= 2 pairs): {}  consistent with one monotone counter Y: {} ({} pairs)",
            self.y_runs, self.y_chain_runs, self.y_chain_pairs
        );
        if !self.exact_per_window.is_empty() {
            let _ = write!(s, "  exact pairs per {WINDOW} pairs:");
            for w in &self.exact_per_window {
                let _ = write!(s, " {w}");
            }
            let _ = writeln!(s);
        }
        s
    }
}

/// Checks the sync records of a replay.
pub fn check_replay(replay: &Replay) -> SyncReport {
    check_blocks(&replay.blocks)
}

/// Checks the sync records of a block list (file order).
pub fn check_blocks(blocks: &[Block]) -> SyncReport {
    let mut state = SyncState::new();
    let mut report = SyncReport {
        net_rotation: participant_rotation(&state.registry_now(), slot::NET),
        ..SyncReport::default()
    };
    // (slot index, increment, predicted value, clock, net after) of the slot that precedes the next record.
    let mut pending: Option<(usize, u16, u32, u32, u32)> = None;
    let mut points: Vec<RecordPoint> = Vec::new();
    for block in blocks {
        match block {
            Block::TimeSlot { id, .. } => {
                let payload = block.time_slot_payload().unwrap_or_default();
                let inc = u16::from_le_bytes([payload[0], payload[1]]);
                let t = tick_clock_at_slot(state.elapsed_ms());
                let predicted = state.time_slot(*id, &payload);
                pending = Some((report.time_slots, inc, predicted, t, state.net().value()));
                report.time_slots += 1;
            }
            Block::Checksum { csum } => {
                if let Some((index, inc, predicted, t, net)) = pending.take() {
                    report.records += 1;
                    if inc != 0 {
                        report.computed += 1;
                        if *csum == predicted {
                            report.absolute_matches += 1;
                        } else if report.first_absolute_mismatch.is_none() {
                            report.first_absolute_mismatch = Some(index);
                        }
                        points.push(RecordPoint {
                            value: *csum,
                            clock: t,
                            net,
                        });
                    }
                }
            }
            Block::LeaveGame {
                reason,
                player_id,
                result,
                ..
            } => {
                state.leave(*reason, *player_id, *result);
                report.leaves += 1;
            }
            Block::ForcedGameEnd { mode, countdown_s } => state.game_end(*mode, *countdown_s),
            _ => {}
        }
    }
    report.final_net = state.net().value();
    let mut run = 0usize;
    let mut y_run: Vec<u32> = Vec::new();
    let mut y_runs: Vec<Vec<u32>> = Vec::new();
    for (k, pair) in points.windows(2).enumerate() {
        let r = residual(&pair[0], &pair[1], report.net_rotation);
        report.pairs += 1;
        if k % WINDOW == 0 {
            report.exact_per_window.push(0);
        }
        if r == 0 {
            report.exact_pairs += 1;
            if let Some(w) = report.exact_per_window.last_mut() {
                *w += 1;
            }
            report.first_exact_pair.get_or_insert(k);
            run += 1;
            report.longest_exact_run = report.longest_exact_run.max(run);
        } else {
            run = 0;
            report.first_inexact_pair.get_or_insert(k);
            if r.rotate_right(6) < (1 << 16) {
                report.tag_like_pairs += 1;
                y_run.push(r.rotate_right(6));
                continue;
            } else {
                report.other_pairs += 1;
            }
        }
        if !y_run.is_empty() {
            y_runs.push(std::mem::take(&mut y_run));
        }
    }
    if !y_run.is_empty() {
        y_runs.push(y_run);
    }
    for mut r in y_runs.into_iter().filter(|r| r.len() >= 2) {
        r.truncate(12);
        report.y_runs += 1;
        if y_chain_feasible(&r, 4096) {
            report.y_chain_runs += 1;
            report.y_chain_pairs += r.len();
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use w3g::CommandBlock;

    fn slot(inc: u16, tag: u8) -> Block {
        Block::TimeSlot {
            id: 0x1F,
            increment_ms: inc,
            commands: vec![CommandBlock {
                player_id: 1,
                offset: 0,
                actions: vec![tag, 7, 8],
            }],
        }
    }

    /// Builds a synthetic stream whose records the tracker produces, with hidden words that change on some slots.
    fn synthetic(n: usize, y_changes_every: usize) -> Vec<Block> {
        let mut state = SyncState::new();
        state.registry[slot::RAND] = Some(0x5555_1234);
        state.rng_hash = 0x0102_0304;
        let mut out = Vec::new();
        for k in 0..n {
            let inc = if k % 3 == 0 { 0 } else { 36 + (k % 5) as u16 };
            if y_changes_every != 0 && k % y_changes_every == 0 {
                state.world.f50 += 3;
            }
            let b = slot(inc, k as u8);
            let payload = b.time_slot_payload().unwrap();
            let rec = state.time_slot(0x1F, &payload);
            out.push(b);
            out.push(Block::Checksum { csum: rec });
        }
        out
    }

    #[test]
    fn constant_hidden_state_is_exact_everywhere() {
        let blocks = synthetic(200, 0);
        let r = check_blocks(&blocks);
        assert_eq!(r.time_slots, 200);
        assert_eq!(r.records, 200);
        assert!(r.computed > 100);
        assert_eq!(r.pairs, r.computed - 1);
        assert_eq!(r.exact_pairs, r.pairs);
        assert_eq!(r.first_inexact_pair, None);
        assert_eq!(r.longest_exact_run, r.pairs);
        assert_eq!(r.net_rotation, 21);
        // The tracker started from the same hidden values only for the cheat/net slots, so the absolute
        // prediction differs (the synthetic stream used a rand slot and an R the checker does not know).
        assert_eq!(r.absolute_matches, 0);
        assert_eq!(r.first_absolute_mismatch, Some(1));
    }

    #[test]
    fn birth_tag_changes_show_up_as_tag_like_pairs() {
        let blocks = synthetic(120, 2);
        let r = check_blocks(&blocks);
        assert!(r.exact_pairs < r.pairs);
        assert!(r.tag_like_pairs > 0);
        assert_eq!(r.other_pairs, 0, "{}", r.render());
        assert!(r.first_inexact_pair.is_some());
    }

    #[test]
    fn matching_start_state_gives_absolute_matches() {
        // A stream produced with the checker's own start state matches slot for slot.
        let mut state = SyncState::new();
        let mut blocks = Vec::new();
        for k in 0..50usize {
            let b = slot(40, k as u8);
            let rec = state.time_slot(0x1F, &b.time_slot_payload().unwrap());
            blocks.push(b);
            blocks.push(Block::Checksum { csum: rec });
        }
        let r = check_blocks(&blocks);
        assert_eq!(r.absolute_matches, 50);
        assert_eq!(r.first_absolute_mismatch, None);
    }

    #[test]
    fn render_names_the_first_mismatch() {
        let r = check_blocks(&synthetic(30, 0));
        let text = r.render();
        assert!(text.contains("first mismatch: slot 1"), "{text}");
        assert!(text.contains("net checksum rotation 21"), "{text}");
    }
}
