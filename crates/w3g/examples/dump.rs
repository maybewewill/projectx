//! Usage: cargo run -p w3g --example dump -- path/to/replay.w3g
use w3g::{Block, Replay};

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <replay.w3g>");
    let data = std::fs::read(&path).expect("read replay");
    let r = Replay::parse(&data).unwrap_or_else(|e| panic!("{path}: {e}"));
    let s = &r.header.sub;
    println!(
        "version 1.{} build {} length {} ms",
        s.version, s.build, s.length_ms
    );
    println!("map {}", String::from_utf8_lossy(&r.game.settings.map_path));
    println!("seed {:#010x}", r.game.random_seed);
    for slot in r.game.slots.iter().filter(|s| s.status == 2) {
        println!(
            "  slot pid={} team={} color={} race={:#04x}",
            slot.player_id, slot.team, slot.color, slot.race
        );
    }
    let ticks = r.ticks().count();
    let checksums = r
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Checksum { .. }))
        .count();
    let actions: usize = r.ticks().map(|(_, c)| c.len()).sum();
    println!(
        "{} blocks, {ticks} ticks, {actions} command blocks, {checksums} checksum blocks",
        r.blocks.len()
    );
}
