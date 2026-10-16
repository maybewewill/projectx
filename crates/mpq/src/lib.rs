//! Read-only MPQ archive reader (format v1, as used by Warcraft III).
//!
//! Supports hash/block tables, file and sector encryption (incl. FIX_KEY), single-unit and
//! sectored files, zlib / bzip2 / PKWARE DCL compression, and the adaptive Huffman and Blizzard
//! ADPCM codecs used by WAV files. Mask bits can be combined; see `decode_sector`.

mod adpcm;
mod bytes;
pub use bytes::FileBytes;
pub mod crypto;
mod huffman;

/// Compression mask bits (first byte of a compressed sector).
const MASK_HUFFMAN: u8 = 0x01;
const MASK_ZLIB: u8 = 0x02;
const MASK_DCL: u8 = 0x08;
const MASK_BZIP2: u8 = 0x10;
const MASK_ADPCM_MONO: u8 = 0x40;
const MASK_ADPCM_STEREO: u8 = 0x80;

use crypto::{
    decrypt_bytes, file_key, hash_string, HASH_FILE_KEY, HASH_NAME_A, HASH_NAME_B,
    HASH_TABLE_OFFSET,
};

pub const FLAG_IMPLODE: u32 = 0x0000_0100;
pub const FLAG_COMPRESS: u32 = 0x0000_0200;
pub const FLAG_ENCRYPTED: u32 = 0x0001_0000;
pub const FLAG_FIX_KEY: u32 = 0x0002_0000;
pub const FLAG_SINGLE_UNIT: u32 = 0x0100_0000;
pub const FLAG_SECTOR_CRC: u32 = 0x0400_0000;
pub const FLAG_EXISTS: u32 = 0x8000_0000;

const HASH_ENTRY_EMPTY: u32 = 0xFFFF_FFFF;
const HASH_ENTRY_DELETED: u32 = 0xFFFF_FFFE;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no MPQ header found")]
    NoHeader,
    #[error("archive truncated: need {needed} bytes at {offset:#x}")]
    Truncated { offset: usize, needed: usize },
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("file {0} is encrypted and its name is required to derive the key")]
    NeedName(usize),
    #[error("unsupported compression mask {0:#04x}")]
    UnsupportedCompression(u8),
    #[error("decompression failed: {0}")]
    Decompress(String),
    #[error("decompressed size mismatch: expected {expected}, got {got}")]
    SizeMismatch { expected: usize, got: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashEntry {
    pub name_a: u32,
    pub name_b: u32,
    pub locale: u16,
    pub platform: u16,
    pub block_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEntry {
    /// Offset relative to the archive start.
    pub offset: u32,
    pub compressed_size: u32,
    pub file_size: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub header_size: u32,
    pub archive_size: u32,
    pub format_version: u16,
    pub sector_size_shift: u16,
    pub hash_table_pos: u32,
    pub block_table_pos: u32,
    pub hash_table_entries: u32,
    pub block_table_entries: u32,
}

pub struct Archive<'a> {
    /// Bytes starting at the MPQ header.
    data: &'a [u8],
    pub header: Header,
    pub hash_table: Vec<HashEntry>,
    pub block_table: Vec<BlockEntry>,
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn slice(d: &[u8], offset: usize, len: usize) -> Result<&[u8], Error> {
    d.get(
        offset..offset.checked_add(len).ok_or(Error::Truncated {
            offset,
            needed: len,
        })?,
    )
    .ok_or(Error::Truncated {
        offset,
        needed: len,
    })
}

fn read_table(data: &[u8], pos: u32, entries: u32, key_name: &str) -> Result<Vec<u8>, Error> {
    let mut buf = slice(data, pos as usize, entries as usize * 16)?.to_vec();
    decrypt_bytes(&mut buf, hash_string(key_name, HASH_FILE_KEY));
    Ok(buf)
}

impl<'a> Archive<'a> {
    /// Finds the MPQ header (on a 512-byte boundary, e.g. after a map's HM3W header) and loads the tables.
    pub fn open(file: &'a [u8]) -> Result<Self, Error> {
        let mut start = None;
        let mut off = 0;
        while off + 32 <= file.len() {
            match &file[off..off + 4] {
                b"MPQ\x1a" => {
                    start = Some(off);
                    break;
                }
                // User data block: points at the real header.
                b"MPQ\x1b" => {
                    let h = u32_at(file, off + 8) as usize;
                    if off + h + 4 <= file.len() && &file[off + h..off + h + 4] == b"MPQ\x1a" {
                        start = Some(off + h);
                        break;
                    }
                }
                _ => {}
            }
            off += 512;
        }
        let start = start.ok_or(Error::NoHeader)?;
        let data = &file[start..];
        let header = Header {
            header_size: u32_at(data, 4),
            archive_size: u32_at(data, 8),
            format_version: u16_at(data, 12),
            sector_size_shift: u16_at(data, 14),
            hash_table_pos: u32_at(data, 16),
            block_table_pos: u32_at(data, 20),
            hash_table_entries: u32_at(data, 24),
            block_table_entries: u32_at(data, 28),
        };
        let raw = read_table(
            data,
            header.hash_table_pos,
            header.hash_table_entries,
            "(hash table)",
        )?;
        let hash_table = raw
            .chunks_exact(16)
            .map(|c| HashEntry {
                name_a: u32_at(c, 0),
                name_b: u32_at(c, 4),
                locale: u16_at(c, 8),
                platform: u16_at(c, 10),
                block_index: u32_at(c, 12),
            })
            .collect();
        let raw = read_table(
            data,
            header.block_table_pos,
            header.block_table_entries,
            "(block table)",
        )?;
        let block_table = raw
            .chunks_exact(16)
            .map(|c| BlockEntry {
                offset: u32_at(c, 0),
                compressed_size: u32_at(c, 4),
                file_size: u32_at(c, 8),
                flags: u32_at(c, 12),
            })
            .collect();
        Ok(Self {
            data,
            header,
            hash_table,
            block_table,
        })
    }

    pub fn sector_size(&self) -> usize {
        512usize << self.header.sector_size_shift
    }

    /// Looks up the block index of a file by name (any locale; neutral locale preferred).
    pub fn find(&self, name: &str) -> Option<usize> {
        let n = self.hash_table.len();
        if n == 0 {
            return None;
        }
        let a = hash_string(name, HASH_NAME_A);
        let b = hash_string(name, HASH_NAME_B);
        let mut i = hash_string(name, HASH_TABLE_OFFSET) as usize % n;
        let mut found = None;
        for _ in 0..n {
            let e = &self.hash_table[i];
            if e.block_index == HASH_ENTRY_EMPTY {
                break;
            }
            if e.block_index != HASH_ENTRY_DELETED && e.name_a == a && e.name_b == b {
                let idx = e.block_index as usize;
                if e.locale == 0 {
                    return Some(idx);
                }
                found.get_or_insert(idx);
            }
            i = (i + 1) % n;
        }
        found
    }

    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Reads and fully decodes a file by name.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        let idx = self
            .find(name)
            .ok_or_else(|| Error::NotFound(name.to_string()))?;
        self.read_block(idx, Some(name))
    }

    /// Names from the archive's `(listfile)`, if present.
    pub fn listfile(&self) -> Result<Vec<String>, Error> {
        let data = self.read("(listfile)")?;
        Ok(String::from_utf8_lossy(&data)
            .split(['\r', '\n', ';'])
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect())
    }

    pub fn read_block(&self, idx: usize, name: Option<&str>) -> Result<Vec<u8>, Error> {
        let b = self.block_table[idx];
        let raw = slice(self.data, b.offset as usize, b.compressed_size as usize)?;
        let key = if b.flags & FLAG_ENCRYPTED != 0 {
            let name = name.ok_or(Error::NeedName(idx))?;
            Some(file_key(
                name,
                b.offset,
                b.file_size,
                b.flags & FLAG_FIX_KEY != 0,
            ))
        } else {
            None
        };
        let size = b.file_size as usize;

        if b.flags & FLAG_SINGLE_UNIT != 0 {
            let mut buf = raw.to_vec();
            if let Some(k) = key {
                decrypt_bytes(&mut buf, k);
            }
            return decode_sector(&buf, size, b.flags);
        }

        let sector = self.sector_size();
        let sectors = size.div_ceil(sector);
        let table_len = sectors + 1 + usize::from(b.flags & FLAG_SECTOR_CRC != 0);
        let mut offsets = if b.flags & (FLAG_COMPRESS | FLAG_IMPLODE) != 0 {
            let mut t = slice(raw, 0, table_len * 4)?.to_vec();
            if let Some(k) = key {
                decrypt_bytes(&mut t, k.wrapping_sub(1));
            }
            t.chunks_exact(4)
                .map(|c| u32_at(c, 0) as usize)
                .collect::<Vec<_>>()
        } else {
            (0..=sectors).map(|i| (i * sector).min(size)).collect()
        };
        offsets.truncate(sectors + 1);

        let mut out = Vec::with_capacity(size);
        for i in 0..sectors {
            let (s, e) = (offsets[i], offsets[i + 1]);
            let mut buf = slice(raw, s, e.saturating_sub(s))?.to_vec();
            if let Some(k) = key {
                decrypt_bytes(&mut buf, k.wrapping_add(i as u32));
            }
            let expected = sector.min(size - i * sector);
            out.extend(decode_sector(&buf, expected, b.flags)?);
        }
        Ok(out)
    }
}

/// Decodes one sector (or a single-unit file). A sector stored at full size is uncompressed.
fn decode_sector(buf: &[u8], expected: usize, flags: u32) -> Result<Vec<u8>, Error> {
    if buf.len() >= expected {
        return Ok(buf[..expected].to_vec());
    }
    let out = if flags & FLAG_COMPRESS != 0 {
        let (&mask, data) = buf
            .split_first()
            .ok_or_else(|| Error::Decompress("empty sector".into()))?;
        decode_masked(mask, data, expected)?
    } else if flags & FLAG_IMPLODE != 0 {
        explode_dcl(buf)?
    } else {
        buf.to_vec()
    };
    if out.len() != expected {
        return Err(Error::SizeMismatch {
            expected,
            got: out.len(),
        });
    }
    Ok(out)
}

/// Undoes the stages named by a compression mask. Stages were applied in the order ADPCM, then
/// Huffman, then the general compressor (zlib, bzip2 or DCL), so they are undone in reverse:
/// general compressor, then Huffman, then ADPCM.
fn decode_masked(mask: u8, data: &[u8], expected: usize) -> Result<Vec<u8>, Error> {
    let general = mask & (MASK_ZLIB | MASK_DCL | MASK_BZIP2);
    let adpcm = mask & (MASK_ADPCM_MONO | MASK_ADPCM_STEREO);
    let known =
        MASK_HUFFMAN | MASK_ZLIB | MASK_DCL | MASK_BZIP2 | MASK_ADPCM_MONO | MASK_ADPCM_STEREO;
    if mask & !known != 0
        || general.count_ones() > 1
        || adpcm == (MASK_ADPCM_MONO | MASK_ADPCM_STEREO)
    {
        return Err(Error::UnsupportedCompression(mask));
    }

    let stage1 = match general {
        0 => data.to_vec(),
        MASK_ZLIB => inflate(flate2::read::ZlibDecoder::new(data), expected)?,
        MASK_DCL => explode_dcl(data)?,
        _ => inflate(bzip2::read::BzDecoder::new(data), expected)?,
    };
    let stage2 = if mask & MASK_HUFFMAN != 0 {
        huffman::decompress(&stage1, expected)?
    } else {
        stage1
    };
    match adpcm {
        0 => Ok(stage2),
        MASK_ADPCM_MONO => adpcm::decompress(&stage2, 1, expected),
        _ => adpcm::decompress(&stage2, 2, expected),
    }
}

fn inflate(mut r: impl std::io::Read, expected: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(expected);
    r.read_to_end(&mut out)
        .map_err(|e| Error::Decompress(e.to_string()))?;
    Ok(out)
}

fn explode_dcl(data: &[u8]) -> Result<Vec<u8>, Error> {
    explode::explode(data).map_err(|e| Error::Decompress(format!("{e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn pcm(n: usize, amp: f64, period: f64) -> Vec<i16> {
        (0..n)
            .map(|i| (amp * (i as f64 * std::f64::consts::TAU / period).sin()).round() as i16)
            .collect()
    }

    fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    /// Mono sector with mask 0x40 (ADPCM only).
    fn adpcm_sector(samples: &[i16], channels: usize) -> Vec<u8> {
        let mut buf = vec![if channels == 1 { 0x40 } else { 0x80 }];
        buf.extend(adpcm::encode(samples, channels, 2));
        buf
    }

    /// Sector with the given mask bits, built in compression order: ADPCM, then Huffman, then
    /// the general compressor.
    fn sector(mask: u8, samples: &[i16], channels: usize) -> (Vec<u8>, usize) {
        let expected = samples.len() * 2;
        let body = if mask & (MASK_ADPCM_MONO | MASK_ADPCM_STEREO) != 0 {
            adpcm::encode(samples, channels, 2)
        } else {
            pcm_bytes(samples)
        };
        let body = if mask & MASK_HUFFMAN != 0 {
            huffman::encode(&body)
        } else {
            body
        };
        let body = if mask & MASK_ZLIB != 0 {
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(&body).unwrap();
            enc.finish().unwrap()
        } else {
            body
        };
        let mut buf = vec![mask];
        buf.extend(body);
        (buf, expected)
    }

    fn decode(buf: &[u8], expected: usize) -> Vec<i16> {
        // Compressed sectors must be smaller than the output, or the raw path is taken.
        assert!(buf.len() < expected, "test sector is not compressed");
        let out = decode_sector(buf, expected, FLAG_COMPRESS).unwrap();
        out.chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    fn max_error(a: &[i16], b: &[i16]) -> i32 {
        a.iter()
            .zip(b)
            .map(|(&x, &y)| (i32::from(x) - i32::from(y)).abs())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn huffman_only_sector() {
        let data = vec![b'A'; 2000];
        let mut buf = vec![MASK_HUFFMAN];
        buf.extend(huffman::encode(&data));
        // Must be smaller than the output, or decode_sector would take the stored-sector path.
        assert!(buf.len() < data.len());
        assert_eq!(
            decode_sector(&buf, data.len(), FLAG_COMPRESS).unwrap(),
            data
        );
    }

    #[test]
    fn adpcm_only_sectors() {
        let mono = pcm(400, 5000.0, 40.0);
        let got = decode(&adpcm_sector(&mono, 1), mono.len() * 2);
        assert_eq!(got.len(), mono.len());
        assert!(max_error(&mono, &got) <= 64);

        let stereo = pcm(400, 3000.0, 80.0);
        let got = decode(&adpcm_sector(&stereo, 2), stereo.len() * 2);
        assert_eq!(got.len(), stereo.len());
        assert!(max_error(&stereo, &got) <= 64);
    }

    #[test]
    fn huffman_then_adpcm_masks() {
        let mono = pcm(256, 5000.0, 40.0);
        let (buf, expected) = sector(0x41, &mono, 1);
        let got = decode(&buf, expected);
        assert_eq!(got.len(), mono.len());
        assert!(max_error(&mono, &got) <= 64);

        let stereo = pcm(256, 3000.0, 80.0);
        let (buf, expected) = sector(0x81, &stereo, 2);
        let got = decode(&buf, expected);
        assert_eq!(got.len(), stereo.len());
        assert!(max_error(&stereo, &got) <= 64);
    }

    #[test]
    fn zlib_then_huffman_mask() {
        // Mask 0x03: zlib is undone first, then Huffman.
        let data: Vec<u8> = (0..600u32).map(|i| (i % 37) as u8).collect();
        let body = huffman::encode(&data);
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&body).unwrap();
        let mut buf = vec![MASK_ZLIB | MASK_HUFFMAN];
        buf.extend(enc.finish().unwrap());
        assert_eq!(
            decode_sector(&buf, data.len(), FLAG_COMPRESS).unwrap(),
            data
        );
    }

    #[test]
    fn unsupported_masks_are_rejected() {
        let data = [0u8; 64];
        for mask in [0x04u8, 0x20, 0x0a, 0xc0] {
            let mut buf = vec![mask];
            buf.extend(&data[..16]);
            assert!(
                matches!(
                    decode_sector(&buf, 64, FLAG_COMPRESS),
                    Err(Error::UnsupportedCompression(m)) if m == mask
                ),
                "mask {mask:#04x} should be rejected"
            );
        }
    }
}
