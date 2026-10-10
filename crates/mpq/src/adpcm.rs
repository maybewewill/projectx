// Blizzard ADPCM sector codec (compression masks 0x40 mono and 0x80 stereo).
//
// Ported from OpenRealm (MIT, (c) 2024 corepunch), common/mpq.c, which follows the StormLib
// description of the format.
//
// Stream layout after the mask byte: one unused byte, the shift byte, then one little-endian
// 16-bit initial sample per channel, then one code byte per further sample (channels are
// interleaved, the first code belongs to channel 0). Code bytes:
//   0x80: decrease the step index of the channel and emit its previous sample again,
//   0x81: increase the step index by 8, skip a channel, emit nothing,
//   other: 6 bits of magnitude (bit 0..5), bit 6 is the sign, bits 0..4 index the step table.

use crate::Error;

const INIT_STEP_INDEX: i32 = 44;
const MAX_STEP_INDEX: i32 = 88;

/// Index change for the low five bits of a code byte.
const NEXT_STEP: [i32; 32] = [
    -1, 0, -1, 4, -1, 2, -1, 6, -1, 1, -1, 5, -1, 3, -1, 7, -1, 1, -1, 5, -1, 3, -1, 7, -1, 2, -1,
    4, -1, 6, -1, 8,
];

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// Magnitude added to the prediction for a normal code, before the sign is applied.
fn delta(step: i32, shift: u8, code: u8) -> i32 {
    let mut diff = step.checked_shr(u32::from(shift)).unwrap_or(0);
    for bit in 0..6 {
        if code & (1 << bit) != 0 {
            diff += step >> bit;
        }
    }
    diff
}

/// Predictor state shared by the decoder and the test encoder.
#[derive(Clone)]
pub(crate) struct State {
    predicted: [i32; 2],
    step_index: [i32; 2],
    shift: u8,
}

impl State {
    pub(crate) fn new(shift: u8) -> Self {
        State {
            predicted: [0; 2],
            step_index: [INIT_STEP_INDEX; 2],
            shift,
        }
    }

    /// The sample a normal `code` would produce on `channel`, without changing the state.
    pub(crate) fn preview(&self, channel: usize, code: u8) -> i32 {
        let step = STEP_TABLE[self.step_index[channel] as usize];
        let d = delta(step, self.shift, code);
        let next = if code & 0x40 != 0 {
            self.predicted[channel] - d
        } else {
            self.predicted[channel] + d
        };
        next.clamp(-32768, 32767)
    }

    /// Applies a normal `code` on `channel` with the sample it produced.
    pub(crate) fn apply(&mut self, channel: usize, code: u8, sample: i32) {
        self.predicted[channel] = sample;
        self.step_index[channel] = (self.step_index[channel] + NEXT_STEP[usize::from(code & 0x1f)])
            .clamp(0, MAX_STEP_INDEX);
    }
}

/// Decodes an ADPCM stream (the bytes after the mask, and after Huffman if present) into at most
/// `expected` bytes of 16-bit little-endian PCM, interleaved by channel.
pub(crate) fn decompress(src: &[u8], channels: usize, expected: usize) -> Result<Vec<u8>, Error> {
    if !(1..=2).contains(&channels) || src.len() < 2 + channels * 2 || expected < channels * 2 {
        return Err(Error::Decompress("adpcm: short or invalid stream".into()));
    }
    let shift = src[1];
    let mut state = State::new(shift);
    let mut out = Vec::with_capacity(expected);
    let mut pos = 2;
    for ch in 0..channels {
        state.predicted[ch] = i32::from(i16::from_le_bytes([src[pos], src[pos + 1]]));
        pos += 2;
        out.extend_from_slice(&(state.predicted[ch] as i16).to_le_bytes());
    }

    let mut channel = channels - 1;
    while pos < src.len() {
        let code = src[pos];
        pos += 1;
        channel = (channel + 1) % channels;
        match code {
            0x80 => {
                if state.step_index[channel] > 0 {
                    state.step_index[channel] -= 1;
                }
            }
            0x81 => {
                state.step_index[channel] = (state.step_index[channel] + 8).min(MAX_STEP_INDEX);
                channel = (channel + 1) % channels;
                continue;
            }
            _ => {
                let sample = state.preview(channel, code);
                state.apply(channel, code, sample);
            }
        }
        if out.len() + 2 > expected {
            break;
        }
        out.extend_from_slice(&(state.predicted[channel] as i16).to_le_bytes());
    }
    Ok(out)
}

/// Test-only encoder: greedy search over the 128 normal codes for each sample, using the same
/// predictor state as the decoder. Returns the stream as it appears after the mask byte.
#[cfg(test)]
pub(crate) fn encode(samples: &[i16], channels: usize, shift: u8) -> Vec<u8> {
    let mut out = vec![0x00, shift];
    let mut state = State::new(shift);
    for (ch, &s) in samples.iter().take(channels).enumerate() {
        out.extend_from_slice(&s.to_le_bytes());
        state.predicted[ch] = i32::from(s);
    }
    for (i, &s) in samples.iter().enumerate().skip(channels) {
        let ch = i % channels;
        let (code, _) = (0u8..0x80)
            .map(|code| {
                let err = (state.preview(ch, code) - i32::from(s)).abs();
                (code, err)
            })
            .min_by_key(|&(_, err)| err)
            .unwrap();
        let sample = state.preview(ch, code);
        state.apply(ch, code, sample);
        out.push(code);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max_error(a: &[i16], b: &[i16]) -> i32 {
        a.iter()
            .zip(b)
            .map(|(&x, &y)| (i32::from(x) - i32::from(y)).abs())
            .max()
            .unwrap_or(0)
    }

    fn to_pcm(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect()
    }

    fn sine(n: usize, period: f64, amp: f64) -> Vec<i16> {
        (0..n)
            .map(|i| (amp * (i as f64 * std::f64::consts::TAU / period).sin()).round() as i16)
            .collect()
    }

    #[test]
    fn hand_built_mono_vector() {
        // Stream: unused byte, shift 0, initial sample 0, one code 0x01.
        // Step index 44 gives step 494; code 0x01 adds step + step = 988.
        let stream = [0x00, 0x00, 0x00, 0x00, 0x01];
        let out = decompress(&stream, 1, 4).unwrap();
        assert_eq!(out, [0x00, 0x00, 0xdc, 0x03]);
    }

    #[test]
    fn hand_built_sign_bit_moves_down() {
        // Code 0x41 is sign plus magnitude bit 0: predicted = 0 - 2 * 494 = -988.
        let stream = [0x00, 0x00, 0x00, 0x00, 0x41];
        let out = decompress(&stream, 1, 4).unwrap();
        assert_eq!(to_pcm(&out), [0, -988]);
    }

    #[test]
    fn mono_sine_round_trip_length_and_error() {
        let samples = sine(2048, 64.0, 8000.0);
        let stream = encode(&samples, 1, 2);
        let out = decompress(&stream, 1, samples.len() * 2).unwrap();
        assert_eq!(out.len(), samples.len() * 2);
        let pcm = to_pcm(&out);
        let err = max_error(&samples, &pcm);
        assert!(err <= 64, "max error {err} too large for a sine");
    }

    #[test]
    fn mono_constant_signal_stays_close() {
        // With shift 0 the smallest step is the initial 494, so use a larger shift.
        let samples = vec![1000i16; 512];
        let stream = encode(&samples, 1, 4);
        let out = decompress(&stream, 1, samples.len() * 2).unwrap();
        let pcm = to_pcm(&out);
        assert_eq!(pcm.len(), samples.len());
        let err = max_error(&samples, &pcm);
        assert!(err <= 64, "max error {err}");
    }

    #[test]
    fn stereo_round_trip_keeps_channels_apart() {
        let n = 1024;
        let left = sine(n, 48.0, 6000.0);
        let right = sine(n, 96.0, -4000.0);
        let mut interleaved = Vec::with_capacity(2 * n);
        for i in 0..n {
            interleaved.push(left[i]);
            interleaved.push(right[i]);
        }
        let stream = encode(&interleaved, 2, 2);
        let out = decompress(&stream, 2, interleaved.len() * 2).unwrap();
        let pcm = to_pcm(&out);
        assert_eq!(pcm.len(), interleaved.len());
        let got_left: Vec<i16> = pcm.iter().step_by(2).copied().collect();
        let got_right: Vec<i16> = pcm.iter().skip(1).step_by(2).copied().collect();
        assert!(max_error(&left, &got_left) <= 64);
        assert!(max_error(&right, &got_right) <= 64);
    }

    #[test]
    fn output_is_capped_at_expected_size() {
        let samples = sine(256, 32.0, 3000.0);
        let stream = encode(&samples, 1, 1);
        let out = decompress(&stream, 1, 100).unwrap();
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn step_control_codes_are_consumed() {
        // 0x80 re-emits the previous sample; 0x81 emits nothing and skips a channel.
        let stream = [0x00, 0x00, 0x00, 0x00, 0x80, 0x01];
        assert_eq!(decompress(&stream, 1, 6).unwrap().len(), 6);

        // Two initial samples, then 0x81 (no output) and two normal codes: 4 + 2 * 2 bytes.
        let stereo = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81, 0x01, 0x01];
        assert_eq!(decompress(&stereo, 2, 8).unwrap().len(), 8);
    }

    #[test]
    fn rejects_short_stream() {
        assert!(decompress(&[0x00, 0x00, 0x00], 1, 2).is_err());
        assert!(decompress(&[0x00, 0x00, 0x00, 0x00, 0x00], 2, 4).is_err());
    }
}
