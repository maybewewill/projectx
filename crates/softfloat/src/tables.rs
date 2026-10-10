//! Lookup tables used by the helpers, read at runtime from the user's own `Game.dll`.
//!
//! No table contents are embedded in this crate. [`Tables::from_game_dll`] takes the
//! bytes of a 1.26a `Game.dll`, checks that it is a PE32 image, maps the fixed virtual
//! addresses below to file offsets and copies the entries out.
//!
//! | table | VA | entries | used by |
//! |---|---|---|---|
//! | reciprocal | `0x6F9774C8` | 1025 x u32 | `recip` (division) |
//! | sine quarter-wave | `0x6F9784D0` | 1025 x u32 | sin / cos / tan |
//! | arc sine | `0x6F9794D8` | 1025 x u32 | asin / acos |
//! | arc sine, near 1 | `0x6F97A4E0` | 153 x u32 | asin / acos |
//!
//! Functions that do not use a table (add, sub, mul, conversions, compares) work
//! without a `Tables` value.

use alloc::vec::Vec;
use core::fmt;

/// Virtual address (image base `0x6F000000`) of the reciprocal table.
pub const RECIP_VA: u32 = 0x6F97_74C8;
/// Number of u32 entries in the reciprocal table (index 0..=1024).
pub const RECIP_LEN: usize = 1025;
/// Virtual address of the sine quarter-wave table.
pub const SIN_VA: u32 = 0x6F97_84D0;
/// Number of u32 entries in the sine table (index 0..=1024).
pub const SIN_LEN: usize = 1025;
/// Virtual address of the arc-sine table for `|x| <= 0.9921875`.
pub const ASIN_VA: u32 = 0x6F97_94D8;
/// Number of u32 entries in the arc-sine table (index 0..=1024).
pub const ASIN_LEN: usize = 1025;
/// Virtual address of the arc-sine table for `|x|` close to 1.
pub const ASIN2_VA: u32 = 0x6F97_A4E0;
/// Number of u32 entries in the near-1 arc-sine table (index 0..=152).
pub const ASIN2_LEN: usize = 153;

/// Error while reading tables from a `Game.dll` image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TablesError {
    /// The file is too short or has no MZ/PE signature.
    NotPe,
    /// The image is not a 32-bit (PE32) image.
    NotPe32,
    /// A table's virtual range is not fully inside the file data of one section.
    Unmapped {
        /// Start virtual address of the table that could not be read.
        va: u32,
    },
}

impl fmt::Display for TablesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TablesError::NotPe => write!(f, "not a PE image"),
            TablesError::NotPe32 => write!(f, "not a 32-bit PE image"),
            TablesError::Unmapped { va } => {
                write!(f, "table at VA {va:#010x} is not mapped by the file")
            }
        }
    }
}

/// Lookup tables of the helpers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tables {
    recip: Vec<u32>,
    sin: Vec<u32>,
    asin: Vec<u32>,
    asin2: Vec<u32>,
}

impl Tables {
    /// Reads all tables from the bytes of a 1.26a `Game.dll`.
    pub fn from_game_dll(dll: &[u8]) -> Result<Tables, TablesError> {
        let image = PeImage::parse(dll)?;
        Ok(Tables {
            recip: image.read_u32s(RECIP_VA, RECIP_LEN)?,
            sin: image.read_u32s(SIN_VA, SIN_LEN)?,
            asin: image.read_u32s(ASIN_VA, ASIN_LEN)?,
            asin2: image.read_u32s(ASIN2_VA, ASIN2_LEN)?,
        })
    }

    /// Builds tables from explicit contents (for callers that cache the values
    /// themselves). Lengths must be the `*_LEN` constants of this module.
    pub fn from_raw(
        recip: Vec<u32>,
        sin: Vec<u32>,
        asin: Vec<u32>,
        asin2: Vec<u32>,
    ) -> Option<Tables> {
        if recip.len() == RECIP_LEN
            && sin.len() == SIN_LEN
            && asin.len() == ASIN_LEN
            && asin2.len() == ASIN2_LEN
        {
            Some(Tables {
                recip,
                sin,
                asin,
                asin2,
            })
        } else {
            None
        }
    }

    /// Reciprocal table entry `i` (`i <= 1024`).
    #[inline]
    pub fn recip_at(&self, i: usize) -> u32 {
        self.recip[i]
    }

    /// Sine table entry `i` (`i <= 1024`).
    #[inline]
    pub fn sin_at(&self, i: usize) -> u32 {
        self.sin[i]
    }

    /// Arc-sine table entry `i` (`i <= 1024`).
    #[inline]
    pub fn asin_at(&self, i: usize) -> u32 {
        self.asin[i]
    }

    /// Near-1 arc-sine table entry `i` (`i <= 152`).
    #[inline]
    pub fn asin2_at(&self, i: usize) -> u32 {
        self.asin2[i]
    }
}

/// Minimal PE32 reader: just enough to map a virtual address to file bytes.
struct PeImage<'a> {
    data: &'a [u8],
    image_base: u32,
    sections: Vec<Section>,
}

#[derive(Clone, Copy)]
struct Section {
    va: u32,
    vsize: u32,
    raw_ptr: u32,
    raw_size: u32,
}

fn le16(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn le32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

impl<'a> PeImage<'a> {
    fn parse(data: &'a [u8]) -> Result<PeImage<'a>, TablesError> {
        if le16(data, 0) != Some(0x5a4d) {
            return Err(TablesError::NotPe);
        }
        let pe = le32(data, 0x3c).ok_or(TablesError::NotPe)? as usize;
        if le32(data, pe) != Some(0x0000_4550) {
            return Err(TablesError::NotPe);
        }
        let nsec = le16(data, pe + 6).ok_or(TablesError::NotPe)? as usize;
        let opt_size = le16(data, pe + 20).ok_or(TablesError::NotPe)? as usize;
        let opt = pe + 24;
        if le16(data, opt) != Some(0x010b) {
            return Err(TablesError::NotPe32);
        }
        let image_base = le32(data, opt + 28).ok_or(TablesError::NotPe)?;
        let mut sections = Vec::with_capacity(nsec);
        let sec0 = opt + opt_size;
        for i in 0..nsec {
            let o = sec0 + i * 40;
            let vsize = le32(data, o + 8).ok_or(TablesError::NotPe)?;
            let va = le32(data, o + 12).ok_or(TablesError::NotPe)?;
            let raw_size = le32(data, o + 16).ok_or(TablesError::NotPe)?;
            let raw_ptr = le32(data, o + 20).ok_or(TablesError::NotPe)?;
            sections.push(Section {
                va,
                vsize,
                raw_ptr,
                raw_size,
            });
        }
        Ok(PeImage {
            data,
            image_base,
            sections,
        })
    }

    fn read_u32s(&self, va: u32, count: usize) -> Result<Vec<u32>, TablesError> {
        let unmapped = TablesError::Unmapped { va };
        let rva = va.wrapping_sub(self.image_base);
        let bytes = count * 4;
        for s in &self.sections {
            let span = s.vsize.max(s.raw_size);
            if rva < s.va || rva.checked_add(bytes as u32).is_none() {
                continue;
            }
            let off = rva - s.va;
            if off.checked_add(bytes as u32).is_none_or(|end| end > span) {
                continue;
            }
            // the bytes must also exist in the file (no zero-fill for these tables)
            let file_off = s.raw_ptr as usize + off as usize;
            let slice = self
                .data
                .get(file_off..file_off + bytes)
                .ok_or(unmapped.clone())?;
            return Ok(slice
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect());
        }
        Err(unmapped)
    }
}
