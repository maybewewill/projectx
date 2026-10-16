//! The full-engine 0x22 oracle: boots the replay's map, runs the simulation slot by slot and compares the sync
//! value computed from the simulated state with the record stored in the file
//! (`w3boot --replay FILE --sim-sync MAP`; docs/spec/lockstep.md sections 13 to 17).
//!
//! For every time slot with a nonzero increment the value is
//! `rol12(T) ^ rol12(c) ^ rol9(X) ^ rol6(Y) ^ rol3(R)` with `c` the registry fold, so the difference `D` between
//! the file record and the computed one is the XOR of the rotated differences of the inputs. The report explains
//! the first mismatch by matching `D` against those shapes:
//!
//! * `static`   `D` equals the `D` of the previous computed slot (or the first slot's): a constant word is missing,
//!   the data-table slots 5..9 and 12..19 (and the cheat word) that do not change during a game;
//! * `Y`        `rotr6(D)` is small: the birth tag counter differs;
//! * `X`        `rotr9(D)` is small: the presence tag differs;
//! * `R`        `rotr3(D)` is small: the generator state hash differs by little (a few draws cannot be small, so
//!   this class rarely fires; a different RNG state gives a random-looking word);
//! * `N`        the net checksum (never expected to fail);
//! * `rand`/`game` the dynamic registry slots 3 (all 45 streams) and 6 (the 12 players), by their rotations.
//!
//! `calibrated` mode takes the static constant from the first computed slot (`K = D_0`) and counts the slots
//! whose `D` equals `K`, i.e. where every dynamic input is right and only the constant is missing.

use std::fmt::Write as _;

use sim::lockstep::{GameSpeed, TurnScheduler};
use w3g::Block;

use crate::{replay, Engine};

/// What [`run`] reports.
#[derive(Debug, Clone, Default)]
pub struct SimSyncReport {
    pub time_slots: usize,
    /// Slots with a nonzero increment (the ones that carry a computed record).
    pub computed: usize,
    /// Records equal to the simulated value without any calibration.
    pub exact: usize,
    pub first_mismatch: Option<usize>,
    /// The first mismatch's difference `D = file ^ computed`.
    pub first_delta: u32,
    /// The input that explains the first mismatch (see the module docs).
    pub first_explained_by: String,
    /// Records equal modulo the constant `K` taken from the first computed slot.
    pub calibrated_exact: usize,
    pub calibrated_first_mismatch: Option<usize>,
    pub calibrated_delta: u32,
    pub calibrated_explained_by: String,
    /// The constant `K = D` of the first computed slot.
    pub constant: u32,
    /// Longest run of consecutive calibrated-exact computed slots from the first one.
    pub calibrated_prefix: usize,
    /// How many records carry the modal delta.
    pub constant_count: usize,
    /// Consecutive computed pairs, and those whose delta did not change (the sim's hidden changes equal the file's).
    pub pairs: usize,
    pub pair_exact: usize,
    pub pair_prefix: usize,
    pub pair_first_mismatch: Option<usize>,
    pub pair_delta: u32,
    pub pair_explained_by: String,
    /// Pair confusion matrix by whether the hidden words changed: (real quiet, sim quiet), (real quiet, sim active),
    /// (real active, sim quiet), (real active, sim active but different), (real active, sim equal).
    pub quiet_both: usize,
    pub real_quiet_sim_active: usize,
    pub real_active_sim_quiet: usize,
    pub both_active_different: usize,
    pub both_active_equal: usize,
    /// Slots where the replay stream could not be turned into commands.
    pub command_errors: Vec<String>,
    /// Per-slot trace lines (when requested).
    pub trace: Vec<String>,
    /// Final simulated words, for the report.
    pub final_x: u32,
    pub final_y: u32,
    pub final_r: u32,
    pub game_seconds: f32,
    pub notes: Vec<String>,
}

impl SimSyncReport {
    pub fn render(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "sim sync (full engine against the stored 0x22 records)");
        let _ = writeln!(
            s,
            "  time slots {}  computed {}  game time {:.1} s",
            self.time_slots, self.computed, self.game_seconds
        );
        let _ = writeln!(
            s,
            "  absolute exact: {} of {}  first mismatch: {}  delta {:#010x}  explained by: {}",
            self.exact,
            self.computed,
            self.first_mismatch.map_or("none".into(), |i| format!("slot {i}")),
            self.first_delta,
            self.first_explained_by
        );
        let _ = writeln!(
            s,
            "  calibrated (modal delta K = {:#010x}, {} records): exact {} of {}  prefix {}  first mismatch: {}  delta^K {:#010x}  explained by: {}",
            self.constant,
            self.constant_count,
            self.calibrated_exact,
            self.computed,
            self.calibrated_prefix,
            self.calibrated_first_mismatch.map_or("none".into(), |i| format!("slot {i}")),
            self.calibrated_delta,
            self.calibrated_explained_by
        );
        let _ = writeln!(
            s,
            "  pairs (no constant needed): exact {} of {}  prefix {}  first mismatching pair at slot {}  pair delta {:#010x}  explained by: {}",
            self.pair_exact,
            self.pairs,
            self.pair_prefix,
            self.pair_first_mismatch.map_or("none".into(), |i| i.to_string()),
            self.pair_delta,
            self.pair_explained_by
        );
        let _ = writeln!(
            s,
            "  hidden-change pairs: both quiet {}  real quiet/sim active {}  real active/sim quiet {}  both active equal {}  both active different {}",
            self.quiet_both,
            self.real_quiet_sim_active,
            self.real_active_sim_quiet,
            self.both_active_equal,
            self.both_active_different
        );
        let _ = writeln!(
            s,
            "  final simulated words: X {:#x}  Y {:#x}  R {:#010x}",
            self.final_x, self.final_y, self.final_r
        );
        for e in self.command_errors.iter().take(5) {
            let _ = writeln!(s, "  command error: {e}");
        }
        for n in &self.notes {
            let _ = writeln!(s, "  note: {n}");
        }
        for t in &self.trace {
            let _ = writeln!(s, "{t}");
        }
        s
    }
}

/// Which input a difference `d` looks like (see the module docs). `rand_rot` and `game_rot` are the rotations of
/// the dynamic registry slots 3 and 6.
pub fn explain(d: u32, rand_rot: u32, game_rot: u32) -> String {
    if d == 0 {
        return "none".into();
    }
    let small = |v: u32| v < (1 << 12);
    let mut parts = Vec::new();
    if small(d.rotate_right(6)) {
        parts.push(format!("Y (dY {})", d.rotate_right(6)));
    }
    if small(d.rotate_right(9)) {
        parts.push(format!("X (dX {})", d.rotate_right(9)));
    }
    if small(d.rotate_right(3)) {
        parts.push(format!("R-small ({})", d.rotate_right(3)));
    }
    if d.rotate_right(rand_rot).count_ones() <= 4 {
        parts.push("rand slot".into());
    }
    if d.rotate_right(game_rot).count_ones() <= 4 {
        parts.push("game slot".into());
    }
    if parts.is_empty() {
        "several inputs".into()
    } else {
        parts.join(" or ")
    }
}

/// Options of [`run`].
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Compute the record after the slot's commands were applied (default: before).
    pub after_commands: bool,
    /// Stop after this many slots (0: all).
    pub max_slots: usize,
    /// Print one trace line per computed slot (slot, file, computed, delta).
    pub trace: bool,
    /// A constant to XOR into every computed value (the static slots), if known.
    pub constant: u32,
    /// Do not run `main` before the first slot (default: run it, as the game does while loading).
    pub defer_main: bool,
}

/// The simulated inputs of one computed record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inputs {
    pub x: u32,
    pub y: u32,
    pub r: u32,
    pub rand: u32,
    pub game: u32,
    pub t: u32,
    pub net: u32,
    /// Bit k: stream k's state changed since the previous slot; bit 45: the global generator.
    pub changed: u64,
}

/// One computed record: slot, file value, simulated value and the simulated inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rec {
    pub slot: usize,
    pub file: u32,
    pub sim: u32,
    pub inputs: Inputs,
}

/// Runs `replay` on `engine` (booted from the same replay) and compares every computed record.
pub fn run(engine: &mut Engine, replay_file: &w3g::Replay, opts: &Options) -> SimSyncReport {
    let speed = GameSpeed::from_setting(replay_file.game.settings.flags & 3);
    let mut sched = TurnScheduler::new(speed);
    let mut report = SimSyncReport::default();
    let mut recs: Vec<Rec> = Vec::new();
    if !opts.defer_main {
        let _ = engine.begin();
    }
    // (slot, increment, predicted value, inputs) of the slot whose record is the next 0x22 block.
    let mut pending: Option<(usize, u16, u32, Inputs)> = None;
    let mut slot_index = 0usize;
    let mut prev_states: Vec<sim::rng::Rng> = Vec::new();
    for block in &replay_file.blocks {
        match block {
            Block::TimeSlot { id, increment_ms, commands } => {
                if opts.max_slots != 0 && slot_index >= opts.max_slots {
                    break;
                }
                let payload = block.time_slot_payload().unwrap_or_default();
                for c in commands {
                    for a in c.decode().unwrap_or_default() {
                        match a {
                            w3g::Action::PauseGame => engine.sync_mut().pause(),
                            w3g::Action::ResumeGame => engine.sync_mut().resume(),
                            _ => {}
                        }
                    }
                }
                // The tick loop runs the 25 ms units the sync tracker releases (it knows about pauses).
                let turn = match replay::push_time_slot(&mut sched, 0, commands) {
                    Ok(t) => t,
                    Err(e) => {
                        report.command_errors.push(format!("slot {slot_index}: {e}"));
                        sched.push_turn(0, Vec::new())
                    }
                };
                let t_now = engine.sync().clock();
                let predicted;
                if opts.after_commands {
                    engine.apply_commands(&turn.actions);
                    predicted = engine.sync_time_slot(*id, &payload);
                } else {
                    predicted = engine.sync_time_slot(*id, &payload);
                    engine.apply_commands(&turn.actions);
                }
                let changed = {
                    let streams = engine.host().rng();
                    let mut now: Vec<sim::rng::Rng> = (0..sim::rng::STREAM_COUNT).map(|k| streams.stream(k)).collect();
                    now.push(streams.global);
                    let mut m = 0u64;
                    if prev_states.len() == now.len() {
                        for (k, (a, b)) in prev_states.iter().zip(&now).enumerate() {
                            if a != b {
                                m |= 1 << k;
                            }
                        }
                    }
                    prev_states = now;
                    m
                };
                let inputs = {
                    let w = engine.sync().world;
                    let reg = engine.sync().registry_now();
                    Inputs {
                        x: w.x(),
                        y: w.y(),
                        r: engine.sync().rng_hash,
                        rand: reg[sim::sync::slot::RAND].unwrap_or(0),
                        game: reg[sim::sync::slot::GAME].unwrap_or(0),
                        t: t_now,
                        net: engine.sync().net().value(),
                        changed,
                    }
                };
                // Units released by this slot (zero while paused).
                let units = engine.sync().last_units();
                engine.step_ticks(units * speed.ticks_per_unit());
                pending = Some((slot_index, *increment_ms, predicted ^ opts.constant, inputs));
                slot_index += 1;
                report.time_slots += 1;
            }
            Block::Checksum { csum } => {
                if let Some((slot, inc, sim, inputs)) = pending.take() {
                    if inc != 0 {
                        recs.push(Rec { slot, file: *csum, sim, inputs });
                    }
                }
            }
            Block::LeaveGame { reason, player_id, result, .. } => {
                engine.sync_mut().leave(*reason, *player_id, *result);
            }
            Block::ForcedGameEnd { mode, countdown_s } => engine.sync_mut().game_end(*mode, *countdown_s),
            _ => {}
        }
    }
    let regs = engine.sync().registry_now();
    let rand_rot = sim::sync::participant_rotation(&regs, sim::sync::slot::RAND);
    let game_rot = sim::sync::participant_rotation(&regs, sim::sync::slot::GAME);
    analyse(&mut report, &recs, rand_rot, game_rot, opts.trace);
    let w = engine.sync().world;
    report.final_x = w.x();
    report.final_y = w.y();
    report.final_r = engine.sync().rng_hash;
    report.game_seconds = engine.host().clock().absolute();
    report
}

/// Fills the comparison numbers of `report` from the computed records.
pub fn analyse(report: &mut SimSyncReport, recs: &[Rec], rand_rot: u32, game_rot: u32, trace: bool) {
    report.computed = recs.len();
    let deltas: Vec<u32> = recs.iter().map(|r| r.file ^ r.sim).collect();
    for (r, &d) in recs.iter().zip(&deltas) {
        if d == 0 {
            report.exact += 1;
        } else if report.first_mismatch.is_none() {
            report.first_mismatch = Some(r.slot);
            report.first_delta = d;
            report.first_explained_by = explain(d, rand_rot, game_rot);
        }
    }
    // The static constant: the most frequent delta (robust against a wrong first slot).
    let mut counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for &d in &deltas {
        *counts.entry(d).or_default() += 1;
    }
    let (&k, &n) = counts.iter().max_by_key(|&(_, n)| *n).unwrap_or((&0, &0));
    report.constant = k;
    report.constant_count = n;
    let mut prefix_alive = true;
    for (r, &d) in recs.iter().zip(&deltas) {
        if d == k {
            report.calibrated_exact += 1;
            if prefix_alive {
                report.calibrated_prefix += 1;
            }
        } else {
            prefix_alive = false;
            if report.calibrated_first_mismatch.is_none() {
                report.calibrated_first_mismatch = Some(r.slot);
                report.calibrated_delta = d ^ k;
                report.calibrated_explained_by = explain(d ^ k, rand_rot, game_rot);
            }
        }
    }
    // Pairs: the sim's hidden changes against the file's (no constant needed).
    let hidden = |value: u32, i: &Inputs| value ^ i.t.rotate_left(12) ^ i.net.rotate_left(21);
    for w in recs.windows(2) {
        let real = hidden(w[0].file, &w[0].inputs) ^ hidden(w[1].file, &w[1].inputs);
        let simc = hidden(w[0].sim, &w[0].inputs) ^ hidden(w[1].sim, &w[1].inputs);
        match (real == 0, simc == 0) {
            (true, true) => report.quiet_both += 1,
            (true, false) => report.real_quiet_sim_active += 1,
            (false, true) => report.real_active_sim_quiet += 1,
            (false, false) => {
                if real == simc {
                    report.both_active_equal += 1;
                } else {
                    report.both_active_different += 1;
                }
            }
        }
    }
    let mut alive = true;
    for (k, w) in deltas.windows(2).enumerate() {
        report.pairs += 1;
        if w[0] == w[1] {
            report.pair_exact += 1;
            if alive {
                report.pair_prefix += 1;
            }
        } else {
            alive = false;
            if report.pair_first_mismatch.is_none() {
                report.pair_first_mismatch = Some(recs[k + 1].slot);
                report.pair_delta = w[0] ^ w[1];
                report.pair_explained_by = explain(w[0] ^ w[1], rand_rot, game_rot);
            }
        }
    }
    if trace {
        let hidden = |value: u32, i: &Inputs| value ^ i.t.rotate_left(12) ^ i.net.rotate_left(21);
        for (i, r) in recs.iter().enumerate() {
            let d = deltas[i];
            let (real_ch, sim_ch) = if i > 0 {
                let p = &recs[i - 1];
                (
                    hidden(r.file, &r.inputs) ^ hidden(p.file, &p.inputs),
                    hidden(r.sim, &r.inputs) ^ hidden(p.sim, &p.inputs),
                )
            } else {
                (0, 0)
            };
            report.trace.push(format!(
                "  slot {:5}  file {:08x} sim {:08x} delta {:08x} hfile {:08x} hsim {:08x} | real change {:08x} [{}] sim change {:08x} [{}] | X {:#x} Y {:#x} R {:08x} rand {:08x} streams {}",
                r.slot,
                r.file,
                r.sim,
                d,
                hidden(r.file, &r.inputs),
                hidden(r.sim, &r.inputs),
                real_ch,
                if real_ch != 0 { explain(real_ch, rand_rot, game_rot) } else { String::new() },
                sim_ch,
                if sim_ch != 0 { explain(sim_ch, rand_rot, game_rot) } else { String::new() },
                r.inputs.x,
                r.inputs.y,
                r.inputs.r,
                r.inputs.rand,
                (0..46).filter(|k| r.inputs.changed >> k & 1 != 0).map(|k| if k == 45 { "G".to_string() } else { k.to_string() }).collect::<Vec<_>>().join(","),
            ));
        }
    }
}
