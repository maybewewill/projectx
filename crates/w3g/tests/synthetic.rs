//! Builds a minimal 1.26-style replay in memory and checks that it parses back.

use std::io::Write;

use w3g::{encode_string, Block, Replay};

fn cstr(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s);
    out.push(0);
}

fn body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x110u32.to_le_bytes());
    // Host player record (custom game: 1 extra byte).
    b.extend_from_slice(&[0x00, 1]);
    cstr(&mut b, b"Host");
    b.extend_from_slice(&[1, 0]);
    cstr(&mut b, b"Test Game");
    cstr(&mut b, b"");
    let mut settings = Vec::new();
    settings.extend_from_slice(&0x0001_8000u32.to_le_bytes());
    settings.push(0);
    settings.extend_from_slice(&116u16.to_le_bytes());
    settings.extend_from_slice(&84u16.to_le_bytes());
    settings.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
    cstr(&mut settings, b"Maps\\FrozenThrone\\(2)EchoIsles.w3x");
    cstr(&mut settings, b"Host");
    settings.push(0);
    cstr(&mut b, &encode_string(&settings));
    b.extend_from_slice(&12u32.to_le_bytes()); // slot count
    b.extend_from_slice(&1u32.to_le_bytes()); // game type
    b.extend_from_slice(&0u32.to_le_bytes()); // language
                                              // Second player.
    b.extend_from_slice(&[0x16, 2]);
    cstr(&mut b, b"Guest");
    b.extend_from_slice(&[1, 0]);
    b.extend_from_slice(&0u32.to_le_bytes());
    // GameStartRecord with two 9-byte slots.
    let slots: [[u8; 9]; 2] = [
        [1, 100, 2, 0, 0, 0, 0x02, 1, 100],
        [2, 100, 2, 0, 1, 1, 0x04, 1, 100],
    ];
    b.push(0x19);
    b.extend_from_slice(&((1 + 2 * 9 + 6) as u16).to_le_bytes());
    b.push(2);
    for s in &slots {
        b.extend_from_slice(s);
    }
    b.extend_from_slice(&0x1234_5678u32.to_le_bytes()); // seed
    b.extend_from_slice(&[0, 2]);
    // Replay blocks.
    for id in [0x1A, 0x1B, 0x1C] {
        b.push(id);
        b.extend_from_slice(&1u32.to_le_bytes());
    }
    // Time slot: 100 ms, player 1 issues a 3-byte action.
    b.push(0x1F);
    b.extend_from_slice(&((2 + 1 + 2 + 3) as u16).to_le_bytes());
    b.extend_from_slice(&100u16.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&[0x10, 0x42, 0x00]);
    b.push(0x22);
    b.push(4);
    b.extend_from_slice(&0xCAFEBABEu32.to_le_bytes());
    // Empty time slot of 250 ms.
    b.push(0x1F);
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&250u16.to_le_bytes());
    // Chat to all.
    b.push(0x20);
    b.push(2);
    let msg = b"gl hf\0";
    b.extend_from_slice(&((1 + 4 + msg.len()) as u16).to_le_bytes());
    b.push(0x20);
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(msg);
    b.push(0x17);
    b.extend_from_slice(&0x0Cu32.to_le_bytes());
    b.push(2);
    b.extend_from_slice(&0x09u32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b
}

fn build_file(body: &[u8], block_size: usize) -> Vec<u8> {
    let mut blocks = Vec::new();
    let chunks: Vec<&[u8]> = body.chunks(block_size).collect();
    for chunk in &chunks {
        let mut padded = chunk.to_vec();
        padded.resize(block_size, 0);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&padded).unwrap();
        let data = z.finish().unwrap();
        blocks.extend_from_slice(&(data.len() as u16).to_le_bytes());
        blocks.extend_from_slice(&(block_size as u16).to_le_bytes());
        blocks.extend_from_slice(&0u32.to_le_bytes());
        blocks.extend_from_slice(&data);
    }
    let mut f = Vec::new();
    f.extend_from_slice(w3g::MAGIC);
    f.extend_from_slice(&0x44u32.to_le_bytes());
    f.extend_from_slice(&((0x44 + blocks.len()) as u32).to_le_bytes());
    f.extend_from_slice(&1u32.to_le_bytes());
    f.extend_from_slice(&(body.len() as u32).to_le_bytes());
    f.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    f.extend_from_slice(b"PX3W");
    f.extend_from_slice(&26u32.to_le_bytes());
    f.extend_from_slice(&6059u16.to_le_bytes());
    f.extend_from_slice(&0x8000u16.to_le_bytes());
    f.extend_from_slice(&350u32.to_le_bytes());
    f.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(f.len(), 0x44);
    f.extend_from_slice(&blocks);
    f
}

#[test]
fn parses_synthetic_replay() {
    let body = body();
    // Small block size forces the body to span several zlib blocks.
    for block_size in [64, 8192] {
        let r = Replay::parse(&build_file(&body, block_size)).unwrap();
        assert_eq!(&r.header.sub.product, &Some(*b"W3XP"));
        assert_eq!(r.header.sub.version, 26);
        assert!(r.header.is_multiplayer());
        assert_eq!(r.game.host.name, b"Host");
        assert_eq!(
            r.game.settings.map_path,
            b"Maps\\FrozenThrone\\(2)EchoIsles.w3x"
        );
        assert_eq!(r.game.settings.map_checksum, 0xDEADBEEF);
        assert_eq!(r.game.players.len(), 1);
        assert_eq!(r.game.slots.len(), 2);
        assert_eq!(r.game.slots[1].race, 0x04);
        assert_eq!(r.game.random_seed, 0x1234_5678);
        let ticks: Vec<_> = r.ticks().collect();
        assert_eq!(ticks.len(), 2);
        assert_eq!(ticks[0].0, 100);
        assert_eq!(ticks[0].1[0].actions, [0x10, 0x42, 0x00]);
        assert_eq!(ticks[1].0, 350);
        assert_eq!(
            r.blocks.iter().find_map(Block::checksum_u32),
            Some(0xCAFEBABE)
        );
        assert!(matches!(
            r.blocks.last(),
            Some(Block::LeaveGame { player_id: 2, .. })
        ));
    }
}

#[test]
fn rejects_garbage() {
    assert!(matches!(
        Replay::parse(b"not a replay at all, definitely not"),
        Err(w3g::Error::BadMagic)
    ));
}
