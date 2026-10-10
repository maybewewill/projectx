use std::io::Read;

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
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .map_err(|e| Error::Inflate {
                block: index,
                source: e,
            })?;
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
