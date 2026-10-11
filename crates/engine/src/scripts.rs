//! Script sources: `Scripts\common.j` and `Scripts\Blizzard.j` from the user's War3.mpq and
//! War3x.mpq, the map's `war3map.j`, and the AI scripts (`Scripts\common.ai` and the race scripts).
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

/// The archive path of the common AI script (`common.ai`).
pub const COMMON_AI: &str = "Scripts\\common.ai";
/// The race AI scripts that the archives are searched for, by file name (lower case).
pub const RACE_AI_SCRIPTS: [&str; 4] = ["human.ai", "orc.ai", "undead.ai", "elf.ai"];

/// The AI sources read from the archives: `common.ai` (if the archives have it) and the race scripts
/// that they have, as `(file name, source)` in [`RACE_AI_SCRIPTS`] order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiSources {
    pub common: Option<String>,
    pub scripts: Vec<(String, String)>,
}

/// Reads `Scripts\common.ai` and the race AI scripts from the archives (later archives override
/// earlier ones). A missing file is not an error: it is left out.
pub fn ai_scripts_from_archives(archives: &[&[u8]]) -> Result<AiSources, Error> {
    let common = read_last(archives, COMMON_AI)?;
    let mut scripts = Vec::new();
    for name in RACE_AI_SCRIPTS {
        if let Some(source) = read_last(archives, &format!("Scripts\\{name}"))? {
            scripts.push((name.to_string(), source));
        }
    }
    Ok(AiSources { common, scripts })
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
