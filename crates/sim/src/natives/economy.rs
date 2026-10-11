//! Waygates and resource nodes: `WaygateActivate`, `WaygateIsActive`, `WaygateGetDestinationX`, `WaygateGetDestinationY`,
//! `WaygateSetDestination`, `AddResourceAmount`, `GetResourceAmount` and `SetResourceAmount`.
//!
//! Addresses are the 1.26a `Game.dll` functions in `docs/spec/natives-1.26a.tsv`. Labels as in `unit.rs`.
//!
//! Waygates. The natives look for the waygate ability `Awrp` (raw `0x41777270`, `FUN_6f0260d0`) on the unit. Without it
//! every waygate native does nothing and returns its default. The activation (`FUN_6f026610`) and the destination
//! (`FUN_6f0265e0`, `FUN_6f4690d0`) are stored by the original in a registry indexed by a small number (1 to 255) that
//! the waygate ability carries (`FUN_6f469060`, `FUN_6f4690d0`); the index assignment and the registry count are not
//! read. The model keeps the state per unit, which is the same for one waygate per unit (UNSURE for shared indices).
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | WaygateActivate | 0x6F3C92D0 | LIKELY | sets the active flag; the ability flag and the registry count are not modelled |
//! | WaygateIsActive | 0x6F3C9310 | LIKELY | the active flag, false without the ability |
//! | WaygateGetDestinationX / Y | 0x6F3C91F0 / 0x6F3C9240 | LIKELY | the stored point, 0.0 when none was set |
//! | WaygateSetDestination | 0x6F3C9290 | UNSURE | the original clamps the point through a bit-level routine (`FUN_6f4690d0`) that is not decoded; the point is stored as given |
//! | AddResourceAmount | 0x6F3C7850 | CONFIRMED | adds to the amount of a resource node (the unit's gold mine ability, `+0x6C`) |
//! | GetResourceAmount | 0x6F3C78B0 | CONFIRMED | the amount, 0 for a unit that is not a resource node |
//! | SetResourceAmount | 0x6F3C77F0 | CONFIRMED | sets the amount of a resource node |
//!
//! Resource nodes. The original tests a flag of the unit (`+0x5C` bit 17, `FUN_6f277300`) and then reads the amount from
//! the unit's gold mine ability (`Agmb`, `FUN_6f082470`). The model has no unit flag for it: a unit is a resource node
//! exactly when the host registered it with [`Game::add_resource_node`] (the map loader does this for mines, UNSURE for
//! the initial amounts, which come from the map data).
//!
//! Gold mines. A resource node owns a [`Mine`] (the remaining gold, `Agld` `+0x6C`, see `economy.rs`). The natives
//! above read and write it, and the harvest path ([`Game::gold_trip`]) takes gold from it.
//!
//! Gold and lumber of the players. [`Game::economy`] holds the stored x10 amounts, which the changer
//! (`economy::change`) and `SetPlayerState` / `GetPlayerState` (states 1 and 2, see `natives/player.rs`) share. The
//! credits of the harvest, the cancel refunds and the upkeep rates go through the methods of the `impl Game` block
//! below. The original's `SetPlayerState` writes the stored value directly (no tax, no ally share; spec economy.md
//! 1.2, CONFIRMED structure), so the native does not go through the changer.
//!
//! The tests are in `tests/world_natives.rs` (mines) and `tests/economy_integration.rs` (the resource paths).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{Game, ObjKey, PLAYER_SLOTS};
use crate::economy::{
    self, EconomyConstants, Flags, Mine, MineNotice, PlayerEconomy, RefundKind, Res, Worker,
};
use super::hashtable::{int_at, real_at};
use super::unit_attr::{NativeEntry, Support};
use softfloat::F32;

/// The waygate ability code `Awrp` (CONFIRMED: `FUN_6f0260d0`).
pub const WAYGATE_ABILITY: u32 = 0x4177_7270;

/// The state of one waygate.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WaygateState {
    /// Set by `WaygateActivate`.
    pub active: bool,
    /// The destination (`WaygateSetDestination`), 0.0 until set.
    pub dest_x: F32,
    pub dest_y: F32,
}

/// The support table of this module.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "WaygateActivate",
        support: Support::Likely,
    },
    NativeEntry {
        name: "WaygateIsActive",
        support: Support::Likely,
    },
    NativeEntry {
        name: "WaygateGetDestinationX",
        support: Support::Likely,
    },
    NativeEntry {
        name: "WaygateGetDestinationY",
        support: Support::Likely,
    },
    NativeEntry {
        name: "WaygateSetDestination",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "AddResourceAmount",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetResourceAmount",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetResourceAmount",
        support: Support::Confirmed,
    },
];

impl Game {
    /// Registers a resource node: `unit` is a gold mine with `amount` gold (the host's entry point, see the module
    /// notes). An existing mine is replaced.
    pub fn add_resource_node(&mut self, unit: ObjKey, amount: i32) {
        self.mines.insert(unit, Mine { remaining: amount });
    }

    /// The amount of the resource node `unit`, or `None` when it is not one.
    pub fn resource_amount(&self, unit: ObjKey) -> Option<i32> {
        self.mines.get(&unit).map(|m| m.remaining)
    }

    /// The economy constants in use (see [`EconomyConstants`]).
    pub fn economy_constants(&self) -> &EconomyConstants {
        &self.economy_constants
    }

    /// Sets the economy constants (the engine does this at boot, from Misc or the defaults) and refreshes every
    /// slot's upkeep rates for food used 0. UNSURE: the original refreshes the rates when the food used changes; a
    /// host that tracks food calls [`Game::refresh_upkeep`] itself.
    pub fn set_economy_constants(&mut self, constants: EconomyConstants) {
        self.economy_constants = constants;
        for p in 0..PLAYER_SLOTS as u32 {
            self.refresh_upkeep(p, 0);
        }
    }

    /// The gold and lumber state of slot `p`, or `None` for an index outside 0 to 15.
    pub fn player_economy(&self, p: u32) -> Option<&PlayerEconomy> {
        self.economy.get(p as usize)
    }

    /// Mutable gold and lumber state of slot `p`, for the host (the ally share table is set from the map here).
    pub fn player_economy_mut(&mut self, p: u32) -> Option<&mut PlayerEconomy> {
        self.economy.get_mut(p as usize)
    }

    /// Recomputes the upkeep tax rates of slot `p` for `food_used` (`0x6F40F620`). Returns `false` for a bad slot.
    pub fn refresh_upkeep(&mut self, p: u32, food_used: i32) -> bool {
        let Some(player) = self.economy.get_mut(p as usize) else {
            return false;
        };
        self.economy_constants.upkeep.refresh_rates(player, food_used);
        true
    }

    /// Credits `amount` script units of `res` to slot `p` through the changer (`0x6F416A00` / `0x6F416A40`).
    /// Returns the credited amount in script units, or `None` for a bad slot.
    pub fn credit_resource(&mut self, p: u32, res: Res, amount: i32, flags: Flags) -> Option<i32> {
        if p as usize >= self.economy.len() {
            return None;
        }
        Some(economy::credit(&mut self.economy, p as usize, res, amount, flags))
    }

    /// Deposits the carry of `worker` to its owner (`0x6F047C80`, tax and share on). Returns the credited amount, 0
    /// for a bad owner or an empty carry.
    pub fn deposit_worker(&mut self, worker: &mut Worker) -> i32 {
        if worker.owner >= self.economy.len() {
            return 0;
        }
        worker.deposit(&mut self.economy)
    }

    /// A gold trip of `worker` at the gold mine `mine` (`0x6F060890`). `None` when `mine` is not a resource node or the
    /// owner is not a slot. `notify` is the host's visibility check for the notice. Returns the amount taken and the
    /// notice to raise (the notice itself is not sent: event ids 0xDE and 0xDF are not mapped to a JASS event yet, see
    /// the module notes of `natives/event.rs`).
    pub fn gold_trip(
        &mut self,
        worker: &mut Worker,
        mine: ObjKey,
        notify: bool,
    ) -> Option<(i32, MineNotice)> {
        if worker.owner >= self.economy.len() {
            return None;
        }
        let low_gold = self.economy_constants.low_gold_amount;
        let m = self.mines.get_mut(&mine)?;
        Some(worker.gold_trip(&mut self.economy, m, low_gold, notify))
    }

    /// The refund of a cancelled slot of `kind` to `owner`, gold then lumber, through the plain changer
    /// (`0x6F062320`). `refund_applies` is false when the slot's bit in the owner's mask is set (the original then
    /// counts instead; not modelled). Returns the credited (gold, lumber) in script units, `(0, 0)` for a bad owner.
    pub fn refund_cancelled(
        &mut self,
        owner: u32,
        kind: RefundKind,
        gold_cost: i32,
        lumber_cost: i32,
        refund_applies: bool,
    ) -> (i32, i32) {
        if owner as usize >= self.economy.len() {
            return (0, 0);
        }
        let rate = self.economy_constants.refund_rate(kind);
        economy::cancel_train_refund(
            &mut self.economy,
            owner as usize,
            gold_cost,
            lumber_cost,
            rate,
            refund_applies,
        )
    }
}

/// Registers the waygate and resource natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("WaygateActivate", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = waygate_unit(&gm, a) {
            gm.waygates.entry(k).or_default().active = a.get(1).is_some_and(bool_of);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("WaygateIsActive", move |_, a| {
        let gm = g.borrow();
        let on =
            waygate_unit(&gm, a).is_some_and(|k| gm.waygates.get(&k).is_some_and(|w| w.active));
        NativeResult::Return(Value::Boolean(on))
    });

    let g = Rc::clone(game);
    reg.register("WaygateGetDestinationX", move |_, a| {
        let gm = g.borrow();
        let x = waygate_unit(&gm, a)
            .and_then(|k| gm.waygates.get(&k))
            .map_or(F32::ZERO, |w| w.dest_x);
        NativeResult::Return(Value::Real(x))
    });

    let g = Rc::clone(game);
    reg.register("WaygateGetDestinationY", move |_, a| {
        let gm = g.borrow();
        let y = waygate_unit(&gm, a)
            .and_then(|k| gm.waygates.get(&k))
            .map_or(F32::ZERO, |w| w.dest_y);
        NativeResult::Return(Value::Real(y))
    });

    let g = Rc::clone(game);
    reg.register("WaygateSetDestination", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = waygate_unit(&gm, a) {
            let w = gm.waygates.entry(k).or_default();
            w.dest_x = real_at(a, 1);
            w.dest_y = real_at(a, 2);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("AddResourceAmount", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = resource_unit(&gm, a) {
            if let Some(m) = gm.mines.get_mut(&k) {
                m.remaining = m.remaining.wrapping_add(int_at(a, 1));
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetResourceAmount", move |_, a| {
        let gm = g.borrow();
        let v = resource_unit(&gm, a)
            .and_then(|k| gm.resource_amount(k))
            .unwrap_or(0);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetResourceAmount", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = resource_unit(&gm, a) {
            gm.mines.insert(k, Mine { remaining: int_at(a, 1) });
        }
        NativeResult::Return(Value::Null)
    });
}

/// The unit in handle argument 0 when it has the waygate ability (the natives do nothing otherwise).
fn waygate_unit(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    let k = gm.unit_of_handle(handle_at(a, 0))?;
    gm.unit(k).abilities.contains(WAYGATE_ABILITY).then_some(k)
}

/// The unit in handle argument 0 when it is a resource node.
fn resource_unit(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    let k = gm.unit_of_handle(handle_at(a, 0))?;
    gm.resource_amount(k).map(|_| k)
}

/// The handle id in argument `i`, or 0 (the null handle).
fn handle_at(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

/// A boolean value (a JASS boolean or integer, nonzero is true).
fn bool_of(v: &Value) -> bool {
    match v {
        Value::Boolean(b) => *b,
        Value::Integer(i) => *i != 0,
        _ => false,
    }
}
