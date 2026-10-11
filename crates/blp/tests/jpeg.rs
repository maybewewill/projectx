//! Synthetic JPEG BLP1 files. Each mip is a 4-component JPEG without a colour transform,
//! written the way Warcraft III stores it: B, G, R and alpha bytes, shared header up to the
//! frame marker, each mip starting with its own frame header. Encoded with `jpeg-encoder`;
//! no game assets are used.

use blp::{Blp, ContentType, Error, HEADER_LEN};

/// Walks the JPEG segments and returns `(shared, frame_and_rest)` the way Warcraft III lays
/// them out: `shared` runs from SOI through the SOF0 marker bytes, `frame_and_rest` starts at
/// the SOF length field. The Adobe APP14 segment that `jpeg-encoder` writes for CMYK is left
/// out, because real BLP frames carry no APP14.
fn split_jpeg(jpeg: &[u8]) -> (Vec<u8>, &[u8]) {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "not a JPEG");
    let mut shared = vec![0xFF, 0xD8];
    let mut pos = 2;
    loop {
        assert_eq!(jpeg[pos], 0xFF, "expected a marker");
        let marker = jpeg[pos + 1];
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if marker == 0xC0 {
            shared.extend_from_slice(&jpeg[pos..pos + 2]);
            return (shared, &jpeg[pos + 2..]);
        }
        if marker != 0xEE {
            shared.extend_from_slice(&jpeg[pos..pos + 2 + len]);
        }
        pos += 2 + len;
    }
}

/// Encodes one 4-component mip whose stored bytes are `bgra` (B, G, R, A per pixel).
/// `jpeg-encoder` takes CMYK input in the Adobe convention, where the stored value is the
/// inverse of the input, so the input is inverted here to store `bgra` as given.
fn encode_mip(w: u16, h: u16, bgra: &[u8]) -> Vec<u8> {
    let input: Vec<u8> = bgra.iter().map(|&b| 255 - b).collect();
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 95)
        .encode(&input, w, h, jpeg_encoder::ColorType::Cmyk)
        .unwrap();
    jpeg
}

/// Wraps JPEG mips (level 0 first) into a BLP1 JPEG file. Every mip is encoded with the same
/// quality, so the shared tables are the same for all of them; the shared header comes from
/// level 0.
fn build_jpeg_blp(levels: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let encoded: Vec<Vec<u8>> = levels
        .iter()
        .map(|(w, h, px)| encode_mip(*w, *h, px))
        .collect();
    let (shared, _) = split_jpeg(&encoded[0]);

    let mut header = vec![0u8; HEADER_LEN];
    header[0..4].copy_from_slice(b"BLP1");
    header[4..8].copy_from_slice(&0u32.to_le_bytes());
    header[8..12].copy_from_slice(&8u32.to_le_bytes());
    header[12..16].copy_from_slice(&u32::from(levels[0].0).to_le_bytes());
    header[16..20].copy_from_slice(&u32::from(levels[0].1).to_le_bytes());
    header[24..28].copy_from_slice(&u32::from(levels.len() > 1).to_le_bytes());

    let mut body = Vec::new();
    let mut file_pos = (HEADER_LEN + 4 + shared.len()) as u32;
    for (level, jpeg) in encoded.iter().enumerate() {
        let (_, frame) = split_jpeg(jpeg);
        header[0x1C + 4 * level..0x20 + 4 * level].copy_from_slice(&file_pos.to_le_bytes());
        header[0x5C + 4 * level..0x60 + 4 * level]
            .copy_from_slice(&(frame.len() as u32).to_le_bytes());
        body.extend_from_slice(frame);
        file_pos += frame.len() as u32;
    }

    let mut file = header;
    file.extend_from_slice(&(shared.len() as u32).to_le_bytes());
    file.extend_from_slice(&shared);
    file.extend_from_slice(&body);
    file
}

/// BGRA bytes for a 16x16 image: left half red, right half blue, a green band across rows
/// 6..10, and the top-left 8x8 block fully transparent (alpha 0) with the rest opaque.
fn sample_16x16() -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            let bgr = if (6..10).contains(&y) {
                [0u8, 255, 0] // green
            } else if x < 8 {
                [0u8, 0, 255] // red
            } else {
                [255u8, 0, 0] // blue
            };
            let alpha = if x < 8 && y < 8 { 0 } else { 255 };
            out.extend_from_slice(&[bgr[0], bgr[1], bgr[2], alpha]);
        }
    }
    out
}

fn near(got: [u8; 4], want: [u8; 4], tol: i32) {
    for c in 0..4 {
        assert!(
            (i32::from(got[c]) - i32::from(want[c])).abs() <= tol,
            "got {got:?}, want ~{want:?}"
        );
    }
}

#[test]
fn four_component_frame_decodes_colour_and_alpha() {
    let file = build_jpeg_blp(&[(16, 16, sample_16x16())]);
    let blp = Blp::parse(&file).unwrap();
    assert_eq!(blp.header.content, ContentType::Jpeg);
    assert_eq!(blp.mip_count(), 1);

    let (w, h, rgba) = blp.mip_rgba(0).unwrap();
    assert_eq!((w, h), (16, 16));
    assert_eq!(rgba.len(), 16 * 16 * 4);

    let at = |x: usize, y: usize| {
        let i = (y * 16 + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    // Interior points, away from the 8x8 block edges where JPEG blurs the alpha.
    near(at(2, 2), [255, 0, 0, 0], 24);
    near(at(4, 8), [0, 255, 0, 255], 24);
    near(at(13, 2), [0, 0, 255, 255], 24);
    near(at(12, 12), [0, 0, 255, 255], 24);
    near(at(3, 14), [255, 0, 0, 255], 24);
}

#[test]
fn opaque_frame_has_full_alpha_everywhere() {
    // Alpha stored as 255 everywhere: the decoded alpha must be 255 too.
    let mut px = sample_16x16();
    for p in px.chunks_exact_mut(4) {
        p[3] = 255;
    }
    let file = build_jpeg_blp(&[(16, 16, px)]);
    let (_, _, rgba) = Blp::parse(&file).unwrap().mip_rgba(0).unwrap();
    let alphas: Vec<u8> = rgba.chunks_exact(4).map(|p| p[3]).collect();
    assert!(alphas.iter().all(|&a| a >= 250), "opaque frame lost alpha");
}

#[test]
fn each_mip_level_uses_its_own_frame_header() {
    // Level 1 is a different size and colour from level 0, so decoding it with level 0's
    // frame header would fail or give the wrong colours.
    let level1: Vec<u8> = (0..8 * 8).flat_map(|_| [40u8, 80, 200, 255]).collect();
    let file = build_jpeg_blp(&[(16, 16, sample_16x16()), (8, 8, level1)]);
    let blp = Blp::parse(&file).unwrap();
    assert_eq!(blp.mip_count(), 2);

    let (w, h, rgba) = blp.mip_rgba(1).unwrap();
    assert_eq!((w, h), (8, 8));
    // Uniform colour: B=40, G=80, R=200 -> R=200, G=80, B=40.
    for p in rgba.chunks_exact(4) {
        near([p[0], p[1], p[2], p[3]], [200, 80, 40, 255], 8);
    }
}

#[test]
fn three_component_frame_is_rejected() {
    // A JFIF RGB frame is not what Warcraft III writes; it must not decode as BGRA.
    let mut jpeg = Vec::new();
    let rgb: Vec<u8> = (0..16 * 16).flat_map(|_| [10u8, 20, 30]).collect();
    jpeg_encoder::Encoder::new(&mut jpeg, 95)
        .encode(&rgb, 16, 16, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let (shared, frame) = split_jpeg(&jpeg);
    let mut file = vec![0u8; HEADER_LEN];
    file[0..4].copy_from_slice(b"BLP1");
    file[12..16].copy_from_slice(&16u32.to_le_bytes());
    file[16..20].copy_from_slice(&16u32.to_le_bytes());
    file[0x1C..0x20].copy_from_slice(&((HEADER_LEN + 4 + shared.len()) as u32).to_le_bytes());
    file[0x5C..0x60].copy_from_slice(&(frame.len() as u32).to_le_bytes());
    file.extend_from_slice(&(shared.len() as u32).to_le_bytes());
    file.extend_from_slice(&shared);
    file.extend_from_slice(frame);
    let blp = Blp::parse(&file).unwrap();
    assert!(matches!(blp.mip_rgba(0), Err(Error::Jpeg(_))));
}
