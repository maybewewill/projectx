use crate::actions::{decode_actions, Action};
use crate::reader::Reader;
use crate::Error;

/// Commands issued by one player inside a time slot.
///
/// `actions` is the raw sub-stream; `offset` is its absolute position in the decompressed replay, used
/// in decoding errors. [`CommandBlock::decode`] gives the typed actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandBlock {
    pub player_id: u8,
    pub offset: usize,
    pub actions: Vec<u8>,
}

impl CommandBlock {
    /// Decodes the typed actions of this player's sub-stream, in stream order.
    pub fn decode(&self) -> Result<Vec<Action>, Error> {
        decode_actions(&self.actions, self.offset)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// 0x17
    LeaveGame {
        reason: u32,
        player_id: u8,
        result: u32,
        unknown: u32,
    },
    /// 0x1A, 0x1B, 0x1C — emitted before the first time slot.
    StartBlock { id: u8, value: u32 },
    /// 0x1E (old) / 0x1F — one lockstep tick.
    TimeSlot {
        increment_ms: u16,
        commands: Vec<CommandBlock>,
    },
    /// 0x20
    Chat {
        player_id: u8,
        flags: u8,
        mode: Option<u32>,
        message: Vec<u8>,
    },
    /// 0x22 — the per-tick sync checksum of the time slot that precedes it (CONFIRMED on a real 1.26a
    /// replay: one record after every time slot, payload = `u8 4` + `u32` LE). The value is the
    /// `CNetData+0x1C64` word the original compares with its own (docs/spec/lockstep.md section 12).
    Checksum { csum: u32 },
    /// 0x23
    Unknown23 { data: [u8; 10] },
    /// 0x2F
    ForcedGameEnd { mode: u32, countdown_s: u32 },
}

impl Block {
    pub fn checksum_u32(&self) -> Option<u32> {
        match self {
            Block::Checksum { csum } => Some(*csum),
            _ => None,
        }
    }
}

/// Splits a time slot's payload into per-player command blocks (`base` = absolute offset of `body`).
fn parse_commands(body: &[u8], base: usize) -> Result<Vec<CommandBlock>, Error> {
    let mut r = Reader::new(body);
    let mut out = Vec::new();
    while r.remaining() > 0 {
        let player_id = r.u8()?;
        let len = r.u16()? as usize;
        let offset = base + r.pos();
        out.push(CommandBlock {
            player_id,
            offset,
            actions: r.bytes(len)?.to_vec(),
        });
    }
    Ok(out)
}

/// Parses replay data blocks until the end of the buffer or zero padding.
pub(crate) fn parse_blocks(r: &mut Reader) -> Result<Vec<Block>, Error> {
    let mut blocks = Vec::new();
    while let Some(id) = r.peek() {
        let offset = r.pos();
        r.u8()?;
        let block = match id {
            0x00 => break,
            0x17 => Block::LeaveGame {
                reason: r.u32()?,
                player_id: r.u8()?,
                result: r.u32()?,
                unknown: r.u32()?,
            },
            0x1A..=0x1C => Block::StartBlock {
                id,
                value: r.u32()?,
            },
            0x1E | 0x1F => {
                let n = r.u16()? as usize;
                if n < 2 {
                    return Err(Error::BadBlockLength { offset, id, len: n });
                }
                let increment_ms = r.u16()?;
                let body_offset = r.pos();
                let commands = parse_commands(r.bytes(n - 2)?, body_offset)?;
                Block::TimeSlot {
                    increment_ms,
                    commands,
                }
            }
            0x20 => {
                let player_id = r.u8()?;
                let n = r.u16()? as usize;
                let mut c = Reader::new(r.bytes(n)?);
                let flags = c.u8()?;
                let mode = if flags == 0x20 { Some(c.u32()?) } else { None };
                Block::Chat {
                    player_id,
                    flags,
                    mode,
                    message: c.cstr()?.to_vec(),
                }
            }
            0x22 => {
                let n = r.u8()? as usize;
                if n != 4 {
                    return Err(Error::BadBlockLength { offset, id, len: n });
                }
                Block::Checksum {
                    csum: r.u32()?,
                }
            }
            0x23 => Block::Unknown23 { data: r.array()? },
            0x2F => Block::ForcedGameEnd {
                mode: r.u32()?,
                countdown_s: r.u32()?,
            },
            _ => return Err(Error::UnknownBlock { offset, id }),
        };
        blocks.push(block);
    }
    Ok(blocks)
}
