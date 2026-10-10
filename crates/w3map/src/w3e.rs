//! `war3map.w3e`: terrain heightmap, tilesets and per-point flags.
//!
//! Layout (little-endian):
//! `"W3E!"`, version u32 (= 11), main tileset u8, custom-tileset flag u32 (0/1),
//! ground tileset count u32 + 4-char ids, cliff tileset count u32 + 4-char ids,
//! width u32, height u32, center x/y f32, then `width * height` tile points of 7 bytes:
//! - ground height i16 (raw, not converted to world units)
//! - water level and edge flag u16: bits 0-13 water level, bit 14 map-edge flag
//! - flags u8: bits 0-3 ground texture, 0x10 ramp, 0x20 blight, 0x40 water, 0x80 boundary
//! - texture details u8: bits 0-4 ground variation, bits 5-7 cliff variation
//! - cliff u8: high nibble cliff texture, low nibble layer height
//!
//! Points are stored row-major with x changing fastest (assumed from community notes).

use crate::reader::Reader;
use crate::{check_magic, check_version, read_bool, Error};

const FILE: &str = "war3map.w3e";
const MAGIC: [u8; 4] = *b"W3E!";
const VERSION: u32 = 11;
const POINT_SIZE: usize = 7;

/// One decoded terrain tile point. Height and water values are raw file values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilePoint {
    pub ground_height: i16,
    /// 14-bit water level (raw).
    pub water_level: u16,
    pub map_edge: bool,
    /// Ground texture index (0-15).
    pub ground_texture: u8,
    pub ramp: bool,
    pub blight: bool,
    pub water: bool,
    pub boundary: bool,
    /// Ground variation index (0-31).
    pub ground_variation: u8,
    /// Cliff variation index (0-7).
    pub cliff_variation: u8,
    /// Cliff texture index (0-15).
    pub cliff_texture: u8,
    /// Cliff layer height (0-15).
    pub layer_height: u8,
}

impl TilePoint {
    fn decode(b: [u8; POINT_SIZE]) -> Self {
        let water_edge = u16::from_le_bytes([b[2], b[3]]);
        let flags = b[4];
        let variation = b[5];
        let cliff_layer = b[6];
        Self {
            ground_height: i16::from_le_bytes([b[0], b[1]]),
            water_level: water_edge & 0x3FFF,
            map_edge: water_edge & 0x4000 != 0,
            ground_texture: flags & 0x0F,
            ramp: flags & 0x10 != 0,
            blight: flags & 0x20 != 0,
            water: flags & 0x40 != 0,
            boundary: flags & 0x80 != 0,
            ground_variation: variation & 0x1F,
            cliff_variation: variation >> 5,
            cliff_texture: cliff_layer >> 4,
            layer_height: cliff_layer & 0x0F,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Terrain {
    pub main_tileset: u8,
    pub custom_tileset: bool,
    pub ground_tilesets: Vec<[u8; 4]>,
    pub cliff_tilesets: Vec<[u8; 4]>,
    /// Width in tile points.
    pub width: u32,
    /// Height in tile points.
    pub height: u32,
    pub center: [f32; 2],
    /// `width * height` points, row-major.
    pub points: Vec<TilePoint>,
}

impl Terrain {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        check_magic(FILE, r.array()?, &MAGIC)?;
        check_version(FILE, "version", r.u32()?, VERSION)?;
        let main_tileset = r.u8()?;
        let custom_tileset = read_bool(&mut r, FILE, "custom tileset flag")?;
        let ground_count = r.count(4, "ground tilesets")?;
        let ground_tilesets = (0..ground_count)
            .map(|_| r.array())
            .collect::<Result<Vec<[u8; 4]>, Error>>()?;
        let cliff_count = r.count(4, "cliff tilesets")?;
        let cliff_tilesets = (0..cliff_count)
            .map(|_| r.array())
            .collect::<Result<Vec<[u8; 4]>, Error>>()?;
        let width = r.u32()?;
        let height = r.u32()?;
        let center = [r.f32()?, r.f32()?];

        let point_count = (width as usize)
            .checked_mul(height as usize)
            .filter(|n| {
                n.checked_mul(POINT_SIZE)
                    .is_some_and(|b| b <= r.remaining())
            })
            .ok_or(Error::CountTooLarge {
                what: "tile points",
                count: (width as usize).saturating_mul(height as usize),
                offset: r.offset(),
            })?;
        let mut points = Vec::with_capacity(point_count);
        for _ in 0..point_count {
            points.push(TilePoint::decode(r.array()?));
        }

        Ok(Self {
            main_tileset,
            custom_tileset,
            ground_tilesets,
            cliff_tilesets,
            width,
            height,
            center,
            points,
        })
    }

    /// Tile point at column `x`, row `y`, or `None` when out of range.
    pub fn point(&self, x: u32, y: u32) -> Option<&TilePoint> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.points
            .get(y as usize * self.width as usize + x as usize)
    }
}
