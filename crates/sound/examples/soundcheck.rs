//! Decodes every `.wav` and `.mp3` listed in each archive to PCM in memory (no playback) and
//! reports counts per archive and format, failures by error kind, and simple signal statistics.
//! Usage: `cargo run -p sound --release --example soundcheck -- archive.mpq [more.mpq ...]`
use std::collections::BTreeMap;

use sound::{SoundBank, SoundError};

#[derive(Default)]
struct Stats {
    ok: usize,
    failed: usize,
    seconds: f64,
    rates: BTreeMap<u32, usize>,
    channels: BTreeMap<u16, usize>,
    /// Files whose mean sample-to-sample step is large compared with their RMS level. Real
    /// audio at game rates stays well below 1; decoder bugs usually push it toward noise.
    rough: Vec<String>,
    errors: BTreeMap<String, Vec<String>>,
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: soundcheck <archive.mpq>...");
        std::process::exit(2);
    }
    let mut totals: BTreeMap<&'static str, Stats> = BTreeMap::new();
    for path in &paths {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{path}: read failed: {e}");
                continue;
            }
        };
        let mut bank = SoundBank::new();
        if let Err(e) = bank.add_archive(bytes) {
            println!("{path}: open failed: {e}");
            continue;
        }
        let names = bank.sound_paths();
        let mut per_archive: BTreeMap<&'static str, Stats> = BTreeMap::new();
        for name in &names {
            let format: &'static str = if name.to_ascii_lowercase().ends_with(".mp3") {
                "mp3"
            } else {
                "wav"
            };
            // Decode once, then record the result in both the archive and the grand total.
            let decoded = bank.load(name);
            for stats in [
                per_archive.entry(format).or_default(),
                totals.entry(format).or_default(),
            ] {
                match &decoded {
                    Ok(pcm) => {
                        stats.ok += 1;
                        stats.seconds += pcm.duration_secs();
                        *stats.rates.entry(pcm.sample_rate).or_default() += 1;
                        *stats.channels.entry(pcm.channels).or_default() += 1;
                        if roughness(&pcm.samples) > 1.0 {
                            stats.rough.push(name.clone());
                        }
                    }
                    Err(e) => {
                        stats.failed += 1;
                        stats
                            .errors
                            .entry(error_kind(e))
                            .or_default()
                            .push(name.clone());
                    }
                }
            }
        }
        println!("{path}: {} sound names", names.len());
        report(&per_archive);
    }
    if paths.len() > 1 {
        println!(
            "total over {} archives (duplicates counted per archive):",
            paths.len()
        );
        report(&totals);
    }
}

fn report(stats: &BTreeMap<&'static str, Stats>) {
    for (format, s) in stats {
        println!(
            "  {format}: {} OK, {} failed, {:.1} s of audio",
            s.ok, s.failed, s.seconds
        );
        for (rate, n) in &s.rates {
            println!("    {rate} Hz: {n}");
        }
        for (ch, n) in &s.channels {
            println!("    {ch} channel(s): {n}");
        }
        if !s.rough.is_empty() {
            println!(
                "    rough signal (mean step > RMS): {} (e.g. {})",
                s.rough.len(),
                s.rough[..s.rough.len().min(3)].join(", ")
            );
        }
        for (kind, files) in &s.errors {
            println!(
                "    {kind}: {} (e.g. {})",
                files.len(),
                files[..files.len().min(3)].join(", ")
            );
        }
    }
}

/// Mean absolute sample-to-sample step divided by the RMS level (0 for silence).
fn roughness(samples: &[i16]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let mut step = 0f64;
    let mut energy = 0f64;
    for w in samples.windows(2) {
        step += f64::from((i32::from(w[1]) - i32::from(w[0])).abs());
    }
    for &s in samples {
        energy += f64::from(s) * f64::from(s);
    }
    let rms = (energy / samples.len() as f64).sqrt();
    if rms == 0.0 {
        return 0.0;
    }
    (step / (samples.len() - 1) as f64) / rms
}

fn error_kind(e: &SoundError) -> String {
    match e {
        SoundError::Mpq(m) => format!("mpq: {}", m.to_string().split(':').next().unwrap_or("")),
        SoundError::Wav(w) => format!("wav: {}", w.to_string().split(':').next().unwrap_or("")),
        SoundError::Mp3(m) => format!("mp3: {}", m.to_string().split(':').next().unwrap_or("")),
        other => other.to_string(),
    }
}
