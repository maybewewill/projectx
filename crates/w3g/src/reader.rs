use crate::Error;

/// Little-endian byte cursor that reports the absolute offset on failure.
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
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

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        Ok(self.bytes(N)?.try_into().unwrap())
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// Null-terminated byte string (terminator consumed, not returned).
    pub fn cstr(&mut self) -> Result<&'a [u8], Error> {
        let rest = &self.buf[self.pos..];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::UnterminatedString { offset: self.pos })?;
        let s = &rest[..len];
        self.pos += len + 1;
        Ok(s)
    }
}
