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
    let class = order.class;
    let (validated, index) = g.borrow_mut().issue_order(unit, order, true);
    if let Some(index) = index {
        fire_issued(g, rt, unit, class, index);
    }
    validated
}

/// Runs the issued-order events for record `index`: the owner's player-unit table first, then the unit's table.
fn fire_issued(
    g: &Rc<RefCell<Game>>,
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
    g: &Rc<RefCell<Game>>,
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
