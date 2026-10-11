//! Parses every `.blp` in each archive and decodes all mip levels, reporting failures grouped
//! by error kind.
//!
//! Names come from the archive's own `(listfile)`. An archive without one (War3Patch.mpq)
//! uses the union of all listfiles given on the command line and only counts names it holds.
//! Usage: `cargo run -p blp --example blpcheck -- archive.mpq [more.mpq ...]`
use std::collections::{BTreeMap, BTreeSet};

use blp::{Blp, ContentType};

/// Error kind: the message with every digit replaced by `#`, so offsets and sizes group together.
fn kind_of(message: &str) -> String {
    message
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

/// Parses a texture and decodes every mip. Returns the content type on success.
fn check(bytes: &[u8]) -> Result<ContentType, String> {
    let blp = Blp::parse(bytes).map_err(|e| kind_of(&e.to_string()))?;
    let h = &blp.header;
    if std::env::var_os("BLPCHECK_STATS").is_some() {
        // Raw header values, plus for JPEG with alpha: the bytes after the JPEG end marker.
        eprintln!(
            "STAT content={:?} alpha_bits={} mips={} has_mipmaps={} extra={}",
            h.content,
            h.alpha_bits,
            blp.mip_count(),
            h.has_mipmaps,
            h.extra
        );
        if h.content == ContentType::Paletted {
            // Stored mip size minus (indices + packed alpha). Zero means the layout matches.
            for level in 0..blp.mip_count() {
                let (w, hgt) = blp.mip_dimensions(level);
                let px = (w * hgt) as i64;
                let alpha = match h.alpha_bits {
                    1 => (px + 7) / 8,
                    4 => (px + 1) / 2,
                    8 => px,
                    _ => 0,
                };
                let delta = i64::from(h.mip_sizes[level]) - (px + alpha);
                eprintln!("STAT paletted_mip_size_delta={delta} alpha_bits={}", h.alpha_bits);
            }
        }
        if h.content == ContentType::Jpeg {
            // Component count (Nf) of the first SOF marker in the shared header and in each mip.
            let hl = u32::from_le_bytes([bytes[0x9C], bytes[0x9D], bytes[0x9E], bytes[0x9F]]) as usize;
            let shared = &bytes[0xA0..0xA0 + hl];
            let nf = |data: &[u8]| -> Option<u8> {
                let pos = data.windows(2).position(|w| w[0] == 0xFF && (w[1] == 0xC0 || w[1] == 0xC1 || w[1] == 0xC2))?;
                data.get(pos + 9).copied()
            };
            eprintln!("STAT jpeg_shared_nf={:?} alpha_bits={}", nf(shared), h.alpha_bits);
            for level in 0..blp.mip_count() {
                let off = h.mip_offsets[level] as usize;
                let size = h.mip_sizes[level] as usize;
                eprintln!("STAT jpeg_mip_nf={:?} alpha_bits={}", nf(&bytes[off..off + size]), h.alpha_bits);
            }
            for level in 0..blp.mip_count() {
                let off = h.mip_offsets[level] as usize;
                let size = h.mip_sizes[level] as usize;
                let mip = &bytes[off..off + size];
                if let Some(pos) = mip.windows(2).rposition(|w| w == [0xFF, 0xD9]) {
                    let trailing = mip.len() - (pos + 2);
                    eprintln!("STAT jpeg_mip0_trailing_after_eoi={} alpha_bits={}", trailing.min(1), h.alpha_bits);
                    if level == 0 {
                        eprintln!("STAT jpeg_trailing_bytes={} alpha_bits={}", trailing, h.alpha_bits);
                    }
                } else {
                    eprintln!("STAT jpeg_no_eoi alpha_bits={}", h.alpha_bits);
                }
            }
        }
    }
    for level in 0..blp.mip_count() {
        blp.mip_rgba(level)
            .map_err(|e| format!("mip {}", kind_of(&format!("{level}: {e}"))))?;
    }
    Ok(h.content)
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: blpcheck <archive.mpq>...");
        std::process::exit(2);
    }

    let mut union = BTreeSet::new();
    for path in &paths {
        let bytes = std::fs::read(path).expect("read archive");
        if let Ok(archive) = mpq::Archive::open(&bytes) {
            if let Ok(names) = archive.listfile() {
                union.extend(names);
            }
        }
    }

    for path in paths {
        let bytes = std::fs::read(&path).expect("read archive");
        let archive = match mpq::Archive::open(&bytes) {
            Ok(a) => a,
            Err(e) => {
                println!("{path}: open failed: {e}");
                continue;
            }
        };
        let (names, from_union) = match archive.listfile() {
            Ok(n) => (n, false),
            Err(_) => (union.iter().cloned().collect(), true),
        };
        let (mut jpeg, mut paletted, mut total, mut missing) = (0usize, 0usize, 0usize, 0usize);
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in names.iter().filter(|n| n.to_ascii_lowercase().ends_with(".blp")) {
            if !archive.contains(name) {
                missing += 1;
                continue;
            }
            total += 1;
            let data = match archive.read(name) {
                Ok(d) => d,
                Err(e) => {
                    let kind = format!("mpq: {}", kind_of(&e.to_string()));
                    errors.entry(kind).or_default().push(name.clone());
                    continue;
                }
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&data)))
                .unwrap_or_else(|_| Err("PANIC".to_string()));
            if std::env::var_os("BLPCHECK_NAMES").is_some() {
                if let Ok(b) = Blp::parse(&data) {
                    eprintln!("NAME alpha={} content={:?} {name}", b.header.alpha_bits, b.header.content);
                }
            }
            match outcome {
                Ok(ContentType::Jpeg) => jpeg += 1,
                Ok(ContentType::Paletted) => paletted += 1,
                Err(kind) => errors.entry(kind).or_default().push(name.clone()),
            }
        }
        let ok = jpeg + paletted;
        let failed: usize = errors.values().map(Vec::len).sum();
        let source = if from_union { "union listfile" } else { "own listfile" };
        println!(
            "{path}: {total} .blp files ({source}), {ok} ok ({jpeg} JPEG, {paletted} paletted), {failed} failed, {missing} names not in archive"
        );
        for (kind, files) in &errors {
            println!("  {kind}: {} (e.g. {})", files.len(), files[..files.len().min(3)].join(", "));
        }
    }
}
