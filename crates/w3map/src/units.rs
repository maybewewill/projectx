//! `war3mapUnits.doo`: pre-placed units, items and start locations (TFT version 8, subversion 11).
//!
//! Header: magic `W3do`, version u32 (8), subversion u32 (11), unit count u32.
//!
//! Unit record (offsets from the record start), verified on Circumvention: all 40 records
//! consume the file exactly, and the start locations, gold mines and neutral buildings
//! decode to plausible values.
//! - type id 4 chars (0), variation u32 (4), position 3 x f32 (8), angle f32 (20),
//!   scale 3 x f32 (24)
//! - flags u8 (36), player u32 (37; 15 = neutral passive), unknown u16 (41)
//! - hit points i32 (43; -1 = default), mana i32 (47; -1 = default)
//! - item table pointer i32 (51; -1 = none), unknown u32 (55)
//! - gold u32 (59; 12500 default on mines), target acquisition f32 (63; -1 = default),
//!   hero level u32 (67)
//! - strength, agility, intelligence u32 (71..83)
//! - item count u32 (83), then per item: slot u32, item id 4 chars
//! - ability count u32, then per ability: id 4 chars, active u32, level u32
//! - random flag u32 (0 = fixed unit), three unknown u32 values (`[1, -1, -1]`), editor id u32
//!
//! Reign of Chaos files (version 7) use a different record (91 bytes fixed, no hero stats,
//! no random flag) that is NOT decoded here: BootyBay (61 units) has records that carry three
//! extra integers before the target acquisition field, and parsing them is not verified.
//! Such files are rejected with `UnsupportedVersion`.
//!
//! The random-unit extension (random flag != 0) is not present in the checked map and its
//! extra fields are uncertain, so such records are reported as `InvalidValue`.

use crate::reader::Reader;
use crate::{check_magic, check_version, Error};

const FILE: &str = "war3mapUnits.doo";
const MAGIC: [u8; 4] = *b"W3do";
const VERSION: u32 = 8;
const SUBVERSION: u32 = 11;
/// Fixed part of a record (everything but the item and ability entries): 111 bytes.
const MIN_UNIT_SIZE: usize = 111;
const ITEM_SIZE: usize = 4 + 4;
const ABILITY_SIZE: usize = 4 + 4 + 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitItem {
    pub slot: u32,
    pub id: [u8; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitAbility {
    pub id: [u8; 4],
    pub active: u32,
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedUnit {
    pub id: [u8; 4],
    pub variation: u32,
    pub position: [f32; 3],
    pub angle: f32,
    pub scale: [f32; 3],
    /// Raw flags byte.
    pub flags: u8,
    /// Owner player slot (0..15; 12..15 are the neutral slots).
    pub player: u32,
    pub unknown_u16: u16,
    /// Hit points; -1 means the type default.
    pub hit_points: i32,
    /// Mana points; -1 means the type default.
    pub mana: i32,
    /// Item table pointer; -1 means none.
    pub item_table_ptr: i32,
    pub unknown_u32: u32,
    /// Gold of a gold mine; 0 for other units.
    pub gold: u32,
    /// Target acquisition range; -1 means the type default.
    pub target_acquisition: f32,
    pub hero_level: u32,
    pub strength: u32,
    pub agility: u32,
    pub intelligence: u32,
    pub items: Vec<UnitItem>,
    pub abilities: Vec<UnitAbility>,
    /// Three unknown u32 values after the random flag (`[1, -1, -1]` in the test map).
    pub unknown_tail: [u32; 3],
    pub editor_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedUnits {
    pub units: Vec<PlacedUnit>,
}

impl PlacedUnits {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        check_magic(FILE, r.array()?, &MAGIC)?;
        check_version(FILE, "version", r.u32()?, VERSION)?;
        check_version(FILE, "subversion", r.u32()?, SUBVERSION)?;

        let count = r.count(MIN_UNIT_SIZE, "units")?;
        let mut units = Vec::with_capacity(count);
        for _ in 0..count {
            units.push(read_unit(&mut r)?);
        }
        Ok(Self { units })
    }
}

fn read_unit(r: &mut Reader<'_>) -> Result<PlacedUnit, Error> {
    let id = r.array()?;
    let variation = r.u32()?;
    let position = [r.f32()?, r.f32()?, r.f32()?];
    let angle = r.f32()?;
    let scale = [r.f32()?, r.f32()?, r.f32()?];
    let flags = r.u8()?;
    let player = r.u32()?;
    let unknown_u16 = r.u16()?;
    let hit_points = r.i32()?;
    let mana = r.i32()?;
    let item_table_ptr = r.i32()?;
    let unknown_u32 = r.u32()?;
    let gold = r.u32()?;
    let target_acquisition = r.f32()?;
    let hero_level = r.u32()?;
    let strength = r.u32()?;
    let agility = r.u32()?;
    let intelligence = r.u32()?;

    let item_count = r.count(ITEM_SIZE, "unit items")?;
    let mut items = Vec::with_capacity(item_count);
    for _ in 0..item_count {
        items.push(UnitItem {
            slot: r.u32()?,
            id: r.array()?,
        });
    }

    let ability_count = r.count(ABILITY_SIZE, "unit abilities")?;
    let mut abilities = Vec::with_capacity(ability_count);
    for _ in 0..ability_count {
        abilities.push(UnitAbility {
            id: r.array()?,
            active: r.u32()?,
            level: r.u32()?,
        });
    }

    let random = r.u32()?;
    if random != 0 {
        return Err(Error::InvalidValue {
            file: FILE,
            field: "unit random flag",
            value: random,
        });
    }
    let unknown_tail = [r.u32()?, r.u32()?, r.u32()?];
    let editor_id = r.u32()?;

    Ok(PlacedUnit {
        id,
        variation,
        position,
        angle,
        scale,
        flags,
        player,
        unknown_u16,
        hit_points,
        mana,
        item_table_ptr,
        unknown_u32,
        gold,
        target_acquisition,
        hero_level,
        strength,
        agility,
        intelligence,
        items,
        abilities,
        unknown_tail,
        editor_id,
    })
}
