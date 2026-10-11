//! `SoundBank`: sounds by game path (for example `Sound\Interface\Click.wav`) from a stack of
//! MPQ archives. Like the game, an archive added later takes precedence over earlier ones, so
//! add `War3Patch.mpq` last. The archive bytes are kept in memory.

use std::collections::BTreeSet;

use crate::{mp3, wav, Pcm};

#[derive(Debug, thiserror::Error)]
pub enum SoundError {
    #[error(transparent)]
    Mpq(#[from] mpq::Error),
    #[error("no archive in the bank holds {0}")]
    NotFound(String),
    #[error("unsupported sound file type: {0}")]
    UnsupportedType(String),
    #[error(transparent)]
    Wav(#[from] wav::WavError),
    #[error(transparent)]
    Mp3(#[from] mp3::Mp3Error),
}

#[derive(Default)]
pub struct SoundBank {
    /// Archive files in load order; the last one has the highest priority.
    archives: Vec<Vec<u8>>,
}

/// Lower-cased extension of a game path, without the dot.
fn extension(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

impl SoundBank {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an archive (the whole file, read by the caller). The header and tables are checked
    /// now, so a bad file is rejected here rather than at the first lookup.
    pub fn add_archive(&mut self, bytes: Vec<u8>) -> Result<(), SoundError> {
        mpq::Archive::open(&bytes)?;
        self.archives.push(bytes);
        Ok(())
    }

    pub fn archive_count(&self) -> usize {
        self.archives.len()
    }

    /// True if some archive holds the file.
    pub fn contains(&self, path: &str) -> bool {
        self.archives.iter().rev().any(|bytes| {
            mpq::Archive::open(bytes)
                .map(|a| a.contains(path))
                .unwrap_or(false)
        })
    }

    /// Raw bytes of a file from the highest-priority archive that holds it.
    pub fn read_raw(&self, path: &str) -> Result<Vec<u8>, SoundError> {
        for bytes in self.archives.iter().rev() {
            let archive = mpq::Archive::open(bytes)?;
            if archive.contains(path) {
                return Ok(archive.read(path)?);
            }
        }
        Err(SoundError::NotFound(path.to_string()))
    }

    /// Loads and decodes a sound, choosing the decoder by extension (`.wav` or `.mp3`).
    pub fn load(&self, path: &str) -> Result<Pcm, SoundError> {
        let ext = extension(path);
        if ext != "wav" && ext != "mp3" {
            return Err(SoundError::UnsupportedType(path.to_string()));
        }
        let data = self.read_raw(path)?;
        match ext.as_str() {
            "wav" => Ok(wav::parse(&data)?),
            _ => Ok(mp3::decode(&data)?),
        }
    }

    /// Every `.wav` and `.mp3` name listed in the archives' `(listfile)`s, without duplicates
    /// (names compare case-insensitively, as in the archives). Archives without a listfile
    /// contribute nothing.
    pub fn sound_paths(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for bytes in &self.archives {
            let Ok(archive) = mpq::Archive::open(bytes) else {
                continue;
            };
            let Ok(names) = archive.listfile() else {
                continue;
            };
            for name in names {
                let ext = extension(&name);
                if (ext == "wav" || ext == "mp3") && seen.insert(name.to_ascii_lowercase()) {
                    out.push(name);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bytes_that_are_not_an_archive() {
        let mut bank = SoundBank::new();
        assert!(bank.add_archive(b"not an mpq".to_vec()).is_err());
        assert_eq!(bank.archive_count(), 0);
        assert!(!bank.contains("Sound\\x.wav"));
        assert!(matches!(
            bank.load("Sound\\x.wav"),
            Err(SoundError::NotFound(_))
        ));
    }

    #[test]
    fn unknown_extensions_are_refused_before_lookup() {
        let bank = SoundBank::new();
        assert!(matches!(
            bank.load("Sound\\x.ogg"),
            Err(SoundError::UnsupportedType(_))
        ));
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(extension("Sound\\A.WAV"), "wav");
        assert_eq!(extension("noext"), "");
    }
}
