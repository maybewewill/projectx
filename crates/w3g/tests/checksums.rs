//! Container CRC rules (docs/spec/replay-load.md sections 1-2) and the typed 0x22 sync record.
//! The synthetic cases always run. `W3G_REPLAY=/path/file.w3g` adds checks on the user's real 1.26a file
//! (never committed): every header and block CRC passes, and the 0x22 records line up with the time slots.

use w3g::{Block, Error, Replay};

/// A GameInfo body followed by three time slots (0x1F, increment 100 ms, no commands).
fn body() -> Vec<u8> {
    let mut b = minimal_game_info();
    for _ in 0..3 {
        b.extend_from_slice(&[0x1F, 0x02, 0x00, 0x64, 0x00]);
    }
    b
}

#[test]
fn synthetic_file_with_correct_crcs_parses() {
    let file = w3g::write_file(&body(), 1024, 6059, 0x8000, 1000);
    let r = Replay::parse(&file).expect("crc-correct file parses");
    assert_eq!(r.header.sub.build, 6059);
    assert_eq!(r.ticks().count(), 3);
}

#[test]
fn header_crc_is_enforced() {
    let mut file = w3g::write_file(&body(), 1024, 6059, 0x8000, 1000);
    file[0x3A] ^= 1; // flags, covered by the header CRC
    assert!(matches!(Replay::parse(&file), Err(Error::HeaderCrc { .. })));
}

#[test]
fn block_header_crc_is_enforced() {
    let mut file = w3g::write_file(&body(), 1024, 6059, 0x8000, 1000);
    file[0x44 + 2] ^= 0x10; // decompressed size of block 0
    assert!(matches!(
        Replay::parse(&file),
        Err(Error::BlockHeaderCrc { block: 0 })
    ));
}

#[test]
fn block_payload_crc_is_enforced() {
    let mut file = w3g::write_file(&body(), 1024, 6059, 0x8000, 1000);
    let last = file.len() - 1;
    file[last] ^= 0x01; // a payload byte of the last block
    assert!(matches!(
        Replay::parse(&file),
        Err(Error::BlockDataCrc { .. })
    ));
}

#[test]
fn folded_crc_is_the_xor_of_its_halves() {
    let c = w3g::crc32(b"123456789");
    assert_eq!(w3g::fold(c), ((c >> 16) ^ c) as u16);
}

/// The typed 0x22 record exposes its u32; other blocks have no sync value.
#[test]
fn sync_record_exposes_its_u32() {
    assert_eq!(
        Block::Checksum { csum: 0x1234_5678 }.checksum_u32(),
        Some(0x1234_5678)
    );
    assert_eq!(
        Block::ForcedGameEnd {
            mode: 0,
            countdown_s: 0
        }
        .checksum_u32(),
        None
    );
}

/// Smallest body the GameInfo reader accepts: header record, empty name, empty settings, no players.
fn minimal_game_info() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x110u32.to_le_bytes());
    b.push(0x00); // host record id
    b.push(1); // player id
    b.extend_from_slice(b"host\0");
    b.extend_from_slice(&[1, 0]); // extra size and one extra byte
    b.extend_from_slice(b"game\0");
    b.push(0); // empty string (password slot)
    // Stat string: flags, zero byte, width, height, map checksum, map path, host name.
    let mut stat = vec![0u8; 13];
    stat.extend_from_slice(b"m\0h\0");
    b.extend_from_slice(&w3g::encode_string(&stat));
    b.push(0); // the stat string is NUL terminated in the file
    b.extend_from_slice(&12u32.to_le_bytes()); // slot count
    b.extend_from_slice(&0u32.to_le_bytes()); // game type
    b.extend_from_slice(&0u32.to_le_bytes()); // language
    // 0x19 GameStartRecord: count, one 9-byte slot, seed, select mode, start spots.
    let slot = [1u8, 100, 2, 0, 0, 0, 0x04, 1, 100];
    b.push(0x19);
    b.extend_from_slice(&16u16.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&slot);
    b.extend_from_slice(&0x1234u32.to_le_bytes());
    b.push(0);
    b.push(1);
    b
}

#[test]
fn minimal_game_info_container_parses() {
    let file = w3g::write_file(&minimal_game_info(), 4096, 6059, 0x8000, 0);
    let r = Replay::parse(&file).expect("minimal game info parses");
    assert_eq!(r.game.slot_count, 12);
    assert!(r.blocks.is_empty());
}

fn real_replay() -> Option<Vec<u8>> {
    let path = std::env::var("W3G_REPLAY").ok()?;
    Some(std::fs::read(path).expect("read W3G_REPLAY"))
}

#[test]
fn real_replay_passes_every_crc_and_sync_records_follow_their_slots() {
    let Some(bytes) = real_replay() else {
        return;
    };
    // Replay::parse fails on any header or block CRC mismatch, so reaching this point checks them all.
    let r = Replay::parse(&bytes).expect("real replay passes every CRC");
    let mut slots = 0usize;
    let mut syncs = 0usize;
    let mut prev_was_slot = false;
    for b in &r.blocks {
        match b {
            Block::TimeSlot { .. } => {
                slots += 1;
                prev_was_slot = true;
                continue;
            }
            Block::Checksum { .. } => {
                syncs += 1;
                assert!(prev_was_slot, "0x22 record must follow a time slot");
            }
            _ => {}
        }
        prev_was_slot = false;
    }
    eprintln!("real replay: {slots} time slots, {syncs} sync records");
    // One record per slot in the first real file (89,259 of 89,259). The DotA 6.83se file (142,616 records for
    // 142,647 slots) lacks the records of 31 slots, all of them in the last 100 slots (the game end), so a few
    // missing records at the tail are allowed.
    assert!(syncs <= slots && slots - syncs <= 64, "slots {slots} records {syncs}");
    assert!(slots > 0);
}
