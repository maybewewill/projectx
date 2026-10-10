//! MPQ hashing and block encryption (public format, as documented by StormLib).

use std::sync::OnceLock;

pub const HASH_TABLE_OFFSET: u32 = 0;
pub const HASH_NAME_A: u32 = 1;
pub const HASH_NAME_B: u32 = 2;
pub const HASH_FILE_KEY: u32 = 3;

fn crypt_table() -> &'static [u32; 0x500] {
    static TABLE: OnceLock<[u32; 0x500]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 0x500];
        let mut seed: u32 = 0x0010_0001;
        for i in 0..0x100 {
            for j in 0..5 {
                seed = (seed * 125 + 3) % 0x2A_AAAB;
                let hi = (seed & 0xFFFF) << 16;
                seed = (seed * 125 + 3) % 0x2A_AAAB;
                let lo = seed & 0xFFFF;
                table[i + j * 0x100] = hi | lo;
            }
        }
        table
    })
}

/// Hashes a file name (case-insensitive, `/` treated as `\`).
pub fn hash_string(name: &str, hash_type: u32) -> u32 {
    let table = crypt_table();
    let mut seed1: u32 = 0x7FED_7FED;
    let mut seed2: u32 = 0xEEEE_EEEE;
    for &b in name.as_bytes() {
        let ch = match b.to_ascii_uppercase() {
            b'/' => b'\\',
            c => c,
        } as u32;
        seed1 = table[(hash_type * 0x100 + ch) as usize] ^ seed1.wrapping_add(seed2);
        seed2 = ch
            .wrapping_add(seed1)
            .wrapping_add(seed2)
            .wrapping_add(seed2 << 5)
            .wrapping_add(3);
    }
    seed1
}

pub fn decrypt(data: &mut [u32], mut key: u32) {
    let table = crypt_table();
    let mut seed: u32 = 0xEEEE_EEEE;
    for v in data {
        seed = seed.wrapping_add(table[0x400 + (key & 0xFF) as usize]);
        let plain = *v ^ key.wrapping_add(seed);
        *v = plain;
        key = ((!key << 0x15).wrapping_add(0x1111_1111)) | (key >> 0x0B);
        seed = plain
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
    }
}

pub fn encrypt(data: &mut [u32], mut key: u32) {
    let table = crypt_table();
    let mut seed: u32 = 0xEEEE_EEEE;
    for v in data {
        seed = seed.wrapping_add(table[0x400 + (key & 0xFF) as usize]);
        let plain = *v;
        *v = plain ^ key.wrapping_add(seed);
        key = ((!key << 0x15).wrapping_add(0x1111_1111)) | (key >> 0x0B);
        seed = plain
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
    }
}

/// Decrypts a byte buffer in place; a trailing partial word is left untouched (as in the format).
pub fn decrypt_bytes(data: &mut [u8], key: u32) {
    let words = data.len() / 4;
    let mut buf: Vec<u32> = data[..words * 4]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    decrypt(&mut buf, key);
    for (dst, w) in data.chunks_exact_mut(4).zip(buf) {
        dst.copy_from_slice(&w.to_le_bytes());
    }
}

pub fn encrypt_bytes(data: &mut [u8], key: u32) {
    let words = data.len() / 4;
    let mut buf: Vec<u32> = data[..words * 4]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    encrypt(&mut buf, key);
    for (dst, w) in data.chunks_exact_mut(4).zip(buf) {
        dst.copy_from_slice(&w.to_le_bytes());
    }
}

/// Encryption key of a file: hash of the base name, optionally adjusted by its position.
pub fn file_key(name: &str, block_offset: u32, file_size: u32, fix_key: bool) -> u32 {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let key = hash_string(base, HASH_FILE_KEY);
    if fix_key {
        key.wrapping_add(block_offset) ^ file_size
    } else {
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_table_keys() {
        assert_eq!(hash_string("(hash table)", HASH_FILE_KEY), 0xC3AF_3770);
        assert_eq!(hash_string("(block table)", HASH_FILE_KEY), 0xEC83_B3A3);
    }

    #[test]
    fn hash_is_case_and_slash_insensitive() {
        assert_eq!(
            hash_string("units\\unitdata.slk", HASH_NAME_A),
            hash_string("Units/UnitData.slk", HASH_NAME_A)
        );
    }

    #[test]
    fn encrypt_roundtrip() {
        let orig: Vec<u32> = (0..64).map(|i| i * 0x0101_0101).collect();
        let mut v = orig.clone();
        encrypt(&mut v, 0x1234_5678);
        assert_ne!(v, orig);
        decrypt(&mut v, 0x1234_5678);
        assert_eq!(v, orig);
    }
}
