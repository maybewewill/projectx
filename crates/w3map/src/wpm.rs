//! `war3map.wpm`: path (pathing) map.
//!
//! Layout (little-endian): `"MP3W"`, version u32 (= 0), width u32, height u32, then
//! `width * height` bytes, row-major.
//! UNCERTAIN: the version value and the meaning of each cell byte are from community notes
//! and were not checked against real maps.

use crate::reader::Reader;
use crate::shd::grid_len;
use crate::{check_magic, check_version, Error};

const FILE: &str = "war3map.wpm";
const MAGIC: [u8; 4] = *b"MP3W";
const VERSION: u32 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    pub width: u32,
    pub height: u32,
    /// `width * height` cells, row-major.
    pub cells: Vec<u8>,
}

impl PathMap {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        check_magic(FILE, r.array()?, &MAGIC)?;
        check_version(FILE, "version", r.u32()?, VERSION)?;
        let width = r.u32()?;
        let height = r.u32()?;
        let expected = grid_len(FILE, width, height)?;
        if r.remaining() != expected {
            return Err(Error::SizeMismatch {
                file: FILE,
                expected,
                found: r.remaining(),
            });
        }
        Ok(Self {
            width,
            height,
            cells: r.rest().to_vec(),
        })
    }

    pub fn get(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }
}
