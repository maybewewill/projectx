use crate::reader::Reader;
use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRecord {
    /// 0x00 for the host (first record), 0x16 for additional players.
    pub record_id: u8,
    pub player_id: u8,
    pub name: Vec<u8>,
    /// 1 byte (custom game) or 8 bytes (ladder: runtime u32, race u32).
    pub extra: Vec<u8>,
}

/// Decoded "stat string" carried in the replay (same layout as the Battle.net game stat string).
/// Field meanings beyond `map_path`/`host_name` are best-effort until checked against real replays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSettings {
    pub flags: u32,
    pub map_width: u16,
    pub map_height: u16,
    pub map_checksum: u32,
    pub map_path: Vec<u8>,
    pub host_name: Vec<u8>,
    /// Full decoded bytes, kept for fields not yet understood.
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub player_id: u8,
    pub download_percent: u8,
    /// 0 empty, 1 closed, 2 used.
    pub status: u8,
    pub computer: u8,
    pub team: u8,
    pub color: u8,
    pub race: u8,
    pub ai_strength: u8,
    pub handicap: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    pub host: PlayerRecord,
    pub game_name: Vec<u8>,
    pub settings: GameSettings,
    pub slot_count: u32,
    pub game_type: u32,
    pub language_id: u32,
    pub players: Vec<PlayerRecord>,
    pub slots: Vec<Slot>,
    /// Seed for the game's deterministic RNG — required to reproduce the simulation.
    pub random_seed: u32,
    pub select_mode: u8,
    pub start_spot_count: u8,
}

/// Decodes the stat-string encoding: every group of up to 7 bytes is preceded by a mask byte;
/// a byte whose mask bit is clear was stored incremented by one (so the stream contains no zeros).
pub fn decode_string(enc: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(enc.len());
    for chunk in enc.chunks(8) {
        let mask = chunk[0];
        for (j, &b) in chunk.iter().enumerate().skip(1) {
            out.push(if mask & (1 << j) == 0 {
                b.wrapping_sub(1)
            } else {
                b
            });
        }
    }
    out
}

/// Inverse of [`decode_string`].
pub fn encode_string(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 7 + 1);
    for chunk in data.chunks(7) {
        let mut mask = 1u8;
        for (j, &b) in chunk.iter().enumerate() {
            if b % 2 == 1 {
                mask |= 1 << (j + 1);
            }
        }
        out.push(mask);
        for &b in chunk {
            out.push(if b % 2 == 1 { b } else { b + 1 });
        }
    }
    out
}

fn parse_player(r: &mut Reader) -> Result<PlayerRecord, Error> {
    let record_id = r.u8()?;
    let player_id = r.u8()?;
    let name = r.cstr()?.to_vec();
    let extra_len = r.u8()? as usize;
    let extra = r.bytes(extra_len)?.to_vec();
    Ok(PlayerRecord {
        record_id,
        player_id,
        name,
        extra,
    })
}

fn parse_settings(raw: Vec<u8>) -> Result<GameSettings, Error> {
    let mut r = Reader::new(&raw);
    let flags = r.u32()?;
    let _zero = r.u8()?;
    let map_width = r.u16()?;
    let map_height = r.u16()?;
    let map_checksum = r.u32()?;
    let map_path = r.cstr()?.to_vec();
    let host_name = r.cstr()?.to_vec();
    Ok(GameSettings {
        flags,
        map_width,
        map_height,
        map_checksum,
        map_path,
        host_name,
        raw,
    })
}

pub(crate) fn parse_game_info(r: &mut Reader) -> Result<GameInfo, Error> {
    let _unknown = r.u32()?;
    let host = parse_player(r)?;
    let game_name = r.cstr()?.to_vec();
    let _empty = r.cstr()?;
    let settings = parse_settings(decode_string(r.cstr()?))?;
    let slot_count = r.u32()?;
    let game_type = r.u32()?;
    let language_id = r.u32()?;

    let mut players = Vec::new();
    while r.peek() == Some(0x16) {
        players.push(parse_player(r)?);
        let _unknown = r.u32()?;
    }

    let offset = r.pos();
    let id = r.u8()?;
    if id != 0x19 {
        return Err(Error::UnexpectedRecord { offset, id });
    }
    let len = r.u16()? as usize;
    let body = r.bytes(len)?;
    let mut g = Reader::new(body);
    let nslots = g.u8()? as usize;
    // Slot record size grew over patches (7, 8, then 9 bytes); derive it from the record length:
    // len = 1 (count) + nslots * size + 4 (seed) + 1 (select mode) + 1 (start spots).
    let slot_size = len.saturating_sub(7).checked_div(nslots).unwrap_or(9);
    if slot_size < 7 {
        return Err(Error::BadSlotRecord {
            offset,
            len,
            nslots,
        });
    }
    let mut slots = Vec::with_capacity(nslots);
    for _ in 0..nslots {
        let s = g.bytes(slot_size)?;
        let at = |i: usize, default: u8| s.get(i).copied().unwrap_or(default);
        slots.push(Slot {
            player_id: s[0],
            download_percent: s[1],
            status: s[2],
            computer: s[3],
            team: s[4],
            color: s[5],
            race: s[6],
            ai_strength: at(7, 1),
            handicap: at(8, 100),
        });
    }
    let random_seed = g.u32()?;
    let select_mode = g.u8()?;
    let start_spot_count = g.u8()?;

    Ok(GameInfo {
        host,
        game_name,
        settings,
        slot_count,
        game_type,
        language_id,
        players,
        slots,
        random_seed,
        select_mode,
        start_spot_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_codec_roundtrip() {
        let data: Vec<u8> = (0u8..=255).chain([0, 0, 1, 2, 3]).collect();
        let enc = encode_string(&data);
        assert!(!enc.contains(&0));
        assert_eq!(decode_string(&enc), data);
    }
}
