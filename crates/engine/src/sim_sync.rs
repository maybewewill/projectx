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
    /// Event timelines by class (see [`EventClass`]): real events found in the file against the simulated inputs
    /// that changed, matched within one slot. One row per class.
    pub timeline: Vec<TimelineRow>,
    /// Slots where the replay stream could not be turned into commands.
    pub command_errors: Vec<String>,
    /// Orders given to units, refused by the dispatcher/validator, and object references that resolved to nothing
    /// (selections and orders together).
    pub orders_accepted: usize,
    pub orders_refused: usize,
    pub refs_unresolved: usize,
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
        let _ = writeln!(s, "  event timelines (real class vs the simulated input, match = same or adjacent slot):");
        for t in &self.timeline {
            let _ = writeln!(
                s,
                "    {:<15} real {:5}  sim {:5}  real matched {:5} (chance {:6.1})  sim matched {:5}",
                t.class, t.real, t.sim, t.real_matched, t.chance, t.sim_matched
            );
        }
        let _ = writeln!(
            s,
            "  final simulated words: X {:#x}  Y {:#x}  R {:#010x}",
            self.final_x, self.final_y, self.final_r
        );
        let _ = writeln!(
            s,
            "  replay commands: orders accepted {}  refused {}  object references unresolved {}",
            self.orders_accepted, self.orders_refused, self.refs_unresolved
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

/// The kind of hidden-word change between two consecutive records, read off the rotation signatures of the inputs
/// (docs/spec/lockstep.md section 18). The classes are exclusive and tried in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClass {
    /// `rol3` of a generator-hash difference: the low three bits are zero and the value is below `2^29` with at
    /// least 24 bits (hash differences are below `2^26`). Generator draws, alone or with a small tag change.
    Generator,
    /// Presence and birth tags: `rol6(dY) ^ rol9(dX)` with 12-bit differences needs at most 21 bits.
    Tags,
    /// The rand slot: the top byte is the low byte of the hash-sum difference and bits 20 to 23 are zero.
    RandSlot,
    /// Anything else: the game-hash slot, a participant that appears, several inputs at once.
    Other,
}

impl EventClass {
    pub const ALL: [EventClass; 4] = [
        EventClass::Generator,
        EventClass::Tags,
        EventClass::RandSlot,
        EventClass::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EventClass::Generator => "generator R",
            EventClass::Tags => "tags X/Y",
            EventClass::RandSlot => "rand slot",
            EventClass::Other => "other",
        }
    }
}

/// Classifies a nonzero hidden-word change (`None` for zero).
pub fn classify(r: u32) -> Option<EventClass> {
    if r == 0 {
        return None;
    }
    let bits = 32 - r.leading_zeros();
    Some(if r & 7 == 0 && r < (1 << 29) && bits >= 24 {
        EventClass::Generator
    } else if bits <= 22 {
        EventClass::Tags
    } else if bits >= 30 && (r >> 20) & 0xF == 0 {
        EventClass::RandSlot
    } else {
        EventClass::Other
    })
}

/// One row of the event timeline comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRow {
    pub class: &'static str,
    /// Events of this class in the file.
    pub real: usize,
    /// Simulated events of the matching input (the generator for R, `Y`, `X`, the rand sum; none for "other").
    pub sim: usize,
    /// Real events with a simulated event of the matching input in the same slot or the neighbouring ones.
    pub real_matched: usize,
    /// Simulated events with a real event of this class nearby.
    pub sim_matched: usize,
    /// Real events one would expect to match by chance if the simulated events were spread at random over the same
    /// number of slots (so a match count near this value means no information).
    pub chance: f64,
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
    /// Register the replay's object ids of the human's start units ([`register_start_units`]).
    pub register_units: bool,
    /// Do not run `main` before the first slot (default: run it, as the game does while loading).
    pub defer_main: bool,
    /// Generator hash -> number of draws since the game-start seed (for the trace; empty: unknown).
    pub draw_index: std::collections::HashMap<u32, u32>,
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
    /// Draws the global generator has made since the seed (`u32::MAX`: not known).
    pub n: u32,
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

/// Registers the replay's object pairs of one player's start units (docs/spec/lockstep.md section 18.3).
///
/// The replay names units by `(id, id)` pairs; the original's ids come from a counter that every agent (units, their
/// abilities, trees, doodads) draws from, around 15,000 at the start units of EchoIsles. The simulation does not
/// allocate such ids, so without help no order of the replay reaches a unit. The heuristic used by the oracle (not
/// the original's rule): the player's start units are the units the simulation created for `sim_player`, in
/// creation order; the ids the player's selections name, in ascending order, are those units in the same order, the
/// first id being the first unit and the following ones placed by their distance in multiples of the smallest gap
/// (on EchoIsles the hall is 14908 and the peasants 14961, 14983, 15005, 15027, 15049: 22 agents per peasant).
/// Returns a note per registered pair.
pub fn register_start_units(engine: &mut Engine, replay: &w3g::Replay, replay_pid: u8, sim_player: u32) -> Vec<String> {
    let mut ids: Vec<u32> = Vec::new();
    for block in &replay.blocks {
        if let Block::TimeSlot { commands, .. } = block {
            for c in commands.iter().filter(|c| c.player_id == replay_pid) {
                for a in c.decode().unwrap_or_default() {
                    let units = match &a {
                        w3g::Action::SelectionChange { units, .. } => units.as_slice(),
                        w3g::Action::AssignGroup { units, .. } => units.as_slice(),
                        _ => continue,
                    };
                    for u in units {
                        if u.id1 == u.id2 && !ids.contains(&u.id1) {
                            ids.push(u.id1);
                        }
                    }
                }
            }
        }
    }
    ids.sort_unstable();
    let units = engine.host().game().owned_units(sim_player);
    let mut notes = Vec::new();
    if ids.is_empty() || units.is_empty() {
        return notes;
    }
    // Index of each id among the units: the first id is unit 0; the others by multiples of the smallest gap
    // between the ids after the first (one unit when there is a single other id).
    let rest = &ids[1..];
    let gap = rest.windows(2).map(|w| w[1] - w[0]).filter(|&g| g > 0).min().unwrap_or(1);
    let mut chosen: Vec<(u32, usize)> = vec![(ids[0], 0)];
    for &id in rest {
        let idx = 1 + ((id - rest[0] + gap / 2) / gap) as usize;
        chosen.push((id, idx));
    }
    for (id, idx) in chosen {
        if let Some(&k) = units.get(idx) {
            engine
                .commands_mut()
                .register_object(sim::commands::ObjectRef { id1: id, id2: id }, k);
            let ty = engine.host().game().unit(k).type_id;
            notes.push(format!("object {id:#x} -> unit {idx} of player {sim_player} (type {ty:#010x})"));
        }
    }
    notes
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
    if opts.register_units {
        // The local human: the first non-computer slot of the replay (replay player id of that slot).
        let human = replay_file
            .game
            .slots
            .iter()
            .enumerate()
            .find(|(_, sl)| sl.status == crate::replay_setup::SLOT_USED && sl.computer == 0)
            .map(|(n, sl)| (sl.player_id, n as u32));
        if let Some((pid, player)) = human {
            report.notes.extend(register_start_units(engine, replay_file, pid, player));
        }
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
                let outcomes;
                if opts.after_commands {
                    outcomes = engine.apply_commands(&turn.actions);
                    predicted = engine.sync_time_slot(*id, &payload);
                } else {
                    predicted = engine.sync_time_slot(*id, &payload);
                    outcomes = engine.apply_commands(&turn.actions);
                }
                for (o, (_, cmd)) in outcomes.iter().zip(&turn.actions) {
                    if let sim::commands::Outcome::Ordered { refused, accepted, .. } = o {
                        if *refused > 0 && report.notes.len() < 40 {
                            report
                                .notes
                                .push(format!("slot {slot_index}: order refused by {refused} (accepted {accepted}): {cmd:?}"));
                        }
                    }
                    match o {
                        sim::commands::Outcome::Selected { unresolved, .. }
                        | sim::commands::Outcome::Grouped { unresolved, .. } => report.refs_unresolved += unresolved,
                        sim::commands::Outcome::Ordered { accepted, refused, unresolved } => {
                            report.orders_accepted += accepted;
                            report.orders_refused += refused;
                            report.refs_unresolved += unresolved;
                        }
                        _ => {}
                    }
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
                        n: opts.draw_index.get(&engine.sync().rng_hash).copied().unwrap_or(u32::MAX),
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
    // Event timelines: the real change class of each pair against the simulated input that changed.
    {
        let hidden = |value: u32, i: &Inputs| value ^ i.t.rotate_left(12) ^ i.net.rotate_left(21);
        let n = recs.len();
        let mut real_at: Vec<Option<EventClass>> = vec![None; n];
        let mut sim_at: Vec<[bool; 3]> = vec![[false; 3]; n];
        for i in 1..n {
            let (p, q) = (&recs[i - 1], &recs[i]);
            real_at[i] = classify(hidden(p.file, &p.inputs) ^ hidden(q.file, &q.inputs));
            sim_at[i] = [
                p.inputs.r != q.inputs.r,
                p.inputs.y != q.inputs.y || p.inputs.x != q.inputs.x,
                p.inputs.rand != q.inputs.rand,
            ];
        }
        let near = |f: &dyn Fn(usize) -> bool, i: usize| (i.saturating_sub(1)..=(i + 1).min(n - 1)).any(f);
        for (ci, class) in EventClass::ALL.iter().enumerate() {
            let real = real_at.iter().filter(|c| **c == Some(*class)).count();
            let (sim, real_matched, sim_matched) = if ci < 3 {
                let sim = sim_at.iter().filter(|s| s[ci]).count();
                let rm = (1..n)
                    .filter(|&i| real_at[i] == Some(*class) && near(&|j| sim_at[j][ci], i))
                    .count();
                let sm = (1..n)
                    .filter(|&i| sim_at[i][ci] && near(&|j| real_at[j] == Some(*class), i))
                    .count();
                (sim, rm, sm)
            } else {
                (0, 0, 0)
            };
            let p = sim as f64 / n.max(1) as f64;
            let chance = real as f64 * (1.0 - (1.0 - p).powi(3));
            report.timeline.push(TimelineRow { class: class.name(), real, sim, real_matched, sim_matched, chance });
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
                "  slot {:5}  file {:08x} sim {:08x} delta {:08x} hfile {:08x} hsim {:08x} | real change {:08x} [{}] sim change {:08x} [{}] | X {:#x} Y {:#x} R {:08x} n {} rand {:08x} streams {}",
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
                r.inputs.n as i64,
                r.inputs.rand,
                (0..46).filter(|k| r.inputs.changed >> k & 1 != 0).map(|k| if k == 45 { "G".to_string() } else { k.to_string() }).collect::<Vec<_>>().join(","),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_reads_the_rotation_signatures() {
        // rol3 of a 26-bit hash difference: low three bits zero, between 2^23 and 2^29
        assert_eq!(classify(0x07de_1e40), Some(EventClass::Generator));
        // rol6 of a birth-tag difference and rol9 of a presence-tag difference are small
        assert_eq!(classify(9 << 6), Some(EventClass::Tags));
        assert_eq!(classify(1470 << 9), Some(EventClass::Tags));
        // bits 20..23 clear and a full-width top byte: the rand sum
        assert_eq!(classify(0xAB0F_FFFF), Some(EventClass::RandSlot));
        // random-looking words with the middle nibble set are something else
        assert_eq!(classify(0xA84F_91F9), Some(EventClass::Other));
        assert_eq!(classify(0), None);
    }

    #[test]
    fn explain_names_small_fields() {
        assert!(explain(5 << 6, 24, 27).contains("Y (dY 5)"));
        assert!(explain(0xA84F_91F9, 24, 27).contains("several inputs"));
        assert_eq!(explain(0, 24, 27), "none");
    }

    fn rec(slot: usize, file: u32, sim: u32) -> Rec {
        Rec { slot, file, sim, inputs: Inputs::default() }
    }

    #[test]
    fn analyse_counts_absolute_modal_and_pair_matches() {
        // sim = file ^ 0x11 everywhere except one record: a constant offset, one different slot.
        let recs = vec![
            rec(0, 0x100, 0x100 ^ 0x11),
            rec(1, 0x200, 0x200 ^ 0x11),
            rec(2, 0x300, 0x300 ^ 0x11 ^ 0x4),
            rec(3, 0x400, 0x400 ^ 0x11),
        ];
        let mut r = SimSyncReport::default();
        analyse(&mut r, &recs, 24, 27, false);
        assert_eq!(r.computed, 4);
        assert_eq!(r.exact, 0);
        assert_eq!(r.first_mismatch, Some(0));
        assert_eq!(r.constant, 0x11);
        assert_eq!(r.calibrated_exact, 3);
        assert_eq!(r.calibrated_prefix, 2);
        assert_eq!(r.calibrated_first_mismatch, Some(2));
        // pairs (0,1) agree; (1,2) and (2,3) each carry the one wrong record
        assert_eq!((r.pairs, r.pair_exact, r.pair_prefix), (3, 1, 1));
        assert_eq!(r.pair_first_mismatch, Some(2));
    }
}
