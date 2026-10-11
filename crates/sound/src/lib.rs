//! Pure-Rust sound loading for projectx: WAV (PCM 8/16-bit) and MP3 decoding to interleaved
//! signed 16-bit PCM, plus a `SoundBank` that reads sounds by game path from an MPQ stack.
//!
//! Nothing here plays audio. Output is in memory only, so the crate builds for wasm32 as well.
//! Presentation only: sound is not part of the bit-exact simulation.

pub mod bank;
pub mod mp3;
pub mod wav;

pub use bank::{SoundBank, SoundError};

/// Decoded audio: interleaved signed 16-bit samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl Pcm {
    /// Number of sample frames (one frame = one sample per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// Playing time in seconds.
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames() as f64 / f64::from(self.sample_rate)
    }
}
