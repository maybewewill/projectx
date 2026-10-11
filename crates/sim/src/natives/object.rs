//! Object-record natives: `UnitId`, `UnitId2String`, `GetFoodMade` and `GetFoodUsed`.
//!
//! Addresses are the 1.26a `Game.dll` functions in `docs/spec/natives-1.26a.tsv`. Labels as in `unit.rs`.
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | UnitId | 0x6F3BB650 | LIKELY | the raw id of the unit type with that name (`UnitTypes::id_by_name`), 0 when unknown |
//! | UnitId2String | 0x6F3BB680 | LIKELY | the registered name of a unit type, "" when unknown |
//! | GetFoodMade | 0x6F3B2E00 | CONFIRMED | the type record's food made (`+0x60`), 0 when the type is unknown |
//! | GetFoodUsed | 0x6F3B2E10 | CONFIRMED | the type record's food used (`+0x5C`), 0 when the type is unknown |
//!
//! The names come from the unit registry, which the engine fills from the unit rows of the data files. The original
//! looks the name up in its own table, which may differ in case (UNSURE, see `UnitTypes::id_by_name`).
//! `AbilityId`, `AbilityId2String`, `GetObjectName`, `GetLocalizedString` and the other name lookups that need the
//! editor's string tables are listed as `NotImplemented` in `pending.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::Game;
use super::player::raw_at;
use super::unit_attr::{NativeEntry, Support};

/// The support table of this module.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "UnitId",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitId2String",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetFoodMade",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetFoodUsed",
        support: Support::Confirmed,
    },
];

/// Registers the object-record natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("UnitId", move |ctx, a| {
        let name = match a.first() {
            Some(Value::String(id)) => ctx.string(*id).to_string(),
            _ => String::new(),
        };
        let id = g.borrow().unit_types().id_by_name(&name).unwrap_or(0);
        NativeResult::Return(Value::Integer(id as i32))
    });

    let g = Rc::clone(game);
    reg.register("UnitId2String", move |ctx, a| {
        let id = raw_at(a, 0) as u32;
        let text = g
            .borrow()
            .unit_types()
            .get(id)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        NativeResult::Return(Value::String(ctx.intern(&text)))
    });

    let g = Rc::clone(game);
    reg.register("GetFoodMade", move |_, a| {
        let food = g
            .borrow()
            .unit_types()
            .get(raw_at(a, 0) as u32)
            .map_or(0, |t| t.food_made);
        NativeResult::Return(Value::Integer(food))
    });

    let g = Rc::clone(game);
    reg.register("GetFoodUsed", move |_, a| {
        let food = g
            .borrow()
            .unit_types()
            .get(raw_at(a, 0) as u32)
            .map_or(0, |t| t.food_used);
        NativeResult::Return(Value::Integer(food))
    });
}
