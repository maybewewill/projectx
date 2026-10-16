//! Archive bytes without reading the whole file: a memory map on native targets (only the pages that are touched
//! become resident: the hash/block tables and the files that are read), an owned buffer everywhere else (wasm32,
//! tests, archives that arrive from the network).

use std::ops::Deref;

/// The bytes of an archive (or any file), mapped or owned. Derefs to `[u8]`, so `Archive::open(&bytes)` works.
#[derive(Clone)]
pub enum FileBytes {
    Owned(Vec<u8>),
    #[cfg(not(target_arch = "wasm32"))]
    Mapped(std::sync::Arc<memmap2::Mmap>),
}

impl FileBytes {
    /// Maps the file read-only (native) or reads it (wasm32). The file must not be modified while mapped.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() == 0 {
            return Ok(FileBytes::Owned(Vec::new()));
        }
        // SAFETY: read-only map of a game archive; the engine never writes these files.
        let map = unsafe { memmap2::Mmap::map(&file)? };
        Ok(FileBytes::Mapped(std::sync::Arc::new(map)))
    }

    /// Reads the file into memory (wasm32 has no mapping).
    #[cfg(target_arch = "wasm32")]
    pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        std::fs::read(path).map(FileBytes::Owned)
    }
}

impl Deref for FileBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            FileBytes::Owned(v) => v,
            #[cfg(not(target_arch = "wasm32"))]
            FileBytes::Mapped(m) => m,
        }
    }
}

impl From<Vec<u8>> for FileBytes {
    fn from(v: Vec<u8>) -> Self {
        FileBytes::Owned(v)
    }
}

impl std::fmt::Debug for FileBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileBytes({} bytes)", self.len())
    }
}
