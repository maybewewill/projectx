//! Synthetic BLP1 files: paletted content at every alpha depth with mip-maps, JPEG content
//! wrapped from a `jpeg-encoder` image, and header error cases. No game assets are used.

use blp::{Blp, ContentType, Error, HEADER_LEN, MAX_MIPS, PALETTE_LEN};

/// BGRA palette entry for index `i`. The alpha byte is deliberately non-255 to check that it
/// is ignored.
fn palette_entry(i: u8) -> [u8; 4] {
    [i, i.wrapping_mul(3), 255 - i, 0x80]
}

/// Packs logical alpha values into the on-disk layout for `bits` per pixel.
/// `values[i]` must fit in `bits` bits (for bits == 1, 0 or 1).
fn pack_alpha(values: &[u8], bits: u32) -> Vec<u8> {
    match bits {
        0 => Vec::new(),
        8 => values.to_vec(),
        // Low nibble first.
        4 => values
            .chunks(2)
            .map(|c| c[0] | (c.get(1).copied().unwrap_or(0) << 4))
            .collect(),
        // LSB first.
        1 => values
            .chunks(8)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0u8, |acc, (bit, &v)| acc | ((v & 1) << bit))
            })
            .collect(),
        _ => unreachable!("invalid alpha depth"),
    }
}

/// Logical alpha value for pixel `i` at depth `bits`, as stored.
fn alpha_value(i: usize, bits: u32) -> u8 {
    match bits {
        0 => 0,
        1 => (i % 2) as u8,
        4 => (i % 16) as u8,
        8 => (i * 37 % 256) as u8,
        _ => unreachable!(),
    }
}

/// Expected decoded alpha for a stored logical value.
fn expected_alpha(v: u8, bits: u32) -> u8 {
    match bits {
        0 => 255,
        1 => {
            if v != 0 {
                255
            } else {
                0
            }
        }
        4 => v * 17,
        8 => v,
        _ => unreachable!(),
    }
}

fn mip_dims(w: u32, h: u32, level: u32) -> (u32, u32) {
    ((w >> level).max(1), (h >> level).max(1))
}

/// Builds a paletted BLP1 file with the given mip levels (level 0 first).
fn build_paletted(w: u32, h: u32, alpha_bits: u32, mip_count: usize) -> Vec<u8> {
    let mut header = vec![0u8; HEADER_LEN];
    header[0..4].copy_from_slice(b"BLP1");
    header[4..8].copy_from_slice(&1u32.to_le_bytes());
    header[8..12].copy_from_slice(&alpha_bits.to_le_bytes());
    header[12..16].copy_from_slice(&w.to_le_bytes());
    header[16..20].copy_from_slice(&h.to_le_bytes());
    header[24..28].copy_from_slice(&u32::from(mip_count > 1).to_le_bytes());

    let mut palette = Vec::with_capacity(PALETTE_LEN);
    for i in 0..=255u8 {
        palette.extend_from_slice(&palette_entry(i));
    }

    let mut body = Vec::new();
    let mut offsets = [0u32; MAX_MIPS];
    let mut sizes = [0u32; MAX_MIPS];
    let mut file_pos = (HEADER_LEN + PALETTE_LEN) as u32;
    for level in 0..mip_count {
        let (mw, mh) = mip_dims(w, h, level as u32);
        let n = (mw * mh) as usize;
        let indices: Vec<u8> = (0..n).map(|j| (j * 53 + level * 7) as u8).collect();
        let alpha: Vec<u8> = (0..n).map(|j| alpha_value(j + level, alpha_bits)).collect();
        let mut mip = indices;
        mip.extend(pack_alpha(&alpha, alpha_bits));
        offsets[level] = file_pos;
        sizes[level] = mip.len() as u32;
        file_pos += mip.len() as u32;
        body.extend(mip);
    }
    for (i, v) in offsets.iter().enumerate() {
        header[0x1C + 4 * i..0x20 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    for (i, v) in sizes.iter().enumerate() {
        header[0x5C + 4 * i..0x60 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }

    let mut file = header;
    file.extend(palette);
    file.extend(body);
    file
}

/// The RGBA pixels the synthetic file should decode to, computed independently of the crate.
fn expected_rgba(w: u32, h: u32, alpha_bits: u32, level: u32) -> Vec<u8> {
    let (mw, mh) = mip_dims(w, h, level);
    let n = (mw * mh) as usize;
    let mut out = Vec::with_capacity(n * 4);
    for j in 0..n {
        let index = (j * 53 + level as usize * 7) as u8;
        let entry = palette_entry(index);
        let a = expected_alpha(alpha_value(j + level as usize, alpha_bits), alpha_bits);
        out.extend_from_slice(&[entry[2], entry[1], entry[0], a]);
    }
    out
}

#[test]
fn paletted_all_alpha_depths_with_mipmaps_match_exactly() {
    for &bits in &[0u32, 1, 4, 8] {
        // 5x3 is deliberately not a power of two, so odd pixel counts hit the packing edges.
        let file = build_paletted(5, 3, bits, 4);
        let blp = Blp::parse(&file).unwrap_or_else(|e| panic!("alpha {bits}: {e}"));
        assert_eq!(blp.header.content, ContentType::Paletted);
        assert_eq!(blp.header.alpha_bits, bits);
        assert_eq!((blp.header.width, blp.header.height), (5, 3));
        assert!(blp.header.has_mipmaps);
        assert_eq!(blp.mip_count(), 4);

        for level in 0..4u32 {
            let (w, h, rgba) = blp.mip_rgba(level as usize).unwrap();
            let (ew, eh) = mip_dims(5, 3, level);
            assert_eq!((w, h), (ew, eh), "alpha {bits}, level {level}");
            assert_eq!(
                rgba,
                expected_rgba(5, 3, bits, level),
                "alpha {bits}, level {level}"
            );
        }
    }
}

#[test]
fn four_bit_alpha_packing_is_low_nibble_first() {
    // 2x2 image: indices 0..3 use palette entries 0..3, alpha nibbles 1, 2, 3, 4.
    let mut file = build_paletted(2, 2, 4, 1);
    let mip_start = HEADER_LEN + PALETTE_LEN;
    // Indices then two alpha bytes: (1 | 2 << 4), (3 | 4 << 4).
    file.truncate(mip_start);
    file.extend_from_slice(&[0, 1, 2, 3]);
    file.extend_from_slice(&[0x21, 0x43]);
    file[0x1C..0x20].copy_from_slice(&(mip_start as u32).to_le_bytes());
    file[0x5C..0x60].copy_from_slice(&6u32.to_le_bytes());

    let blp = Blp::parse(&file).unwrap();
    let (w, h, rgba) = blp.mip_rgba(0).unwrap();
    assert_eq!((w, h), (2, 2));
    let expected: Vec<u8> = [(0u8, 17u8), (1, 34), (2, 51), (3, 68)]
        .iter()
        .flat_map(|&(i, a)| {
            let e = palette_entry(i);
            [e[2], e[1], e[0], a]
        })
        .collect();
    assert_eq!(rgba, expected);
}

#[test]
fn one_bit_alpha_packing_is_lsb_first() {
    // 3x3 image, 9 pixels -> 2 alpha bytes. Alpha bits (pixel 0..8): 1,0,1,1,0,0,0,0,1.
    let mut file = build_paletted(3, 3, 1, 1);
    let mip_start = HEADER_LEN + PALETTE_LEN;
    file.truncate(mip_start);
    file.extend_from_slice(&[7u8; 9]);
    // byte0 bits 0..7 = 1,0,1,1,0,0,0,0 -> 0b0000_1101 = 0x0D
    // byte1 bit 0 = pixel 8 = 1 -> 0x01
    file.extend_from_slice(&[0x0D, 0x01]);
    file[0x1C..0x20].copy_from_slice(&(mip_start as u32).to_le_bytes());
    file[0x5C..0x60].copy_from_slice(&11u32.to_le_bytes());

    let blp = Blp::parse(&file).unwrap();
    let (_, _, rgba) = blp.mip_rgba(0).unwrap();
    let alphas: Vec<u8> = rgba.chunks_exact(4).map(|p| p[3]).collect();
    assert_eq!(alphas, vec![255, 0, 255, 255, 0, 0, 0, 0, 255]);
    let e = palette_entry(7);
    assert!(rgba.chunks_exact(4).all(|p| p[..3] == [e[2], e[1], e[0]]));
}

#[test]
fn no_mipmaps_reports_one_level() {
    let mut file = build_paletted(4, 4, 8, 1);
    file[24..28].copy_from_slice(&0u32.to_le_bytes());
    let blp = Blp::parse(&file).unwrap();
    assert!(!blp.header.has_mipmaps);
    assert_eq!(blp.mip_count(), 1);
    assert!(blp.mip_rgba(0).is_ok());
    assert!(matches!(
        blp.mip_rgba(1),
        Err(Error::NoSuchMip { level: 1, count: 1 })
    ));
}

#[test]
fn mip_out_of_bounds_is_an_error() {
    let mut file = build_paletted(4, 4, 0, 1);
    // Claim a size that runs past the end of the file.
    file[0x5C..0x60].copy_from_slice(&1000u32.to_le_bytes());
    let blp = Blp::parse(&file).unwrap();
    assert!(matches!(
        blp.mip_rgba(0),
        Err(Error::MipOutOfBounds { level: 0, .. })
    ));
}

#[test]
fn blp2_is_reported_as_unsupported() {
    let mut file = build_paletted(4, 4, 8, 1);
    file[0..4].copy_from_slice(b"BLP2");
    assert!(matches!(Blp::parse(&file), Err(Error::Unsupported(_))));
}

#[test]
fn bad_magic_and_header_fields_are_rejected() {
    let mut file = build_paletted(4, 4, 8, 1);
    file[0..4].copy_from_slice(b"PNG\x89");
    assert!(matches!(Blp::parse(&file), Err(Error::BadMagic(_))));

    let mut file = build_paletted(4, 4, 8, 1);
    file[8..12].copy_from_slice(&3u32.to_le_bytes());
    assert!(matches!(Blp::parse(&file), Err(Error::InvalidAlphaBits(3))));

    let mut file = build_paletted(4, 4, 8, 1);
    file[4..8].copy_from_slice(&7u32.to_le_bytes());
    assert!(matches!(
        Blp::parse(&file),
        Err(Error::InvalidContentType(7))
    ));

    let mut file = build_paletted(4, 4, 8, 1);
    file[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        Blp::parse(&file),
        Err(Error::InvalidDimensions { .. })
    ));
}

#[test]
fn truncated_files_are_rejected() {
    let file = build_paletted(4, 4, 8, 1);
    assert!(matches!(
        Blp::parse(&file[..HEADER_LEN - 1]),
        Err(Error::Truncated { .. })
    ));
    assert!(matches!(
        Blp::parse(&file[..HEADER_LEN + 10]),
        Err(Error::Truncated { .. })
    ));
}

/// Splits a baseline JPEG into the shared header (everything before the SOS marker) and the
/// rest (SOS segment, entropy-coded data, EOI).
fn split_jpeg(jpeg: &[u8]) -> (&[u8], &[u8]) {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "not a JPEG");
    let mut pos = 2;
    loop {
        assert_eq!(jpeg[pos], 0xFF, "expected a marker");
        let marker = jpeg[pos + 1];
        if marker == 0xDA {
            return (&jpeg[..pos], &jpeg[pos..]);
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        pos += 2 + len;
    }
}

/// Wraps a JPEG into a single-level BLP1 JPEG file. `bgr` must hold `w*h` pixels in B, G, R
/// order (BLP convention, see the crate docs).
fn build_jpeg_blp(w: u16, h: u16, bgr: &[u8]) -> Vec<u8> {
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 95)
        .encode(bgr, w, h, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let (shared, mip) = split_jpeg(&jpeg);

    let mut header = vec![0u8; HEADER_LEN];
    header[0..4].copy_from_slice(b"BLP1");
    header[8..12].copy_from_slice(&0u32.to_le_bytes());
    header[12..16].copy_from_slice(&u32::from(w).to_le_bytes());
    header[16..20].copy_from_slice(&u32::from(h).to_le_bytes());
    let mip_offset = (HEADER_LEN + 4 + shared.len()) as u32;
    header[0x1C..0x20].copy_from_slice(&mip_offset.to_le_bytes());
    header[0x5C..0x60].copy_from_slice(&(mip.len() as u32).to_le_bytes());

    let mut file = header;
    file.extend_from_slice(&(shared.len() as u32).to_le_bytes());
    file.extend_from_slice(shared);
    file.extend_from_slice(mip);
    file
}

#[test]
fn jpeg_content_decodes_with_approximate_colors() {
    // 16x16 image in BGR byte order: left half pure red, right half pure blue, with a green
    // band across the middle rows.
    let (w, h) = (16u16, 16u16);
    let mut bgr = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let px = if (6..10).contains(&y) {
                [0u8, 255, 0] // B, G, R = green
            } else if x < 8 {
                [0u8, 0, 255] // red
            } else {
                [255u8, 0, 0] // blue
            };
            bgr.extend_from_slice(&px);
        }
    }
    let file = build_jpeg_blp(w, h, &bgr);
    let blp = Blp::parse(&file).unwrap();
    assert_eq!(blp.header.content, ContentType::Jpeg);
    assert_eq!(blp.mip_count(), 1);

    let (dw, dh, rgba) = blp.mip_rgba(0).unwrap();
    assert_eq!((dw, dh), (16, 16));
    assert_eq!(rgba.len(), 16 * 16 * 4);

    let at = |x: usize, y: usize| {
        let i = (y * 16 + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let near = |got: [u8; 4], want: [u8; 3]| {
        for c in 0..3 {
            assert!(
                (i32::from(got[c]) - i32::from(want[c])).abs() <= 24,
                "got {got:?}, want ~{want:?}"
            );
        }
        assert_eq!(got[3], 255, "JPEG content is opaque in this path");
    };
    // Interior points, away from chroma-subsampling edges.
    near(at(2, 2), [255, 0, 0]);
    near(at(13, 2), [0, 0, 255]);
    near(at(4, 8), [0, 255, 0]);
    near(at(12, 12), [0, 0, 255]);
}
