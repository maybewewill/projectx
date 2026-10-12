//! The live units of a booted engine, for drawing them (`viewer --sim-seconds`). Read-only: it copies
//! the fields the viewer needs out of `sim::Game`; nothing here steps or changes the simulation.

use std::collections::BTreeMap;

use crate::scene_objects::SimUnit;

/// Copies every unit object of the game (creation order; dead, hidden and removed ones included, the
/// drawing filters them).
pub fn units(engine: &engine::Engine) -> Vec<SimUnit> {
    let host = engine.host();
    let game = host.game();
    game.unit_objects()
        .map(|(_, u)| SimUnit {
            type_id: u.type_id,
            owner: u.owner,
            x: u.x.to_f32(),
            y: u.y.to_f32(),
            facing: u.facing.to_f32(),
            scale: u.scale.to_f32(),
            hidden: u.hidden,
            dead: u.dead,
            removed: u.removed,
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
