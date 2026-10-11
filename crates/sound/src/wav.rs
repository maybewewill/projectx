//! RIFF/WAVE parser for uncompressed PCM (8-bit unsigned and 16-bit signed, mono or stereo or
//! more channels). WAVE_FORMAT_EXTENSIBLE with a PCM sub-format is accepted too.
//! IMA/MS ADPCM and other formats are reported as `Unsupported`; the Warcraft III archives
//! hold none of them (checked with `crates/mpq/examples/wavcheck.rs`).

use crate::Pcm;

const FORMAT_PCM: u16 = 1;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WavError {
    #[error("not a RIFF/WAVE file")]
    NotWave,
    #[error("truncated {0} chunk")]
    Truncated(&'static str),
    #[error("missing {0} chunk")]
    Missing(&'static str),
    #[error("invalid fmt chunk: {0}")]
    Invalid(&'static str),
    #[error("unsupported WAV format tag {tag:#06x} with {bits} bits per sample")]
    Unsupported { tag: u16, bits: u16 },
    #[error("data chunk claims {claimed} bytes but only {available} remain")]
    DataTruncated { claimed: usize, available: usize },
}

/// Parses a WAV file and returns its samples as signed 16-bit PCM.
pub fn parse(bytes: &[u8]) -> Result<Pcm, WavError> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(WavError::NotWave);
    }
    let mut pos = 12usize;
    let mut fmt: Option<Format> = None;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = pos + 8;
        let available = bytes.len() - body;
        match id {
            b"fmt " => {
                if size > available {
                    return Err(WavError::Truncated("fmt"));
                }
                fmt = Some(parse_fmt(&bytes[body..body + size])?);
            }
            b"data" => {
                if size > available {
                    return Err(WavError::DataTruncated {
                        claimed: size,
                        available,
                    });
                }
                data = Some(&bytes[body..body + size]);
            }
            _ => {}
        }
        if data.is_some() && fmt.is_some() {
            break;
        }
        // Chunks are padded to an even length.
        pos = body.saturating_add(size).saturating_add(size & 1);
    }
    let fmt = fmt.ok_or(WavError::Missing("fmt"))?;
    let data = data.ok_or(WavError::Missing("data"))?;
    decode_pcm(&fmt, data)
}

#[derive(Debug, Clone, Copy)]
struct Format {
    sample_rate: u32,
    channels: u16,
    bits: u16,
    block_align: u16,
}

fn parse_fmt(body: &[u8]) -> Result<Format, WavError> {
    if body.len() < 16 {
        return Err(WavError::Truncated("fmt"));
    }
    let u16_at = |o: usize| u16::from_le_bytes([body[o], body[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes([body[o], body[o + 1], body[o + 2], body[o + 3]]);
    let mut tag = u16_at(0);
    let channels = u16_at(2);
    let sample_rate = u32_at(4);
    let block_align = u16_at(12);
    let bits = u16_at(14);
    if tag == FORMAT_EXTENSIBLE {
        // cbSize (2), validBits (2), channelMask (4), then the sub-format GUID whose first two
        // bytes are the format tag.
        if body.len() < 26 {
            return Err(WavError::Truncated("fmt"));
        }
        tag = u16_at(24);
    }
    if tag != FORMAT_PCM {
        return Err(WavError::Unsupported { tag, bits });
    }
    if channels == 0 {
        return Err(WavError::Invalid("zero channels"));
    }
    if sample_rate == 0 {
        return Err(WavError::Invalid("zero sample rate"));
    }
    if bits != 8 && bits != 16 {
        return Err(WavError::Unsupported { tag, bits });
    }
    let bytes_per_frame = usize::from(channels) * usize::from(bits / 8);
    if usize::from(block_align) != bytes_per_frame {
        return Err(WavError::Invalid(
            "block align does not match channels and bits",
        ));
    }
    Ok(Format {
        sample_rate,
        channels,
        bits,
        block_align,
    })
}

fn decode_pcm(fmt: &Format, data: &[u8]) -> Result<Pcm, WavError> {
    // A trailing partial frame cannot be played; drop it.
    let whole = data.len() - data.len() % usize::from(fmt.block_align);
    let data = &data[..whole];
    let samples: Vec<i16> = if fmt.bits == 8 {
        data.iter().map(|&b| (i16::from(b) - 128) << 8).collect()
    } else {
        data.chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    };
    Ok(Pcm {
        sample_rate: fmt.sample_rate,
        channels: fmt.channels,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block_align = channels * bits / 8;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&tag.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        body.extend_from_slice(&fmt);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(data);
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn mono_16_bit() {
        let data = [0x01, 0x00, 0xFF, 0xFF, 0x00, 0x80];
        let pcm = parse(&wav(1, 1, 22050, 16, &data)).unwrap();
        assert_eq!(pcm.sample_rate, 22050);
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.samples, vec![1, -1, i16::MIN]);
        assert_eq!(pcm.frames(), 3);
    }

    #[test]
    fn stereo_16_bit_keeps_interleaving() {
        let data: Vec<u8> = [10i16, -10, 20, -20]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let pcm = parse(&wav(1, 2, 44100, 16, &data)).unwrap();
        assert_eq!(pcm.samples, vec![10, -10, 20, -20]);
        assert_eq!(pcm.frames(), 2);
    }

    #[test]
    fn unsigned_8_bit_is_centred_and_scaled() {
        let pcm = parse(&wav(1, 1, 8000, 8, &[0, 128, 255])).unwrap();
        assert_eq!(pcm.samples, vec![-32768, 0, 127 << 8]);
    }

    #[test]
    fn extensible_pcm_is_accepted() {
        // Built by hand: a fmt chunk as WAVE_FORMAT_EXTENSIBLE with a PCM sub-format.
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&FORMAT_EXTENSIBLE.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&16000u32.to_le_bytes());
        fmt.extend_from_slice(&32000u32.to_le_bytes());
        fmt.extend_from_slice(&2u16.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());
        fmt.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        fmt.extend_from_slice(&16u16.to_le_bytes()); // validBits
        fmt.extend_from_slice(&4u32.to_le_bytes()); // channel mask
        fmt.extend_from_slice(&FORMAT_PCM.to_le_bytes());
        fmt.extend_from_slice(&[0u8; 14]);
        let mut rebuilt = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        rebuilt.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        rebuilt.extend_from_slice(&fmt);
        rebuilt.extend_from_slice(b"data\x02\0\0\0\x05\x00");
        let pcm = parse(&rebuilt).unwrap();
        assert_eq!(pcm.samples, vec![5]);
    }

    #[test]
    fn trailing_partial_frame_is_dropped() {
        let pcm = parse(&wav(1, 1, 8000, 16, &[1, 0, 2])).unwrap();
        assert_eq!(pcm.samples, vec![1]);
    }

    #[test]
    fn adpcm_is_reported_as_unsupported() {
        assert_eq!(
            parse(&wav(0x11, 1, 22050, 4, &[0; 4])),
            Err(WavError::Unsupported { tag: 0x11, bits: 4 })
        );
    }

    #[test]
    fn rejects_non_wave_and_truncated_data() {
        assert_eq!(parse(b"RIFF\0\0\0\0AVI "), Err(WavError::NotWave));
        let mut bytes = wav(1, 1, 8000, 16, &[0, 0, 0, 0]);
        bytes.truncate(bytes.len() - 2);
        assert!(matches!(
            parse(&bytes),
            Err(WavError::DataTruncated {
                claimed: 4,
                available: 2
            })
        ));
    }

    #[test]
    fn corrupt_headers_never_panic() {
        let base = wav(1, 2, 22050, 16, &[1, 2, 3, 4, 5, 6, 7, 8]);
        for cut in 0..base.len() {
            let _ = parse(&base[..cut]);
        }
        for i in 0..base.len() {
            let mut b = base.clone();
            b[i] ^= 0xFF;
            let _ = parse(&b);
        }
    }
}
