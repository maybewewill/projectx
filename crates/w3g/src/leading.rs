//! The leading part of a replay body, read the way the 1.26a game setup reads it.
//!
//! The game setup builder `0x6F443860` reads the decompressed body from its stream object from
//! byte 0, positionally: there is no record-id compare in this path. Each read is one of the
//! primitives below, in this order (docs/spec/replay-load.md, section 3; reader checked against the
//! machine code under Unicorn, `tools/emu/replay_leading_emu.py`, 400 random bodies, 0 mismatches):
//!
//! 1. NUL-terminated string, at most 2047 bytes (`0x6F309A00`), stored as the first name;
//! 2. the same string again, only when the build is 6027 or more (`0x178B`);
//! 3. a 184-byte block (`0x6F6521B0`): string of at most 32 bytes, string of at most 16 bytes,
//!    string of at most 128 bytes (the encoded settings), then two dwords;
//! 4. a `u16` length (at most 128) and that many bytes (`0x6F443720`): the start blob, which holds the
//!    `0x19` game-start container (`crate::start::parse_start_body`);
//! 5. two dwords; for build 6058 (`0x17AA`) or more, 20 raw bytes; two dwords; a NUL-terminated
//!    string of at most 2047 bytes.
//!
//! A string must end with its NUL inside the given maximum (the NUL counts). A read past the end of
//! the body, or a string that does not fit, is an error. The original then keeps the position at
//! `len + 1`.
//!
//! This is what the original accepts. It is **not** the same layout as [`crate::GameInfo`] (host record,
//! `0x16` player records, `0x19` block id), which is kept for the existing tests. The two cannot both
//! be right for a real replay; the real file decides (not available yet).

/// The maximum string length of the first name read (`0x800` bytes including the NUL).
pub const NAME_MAX: usize = 2048;

/// The first build that reads a second name (`0x178B`).
pub const SECOND_NAME_BUILD: u32 = 0x178B;

/// The first build that reads the 20 raw bytes in the tail (`0x17AA`).
pub const TAIL_BYTES_BUILD: u32 = 0x17AA;

/// The start blob is at most this many bytes (`0x80`).
pub const BLOB_MAX: usize = 128;

/// The fields of the leading part, in body order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leading {
    pub name_a: Vec<u8>,
    /// Present only for builds 6027 and later.
    pub name_b: Option<Vec<u8>>,
    pub c32: Vec<u8>,
    pub c16: Vec<u8>,
    /// The encoded settings string (at most 128 bytes, NUL-terminated in the body).
    pub stat: Vec<u8>,
    pub u32_b0: u32,
    pub u32_b4: u32,
    /// The start blob: the `0x19` game-start body (without the block id and length).
    pub blob: Vec<u8>,
    pub u32_430: u32,
    pub u32_434: u32,
    /// Present only for builds 6058 and later.
    pub tail20: Option<[u8; 20]>,
    pub u32_404: u32,
    pub u32_408: u32,
    pub name_tail: Vec<u8>,
}

/// A read that the original refuses (end of body, or a string that does not fit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadingError {
    /// The offset at which the failing read started.
    pub offset: usize,
}

impl std::fmt::Display for LeadingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "leading record read fails at offset {:#x}", self.offset)
    }
}

impl std::error::Error for LeadingError {}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], LeadingError> {
        let start = self.pos;
        if self.data.len().saturating_sub(start) < n {
            return Err(LeadingError { offset: start });
        }
        self.pos += n;
        Ok(&self.data[start..start + n])
    }

    fn u16(&mut self) -> Result<u16, LeadingError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, LeadingError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A NUL-terminated string whose NUL fits in `max` bytes; returns the text without the NUL.
    fn cstr(&mut self, max: usize) -> Result<Vec<u8>, LeadingError> {
        let start = self.pos;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.data.get(self.pos) else {
                return Err(LeadingError { offset: start });
            };
            self.pos += 1;
            if c == 0 {
                return Ok(out);
            }
            out.push(c);
            if out.len() >= max {
                return Err(LeadingError { offset: start });
            }
        }
    }
}

/// Reads the leading part of a body of the given build number.
pub fn read_leading(body: &[u8], build: u32) -> Result<Leading, LeadingError> {
    let mut c = Cursor { data: body, pos: 0 };
    let name_a = c.cstr(NAME_MAX)?;
    let name_b = if build >= SECOND_NAME_BUILD {
        Some(c.cstr(NAME_MAX)?)
    } else {
        None
    };
    let c32 = c.cstr(32)?;
    let c16 = c.cstr(16)?;
    let stat = c.cstr(128)?;
    let u32_b0 = c.u32()?;
    let u32_b4 = c.u32()?;
    let at = c.pos;
    let n = usize::from(c.u16()?);
    if n > BLOB_MAX {
        return Err(LeadingError { offset: at });
    }
    let blob = c.take(n)?.to_vec();
    let u32_430 = c.u32()?;
    let u32_434 = c.u32()?;
    let tail20 = if build >= TAIL_BYTES_BUILD {
        let b = c.take(20)?;
        let mut arr = [0u8; 20];
        arr.copy_from_slice(b);
        Some(arr)
    } else {
        None
    };
    let u32_404 = c.u32()?;
    let u32_408 = c.u32()?;
    let name_tail = c.cstr(NAME_MAX)?;
    Ok(Leading {
        name_a,
        name_b,
        c32,
        c16,
        stat,
        u32_b0,
        u32_b4,
        blob,
        u32_430,
        u32_434,
        tail20,
        u32_404,
        u32_408,
        name_tail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(out: &mut Vec<u8>, s: &[u8]) {
        out.extend_from_slice(s);
        out.push(0);
    }

    /// A body in the order the original reads, for build 6059.
    fn model_body(blob: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        cs(&mut b, b"Name A");
        cs(&mut b, b"Name B");
        cs(&mut b, b"c32");
        cs(&mut b, b"");
        cs(&mut b, b"stat");
        b.extend_from_slice(&7u32.to_le_bytes());
        b.extend_from_slice(&9u32.to_le_bytes());
        b.extend_from_slice(&(blob.len() as u16).to_le_bytes());
        b.extend_from_slice(blob);
        b.extend_from_slice(&11u32.to_le_bytes());
        b.extend_from_slice(&12u32.to_le_bytes());
        b.extend_from_slice(&[0xAB; 20]);
        b.extend_from_slice(&13u32.to_le_bytes());
        b.extend_from_slice(&14u32.to_le_bytes());
        cs(&mut b, b"tail");
        b
    }

    #[test]
    fn reads_fields_in_the_original_order() {
        let body = model_body(&[1, 2, 3]);
        let l = read_leading(&body, 6059).unwrap();
        assert_eq!(l.name_a, b"Name A");
        assert_eq!(l.name_b.as_deref(), Some(&b"Name B"[..]));
        assert_eq!(l.c32, b"c32");
        assert_eq!(l.c16, b"");
        assert_eq!(l.stat, b"stat");
        assert_eq!((l.u32_b0, l.u32_b4), (7, 9));
        assert_eq!(l.blob, [1, 2, 3]);
        assert_eq!((l.u32_430, l.u32_434), (11, 12));
        assert_eq!(l.tail20, Some([0xAB; 20]));
        assert_eq!((l.u32_404, l.u32_408), (13, 14));
        assert_eq!(l.name_tail, b"tail");
        assert!(read_leading(&body[..body.len() - 1], 6059).is_err());
    }

    #[test]
    fn builds_before_6027_and_6058_skip_their_fields() {
        let mut b = Vec::new();
        cs(&mut b, b"A");
        cs(&mut b, b"c32");
        cs(&mut b, b"");
        cs(&mut b, b"stat");
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&[0, 0]); // empty blob
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&[0; 8]);
        cs(&mut b, b"");
        let l = read_leading(&b, 5999).unwrap();
        assert!(l.name_b.is_none());
        assert!(l.tail20.is_none());
    }

    #[test]
    fn string_must_end_inside_its_maximum() {
        // c32 may hold 31 characters plus the NUL; 32 characters with no NUL in time fail.
        let mut b = Vec::new();
        cs(&mut b, b"A");
        b.extend_from_slice(&[b'x'; 32]);
        b.push(0);
        assert!(read_leading(&b, 5999).is_err());
        let mut ok = Vec::new();
        cs(&mut ok, b"A");
        ok.extend_from_slice(&[b'x'; 31]);
        ok.push(0);
        cs(&mut ok, b"");
        cs(&mut ok, b"");
        ok.extend_from_slice(&[0; 8]);
        ok.extend_from_slice(&[0, 0]);
        ok.extend_from_slice(&[0; 8]);
        ok.extend_from_slice(&[0; 8]);
        cs(&mut ok, b"");
        assert!(read_leading(&ok, 5999).is_ok());
    }

    #[test]
    fn blob_longer_than_128_is_refused() {
        let mut long = Vec::new();
        cs(&mut long, b"A");
        cs(&mut long, b"B");
        cs(&mut long, b"c32");
        cs(&mut long, b"");
        cs(&mut long, b"stat");
        long.extend_from_slice(&[0; 8]);
        long.extend_from_slice(&129u16.to_le_bytes());
        long.extend_from_slice(&[0; 129]);
        assert!(read_leading(&long, 6059).is_err());
    }
}
