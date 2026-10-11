//! Decoder for Warcraft III **BLP1** textures, producing RGBA8 pixels.
//!
//! Layout (all integers little-endian):
//!
//! ```text
//! 0x00  "BLP1"                      magic
//! 0x04  u32 content                 0 = JPEG, 1 = paletted
//! 0x08  u32 alpha_bits              0, 1, 4 or 8
//! 0x0C  u32 width
//! 0x10  u32 height
//! 0x14  u32 extra                   unused here
//! 0x18  u32 has_mipmaps
//! 0x1C  u32 mip_offset[16]          absolute file offsets
//! 0x5C  u32 mip_size[16]
//! 0x9C  ...                         content-specific data (see below)
//! ```
//!
//! * Paletted: a 256-entry BGRA palette (1024 bytes) follows the header. Each mip is
//!   `w*h` palette indices followed by alpha data packed at `alpha_bits` per pixel.
//! * JPEG: a u32 header size, then the shared JPEG header bytes (SOI through the start of the
//!   frame header). Each mip is stored without the shared header; the decoder prepends it.
//!   Every JPEG mip is a 4-component frame with no colour transform: the components are
//!   B, G, R and alpha, stored as-is. `alpha_bits` is 0 for opaque files and 8 when the fourth
//!   component carries alpha. The layout was checked against the shipped Warcraft III 1.26
//!   textures: the decoded colours and the alpha of a glowing rune texture come out right
//!   only with this reading.
//!
//! Details not confirmed by real files are marked `UNVERIFIED` in the code: the 1-bit and
//! 4-bit alpha packing (the shipped archives only use 0 and 8), and top-to-bottom row order.

/// Size of the fixed header (magic through the mip size table).
pub const HEADER_LEN: usize = 0x9C;
/// Number of mip-map slots in the header.
pub const MAX_MIPS: usize = 16;
/// Size of the BGRA palette used by paletted content.
pub const PALETTE_LEN: usize = 256 * 4;
/// Largest accepted width or height. Guards against absurd allocations from corrupt headers.
pub const MAX_DIMENSION: u32 = 16_384;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("data truncated: need {needed} bytes, have {have}")]
    Truncated { needed: usize, have: usize },
    #[error("bad magic {0:?}, expected \"BLP1\"")]
    BadMagic([u8; 4]),
    #[error("unsupported BLP feature: {0}")]
    Unsupported(String),
    #[error("invalid content type {0} (expected 0 = JPEG or 1 = paletted)")]
    InvalidContentType(u32),
    #[error("invalid alpha bit depth {0} (expected 0, 1, 4 or 8)")]
    InvalidAlphaBits(u32),
    #[error("invalid texture size {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("mip level {level} out of range (texture has {count} levels)")]
    NoSuchMip { level: usize, count: usize },
    #[error("mip level {level} data is outside the file (offset {offset}, size {size})")]
    MipOutOfBounds {
        level: usize,
        offset: usize,
        size: usize,
    },
    #[error("JPEG decoding failed: {0}")]
    Jpeg(String),
}

/// Content encoding of the mip-map data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Jpeg,
    Paletted,
}

/// Parsed BLP1 header fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub content: ContentType,
    /// Alpha bit depth: 0, 1, 4 or 8.
    pub alpha_bits: u32,
    /// Size of mip level 0 in pixels.
    pub width: u32,
    pub height: u32,
    pub extra: u32,
    pub has_mipmaps: bool,
    /// Absolute file offsets of each mip level (0 when unused).
    pub mip_offsets: [u32; MAX_MIPS],
    /// Byte sizes of each mip level (0 when unused).
    pub mip_sizes: [u32; MAX_MIPS],
}

/// A parsed BLP1 texture. Owns a copy of the file bytes; mips are decoded on demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blp {
    pub header: Header,
    data: Vec<u8>,
}

impl Blp {
    /// Parses a BLP1 file. Mip data is not decoded or range-checked here, so a file whose
    /// smaller mips are damaged can still yield its larger levels.
    pub fn parse(bytes: &[u8]) -> Result<Blp, Error> {
        if bytes.len() < HEADER_LEN {
            return Err(Error::Truncated {
                needed: HEADER_LEN,
                have: bytes.len(),
            });
        }
        let magic: [u8; 4] = [bytes[0], bytes[1], bytes[2], bytes[3]];
        match &magic {
            b"BLP1" => {}
            b"BLP2" => {
                return Err(Error::Unsupported(
                    "BLP2 (Warcraft III Reforged) is not supported yet".to_owned(),
                ))
            }
            _ => return Err(Error::BadMagic(magic)),
        }

        let content = match u32_at(bytes, 4) {
            0 => ContentType::Jpeg,
            1 => ContentType::Paletted,
            other => return Err(Error::InvalidContentType(other)),
        };
        let alpha_bits = u32_at(bytes, 8);
        if !matches!(alpha_bits, 0 | 1 | 4 | 8) {
            return Err(Error::InvalidAlphaBits(alpha_bits));
        }
        let width = u32_at(bytes, 12);
        let height = u32_at(bytes, 16);
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(Error::InvalidDimensions { width, height });
        }
        let extra = u32_at(bytes, 20);
        let has_mipmaps = u32_at(bytes, 24) != 0;

        let mut mip_offsets = [0u32; MAX_MIPS];
        let mut mip_sizes = [0u32; MAX_MIPS];
        for i in 0..MAX_MIPS {
            mip_offsets[i] = u32_at(bytes, 0x1C + 4 * i);
            mip_sizes[i] = u32_at(bytes, 0x5C + 4 * i);
        }

        match content {
            ContentType::Paletted => {
                let needed = HEADER_LEN + PALETTE_LEN;
                if bytes.len() < needed {
                    return Err(Error::Truncated {
                        needed,
                        have: bytes.len(),
                    });
                }
            }
            ContentType::Jpeg => {
                let needed = HEADER_LEN + 4;
                if bytes.len() < needed {
                    return Err(Error::Truncated {
                        needed,
                        have: bytes.len(),
                    });
                }
                let jpeg_header_len = u32_at(bytes, HEADER_LEN) as usize;
                let needed = HEADER_LEN + 4 + jpeg_header_len;
                if bytes.len() < needed {
                    return Err(Error::Truncated {
                        needed,
                        have: bytes.len(),
                    });
                }
            }
        }

        Ok(Blp {
            header: Header {
                content,
                alpha_bits,
                width,
                height,
                extra,
                has_mipmaps,
                mip_offsets,
                mip_sizes,
            },
            data: bytes.to_vec(),
        })
    }

    /// Number of usable mip levels, including level 0.
    ///
    /// Without mip-maps this is 1. With mip-maps it counts the leading slots that have a
    /// non-zero offset and size.
    pub fn mip_count(&self) -> usize {
        if !self.header.has_mipmaps {
            return 1;
        }
        let mut count = 1;
        while count < MAX_MIPS
            && self.header.mip_offsets[count] != 0
            && self.header.mip_sizes[count] != 0
        {
            count += 1;
        }
        count
    }

    /// Width and height of a mip level (each dimension halves per level, minimum 1).
    pub fn mip_dimensions(&self, level: usize) -> (u32, u32) {
        let shift = level.min(31) as u32;
        (
            (self.header.width >> shift).max(1),
            (self.header.height >> shift).max(1),
        )
    }

    /// Decodes a mip level to tightly packed RGBA8 pixels. UNVERIFIED: rows are assumed to be
    /// stored top to bottom, as written, with no flip.
    ///
    /// Returns `(width, height, pixels)` with `pixels.len() == width * height * 4`.
    pub fn mip_rgba(&self, level: usize) -> Result<(u32, u32, Vec<u8>), Error> {
        let count = self.mip_count();
        if level >= count {
            return Err(Error::NoSuchMip { level, count });
        }
        let (w, h) = self.mip_dimensions(level);
        let offset = self.header.mip_offsets[level] as usize;
        let size = self.header.mip_sizes[level] as usize;
        let end = offset.checked_add(size).filter(|&e| e <= self.data.len());
        let Some(end) = end else {
            return Err(Error::MipOutOfBounds {
                level,
                offset,
                size,
            });
        };
        let mip = &self.data[offset..end];

        let rgba = match self.header.content {
            ContentType::Paletted => self.decode_paletted(level, mip, w as usize, h as usize)?,
            ContentType::Jpeg => self.decode_jpeg(mip, w, h)?,
        };
        Ok((w, h, rgba))
    }

    fn decode_paletted(
        &self,
        level: usize,
        mip: &[u8],
        w: usize,
        h: usize,
    ) -> Result<Vec<u8>, Error> {
        let palette = &self.data[HEADER_LEN..HEADER_LEN + PALETTE_LEN];
        let pixels = w * h;
        let alpha_bits = self.header.alpha_bits;
        let alpha_len = match alpha_bits {
            1 => pixels.div_ceil(8),
            4 => pixels.div_ceil(2),
            8 => pixels,
            _ => 0,
        };
        let needed = pixels + alpha_len;
        if mip.len() < needed {
            return Err(Error::MipOutOfBounds {
                level,
                offset: self.header.mip_offsets[level] as usize,
                size: needed,
            });
        }
        let (indices, alpha) = mip.split_at(pixels);

        let mut out = vec![0u8; pixels * 4];
        for (i, (&index, px)) in indices.iter().zip(out.chunks_exact_mut(4)).enumerate() {
            // Palette entries are BGRA; the palette's own alpha byte is ignored and per-pixel
            // alpha comes from the alpha section.
            let entry = &palette[index as usize * 4..index as usize * 4 + 4];
            px[0] = entry[2];
            px[1] = entry[1];
            px[2] = entry[0];
            px[3] = alpha_at(alpha, alpha_bits, i);
        }
        Ok(out)
    }

    /// Decodes one JPEG mip: the shared header followed by this mip's JPEG data. Alpha is the
    /// fourth JPEG component; there is no separate alpha section (checked on real terrain
    /// textures: mip 0 ends with the JPEG end-of-image marker and the fourth component spans
    /// the full 0..=255 range).
    fn decode_jpeg(&self, mip: &[u8], w: u32, h: u32) -> Result<Vec<u8>, Error> {
        let header_len = u32_at(&self.data, HEADER_LEN) as usize;
        let shared = &self.data[HEADER_LEN + 4..HEADER_LEN + 4 + header_len];
        let jpeg_data = mip;

        let mut jpeg = Vec::with_capacity(shared.len() + jpeg_data.len());
        jpeg.extend_from_slice(shared);
        jpeg.extend_from_slice(jpeg_data);

        // jpeg-decoder does no colour conversion for 4-component frames without a colour
        // transform, but it reports each CMYK component as 255 - stored value. Undoing that
        // gives the stored B, G, R and alpha bytes. zune-jpeg is not used: it converts CMYK
        // to RGB and drops the fourth component.
        let mut decoder = jpeg_decoder::Decoder::new(jpeg.as_slice());
        let pixels = decoder.decode().map_err(|e| Error::Jpeg(e.to_string()))?;
        let info = decoder
            .info()
            .ok_or_else(|| Error::Jpeg("missing image info".to_owned()))?;
        if u32::from(info.width) != w || u32::from(info.height) != h {
            return Err(Error::Jpeg(format!(
                "JPEG is {}x{}, BLP mip is {}x{}",
                info.width, info.height, w, h
            )));
        }
        if info.pixel_format != jpeg_decoder::PixelFormat::CMYK32 {
            return Err(Error::Jpeg(format!(
                "expected a 4-component frame, got {:?}",
                info.pixel_format
            )));
        }
        let count = w as usize * h as usize;
        if pixels.len() != count * 4 {
            return Err(Error::Jpeg(format!(
                "decoded {} bytes, expected {}",
                pixels.len(),
                count * 4
            )));
        }
        let mut out = vec![0u8; count * 4];
        for (dst, src) in out.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
            dst.copy_from_slice(&[255 - src[2], 255 - src[1], 255 - src[0], 255 - src[3]]);
        }
        Ok(out)
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Alpha of pixel `i` for the given bit depth, scaled to 0..=255.
fn alpha_at(alpha: &[u8], bits: u32, i: usize) -> u8 {
    match bits {
        1 => {
            if (alpha[i / 8] >> (i % 8)) & 1 != 0 {
                255
            } else {
                0
            }
        }
        // UNVERIFIED nibble order: pixel 0 is the low nibble of the first byte.
        4 => {
            let byte = alpha[i / 2];
            let v = if i.is_multiple_of(2) {
                byte & 0x0F
            } else {
                byte >> 4
            };
            v * 17
        }
        8 => alpha[i],
        _ => 255,
    }
}
