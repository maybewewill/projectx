//! Game-start seed for the simulation, taken from a replay's game-start record.
//!
//! The original seeds the global generator and the 45 streams with one value `S` at game
//! start (`docs/spec/game-start.md`).
//!
//! Confirmed in `Game.dll` 1.26a:
//! - The game-start body (block `0x19`) is parsed by `0x6F6559A0` / `0x6F5BE740`
//!   ([`w3g::start::parse_start_body`]). When it is accepted, its seed field becomes `S`
//!   ([`setup_seed`], [`SetupPath::Record`]). This is the path a replay takes when its record is valid
//!   (LIKELY for replays, see the spec).
//! - When the record is refused and the setup mode is 5, `S` is a rotate-xor fold over per-slot
//!   sub-records ([`fold_input`], [`fold_seed`], [`SetupPath::Fold`]). The per-slot input is checked
//!   under Unicorn; where those sub-records come from in a replay is NOT established.
//! - The constant `0x77617233` replaces `S` when the settings flag `0x8000` is set (applied by
//!   `sim::rng::game_start_seed`). Whether the replay's `flags` carry that bit: UNSURE.
//!
//! [`resolve`] still uses the replay's record seed (or `--seed`) as the simulation seed.

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

/// Tag byte that makes the per-slot fold step run (the reader limit, `0x6F5C3A35`).
pub const FOLD_TAG: u8 = 8;

/// Which input the setup function took the seed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPath {
    /// The game-start record was accepted; its seed field is the value `S`.
    Record,
    /// The record was refused and the setup mode is 5: `S` is the fold over the slot sub-records.
    Fold,
}

/// The fold input `X` of one per-slot sub-record (CONFIRMED under Unicorn, `tools/emu/game_start_x_emu.py`).
///
/// `sub` starts at the sub-record's first byte (its `+0x00`). The original copies the record's four
/// dwords (`+0x19`, `+0x1D`, `+0x21`, `+0x25`), the count byte (`+0x29`) and the `count` name bytes
/// (`+0x2A`) into a blob (`0x6F545A00`). The caller then reads its reader limit from the low byte of
/// the `+0x25` dword and the element from the bytes after it. The fold step runs only when that limit
/// byte is [`FOLD_TAG`]; then `X` is the bytes `+0x26, +0x27, +0x28, +0x29` read as a little-endian
/// dword (the count byte becomes the top byte). Otherwise the step is skipped and `None` is returned.
///
/// The origin of these sub-records in a replay-driven game is not established (see
/// `docs/spec/game-start.md`, section 3).
pub fn fold_input(sub: &[u8]) -> Option<u32> {
    if *sub.get(0x25)? != FOLD_TAG {
        return None;
    }
    let count = *sub.get(0x29)?;
    Some(u32::from_le_bytes([sub[0x26], sub[0x27], sub[0x28], count]))
}

/// The fold over the per-slot sub-records, in player id order (the caller's loop `1..=16`, only for
/// ids that map to a slot). Skipped sub-records contribute nothing.
pub fn fold_seed<'a>(subs: impl IntoIterator<Item = &'a [u8]>) -> u32 {
    sim::rng::slot_fold(subs.into_iter().filter_map(fold_input))
}

/// The seed `S` the setup function chooses (CONFIRMED structure, `0x6F5C37A0`).
///
/// - The game-start body is parsed by `0x6F5BE740`. If it is accepted, the seed field is `S`
///   (`SetupPath::Record`), and the 0x8000 rule of `sim::rng::game_start_seed` is applied afterwards.
/// - If it is refused and `mode == 5` (the setup argument compared at `0x6F5C382D`), `S` is the fold
///   over `sub_records` (`SetupPath::Fold`, `S` starts at 0 at `0x6F5C38F4`).
/// - Otherwise the path is not modelled here and `None` is returned.
pub fn setup_seed(body: Option<&[u8]>, mode: u32, sub_records: &[&[u8]]) -> Option<(u32, SetupPath)> {
    if let Some(b) = body {
        if let Ok(rec) = w3g::start::parse_start_body(b) {
            return Some((rec.seed, SetupPath::Record));
        }
    }
    if mode == 5 {
        return Some((fold_seed(sub_records.iter().copied()), SetupPath::Fold));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a per-slot sub-record with the layout of `0x6F545A00`: 12 key bytes, the tag dword
    /// (tag, byte1, byte2, byte3 at `+0x25..+0x28`), the count at `+0x29` and the name at `+0x2A`.
    fn sub_record(tag: u8, b1: u8, count_override: Option<u8>, name: &[u8]) -> Vec<u8> {
        let mut s = vec![0u8; 0x19];
        s.extend(0x10u8..0x1C); // key bytes +0x19..+0x24
        s.extend_from_slice(&[tag, b1, 0, 0]); // dword at +0x25
        s.push(count_override.unwrap_or(name.len() as u8)); // +0x29
        s.extend_from_slice(name);
        s
    }

    /// Unicorn (`tools/emu/game_start_x_emu.py vectors`): record tag 8, byte1 0x16, name "Ab" -> X 0x02000016.
    #[test]
    fn fold_input_matches_unicorn_vectors() {
        let a = sub_record(8, 0x16, None, b"Ab");
        assert_eq!(a.len(), 44);
        assert_eq!(fold_input(&a), Some(0x0200_0016));
        // tag 8, byte1 0, name "Xyz1" -> X 0x04000000 (the name's first bytes do not enter X).
        let b = sub_record(8, 0x00, None, b"Xyz1");
        assert_eq!(fold_input(&b), Some(0x0400_0000));
        // tag 4: the caller's limit is 4, the second read fails, the fold step does not run.
        let c = sub_record(4, 0x03, None, b"Q");
        assert_eq!(fold_input(&c), None);
    }

    #[test]
    fn fold_input_uses_the_count_byte_not_the_name_length_field() {
        // X's top byte is the count byte at +0x29; a different count changes X even with equal names.
        let a = sub_record(8, 0x16, Some(5), b"Ab");
        assert_eq!(fold_input(&a), Some(0x0500_0016));
    }

    #[test]
    fn fold_is_the_slot_fold_over_present_slots() {
        let a = sub_record(8, 0x16, None, b"Ab");
        let b = sub_record(8, 0x00, None, b"Xyz1");
        let skipped = sub_record(4, 0x03, None, b"Q");
        let want = sim::rng::slot_fold([0x0200_0016, 0x0400_0000]);
        assert_eq!(fold_seed([a.as_slice(), skipped.as_slice(), b.as_slice()]), want);
    }

    /// A replay body with two slots and the given seed (the layout of the synthetic replay below).
    fn start_body(seed: u32, spots: u8) -> Vec<u8> {
        let mut rec = vec![2u8];
        rec.extend_from_slice(&[1, 100, 2, 0, 0, 0, 0x20, 1, 100]);
        rec.extend_from_slice(&[2, 100, 2, 0, 1, 1, 0x20, 1, 100]);
        rec.extend_from_slice(&seed.to_le_bytes());
        rec.extend_from_slice(&[0x09, spots]);
        rec
    }

    #[test]
    fn accepted_record_seed_is_used_on_the_record_path() {
        let body = start_body(0xCAFE_F00D, 2);
        let subs = [sub_record(8, 0x16, None, b"Ab")];
        let refs: Vec<&[u8]> = subs.iter().map(|s| s.as_slice()).collect();
        assert_eq!(
            setup_seed(Some(&body), 5, &refs),
            Some((0xCAFE_F00D, SetupPath::Record))
        );
        assert_eq!(setup_seed(Some(&body), 1, &refs), Some((0xCAFE_F00D, SetupPath::Record)));
    }

    #[test]
    fn refused_record_with_mode_5_folds() {
        let body = start_body(0xCAFE_F00D, 3); // more start spots than slots: refused
        let subs = [sub_record(8, 0x16, None, b"Ab"), sub_record(8, 0x00, None, b"Xyz1")];
        let refs: Vec<&[u8]> = subs.iter().map(|s| s.as_slice()).collect();
        let want = sim::rng::slot_fold([0x0200_0016, 0x0400_0000]);
        assert_eq!(setup_seed(Some(&body), 5, &refs), Some((want, SetupPath::Fold)));
        assert_eq!(setup_seed(None, 5, &refs), Some((want, SetupPath::Fold)));
    }

    #[test]
    fn refused_record_other_mode_is_not_modelled() {
        let refs: Vec<&[u8]> = Vec::new();
        assert_eq!(setup_seed(None, 1, &refs), None);
    }

    #[test]
    fn record_seed_is_the_replay_seed_field() {
        // The synthetic replay of the tests below: its 0x19 body parses to the same seed as GameInfo.
        let r = synthetic_replay(0x1234_5678);
        let body = start_body(0x1234_5678, 2);
        assert_eq!(w3g::start::parse_start_body(&body).unwrap().seed, r.game.random_seed);
    }

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
