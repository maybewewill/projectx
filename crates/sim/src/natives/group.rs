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
//!   3.5), in arena order, which is creation order (LIKELY). The area queries walk a spatial index of
//!   32-unit cells (CONFIRMED order, spec 1.3 and 1.5): cells x-major with y ascending, and each cell's
//!   chain from its head. The chain order is UNSURE ([`LINK_AT_HEAD`], newest first, spec open item 2). The
//!   index is rebuilt from the unit arena for each query (UNVERIFIED: the original keeps its entries
//!   linked as units move). The box of the walk is the circle's bounding box or the rect (UNSURE, spec 1.6).
//!   The candidates are read once before the query starts (UNVERIFIED).
//! * Dedupe (spec 1.4): a per-query stamp on each unit ([`CellIndex::walk`]) so a unit is a candidate at
//!   most once per query. Each unit is linked into the cell of its position only (UNVERIFIED: the original
//!   may link a unit into every cell it spans).
//! * The circle test is [`in_range`]: `D <= V` with cell units, CONFIRMED on the harness (spec 2.1). The
//!   origin pairing is UNSURE (see [`in_range`]). The rect test is [`in_rect`]: an UNVERIFIED placeholder
//!   (spec 3.7: the rect path is the box mode and may not test units one by one).
//! * `DestroyGroup` (LIKELY): the members are released (the set's destructor frees its nodes, which hold
//!   references), then the group's creation reference is released. UNSURE whether the original clears them.
//! * Not implemented: `GroupEnumUnitsSelected` (its own snapshot of the player selection, spec 3.6) and the
//!   order natives. The selection set is not modelled yet.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use jass::vm::{NativeCtx, NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{Game, GroupMember, GroupObj, HandleKind, ObjKey, Object, RectObj};
use super::player::raw_at;
use super::trigger::eval_boolexpr;
use super::ScriptRuntime;
use crate::pathing::{cell_coordinate, scale_by_1_32};

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
        candidates_of(&gm, &query)
    };
    for unit in candidates {
        // Candidate level (spec 3.5 and 3.7): removed units are skipped, and the area test applies.
        let candidate = {
            let gm = game.borrow();
            let u = gm.unit(unit);
            !u.removed && in_query(u.x, u.y, &query, grid_origin(&gm))
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

/// Link order of a unit into its cell chain (UNSURE, spec 1.5 and open item 2). `true`: a new entry
/// goes to the head, so a chain runs newest first (the spec's guess). `false`: creation order. Only
/// [`CellIndex::link`] reads this.
const LINK_AT_HEAD: bool = true;

/// The spatial index of the area queries (spec 1.2 to 1.5). Units are bucketed by their cell, which is
/// `(x - origin) / 32` floored, one key per (cell x, cell y). A `BTreeMap` keeps the keys ordered by x
/// and then y, so a range walk visits the cells x-major, then y ascending (spec 1.3, CONFIRMED).
///
/// The index is built from the unit arena for each query (UNVERIFIED for the original's maintenance:
/// it keeps its own entries linked as units move). A unit goes into the cell of its own position only;
/// the original may link a unit into every cell it spans (spec 1.4), which the stamp below handles.
struct CellIndex {
    cells: BTreeMap<(i32, i32), VecDeque<ObjKey>>,
}

impl CellIndex {
    /// Links every unit of the arena into the cell of its position, in creation order.
    fn build(gm: &Game, origin: (F32, F32)) -> Self {
        let mut index = CellIndex {
            cells: BTreeMap::new(),
        };
        for unit in world_units(gm) {
            let u = gm.unit(unit);
            let cell = (
                cell_coordinate(u.x, origin.0),
                cell_coordinate(u.y, origin.1),
            );
            index.link(cell, unit);
        }
        index
    }

    /// Adds `unit` to the chain of `cell` at the head or the tail (see [`LINK_AT_HEAD`]).
    fn link(&mut self, cell: (i32, i32), unit: ObjKey) {
        let chain = self.cells.entry(cell).or_default();
        if LINK_AT_HEAD {
            chain.push_front(unit);
        } else {
            chain.push_back(unit);
        }
    }

    /// The units of the cells `x0 <= x < x1` and `y0 <= y < y1`, in walk order: x ascending, then y
    /// ascending, then each chain from its head. A unit already seen in this walk is skipped: `stamps`
    /// holds, per unit, the id of the last walk that visited it, and `walk` sets it to `walk_id`.
    fn walk(&self, bounds: CellBox, stamps: &mut [u32], walk_id: u32) -> Vec<ObjKey> {
        let mut out = Vec::new();
        let (x0, y0, x1, y1) = bounds;
        if x0 >= x1 || y0 >= y1 {
            return out;
        }
        for (_, chain) in self
            .cells
            .range((x0, i32::MIN)..(x1, i32::MIN))
            .filter(|((_, y), _)| *y >= y0 && *y < y1)
        {
            for &unit in chain {
                let slot = &mut stamps[unit.0 as usize];
                if *slot != walk_id {
                    *slot = walk_id;
                    out.push(unit);
                }
            }
        }
        out
    }
}

/// A box of cells as `(x0, y0, x1, y1)`, with the upper bounds exclusive.
type CellBox = (i32, i32, i32, i32);

/// The origin of the group grid: the playable area's minimum corner, or zero before a host sets the
/// playable area (UNSURE: the original keeps the origin in its world object, spec 1.2).
fn grid_origin(gm: &Game) -> (F32, F32) {
    gm.playable()
        .map_or((F32::ZERO, F32::ZERO), |p| (p.min_x, p.min_y))
}

/// The cell box of a world box `[lo_x, hi_x] x [lo_y, hi_y]`: each minimum is floored, and each maximum
/// is floored and then increased by one (spec 1.6, UNSURE beyond the floor rule).
fn cell_box(lo: (F32, F32), hi: (F32, F32), origin: (F32, F32)) -> CellBox {
    (
        cell_coordinate(lo.0, origin.0),
        cell_coordinate(lo.1, origin.1),
        cell_coordinate(hi.0, origin.0).saturating_add(1),
        cell_coordinate(hi.1, origin.1).saturating_add(1),
    )
}

/// The candidates of a query, in the order the collector sees them. The type and owner queries walk the
/// world unit list in creation order (spec 3.5; UNVERIFIED). The area queries walk the cells of the
/// query box (spec 1.3 and 1.5).
fn candidates_of(gm: &Game, query: &Query) -> Vec<ObjKey> {
    let origin = grid_origin(gm);
    let bounds = match *query {
        Query::World { .. } => return world_units(gm),
        Query::Circle { x, y, radius } => {
            // The box is the circle's bounding box. A negative radius gives the same gate (its
            // square is used), so the box uses the magnitude too (UNSURE).
            let r = F32::from_bits(radius.to_bits() & 0x7FFF_FFFF);
            if r.to_bits() >= 0x4B80_0000 {
                // Beyond 2^24 world units (and for NaN) the cell conversion wraps, so walk every
                // cell (UNSURE: the original's behaviour here is not decoded).
                (i32::MIN, i32::MIN, i32::MAX, i32::MAX)
            } else {
                cell_box(
                    (arith::sub(x, r), arith::sub(y, r)),
                    (arith::add(x, r), arith::add(y, r)),
                    origin,
                )
            }
        }
        Query::Rect(r) => cell_box((r.min_x, r.min_y), (r.max_x, r.max_y), origin),
    };
    let index = CellIndex::build(gm, origin);
    let mut stamps = vec![0u32; gm.object_count()];
    index.walk(bounds, &mut stamps, 1)
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
fn in_query(x: F32, y: F32, query: &Query, origin: (F32, F32)) -> bool {
    match query {
        Query::World { .. } => true,
        Query::Circle {
            x: cx,
            y: cy,
            radius,
        } => in_range(x, y, *cx, *cy, *radius, origin),
        Query::Rect(r) => in_rect(x, y, r),
    }
}

/// A world coordinate in cell units: `(v - origin) / 32`, with the subtract and the exact `/ 32` of the
/// pathing grid (`scale_by_1_32`).
fn cell_float(v: F32, origin: F32) -> F32 {
    scale_by_1_32(arith::sub(v, origin))
}

/// The range test of the circle queries (spec 2.1, CONFIRMED on 900 of 900 harness cases). A unit is
/// inside when `D <= V`: `D` is the squared distance in cell units between the unit's cell point and the
/// centre's cell point, through the fused routine [`arith::dist2`] (the unit's point first), and
/// `V = (r / 32) * (r / 32)` through the multiply core. The bound is inclusive.
///
/// Origins (UNSURE): spec 2.1 subtracts `W+0x6C` from the centre's first coordinate and `W+0x70` from the
/// second, while spec 1.2 names `W+0x70` as the x origin. The port uses one origin per axis (the playable
/// minimum), which is the reading in which a unit at the centre has distance 0.
pub(super) fn in_range(x: F32, y: F32, cx: F32, cy: F32, radius: F32, origin: (F32, F32)) -> bool {
    let d = arith::dist2(
        cell_float(x, origin.0),
        cell_float(y, origin.1),
        cell_float(cx, origin.0),
        cell_float(cy, origin.1),
    );
    let s = scale_by_1_32(radius);
    arith::le(d, arith::mul(s, s))
}

/// The bounds test of the rect query. UNVERIFIED (spec 3.7): the edges are inclusive here, and the
/// original may test only the cells of the box (its rect path is the box mode, not decoded).
pub(super) fn in_rect(x: F32, y: F32, r: &RectObj) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: (F32, F32) = (F32::ZERO, F32::ZERO);

    fn bits(b: u32) -> F32 {
        F32::from_bits(b)
    }

    /// Spec 2.1 boundary, centre (0, 0), radius 500.0. The bit patterns are the floats nearest to
    /// the decimal values (500.0001 is 0x43FA0003).
    #[test]
    fn range_bound_is_inclusive_and_exact() {
        let r = bits(0x43FA_0000); // 500.0
        let z = F32::ZERO;
        // Distance exactly 500.0 is accepted, and so is 499.0.
        assert!(in_range(bits(0x43FA_0000), z, z, z, r, ORIGIN));
        assert!(in_range(bits(0x43F9_8000), z, z, z, r, ORIGIN));
        // 500.0001 is rejected.
        assert!(!in_range(bits(0x43FA_0003), z, z, z, r, ORIGIN));
        // 354 by 354 is at distance 500.63: rejected.
        assert!(!in_range(
            bits(0x43B1_0000),
            bits(0x43B1_0000),
            z,
            z,
            r,
            ORIGIN
        ));
        // The centre itself is inside.
        assert!(in_range(z, z, z, z, r, ORIGIN));
    }

    /// The gate is exact for a centre away from the origin as well (cell units are (v - o) / 32).
    #[test]
    fn range_bound_moves_with_the_centre() {
        let r = bits(0x43FA_0000); // 500.0
        let c = bits(0x43C8_0000); // 400.0
                                   // A unit 500 to the right of the centre (x = 900) is on the boundary: accepted.
        assert!(in_range(bits(0x4461_0000), c, c, c, r, ORIGIN));
        // One float step further (900.0001 rounds to 0x44610003) is outside.
        assert!(!in_range(bits(0x4461_0003), c, c, c, r, ORIGIN));
    }

    /// A unit linked into two cells is walked once per query.
    #[test]
    fn dedupe_stamp_walks_a_unit_once() {
        let mut idx = CellIndex {
            cells: BTreeMap::new(),
        };
        idx.link((0, 0), ObjKey(1));
        idx.link((1, 0), ObjKey(1));
        idx.link((1, 0), ObjKey(2));
        let mut stamps = vec![0u32; 4];
        let walk = idx.walk((0, 0, 2, 1), &mut stamps, 7);
        assert_eq!(walk.len(), 2);
        assert_eq!(walk.iter().filter(|k| k.0 == 1).count(), 1);
        // The same query id sees every unit as already stamped; a new id walks them again.
        assert!(idx.walk((0, 0, 2, 1), &mut stamps, 7).is_empty());
        assert_eq!(idx.walk((0, 0, 2, 1), &mut stamps, 8).len(), 2);
    }

    /// Cells are visited x-major with y ascending, and each chain runs from its head.
    #[test]
    fn walk_order_is_x_major_then_y_then_chain_head_first() {
        let mut idx = CellIndex {
            cells: BTreeMap::new(),
        };
        // Units linked in creation order (1 first).
        idx.link((0, 0), ObjKey(1));
        idx.link((1, 0), ObjKey(2));
        idx.link((0, 1), ObjKey(3));
        idx.link((0, 0), ObjKey(4));
        idx.link((-1, 2), ObjKey(5));
        let mut stamps = vec![0u32; 8];
        let walk = idx.walk((-1, 0, 2, 3), &mut stamps, 1);
        // x = -1: (-1, 2) -> 5. x = 0: y = 0 chain 4 then 1 (newest first), then y = 1 -> 3.
        // x = 1: y = 0 -> 2.
        let expected = if LINK_AT_HEAD {
            vec![ObjKey(5), ObjKey(4), ObjKey(1), ObjKey(3), ObjKey(2)]
        } else {
            vec![ObjKey(5), ObjKey(1), ObjKey(4), ObjKey(3), ObjKey(2)]
        };
        assert_eq!(walk, expected);
    }

    #[test]
    fn walk_skips_cells_outside_the_box_and_empty_boxes() {
        let mut idx = CellIndex {
            cells: BTreeMap::new(),
        };
        idx.link((0, 0), ObjKey(1));
        idx.link((5, 0), ObjKey(2));
        idx.link((0, 9), ObjKey(3));
        let mut stamps = vec![0u32; 4];
        assert_eq!(idx.walk((0, 0, 1, 1), &mut stamps, 1), vec![ObjKey(1)]);
        assert!(idx.walk((2, 0, 2, 5), &mut stamps, 2).is_empty());
        assert!(idx.walk((3, 0, 1, 5), &mut stamps, 3).is_empty());
    }

    /// The box of a circle of radius 500 around the origin covers cells -16 to 15 (exclusive upper 16).
    #[test]
    fn circle_box_covers_the_bounding_square() {
        let lo = (bits(0xC3FA_0000), bits(0xC3FA_0000)); // -500.0
        let hi = (bits(0x43FA_0000), bits(0x43FA_0000)); // 500.0
        assert_eq!(cell_box(lo, hi, ORIGIN), (-16, -16, 16, 16));
    }
}
