//! Minimal PE32 reader: just enough to copy bytes out of the image at a
//! virtual address. No section is ever parsed beyond its file-backed range,
//! so zero-filled (BSS-like) memory is never mistaken for data.

/// Reads `len` bytes at virtual address `va` from a PE32 file image.
///
/// Returns a reason string on malformed input or when the range is not backed
/// by file data.
pub(crate) fn read_va(file: &[u8], va: u32, len: usize) -> Result<&[u8], &'static str> {
    let image_base = image_base(file)?;
    let rva = va
        .checked_sub(image_base)
        .ok_or("address below image base")?;
    let start = find_file_offset(file, rva, len)?;
    Ok(&file[start..start + len])
}

fn u16_at(data: &[u8], off: usize) -> Result<u16, &'static str> {
    let b = data.get(off..off + 2).ok_or("truncated PE header")?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], off: usize) -> Result<u32, &'static str> {
    let b = data.get(off..off + 4).ok_or("truncated PE header")?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Offset of the PE signature, checked and returned.
fn pe_offset(file: &[u8]) -> Result<usize, &'static str> {
    if u16_at(file, 0)? != u16::from_le_bytes(*b"MZ") {
        return Err("missing MZ signature");
    }
    let off = u32_at(file, 0x3C)? as usize;
    if u32_at(file, off)? != u32::from_le_bytes(*b"PE\0\0") {
        return Err("missing PE signature");
    }
    Ok(off)
}

fn image_base(file: &[u8]) -> Result<u32, &'static str> {
    let pe = pe_offset(file)?;
    let opt = pe + 24;
    match u16_at(file, opt)? {
        0x010B => u32_at(file, opt + 28),
        0x020B => Err("PE32+ images are not supported"),
        _ => Err("unknown optional header magic"),
    }
}

/// File offset that backs `rva .. rva + len`, provided the whole range lies in
/// the file-backed part of one section.
fn find_file_offset(file: &[u8], rva: u32, len: usize) -> Result<usize, &'static str> {
    let pe = pe_offset(file)?;
    let count = u16_at(file, pe + 6)? as usize;
    let opt_size = u16_at(file, pe + 20)? as usize;
    let table = pe + 24 + opt_size;
    for i in 0..count {
        let sec = table + 40 * i;
        let vaddr = u32_at(file, sec + 12)?;
        let raw_size = u32_at(file, sec + 16)?;
        let raw_ptr = u32_at(file, sec + 20)? as usize;
        let end = vaddr.checked_add(raw_size).ok_or("section overflows")?;
        let range_end = rva.checked_add(u32::try_from(len).map_err(|_| "length too large")?);
        let Some(range_end) = range_end else {
            return Err("address range overflows");
        };
        if rva >= vaddr && range_end <= end {
            let start = raw_ptr + (rva - vaddr) as usize;
            if start + len > file.len() {
                return Err("section data beyond end of file");
            }
            return Ok(start);
        }
    }
    Err("address is not backed by file data")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a tiny PE32 image with one section at RVA 0x1000 whose raw bytes
    /// are `payload`, based at 0x40000000.
    fn tiny_pe(payload: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 0x400 + payload.len()];
        f[0..2].copy_from_slice(b"MZ");
        f[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        f[0x80..0x84].copy_from_slice(b"PE\0\0");
        f[0x84..0x86].copy_from_slice(&0x14Cu16.to_le_bytes()); // machine
        f[0x86..0x88].copy_from_slice(&1u16.to_le_bytes()); // one section
        f[0x94..0x96].copy_from_slice(&0xE0u16.to_le_bytes()); // optional header size
        let opt = 0x98;
        f[opt..opt + 2].copy_from_slice(&0x010Bu16.to_le_bytes());
        f[opt + 28..opt + 32].copy_from_slice(&0x4000_0000u32.to_le_bytes());
        let sec = opt + 0xE0;
        f[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        f[sec + 16..sec + 20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        f[sec + 20..sec + 24].copy_from_slice(&0x400u32.to_le_bytes());
        f[0x400..].copy_from_slice(payload);
        f
    }

    #[test]
    fn reads_bytes_at_virtual_address() {
        let payload: Vec<u8> = (0..64u8).collect();
        let file = tiny_pe(&payload);
        let got = read_va(&file, 0x4000_1010, 16).unwrap();
        assert_eq!(got, &payload[0x10..0x20]);
    }

    #[test]
    fn rejects_range_past_raw_data() {
        let file = tiny_pe(&[1, 2, 3, 4]);
        assert!(read_va(&file, 0x4000_1000, 8).is_err());
        assert!(read_va(&file, 0x3FFF_0000, 1).is_err());
    }

    #[test]
    fn rejects_non_pe() {
        assert!(read_va(b"not a dll at all, just text", 0x1000, 4).is_err());
    }
}
