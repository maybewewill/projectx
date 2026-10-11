//! Regions: `CreateRegion`, `RemoveRegion`, `RegionAddRect` / `RegionClearRect`, `RegionAddCell` /
//! `RegionClearCell` (and the `...AtLoc` forms), `IsUnitInRegion`, `IsPointInRegion`,
//! `IsLocationInRegion`, `TriggerRegisterEnterRegion` / `TriggerRegisterLeaveRegion`, `GetEnteringUnit`,
//! `GetLeavingUnit` and `GetTriggeringRegion`.
//!
//! Addresses are the 1.26a `Game.dll` functions. Labels: CONFIRMED = read in the disassembly (and in
//! Unicorn where stated), LIKELY = strong reading, UNSURE = open. Arithmetic goes through `softfloat` in
//! the operand order of the original. Decompiled code is not copied here; only the behaviour is.
//!
//! ## Representation
//! * A region is a set of cells of the 32-unit grid that the pathing and group indexes use (CONFIRMED:
//!   the 1/32 scale is the exponent edit of `grid::scale_by_1_32`, the same as the pathing code). The grid
//!   index of cell `(x, y)` is `y * width + x` (FUN_6F49FE50 / FUN_6F49FCD0, CONFIRMED structure).
//! * Membership is stored as one occupant entry per cell and region key (region+0x20). An "add" entry
//!   (kind 1) makes the cell a member and a "clear" entry (kind 0) removes it. The newest entry of the
//!   region in that cell decides, because the walk visits the head first and a clear stamps the region key
//!   so an older add in the same walk is skipped (CONFIRMED structure, LIKELY result). This model keeps the
//!   answers only: add inserts a cell, clear removes it.
//!
//! ## Point to cell (CONFIRMED)
//! * `RegionAddCell`, `RegionClearCell` and `IsPointInRegion` take their x and y as pointers to floats
//!   (the JASS `real` arguments). The first coordinate is shifted by the world field at `+0x6C`, the second
//!   by `+0x70` (FUN_6F47CFB0 and FUN_6F47D080: `sub` through the softfloat helper `FUN_6F6EEEF0`). Each
//!   difference is scaled by 1/32 (exponent edit, values below 2^-121 flush to +0, negative values keep
//!   their sign), floored (`FUN_6F6EF170`) and converted with f2i (`FUN_6F6EEC20`). That is
//!   [`cell_coordinate`]: `f2i(floor(scale(v - origin)))`.
//! * Which playable field is `+0x6C` is LIKELY the playable minimum x (the same field the rect clamp
//!   uses), so the origin here is `(min_x, min_y)` of the playable area, or `(0, 0)` when none is set
//!   (UNSURE: the map is not loaded yet).
//! * Earlier notes (groups-events-2.md section 1.2) put the x origin at `+0x70`. The disassembly says
//!   `+0x6C` for x and `+0x70` for y; section 2.1 of that spec already had this pairing.
//!
//! ## Rect to cells (CONFIRMED by disassembly and by Unicorn)
//! * `RegionAddRect` / `RegionClearRect` (FUN_6F47CBA0) read the four edges in rect order (min y at
//!   `+0x24`, min x at `+0x28`, max y at `+0x2C`, max x at `+0x30`), scale each as above, and build a box
//!   with FUN_6F47BA20. That box is `[floor(min y), floor(min x), floor(max y) + 1, floor(max x) + 1)`, in
//!   the order `(y0, x0, y1, x1)` with the ends exclusive. Unicorn ran FUN_6F47BA20 on 120 scaled inputs
//!   (values from -7.5 to 64, including -0.0, 0.999999 and -1e-6, each edge varied); every output matched
//!   [`box_from_scaled`]. This confirms the floor, the `+1` on the two max cells, and the argument order.
//! * So the region covers every cell from the min-edge cell up to the cell that **contains** the max edge,
//!   both included. A max edge exactly on a boundary still includes the cell that starts there: a rect with
//!   max x = 64 includes cell 2 (`[64, 96)` with origin 0). Min x = 32 starts at cell 1.
//! * The original clips the box to the grid bounds (`+0x54..+0x60`). Cells below 0 are dropped here (the
//!   point test compares unsigned, so a negative cell can never be a member). The upper clip is not
//!   modelled (see the open issues).
//! * `Rect` already clamps its corners to the playable area (`location.rs`), so with a playable area set
//!   the box lies inside it.
//!
//! ## Membership tests (CONFIRMED structure)
//! * `IsPointInRegion(region, x, y)`: the cell of `(x, y)` is a member. `IsLocationInRegion` does the same
//!   with the location's x and y (`+0x24`, `+0x28`).
//! * `IsUnitInRegion(region, unit)`: the cell of the unit's own position (the mover at unit+0x164 is what
//!   the original reads; LIKELY equal to `GetUnitX` / `GetUnitY`).
//! * An invalid region, unit or location gives false.
//!
//! ## Handles (CONFIRMED)
//! * `CreateRegion` makes an object with handle flag 0 (`FUN_6F430C80(_, 0)`), and the id is allocated last.
//!   `RemoveRegion` releases the creation reference through the object's death call (vtable `+0x5C`). LIKELY
//!   the cells are dropped with it; here they are cleared so a removed region cannot match.
//!
//! ## Enter and leave (CONFIRMED structure, timing LIKELY)
//! * `TriggerRegisterEnterRegion` (FUN_6F3D2700) and `TriggerRegisterLeaveRegion` (FUN_6F3D2800) need a
//!   trigger and a region. The boolexpr may be null. An invalid boolexpr is treated as null (LIKELY). They
//!   register key `0x80205` (enter) or `0x80206` (leave) on the region's observer table with an event
//!   object (handle flag 1). The rules of unit observers apply: re-registering the same key and trigger
//!   keeps the older position and takes the new filter and event (unit-event spec 7.2).
//! * Detection (LIKELY): the original's handler `FUN_6F43BF20` runs when a unit's cell changes across the
//!   region edge. It pushes `GetFilterUnit` = the unit, evaluates the registration's filter boolexpr at
//!   detection time, and queues the event only when the filter passes (CONFIRMED structure). The queued
//!   events are drained later by the handler for event `0x80275` (FUN_6F43CA90), which pushes the unit as
//!   the trigger unit and fires key `0x80205` or `0x80206` (CONFIRMED structure). The queue is the global
//!   deferred-call pool `DAT_6FAAE470` (UNSURE when it runs).
//! * Here everything runs inside the native that moved the unit: first the filters of every candidate (in
//!   order), then the queued triggers. Each trigger's conditions run at once (`fire_with_unit`), and its
//!   actions start as a chain that the VM runs after the native returns.
//! * Which natives detect (LIKELY): `SetUnitPosition` and `SetUnitPositionLoc` run the full move path in the
//!   original (unit-core spec 4.3n), while `SetUnitX` and `SetUnitY` write the position directly (same spec).
//!   So only the first two detect here. `CreateUnit` and `RemoveUnit` do not detect (UNSURE).
//! * Order (LIKELY for the observer rule, UNSURE across regions): regions in creation order, then each
//!   region's registrations oldest first. Triggers on the same region fire in registration order.
//! * A removed region has no members, so it never fires.
//!
//! ## Event ids and getters (CONFIRMED from the getters)
//! * `GetEnteringUnit` (FUN_6F3BB6D0) answers for ids 5 and `0x3D`. **`0x3D` is not a region event.** Its
//!   registration native is `TriggerRegisterUnitInRange` (FUN_6F3D25F0, key `0x8023D`, see
//!   `FUN_6F4336D0`). The spec's note that 0x3D is an alternative region enter is wrong. Range events are
//!   not implemented here.
//! * `GetLeavingUnit` (FUN_6F3BB6F0) answers for id 6.
//! * `GetTriggeringRegion` (FUN_6F3C3D50) answers for ids 5 and 6 and returns the region that the event
//!   carries in its slot 4 (FUN_6F3C1D40 reads slot 4, CONFIRMED).
//!
//! ## Open issues
//! * The map grid bounds (`+0x54..+0x60`) and the cell upper bound are not known yet. Cells at or beyond
//!   [`CELL_LIMIT`] are not stored, which is a placeholder that keeps the box loops bounded.
//! * The origin when no playable area is set is `(0, 0)` (UNSURE).
//! * Per-tick mover moves should run the same detection (LIKELY), but the simulation has no mover yet.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, conv, math, F32};

use super::game::{
    Ctx, EventSource, Game, HandleKind, LocationObj, ObjKey, Object, Observer, RectObj, RegionObj,
};
use super::hashtable::real_at;
use super::trigger::fire_with_unit;
use super::unit_event::{self, filter_passes, KEY_BASE};
use super::ScriptRuntime;
use crate::pathing::grid::{cell_coordinate, scale_by_1_32};

/// The event id of region enter (`0x80205` = `KEY_BASE` + 5).
pub const EVENT_ENTER: u32 = 5;
/// The event id of region leave (`0x80206` = `KEY_BASE` + 6).
pub const EVENT_LEAVE: u32 = 6;
/// The event id that `GetEnteringUnit` also accepts (`TriggerRegisterUnitInRange`, not a region event).
pub const EVENT_UNIT_IN_RANGE: u32 = 0x3D;

/// The enter registration key.
const KEY_ENTER: u32 = KEY_BASE + EVENT_ENTER;
/// The leave registration key.
const KEY_LEAVE: u32 = KEY_BASE + EVENT_LEAVE;

/// Cells at or beyond this (per axis) are not stored. A placeholder for the map's grid bounds (open issue).
pub const CELL_LIMIT: i64 = 4096;

/// Registers the region natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateRegion", move |_, _| {
        let (_, id) = g
            .borrow_mut()
            .add_object(Object::Region(RegionObj::default()), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("RemoveRegion", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = region_of(&gm, a, 0) {
            region_mut(&mut gm, k).cells.clear();
            gm.kill(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RegionAddRect", move |_, a| {
        set_rect(&g, a, true);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RegionClearRect", move |_, a| {
        set_rect(&g, a, false);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RegionAddCell", move |_, a| {
        set_point(&g, a, true);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RegionClearCell", move |_, a| {
        set_point(&g, a, false);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RegionAddCellAtLoc", move |_, a| {
        set_loc(&g, a, true);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RegionClearCellAtLoc", move |_, a| {
        set_loc(&g, a, false);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsUnitInRegion", move |_, a| {
        let gm = g.borrow();
        let inside = match (region_of(&gm, a, 0), unit_at(&gm, a, 1)) {
            (Some(r), Some(u)) => {
                let (x, y) = (gm.unit(u).x, gm.unit(u).y);
                is_member(&gm, r, cell_at(origin(&gm), x, y))
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(inside))
    });

    let g = Rc::clone(game);
    reg.register("IsPointInRegion", move |_, a| {
        let gm = g.borrow();
        let inside = region_of(&gm, a, 0)
            .is_some_and(|r| is_member(&gm, r, cell_at(origin(&gm), real_at(a, 1), real_at(a, 2))));
        NativeResult::Return(Value::Boolean(inside))
    });

    let g = Rc::clone(game);
    reg.register("IsLocationInRegion", move |_, a| {
        let gm = g.borrow();
        let inside = match (region_of(&gm, a, 0), loc_at(&gm, a, 1)) {
            (Some(r), Some(l)) => is_member(&gm, r, cell_at(origin(&gm), l.x, l.y)),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(inside))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterEnterRegion", move |_, a| {
        let id = register_region_event(&mut g.borrow_mut(), a, EVENT_ENTER);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterLeaveRegion", move |_, a| {
        let id = register_region_event(&mut g.borrow_mut(), a, EVENT_LEAVE);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetEnteringUnit", move |ctx, _| {
        let gm = g.borrow();
        let thread = ctx.thread_key();
        let id = match unit_event::current_event_id(&gm, thread) {
            Some(EVENT_ENTER | EVENT_UNIT_IN_RANGE) => unit_event::trigger_unit_handle(&gm, thread),
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetLeavingUnit", move |ctx, _| {
        let gm = g.borrow();
        let thread = ctx.thread_key();
        let id = match unit_event::current_event_id(&gm, thread) {
            Some(EVENT_LEAVE) => unit_event::trigger_unit_handle(&gm, thread),
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetTriggeringRegion", move |ctx, _| {
        let gm = g.borrow();
        let thread = ctx.thread_key();
        let id = match unit_event::current_event_id(&gm, thread) {
            Some(EVENT_ENTER | EVENT_LEAVE) => {
                current_region(&gm, thread).map_or(0, |r| gm.handle_of(r))
            }
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });
}

/// Runs the enter or leave registrations of the cell change of `unit`, which moved from `from` to its
/// current position (see the module notes, "Enter and leave"). Called by the position natives after their
/// borrow of the game is released.
pub(crate) fn unit_moved(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    from: (F32, F32),
) {
    super::event::unit_range_moved(game, rt, unit, from);
    // Phase 1, detection: every live region whose membership changed between the two cells, its matching
    // registrations in order. Nothing runs here.
    let candidates: Vec<Observer> = {
        let gm = game.borrow();
        let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
        let o = origin(&gm);
        let before = cell_at(o, from.0, from.1);
        let after = cell_at(o, x, y);
        if before == after {
            return;
        }
        let mut found = Vec::new();
        for agent in gm.observer_agents() {
            if !gm.is_alive(agent) {
                continue;
            }
            let Object::Region(r) = gm.object(agent) else {
                continue;
            };
            let (was, now) = (r.cells.contains(&before), r.cells.contains(&after));
            if was == now {
                continue;
            }
            let key = if now { KEY_ENTER } else { KEY_LEAVE };
            found.extend(gm.observers(agent).iter().filter(|n| n.key == key).copied());
        }
        found
    };
    // Phase 2, the filters (GetFilterUnit = the unit). A registration without a filter always passes.
    let mut queued = Vec::new();
    for node in candidates {
        let passes = match node.filter {
            None => true,
            Some(b) => filter_passes(game, rt, b, node, unit),
        };
        if passes {
            queued.push(node);
        }
    }
    // Phase 3, the triggers (oldest registration first, in region order).
    for node in queued {
        fire_with_unit(game, rt, node.trigger, Some(node.event), Some(unit));
    }
}

/// The playable origin of the cell grid: `(x, y)` = `(min x, min y)`, or `(0, 0)` without a playable area.
fn origin(gm: &Game) -> (F32, F32) {
    match gm.playable() {
        Some(p) => (p.min_x, p.min_y),
        None => (F32::ZERO, F32::ZERO),
    }
}

/// The cell of a world point (CONFIRMED arithmetic, see the module notes). The result can lie outside the
/// grid; membership tests reject it.
pub(crate) fn cell_at(origin: (F32, F32), x: F32, y: F32) -> (i32, i32) {
    (cell_coordinate(x, origin.0), cell_coordinate(y, origin.1))
}

/// One edge after the shift and the scale: `scale(v - origin)` (the value the box builder floors).
fn scaled_edge(v: F32, origin: F32) -> F32 {
    scale_by_1_32(arith::sub(v, origin))
}

/// The box builder (FUN_6F47BA20, CONFIRMED with Unicorn): from four scaled edges in the order
/// `(min y, min x, max y, max x)` returns `[floor(a0), floor(a1), floor(a2) + 1, floor(a3) + 1]`, that is
/// `(y0, x0, y1, x1)` with the ends exclusive, so the max cells are included.
pub(crate) fn box_from_scaled(a: [F32; 4]) -> [i64; 4] {
    let cell = |v: F32| i64::from(conv::f2i(math::floor(v)));
    [cell(a[0]), cell(a[1]), cell(a[2]) + 1, cell(a[3]) + 1]
}

/// The cell box of a rect in the box order `(y0, x0, y1, x1)`, ends exclusive (see [`box_from_scaled`]).
pub(crate) fn rect_box(origin: (F32, F32), r: &RectObj) -> [i64; 4] {
    box_from_scaled([
        scaled_edge(r.min_y, origin.1),
        scaled_edge(r.min_x, origin.0),
        scaled_edge(r.max_y, origin.1),
        scaled_edge(r.max_x, origin.0),
    ])
}

/// Sets (`member = true`) or clears the cells of `[y0, y1) x [x0, x1)` that lie inside the grid.
fn set_box(r: &mut RegionObj, b: [i64; 4], member: bool) {
    let [y0, x0, y1, x1] = b;
    for y in y0.max(0)..y1.min(CELL_LIMIT) {
        for x in x0.max(0)..x1.min(CELL_LIMIT) {
            set_cell(r, (x as i32, y as i32), member);
        }
    }
}

/// Sets or clears one cell, when it lies inside the grid (cells outside it are never members).
fn set_cell(r: &mut RegionObj, cell: (i32, i32), member: bool) {
    let inside = |v: i32| (0..CELL_LIMIT as i32).contains(&v);
    if !(inside(cell.0) && inside(cell.1)) {
        return;
    }
    if member {
        r.cells.insert(cell);
    } else {
        r.cells.remove(&cell);
    }
}

/// Whether `cell` is a member of region `k`.
fn is_member(gm: &Game, k: ObjKey, cell: (i32, i32)) -> bool {
    match gm.object(k) {
        Object::Region(r) => r.cells.contains(&cell),
        _ => false,
    }
}

/// `RegionAddRect` (`member = true`) and `RegionClearRect`: the box of the rect (no clamp; `Rect` already
/// clamped the edges).
fn set_rect(g: &Rc<RefCell<Game>>, a: &[Value], member: bool) {
    let mut gm = g.borrow_mut();
    let (Some(k), Some(rk)) = (region_of(&gm, a, 0), rect_of(&gm, a, 1)) else {
        return;
    };
    let b = rect_box(origin(&gm), &rk);
    set_box(region_mut(&mut gm, k), b, member);
}

/// `RegionAddCell` (`member = true`) and `RegionClearCell` with x and y as reals.
fn set_point(g: &Rc<RefCell<Game>>, a: &[Value], member: bool) {
    let mut gm = g.borrow_mut();
    let Some(k) = region_of(&gm, a, 0) else {
        return;
    };
    let cell = cell_at(origin(&gm), real_at(a, 1), real_at(a, 2));
    set_cell(region_mut(&mut gm, k), cell, member);
}

/// `RegionAddCellAtLoc` and `RegionClearCellAtLoc`.
fn set_loc(g: &Rc<RefCell<Game>>, a: &[Value], member: bool) {
    let mut gm = g.borrow_mut();
    let (Some(k), Some(l)) = (region_of(&gm, a, 0), loc_at(&gm, a, 1)) else {
        return;
    };
    let cell = cell_at(origin(&gm), l.x, l.y);
    set_cell(region_mut(&mut gm, k), cell, member);
}

/// Registers the enter (`event = 5`) or leave (`event = 6`) event of trigger `a[0]` on region `a[1]` with
/// the optional filter `a[2]`. Returns the new event's handle id, or 0 when the trigger or region is not live.
fn register_region_event(gm: &mut Game, a: &[Value], event: u32) -> u32 {
    let trigger = gm.handle_of_kind(handle_arg(a, 0), HandleKind::Trigger);
    let region = gm.handle_of_kind(handle_arg(a, 1), HandleKind::Region);
    let filter = match handle_arg(a, 2) {
        0 => None,
        id => gm.handle_of_kind(id, HandleKind::BoolExpr),
    };
    let (Some(t), Some(r)) = (trigger, region) else {
        return 0;
    };
    let key = KEY_BASE + event;
    let source = EventSource::RegionEvent { key, region: r };
    unit_event::register_observer(gm, r, key, t, filter, source)
}

/// The region of the running region event, if the thread runs one.
fn current_region(gm: &Game, thread: u32) -> Option<ObjKey> {
    let Some(Ctx::Trigger { event: Some(e), .. }) = gm.context(thread) else {
        return None;
    };
    match gm.object(e) {
        Object::Event(ev) => match ev.source {
            EventSource::RegionEvent { region, .. } => Some(region),
            _ => None,
        },
        _ => None,
    }
}

/// The region in argument `i`, if it is a live region.
fn region_of(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.handle_of_kind(handle_arg(a, i), HandleKind::Region)
}

fn region_mut(gm: &mut Game, k: ObjKey) -> &mut RegionObj {
    match gm.object_mut(k) {
        Object::Region(r) => r,
        _ => unreachable!("region_mut on a non-region key"),
    }
}

fn unit_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.unit_of_handle(handle_arg(a, i))
}

fn loc_at(gm: &Game, a: &[Value], i: usize) -> Option<LocationObj> {
    let k = gm.handle_of_kind(handle_arg(a, i), HandleKind::Location)?;
    match gm.object(k) {
        Object::Location(l) => Some(*l),
        _ => None,
    }
}

fn rect_of(gm: &Game, a: &[Value], i: usize) -> Option<RectObj> {
    let k = gm.handle_of_kind(handle_arg(a, i), HandleKind::Rect)?;
    match gm.object(k) {
        Object::Rect(r) => Some(*r),
        _ => None,
    }
}

/// The handle id in argument `i`, or 0 (the null handle) when there is none.
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    /// Rows recorded in Unicorn from FUN_6F47BA20: `[a0, a1, a2, a3]` (scaled edges) and the box the
    /// original returned. The probe scripts live in the scratchpad, not in the repo.
    #[test]
    fn box_builder_matches_the_original() {
        let rows: [([f32; 4], [i64; 4]); 12] = [
            ([0.0, -2.0, 0.0, 1.0], [0, -2, 1, 2]),
            ([-0.0, -2.0, 1.0, 1.0], [0, -2, 2, 2]),
            ([0.999_999, -2.0, 0.0, 1.0], [0, -2, 1, 2]),
            ([1.0, -2.0, 0.0, 1.0], [1, -2, 1, 2]),
            ([-0.5, -2.0, 0.0, 1.0], [-1, -2, 1, 2]),
            ([-0.000_001, -2.0, 1.0, 1.0], [-1, -2, 2, 2]),
            ([63.99, -2.0, 0.0, 1.0], [63, -2, 1, 2]),
            ([64.0, -2.0, 0.5, 1.0], [64, -2, 1, 2]),
            ([3.25, -2.0, 1.5, 1.0], [3, -2, 2, 2]),
            ([-7.5, -2.0, 0.0, 1.0], [-8, -2, 1, 2]),
            ([0.5, -2.0, 0.5, 2.5], [0, -2, 1, 3]),
            ([0.5, 2.5, 0.5, -0.5], [0, 2, 1, 0]),
        ];
        for (a, want) in rows {
            assert_eq!(box_from_scaled(a.map(f)), want, "edges {a:?}");
        }
    }

    #[test]
    fn point_cells_at_the_exact_boundaries() {
        let o = (F32::ZERO, F32::ZERO);
        // 32.0 is the first unit of cell 1; the largest float below it is still cell 0.
        assert_eq!(cell_at(o, f(32.0), f(0.0)), (1, 0));
        assert_eq!(cell_at(o, f(31.999_998), f(0.0)), (0, 0));
        // Negative values floor toward minus infinity: -0.5 is cell -1.
        assert_eq!(cell_at(o, f(-0.5), f(0.0)), (-1, 0));
        // The 1/32 scale flushes a value of magnitude below 2^-121 to +0 (the sign-crossing test), so
        // -1e-37 is cell 0 while -1e-30 (above that bound) floors to -1.
        assert_eq!(cell_at(o, f(-1.0e-37), f(0.0)), (0, 0));
        assert_eq!(cell_at(o, f(-1.0e-30), f(0.0)), (-1, 0));
        // The origin is subtracted first: 132.0 is cell 1 with origin 100.0.
        let p = (f(100.0), f(100.0));
        assert_eq!(cell_at(p, f(132.0), f(131.999_98)), (1, 0));
    }

    #[test]
    fn rect_box_includes_the_cell_of_the_max_edge() {
        let o = (F32::ZERO, F32::ZERO);
        let r = RectObj {
            min_x: f(32.0),
            min_y: f(32.0),
            max_x: f(64.0),
            max_y: f(64.0),
        };
        // Cells 1 and 2 on both axes: the max edge at 64 is the start of cell 2, which is included.
        assert_eq!(rect_box(o, &r), [1, 1, 3, 3]);
    }
}
