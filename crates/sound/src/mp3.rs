//! MP3 decoding to signed 16-bit PCM with Symphonia (pure Rust, no threads, builds for wasm32).
//! Only the MPEG audio decoder is enabled in `Cargo.toml`, so other containers are rejected.

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::Pcm;

#[derive(Debug, thiserror::Error)]
pub enum Mp3Error {
    #[error("not an MP3 stream: {0}")]
    Probe(String),
    #[error("no decodable audio track")]
    NoTrack,
    #[error("unsupported codec or stream layout: {0}")]
    Unsupported(String),
    #[error("decode failed after {frames} frames: {msg}")]
    Decode { frames: usize, msg: String },
}

/// Decodes a complete MP3 file held in memory. Any packet that fails to decode is an error,
/// so a result means every packet was decoded.
pub fn decode(bytes: &[u8]) -> Result<Pcm, Mp3Error> {
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| Mp3Error::Probe(e.to_string()))?;
    let mut format = probed.format;

    let (track_id, params) = {
        let track = format.default_track().ok_or(Mp3Error::NoTrack)?;
        (track.id, track.codec_params.clone())
    };
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| Mp3Error::Unsupported(e.to_string()))?;

    let mut out: Option<Pcm> = None;
    let mut frames = 0usize;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                return Err(Mp3Error::Decode {
                    frames,
                    msg: e.to_string(),
                })
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| Mp3Error::Decode {
            frames,
            msg: e.to_string(),
        })?;
        let spec = *decoded.spec();
        let pcm = out.get_or_insert_with(|| Pcm {
            sample_rate: spec.rate,
            channels: spec.channels.count() as u16,
            samples: Vec::new(),
        });
        if pcm.sample_rate != spec.rate || usize::from(pcm.channels) != spec.channels.count() {
            return Err(Mp3Error::Unsupported(
                "sample rate or channel count changes mid-stream".into(),
            ));
        }
        let mut buf = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        frames += buf.len() / usize::from(pcm.channels.max(1));
        pcm.samples.extend_from_slice(buf.samples());
    }
    out.ok_or(Mp3Error::NoTrack)
}
