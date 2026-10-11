//! Item natives: `CreateItem`, `RemoveItem`, position (`SetItemPosition`, `GetItemX/Y`), type, owner, visibility,
//! invulnerability, charges, the item flag natives, the unit pack (`UnitAddItem` family, `UnitRemoveItem*`,
//! `UnitHasItem`, `UnitItemInSlot`, `UnitInventorySize`), the drop natives, `EnumItemsInRect` with
//! `GetEnumItem` / `GetFilterItem`, and `ChooseRandomItem` / `ChooseRandomItemEx`.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions in `docs/spec/items.md`. Labels as in `unit.rs`:
//! CONFIRMED = read in the decompile or disassembly, LIKELY = strong inference, UNSURE = guessed or not read.
//!
//! * Handle resolution (CONFIRMED, spec 1.): every item native resolves its item through
//!   [`Game::item_of_handle`]. A null, freed, wrong-type or removed handle gives the native's default, with no side
//!   effect.
//! * Handle order (CONFIRMED, spec 3.1 and 3.2): `CreateItem` builds the item and allocates its handle last, so an
//!   unknown type allocates no id. The `UnitAddItem*` creators build the item at the unit's position, do the add, and
//!   only then allocate the handle (spec 5.4). `UnitAddItemById` returns that handle whatever the add result
//!   (LIKELY, from the ignored return value of the add; spec 5.4 also says it returns 0 on a failed add, which the
//!   spec contradicts itself on). `UnitAddItemToSlotById` returns the add result as a boolean (TSV signature), and
//!   the item keeps its handle on the ground either way (LIKELY).
//! * Removal (CONFIRMED for the detach and the ground placement, spec 4 and 5.5; UNSURE for the id): `RemoveItem`
//!   marks the item removed and detaches it from its holder, then releases the creation reference, so the id is
//!   freed when no saved handle or group holds it. The spec says only that the id stays valid until the refcount
//!   reaches zero; what drops the creation reference is not read. This follows the unit model (`RemoveUnit`).
//! * Pack (CONFIRMED structure, spec 5.1 to 5.5): the capacity is the unit type's `inventory_size` (the original's
//!   pack capacity source is UNSURE, spec 9.6). A unit with capacity 0 has no pack, so every pack add fails. A
//!   non-powerup takes the lowest free slot, and a full pack fails. A powerup (flag 0x800) is consumed at once and
//!   never takes a slot, so it is not limited by the capacity. Not modelled: the gold/lumber resource-slot check of
//!   powerups (`0xD6`), the unit flag 0x4 in the full-pack test, and the pickup refusals (`0x55`, `0xDD`, `0xD3`,
//!   spec 9.7). Adding an item that another unit carries first takes it out of that pack (UNSURE). Adding an item
//!   already in this unit's pack fails (UNSURE).
//! * Drop and use (CONFIRMED that an order is queued, spec 5.6): `UnitDropItem*` check the unit, the item (in
//!   its pack), and for the slot form the slot range, then record an [`ItemOrder`] on the game and return `true`.
//!   The order check of the original (its code `0` or `0xDC`) is not modelled, so it always passes (UNSURE). The
//!   move happens later in the order system, which does not exist yet. `UnitDropItemTarget` accepts a live unit as
//!   its target (UNSURE: the original's widget rule is not read). Use orders (`UnitUseItem*`) are recorded the same way (see [`queue_use`]).
//! * `SetItemVisible(false)` and `RemoveItem` set the hidden bit (bit 0 of the flag word). `SetItemVisible(true)`
//!   clears it (UNSURE, spec 9.4). A carried item cannot be shown or hidden: the call does nothing (CONFIRMED).
//! * Charges (CONFIRMED, spec 4): `n <= 0` sets the no-charges flag (0x1000) and the charges to 0, `n > 0` clears
//!   the flag and sets the charges to `n`. A new item with template charges 0 gets the flag (CONFIRMED for the
//!   creation rule, the `<= 0` extension is UNSURE).
//! * Owner (LIKELY, spec 4): `SetItemPlayer` stores the owner index. A null or unknown player does nothing (UNSURE).
//!   `changeColor` has no effect here (UNSURE). `GetItemPlayer` returns the handle of the owner (LIKELY).
//! * `IsItemOwned` is true while the holder unit is alive (CONFIRMED, spec 4). A holder that was removed gives false.
//! * `EnumItemsInRect` (spec 6.1, walk order UNSURE): the candidates are the items of the arena in creation order
//!   ([`item_walk`], isolated for the day the walker is read). Per candidate the same order as the spec's callback:
//!   skip the removed, the carried and the handle-less items; set `GetEnumItem` and `GetFilterItem` to the item; run
//!   the boolexpr; when it passes and an action is given, run the action; clear both. So the filter order is the
//!   unit one (`GetFilterUnit` is set for the filter too). The spec's step-4 reading sets `GetFilterItem` only after
//!   the boolexpr passes, and is UNSURE (spec 6.2); that is the point to confirm against the original. The rect
//!   test is inclusive, as the unit query's placeholder (UNSURE). Hidden items are visited (UNSURE, spec 6.1).
//! * `ChooseRandomItem(level)` and `ChooseRandomItemEx(type, level)` (CONFIRMED, spec 7): the candidates come from
//!   the registry in registration order ([`crate::natives::game::ItemTypes`], the original's list order is LIKELY
//!   load-history based, spec 9.3). A candidate passes when its id is not 0, its class matches the category (any
//!   class for category 8, always for the plain form), its level field matches (`-1` matches any; the plain
//!   category 7 reads `level_plain`, other categories read `level_ex`), and its `flag_48` is nonzero (flag5 is 0 in
//!   both natives). With no candidate there is no draw. Otherwise one step of RNG stream 35 gives
//!   `candidates[high32(r * n)]`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, RunStatus, Value};
use softfloat::F32;

use super::game::{
    item_flags, Game, HandleKind, ItemObj, ItemOrder, ItemOrderKind, ObjKey, Object, RectObj,
};
use super::group::in_rect;
use super::hashtable::enum_ret;
use super::player::raw_at;
use super::trigger::eval_boolexpr;
use super::unit_attr::{NativeEntry, Support};
use super::ScriptRuntime;

/// The RNG stream of the item selection (spec 7.3, `0x23`).
const ITEM_STREAM: usize = 35;
/// The category of the plain `ChooseRandomItem` (spec 7.1).
const CATEGORY_PLAIN: i32 = 7;
/// The category that accepts every item class in `ChooseRandomItemEx` (spec 7.2 item 2).
const CATEGORY_ANY: i32 = 8;

/// The support table of the item-type and item-data natives registered by [`register_types`].
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "IsItemIdPowerup",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsItemIdSellable",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsItemIdPawnable",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsItemSellable",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetItemType",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetItemLevel",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetItemUserData",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetItemUserData",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetItemDropID",
        support: Support::Likely,
    },
];

/// Registers the item natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateItem", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match new_item(&gm, raw_at(a, 0) as u32, real_at(a, 1), real_at(a, 2)) {
            Some(obj) => gm.add_object(Object::Item(obj), 0).1,
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("RemoveItem", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = item_at(&gm, a, 0) {
            remove_item(&mut gm, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetItemPosition", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = item_at(&gm, a, 0) {
            if carried_by(&gm, k).is_some() {
                detach(&mut gm, k, true);
            }
            let it = gm.item_mut(k);
            it.x = real_at(a, 1);
            it.y = real_at(a, 2);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetItemX", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.item(k).x);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetItemY", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.item(k).y);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetItemTypeId", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).map_or(0, |k| gm.item(k).type_id as i32);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetItemCharges", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).map_or(0, |k| gm.item(k).charges);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetItemCharges", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = item_at(&gm, a, 0) {
            set_charges(&mut gm, k, raw_at(a, 1));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetItemPlayer", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match item_at(&gm, a, 0) {
            Some(k) => {
                let owner = gm.item(k).owner;
                gm.player_handle(owner)
            }
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("SetItemPlayer", move |_, a| {
        let mut gm = g.borrow_mut();
        let owner = gm.player_index_of_handle(handle_arg(a, 1));
        if let (Some(k), Some(owner)) = (item_at(&gm, a, 0), owner) {
            gm.item_mut(k).owner = owner;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsItemOwned", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).is_some_and(|k| carried_by(&gm, k).is_some());
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("SetItemVisible", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = item_at(&gm, a, 0) {
            if carried_by(&gm, k).is_none() {
                set_flag(&mut gm, k, item_flags::HIDDEN, !bool_arg(a, 1));
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsItemVisible", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).is_some_and(|k| gm.item(k).flags & item_flags::HIDDEN == 0);
        NativeResult::Return(Value::Boolean(v))
    });

    flag_setter(reg, game, "SetItemInvulnerable", item_flags::INVULNERABLE);
    flag_getter(reg, game, "IsItemInvulnerable", item_flags::INVULNERABLE);
    flag_setter(reg, game, "SetItemDropOnDeath", item_flags::DROP_ON_DEATH);
    flag_setter(reg, game, "SetItemDroppable", item_flags::DROPPABLE);
    flag_getter(reg, game, "IsItemPowerup", item_flags::POWERUP);
    flag_setter(reg, game, "SetItemPawnable", item_flags::PAWNABLE);
    flag_getter(reg, game, "IsItemPawnable", item_flags::PAWNABLE);

    let g = Rc::clone(game);
    reg.register("UnitAddItem", move |_, a| {
        let mut gm = g.borrow_mut();
        let ok = match (unit_at(&gm, a, 0), item_at(&gm, a, 1)) {
            (Some(u), Some(i)) => add_to_unit(&mut gm, u, i),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("UnitAddItemById", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match unit_at(&gm, a, 0) {
            Some(u) => match new_item_at(&mut gm, u, raw_at(a, 1) as u32) {
                Some(item) => {
                    gm.item_mut(item).flags |= item_flags::ADDED_BY_ID;
                    add_to_unit(&mut gm, u, item);
                    give_handle(&mut gm, item)
                }
                None => 0,
            },
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("UnitAddItemToSlotById", move |_, a| {
        let mut gm = g.borrow_mut();
        let ok = match unit_at(&gm, a, 0) {
            Some(u) => match new_item_at(&mut gm, u, raw_at(a, 1) as u32) {
                Some(item) => {
                    gm.item_mut(item).flags |= item_flags::ADDED_BY_ID;
                    let ok = add_to_slot(&mut gm, u, item, raw_at(a, 2));
                    give_handle(&mut gm, item);
                    ok
                }
                None => false,
            },
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("UnitRemoveItem", move |_, a| {
        let mut gm = g.borrow_mut();
        if let (Some(u), Some(i)) = (unit_at(&gm, a, 0), item_at(&gm, a, 1)) {
            if slot_of(&gm, u, i).is_some() {
                detach(&mut gm, i, true);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitRemoveItemFromSlot", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match unit_at(&gm, a, 0).and_then(|u| slot_item_at(&gm, u, raw_at(a, 1))) {
            Some(item) => {
                detach(&mut gm, item, true);
                gm.handle_or_create(item)
            }
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("UnitHasItem", move |_, a| {
        let gm = g.borrow();
        let v = match (unit_at(&gm, a, 0), item_at(&gm, a, 1)) {
            (Some(u), Some(i)) => slot_of(&gm, u, i).is_some(),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("UnitItemInSlot", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match unit_at(&gm, a, 0).and_then(|u| slot_item_at(&gm, u, raw_at(a, 1))) {
            Some(item) => gm.handle_or_create(item),
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("UnitInventorySize", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(0, |u| capacity(&gm, u) as i32);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("UnitDropItemPoint", move |_, a| {
        let mut gm = g.borrow_mut();
        let kind = ItemOrderKind::DropPoint {
            x: real_at(a, 2),
            y: real_at(a, 3),
        };
        let ok = queue_drop(&mut gm, a, kind);
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("UnitDropItemSlot", move |_, a| {
        let mut gm = g.borrow_mut();
        let ok = match (unit_at(&gm, a, 0), item_at(&gm, a, 1)) {
            (Some(u), Some(i)) if slot_of(&gm, u, i).is_some() => {
                match usize::try_from(raw_at(a, 2)) {
                    Ok(slot) if slot < capacity(&gm, u) => {
                        let kind = ItemOrderKind::DropSlot { slot: slot as u32 };
                        queue_drop(&mut gm, a, kind)
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("UnitDropItemTarget", move |_, a| {
        let mut gm = g.borrow_mut();
        // UNSURE: only a live unit is accepted as the widget target (see the module notes).
        let target = handle_arg(a, 2);
        let ok = gm.unit_of_handle(target).is_some()
            && queue_drop(&mut gm, a, ItemOrderKind::DropTarget { target });
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("UnitUseItem", move |_, a| {
        let mut gm = g.borrow_mut();
        let ok = queue_use(&mut gm, a, ItemOrderKind::UseImmediate);
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("UnitUseItemPoint", move |_, a| {
        let mut gm = g.borrow_mut();
        let kind = ItemOrderKind::UsePoint {
            x: real_at(a, 2),
            y: real_at(a, 3),
        };
        let ok = queue_use(&mut gm, a, kind);
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("UnitUseItemTarget", move |_, a| {
        let mut gm = g.borrow_mut();
        // The widget must resolve (0x6F3BE5B0). Only units are modelled as widgets here (UNSURE for the others).
        let target = handle_arg(a, 2);
        let ok = gm.unit_of_handle(target).is_some()
            && queue_use(&mut gm, a, ItemOrderKind::UseTarget { target });
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("EnumItemsInRect", move |ctx, a| {
        let key = ctx.thread_key();
        let rect = rect_at(&g.borrow(), a, 0);
        if let Some(rect) = rect {
            let filter = boolexpr_at(&g, a, 1);
            let action = code_at(a, 2);
            enum_items_in_rect(&g, ctx, key, rect, filter, action);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetEnumItem", move |_, _| {
        let id = g.borrow().enum_item();
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetFilterItem", move |_, _| {
        let id = g.borrow().filter_item();
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("ChooseRandomItem", move |_, a| {
        let mut gm = g.borrow_mut();
        let result = choose_random(&mut gm, CATEGORY_PLAIN, true, raw_at(a, 0));
        match result {
            Ok(id) => NativeResult::Return(Value::Integer(id as i32)),
            Err(status) => NativeResult::Fail(status),
        }
    });

    let g = Rc::clone(game);
    reg.register("ChooseRandomItemEx", move |_, a| {
        let mut gm = g.borrow_mut();
        let result = choose_random(&mut gm, raw_at(a, 0), false, raw_at(a, 1));
        match result {
            Ok(id) => NativeResult::Return(Value::Integer(id as i32)),
            Err(status) => NativeResult::Fail(status),
        }
    });

    register_types(reg, game);
}

/// Registers the item-type and item-data natives: the `IsItemId*` tests on a type id, `IsItemSellable`, `GetItemType`,
/// `GetItemLevel`, the user data and the drop id. The type record offsets are in `docs/spec/items.md` 3.4 (CONFIRMED
/// for `+0x38`, `+0x3C`, `+0x4C`, `+0x50`, `+0x54`: `FUN_6f2b8c50`, `FUN_6f2b8cb0`, `FUN_6f2b8dd0`, `FUN_6f2b8e00`,
/// `FUN_6f2b8e30`), the item fields `+0x80` and `+0xCC` (CONFIRMED: `FUN_6f3c50f0`, `FUN_6f3c5030`).
fn register_types(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("IsItemIdPowerup", move |_, a| {
        let v = g
            .borrow()
            .item_types()
            .get(raw_at(a, 0) as u32)
            .is_some_and(|t| t.powerup);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsItemIdSellable", move |_, a| {
        let v = g
            .borrow()
            .item_types()
            .get(raw_at(a, 0) as u32)
            .is_some_and(|t| t.sellable);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsItemIdPawnable", move |_, a| {
        let v = g
            .borrow()
            .item_types()
            .get(raw_at(a, 0) as u32)
            .is_some_and(|t| t.pawnable);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsItemSellable", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0)
            .and_then(|k| gm.item_types().get(gm.item(k).type_id))
            .is_some_and(|t| t.sellable);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("GetItemType", move |_, a| {
        // The original answers 7 (any) for a null item and for a type it cannot find.
        let gm = g.borrow();
        let class = item_at(&gm, a, 0)
            .and_then(|k| gm.item_types().get(gm.item(k).type_id))
            .map_or(7, |t| t.class as i32);
        NativeResult::Return(enum_ret(class))
    });

    let g = Rc::clone(game);
    reg.register("GetItemLevel", move |_, a| {
        let gm = g.borrow();
        let level = item_at(&gm, a, 0)
            .and_then(|k| gm.item_types().get(gm.item(k).type_id))
            .map_or(0, |t| t.level_ex);
        NativeResult::Return(Value::Integer(level))
    });

    let g = Rc::clone(game);
    reg.register("GetItemUserData", move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).map_or(0, |k| gm.item(k).user_data);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetItemUserData", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = item_at(&gm, a, 0) {
            gm.item_mut(k).user_data = raw_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetItemDropID", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = item_at(&gm, a, 0) {
            gm.item_mut(k).drop_id = raw_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });
}

/// Registers a native that sets (`on` argument) one bit of the flag word.
fn flag_setter(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>, name: &str, bit: u32) {
    let g = Rc::clone(game);
    reg.register(name, move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = item_at(&gm, a, 0) {
            set_flag(&mut gm, k, bit, bool_arg(a, 1));
        }
        NativeResult::Return(Value::Null)
    });
}

/// Registers a native that returns one bit of the flag word.
fn flag_getter(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>, name: &str, bit: u32) {
    let g = Rc::clone(game);
    reg.register(name, move |_, a| {
        let gm = g.borrow();
        let v = item_at(&gm, a, 0).is_some_and(|k| gm.item(k).flags & bit != 0);
        NativeResult::Return(Value::Boolean(v))
    });
}

/// The item of a new `CreateItem` (spec 3.2): the type record must exist, else the builder fails and nothing is
/// made. The flag word starts from the type's bits, with the no-charges bit when the charges are 0 (spec 3.3, the
/// `<= 0` form is UNSURE). The item starts on the ground, owned by player 0 (UNSURE).
pub(super) fn new_item(gm: &Game, type_id: u32, x: F32, y: F32) -> Option<ItemObj> {
    let ty = gm.item_types().get(type_id)?;
    let mut flags = ty.flags;
    if ty.charges <= 0 {
        flags |= item_flags::NO_CHARGES;
    }
    Some(ItemObj {
        type_id,
        x,
        y,
        flags,
        charges: ty.charges,
        owner: 0,
        holder: None,
        // UNSURE: the user data and the drop id start at 0 (the original's new object is zeroed there).
        user_data: 0,
        drop_id: 0,
        removed: false,
    })
}

/// A new item of `type_id` at the position of `unit`, without a handle yet (spec 5.4). `None` for an unknown
/// type.
fn new_item_at(gm: &mut Game, unit: ObjKey, type_id: u32) -> Option<ObjKey> {
    let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
    let obj = new_item(gm, type_id, x, y)?;
    Some(gm.push_object(Object::Item(obj)))
}

/// Gives a built item its handle, the last step of its creation (spec 5.4). An item that was consumed on the way
/// is released again at once, so its id is not kept by the table. Returns the handle id.
fn give_handle(gm: &mut Game, item: ObjKey) -> u32 {
    let id = gm.handle_or_create(item);
    if gm.item(item).removed {
        gm.kill(item);
    }
    id
}

/// `RemoveItem` (spec 4 and 5.5): detaches the item from its holder (without moving it), marks it removed and
/// releases its creation reference.
fn remove_item(gm: &mut Game, item: ObjKey) {
    detach(gm, item, false);
    let it = gm.item_mut(item);
    it.removed = true;
    it.flags |= item_flags::HIDDEN;
    gm.kill(item);
}

/// The unit that carries `item` (its holder), if the holder is alive. The spec's "carried" test (`IsItemOwned`).
fn carried_by(gm: &Game, item: ObjKey) -> Option<ObjKey> {
    gm.item(item).holder.filter(|&u| !gm.unit(u).removed)
}

/// Sets or clears one bit of the flag word.
fn set_flag(gm: &mut Game, item: ObjKey, bit: u32, on: bool) {
    let flags = &mut gm.item_mut(item).flags;
    if on {
        *flags |= bit;
    } else {
        *flags &= !bit;
    }
}

/// `SetItemCharges` (spec 4): 0 or less sets the no-charges flag and the charges to 0, more clears the flag and
/// sets the charges.
fn set_charges(gm: &mut Game, item: ObjKey, n: i32) {
    let it = gm.item_mut(item);
    if n <= 0 {
        it.flags |= item_flags::NO_CHARGES;
        it.charges = 0;
    } else {
        it.flags &= !item_flags::NO_CHARGES;
        it.charges = n;
    }
}

// Pack model (spec 5.1). A pack holds `capacity` slots, each the item in it or `None`. The slot of an item is
// the index in the pack. The holder of an item is kept in step with its slot.

/// The capacity of the pack of `unit`: the type's inventory size (0 when the unit has no inventory).
fn capacity(gm: &Game, unit: ObjKey) -> usize {
    gm.unit_types()
        .get(gm.unit(unit).type_id)
        .map_or(0, |t| t.inventory_size as usize)
}

/// The item in `slot` of `unit`'s pack, for slots below the capacity.
fn slot_item(gm: &Game, unit: ObjKey, slot: usize) -> Option<ObjKey> {
    gm.packs
        .get(&unit)
        .and_then(|pack| pack.get(slot))
        .copied()
        .flatten()
}

/// The item in JASS slot index `slot` (negative or out of range gives `None`).
fn slot_item_at(gm: &Game, unit: ObjKey, slot: i32) -> Option<ObjKey> {
    let slot = usize::try_from(slot).ok()?;
    if slot >= capacity(gm, unit) {
        return None;
    }
    slot_item(gm, unit, slot)
}

/// The slot index of `item` in `unit`'s pack, if it is there.
fn slot_of(gm: &Game, unit: ObjKey, item: ObjKey) -> Option<usize> {
    (0..capacity(gm, unit)).find(|&s| slot_item(gm, unit, s) == Some(item))
}

/// The number of occupied slots.
fn occupied(gm: &Game, unit: ObjKey) -> usize {
    (0..capacity(gm, unit))
        .filter(|&s| slot_item(gm, unit, s).is_some())
        .count()
}

/// Whether `unit` can take an item (the add check of spec 5.2, for the flags that are modelled). A unit without a
/// pack takes nothing. A powerup is always accepted, because it takes no slot. Any other item needs a free slot.
fn can_take(gm: &Game, unit: ObjKey, powerup: bool) -> bool {
    let cap = capacity(gm, unit);
    cap > 0 && (powerup || occupied(gm, unit) < cap)
}

/// Puts `item` into `slot` of `unit`'s pack and makes `unit` its holder.
fn put(gm: &mut Game, unit: ObjKey, slot: usize, item: ObjKey) {
    let pack = gm.packs.entry(unit).or_default();
    if pack.len() <= slot {
        pack.resize(slot + 1, None);
    }
    pack[slot] = Some(item);
    gm.item_mut(item).holder = Some(unit);
}

/// Takes `item` out of its holder's pack, clearing the slot. With `to_ground` the item is placed at the
/// holder's position (the spec's drop placement, spec 5.5); otherwise it keeps its position.
fn detach(gm: &mut Game, item: ObjKey, to_ground: bool) {
    let Some(unit) = gm.item(item).holder else {
        return;
    };
    if let Some(pack) = gm.packs.get_mut(&unit) {
        for slot in pack.iter_mut() {
            if *slot == Some(item) {
                *slot = None;
            }
        }
    }
    let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
    let it = gm.item_mut(item);
    it.holder = None;
    if to_ground {
        it.x = x;
        it.y = y;
    }
}

/// The add of `item` to `unit` (`UnitAddItem`, spec 5.2): a powerup is consumed, any other item takes the lowest
/// free slot. Returns whether the item was taken. An item carried by another unit leaves that pack first.
fn add_to_unit(gm: &mut Game, unit: ObjKey, item: ObjKey) -> bool {
    let powerup = gm.item(item).flags & item_flags::POWERUP != 0;
    if gm.item(item).holder == Some(unit) || !can_take(gm, unit, powerup) {
        return false;
    }
    detach(gm, item, true);
    if powerup {
        remove_item(gm, item);
        return true;
    }
    let free = (0..capacity(gm, unit)).find(|&s| slot_item(gm, unit, s).is_none());
    match free {
        Some(slot) => {
            put(gm, unit, slot, item);
            true
        }
        None => false,
    }
}

/// The add of `item` to the given JASS slot (`UnitAddItemToSlotById`): the slot must be in range and empty. A
/// powerup is consumed, as in [`add_to_unit`] (UNSURE for the slot form).
fn add_to_slot(gm: &mut Game, unit: ObjKey, item: ObjKey, slot: i32) -> bool {
    let powerup = gm.item(item).flags & item_flags::POWERUP != 0;
    if gm.item(item).holder == Some(unit) || !can_take(gm, unit, powerup) {
        return false;
    }
    if powerup {
        remove_item(gm, item);
        return true;
    }
    let Some(slot) = usize::try_from(slot)
        .ok()
        .filter(|&s| s < capacity(gm, unit) && slot_item(gm, unit, s).is_none())
    else {
        return false;
    };
    detach(gm, item, true);
    put(gm, unit, slot, item);
    true
}

/// The shared checks and the queue of the drop natives (spec 5.6): the item must be in the unit's pack. The
/// slot-range check is made by the caller. Returns `true` when the order is queued.
fn queue_drop(gm: &mut Game, a: &[Value], kind: ItemOrderKind) -> bool {
    let (Some(unit), Some(item)) = (unit_at(gm, a, 0), item_at(gm, a, 1)) else {
        return false;
    };
    if slot_of(gm, unit, item).is_none() {
        return false;
    }
    gm.push_item_order(ItemOrder { unit, item, kind });
    true
}

/// The shared checks and the queue of the use natives (`0x6F3C9090`, `0x6F3C9100`, `0x6F3C9170`, CONFIRMED by
/// disassembly): the unit resolves and can use items (`0x6F282C60`: it has an inventory), the item resolves and is in
/// the unit's pack (`0x6F26EC40`), then the item's ability order id is read (`0x6F2B6CC0`) and the order is issued
/// with the owner as the issuer (replace = 1, drain = 1). The ability order id is data driven and not modelled, so
/// only the order kind is recorded. The validator result of the issue is not modelled (UNSURE, as for the drops).
fn queue_use(gm: &mut Game, a: &[Value], kind: ItemOrderKind) -> bool {
    let (Some(unit), Some(item)) = (unit_at(gm, a, 0), item_at(gm, a, 1)) else {
        return false;
    };
    if capacity(gm, unit) == 0 || slot_of(gm, unit, item).is_none() {
        return false;
    }
    gm.push_item_order(ItemOrder { unit, item, kind });
    true
}

/// The candidate items of `EnumItemsInRect`, in creation order (UNSURE, spec 6.1: the walker is not read). Only
/// the arena is walked, so this is the one place that defines the order.
fn item_walk(gm: &Game) -> Vec<ObjKey> {
    (0..gm.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(gm.object(k), Object::Item(_)))
        .collect()
}

/// The random item selection (spec 7.2), stream 35 for the pick (spec 7.3). `plain_native` is true for
/// `ChooseRandomItem`, which passes the class test by its constant 7. A null result is `0`.
fn choose_random(
    gm: &mut Game,
    category: i32,
    plain_native: bool,
    level: i32,
) -> Result<u32, RunStatus> {
    let candidates = random_candidates(gm, category, plain_native, level);
    if candidates.is_empty() {
        return Ok(0);
    }
    let (table, streams) = gm.rng_parts().ok_or(RunStatus::MissingTables)?;
    let r = streams.stream_mut(ITEM_STREAM).next_u32(table);
    let index = ((u64::from(r) * candidates.len() as u64) >> 32) as usize;
    Ok(candidates[index])
}

/// The candidate ids of the random selection, in registry order (see the module notes).
fn random_candidates(gm: &Game, category: i32, plain_native: bool, level: i32) -> Vec<u32> {
    // The level field is `+0x40` when the category is 7 (both forms), else `+0x38`.
    let plain_level = category == CATEGORY_PLAIN;
    gm.item_types()
        .ids()
        .iter()
        .copied()
        .filter(|&id| {
            if id == 0 {
                return false;
            }
            let Some(ty) = gm.item_types().get(id) else {
                return false;
            };
            let class_ok = plain_native || category == CATEGORY_ANY || ty.class == category as u32;
            let field = if plain_level {
                ty.level_plain
            } else {
                ty.level_ex
            };
            let level_ok = level == -1 || field == level;
            class_ok && level_ok && ty.flag_48 != 0
        })
        .collect()
}

/// `EnumItemsInRect` (spec 6.1 and 6.2): the callback for each candidate, with `GetEnumItem` set for its
/// duration. `GetFilterItem` is set for the boolexpr and for the action, as the unit filter is (the spec's
/// step-4 reading would set it only for the action; that order is UNSURE, see the module notes).
fn enum_items_in_rect(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    key: u32,
    rect: RectObj,
    filter: Option<ObjKey>,
    action: Option<u32>,
) {
    let candidates = item_walk(&game.borrow());
    for item in candidates {
        {
            let mut gm = game.borrow_mut();
            if !candidate_in_rect(&gm, item, &rect) {
                continue;
            }
            let id = gm.handle_or_create(item);
            gm.push_enum_item(id);
            gm.push_filter_item(id);
        }
        let passed = match filter {
            None => true,
            Some(b) => eval_boolexpr(game, rt, b, key),
        };
        if passed {
            if let Some(code) = action {
                let _ = rt.call_keyed(code, key);
            }
        }
        let mut gm = game.borrow_mut();
        gm.pop_filter_item();
        gm.pop_enum_item();
    }
}

/// The level test of a candidate: not removed, on the ground, with a live handle, and inside the rect (the
/// rect test is the unit query's inclusive placeholder, UNSURE).
fn candidate_in_rect(gm: &Game, item: ObjKey, rect: &RectObj) -> bool {
    let it = gm.item(item);
    !it.removed && it.holder.is_none() && gm.is_alive(item) && in_rect(it.x, it.y, rect)
}

/// The unit in handle argument `i`, if it is a live unit.
fn unit_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.unit_of_handle(handle_arg(a, i))
}

/// The item in handle argument `i`, if it is a live item.
fn item_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.item_of_handle(handle_arg(a, i))
}

/// The rect in handle argument `i`, if it is a live rect.
fn rect_at(gm: &Game, a: &[Value], i: usize) -> Option<RectObj> {
    let k = gm.handle_of_kind(handle_arg(a, i), HandleKind::Rect)?;
    match gm.object(k) {
        Object::Rect(r) => Some(*r),
        _ => None,
    }
}

/// The boolexpr in argument `i`, if it is a live boolexpr. A null or invalid filter means no filter.
fn boolexpr_at(game: &RefCell<Game>, a: &[Value], i: usize) -> Option<ObjKey> {
    let gm = game.borrow();
    gm.handle_of_kind(a.get(i)?.as_handle()?, HandleKind::BoolExpr)
}

/// The code in argument `i`, if it is a function value.
fn code_at(a: &[Value], i: usize) -> Option<u32> {
    match a.get(i)? {
        Value::Code(f) => Some(*f),
        _ => None,
    }
}

/// The handle id in argument `i`, or 0 (the null handle) when there is none.
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

/// The real in argument `i`, or 0.0.
fn real_at(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

/// The boolean in argument `i`, or false.
fn bool_arg(a: &[Value], i: usize) -> bool {
    match a.get(i) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(n)) => *n != 0,
        _ => false,
    }
}
