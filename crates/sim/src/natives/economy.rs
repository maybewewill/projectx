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
//! The tests are in `tests/world_natives.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{Game, ObjKey};
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
    /// Registers a resource node: `unit` gives `amount` (the host's entry point, see the module notes).
    pub fn add_resource_node(&mut self, unit: ObjKey, amount: i32) {
        self.resources.insert(unit, amount);
    }

    /// The amount of the resource node `unit`, or `None` when it is not one.
    pub fn resource_amount(&self, unit: ObjKey) -> Option<i32> {
        self.resources.get(&unit).copied()
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
            let v = gm.resources.entry(k).or_insert(0);
            *v = v.wrapping_add(int_at(a, 1));
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
            gm.resources.insert(k, int_at(a, 1));
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
