//! Dumps one TSV row per time slot of a replay for offline analysis of the 0x22 sync record
//! (docs/spec/lockstep.md section 16).
//!
//! Columns: slot, increment_ms, T (tick clock model), net checksum after the slot's own block (hex),
//! record (hex, `-` when the file has none directly after the slot), actions of the slot as a comma list
//! `player:id` or `player:id:ability` (ability/order id in hex for 0x10..0x15).
//!
//! Usage: `cargo run -p engine --example syncdump -- FILE.w3g > slots.tsv`

use sim::sync::{tick_clock_at_slot, SyncState};
use w3g::{Action, Block, Replay};

fn ability(a: &Action) -> Option<u32> {
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

fn main() {
    let path = std::env::args().nth(1).expect("usage: syncdump FILE.w3g");
    let bytes = std::fs::read(&path).expect("read replay");
    let replay = Replay::parse(&bytes).expect("parse replay");
    let mut state = SyncState::new();
    // (slot, inc, t, net, actions)
    let mut pending: Option<(usize, u16, u32, u32, String)> = None;
    let mut slot = 0usize;
    let flush = |p: &mut Option<(usize, u16, u32, u32, String)>, rec: Option<u32>| {
        if let Some((i, inc, t, net, acts)) = p.take() {
            let r = rec.map_or("-".to_string(), |v| format!("{v:08x}"));
            println!("{i}\t{inc}\t{t}\t{net:08x}\t{r}\t{acts}");
        }
    };
    for block in &replay.blocks {
        match block {
            Block::TimeSlot { id, commands, .. } => {
                flush(&mut pending, None);
                let payload = block.time_slot_payload().unwrap_or_default();
                let inc = u16::from_le_bytes([payload[0], payload[1]]);
                let t = tick_clock_at_slot(state.elapsed_ms());
                state.time_slot(*id, &payload);
                let mut acts = Vec::new();
                for c in commands {
                    match c.decode() {
                        Ok(v) => {
                            for a in v {
                                match ability(&a) {
                                    Some(ab) => {
                                        acts.push(format!("{}:{:02x}:{:x}", c.player_id, a.id(), ab))
                                    }
                                    None => acts.push(format!("{}:{:02x}", c.player_id, a.id())),
                                }
                            }
                        }
                        Err(_) => acts.push(format!("{}:err", c.player_id)),
                    }
                }
                pending = Some((slot, inc, t, state.net().value(), acts.join(",")));
                slot += 1;
            }
            Block::Checksum { csum } => flush(&mut pending, Some(*csum)),
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
    flush(&mut pending, None);
}
