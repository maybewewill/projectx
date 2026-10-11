//! `war3map.doo`: doodad placements and special doodads.
//!
//! Two versions exist: TFT (version 8, subversion 11) and Reign of Chaos (version 7,
//! subversion 9). ROC records end after the item table pointer (42 bytes); TFT records add
//! the dropped item sets and an editor id. Both layouts were checked on maps by parsing every
//! record to the exact end of the section (BootyBay ROC, Circumvention TFT).
//!
//! Doodad record (little-endian): type id 4 chars, variation u32, position 3 x f32,
//! angle f32, scale 3 x f32, flags u8, life u8, item table pointer u32, and for TFT only:
//! dropped item sets (count u32, each: item count u32 then items of 4-char id + chance u32),
//! editor id u32.
//!
//! Special doodads follow the doodad list: version u32 (0), count u32, then per entry
//! 4-char id, x i32, y i32 and z f32. The meaning of x/y/z units and the z type are
//! uncertain and not checked against real maps.

use crate::reader::Reader;
use crate::{check_magic, check_version, Error};

const FILE: &str = "war3map.doo";
const MAGIC: [u8; 4] = *b"W3do";
const VERSION: u32 = 8;
const SUBVERSION: u32 = 11;
/// Reign of Chaos files (`W3do` version 7, subversion 9): shorter doodad records.
const ROC_VERSION: u32 = 7;
const SPECIAL_VERSION: u32 = 0;
/// Smallest TFT doodad record with no dropped item sets.
const MIN_DOODAD_SIZE: usize = 4 + 4 + 12 + 4 + 12 + 1 + 1 + 4 + 4 + 4;
/// Reign of Chaos doodad record (42 bytes).
const MIN_ROC_DOODAD_SIZE: usize = 4 + 4 + 12 + 4 + 12 + 1 + 1 + 4;
const MIN_SPECIAL_SIZE: usize = 4 + 4 + 4 + 4;
const MIN_ITEM_SET_SIZE: usize = 4;
const DROPPED_ITEM_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DroppedItem {
    pub id: [u8; 4],
    pub chance: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Doodad {
    pub id: [u8; 4],
    pub variation: u32,
    pub position: [f32; 3],
    pub angle: f32,
    pub scale: [f32; 3],
    /// Raw flags byte (visibility, selectability and similar bits).
    pub flags: u8,
    /// Life percentage for destructibles, raw.
    pub life: u8,
    /// Item table pointer; `0xFFFFFFFF` means none.
    pub item_table_ptr: u32,
    pub dropped_item_sets: Vec<Vec<DroppedItem>>,
    pub editor_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpecialDoodad {
    pub id: [u8; 4],
    pub x: i32,
    pub y: i32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Doodads {
    pub doodads: Vec<Doodad>,
    pub special: Vec<SpecialDoodad>,
}

impl Doodads {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        check_magic(FILE, r.array()?, &MAGIC)?;
        let version = r.u32()?;
        match version {
            ROC_VERSION => {
                // Reign of Chaos maps: a subversion field follows (9 in every map checked).
                r.u32()?;
            }
            _ => {
                check_version(FILE, "version", version, VERSION)?;
                check_version(FILE, "subversion", r.u32()?, SUBVERSION)?;
            }
        }
        let min_size = if version == ROC_VERSION {
            MIN_ROC_DOODAD_SIZE
        } else {
            MIN_DOODAD_SIZE
        };

        let count = r.count(min_size, "doodads")?;
        let mut doodads = Vec::with_capacity(count);
        for _ in 0..count {
            doodads.push(read_doodad(&mut r, version == ROC_VERSION)?);
        }

        check_version(FILE, "special doodad version", r.u32()?, SPECIAL_VERSION)?;
        let special_count = r.count(MIN_SPECIAL_SIZE, "special doodads")?;
        let mut special = Vec::with_capacity(special_count);
        for _ in 0..special_count {
            special.push(SpecialDoodad {
                id: r.array()?,
                x: r.i32()?,
                y: r.i32()?,
                z: r.f32()?,
            });
        }

        Ok(Self { doodads, special })
    }
}

fn read_doodad(r: &mut Reader<'_>, roc: bool) -> Result<Doodad, Error> {
    let id = r.array()?;
    let variation = r.u32()?;
    let position = [r.f32()?, r.f32()?, r.f32()?];
    let angle = r.f32()?;
    let scale = [r.f32()?, r.f32()?, r.f32()?];
    let flags = r.u8()?;
    let life = r.u8()?;
    let item_table_ptr = r.u32()?;
    if roc {
        // Reign of Chaos records end after the item table pointer: no dropped item sets
        // and no editor id (42 bytes per record, checked on BootyBay).
        return Ok(Doodad {
            id,
            variation,
            position,
            angle,
            scale,
            flags,
            life,
            item_table_ptr,
            dropped_item_sets: Vec::new(),
            editor_id: 0,
        });
    }

    let set_count = r.count(MIN_ITEM_SET_SIZE, "dropped item sets")?;
    let mut dropped_item_sets = Vec::with_capacity(set_count);
    for _ in 0..set_count {
        let item_count = r.count(DROPPED_ITEM_SIZE, "dropped items")?;
        let mut items = Vec::with_capacity(item_count);
        for _ in 0..item_count {
            items.push(DroppedItem {
                id: r.array()?,
                chance: r.u32()?,
            });
        }
        dropped_item_sets.push(items);
    }

    let editor_id = r.u32()?;
    Ok(Doodad {
        id,
        variation,
        position,
        angle,
        scale,
        flags,
        life,
        item_table_ptr,
        dropped_item_sets,
        editor_id,
    })
}
