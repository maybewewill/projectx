//! Script sources: `Scripts\common.j` and `Scripts\Blizzard.j` from the user's War3.mpq and
//! War3x.mpq, and the map's `war3map.j`.
//!
//! Archives are searched in the order given, and a later archive overrides an earlier one, so pass
//! `War3.mpq` then `War3x.mpq` (the TFT archive overrides RoC files). Sources are decoded as UTF-8
//! with lossy replacement; the 1.26a scripts are ASCII apart from comments as far as is known.

use mpq::Archive;

use crate::Error;

/// The archive path of the shared script (`common.j`).
pub const COMMON_J: &str = "Scripts\\common.j";
/// The archive path of the Blizzard script (`blizzard.j`).
pub const BLIZZARD_J: &str = "Scripts\\Blizzard.j";

/// Reads `name` from the last archive that has it, in the given order.
fn read_last(archives: &[&[u8]], name: &str) -> Result<Option<String>, Error> {
    let mut found = None;
    for (index, bytes) in archives.iter().enumerate() {
        let archive = Archive::open(bytes).map_err(|source| Error::Archive { index, source })?;
        match archive.read(name) {
            Ok(data) => found = Some(String::from_utf8_lossy(&data).into_owned()),
            Err(mpq::Error::NotFound(_)) => {}
            Err(source) => return Err(Error::Archive { index, source }),
        }
    }
    Ok(found)
}

/// Returns `(common.j, blizzard.j)` from the archives (later archives override earlier ones).
pub fn scripts_from_archives(archives: &[&[u8]]) -> Result<(String, String), Error> {
    let common = read_last(archives, COMMON_J)?.ok_or_else(|| Error::MissingFile {
        what: "common.j",
        name: COMMON_J.to_string(),
    })?;
    let blizzard = read_last(archives, BLIZZARD_J)?.ok_or_else(|| Error::MissingFile {
        what: "blizzard.j",
        name: BLIZZARD_J.to_string(),
    })?;
    Ok((common, blizzard))
}
