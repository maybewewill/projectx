//! Decodes every `.wav` named in an archive's `(listfile)`, checks the RIFF/WAVE header, and
//! reports which sector codec masks the archive used (ADPCM, Huffman, zlib, ...).
//! Usage: `cargo run -p mpq --example wavcheck -- archive.mpq [more.mpq ...]`
use std::collections::BTreeMap;

use mpq::crypto::{decrypt_bytes, file_key};
use mpq::{
    Archive, FLAG_COMPRESS, FLAG_ENCRYPTED, FLAG_FIX_KEY, FLAG_SECTOR_CRC, FLAG_SINGLE_UNIT,
};

/// Minimal RIFF/WAVE header check: returns `(format_tag, channels, bits)` or an error text.
fn check_riff(data: &[u8]) -> Result<(u16, u16, u16), String> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err("no RIFF/WAVE signature".into());
    }
    let riff_size = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if riff_size + 8 != data.len() {
        return Err(format!(
            "RIFF size {} does not match file length {}",
            riff_size + 8,
            data.len()
        ));
    }
    let mut pos = 12usize;
    let mut fmt = None;
    let mut has_data = false;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            if body + 16 > data.len() {
                return Err("truncated fmt chunk".into());
            }
            let tag = u16::from_le_bytes([data[body], data[body + 1]]);
            let ch = u16::from_le_bytes([data[body + 2], data[body + 3]]);
            let bits = u16::from_le_bytes([data[body + 14], data[body + 15]]);
            fmt = Some((tag, ch, bits));
        } else if id == b"data" {
            if body.saturating_add(size) > data.len() {
                return Err(format!(
                    "data chunk claims {size} bytes, file has {}",
                    data.len() - body
                ));
            }
            has_data = true;
        }
        pos = body + size + (size & 1);
    }
    match (fmt, has_data) {
        (None, _) => Err("missing fmt chunk".into()),
        (_, false) => Err("missing data chunk".into()),
        (Some(f), true) => Ok(f),
    }
}

fn mask_name(m: u8) -> String {
    let mut parts = Vec::new();
    if m & 0x80 != 0 {
        parts.push("adpcm-stereo");
    }
    if m & 0x40 != 0 {
        parts.push("adpcm-mono");
    }
    if m & 0x08 != 0 {
        parts.push("dcl");
    }
    if m & 0x10 != 0 {
        parts.push("bzip2");
    }
    if m & 0x02 != 0 {
        parts.push("zlib");
    }
    if m & 0x01 != 0 {
        parts.push("huffman");
    }
    if m & 0x20 != 0 {
        parts.push("0x20");
    }
    if m & 0x04 != 0 {
        parts.push("0x04");
    }
    if parts.is_empty() {
        format!("{m:#04x}")
    } else {
        parts.join("+")
    }
}

/// First four bytes of a stored block, decrypted with the given key when the file is encrypted.
fn head4(raw: &[u8], key: Option<u32>) -> Option<[u8; 4]> {
    let mut h = [0u8; 4];
    let n = raw.len().min(4);
    h[..n].copy_from_slice(&raw[..n]);
    if let Some(k) = key {
        decrypt_bytes(&mut h, k);
    }
    Some(h)
}

/// Collects the mask bytes of every compressed sector of a file (raw sectors are skipped).
/// `bytes` is the whole archive file, `base` the offset of the MPQ header inside it.
fn sector_masks(archive: &Archive, bytes: &[u8], base: usize, name: &str) -> Option<Vec<u8>> {
    let idx = archive.find(name)?;
    let b = archive.block_table[idx];
    if b.flags & FLAG_COMPRESS == 0 {
        return Some(Vec::new());
    }
    let start = base + b.offset as usize;
    let raw = bytes.get(start..start + b.compressed_size as usize)?;
    let key = (b.flags & FLAG_ENCRYPTED != 0)
        .then(|| file_key(name, b.offset, b.file_size, b.flags & FLAG_FIX_KEY != 0));
    let size = b.file_size as usize;
    if b.flags & FLAG_SINGLE_UNIT != 0 {
        if raw.len() >= size {
            return Some(Vec::new()); // stored raw
        }
        return Some(vec![head4(raw, key)?[0]]);
    }
    let sector = archive.sector_size();
    let sectors = size.div_ceil(sector);
    let table_len = sectors + 1 + usize::from(b.flags & FLAG_SECTOR_CRC != 0);
    let mut table = raw.get(..table_len * 4)?.to_vec();
    if let Some(k) = key {
        decrypt_bytes(&mut table, k.wrapping_sub(1));
    }
    let offs: Vec<usize> = table
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()) as usize)
        .collect();
    let mut masks = Vec::new();
    for i in 0..sectors {
        let (s, e) = (offs[i], offs[i + 1]);
        let expected = sector.min(size - i * sector);
        if e.saturating_sub(s) >= expected {
            continue; // stored raw
        }
        let sub = raw.get(s..e)?;
        masks.push(head4(sub, key.map(|k| k.wrapping_add(i as u32)))?[0]);
    }
    Some(masks)
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: wavcheck <archive.mpq>...");
        std::process::exit(2);
    }
    for path in paths {
        let bytes = std::fs::read(&path).expect("read archive");
        let archive = match Archive::open(&bytes) {
            Ok(a) => a,
            Err(e) => {
                println!("{path}: open failed: {e}");
                continue;
            }
        };
        let base = bytes.windows(4).position(|w| w == b"MPQ\x1a").unwrap_or(0);
        let names = match archive.listfile() {
            Ok(n) => n,
            Err(e) => {
                println!("{path}: no listfile: {e}");
                continue;
            }
        };
        let wavs: Vec<&String> = names
            .iter()
            .filter(|n| n.to_ascii_lowercase().ends_with(".wav"))
            .collect();
        let (mut ok, mut mpq_fail, mut riff_fail) = (0usize, 0usize, 0usize);
        let mut formats: BTreeMap<String, usize> = BTreeMap::new();
        let mut codecs: BTreeMap<String, usize> = BTreeMap::new();
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in &wavs {
            if !archive.contains(name) {
                continue;
            }
            let mut set: Vec<String> = sector_masks(&archive, &bytes, base, name)
                .unwrap_or_default()
                .into_iter()
                .map(mask_name)
                .collect();
            set.sort();
            set.dedup();
            let codec_key = if set.is_empty() {
                "uncompressed".to_string()
            } else {
                set.join(" / ")
            };
            match archive.read(name) {
                Err(e) => {
                    mpq_fail += 1;
                    let key = format!("mpq: {}", e.to_string().split(':').next().unwrap_or(""));
                    errors.entry(key).or_default().push((*name).clone());
                }
                Ok(data) => match check_riff(&data) {
                    Err(msg) => {
                        riff_fail += 1;
                        errors
                            .entry(format!("riff: {msg}"))
                            .or_default()
                            .push((*name).clone());
                    }
                    Ok((tag, ch, bits)) => {
                        ok += 1;
                        *codecs.entry(codec_key).or_default() += 1;
                        *formats
                            .entry(format!("tag {tag:#06x} ch {ch} bits {bits}"))
                            .or_default() += 1;
                    }
                },
            }
        }
        println!(
            "{path}: {} wav names, {ok} OK, {mpq_fail} MPQ decode failed, {riff_fail} RIFF failed",
            wavs.len()
        );
        for (fmt, n) in &formats {
            println!("  format {fmt}: {n}");
        }
        for (codec, n) in &codecs {
            println!("  codec {codec}: {n}");
        }
        for (kind, files) in &errors {
            println!(
                "  {kind}: {} (e.g. {})",
                files.len(),
                files[..files.len().min(3)].join(", ")
            );
        }
    }
}
