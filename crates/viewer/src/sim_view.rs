//! The live units of a booted engine, for drawing them (`viewer --sim-seconds`). Read-only: it copies
//! the fields the viewer needs out of `sim::Game`; nothing here steps or changes the simulation.

use std::collections::BTreeMap;

use crate::scene_objects::{SimUnit, SimUnitSequenceState};

/// Determines the animation sequence state of a unit based on its simulation state.
/// Returns a sequence state name prefix (for case-insensitive matching in Model::find_sequence).
fn sequence_state_from_unit(
    _unit_key: u32,
    dead: bool,
    move_speed: f32,
    in_combat: bool,
    is_under_construction: bool,
) -> SimUnitSequenceState {
    if dead {
        SimUnitSequenceState::Dead
    } else if is_under_construction {
        SimUnitSequenceState::Building
    } else if in_combat {
        SimUnitSequenceState::Attacking
    } else if move_speed > 0.01 {
        SimUnitSequenceState::Moving
    } else {
        SimUnitSequenceState::Standing
    }
}

/// Copies every unit object of the game (creation order; dead, hidden and removed ones included, the
/// drawing filters them).
pub fn units(engine: &engine::Engine) -> Vec<SimUnit> {
    let host = engine.host();
    let game = host.game();
    game.unit_objects()
        .map(|(unit_key, u)| {
            // Check if the unit is under construction (progress_type != 0 in some implementations)
            // For now, we use a simple heuristic: life is between 0 and max_life but life < 0.1 * max_life
            // This would indicate construction. However, this is a placeholder.
            let is_under_construction = false; // TODO: check construction progress from engine

            // Check if the unit is in combat (has active combat state)
            let in_combat = false; // TODO: check combat state from engine

            let sequence_state = sequence_state_from_unit(
                unit_key.0,
                u.dead,
                u.move_speed.to_f32(),
                in_combat,
                is_under_construction,
            );

            SimUnit {
                type_id: u.type_id,
                owner: u.owner,
                x: u.x.to_f32(),
                y: u.y.to_f32(),
                facing: u.facing.to_f32(),
                scale: u.scale.to_f32(),
                hidden: u.hidden,
                dead: u.dead,
                removed: u.removed,
                handle: unit_key.0,
                sequence_state,
            }
        })
        .collect()
}

/// Units alive per owner slot (not dead, not removed), in slot order. Slots with no units are absent.
pub fn alive_per_owner(units: &[SimUnit]) -> BTreeMap<u32, usize> {
    let mut counts = BTreeMap::new();
    for u in units.iter().filter(|u| !u.dead && !u.removed) {
        *counts.entry(u.owner).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(owner: u32, dead: bool, removed: bool) -> SimUnit {
        SimUnit {
            type_id: 0,
            owner,
            x: 0.0,
            y: 0.0,
            facing: 0.0,
            scale: 1.0,
            hidden: false,
            dead,
            removed,
            handle: 0x100000,
            sequence_state: SimUnitSequenceState::Standing,
        }
    }

    #[test]
    fn alive_counts_skip_dead_and_removed() {
        let units = [unit(0, false, false), unit(0, true, false), unit(1, false, false), unit(1, false, true)];
        let counts = alive_per_owner(&units);
        assert_eq!(counts.get(&0), Some(&1));
        assert_eq!(counts.get(&1), Some(&1));
        assert_eq!(counts.len(), 2);
    }
}
