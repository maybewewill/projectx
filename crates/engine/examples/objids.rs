//! Prints the object ids named by the selection and order actions of a replay, with the slot index
//! (docs/spec/replay-actions.md section 10; docs/spec/lockstep.md section 18).
//!
//! `cargo run -p engine --example objids -- FILE.w3g`

use w3g::{Action, Block, Replay};

fn main() {
    let path = std::env::args().nth(1).expect("usage: objids FILE.w3g");
    let bytes = std::fs::read(&path).expect("read replay");
    let replay = Replay::parse(&bytes).expect("parse replay");
    let mut slot = 0usize;
    for block in &replay.blocks {
        if let Block::TimeSlot { commands, .. } = block {
            for c in commands {
                for a in c.decode().unwrap_or_default() {
                    match &a {
                        Action::SelectionChange { mode, units } => {
                            let ids: Vec<String> =
                                units.iter().map(|u| format!("{:08x}:{:08x}", u.id1, u.id2)).collect();
                            println!("{slot}\tp{}\tselect mode {mode}\t{}", c.player_id, ids.join(" "));
                        }
                        Action::AssignGroup { group, units } => {
                            let ids: Vec<String> =
                                units.iter().map(|u| format!("{:08x}:{:08x}", u.id1, u.id2)).collect();
                            println!("{slot}\tp{}\tgroup {group}\t{}", c.player_id, ids.join(" "));
                        }
                        Action::AbilityPointObject { header, point, target } => println!(
                            "{slot}\tp{}\tpoint-object {:x}\t({}, {})\t{:08x}:{:08x}",
                            c.player_id, header.ability, point.x, point.y, target.id1, target.id2
                        ),
                        Action::AbilityPoint { header, point } => println!(
                            "{slot}\tp{}\tpoint {:x}\t({}, {})",
                            c.player_id, header.ability, point.x, point.y
                        ),
                        Action::Ability(h) => println!("{slot}\tp{}\tability {:x}", c.player_id, h.ability),
                        other => println!("{slot}\tp{}\t{:?}", c.player_id, other.id()),
                    }
                }
            }
            slot += 1;
        }
    }
}
