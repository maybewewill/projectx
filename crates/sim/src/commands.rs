//! Replay commands applied to the simulation: selection, hotkey groups, orders issued to the selection,
//! and the game speed setting.
//!
//! The input is [`Command`], a resolved form of the typed replay actions (the `w3g` crate decodes the
//! bytes; the engine converts them, so this crate does not depend on the replay parser). The model and
//! its evidence are in `docs/spec/replay-actions.md`. Labels there apply here:
//!
//! * Object references are `(id1, id2)` pairs ([`ObjectRef`]). The original's resolver is not located
//!   (`docs/spec/replay-actions.md` section 10). A pair is resolved first by an explicit registration
//!   ([`CommandState::register_object`]), then by the object-pair model ([`model_resolve`], UNSURE: `(handle id, 0)`).
//!   A pair that neither knows is reported as unresolved and has no effect.
//! * An order command applies to the player's current selection, in selection order (LIKELY, the public
//!   replay model). Each unit gets one call to the order dispatcher with `replace = !queued`
//!   ([`Command::Order`] flags, bit [`FLAG_QUEUED`], UNSURE).
//! * Orders go through [`Game::issue_order`], the same path as the natives (`orders.rs`). With
//!   [`CommandState::apply_fired`] they also fire the issued-order events, per accepted unit, through the same
//!   function as the natives (`natives::order::fire_issued`).
//! * Speed only records the setting ([`CommandState::speed_setting`]); the caller's `TurnScheduler` reads it.
//!   The original's speed table is in `lockstep.rs`.
//! * Actions with no model here yet are recorded as [`Outcome::Recorded`] (pause, save, chat, ping, ...).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::F32;

use crate::natives::game::{Game, ObjKey, ScriptRuntime};
use crate::natives::order::fire_issued;
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

    /// The unit behind a pair, if registered explicitly (no model lookup; see [`CommandState::resolve_in`]).
    pub fn resolve(&self, object: ObjectRef) -> Option<ObjKey> {
        self.objects.get(&object).copied()
    }

    /// The unit behind a pair: an explicit registration first, then the object-pair model
    /// ([`model_resolve`], UNSURE, `docs/spec/replay-actions.md` section 10). `None` when neither knows it.
    pub fn resolve_in(&self, game: &Game, object: ObjectRef) -> Option<ObjKey> {
        self.objects
            .get(&object)
            .copied()
            .or_else(|| model_resolve(game, object))
    }

    /// The player's current selection, in selection order.
    pub fn selection(&self, player: u8) -> &[ObjKey] {
        self.selections.get(&player).map_or(&[], Vec::as_slice)
    }

    /// The units of hotkey group `group` of `player`.
    pub fn group(&self, player: u8, group: u8) -> &[ObjKey] {
        self.groups.get(&(player, group)).map_or(&[], Vec::as_slice)
    }

    /// The last speed setting seen (0 slow, 1 normal, 2 fast), or 1 (normal) before any command.
    pub fn speed_setting(&self) -> u8 {
        self.speed.unwrap_or(1)
    }

    /// The number of commands applied so far.
    pub fn applied(&self) -> u64 {
        self.applied
    }

    /// Applies one command without firing script events. Commands take effect in the order they are given (the
    /// caller applies a turn's commands in stream order before its ticks). Use [`CommandState::apply_fired`] when
    /// script triggers must see the orders.
    pub fn apply(&mut self, game: &RefCell<Game>, cmd: &Command) -> Outcome {
        self.apply_with(game, None, cmd)
    }

    /// Applies one command and fires the script events of the orders it issues (`ISSUED_ORDER` player-unit and
    /// unit events, `natives::order::fire_issued`), per accepted unit right after its dispatch, the same as a
    /// native order. `rt` is the script runtime that runs the trigger threads (the main VM).
    pub fn apply_fired(
        &mut self,
        game: &RefCell<Game>,
        rt: &mut dyn ScriptRuntime,
        cmd: &Command,
    ) -> Outcome {
        self.apply_with(game, Some(rt), cmd)
    }

    fn apply_with(
        &mut self,
        game: &RefCell<Game>,
        rt: Option<&mut dyn ScriptRuntime>,
        cmd: &Command,
    ) -> Outcome {
        // The selections live in `Game::lane` (the selection natives edit them too): take them in, apply, give back.
        self.selections = game.borrow().lane.selections.clone();
        let out = self.apply_inner(game, rt, cmd);
        game.borrow_mut().lane.selections = self.selections.clone();
        out
    }

    fn apply_inner(
        &mut self,
        game: &RefCell<Game>,
        rt: Option<&mut dyn ScriptRuntime>,
        cmd: &Command,
    ) -> Outcome {
        self.applied += 1;
        match cmd {
            Command::Select { player, add, units } => {
                let (keys, unresolved) = self.resolve_all(game, units);
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
                let (keys, unresolved) = self.resolve_all(game, units);
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
                let (keys, unresolved) = self.resolve_all(game, std::slice::from_ref(unit));
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
            } => self.apply_order(game, rt, *player, *flags, *id, *target),
            Command::SetSpeed { setting, .. } => {
                let setting = (*setting).min(2);
                self.speed = Some(setting);
                Outcome::Speed { setting }
            }
            Command::Recorded { .. } => Outcome::Recorded,
        }
    }

    /// Resolves pairs in the given order; returns the units found and the number of pairs with no entry.
    fn resolve_all(&self, game: &RefCell<Game>, objects: &[ObjectRef]) -> (Vec<ObjKey>, usize) {
        let g = game.borrow();
        let mut keys = Vec::with_capacity(objects.len());
        let mut unresolved = 0;
        for o in objects {
            match self.resolve_in(&g, *o) {
                Some(k) => keys.push(k),
                None => unresolved += 1,
            }
        }
        (keys, unresolved)
    }

    fn apply_order(
        &mut self,
        game: &RefCell<Game>,
        mut rt: Option<&mut dyn ScriptRuntime>,
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
            OrderTarget::Object { object, point } => {
                let found = self.resolve_in(&game.borrow(), object).or_else(|| {
                    // The agent-id model is not known for neutral objects (mines, creeps): the order names its target
                    // by the point the replay carries, so the live unit nearest to that point stands in for the
                    // object (EchoIsles: the gold mine `0x1929` of the harvest order).
                    let p = point?;
                    nearest_unit(&game.borrow(), p.x.to_f32(), p.y.to_f32(), NEAREST_UNIT_RADIUS)
                });
                match found {
                    Some(k) => {
                        let h = game.borrow().handle_of(k);
                        (OrderClass::Target, point.map(|p| (p.x, p.y)), h)
                    }
                    None => {
                        if id == crate::production::ORDER_HARVEST {
                            // The target is no unit: a harvest order at a tree (destructable). The replay's point
                            // finds the tree (EchoIsles: `0x1929` is a tree of the forest next to the base).
                            if let Some(p) = point {
                                let mut gm = game.borrow_mut();
                                let tree = gm.tree_near(p.x.to_f32(), p.y.to_f32(), NEAREST_UNIT_RADIUS);
                                let mut accepted = 0;
                                let mut refused = 0;
                                for unit in self.selection(player).to_vec() {
                                    if tree.is_some() && gm.order_harvest(unit, None, tree) {
                                        accepted += 1;
                                    } else {
                                        refused += 1;
                                    }
                                }
                                return Outcome::Ordered { accepted, refused, unresolved: 0 };
                            }
                        }
                        // Nothing is issued for an object the table does not know.
                        unresolved += 1;
                        return Outcome::Ordered {
                            accepted: 0,
                            refused: 0,
                            unresolved,
                        };
                    }
                }
            }
        };
        let units = self.selection(player).to_vec();
        let mut accepted = 0;
        let mut refused = 0;
        if matches!(target, OrderTarget::None) && is_unit_rawcode(id) {
            // A train order: the order id is the rawcode of the unit type. The queue is the production model's
            // (the AI's path); the building that has the shortest queue would take it in the game's UI, here the
            // selected buildings take it in selection order. UNSURE: the original routes it through the
            // building's train ability (`CAbilityTrain`), which the order system does not hold.
            for unit in units {
                let mut gm = game.borrow_mut();
                if gm.building_usable(unit) && gm.can_train(unit, id) && gm.train_unit(unit, id).is_ok() {
                    accepted += 1;
                } else {
                    refused += 1;
                }
            }
            return Outcome::Ordered { accepted, refused, unresolved };
        }
        for unit in units {
            let order = Order {
                id,
                class,
                point,
                target: handle,
            };
            let (validated, index) = game.borrow_mut().issue_order(unit, order, replace);
            if validated && index.is_some() {
                accepted += 1;
            } else {
                refused += 1;
            }
            // The same events as a native order, right after this unit's dispatch (natives/order.rs `issue`).
            if let (Some(index), Some(rt)) = (index, rt.as_deref_mut()) {
                fire_issued(game, rt, unit, class, index);
            }
        }
        Outcome::Ordered {
            accepted,
            refused,
            unresolved,
        }
    }
}

/// Whether an order id is a four-letter rawcode (train and build orders carry the unit type as the id).
fn is_unit_rawcode(id: u32) -> bool {
    id >= 0x2020_2020 && id.to_be_bytes().iter().all(|b| b.is_ascii_alphanumeric())
}

/// How far from the replay's point a unit may be to stand in for an unresolved target object (game units).
pub const NEAREST_UNIT_RADIUS: f32 = 256.0;

/// The live unit nearest to `(x, y)` within `radius` (ties: the lower object key). Presentation-grade stand-in for the
/// replay's agent-id lookup, which is not decoded (`docs/spec/lockstep.md` section 19).
fn nearest_unit(game: &Game, x: f32, y: f32, radius: f32) -> Option<ObjKey> {
    let mut best: Option<(f32, ObjKey)> = None;
    for (k, u) in game.unit_objects() {
        if u.dead || u.removed {
            continue;
        }
        let (dx, dy) = (u.x.to_f32() - x, u.y.to_f32() - y);
        let d = (dx * dx + dy * dy).sqrt();
        if d <= radius && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

/// The object-pair model for units with no explicit registration (UNSURE, `docs/spec/replay-actions.md` section 10).
///
/// The pair of a unit is taken to be `(handle id, 0)`: the first word is the unit's JASS handle id (the one identity
/// the simulation allocates per object, `natives/game.rs`), the second word is zero. A pair with a non-zero second
/// word, or with a first word that is no live unit's handle, resolves to nothing. This is a model, not the original's
/// rule: the original's pair source is the agent's `+0x0C/+0x10` (CONFIRMED layout, LIKELY source), and the inverse
/// is not located. Replays with pairs from another scheme stay unresolved and have no effect.
fn model_resolve(game: &Game, object: ObjectRef) -> Option<ObjKey> {
    if object.id2 != 0 {
        return None;
    }
    game.unit_of_handle(object.id1)
}

/// The object pair the model assigns to a unit (`(handle id, 0)`, see [`model_resolve`]).
pub fn model_pair(game: &Game, unit: ObjKey) -> ObjectRef {
    ObjectRef {
        id1: game.handle_of(unit),
        id2: 0,
    }
}
