use crate::{fourcc, Error, Tag};

/// Bounds-checked little-endian cursor over a byte slice.
///
/// `base` is the absolute file offset of the first byte, so errors report file offsets
/// even inside nested records.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    base: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8], base: usize) -> Self {
        Self { buf, pos: 0, base }
    }

    pub(crate) fn offset(&self) -> usize {
        self.base + self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.remaining() < n {
            return Err(Error::UnexpectedEof {
                offset: self.offset(),
                wanted: n,
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Splits off the next `n` bytes as an independent reader.
    pub(crate) fn sub(&mut self, n: usize) -> Result<Reader<'a>, Error> {
        let offset = self.offset();
        Ok(Reader::new(self.bytes(n)?, offset))
    }

    /// Reads an inclusive-size record (the size field counts itself) and returns a reader
    /// over its body. The size is checked against the bytes left in the parent.
    pub(crate) fn record(&mut self, what: &'static str) -> Result<Reader<'a>, Error> {
        let offset = self.offset();
        let size = self.u32()? as usize;
        if size < 4 || size - 4 > self.remaining() {
            return Err(Error::RecordSize { what, offset, size });
        }
        self.sub(size - 4)
    }

    /// The next four bytes without consuming them, if at least four bytes remain.
    pub(crate) fn peek_tag(&self) -> Option<Tag> {
        self.buf
            .get(self.pos..self.pos + 4)
            .map(|s| [s[0], s[1], s[2], s[3]])
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    pub(crate) fn tag(&mut self, expected: &Tag) -> Result<(), Error> {
        let offset = self.offset();
        let found = self.array::<4>()?;
        if &found != expected {
            return Err(Error::BadTag {
                expected: fourcc(*expected),
                found: fourcc(found),
                offset,
            });
        }
        Ok(())
    }

    pub(crate) fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub(crate) fn vec2(&mut self) -> Result<[f32; 2], Error> {
        Ok([self.f32()?, self.f32()?])
    }

    pub(crate) fn vec3(&mut self) -> Result<[f32; 3], Error> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    pub(crate) fn quat(&mut self) -> Result<[f32; 4], Error> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }

    /// Fixed-size, NUL-padded string. Stops at the first NUL; decoded lossily.
    pub(crate) fn fixed_str(&mut self, n: usize) -> Result<String, Error> {
        let raw = self.bytes(n)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
    }

    /// Reads a u32 element count and checks that `count * min_elem` bytes can still follow,
    /// so a corrupt count cannot trigger a huge allocation.
    pub(crate) fn count(&mut self, min_elem: usize, what: &'static str) -> Result<usize, Error> {
        let offset = self.offset();
        let n = self.u32()? as usize;
        match n.checked_mul(min_elem) {
            Some(bytes) if bytes <= self.remaining() => Ok(n),
            _ => Err(Error::CountTooLarge {
                what,
                count: n,
                offset,
            }),
        }
    }

    /// A tag, a count, then `count` elements read with `f`.
    pub(crate) fn tagged_array<T>(
        &mut self,
        tag: &Tag,
        elem: usize,
        what: &'static str,
        mut f: impl FnMut(&mut Self) -> Result<T, Error>,
    ) -> Result<Vec<T>, Error> {
        self.tag(tag)?;
        let n = self.count(elem, what)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }
}
