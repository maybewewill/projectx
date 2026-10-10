//! `war3map.doo`: doodad placements (TFT version 8, subversion 11) and special doodads.
//!
//! Doodad record (little-endian): type id 4 chars, variation u32, position 3 x f32,
//! angle f32, scale 3 x f32, flags u8, life u8, item table pointer u32, dropped item
//! sets (count u32, each: item count u32 then items of 4-char id + chance u32),
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
const SPECIAL_VERSION: u32 = 0;
/// Smallest doodad record with no dropped item sets.
const MIN_DOODAD_SIZE: usize = 4 + 4 + 12 + 4 + 12 + 1 + 1 + 4 + 4 + 4;
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
        check_version(FILE, "version", r.u32()?, VERSION)?;
        check_version(FILE, "subversion", r.u32()?, SUBVERSION)?;

        let count = r.count(MIN_DOODAD_SIZE, "doodads")?;
        let mut doodads = Vec::with_capacity(count);
        for _ in 0..count {
            doodads.push(read_doodad(&mut r)?);
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

fn read_doodad(r: &mut Reader<'_>) -> Result<Doodad, Error> {
    let id = r.array()?;
    let variation = r.u32()?;
    let position = [r.f32()?, r.f32()?, r.f32()?];
    let angle = r.f32()?;
    let scale = [r.f32()?, r.f32()?, r.f32()?];
    let flags = r.u8()?;
    let life = r.u8()?;
    let item_table_ptr = r.u32()?;

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
