//! Replay commands applied to the simulation: selection, hotkey groups, orders issued to the selection,
//! and the game speed setting.
//!
//! The input is [`Command`], a resolved form of the typed replay actions (the `w3g` crate decodes the
//! bytes; the engine converts them, so this crate does not depend on the replay parser). The model and
//! its evidence are in `docs/spec/replay-actions.md`. Labels there apply here:
//!
//! * Object references are `(id1, id2)` pairs ([`ObjectRef`]). The original's resolver is not located
//!   yet (UNSURE), so the table is filled explicitly with [`CommandState::register_object`]. A pair with no
//!   entry is reported as unresolved and has no effect. Nothing guesses a unit from a pair.
//! * An order command applies to the player's current selection, in selection order (LIKELY, the public
//!   replay model). Each unit gets one call to the order dispatcher with `replace = !queued`
//!   ([`Command::Order`] flags, bit [`FLAG_QUEUED`], UNSURE).
//! * Orders go through [`Game::issue_order`], the same path as the natives (`orders.rs`). The issued-order
//!   events are not fired for replay orders yet (the event path lives in `natives/order.rs`).
//! * Speed only records the setting ([`CommandState::speed_setting`]); the caller's `TurnScheduler` reads it.
//!   The original's speed table is in `lockstep.rs`.
//! * Actions with no model here yet are recorded as [`Outcome::Recorded`] (pause, save, chat, ping, ...).

use std::collections::BTreeMap;

use softfloat::F32;

use crate::natives::game::{Game, ObjKey};
use crate::orders::{point_capable, Order, OrderClass};

/// Order flag bit that makes the order queued behind the current one (`replace = false`). UNSURE: the
/// bit is bit 0 of the flags word the sender builds from the input state (`0x6F37AC..`, `0x6F339DD0`).
pub const FLAG_QUEUED: u16 = 0x0001;

/// A replay object reference: the two words of the pair (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectRef {
    pub id1: u32,
    pub id2: u32,
}

/// A world point in single precision (softfloat bits, as the simulation keeps them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPoint {
    pub x: F32,
    pub y: F32,
}

/// What an order command points at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OrderTarget {
    /// No point and no object (immediate order).
    None,
    /// A point.
    Point(WorldPoint),
    /// An object, with the point the replay gives with it (when there is one).
    Object {
        object: ObjectRef,
        point: Option<WorldPoint>,
    },
}

/// A resolved replay command of one player.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Replay 0x16: `add` = mode 1, otherwise remove (mode 2).
    Select {
        player: u8,
        add: bool,
        units: Vec<ObjectRef>,
    },
    /// Replay 0x17: assign the player's hotkey group `group` to these units.
    AssignGroup {
        player: u8,
        group: u8,
        units: Vec<ObjectRef>,
    },
    /// Replay 0x18: the selection becomes group `group` of the player.
    SelectGroup { player: u8, group: u8 },
    /// Replay 0x19: the selection becomes the one unit (the subgroup choice), item code as given.
    SelectSubgroup {
        player: u8,
        item: u32,
        unit: ObjectRef,
    },
    /// Replay 0x1A: nothing changes in the model.
    PreSubselection { player: u8 },
    /// Replay 0x10..0x15 (an order or ability with its target). `id` is the ability rawcode or order id.
    Order {
        player: u8,
        flags: u16,
        id: u32,
        target: OrderTarget,
    },
    /// Replay 0x03: the game speed setting (0 slow, 1 normal, 2 fast; larger values are fast).
    SetSpeed { player: u8, setting: u8 },
    /// Any other action. The id is kept for the report; the model does nothing with it yet.
    Recorded { player: u8, id: u8 },
}

/// The result of applying one command. Counts are per command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The selection changed; `now` is the player's selection size afterwards.
    Selected { now: usize, unresolved: usize },
    /// The hotkey group was assigned or read.
    Grouped { units: usize, unresolved: usize },
    /// An order was given to `accepted` of the selected units (`refused` were dropped by the dispatcher or
    /// the validator, for example a locked unit or an unknown order id for the unit).
    Ordered {
        accepted: usize,
        refused: usize,
        unresolved: usize,
    },
    /// The speed setting was recorded (clamped as the original does: values above 2 are fast).
    Speed { setting: u8 },
    /// Nothing to do in the model (see the module docs).
    Recorded,
}

/// The replay command state of one game: selections, hotkey groups, the object table and the speed setting.
#[derive(Clone, Debug, Default)]
pub struct CommandState {
    selections: BTreeMap<u8, Vec<ObjKey>>,
    groups: BTreeMap<(u8, u8), Vec<ObjKey>>,
    objects: BTreeMap<ObjectRef, ObjKey>,
    speed: Option<u8>,
    applied: u64,
}

impl CommandState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the unit behind an object pair. The replay's pair for a unit must be registered before
    /// the commands that name it.
    pub fn register_object(&mut self, object: ObjectRef, unit: ObjKey) {
        self.objects.insert(object, unit);
    }

    /// The unit behind a pair, if registered.
    pub fn resolve(&self, object: ObjectRef) -> Option<ObjKey> {
        self.objects.get(&object).copied()
    }

    /// The player's current selection, in selection order.
    pub fn selection(&self, player: u8) -> &[ObjKey] {
        self.selections.get(&player).map_or(&[], Vec::as_slice)
    }

    /// The units of hotkey group `group` of `player`.
    pub fn group(&self, player: u8, group: u8) -> &[ObjKey] {
        self.groups
            .get(&(player, group))
            .map_or(&[], Vec::as_slice)
    }

    /// The last speed setting seen (0 slow, 1 normal, 2 fast), or 1 (normal) before any command.
    pub fn speed_setting(&self) -> u8 {
        self.speed.unwrap_or(1)
    }

    /// The number of commands applied so far.
    pub fn applied(&self) -> u64 {
        self.applied
    }

    /// Applies one command. Commands take effect in the order they are given (the caller applies a turn's
    /// commands in stream order before its ticks).
    pub fn apply(&mut self, game: &mut Game, cmd: &Command) -> Outcome {
        self.applied += 1;
        match cmd {
            Command::Select { player, add, units } => {
                let (keys, unresolved) = self.resolve_all(units);
                let sel = self.selections.entry(*player).or_default();
                if *add {
                    for k in keys {
                        if !sel.contains(&k) {
                            sel.push(k);
                        }
                    }
                } else {
                    sel.retain(|k| !keys.contains(k));
                }
                Outcome::Selected {
                    now: sel.len(),
                    unresolved,
                }
            }
            Command::AssignGroup {
                player,
                group,
                units,
            } => {
                let (keys, unresolved) = self.resolve_all(units);
                let n = keys.len();
                self.groups.insert((*player, *group), keys);
                Outcome::Grouped {
                    units: n,
                    unresolved,
                }
            }
            Command::SelectGroup { player, group } => {
                let keys = self.group(*player, *group).to_vec();
                let n = keys.len();
                self.selections.insert(*player, keys);
                Outcome::Grouped {
                    units: n,
                    unresolved: 0,
                }
            }
            Command::SelectSubgroup { player, unit, .. } => {
                let (keys, unresolved) = self.resolve_all(std::slice::from_ref(unit));
                self.selections.insert(*player, keys);
                Outcome::Selected {
                    now: self.selection(*player).len(),
                    unresolved,
                }
            }
            Command::PreSubselection { .. } => Outcome::Recorded,
            Command::Order {
                player,
                flags,
                id,
                target,
            } => self.apply_order(game, *player, *flags, *id, *target),
            Command::SetSpeed { setting, .. } => {
                let setting = (*setting).min(2);
                self.speed = Some(setting);
                Outcome::Speed { setting }
            }
            Command::Recorded { .. } => Outcome::Recorded,
        }
    }

    /// Resolves pairs in the given order; returns the units found and the number of pairs with no entry.
    fn resolve_all(&self, objects: &[ObjectRef]) -> (Vec<ObjKey>, usize) {
        let mut keys = Vec::with_capacity(objects.len());
        let mut unresolved = 0;
        for o in objects {
            match self.resolve(*o) {
                Some(k) => keys.push(k),
                None => unresolved += 1,
            }
        }
        (keys, unresolved)
    }

    fn apply_order(
        &mut self,
        game: &mut Game,
        player: u8,
        flags: u16,
        id: u32,
        target: OrderTarget,
    ) -> Outcome {
        let replace = flags & FLAG_QUEUED == 0;
        let mut unresolved = 0;
        let (class, point, handle) = match target {
            OrderTarget::None => (OrderClass::Immediate, None, 0),
            OrderTarget::Point(p) => {
                let class = if point_capable(id) {
                    OrderClass::Point
                } else {
                    OrderClass::Target
                };
                (class, Some((p.x, p.y)), 0)
            }
            OrderTarget::Object { object, point } => match self.resolve(object) {
                Some(k) => {
                    let h = game.handle_of(k);
                    (OrderClass::Target, point.map(|p| (p.x, p.y)), h)
                }
                None => {
                    // Nothing is issued for an object the table does not know.
                    unresolved += 1;
                    return Outcome::Ordered {
                        accepted: 0,
                        refused: 0,
                        unresolved,
                    };
                }
            },
        };
        let units = self.selection(player).to_vec();
        let mut accepted = 0;
        let mut refused = 0;
        for unit in units {
            let order = Order {
                id,
                class,
                point,
                target: handle,
            };
            let (validated, index) = game.issue_order(unit, order, replace);
            if validated && index.is_some() {
                accepted += 1;
            } else {
                refused += 1;
            }
        }
        Outcome::Ordered {
            accepted,
            refused,
            unresolved,
        }
    }
}

