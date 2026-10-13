//! Order natives (`IssueImmediateOrder*`, `IssuePointOrder*`, `IssueTargetOrder*`, `IssueInstant*Order*`,
//! `GetUnitCurrentOrder`, `GetIssuedOrderId`, the `GetOrderPoint*` and `GetOrderTarget*` getters, `OrderId`,
//! `OrderId2String`) and the issued-order events (player-unit 38/39/40 and unit 75/76/77).
//!
//! The model is in `crate::orders` (lists, dispatcher, ability lookup, mover). Addresses and labels are in
//! `docs/spec/orders.md` and `orders-2.md`.
//!
//! * Native results (CONFIRMED, orders.md 3.2 and 3.3): the native returns `true` when the validator accepts
//!   the order and the order was dispatched. A dispatch that is dropped (locked unit, count limit) still
//!   returns `true`. Name forms return `false` for an unknown name. A null unit returns `false`.
//! * Class of a point order (CONFIRMED correction C1, orders-2): an id with flags 0 builds the target class;
//!   [`crate::orders::point_capable`] stands in for the flag table (UNVERIFIED). Instant point orders build a
//!   point order with the widget as its optional target; instant target orders build a target order (UNVERIFIED).
//! * Events (LIKELY timing, UNVERIFIED): after an accepted order, the player-unit event of the owner (38 immediate,
//!   39 point, 40 target) runs, then the unit event (75, 76, 77) of the unit. The timing is at the issue, not at
//!   the start or completion (the spec names the event ids; the firing site is not located). The trigger threads
//!   carry the record index, which the getters read.
//! * Getters (CONFIRMED kinds, orders.md 3.7): `GetOrderPointX/Y/Loc` answer for point orders and 0 otherwise;
//!   `GetOrderTarget*` answer the target handle (UNVERIFIED for the typed variants: unit and item are checked by
//!   the handle's type; destructable is not registered here, see `natives/destructable.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{Ctx, Game, HandleKind, LocationObj, ObjKey, Object};
use super::player::raw_at;
use super::trigger::fire_with_order;
use super::unit_event::{filter_passes, KEY_BASE};
use super::ScriptRuntime;
use crate::orders::{order_id_by_name, order_name, point_capable, Order, OrderClass};
use softfloat::F32;

/// Player-unit event ids of the issued order: immediate, point, target (CONFIRMED values 38, 39, 40).
const PLAYER_ISSUED: [u32; 3] = [38, 39, 40];
/// Unit event ids of the issued order: immediate, point, target (75, 76, 77).
const UNIT_ISSUED: [u32; 3] = [75, 76, 77];

/// The support table of the group order natives registered by [`register_group`]. Immediate orders reach every member,
/// point and target orders reach the first 12 entries (CONFIRMED from the decompile). The target order returns true
/// without issuing when the target is not a live unit (CONFIRMED). The return value is "any member accepted"
/// (CONFIRMED). The issue path is the one of the single-unit natives (`issue_order`).
pub const GROUP_NATIVES: &[super::unit_attr::NativeEntry] = &[
    super::unit_attr::NativeEntry {
        name: "GroupImmediateOrder",
        support: super::unit_attr::Support::Confirmed,
    },
    super::unit_attr::NativeEntry {
        name: "GroupImmediateOrderById",
        support: super::unit_attr::Support::Confirmed,
    },
    super::unit_attr::NativeEntry {
        name: "GroupPointOrder",
        support: super::unit_attr::Support::Confirmed,
    },
    super::unit_attr::NativeEntry {
        name: "GroupPointOrderById",
        support: super::unit_attr::Support::Confirmed,
    },
    super::unit_attr::NativeEntry {
        name: "GroupPointOrderByIdLoc",
        support: super::unit_attr::Support::Confirmed,
    },
    super::unit_attr::NativeEntry {
        name: "GroupPointOrderLoc",
        support: super::unit_attr::Support::Confirmed,
    },
    super::unit_attr::NativeEntry {
        name: "GroupTargetOrder",
        support: super::unit_attr::Support::Likely,
    },
    super::unit_attr::NativeEntry {
        name: "GroupTargetOrderById",
        support: super::unit_attr::Support::Likely,
    },
];

/// Registers the order natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("IssueImmediateOrder", move |ctx, a| {
        let id = string_id(ctx, a, 1);
        let ok = match id {
            Some(id) => immediate(&g, ctx, unit_arg(&g, a, 0), id),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueImmediateOrderById", move |ctx, a| {
        let ok = immediate(&g, ctx, unit_arg(&g, a, 0), raw_at(a, 1) as u32);
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssuePointOrder", move |ctx, a| {
        let ok = match string_id(ctx, a, 1) {
            Some(id) => point(
                &g,
                ctx,
                unit_arg(&g, a, 0),
                id,
                real_at(a, 2),
                real_at(a, 3),
            ),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssuePointOrderLoc", move |ctx, a| {
        let ok = match (string_id(ctx, a, 1), loc_arg(&g, a, 2)) {
            (Some(id), Some(loc)) => point(&g, ctx, unit_arg(&g, a, 0), id, loc.x, loc.y),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssuePointOrderById", move |ctx, a| {
        let ok = point(
            &g,
            ctx,
            unit_arg(&g, a, 0),
            raw_at(a, 1) as u32,
            real_at(a, 2),
            real_at(a, 3),
        );
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssuePointOrderByIdLoc", move |ctx, a| {
        let ok = match loc_arg(&g, a, 2) {
            Some(loc) => point(
                &g,
                ctx,
                unit_arg(&g, a, 0),
                raw_at(a, 1) as u32,
                loc.x,
                loc.y,
            ),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueTargetOrder", move |ctx, a| {
        let ok = match string_id(ctx, a, 1) {
            Some(id) => target(&g, ctx, unit_arg(&g, a, 0), id, handle_arg(a, 2), None),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueTargetOrderById", move |ctx, a| {
        let ok = target(
            &g,
            ctx,
            unit_arg(&g, a, 0),
            raw_at(a, 1) as u32,
            handle_arg(a, 2),
            None,
        );
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueInstantPointOrder", move |ctx, a| {
        let ok = match string_id(ctx, a, 1) {
            Some(id) => instant_point(
                &g,
                ctx,
                unit_arg(&g, a, 0),
                id,
                real_at(a, 2),
                real_at(a, 3),
                handle_arg(a, 4),
            ),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueInstantPointOrderById", move |ctx, a| {
        let ok = instant_point(
            &g,
            ctx,
            unit_arg(&g, a, 0),
            raw_at(a, 1) as u32,
            real_at(a, 2),
            real_at(a, 3),
            handle_arg(a, 4),
        );
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueInstantTargetOrder", move |ctx, a| {
        let ok = match string_id(ctx, a, 1) {
            Some(id) => target(
                &g,
                ctx,
                unit_arg(&g, a, 0),
                id,
                handle_arg(a, 2),
                Some(handle_arg(a, 3)),
            ),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueInstantTargetOrderById", move |ctx, a| {
        let ok = target(
            &g,
            ctx,
            unit_arg(&g, a, 0),
            raw_at(a, 1) as u32,
            handle_arg(a, 2),
            Some(handle_arg(a, 3)),
        );
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitCurrentOrder", move |_, a| {
        let gm = g.borrow();
        let id = unit_arg(&g, a, 0)
            .and_then(|k| gm.unit_order_ids(k).first().copied())
            .unwrap_or(0);
        NativeResult::Return(Value::Integer(id as i32))
    });

    let g = Rc::clone(game);
    reg.register("GetIssuedOrderId", move |ctx, _| {
        let gm = g.borrow();
        let id = issued_of(&gm, ctx.thread_key()).map_or(0, |o| o.id);
        NativeResult::Return(Value::Integer(id as i32))
    });

    let g = Rc::clone(game);
    reg.register("GetOrderPointX", move |ctx, _| {
        let gm = g.borrow();
        let v = point_of(&gm, ctx.thread_key()).map_or(F32::ZERO, |p| p.0);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetOrderPointY", move |ctx, _| {
        let gm = g.borrow();
        let v = point_of(&gm, ctx.thread_key()).map_or(F32::ZERO, |p| p.1);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetOrderPointLoc", move |ctx, _| {
        let mut gm = g.borrow_mut();
        let id = match point_of(&gm, ctx.thread_key()) {
            Some((x, y)) => gm.add_object(Object::Location(LocationObj { x, y }), 1).1,
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetOrderTarget", move |ctx, _| {
        let gm = g.borrow();
        let id = target_of(&gm, ctx.thread_key());
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetOrderTargetUnit", move |ctx, _| {
        let gm = g.borrow();
        let t = target_of(&gm, ctx.thread_key());
        let id = if gm.unit_of_handle(t).is_some() { t } else { 0 };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetOrderTargetItem", move |ctx, _| {
        let gm = g.borrow();
        let t = target_of(&gm, ctx.thread_key());
        let id = if gm.handle_of_kind(t, HandleKind::Item).is_some() {
            t
        } else {
            0
        };
        NativeResult::Return(Value::Handle(id))
    });

    reg.register("OrderId", move |ctx, a| {
        let id = string_id(ctx, a, 0).unwrap_or(0);
        NativeResult::Return(Value::Integer(id as i32))
    });

    reg.register("OrderId2String", move |ctx, a| {
        let id = raw_at(a, 0) as u32;
        let text = order_name(id).unwrap_or("");
        NativeResult::Return(Value::String(ctx.intern(text)))
    });

    register_group(reg, game);
}

/// The group natives that issue one order to every member: `GroupImmediateOrder*`, `GroupPointOrder*` and
/// `GroupTargetOrder*`. Their bodies are read in `FUN_6f3f50a0` (immediate), `FUN_6f3f5120` (point) and `FUN_6f3f5280`
/// (target), with the collector `FUN_6f3efcc0` (CONFIRMED, see the module table [`GROUP_NATIVES`]).
fn register_group(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("GroupImmediateOrder", move |ctx, a| {
        let ok = match (string_id(ctx, a, 1), group_arg(&g, a, 0)) {
            (Some(id), Some(group)) => group_immediate(&g, ctx, group, id),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GroupImmediateOrderById", move |ctx, a| {
        let ok = match group_arg(&g, a, 0) {
            Some(group) => group_immediate(&g, ctx, group, raw_at(a, 1) as u32),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GroupPointOrder", move |ctx, a| {
        let ok = match (string_id(ctx, a, 1), group_arg(&g, a, 0)) {
            (Some(id), Some(group)) => {
                group_point(&g, ctx, group, id, real_at(a, 2), real_at(a, 3))
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GroupPointOrderById", move |ctx, a| {
        let ok = match group_arg(&g, a, 0) {
            Some(group) => group_point(
                &g,
                ctx,
                group,
                raw_at(a, 1) as u32,
                real_at(a, 2),
                real_at(a, 3),
            ),
            None => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GroupPointOrderLoc", move |ctx, a| {
        let ok = match (string_id(ctx, a, 1), group_arg(&g, a, 0), loc_arg(&g, a, 2)) {
            (Some(id), Some(group), Some(loc)) => group_point(&g, ctx, group, id, loc.x, loc.y),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GroupPointOrderByIdLoc", move |ctx, a| {
        let ok = match (group_arg(&g, a, 0), loc_arg(&g, a, 2)) {
            (Some(group), Some(loc)) => {
                group_point(&g, ctx, group, raw_at(a, 1) as u32, loc.x, loc.y)
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GroupTargetOrder", move |ctx, a| {
        let ok = match (string_id(ctx, a, 1), widget_arg(&g, a, 2)) {
            (Some(id), Some(widget)) => match group_arg(&g, a, 0) {
                Some(group) => group_target(&g, ctx, group, id, widget),
                None => false,
            },
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GroupTargetOrderById", move |ctx, a| {
        let ok = match (widget_arg(&g, a, 2), group_arg(&g, a, 0)) {
            (Some(widget), Some(group)) => {
                group_target(&g, ctx, group, raw_at(a, 1) as u32, widget)
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(ok))
    });
}

/// How many entries of a group a point or target order reaches: the collector walks the first 12 entries of the
/// snapshot (CONFIRMED: both passes of `FUN_6f3f5120` and `FUN_6f3f5280` pass the count 0xC). The immediate order
/// has no limit (CONFIRMED: `FUN_6f3f50a0` passes 0xFFFFFFFF on its second pass).
const POINT_TARGET_REACH: usize = 12;

/// The members a group order reaches (`FUN_6f3efcc0`): the first `reach` entries of the group, in insertion order,
/// minus the entries whose unit is removed (a removed unit is a ghost; it still uses up a slot of the walk).
fn group_reach(g: &Rc<RefCell<Game>>, group: ObjKey, reach: usize) -> Vec<ObjKey> {
    let gm = g.borrow();
    gm.group(group)
        .members
        .iter()
        .take(reach)
        .map(|m| m.unit)
        .filter(|&u| !gm.unit(u).removed)
        .collect()
}

/// Issues `order` to each unit of the group reach, in order, and fires the issued-order events of each accepted one.
/// Returns whether any unit's validator accepted the order (CONFIRMED: the flag is set by the accepting callback and
/// never breaks the walk, so the result is "any", not "all").
fn issue_to_group(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    group: ObjKey,
    reach: usize,
    order: Order,
) -> bool {
    let class = order.class;
    let mut any = false;
    for unit in group_reach(g, group, reach) {
        let (validated, index) = g.borrow_mut().issue_order(unit, order.clone(), true);
        if let Some(index) = index {
            fire_issued(g, rt, unit, class, index);
        }
        any |= validated;
    }
    any
}

/// `GroupImmediateOrder*` (`FUN_6f3f50a0`): every member, immediate class.
fn group_immediate(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    group: ObjKey,
    id: u32,
) -> bool {
    let order = Order {
        id,
        class: OrderClass::Immediate,
        point: None,
        target: 0,
    };
    issue_to_group(g, rt, group, usize::MAX, order)
}

/// `GroupPointOrder*` (`FUN_6f3f5120`): the first 12 entries, point class at `(x, y)`.
fn group_point(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    group: ObjKey,
    id: u32,
    x: F32,
    y: F32,
) -> bool {
    let order = Order {
        id,
        class: OrderClass::Point,
        point: Some((x, y)),
        target: 0,
    };
    issue_to_group(g, rt, group, POINT_TARGET_REACH, order)
}

/// `GroupTargetOrder*` (`FUN_6f3f5280`): a target that is not a live unit gives `true` and issues nothing
/// (CONFIRMED: the early exit returns 1). A unit target goes to the first 12 entries of the group.
fn group_target(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    group: ObjKey,
    id: u32,
    widget: u32,
) -> bool {
    let target_is_unit = g.borrow().unit_of_handle(widget).is_some();
    if !target_is_unit {
        return true;
    }
    let order = Order {
        id,
        class: OrderClass::Target,
        point: None,
        target: widget,
    };
    issue_to_group(g, rt, group, POINT_TARGET_REACH, order)
}

/// The group in handle argument `i`, if it is a live group.
fn group_arg(g: &Rc<RefCell<Game>>, a: &[Value], i: usize) -> Option<ObjKey> {
    g.borrow()
        .handle_of_kind(handle_arg(a, i), HandleKind::Group)
}

/// The widget in handle argument `i`: a unit, an item or a destructable with a live handle (the original's widget
/// resolver, UNSURE beyond these three kinds). Returns the handle id, which the order stores as its target.
fn widget_arg(g: &Rc<RefCell<Game>>, a: &[Value], i: usize) -> Option<u32> {
    let id = handle_arg(a, i);
    let gm = g.borrow();
    let kind = gm.handle_object(id).map(|k| gm.object(k).handle_kind())?;
    matches!(
        kind,
        HandleKind::Unit | HandleKind::Item | HandleKind::Destructable
    )
    .then_some(id)
}

/// `IssueImmediateOrder*`: an immediate order with replace.
fn immediate(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    unit: Option<ObjKey>,
    id: u32,
) -> bool {
    let Some(unit) = unit else {
        return false;
    };
    issue(
        g,
        rt,
        unit,
        Order {
            id,
            class: OrderClass::Immediate,
            point: None,
            target: 0,
        },
    )
}

/// `IssuePointOrder*`: a point order, or a target order for an id with no point flags (orders-2 C1).
fn point(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    unit: Option<ObjKey>,
    id: u32,
    x: F32,
    y: F32,
) -> bool {
    let Some(unit) = unit else {
        return false;
    };
    let class = if point_capable(id) {
        OrderClass::Point
    } else {
        OrderClass::Target
    };
    issue(
        g,
        rt,
        unit,
        Order {
            id,
            class,
            point: Some((x, y)),
            target: 0,
        },
    )
}

/// `IssueTargetOrder*`: a target order. `second` is the instant form's second widget and is not used by the
/// model (UNVERIFIED), so the target is the first widget.
fn target(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    unit: Option<ObjKey>,
    id: u32,
    widget: u32,
    _second: Option<u32>,
) -> bool {
    let Some(unit) = unit else {
        return false;
    };
    if widget == 0 {
        return false;
    }
    issue(
        g,
        rt,
        unit,
        Order {
            id,
            class: OrderClass::Target,
            point: None,
            target: widget,
        },
    )
}

/// `IssueInstantPointOrder*`: a point order with the widget as its optional target (UNVERIFIED class).
fn instant_point(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn ScriptRuntime,
    unit: Option<ObjKey>,
    id: u32,
    x: F32,
    y: F32,
    widget: u32,
) -> bool {
    let Some(unit) = unit else {
        return false;
    };
    issue(
        g,
        rt,
        unit,
        Order {
            id,
            class: OrderClass::Point,
            point: Some((x, y)),
            target: widget,
        },
    )
}

/// Validates and dispatches an order from a native, then fires the issued-order events when the unit accepted
/// it. Returns the validator's result (the native's return value).
fn issue(g: &Rc<RefCell<Game>>, rt: &mut dyn ScriptRuntime, unit: ObjKey, order: Order) -> bool {
    // A spell order (id = the ability's order index on top of 0xD0028, abilities-4 section 2.1) starts the cast
    // pipeline of `cast.rs`; the native's result is whether the cast started (the readiness errors refuse it).
    if matches!(order.class, OrderClass::Target | OrderClass::Point | OrderClass::Immediate) {
        let spell = g.borrow().spell_for_order(unit, order.id);
        if let Some(base) = spell {
            let target = match (order.class, order.point) {
                (_, Some((x, y))) => crate::cast::CastTarget::Point(x, y),
                _ => {
                    let t = g.borrow().handle_of_kind(order.target, HandleKind::Unit);
                    t.map_or(crate::cast::CastTarget::None, crate::cast::CastTarget::Unit)
                }
            };
            return crate::cast::begin_cast(g, rt, unit, base, target).is_ok();
        }
    }
    let class = order.class;
    let (validated, index) = g.borrow_mut().issue_order(unit, order, true);
    if let Some(index) = index {
        fire_issued(g, rt, unit, class, index);
    }
    validated
}

/// Runs the issued-order events for record `index`: the owner's player-unit table first, then the unit's table.
///
/// Shared by the natives (`issue`, `issue_to_group`) and the replay path (`commands::CommandState::apply_fired`), so an
/// order from a replay fires the same events as one from a native. The game must not be borrowed by the caller.
pub(crate) fn fire_issued(
    g: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    class: OrderClass,
    index: u32,
) {
    let slot = match class {
        OrderClass::Immediate => 0,
        OrderClass::Point => 1,
        OrderClass::Target => 2,
    };
    let owner = g.borrow().unit(unit).owner;
    fire_observers_order(
        g,
        rt,
        ObjKey(owner),
        PLAYER_ISSUED[slot] + KEY_BASE,
        unit,
        index,
    );
    fire_observers_order(g, rt, unit, UNIT_ISSUED[slot] + KEY_BASE, unit, index);
}

/// Fires the entries of `agent`'s table with `key`, oldest first, with the order record `index` in their
/// context. Mirrors `unit_event::fire_observers`.
fn fire_observers_order(
    g: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    agent: ObjKey,
    key: u32,
    unit: ObjKey,
    index: u32,
) {
    let nodes: Vec<_> = g
        .borrow()
        .observers(agent)
        .iter()
        .filter(|o| o.key == key)
        .copied()
        .collect();
    for node in nodes {
        if let Some(b) = node.filter {
            if !filter_passes(g, rt, b, node, unit) {
                continue;
            }
        }
        fire_with_order(
            g,
            rt,
            node.trigger,
            Some(node.event),
            Some(unit),
            Some(index),
        );
    }
}

/// The issued order of the running thread's trigger context, if it is an order event.
fn issued_of(gm: &Game, thread: u32) -> Option<&Order> {
    let Some(Ctx::Trigger { order: Some(i), .. }) = gm.context(thread) else {
        return None;
    };
    gm.orders().issued().get(i as usize).map(|r| &r.order)
}

/// The point of the running order event, for point orders only.
fn point_of(gm: &Game, thread: u32) -> Option<(F32, F32)> {
    let o = issued_of(gm, thread)?;
    if o.class == OrderClass::Point {
        o.point
    } else {
        None
    }
}

/// The target handle of the running order event (0 when it has none).
fn target_of(gm: &Game, thread: u32) -> u32 {
    issued_of(gm, thread).map_or(0, |o| o.target)
}

/// The order id of a name argument, or `None` when the name is unknown (the native then returns false).
fn string_id(ctx: &jass::vm::NativeCtx<'_>, a: &[Value], i: usize) -> Option<u32> {
    let name = match a.get(i) {
        Some(Value::String(id)) => ctx.string(*id).to_string(),
        _ => String::new(),
    };
    order_id_by_name(&name)
}

/// The unit in argument `i`, if it is a live unit.
fn unit_arg(g: &Rc<RefCell<Game>>, a: &[Value], i: usize) -> Option<ObjKey> {
    g.borrow().unit_of_handle(handle_arg(a, i))
}

/// The location in argument `i`, if it is a live location.
fn loc_arg(g: &Rc<RefCell<Game>>, a: &[Value], i: usize) -> Option<LocationObj> {
    let gm = g.borrow();
    let k = gm.handle_of_kind(handle_arg(a, i), HandleKind::Location)?;
    match gm.object(k) {
        Object::Location(l) => Some(*l),
        _ => None,
    }
}

/// The handle id in argument `i`, or 0 (the null handle).
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

/// The real in argument `i`, or 0.0.
fn real_at(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}
