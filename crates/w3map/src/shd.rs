//! `war3map.shd`: shadow grid.
//!
//! The file has no header, so the grid size comes from the terrain (`war3map.w3e`).
//! It is parsed here as a plain byte grid: exactly `width * height` bytes, row-major.
//! UNCERTAIN: the on-disk shadow format may be bit-packed rather than one byte per cell.
//! Verify on a real map before relying on the cell values.

use crate::Error;

const FILE: &str = "war3map.shd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowMap {
    pub width: u32,
    pub height: u32,
    /// `width * height` cells, row-major.
    pub cells: Vec<u8>,
}

impl ShadowMap {
    pub fn parse(data: &[u8], width: u32, height: u32) -> Result<Self, Error> {
        let expected = grid_len(FILE, width, height)?;
        if data.len() != expected {
            return Err(Error::SizeMismatch {
                file: FILE,
                expected,
                found: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            cells: data.to_vec(),
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

/// Byte count of a `width x height` grid, or an error when it overflows `usize`.
pub(crate) fn grid_len(file: &'static str, width: u32, height: u32) -> Result<usize, Error> {
    (width as usize)
        .checked_mul(height as usize)
        .ok_or(Error::Dimensions {
            file,
            width,
            height,
        })
}
