//! Replay time slots as engine turns: the typed player actions of a slot, in stream order, with the slot's
//! time increment (`docs/spec/lockstep.md` sections 1.3 and 3; `docs/spec/replay-actions.md`).
//!
//! The steps are: decode each player's sub-stream (`w3g::CommandBlock::decode`), convert the actions to
//! [`sim::commands::Command`], and push the slot to a [`sim::lockstep::TurnScheduler`]. The resulting
//! [`TurnReport`] goes to [`crate::Engine::run_turn`], which applies the commands before the ticks.

use std::fmt;

use sim::commands::{Command, ObjectRef, OrderTarget, WorldPoint};
use sim::lockstep::{TurnReport, TurnScheduler};
use softfloat::F32;
use w3g::{Action, CommandBlock, ObjectId, Point};

/// Why a slot could not be turned into commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    /// The player's sub-stream does not decode (see `w3g::Error`, which carries the offset).
    Decode(String),
    /// A selection change with a mode other than 1 (add) or 2 (remove).
    SelectionMode { offset: usize, mode: u8 },
    /// A hotkey group other than 0..=9.
    Group { offset: usize, group: u8 },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Decode(e) => write!(f, "{e}"),
            ReplayError::SelectionMode { offset, mode } => write!(
                f,
                "selection change with mode {mode} at decompressed offset {offset:#x} (expected 1 or 2)"
            ),
            ReplayError::Group { offset, group } => write!(
                f,
                "hotkey group {group} at decompressed offset {offset:#x} (expected 0..=9)"
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

fn object(o: ObjectId) -> ObjectRef {
    ObjectRef {
        id1: o.id1,
        id2: o.id2,
    }
}

fn point(p: Point) -> WorldPoint {
    WorldPoint {
        x: F32::from_bits(p.x.to_bits()),
        y: F32::from_bits(p.y.to_bits()),
    }
}

/// The order target of an ability action: an object when the pair is set, else the point, else none.
fn target(point_: Option<Point>, obj: Option<ObjectId>) -> OrderTarget {
    match (obj.filter(|o| !o.is_none()), point_) {
        (Some(o), p) => OrderTarget::Object {
            object: object(o),
            point: p.map(point),
        },
        (None, Some(p)) => OrderTarget::Point(point(p)),
        (None, None) => OrderTarget::None,
    }
}

/// Converts one decoded action of `player`. `offset` is used only for errors.
pub fn convert(player: u8, action: &Action, offset: usize) -> Result<Command, ReplayError> {
    let id = action.id();
    Ok(match action {
        Action::SetGameSpeed { setting } => Command::SetSpeed {
            player,
            setting: *setting,
        },
        Action::Ability(h) => order(player, h.flags, h.ability, target(None, None)),
        Action::AbilityPoint { header, point: p } => {
            order(player, header.flags, header.ability, target(Some(*p), None))
        }
        Action::AbilityPointObject {
            header,
            point: p,
            target: t,
        } => order(
            player,
            header.flags,
            header.ability,
            target(Some(*p), Some(*t)),
        ),
        Action::AbilityPointTwoObjects {
            header,
            point: p,
            first,
            ..
        } => order(
            player,
            header.flags,
            header.ability,
            target(Some(*p), Some(*first)),
        ),
        Action::SelectionChange { mode, units } => {
            let add = match mode {
                1 => true,
                2 => false,
                _ => return Err(ReplayError::SelectionMode { offset, mode: *mode }),
            };
            Command::Select {
                player,
                add,
                units: units.iter().map(|u| object(*u)).collect(),
            }
        }
        Action::AssignGroup { group, units } => {
            if *group > 9 {
                return Err(ReplayError::Group {
                    offset,
                    group: *group,
                });
            }
            Command::AssignGroup {
                player,
                group: *group,
                units: units.iter().map(|u| object(*u)).collect(),
            }
        }
        Action::SelectGroup { group, .. } => {
            if *group > 9 {
                return Err(ReplayError::Group {
                    offset,
                    group: *group,
                });
            }
            Command::SelectGroup {
                player,
                group: *group,
            }
        }
        Action::SelectSubgroup { item, unit } => Command::SelectSubgroup {
            player,
            item: *item,
            unit: object(*unit),
        },
        Action::PreSubselection => Command::PreSubselection { player },
        // 0x14 and 0x15 (item and two-point layouts) and every other action: recorded only, see
        // `docs/spec/replay-actions.md` section 6.
        _ => Command::Recorded { player, id },
    })
}

fn order(player: u8, flags: u16, id: u32, target: OrderTarget) -> Command {
    Command::Order {
        player,
        flags,
        id,
        target,
    }
}

/// Decodes one player's block into `(player, command)` pairs in stream order.
pub fn commands_of_block(block: &CommandBlock) -> Result<Vec<(u8, Command)>, ReplayError> {
    let actions = block
        .decode()
        .map_err(|e| ReplayError::Decode(e.to_string()))?;
    // The offset of each action is not kept by the decoder; errors name the block's start.
    actions
        .iter()
        .map(|a| Ok((block.player_id, convert(block.player_id, a, block.offset)?)))
        .collect()
}

/// Decodes a whole time slot (all players, in stream order) and pushes it to the scheduler.
pub fn push_time_slot(
    scheduler: &mut TurnScheduler,
    increment_ms: u16,
    blocks: &[CommandBlock],
) -> Result<TurnReport<Command>, ReplayError> {
    let mut cmds = Vec::new();
    for b in blocks {
        cmds.extend(commands_of_block(b)?);
    }
    Ok(scheduler.push_turn(u64::from(increment_ms), cmds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use w3g::{AbilityHeader, Action, NO_OBJECT};

    fn header(flags: u16, ability: u32) -> AbilityHeader {
        AbilityHeader {
            flags,
            ability,
            unknown: [0, 0],
        }
    }

    #[test]
    fn point_order_becomes_a_point_target() {
        let a = Action::AbilityPoint {
            header: header(0, 0xD0012),
            point: Point { x: 10.0, y: -4.0 },
        };
        let c = convert(2, &a, 0).unwrap();
        assert_eq!(
            c,
            Command::Order {
                player: 2,
                flags: 0,
                id: 0xD0012,
                target: OrderTarget::Point(WorldPoint {
                    x: F32::from_bits(10.0f32.to_bits()),
                    y: F32::from_bits((-4.0f32).to_bits()),
                }),
            }
        );
    }

    #[test]
    fn no_target_pair_falls_back_to_the_point_or_none() {
        let a = Action::AbilityPointObject {
            header: header(0, 0xD0003),
            point: Point { x: 1.0, y: 2.0 },
            target: NO_OBJECT,
        };
        match convert(1, &a, 0).unwrap() {
            Command::Order {
                target: OrderTarget::Point(_),
                ..
            } => {}
            other => panic!("unexpected {other:?}"),
        }
        let b = Action::Ability(header(0, 0xD0019));
        match convert(1, &b, 0).unwrap() {
            Command::Order {
                target: OrderTarget::None,
                ..
            } => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn selection_mode_other_than_one_or_two_is_an_error() {
        let a = Action::SelectionChange {
            mode: 3,
            units: vec![],
        };
        assert_eq!(
            convert(1, &a, 0x40),
            Err(ReplayError::SelectionMode {
                offset: 0x40,
                mode: 3
            })
        );
    }

    #[test]
    fn group_numbers_are_0_to_9() {
        let a = Action::SelectGroup {
            group: 10,
            unknown: 0,
        };
        assert!(matches!(convert(1, &a, 5), Err(ReplayError::Group { group: 10, .. })));
    }

    #[test]
    fn unmodelled_actions_are_recorded_with_their_id() {
        let a = Action::AbilityPointObjectPoint(w3g::AbilityPointObject {
            header: header(0, 0xD0003),
            point: Point { x: 0.0, y: 0.0 },
            target: NO_OBJECT,
            unknown_38: 0,
            unknown_3c: 0,
            point2: Point { x: 0.0, y: 0.0 },
        });
        assert_eq!(
            convert(3, &a, 0).unwrap(),
            Command::Recorded { player: 3, id: 0x14 }
        );
        assert_eq!(
            convert(3, &Action::PauseGame, 0).unwrap(),
            Command::Recorded { player: 3, id: 0x01 }
        );
    }
}
