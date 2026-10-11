//! Parser for classic Warcraft III replays (`.w3g`, patch 1.26a TFT and nearby versions).
//!
//! The replay is the test oracle for the engine rewrite: it holds the initial game setup,
//! the RNG seed and every player command per lockstep tick.

mod actions;
mod blocks;
mod checksum;
mod game;
mod header;
pub mod leading;
mod reader;
pub mod start;
pub mod sync;
mod writer;

pub use checksum::{block_data_check, block_header_check, crc32, fold, header_check};
pub use writer::write_file;

pub use actions::{
    decode_actions, Action, AbilityHeader, AbilityPointObject, ObjectId, Point, NO_OBJECT,
};
pub use blocks::{Block, CommandBlock};
pub use game::{decode_string, encode_string, GameInfo, GameSettings, PlayerRecord, Slot};
pub use header::{Header, SubHeader, MAGIC};

use reader::Reader;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a Warcraft III replay (bad magic)")]
    BadMagic,
    #[error("unsupported header version {0}")]
    UnsupportedHeaderVersion(u32),
    #[error("header version {version} has size {size:#x}, expected {expected:#x}")]
    BadHeaderSize {
        version: u32,
        size: u32,
        expected: u32,
    },
    /// The header CRC (`FUN_6f534750`, error `0x0D`) does not match.
    #[error("replay header CRC {computed:#010x} does not match the stored {expected:#010x}")]
    HeaderCrc { expected: u32, computed: u32 },
    /// The block's 8-byte header CRC (`FUN_6f535050`, error `0x13`) does not match.
    #[error("block {block}: header CRC does not match")]
    BlockHeaderCrc { block: u32 },
    /// The block's compressed payload CRC (`FUN_6f535050`, error `0x19`) does not match.
    #[error("block {block}: payload CRC does not match")]
    BlockDataCrc { block: u32 },
    #[error("unexpected end of data at offset {offset:#x} (wanted {wanted} bytes)")]
    UnexpectedEof { offset: usize, wanted: usize },
    #[error("unterminated string at offset {offset:#x}")]
    UnterminatedString { offset: usize },
    #[error("failed to inflate block {block}: {source}")]
    Inflate { block: u32, source: std::io::Error },
    #[error("block {block} inflated to {got} bytes, expected {expected}")]
    ShortBlock {
        block: u32,
        expected: usize,
        got: usize,
    },
    #[error("unexpected record {id:#04x} at offset {offset:#x}")]
    UnexpectedRecord { offset: usize, id: u8 },
    #[error("bad slot record at offset {offset:#x}: len {len}, {nslots} slots")]
    BadSlotRecord {
        offset: usize,
        len: usize,
        nslots: usize,
    },
    #[error("bad length {len} for block {id:#04x} at offset {offset:#x}")]
    BadBlockLength { offset: usize, id: u8, len: usize },
    #[error("unknown block {id:#04x} at decompressed offset {offset:#x}")]
    UnknownBlock { offset: usize, id: u8 },
    /// An action id the original's command switch does not handle (`0x6F550730`, default case).
    #[error("unknown player action {id:#04x} at decompressed offset {offset:#x}")]
    UnknownAction { offset: usize, id: u8 },
    /// An action id the original applies that this decoder does not model yet.
    #[error("unmodelled player action {id:#04x} at decompressed offset {offset:#x}")]
    UnsupportedAction { offset: usize, id: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub header: Header,
    pub game: GameInfo,
    pub blocks: Vec<Block>,
}

impl Replay {
    pub fn parse(file: &[u8]) -> Result<Self, Error> {
        let header = header::parse_header(&mut Reader::new(file))?;
        let data = header::decompress_blocks(file, &header)?;
        let mut r = Reader::new(&data);
        let game = game::parse_game_info(&mut r)?;
        let blocks = blocks::parse_blocks(&mut r)?;
        Ok(Self {
            header,
            game,
            blocks,
        })
    }

    /// Iterates time slots as `(game_time_ms_after_slot, commands)`.
    pub fn ticks(&self) -> impl Iterator<Item = (u64, &[CommandBlock])> {
        let mut t = 0u64;
        self.blocks.iter().filter_map(move |b| match b {
            Block::TimeSlot {
                increment_ms,
                commands,
                ..
            } => {
                t += *increment_ms as u64;
                Some((t, commands.as_slice()))
            }
            _ => None,
        })
    }
}
