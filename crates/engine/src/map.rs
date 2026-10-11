//! Map setup from `war3map.w3i`: playable area, player slots, teams and the local player.
//!
//! Everything here is a host-side guess at what the original does while loading a map. The
//! points that are not confirmed are marked UNSURE in the code and listed in the boot notes.

use sim::natives::game::PLAYER_SLOTS;
use sim::natives::{Game, Playable};
use softfloat::F32;
use w3map::MapInfo;

/// `MAP_CONTROL_USER`, the controller of a human slot (UNSURE: JASS constant values as remembered).
pub const CONTROL_USER: u32 = 0;
/// `MAP_CONTROL_COMPUTER`.
pub const CONTROL_COMPUTER: u32 = 1;
/// `MAP_CONTROL_RESCUABLE`.
pub const CONTROL_RESCUABLE: u32 = 2;
/// `MAP_CONTROL_NEUTRAL`.
pub const CONTROL_NEUTRAL: u32 = 3;
/// `MAP_CONTROL_NONE`, the controller of an empty slot.
pub const CONTROL_NONE: u32 = 5;

/// World units per terrain tile.
const TILE_UNITS: f32 = 128.0;

/// The whole map size in tiles: the playable size plus the left/right and bottom/top margins
/// (`w3i` camera complements, stored left, right, bottom, top).
pub fn map_tiles(info: &MapInfo) -> (u32, u32) {
    let c = info.camera_complements;
    (
        info.playable_width + (c[0] + c[1]) as u32,
        info.playable_height + (c[2] + c[3]) as u32,
    )
}

/// The minimum corner of the whole map in world units: the `w3e` center offset when the terrain
/// file is there. Without it the map is assumed centered on the origin (UNSURE).
pub fn map_corner(info: &MapInfo, w3e_center: Option<[f32; 2]>) -> (f32, f32) {
    match w3e_center {
        Some([x, y]) => (x, y),
        None => {
            let (w, h) = map_tiles(info);
            (
                -(w as f32) * TILE_UNITS / 2.0,
                -(h as f32) * TILE_UNITS / 2.0,
            )
        }
    }
}

/// Playable area for [`sim::ScriptHost::set_playable`]: the map corner moved in by the left and
/// bottom margins, `playable_width` x `playable_height` tiles in size.
///
/// Checked on a real 1.26a melee map ((2)Circumvention): w3e center (-6144, -6144), complements
/// 7, 7, 5, 9, playable 82 x 82 give x -5248..5248, y -5504..4992, the same rectangle the map's own
/// `SetCameraBounds` call starts from. (The `w3i` camera bounds are four corner points, already
/// inset by the camera margins, so they are not the playable area.)
pub fn playable_area(info: &MapInfo, corner: (f32, f32)) -> Playable {
    let c = info.camera_complements;
    let min_x = corner.0 + c[0] as f32 * TILE_UNITS;
    let min_y = corner.1 + c[2] as f32 * TILE_UNITS;
    let max_x = min_x + info.playable_width as f32 * TILE_UNITS;
    let max_y = min_y + info.playable_height as f32 * TILE_UNITS;
    // The values are small multiples of 128, exact in f32; build the soft floats from the bits.
    let f = |v: f32| F32::from_bits(v.to_bits());
    Playable {
        min_x: f(min_x),
        min_y: f(min_y),
        max_x: f(max_x),
        max_y: f(max_y),
    }
}

/// Maps the raw `w3i` player type to the JASS map-control value (UNSURE: the value table is not
/// in the spec). Unknown types become an empty slot.
pub fn controller_of(player_type: u32) -> u32 {
    match player_type {
        1 => CONTROL_USER,
        2 => CONTROL_COMPUTER,
        3 => CONTROL_NEUTRAL,
        4 => CONTROL_RESCUABLE,
        _ => CONTROL_NONE,
    }
}

/// Fills the player slots from the `w3i` player and force records.
///
/// Per player `n` (only `n < 16`): name, race (the raw `w3i` value, no mapping: UNSURE), controller,
/// slot state (1 playing unless the slot is empty), team (index of the first force that lists the
/// player, else 0) and start location (set to the slot number: UNSURE, the real start points come
/// from `DefineStartLocation` in `config`). The local player is the first human slot.
///
/// Player records outside the 16 slots are reported in `notes` and skipped.
pub fn apply_players(game: &mut Game, info: &MapInfo, notes: &mut Vec<String>) {
    let mut local = None;
    for p in &info.players {
        let n = p.number as usize;
        if n >= PLAYER_SLOTS {
            notes.push(format!(
                "player record {n} is outside the {PLAYER_SLOTS} slots; skipped"
            ));
            continue;
        }
        let team = info
            .forces
            .iter()
            .position(|f| f.player_masks & (1 << n) != 0)
            .map_or(0, |i| i as i32);
        let controller = controller_of(p.player_type);
        let Some(slot) = game.player_mut(n as u32) else {
            continue;
        };
        slot.name = p.name.clone();
        slot.race = p.race;
        slot.controller = controller;
        slot.slot_state = if controller == CONTROL_NONE { 0 } else { 1 };
        slot.team = team;
        slot.start_location = n as i32;
        if controller == CONTROL_USER && local.is_none() {
            local = Some(n as u32);
        }
    }
    game.set_local_player(local.unwrap_or(0));
}
