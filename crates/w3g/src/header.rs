use crate::reader::Reader;
use crate::Error;

pub const MAGIC: &[u8; 28] = b"Warcraft III recorded game\x1a\0";

/// File header of a classic (pre-Reforged) replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub header_size: u32,
    pub compressed_size: u32,
    pub header_version: u32,
    pub decompressed_size: u32,
    pub block_count: u32,
    pub sub: SubHeader,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubHeader {
    /// `b"W3XP"` (TFT) or `b"WAR3"` (RoC); `None` for header version 0.
    pub product: Option<[u8; 4]>,
    /// Patch minor version, e.g. 26 for 1.26.
    pub version: u32,
    pub build: u16,
    /// 0x0000 single player, 0x8000 multiplayer.
    pub flags: u16,
    pub length_ms: u32,
    pub header_crc32: u32,
}

impl Header {
    pub fn is_multiplayer(&self) -> bool {
        self.sub.flags & 0x8000 != 0
    }
}

pub(crate) fn parse_header(r: &mut Reader) -> Result<Header, Error> {
    if r.bytes(MAGIC.len())? != MAGIC {
        return Err(Error::BadMagic);
    }
    let header_size = r.u32()?;
    let compressed_size = r.u32()?;
    let header_version = r.u32()?;
    let decompressed_size = r.u32()?;
    let block_count = r.u32()?;
    // The original refuses a header whose size field does not match its version (0x40 for version 0,
    // 0x44 for version 1; `FUN_6f534750`, docs/spec/replay-load.md section 2).
    let expected = match header_version {
        0 => 0x40,
        1 => 0x44,
        v => return Err(Error::UnsupportedHeaderVersion(v)),
    };
    if header_size != expected {
        return Err(Error::BadHeaderSize {
            version: header_version,
            size: header_size,
            expected,
        });
    }
    let sub = match header_version {
        0 => {
            let _unknown = r.u16()?;
            let version = r.u16()? as u32;
            SubHeader {
                product: None,
                version,
                build: r.u16()?,
                flags: r.u16()?,
                length_ms: r.u32()?,
                header_crc32: r.u32()?,
            }
        }
        1 => {
            // Stored reversed on disk ("PX3W").
            let mut product = r.array::<4>()?;
            product.reverse();
            SubHeader {
                product: Some(product),
                version: r.u32()?,
                build: r.u16()?,
                flags: r.u16()?,
                length_ms: r.u32()?,
                header_crc32: r.u32()?,
            }
        }
        v => return Err(Error::UnsupportedHeaderVersion(v)),
    };
    Ok(Header {
        header_size,
        compressed_size,
        header_version,
        decompressed_size,
        block_count,
        sub,
    })
}

/// Inflates all zlib blocks following the header into one contiguous buffer.
pub(crate) fn decompress_blocks(file: &[u8], header: &Header) -> Result<Vec<u8>, Error> {
    let mut r = Reader::new(file);
    r.bytes(header.header_size as usize)?;
    let mut out = Vec::with_capacity(header.decompressed_size as usize);
    for index in 0..header.block_count {
        let csize = r.u16()? as usize;
        let dsize = r.u16()? as usize;
        let _checksum = r.u32()?;
        let data = r.bytes(csize)?;
        let start = out.len();
        // Real 1.26a blocks end with a sync flush, not a finished zlib stream, so inflate
        // until `dsize` bytes are out instead of requiring the stream end.
        out.reserve(dsize);
        let mut z = flate2::Decompress::new(true);
        loop {
            let before = (z.total_in(), z.total_out());
            let status = z
                .decompress_vec(
                    &data[z.total_in() as usize..],
                    &mut out,
                    flate2::FlushDecompress::Sync,
                )
                .map_err(|e| Error::Inflate {
                    block: index,
                    source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                })?;
            let produced = out.len() - start;
            if status == flate2::Status::StreamEnd || produced >= dsize {
                break;
            }
            if out.capacity() == out.len() {
                out.reserve(dsize.max(4096));
            }
            if (z.total_in(), z.total_out()) == before {
                break;
            }
        }
        if out.len() - start < dsize {
            return Err(Error::ShortBlock {
                block: index,
                expected: dsize,
                got: out.len() - start,
            });
        }
        out.truncate(start + dsize);
    }
    // The last block is padded with zeros up to the fixed block size.
    out.truncate(header.decompressed_size as usize);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real 1.26a blocks end with a zlib sync flush (no stream end); they must still inflate.
    #[test]
    fn sync_flushed_block_inflates() {
        let payload: Vec<u8> = (0..8192u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut z = flate2::Compress::new(flate2::Compression::default(), true);
        let mut block = Vec::with_capacity(16384);
        z.compress_vec(&payload, &mut block, flate2::FlushCompress::Sync)
            .unwrap();
        let mut file = vec![0u8; 0x44];
        file.extend_from_slice(&(block.len() as u16).to_le_bytes());
        file.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&block);
        let header = Header {
            header_size: 0x44,
            compressed_size: file.len() as u32,
            header_version: 1,
            decompressed_size: payload.len() as u32,
            block_count: 1,
            sub: SubHeader {
                product: Some(*b"W3XP"),
                version: 26,
                build: 6059,
                flags: 0x8000,
                length_ms: 0,
                header_crc32: 0,
            },
        };
        assert_eq!(decompress_blocks(&file, &header).unwrap(), payload);
    }

    /// The header size must match the version (`FUN_6f534750`: 0x40 for version 0, 0x44 for version 1).
    #[test]
    fn header_size_must_match_the_version() {
        let mut file = Vec::new();
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&0x40u32.to_le_bytes()); // size: wrong for version 1
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&[0u8; 20]);
        let err = parse_header(&mut Reader::new(&file)).unwrap_err();
        assert!(matches!(
            err,
            Error::BadHeaderSize {
                version: 1,
                size: 0x40,
                expected: 0x44
            }
        ));
    }
}
