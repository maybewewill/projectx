//! CRC rules of the replay container (CONFIRMED on a real 1.26a replay, docs/spec/replay-load.md section 1-2).
//!
//! * Header (version 1, size 0x44): the CRC-32 (zlib polynomial, init and final xor as zlib) of the whole
//!   header with its own u32 field zeroed (field at `+0x40` for version 1, `+0x3C` for version 0) must equal
//!   that field. The original reads it in `FUN_6f534750` (error `0x0D` on mismatch).
//! * Block header (8 bytes): `+4` holds the **folded** CRC of the 8-byte block header with bytes 4..7 zeroed,
//!   where fold(c) = `(c >> 16) ^ c` truncated to 16 bits. The original checks it in `FUN_6f535050` (error `0x13`).
//! * Block payload: `+6` holds the folded CRC of the compressed payload (`csize` bytes). Checked after the
//!   payload is read (error `0x19`). The field the old notes called "not compared" is this one.
//!
//! The header and block rules were checked against all 152 blocks of the real file (0 failures).

/// CRC-32 with the zlib polynomial (reflected 0xEDB88320, init 0xFFFFFFFF, final xor 0xFFFFFFFF).
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(data);
    c.sum()
}

/// The 16-bit fold the block fields use: `(c >> 16) ^ c`, low 16 bits.
pub fn fold(c: u32) -> u16 {
    ((c >> 16) ^ c) as u16
}

/// Folded CRC of an 8-byte block header whose checksum bytes (4..8) are zeroed.
pub fn block_header_check(header: &[u8; 8]) -> u16 {
    let mut h = *header;
    h[4..8].fill(0);
    fold(crc32(&h))
}

/// Folded CRC of a block's compressed payload.
pub fn block_data_check(compressed: &[u8]) -> u16 {
    fold(crc32(compressed))
}

/// CRC-32 of a replay header with its CRC field (at `field`, 4 bytes) zeroed.
pub fn header_check(header: &[u8], field: usize) -> u32 {
    let mut h = header.to_vec();
    h[field..field + 4].fill(0);
    crc32(&h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_the_zlib_polynomial() {
        // Standard check value of CRC-32/ISO-HDLC.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn fold_xors_the_halves() {
        assert_eq!(fold(0xBD65_41EB), 0xFC8E);
    }
}
