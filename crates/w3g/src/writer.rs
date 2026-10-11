//! Writes a version-1 replay container around a decompressed body, with the CRC fields the original
//! checks (see `checksum.rs`). Used by the synthetic test fixtures; the output is never a real game file.

use crate::checksum::{block_data_check, block_header_check, header_check};
use crate::MAGIC;
use std::io::Write;

/// Builds a `.w3g` file (header version 1, `product` "W3XP") whose body is `body`, split into blocks of
/// `block_size` decompressed bytes (the last block is zero-padded, as the game writes it).
pub fn write_file(body: &[u8], block_size: usize, build: u16, flags: u16, length_ms: u32) -> Vec<u8> {
    let block_size = block_size.max(1);
    let chunks: Vec<&[u8]> = if body.is_empty() {
        vec![&[][..]]
    } else {
        body.chunks(block_size).collect()
    };
    let mut blocks = Vec::new();
    for chunk in &chunks {
        let mut padded = chunk.to_vec();
        padded.resize(block_size, 0);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&padded).expect("in-memory write");
        let data = z.finish().expect("in-memory finish");
        let mut head = [0u8; 8];
        head[0..2].copy_from_slice(&(data.len() as u16).to_le_bytes());
        head[2..4].copy_from_slice(&(block_size as u16).to_le_bytes());
        let header_check = block_header_check(&head);
        head[4..6].copy_from_slice(&header_check.to_le_bytes());
        head[6..8].copy_from_slice(&block_data_check(&data).to_le_bytes());
        blocks.extend_from_slice(&head);
        blocks.extend_from_slice(&data);
    }
    let header_size = 0x44usize;
    let total = header_size + blocks.len();
    let mut header = Vec::with_capacity(header_size);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&(header_size as u32).to_le_bytes());
    header.extend_from_slice(&(total as u32).to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&(body.len() as u32).to_le_bytes());
    header.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    header.extend_from_slice(b"PX3W"); // "W3XP" stored reversed
    header.extend_from_slice(&26u32.to_le_bytes());
    header.extend_from_slice(&build.to_le_bytes());
    header.extend_from_slice(&flags.to_le_bytes());
    header.extend_from_slice(&length_ms.to_le_bytes());
    header.extend_from_slice(&[0u8; 4]);
    let crc = header_check(&header, 0x40);
    header[0x40..0x44].copy_from_slice(&crc.to_le_bytes());
    let mut f = header;
    f.extend_from_slice(&blocks);
    f
}
