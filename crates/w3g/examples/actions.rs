//! Decodes every player action of a replay and prints a histogram by action id
//! (or the first decode error). Usage: `cargo run -p w3g --example actions -- file.w3g`
use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: actions <file.w3g>");
    let bytes = std::fs::read(&path).expect("read replay");
    let replay = w3g::Replay::parse(&bytes).expect("parse replay");
    let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
    let (mut blocks, mut errors) = (0usize, 0usize);
    for (t, cmds) in replay.ticks() {
        for c in cmds {
            blocks += 1;
            match c.decode() {
                Ok(actions) => {
                    for a in actions {
                        *counts.entry(a.id()).or_default() += 1;
                    }
                }
                Err(e) => {
                    errors += 1;
                    if errors <= 5 {
                        println!("t={t} ms player {}: {e}", c.player_id);
                    }
                }
            }
        }
    }
    println!("command blocks {blocks}, failed {errors}");
    for (id, n) in counts {
        println!("  {id:#04x}: {n}");
    }
}
