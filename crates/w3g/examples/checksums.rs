//! Usage: cargo run -p w3g --example checksums -- path/to/replay.w3g [count|--tsv]
//!
//! Without options, prints the first `count` (default 8) 0x22 records and a summary of their lengths.
//! With `--tsv`, prints one line per time slot: `index  time_before_ms  increment_ms  next_block  csum_hex`,
//! where `next_block` is the kind of the block that follows the slot (22 when a checksum record came next).
use w3g::{Block, Replay};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: checksums <replay.w3g> [count|--tsv]");
    let opt = args.next().unwrap_or_else(|| "8".into());
    let data = std::fs::read(&path).expect("read replay");
    let r = Replay::parse(&data).unwrap_or_else(|e| panic!("{path}: {e}"));
    if opt == "--tsv" {
        let mut time = 0u64;
        let mut index = 0usize;
        let blocks = &r.blocks;
        for (i, b) in blocks.iter().enumerate() {
            if let Block::TimeSlot { increment_ms, .. } = b {
                let (next_id, csum) = match blocks.get(i + 1) {
                    Some(Block::Checksum { csum }) => ("22", format!("{csum:08x}")),
                    Some(_) => ("other", String::new()),
                    None => ("end", String::new()),
                };
                println!("{index}\t{time}\t{increment_ms}\t{next_id}\t{csum}");
                time += *increment_ms as u64;
                index += 1;
            }
        }
        return;
    }
    let count: usize = opt.parse().expect("count");
    let mut lens = std::collections::BTreeMap::<usize, usize>::new();
    let mut shown = 0;
    let mut slot_index = 0usize;
    for b in &r.blocks {
        match b {
            Block::TimeSlot { .. } => slot_index += 1,
            Block::Checksum { csum } => {
                *lens.entry(4).or_default() += 1;
                if shown < count {
                    shown += 1;
                    println!("slot {slot_index:6}  csum {csum:08x}");
                }
            }
            _ => {}
        }
    }
    println!("0x22 record lengths: {lens:?}");
}
