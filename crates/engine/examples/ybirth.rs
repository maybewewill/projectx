//! Compares the birth-tag steps the 0x22 sync records imply with what the movement-object model predicts from the
//! replay's own order stream (docs/spec/lockstep.md section 17.6).
//!
//! For every pair of consecutive computed records whose residual (clock and net checksum removed) is a pure
//! birth-tag change, the feasible step set is `{d : exists y, y ^ (y + d) = rotr6(residual)}`. The model predicts
//! `3 * n` births for each movement-type order issued in the window to a selection of `n` units (a mover start is
//! three registrations, section 17.2). Everything the replay stream does not show (creeps, AI, missiles, group
//! members out of the selection) is the unknown rest; the report therefore shows the relation, not an equality.
//!
//! Usage: `cargo run -p engine --example ybirth -- FILE.w3g`

use std::collections::{BTreeMap, HashMap};

use sim::sync::{participant_rotation, residual, slot, tick_clock_at_slot, RecordPoint, SyncState};
use w3g::{Action, Block, ObjectId, Replay};

const MAXD: u32 = 1500;
const MOVE_IDS: [u32; 5] = [0xD0012, 0xD0003, 0xD000F, 0xD0014, 0xD0016];

fn order_id(a: &Action) -> Option<u32> {
    match a {
        Action::Ability(h) => Some(h.ability),
        Action::AbilityPoint { header, .. } => Some(header.ability),
        Action::AbilityPointObject { header, .. } => Some(header.ability),
        Action::AbilityPointTwoObjects { header, .. } => Some(header.ability),
        Action::AbilityPointObjectPoint(b) => Some(b.header.ability),
        Action::AbilityPointObjectPointEx { base, .. } => Some(base.header.ability),
        _ => None,
    }
}

/// Smallest and the set (up to `max`) of steps d with `y ^ (y + d) == rr` for some y below 2^17.
fn feasible(rr: u32, max: u32, cache: &mut HashMap<u32, Vec<u32>>) -> Vec<u32> {
    if let Some(v) = cache.get(&rr) {
        return v.clone();
    }
    let mut set = std::collections::BTreeSet::new();
    for y in 0u32..(1 << 19) {
        let z = y ^ rr;
        if z > y && z - y <= max {
            set.insert(z - y);
        }
    }
    let v: Vec<u32> = set.into_iter().collect();
    cache.insert(rr, v.clone());
    v
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: ybirth FILE.w3g");
    let replay = Replay::parse(&std::fs::read(&path).expect("read")).expect("parse");
    let mut state = SyncState::new();
    let rot = participant_rotation(&state.registry_now(), slot::NET);
    // selections and groups per player
    let mut sel: BTreeMap<u8, Vec<ObjectId>> = BTreeMap::new();
    let mut groups: BTreeMap<(u8, u8), Vec<ObjectId>> = BTreeMap::new();
    // per computed record: (point, predicted births of the orders issued in the slot, order count)
    let mut pts: Vec<(RecordPoint, u32, u32, u8)> = Vec::new();
    let mut pending: Option<(RecordPoint, u16, u32, u32, u8)> = None;
    for block in &replay.blocks {
        match block {
            Block::TimeSlot { id, commands, .. } => {
                let payload = block.time_slot_payload().unwrap_or_default();
                let inc = u16::from_le_bytes([payload[0], payload[1]]);
                let t = tick_clock_at_slot(state.elapsed_ms());
                state.time_slot(*id, &payload);
                let mut births = 0u32;
                let mut orders = 0u32;
                // bit 0: selection / group action, bit 1: movement order, bit 2: other order or ability, bit 3: anything else
                let mut flags = 0u8;
                for c in commands {
                    let Ok(acts) = c.decode() else { continue };
                    for a in acts {
                        match &a {
                            Action::SelectionChange { .. }
                            | Action::AssignGroup { .. }
                            | Action::SelectGroup { .. }
                            | Action::SelectSubgroup { .. } => flags |= 1,
                            _ if order_id(&a).is_some() => {}
                            _ => flags |= 8,
                        }
                        match &a {
                            Action::SelectionChange { mode, units } => {
                                let s = sel.entry(c.player_id).or_default();
                                if *mode == 1 {
                                    for u in units {
                                        if !s.contains(u) {
                                            s.push(*u);
                                        }
                                    }
                                } else {
                                    s.retain(|x| !units.contains(x));
                                }
                            }
                            Action::AssignGroup { group, units } => {
                                groups.insert((c.player_id, *group), units.clone());
                            }
                            Action::SelectGroup { group, .. } => {
                                let g = groups.get(&(c.player_id, *group)).cloned().unwrap_or_default();
                                sel.insert(c.player_id, g);
                            }
                            Action::SelectSubgroup { unit, .. } => {
                                sel.insert(c.player_id, vec![*unit]);
                            }
                            _ => {}
                        }
                        if let Some(o) = order_id(&a) {
                            flags |= if MOVE_IDS.contains(&o) { 2 } else { 4 };
                            if MOVE_IDS.contains(&o) {
                                let n = sel.get(&c.player_id).map_or(0, Vec::len) as u32;
                                births += 3 * n;
                                orders += 1;
                            }
                        }
                    }
                }
                pending = Some((
                    RecordPoint {
                        value: 0,
                        clock: t,
                        net: state.net().value(),
                    },
                    inc,
                    births,
                    orders,
                    flags,
                ));
            }
            Block::Checksum { csum } => {
                if let Some((mut p, inc, b, o, fl)) = pending.take() {
                    if inc != 0 {
                        p.value = *csum;
                        pts.push((p, b, o, fl));
                    }
                }
            }
            Block::LeaveGame {
                reason,
                player_id,
                result,
                ..
            } => state.leave(*reason, *player_id, *result),
            Block::ForcedGameEnd { mode, countdown_s } => state.game_end(*mode, *countdown_s),
            _ => {}
        }
    }
    let mut cache = HashMap::new();
    // buckets of predicted order births: 0, 1-3, 4-6, 7-12, 13+
    let names = ["0", "1-3", "4-6", "7-12", "13+"];
    let mut sum = [0u64; 5];
    let mut cnt = [0u64; 5];
    let mut hit_exact = 0;
    let mut hit_with_base = 0;
    let (mut tag_pairs, mut feasible_small) = (0, 0);
    // idle baseline: the median of the minimal step over windows with no movement order
    let mut idle: Vec<u32> = Vec::new();
    let mut rows: Vec<(u32, u32, Vec<u32>)> = Vec::new();
    let mut hits: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
    let mut class_n: BTreeMap<u8, u32> = BTreeMap::new();
    let mut by_flag: BTreeMap<u8, Vec<u32>> = BTreeMap::new();
    let mut after: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (wi, w) in pts.windows(2).enumerate() {
        let r = residual(&w[0].0, &w[1].0, rot);
        if r == 0 || r.rotate_right(6) >= (1 << 16) {
            continue;
        }
        tag_pairs += 1;
        let f = feasible(r.rotate_right(6), MAXD, &mut cache);
        if f.is_empty() {
            continue;
        }
        feasible_small += 1;
        let predicted = w[1].1;
        if predicted == 0 && w[1].2 == 0 {
            idle.push(f[0]);
        }
        by_flag.entry(w[1].3).or_default().push(f[0]);
        if w[1].3 == 0 {
            // movement orders issued in the 20 records before this one (about 2 s of game time at 100 ms per record pair)
            let lo = (wi + 1).saturating_sub(20);
            let recent: u32 = pts[lo..=wi].iter().map(|p| p.2).sum();
            after.entry(recent.min(8)).or_default().push(f[0]);
        }
        // class: 0 no action, 1 exactly one movement order to one selected unit, 2 exactly one movement order to 2..12
        let class = if w[1].3 == 0 {
            0
        } else if w[1].3 == 2 && w[1].2 == 1 && w[1].1 == 3 {
            1
        } else if w[1].3 == 2 && w[1].2 == 1 {
            2
        } else {
            255
        };
        if class != 255 {
            *class_n.entry(class).or_default() += 1;
            let h = hits.entry(class).or_insert_with(|| vec![0; MAXD as usize + 1]);
            for &d in &f {
                h[d as usize] += 1;
            }
        }
        rows.push((predicted, w[1].2, f));
    }
    idle.sort_unstable();
    let baseline = idle.get(idle.len() / 2).copied().unwrap_or(0);
    for (predicted, _orders, f) in &rows {
        let b = match predicted {
            0 => 0,
            1..=3 => 1,
            4..=6 => 2,
            7..=12 => 3,
            _ => 4,
        };
        sum[b] += f[0] as u64;
        cnt[b] += 1;
        if f.contains(predicted) && *predicted != 0 {
            hit_exact += 1;
        }
        if f.contains(&(predicted + baseline)) && *predicted != 0 {
            hit_with_base += 1;
        }
    }
    println!(
        "computed records {}  tag-like pairs {}  with a feasible step <= 400: {}",
        pts.len(),
        tag_pairs,
        feasible_small
    );
    println!("idle baseline (median minimal step, windows without a movement order): {baseline}");
    println!("predicted order births (3 * selected units per movement order) vs mean minimal step:");
    for i in 0..5 {
        if cnt[i] > 0 {
            println!("  predicted {:>5}: {:>6} pairs, mean minimal step {:.1}", names[i], cnt[i], sum[i] as f64 / cnt[i] as f64);
        }
    }
    println!("minimal step by what the slot's players did (1 selection, 2 movement order, 4 other order, 8 other action):");
    for (fl, mut v) in by_flag {
        v.sort_unstable();
        println!(
            "  flags {:>2}: {:>5} pairs  median {:>4}  mean {:.1}",
            fl,
            v.len(),
            v[v.len() / 2],
            v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64
        );
    }
    println!("idle windows (no action) by movement orders in the 20 records before:");
    for (k, mut v) in after {
        v.sort_unstable();
        println!(
            "  {:>2}{}: {:>5} pairs  median {:>4}  mean {:.1}",
            k,
            if k == 8 { "+" } else { " " },
            v.len(),
            v[v.len() / 2],
            v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64
        );
    }
    for (class, h) in &hits {
        let n = class_n[class];
        let mut idx: Vec<usize> = (1..=MAXD as usize).collect();
        idx.sort_by_key(|&d| std::cmp::Reverse(h[d]));
        let top: Vec<String> = idx.iter().take(12).map(|&d| format!("{d}:{:.0}%", 100.0 * h[d] as f64 / n as f64)).collect();
        println!("  class {class} ({n} pairs) most common feasible steps: {}", top.join(" "));
        // hit rate profile in bins of 50
        let mut prof = String::new();
        for b in 0..(MAXD as usize / 50) {
            let t: u64 = (b * 50 + 1..=(b + 1) * 50).map(|d| h[d] as u64).sum();
            prof.push_str(&format!(" {}-{}:{:.1}", b * 50 + 1, (b + 1) * 50, t as f64 / (50.0 * n as f64) * 100.0));
        }
        println!("    mean hit rate per step, bins of 50:{prof}");
    }
    println!("pairs with predicted > 0 whose feasible set contains exactly the predicted step: {hit_exact}");
    println!("... contains predicted + idle baseline: {hit_with_base}");
}
