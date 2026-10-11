use crate::Error;

/// Bounds-checked little-endian cursor. Every read reports the absolute offset on failure.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub(crate) fn offset(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.remaining() < n {
            return Err(Error::UnexpectedEof {
                offset: self.pos,
                wanted: n,
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
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

    /// Null-terminated string, decoded lossily as UTF-8. The terminator is consumed.
    pub(crate) fn cstr(&mut self) -> Result<String, Error> {
        let rest = &self.buf[self.pos..];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::UnterminatedString { offset: self.pos })?;
        let s = String::from_utf8_lossy(&rest[..len]).into_owned();
        self.pos += len + 1;
        Ok(s)
    }

    /// Reads a u32 element count and checks that `count * min_elem` bytes can still follow,
    /// so a corrupt count cannot trigger a huge allocation.
    pub(crate) fn count(&mut self, min_elem: usize, what: &'static str) -> Result<usize, Error> {
        let offset = self.pos;
        let n = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        match n.checked_mul(min_elem) {
            Some(bytes) if bytes <= self.remaining() => Ok(n),
            _ => Err(Error::CountTooLarge {
                what,
                count: n,
                offset,
            }),
        }
    }

    /// Consumes and returns everything that is left.
    pub(crate) fn rest(&mut self) -> &'a [u8] {
        let s = &self.buf[self.pos..];
        self.pos = self.buf.len();
        s
    }
}
