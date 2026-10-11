//! Parsers for Warcraft III text data: SYLK (`.slk`) tables and profile
//! (`.txt`, INI-like) files, plus layered merging of several files.
//!
//! Inputs are text in UTF-8 or Windows-1252, with LF or CRLF line endings.
//! The `parse_bytes` variants decode raw bytes first.

pub mod combat;
pub mod misc;
pub mod profile;
pub mod real;
pub mod slk;
pub mod unittype;

pub use combat::{ArmorType, DamageType, Defense, UnitCombat, UnitCombatData, Weapon};
pub use misc::MiscData;
pub use profile::Profile;
pub use slk::{Row, Slk, Value};

/// Error produced while parsing a data file.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// A malformed SLK record at the given 1-based line.
    #[error("line {line}: SLK: {msg}")]
    Slk { line: usize, msg: String },
    /// A malformed profile line at the given 1-based line.
    #[error("line {line}: profile: {msg}")]
    Profile { line: usize, msg: String },
    /// Two SLK rows share the same row key within one file.
    #[error("duplicate SLK row key {key:?}")]
    DuplicateRowKey { key: String },
}

/// Windows-1252 code points for bytes `0x80..=0x9F`.
/// Undefined slots map to the C1 control characters, as WHATWG decoders do.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

/// Decodes game text: strips a UTF-8 BOM, uses UTF-8 when valid and
/// Windows-1252 otherwise. Never fails.
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes
        .strip_prefix(&[0xEF_u8, 0xBB, 0xBF][..])
        .unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => CP1252_HIGH[usize::from(b - 0x80)],
                _ => char::from(b),
            })
            .collect(),
    }
}
