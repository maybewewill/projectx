//! The game-start record body (block `0x19`) the way the 1.26a engine reads it.
//!
//! Two machine-code functions of `Game.dll` handle the body (see `docs/spec/game-start.md`, section 4):
//!
//! - `0x6F6559A0` (stream parser): a count byte `n` (rejected when `n >= 16`), then `n` records of
//!   9 bytes (read by `0x6F653E40`), then a dword (the random seed), a byte (select mode) and a byte
//!   (start spot count). Trailing bytes are not read or checked.
//! - `0x6F5BE740` (acceptance): the parsed record is accepted only when `n != 0` and the start
//!   spot count is not greater than `n` (unsigned compare at `0x6F5BE75A`). The duplicate check
//!   that follows in the original is not modelled here (UNSURE).
//!
//! The body is the same layout [`crate::GameInfo`] reads from the `0x19` block; this module keeps
//! the parts the engine needs for game-start seeding, with the same limits.

/// Size of one slot record inside the body (CONFIRMED by the `eax*9` stride at `0x6F6559D4`).
pub const SLOT_RECORD_SIZE: usize = 9;

/// Largest slot count the parser accepts (`cmp al, 0x10; jae` at `0x6F6559BC`).
pub const MAX_SLOTS: u8 = 15;

/// One parsed game-start record body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRecord {
    /// The raw 9-byte slot records, in body order.
    pub slots: Vec<[u8; SLOT_RECORD_SIZE]>,
    /// The random seed field: the value the original stores in the seed slot of the game-start
    /// container (`+0x0C`), which the seeding tail passes to the global generator when the
    /// fixed-seed flag is clear (LIKELY link to the replay, see the spec).
    pub seed: u32,
    /// Select mode byte (container `+0x10`).
    pub select_mode: u8,
    /// Start spot count byte (container `+0x11`).
    pub start_spots: u8,
}

/// Why a body was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRecordError {
    /// The body is empty or ends before the fields it declares.
    Truncated,
    /// The slot count byte is 16 or more; the original's parser refuses the record.
    TooManySlots(u8),
    /// No slots: the original's acceptance check refuses a record with count 0.
    NoSlots,
    /// The start spot count is greater than the slot count (the original refuses it).
    StartSpotsAboveSlots { slots: u8, start_spots: u8 },
}

impl std::fmt::Display for StartRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartRecordError::Truncated => write!(f, "game-start record is truncated"),
            StartRecordError::TooManySlots(n) => write!(f, "game-start record has {n} slots (max 15)"),
            StartRecordError::NoSlots => write!(f, "game-start record has no slots"),
            StartRecordError::StartSpotsAboveSlots { slots, start_spots } => write!(
                f,
                "game-start record has {start_spots} start spots for {slots} slots"
            ),
        }
    }
}

impl std::error::Error for StartRecordError {}

/// Parses a `0x19` body (the bytes after the 2-byte block length) with the original's limits.
///
/// Returns the first error the original would hit: a count of 16 or more, a truncated body, then
/// the acceptance rules (no slots, or more start spots than slots).
pub fn parse_start_body(body: &[u8]) -> Result<StartRecord, StartRecordError> {
    let (&n, rest) = body.split_first().ok_or(StartRecordError::Truncated)?;
    if n > MAX_SLOTS {
        return Err(StartRecordError::TooManySlots(n));
    }
    let slot_bytes = usize::from(n) * SLOT_RECORD_SIZE;
    if rest.len() < slot_bytes + 6 {
        return Err(StartRecordError::Truncated);
    }
    let (slot_data, tail) = rest.split_at(slot_bytes);
    let slots = slot_data
        .chunks_exact(SLOT_RECORD_SIZE)
        .map(|c| {
            let mut s = [0u8; SLOT_RECORD_SIZE];
            s.copy_from_slice(c);
            s
        })
        .collect();
    let seed = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
    let select_mode = tail[4];
    let start_spots = tail[5];
    let rec = StartRecord {
        slots,
        seed,
        select_mode,
        start_spots,
    };
    rec.check_accepted()?;
    Ok(rec)
}

impl StartRecord {
    /// The acceptance rules of `0x6F5BE740` (count 0 is refused; start spots must not exceed the
    /// slot count). Called by [`parse_start_body`]; exposed for records built in code.
    pub fn check_accepted(&self) -> Result<(), StartRecordError> {
        let n = self.slots.len() as u8;
        if n == 0 {
            return Err(StartRecordError::NoSlots);
        }
        if self.start_spots > n {
            return Err(StartRecordError::StartSpotsAboveSlots {
                slots: n,
                start_spots: self.start_spots,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(slots: &[[u8; 9]], seed: u32, mode: u8, spots: u8) -> Vec<u8> {
        let mut b = vec![slots.len() as u8];
        for s in slots {
            b.extend_from_slice(s);
        }
        b.extend_from_slice(&seed.to_le_bytes());
        b.push(mode);
        b.push(spots);
        b
    }

    const S0: [u8; 9] = [1, 100, 2, 0, 0, 0, 0x20, 1, 100];
    const S1: [u8; 9] = [2, 100, 2, 0, 1, 1, 0x20, 1, 100];

    #[test]
    fn parses_seed_after_nine_byte_slots() {
        let b = body(&[S0, S1], 0xDEAD_BEEF, 9, 2);
        let r = parse_start_body(&b).unwrap();
        assert_eq!(r.slots, vec![S0, S1]);
        assert_eq!(r.seed, 0xDEAD_BEEF);
        assert_eq!(r.select_mode, 9);
        assert_eq!(r.start_spots, 2);
    }

    #[test]
    fn seed_offset_is_one_plus_nine_per_slot() {
        // Same bytes as the synthetic replay in the engine tests: the seed starts at 1 + 9n.
        let b = body(&[S0, S1], 0x1234_5678, 0, 0);
        assert_eq!(&b[1 + 9 * 2..1 + 9 * 2 + 4], &0x1234_5678u32.to_le_bytes());
    }

    #[test]
    fn sixteen_slots_are_refused() {
        let mut b = vec![16u8];
        b.resize(1 + 16 * 9 + 6, 0);
        assert_eq!(parse_start_body(&b), Err(StartRecordError::TooManySlots(16)));
    }

    #[test]
    fn zero_slots_are_refused_after_parsing() {
        let b = body(&[], 7, 0, 0);
        assert_eq!(parse_start_body(&b), Err(StartRecordError::NoSlots));
    }

    #[test]
    fn start_spots_above_slots_are_refused() {
        let b = body(&[S0], 7, 0, 2);
        assert_eq!(
            parse_start_body(&b),
            Err(StartRecordError::StartSpotsAboveSlots { slots: 1, start_spots: 2 })
        );
    }

    #[test]
    fn truncated_bodies_are_refused() {
        let b = body(&[S0, S1], 7, 0, 2);
        for cut in 0..b.len() {
            assert!(parse_start_body(&b[..cut]).is_err(), "cut {cut}");
        }
    }

    #[test]
    fn trailing_bytes_are_ignored() {
        let mut b = body(&[S0], 7, 0, 1);
        b.extend_from_slice(&[0xFF, 0xFF]);
        assert_eq!(parse_start_body(&b).unwrap().seed, 7);
    }
}
