//! Parsers for the binary files inside a Warcraft III map archive (patch 1.26, TFT).
//!
//! Covered files: `war3map.w3e` (terrain), `war3map.w3i` (map info, format 25),
//! `war3map.doo` (doodads, version 8.11), `war3map.shd` (shadow grid) and
//! `war3map.wpm` (path map, `MP3W`).
//!
//! Layouts follow public community format notes. Fields marked "uncertain" in the
//! module docs are decoded on a best-effort basis and should be checked against real maps.
//! All parsing is bounds-checked: malformed input returns an [`Error`], never a panic.

mod doo;
mod reader;
mod shd;
mod units;
mod w3e;
mod w3i;
mod wpm;

pub use doo::{Doodad, Doodads, DroppedItem, SpecialDoodad};
pub use shd::ShadowMap;
pub use units::{PlacedUnit, PlacedUnits, UnitAbility, UnitItem};
pub use w3e::{Terrain, TilePoint};
pub use w3i::{Fog, Force, MapInfo, Player, Screen};
pub use wpm::PathMap;

use reader::Reader;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("{file}: bad magic {found:02x?}")]
    BadMagic { file: &'static str, found: [u8; 4] },
    #[error("{file}: unsupported {field} {found} (expected {expected})")]
    UnsupportedVersion {
        file: &'static str,
        field: &'static str,
        found: u32,
        expected: u32,
    },
    #[error("unexpected end of data at offset {offset:#x} (wanted {wanted} bytes)")]
    UnexpectedEof { offset: usize, wanted: usize },
    #[error("unterminated string at offset {offset:#x}")]
    UnterminatedString { offset: usize },
    #[error("{what} count {count} at offset {offset:#x} does not fit in the remaining data")]
    CountTooLarge {
        what: &'static str,
        count: usize,
        offset: usize,
    },
    #[error("{file}: invalid {field} value {value}")]
    InvalidValue {
        file: &'static str,
        field: &'static str,
        value: u32,
    },
    #[error("{file}: expected {expected} bytes, found {found}")]
    SizeMismatch {
        file: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("{file}: grid dimensions {width}x{height} overflow")]
    Dimensions {
        file: &'static str,
        width: u32,
        height: u32,
    },
}

fn check_magic(file: &'static str, found: [u8; 4], expected: &[u8; 4]) -> Result<(), Error> {
    if &found == expected {
        Ok(())
    } else {
        Err(Error::BadMagic { file, found })
    }
}

fn check_version(
    file: &'static str,
    field: &'static str,
    found: u32,
    expected: u32,
) -> Result<(), Error> {
    if found == expected {
        Ok(())
    } else {
        Err(Error::UnsupportedVersion {
            file,
            field,
            found,
            expected,
        })
    }
}

/// Reads one little-endian u32 and checks it is a known boolean value (0 or 1).
fn read_bool(r: &mut Reader<'_>, file: &'static str, field: &'static str) -> Result<bool, Error> {
    match r.u32()? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(Error::InvalidValue { file, field, value }),
    }
}
