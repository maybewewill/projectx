//! `war3map.w3i`: map info, TFT format version 25.
//!
//! Parsed: header, names and description, camera bounds, playable size, flags, loading and
//! prologue screens, fog and environment, player and force records.
//! Everything after the force records (upgrade and tech availability, random unit and item
//! tables) is kept as raw bytes in [`MapInfo::rest`]. Those tables are not decoded here.
//!
//! Order checked against community format notes and a public open-source reader.

use crate::reader::Reader;
use crate::{check_version, Error};

const FILE: &str = "war3map.w3i";
const VERSION: u32 = 25;
/// Smallest possible player record: 4 u32 fields, empty name, 2 f32, 2 u32.
const MIN_PLAYER_SIZE: usize = 16 + 1 + 8 + 8;
/// Smallest possible force record: 2 u32 fields and an empty name.
const MIN_FORCE_SIZE: usize = 8 + 1;

/// Loading or prologue screen texts. `model` is a model path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screen {
    pub model: String,
    pub text: String,
    pub title: String,
    pub subtitle: String,
}

/// Fog settings. `color` is kept as the raw 4 bytes from the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Fog {
    pub style: u32,
    pub start_z: f32,
    pub end_z: f32,
    pub density: f32,
    pub color: [u8; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    /// Player slot number (0-based).
    pub number: u32,
    /// Raw player type (0 none, 1 human, 2 computer, 3 neutral, 4 rescuable).
    pub player_type: u32,
    /// Raw race value.
    pub race: u32,
    pub flags: u32,
    pub name: String,
    pub start_x: f32,
    pub start_y: f32,
    /// Bit `x` set means this player allies player `x` at low priority.
    pub ally_low_priorities: u32,
    /// Bit `x` set means this player allies player `x` at high priority.
    pub ally_high_priorities: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Force {
    pub flags: u32,
    /// Bit `x` set means player `x` belongs to this force.
    pub player_masks: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapInfo {
    /// Always 25 for accepted files.
    pub file_version: u32,
    pub saves: u32,
    pub editor_version: u32,
    pub name: String,
    pub author: String,
    pub description: String,
    pub recommended_players: String,
    /// Camera bounds as stored: left, right, bottom, top, then four more bounds.
    pub camera_bounds: [f32; 8],
    /// Camera complements as stored: left, right, bottom, top.
    pub camera_complements: [i32; 4],
    pub playable_width: u32,
    pub playable_height: u32,
    pub flags: u32,
    pub main_tileset: u8,
    pub campaign_background: u32,
    pub loading_screen: Screen,
    pub game_data_set: u32,
    pub prologue_screen: Screen,
    pub fog: Fog,
    /// Weather effect id, 4 chars, raw bytes.
    pub weather_id: [u8; 4],
    pub sound_environment: String,
    pub light_environment_tileset: u8,
    /// Water tint, raw 4 bytes.
    pub water_color: [u8; 4],
    pub players: Vec<Player>,
    pub forces: Vec<Force>,
    /// Undecoded tail: upgrade and tech availability, random unit and item tables.
    pub rest: Vec<u8>,
}

impl Screen {
    fn read(r: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(Self {
            model: r.cstr()?,
            text: r.cstr()?,
            title: r.cstr()?,
            subtitle: r.cstr()?,
        })
    }
}

impl MapInfo {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let file_version = r.u32()?;
        check_version(FILE, "format version", file_version, VERSION)?;
        let saves = r.u32()?;
        let editor_version = r.u32()?;
        let name = r.cstr()?;
        let author = r.cstr()?;
        let description = r.cstr()?;
        let recommended_players = r.cstr()?;
        let mut camera_bounds = [0.0f32; 8];
        for v in &mut camera_bounds {
            *v = r.f32()?;
        }
        let camera_complements = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
        let playable_width = r.u32()?;
        let playable_height = r.u32()?;
        let flags = r.u32()?;
        let main_tileset = r.u8()?;
        let campaign_background = r.u32()?;
        let loading_screen = Screen::read(&mut r)?;
        let game_data_set = r.u32()?;
        let prologue_screen = Screen::read(&mut r)?;
        let fog = Fog {
            style: r.u32()?,
            start_z: r.f32()?,
            end_z: r.f32()?,
            density: r.f32()?,
            color: r.array()?,
        };
        let weather_id = r.array()?;
        let sound_environment = r.cstr()?;
        let light_environment_tileset = r.u8()?;
        let water_color = r.array()?;

        let player_count = r.count(MIN_PLAYER_SIZE, "players")?;
        let mut players = Vec::with_capacity(player_count);
        for _ in 0..player_count {
            players.push(Player {
                number: r.u32()?,
                player_type: r.u32()?,
                race: r.u32()?,
                flags: r.u32()?,
                name: r.cstr()?,
                start_x: r.f32()?,
                start_y: r.f32()?,
                ally_low_priorities: r.u32()?,
                ally_high_priorities: r.u32()?,
            });
        }

        let force_count = r.count(MIN_FORCE_SIZE, "forces")?;
        let mut forces = Vec::with_capacity(force_count);
        for _ in 0..force_count {
            forces.push(Force {
                flags: r.u32()?,
                player_masks: r.u32()?,
                name: r.cstr()?,
            });
        }

        Ok(Self {
            file_version,
            saves,
            editor_version,
            name,
            author,
            description,
            recommended_players,
            camera_bounds,
            camera_complements,
            playable_width,
            playable_height,
            flags,
            main_tileset,
            campaign_background,
            loading_screen,
            game_data_set,
            prologue_screen,
            fog,
            weather_id,
            sound_environment,
            light_environment_tileset,
            water_color,
            players,
            forces,
            rest: r.rest().to_vec(),
        })
    }
}
