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

/// Playable area for [`sim::ScriptHost::set_playable`].
///
/// UNSURE. No spec in `docs/spec` gives the formula. This uses the camera bounds as stored in
/// `w3i` (left, right, bottom, top) as the playable rectangle and ignores the camera complements
/// and the playable width and height. The real game may derive it differently, for example from
/// the terrain grid, so this must be checked against a 1.26a run.
pub fn playable_area(info: &MapInfo) -> Playable {
    let b = info.camera_bounds;
    Playable {
        min_x: F32::from_bits(b[0].to_bits()),
        min_y: F32::from_bits(b[2].to_bits()),
        max_x: F32::from_bits(b[1].to_bits()),
        max_y: F32::from_bits(b[3].to_bits()),
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
