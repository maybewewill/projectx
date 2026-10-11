//! Shop stock natives (`docs/spec/shop-stock.md`): `Add/Remove{Item,Unit}{To,From}[All]Stock` and
//! `Set{All}{Item,Unit}TypeSlots`.
//!
//! A per-shop native finds the unit's `Asid` (items) or `Asud` (units) ability and does nothing without it, and
//! without validating the type id. The "All" natives first check the id (item type exists / unit type exists; the
//! slot natives do not check anything) and then visit every unit that has the ability, in creation order (LIKELY: the
//! original walks the ability category list of the world, whose order was not located).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, ObjKey};
use super::player::raw_at;
use super::unit_attr::{NativeEntry, Support};
use crate::shop::{StockList, TypeStock, SELL_ITEMS, SELL_UNITS};

/// The support table of the natives registered here (`SetUnitTypeSlots` is listed in `unit_attr.rs`).
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry { name: "AddItemToStock", support: Support::Likely },
    NativeEntry { name: "AddItemToAllStock", support: Support::Likely },
    NativeEntry { name: "RemoveItemFromStock", support: Support::Likely },
    NativeEntry { name: "RemoveItemFromAllStock", support: Support::Likely },
    NativeEntry { name: "SetItemTypeSlots", support: Support::Likely },
    NativeEntry { name: "SetAllItemTypeSlots", support: Support::Likely },
    NativeEntry { name: "AddUnitToStock", support: Support::Likely },
    NativeEntry { name: "AddUnitToAllStock", support: Support::Likely },
    NativeEntry { name: "RemoveUnitFromStock", support: Support::Likely },
    NativeEntry { name: "RemoveUnitFromAllStock", support: Support::Likely },
    NativeEntry { name: "SetAllUnitTypeSlots", support: Support::Likely },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Items,
    Units,
}

impl Kind {
    fn code(self) -> u32 {
        match self {
            Kind::Items => SELL_ITEMS,
            Kind::Units => SELL_UNITS,
        }
    }
}

fn type_stock(gm: &Game, kind: Kind, id: u32) -> TypeStock {
    match kind {
        Kind::Items => gm.shops.item_stock.get(&id).copied().unwrap_or(TypeStock { stock_max: 0, regen: F32::ONE }),
        Kind::Units => gm.shops.unit_stock.get(&id).copied().unwrap_or(TypeStock { stock_max: 0, regen: F32::ZERO }),
    }
}

fn valid_type(gm: &Game, kind: Kind, id: u32) -> bool {
    id != 0
        && match kind {
            Kind::Items => gm.item_types().get(id).is_some(),
            Kind::Units => gm.unit_types().get(id).is_some(),
        }
}

fn list_mut(gm: &mut Game, kind: Kind, k: ObjKey) -> Option<&mut StockList> {
    if !gm.unit(k).abilities.contains(kind.code()) {
        return None;
    }
    let map = match kind {
        Kind::Items => &mut gm.shops.items,
        Kind::Units => &mut gm.shops.units,
    };
    Some(map.entry(k).or_default())
}

/// The shops in visiting order: live units with the ability, creation order.
fn shops(gm: &Game, kind: Kind) -> Vec<ObjKey> {
    gm.unit_objects().filter(|(_, u)| !u.removed && u.abilities.contains(kind.code())).map(|(k, _)| k).collect()
}

fn shop_unit(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    gm.unit_of_handle(a.first().and_then(|v| v.as_handle()).unwrap_or(0))
}

fn null() -> NativeResult {
    NativeResult::Return(Value::Null)
}

fn register_kind(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>, kind: Kind, n: [&'static str; 6]) {
    let [add, add_all, rem, rem_all, slots, slots_all] = n;

    let g = Rc::clone(game);
    reg.register(add, move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = shop_unit(&gm, a) {
            let id = raw_at(a, 1) as u32;
            let ty = type_stock(&gm, kind, id);
            if let Some(l) = list_mut(&mut gm, kind, k) {
                l.add(id, raw_at(a, 2), raw_at(a, 3), ty);
            }
        }
        null()
    });

    let g = Rc::clone(game);
    reg.register(add_all, move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 0) as u32;
        if valid_type(&gm, kind, id) {
            let ty = type_stock(&gm, kind, id);
            for k in shops(&gm, kind) {
                if let Some(l) = list_mut(&mut gm, kind, k) {
                    l.add(id, raw_at(a, 1), raw_at(a, 2), ty);
                }
            }
        }
        null()
    });

    let g = Rc::clone(game);
    reg.register(rem, move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = shop_unit(&gm, a) {
            if let Some(l) = list_mut(&mut gm, kind, k) {
                l.remove(raw_at(a, 1) as u32);
            }
        }
        null()
    });

    let g = Rc::clone(game);
    reg.register(rem_all, move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 0) as u32;
        if valid_type(&gm, kind, id) {
            for k in shops(&gm, kind) {
                if let Some(l) = list_mut(&mut gm, kind, k) {
                    l.remove(id);
                }
            }
        }
        null()
    });

    let g = Rc::clone(game);
    reg.register(slots, move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = shop_unit(&gm, a) {
            if let Some(l) = list_mut(&mut gm, kind, k) {
                l.set_slots(raw_at(a, 1));
            }
        }
        null()
    });

    let g = Rc::clone(game);
    reg.register(slots_all, move |_, a| {
        let mut gm = g.borrow_mut();
        for k in shops(&gm, kind) {
            if let Some(l) = list_mut(&mut gm, kind, k) {
                l.set_slots(raw_at(a, 0));
            }
        }
        null()
    });
}

/// Registers the shop stock natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    register_kind(
        reg,
        game,
        Kind::Items,
        [
            "AddItemToStock",
            "AddItemToAllStock",
            "RemoveItemFromStock",
            "RemoveItemFromAllStock",
            "SetItemTypeSlots",
            "SetAllItemTypeSlots",
        ],
    );
    register_kind(
        reg,
        game,
        Kind::Units,
        [
            "AddUnitToStock",
            "AddUnitToAllStock",
            "RemoveUnitFromStock",
            "RemoveUnitFromAllStock",
            "SetUnitTypeSlots",
            "SetAllUnitTypeSlots",
        ],
    );
}
