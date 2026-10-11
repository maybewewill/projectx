//! Usage: cargo run -p w3g --example slotdump -- path/to/replay.w3g
//!
//! One line per replayed block: `T <id> <payload hex>` for each time slot, `C <csum hex>` for each 0x22 record,
//! `L` for leave blocks and `X <id>` for the rest. Used by tools/analysis/net_checksum.py.
use w3g::{Block, Replay};

fn main() {
    let path = std::env::args().nth(1).expect("usage: slotdump <replay.w3g>");
    let data = std::fs::read(&path).expect("read replay");
    let r = Replay::parse(&data).unwrap_or_else(|e| panic!("{path}: {e}"));
    for b in &r.blocks {
        match b {
            Block::TimeSlot { id, .. } => {
                let p = b.time_slot_payload().unwrap();
                let hex: String = p.iter().map(|x| format!("{x:02x}")).collect();
                println!("T {id} {hex}");
            }
            Block::Checksum { csum } => println!("C {csum:08x}"),
            Block::LeaveGame { .. } => println!("L"),
            Block::Chat { .. } => println!("X 32"),
            Block::StartBlock { id, .. } => println!("X {id}"),
            _ => println!("X ?"),
        }
    }
}
