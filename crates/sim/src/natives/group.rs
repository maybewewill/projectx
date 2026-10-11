//! Unit group natives: `CreateGroup`, `DestroyGroup`, `GroupAddUnit`, `GroupRemoveUnit`, `GroupClear`,
//! `IsUnitInGroup`, `FirstOfGroup`, `ForGroup`, the `GroupEnumUnits*` natives (with their `Counted`
//! variants), `GetEnumUnit` and `GetFilterUnit`.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions in `docs/spec/groups-events.md`. Labels as
//! in `unit.rs`: CONFIRMED = read in the decompile or disassembly, LIKELY = strong inference, UNSURE =
//! guessed or not read, UNVERIFIED = our model, not checked against the original.
//!
//! * Storage (CONFIRMED, spec 1.2 to 2.5). A group keeps its unit references in insertion order. A
//!   unit is stored once: a second insert of the same unit does nothing. Insert appends (LIKELY: units
//!   are never reordered by the sorted path, spec 2.3 step 6).
//! * Handle references (LIKELY, spec 1.4 and 2.3): a member holds one reference to the unit's handle id
//!   ([`Game::add_ref`]), released when it is removed, cleared or the group is destroyed. So a unit that
//!   is still in a group keeps its id after `RemoveUnit`, and the id is freed only when the group lets it
//!   go. The spec states the count on the agent object; that the group's reference is the handle table's
//!   reference is our reading (UNVERIFIED).
//! * Ghosts (CONFIRMED for the skip rules, spec 4.3). `RemoveUnit` does not touch groups, so a removed
//!   unit stays in its groups until the group is cleared or enumerated. Its count still includes it.
//!   `GroupAddUnit`, `GroupRemoveUnit` and `IsUnitInGroup` resolve the unit first, so they ignore a ghost.
//! * `FirstOfGroup` (CONFIRMED, spec 2.7) does not check the removed flag. It converts the head to a handle
//!   with the object-to-handle conversion, which creates a new id when the object has none (LIKELY: a
//!   removed unit has none, so a ghost gets a fresh id here).
//! * `ForGroup` (CONFIRMED, spec 4.1 and 4.2). The members are copied first, each copy holds a reference
//!   for the loop, and those references are released afterwards (LIKELY for the release). Units added
//!   during the loop are not visited. A unit whose removed flag is set when its turn comes is skipped. A
//!   dead unit is not skipped (CONFIRMED for the skip rule, LIKELY for dead units). The callback sees the
//!   unit as `GetEnumUnit` for its duration. The callback runs synchronously, with the thread's context.
//! * `GroupEnumUnits*` (CONFIRMED, spec 3.1 to 3.5). The group is cleared first, but only after the
//!   native's own checks pass (type name resolves, player and location or rect valid). The query state
//!   (filter, type, player, limit, counter) is stored in the group, and the collector reads it for each
//!   candidate. The collector order is: candidate test (removed flag, and the area test for the spatial
//!   queries), then the limit, then the type and owner test, then the filter (with `GetFilterUnit` set to
//!   the candidate), then the insert.
//! * Counted natives (CONFIRMED, spec 3.3, LIKELY for the meaning): the counter goes up only when the
//!   insert returns 0 (a failed insert, which in practice means a duplicate). A fresh candidate is always
//!   inserted, so the limit normally never stops the query. Reproduced as written; to be confirmed in the
//!   harness (spec section 10, item 1).
//! * Candidate source. The type and player queries walk the world unit list (CONFIRMED structure, spec
//!   3.5). Here that is the arena order of unit objects, which is creation order (LIKELY). The area queries
//!   use the same list, not the original's spatial cells (UNVERIFIED, spec 3.7 and 10 items 2 and 3). The
//!   candidates are read once before the query starts (UNVERIFIED).
//! * The area test is [`in_range`] and [`in_rect`] (UNVERIFIED: the original's arithmetic and compare
//!   are not decoded, spec 3.7 and 3.8). Replace these two functions when the formula is known.
//! * `DestroyGroup` (LIKELY): the members are released (the set's destructor frees its nodes, which hold
//!   references), then the group's creation reference is released. UNSURE whether the original clears them.
//! * Not implemented: `GroupEnumUnitsSelected` (its own snapshot of the player selection, spec 3.6) and the
//!   order natives. The selection set is not modelled yet.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeCtx, NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{Game, GroupMember, GroupObj, HandleKind, ObjKey, Object, RectObj};
use super::player::raw_at;
use super::trigger::eval_boolexpr;
use super::ScriptRuntime;

/// The candidate query of an enumeration and the test it applies before the collector.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Query {
    /// The world unit list, filtered by unit type and owner (`None` = any). Type and owner are the
    /// collector parameters A and B of the spec (3.2).
    World {
        type_id: Option<u32>,
        player: Option<u32>,
    },
    /// Units inside a circle (`GroupEnumUnitsInRange`, `GroupEnumUnitsInRangeOfLoc`).
    Circle { x: F32, y: F32, radius: F32 },
    /// Units inside a rect (`GroupEnumUnitsInRect`).
    Rect(RectObj),
}

/// Registers the group natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateGroup", move |_, _| {
        let (_, id) = g
            .borrow_mut()
            .add_object(Object::Group(GroupObj::default()), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("DestroyGroup", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = group_at(&gm, a, 0) {
            clear_members(&mut gm, k);
            gm.kill(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupAddUnit", move |_, a| {
        let mut gm = g.borrow_mut();
        let unit = gm.unit_of_handle(handle_arg(a, 1));
        if let (Some(group), Some(unit)) = (group_at(&gm, a, 0), unit) {
            insert(&mut gm, group, unit);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupRemoveUnit", move |_, a| {
        let mut gm = g.borrow_mut();
        let unit = gm.unit_of_handle(handle_arg(a, 1));
        if let (Some(group), Some(unit)) = (group_at(&gm, a, 0), unit) {
            remove(&mut gm, group, unit);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupClear", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(group) = group_at(&gm, a, 0) {
            clear_members(&mut gm, group);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsUnitInGroup", move |_, a| {
        let gm = g.borrow();
        let v = match (gm.unit_of_handle(handle_arg(a, 0)), group_at(&gm, a, 1)) {
            (Some(unit), Some(group)) => gm.group(group).members.iter().any(|m| m.unit == unit),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("FirstOfGroup", move |_, a| {
        let mut gm = g.borrow_mut();
        let first = group_at(&gm, a, 0).and_then(|group| gm.group(group).members.first().copied());
        let id = match first {
            Some(m) => gm.handle_or_create(m.unit),
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("ForGroup", move |ctx, a| {
        let key = ctx.thread_key();
        let target = {
            let gm = g.borrow();
            group_at(&gm, a, 0).zip(code_at(a, 1))
        };
        if let Some((group, code)) = target {
            for_group(&g, ctx, key, group, code);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsOfType", move |ctx, a| {
        let key = ctx.thread_key();
        let name = string_arg(ctx, a, 1);
        enum_of_type(&g, ctx, key, a, &name, 2, 0);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsOfTypeCounted", move |ctx, a| {
        let key = ctx.thread_key();
        let name = string_arg(ctx, a, 1);
        enum_of_type(&g, ctx, key, a, &name, 2, limit_at(a, 3));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsOfPlayer", move |ctx, a| {
        let key = ctx.thread_key();
        enum_of_player(&g, ctx, key, a);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsInRange", move |ctx, a| {
        let key = ctx.thread_key();
        let query = Query::Circle {
            x: real_at(a, 1),
            y: real_at(a, 2),
            radius: real_at(a, 3),
        };
        enum_area(&g, ctx, key, a, query, 4, 0);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsInRangeCounted", move |ctx, a| {
        let key = ctx.thread_key();
        let query = Query::Circle {
            x: real_at(a, 1),
            y: real_at(a, 2),
            radius: real_at(a, 3),
        };
        enum_area(&g, ctx, key, a, query, 4, limit_at(a, 5));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsInRangeOfLoc", move |ctx, a| {
        let key = ctx.thread_key();
        if let Some(query) = circle_of_loc(&g, a) {
            enum_area(&g, ctx, key, a, query, 3, 0);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsInRangeOfLocCounted", move |ctx, a| {
        let key = ctx.thread_key();
        if let Some(query) = circle_of_loc(&g, a) {
            enum_area(&g, ctx, key, a, query, 3, limit_at(a, 4));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsInRect", move |ctx, a| {
        let key = ctx.thread_key();
        if let Some(query) = rect_query(&g, a) {
            enum_area(&g, ctx, key, a, query, 2, 0);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GroupEnumUnitsInRectCounted", move |ctx, a| {
        let key = ctx.thread_key();
        if let Some(query) = rect_query(&g, a) {
            enum_area(&g, ctx, key, a, query, 2, limit_at(a, 3));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetEnumUnit", move |_, _| {
        let id = g.borrow().enum_unit();
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetFilterUnit", move |_, _| {
        let id = g.borrow().filter_unit();
        NativeResult::Return(Value::Handle(id))
    });
}

/// `GroupEnumUnitsOfType` (and the counted form). The name must resolve to a registered type, else the
/// group is left as it is. `filter_arg` is the index of the boolexpr argument.
fn enum_of_type(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    key: u32,
    a: &[Value],
    name: &str,
    filter_arg: usize,
    limit: u32,
) {
    let target = {
        let gm = game.borrow();
        let type_id = gm.unit_types().id_by_name(name);
        group_at(&gm, a, 0).zip(type_id)
    };
    if let Some((group, type_id)) = target {
        let query = Query::World {
            type_id: Some(type_id),
            player: None,
        };
        run_enum(
            game,
            rt,
            key,
            group,
            query,
            boolexpr_at(game, a, filter_arg),
            limit,
        );
    }
}

/// `GroupEnumUnitsOfPlayer`: the player must be a live player handle, else the group is left as it is.
fn enum_of_player(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, key: u32, a: &[Value]) {
    let target = {
        let gm = game.borrow();
        let player = gm.player_index_of_handle(handle_arg(a, 1));
        group_at(&gm, a, 0).zip(player)
    };
    if let Some((group, player)) = target {
        let query = Query::World {
            type_id: None,
            player: Some(player),
        };
        run_enum(game, rt, key, group, query, boolexpr_at(game, a, 2), 0);
    }
}

/// The area natives (`InRange`, `InRangeOfLoc`, `InRect` and their counted forms). `filter_arg` is the
/// index of the boolexpr argument. The group is cleared and enumerated as for the other natives.
fn enum_area(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    key: u32,
    a: &[Value],
    query: Query,
    filter_arg: usize,
    limit: u32,
) {
    let group = group_at(&game.borrow(), a, 0);
    if let Some(group) = group {
        run_enum(
            game,
            rt,
            key,
            group,
            query,
            boolexpr_at(game, a, filter_arg),
            limit,
        );
    }
}

/// The circle of `GroupEnumUnitsInRangeOfLoc`: the centre is the location's coordinates. `None` when the
/// location does not resolve.
fn circle_of_loc(game: &RefCell<Game>, a: &[Value]) -> Option<Query> {
    let gm = game.borrow();
    let k = gm.handle_of_kind(handle_arg(a, 1), HandleKind::Location)?;
    let Object::Location(loc) = gm.object(k) else {
        return None;
    };
    Some(Query::Circle {
        x: loc.x,
        y: loc.y,
        radius: real_at(a, 2),
    })
}

/// The rect of `GroupEnumUnitsInRect`. `None` when the rect does not resolve.
fn rect_query(game: &RefCell<Game>, a: &[Value]) -> Option<Query> {
    let gm = game.borrow();
    let k = gm.handle_of_kind(handle_arg(a, 1), HandleKind::Rect)?;
    match gm.object(k) {
        Object::Rect(r) => Some(Query::Rect(*r)),
        _ => None,
    }
}

/// The shared enumeration core (spec 3.1 and 3.3). The group is cleared, its query state is stored, and
/// the collector runs for each candidate. Script code (the filter) runs without a borrow of the game.
fn run_enum(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    key: u32,
    group: ObjKey,
    query: Query,
    filter: Option<ObjKey>,
    limit: u32,
) {
    let candidates = {
        let mut gm = game.borrow_mut();
        clear_members(&mut gm, group);
        let (type_id, player) = match query {
            Query::World { type_id, player } => (type_id, player),
            Query::Circle { .. } | Query::Rect(_) => (None, None),
        };
        let state = gm.group_mut(group);
        state.filter = filter;
        state.type_id = type_id;
        state.player = player;
        state.limit = limit;
        state.counter = 0;
        world_units(&gm)
    };
    for unit in candidates {
        // Candidate level (spec 3.5 and 3.7): removed units are skipped, and the area test applies.
        let candidate = {
            let gm = game.borrow();
            let u = gm.unit(unit);
            !u.removed && in_query(u.x, u.y, &query)
        };
        if !candidate {
            continue;
        }
        // Collector (spec 3.3). The limit is read from the group for each candidate.
        let (limit_hit, type_owner_ok, filter) = {
            let gm = game.borrow();
            let state = gm.group(group);
            let u = gm.unit(unit);
            let limit_hit = state.limit != 0 && state.limit <= state.counter;
            let type_owner_ok = state.type_id.is_none_or(|t| t == u.type_id)
                && state.player.is_none_or(|p| p == u.owner);
            (limit_hit, type_owner_ok, state.filter)
        };
        if limit_hit {
            break;
        }
        if !type_owner_ok {
            continue;
        }
        let id = game.borrow_mut().handle_or_create(unit);
        game.borrow_mut().push_filter_unit(id);
        let passed = match filter {
            None => true,
            Some(b) => eval_boolexpr(game, rt, b, key),
        };
        game.borrow_mut().pop_filter_unit();
        if !passed {
            continue;
        }
        let mut gm = game.borrow_mut();
        if !insert(&mut gm, group, unit) {
            let state = gm.group_mut(group);
            state.counter = state.counter.wrapping_add(1);
        }
    }
}

/// `ForGroup` (spec 4.1 and 4.2): runs `code` for each member, from a snapshot taken first.
fn for_group(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, key: u32, group: ObjKey, code: u32) {
    let snapshot: Vec<GroupMember> = game.borrow().group(group).members.clone();
    {
        let mut gm = game.borrow_mut();
        for m in &snapshot {
            gm.add_ref(m.id);
        }
    }
    for m in &snapshot {
        let live = !game.borrow().unit(m.unit).removed;
        if !live {
            continue;
        }
        game.borrow_mut().push_enum_unit(m.id);
        let _ = rt.call_keyed(code, key);
        game.borrow_mut().pop_enum_unit();
    }
    let mut gm = game.borrow_mut();
    for m in &snapshot {
        gm.release(m.id);
    }
}

/// Adds `unit` to `group` (spec 2.3). Returns `false` when it is already a member. The group takes a
/// reference on the unit's handle id.
fn insert(gm: &mut Game, group: ObjKey, unit: ObjKey) -> bool {
    if gm.group(group).members.iter().any(|m| m.unit == unit) {
        return false;
    }
    let id = gm.handle_or_create(unit);
    gm.add_ref(id);
    gm.group_mut(group).members.push(GroupMember { unit, id });
    true
}

/// Removes `unit` from `group` and releases the group's reference (spec 2.4). Returns whether it was a
/// member.
fn remove(gm: &mut Game, group: ObjKey, unit: ObjKey) -> bool {
    let members = &mut gm.group_mut(group).members;
    let Some(i) = members.iter().position(|m| m.unit == unit) else {
        return false;
    };
    let m = members.remove(i);
    gm.release(m.id);
    true
}

/// Empties `group` and releases its references in list order (spec 2.5).
fn clear_members(gm: &mut Game, group: ObjKey) {
    let members = std::mem::take(&mut gm.group_mut(group).members);
    for m in members {
        gm.release(m.id);
    }
}

/// The candidates of the world unit list, in arena (creation) order (UNVERIFIED for the order of
/// the original's list, spec 10 item 4).
fn world_units(gm: &Game) -> Vec<ObjKey> {
    (0..gm.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(gm.object(k), Object::Unit(_)))
        .collect()
}

/// Whether a unit at `(x, y)` passes the spatial part of `query` (always true for the world query).
fn in_query(x: F32, y: F32, query: &Query) -> bool {
    match query {
        Query::World { .. } => true,
        Query::Circle {
            x: cx,
            y: cy,
            radius,
        } => in_range(x, y, *cx, *cy, *radius),
        Query::Rect(r) => in_rect(x, y, r),
    }
}

/// The range test of the circle queries. UNVERIFIED: the original's formula is not decoded (spec 3.7 and
/// 3.8). Here: squared distance <= squared radius, computed with softfloat in this order.
fn in_range(x: F32, y: F32, cx: F32, cy: F32, radius: F32) -> bool {
    let dx = arith::sub(x, cx);
    let dy = arith::sub(y, cy);
    let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
    arith::le(d2, arith::mul(radius, radius))
}

/// The bounds test of the rect query. UNVERIFIED (spec 3.7): the edges are inclusive here.
fn in_rect(x: F32, y: F32, r: &RectObj) -> bool {
    arith::le(r.min_x, x) && arith::le(x, r.max_x) && arith::le(r.min_y, y) && arith::le(y, r.max_y)
}

/// The group in handle argument `i`, if it is a live group.
fn group_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.handle_of_kind(handle_arg(a, i), HandleKind::Group)
}

/// The boolexpr in argument `i`, if it is a live boolexpr. A null or invalid filter means no filter (the
/// same convention as `force.rs`).
fn boolexpr_at(game: &RefCell<Game>, a: &[Value], i: usize) -> Option<ObjKey> {
    let gm = game.borrow();
    gm.handle_of_kind(a.get(i)?.as_handle()?, HandleKind::BoolExpr)
}

/// The count of a `Counted` native (an integer argument read as unsigned, so a negative count never
/// stops the query, as in the original's unsigned compare).
fn limit_at(a: &[Value], i: usize) -> u32 {
    raw_at(a, i) as u32
}

/// The handle id in argument `i`, or 0 (the null handle) when there is none.
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

/// The real in argument `i`, or 0.0.
fn real_at(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

/// The code in argument `i`, if it is a function value.
fn code_at(a: &[Value], i: usize) -> Option<u32> {
    match a.get(i)? {
        Value::Code(f) => Some(*f),
        _ => None,
    }
}

/// The string in argument `i` as text, or the empty string.
fn string_arg(ctx: &NativeCtx<'_>, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(id)) => ctx.string(*id).to_string(),
        _ => String::new(),
    }
}
