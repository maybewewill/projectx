//! Game-start seed for the simulation, taken from a replay's game-start record.
//!
//! The original seeds the global generator and the 45 streams with one value `S` at game
//! start (`docs/spec/game-start.md`). The machine code confirms the two seeding paths and the
//! rotate-xor fold (`sim::rng::slot_fold_step`), and that the constant `0x77617233` replaces
//! the fold when the settings flag `0x8000` is set. What the fold reads (which per-player data)
//! and how the settings object's flag word relates to the replay's `flags` field are NOT yet
//! established. Until they are, a replay supplies its record's random seed field as `S`
//! (the intended source, UNSURE), and `--seed` overrides it.

/// Where the game-start seed came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedSource {
    /// An explicit seed given by the caller (`--seed`).
    Explicit,
    /// The random seed field of the replay's game-start record (link UNSURE, see module docs).
    ReplayRecord,
}

/// The seed to start the simulation with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartSeed {
    /// The value `S` given to `sim::Streams::start_game`.
    pub seed: u32,
    /// Where it came from.
    pub source: SeedSource,
}

/// The seed of a replay: its game-start record's random seed, unless `explicit` is given.
pub fn resolve(explicit: Option<u32>, replay: Option<&w3g::Replay>) -> StartSeed {
    if let Some(seed) = explicit {
        return StartSeed {
            seed,
            source: SeedSource::Explicit,
        };
    }
    match replay {
        Some(r) => StartSeed {
            seed: r.game.random_seed,
            source: SeedSource::ReplayRecord,
        },
        None => StartSeed {
            seed: 0,
            source: SeedSource::Explicit,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A replay with a synthetic game-start record (see `w3g/tests/synthetic.rs` for the layout).
    fn synthetic_replay(seed: u32) -> w3g::Replay {
        let mut body = Vec::new();
        body.extend_from_slice(&0x110u32.to_le_bytes());
        // Host record: id, player id, name, one extra byte, empty stat string.
        body.extend_from_slice(&[0x00, 1]);
        body.extend_from_slice(b"Host\0");
        body.extend_from_slice(&[1, 0]);
        body.extend_from_slice(b"Test Game\0");
        body.push(0);
        let mut settings = Vec::new();
        settings.extend_from_slice(&0x0001_8000u32.to_le_bytes());
        settings.push(0);
        settings.extend_from_slice(&116u16.to_le_bytes());
        settings.extend_from_slice(&84u16.to_le_bytes());
        settings.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        settings.extend_from_slice(b"Maps\\x.w3x\0Host\0\0");
        body.extend_from_slice(&w3g::encode_string(&settings));
        body.push(0); // terminator of the encoded settings string
        body.extend_from_slice(&2u32.to_le_bytes()); // slot count
        body.extend_from_slice(&0u32.to_le_bytes()); // game type
        body.extend_from_slice(&0u32.to_le_bytes()); // language
        // Game-start record 0x19: two 9-byte slots, the random seed, select mode, start spots.
        let mut rec = vec![2u8];
        rec.extend_from_slice(&[1, 100, 2, 0, 0, 0, 0x20, 1, 100]);
        rec.extend_from_slice(&[2, 100, 2, 0, 1, 1, 0x20, 1, 100]);
        rec.extend_from_slice(&seed.to_le_bytes());
        rec.extend_from_slice(&[0x09, 2]);
        body.push(0x19);
        body.extend_from_slice(&(rec.len() as u16).to_le_bytes());
        body.extend_from_slice(&rec);
        let file = synthetic_file(&body);
        w3g::Replay::parse(&file).expect("synthetic replay parses")
    }

    /// Wraps a body in one zlib block with the header layout the w3g tests use.
    fn synthetic_file(body: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let mut padded = body.to_vec();
        padded.resize(body.len().max(1), 0);
        z.write_all(&padded).unwrap();
        let data = z.finish().unwrap();
        let mut blocks = Vec::new();
        blocks.extend_from_slice(&(data.len() as u16).to_le_bytes());
        blocks.extend_from_slice(&(body.len() as u16).to_le_bytes());
        blocks.extend_from_slice(&0u32.to_le_bytes());
        blocks.extend_from_slice(&data);
        let mut f = Vec::new();
        f.extend_from_slice(w3g::MAGIC);
        f.extend_from_slice(&0x44u32.to_le_bytes());
        f.extend_from_slice(&((0x44 + blocks.len()) as u32).to_le_bytes());
        f.extend_from_slice(&1u32.to_le_bytes());
        f.extend_from_slice(&(body.len() as u32).to_le_bytes());
        f.extend_from_slice(&1u32.to_le_bytes());
        f.extend_from_slice(b"PX3W");
        f.extend_from_slice(&26u32.to_le_bytes());
        f.extend_from_slice(&6059u16.to_le_bytes());
        f.extend_from_slice(&0x8000u16.to_le_bytes());
        f.extend_from_slice(&350u32.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&blocks);
        f
    }

    #[test]
    fn explicit_seed_overrides_replay() {
        let r = synthetic_replay(0xDEAD_BEEF);
        let s = resolve(Some(7), Some(&r));
        assert_eq!(s, StartSeed { seed: 7, source: SeedSource::Explicit });
    }

    #[test]
    fn replay_record_seed_is_used_without_override() {
        let r = synthetic_replay(0x1234_5678);
        let s = resolve(None, Some(&r));
        assert_eq!(s, StartSeed { seed: 0x1234_5678, source: SeedSource::ReplayRecord });
    }

    #[test]
    fn no_input_gives_zero_explicit() {
        assert_eq!(resolve(None, None), StartSeed { seed: 0, source: SeedSource::Explicit });
    }
}
