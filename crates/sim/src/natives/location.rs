//! Location and rect natives: `Location`, `RemoveLocation`, `MoveLocation`, the `GetLocation*`
//! getters, `Rect`, `RectFromLoc`, `RemoveRect`, `SetRect`, `SetRectFromLoc`, `MoveRectTo`,
//! `MoveRectToLoc`, `GetRectCenter*` and `GetRectMin*` / `GetRectMax*`.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions. Helper bodies were checked by
//! disassembly (capstone) of the same binary, and every float operation goes through `softfloat`
//! in the operand order of the original. Labels as in `hashtable.rs`.
//!
//! * `Location(x, y)` (0x6F3D03B0): a new location with the two reals stored raw. No clamp, no
//!   terrain. Handle flag 1. CONFIRMED.
//! * `MoveLocation` (0x6F3C0DF0): stores x and y raw. CONFIRMED.
//! * `GetLocationX` / `GetLocationY` (0x6F3C0E10 / 0x6F3C0E30): the stored reals. A null or freed
//!   location gives 0.0. CONFIRMED.
//! * `GetLocationZ` (0x6F3C0E50): the original reads the terrain height at (x, y). The terrain is not
//!   loaded, so it returns 0.0 (UNSURE: the value is wrong for any map with a height).
//! * `RemoveLocation` (0x6F3C0DD0): the object's death call (virtual slot 0x5C), so the handle is
//!   released. CONFIRMED for the call, LIKELY for the release.
//! * Rects store four edges: min y at +0x24, min x at +0x28, max y at +0x2C and max x at +0x30
//!   (CONFIRMED from the getters and the setter 0x6F3DB9D0). Rect handles have flag 0 (`Rect` and
//!   `RectFromLoc`, CONFIRMED).
//! * Clamp (0x6F469490, called on each corner pair): the pair (x, y) is clamped into the playable
//!   area. The x value is clamped to [min x, max x] and y to [min y, max y], lower bound first. The
//!   upper bound is `max - C` where C is the float at 0x6FAAE564; that global is never written by the
//!   module, so C = 0.0 and the bound is the max itself (CONFIRMED for the code, LIKELY for C = 0).
//!   The playable area is the world struct's (+0x6C..+0x78) values. It is not loaded by the
//!   simulation yet, so with no playable area set the clamp does nothing (UNSURE, see
//!   `Game::set_playable`).
//! * `Rect(minx, miny, maxx, maxy)` (0x6F3D0A20) and `RectFromLoc` (0x6F3D0BD0): the two corners are
//!   clamped, then the edges are sorted. A null location gives the null rect. CONFIRMED for the
//!   clamp, LIKELY for the sort (the x87 compares of the body were read, not traced).
//! * `SetRect` (0x6F3C1710) and `SetRectFromLoc` (0x6F3C17F0): the same clamp and sort on an existing
//!   rect. A null rect does nothing. CONFIRMED.
//! * Sort (CONFIRMED from `SetRect`): min x = (x2 <= x1) ? x2 : x1, max x = (x1 <= x2) ? x2 : x1, and
//!   likewise for y. The compares are IEEE on the stored values (x87 `fcom`), so a NaN keeps the first
//!   operand.
//! * Center (0x6F3B1FB0, used by `GetRectCenterX/Y` and `MoveRectTo`): center x = (max x + min x) * 0.5
//!   and center y = (max y + min y) * 0.5, with `softfloat` add then mul. CONFIRMED.
//! * `GetRectCenterX/Y` (0x6F3C18E0 / 0x6F3C1910): the center. A null rect gives 0.0. CONFIRMED.
//! * `MoveRectTo(r, x, y)` (0x6F3C19C0 -> 0x6F3B2360): the rect moves so that its center is (x, y).
//!   dx = x - center x and dy = y - center y (softfloat sub), then each edge is moved with a softfloat
//!   add: min y += dy, min x += dx, max y += dy, max x += dx (0x6F3B1F10). No clamp. CONFIRMED.
//! * `MoveRectToLoc` (0x6F3C19F0): `MoveRectTo` to the location's coordinates. A null rect or
//!   location does nothing. CONFIRMED.
//! * `GetRectMin*` / `GetRectMax*` (0x6F3C1940..0x6F3C19A0): the stored edges. A null rect gives 0.0.
//!   CONFIRMED.
//! * `RemoveRect` (0x6F3C16F0): the death call, as for locations. CONFIRMED for the call.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{Game, HandleKind, LocationObj, ObjKey, Object, Playable, RectObj};
use super::hashtable::real_at;

/// The constant 0.5 (bits 0x3F000000), used for the rect center.
const HALF: F32 = F32::from_bits(0x3F00_0000);

/// Registers the location and rect natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("Location", move |_, a| {
        let loc = LocationObj {
            x: real_at(a, 0),
            y: real_at(a, 1),
        };
        let (_, id) = g.borrow_mut().add_object(Object::Location(loc), 1);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("RemoveLocation", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = obj_of(&gm, a, HandleKind::Location) {
            gm.kill(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("MoveLocation", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = obj_of(&gm, a, HandleKind::Location) {
            if let Object::Location(l) = gm.object_mut(k) {
                l.x = real_at(a, 1);
                l.y = real_at(a, 2);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetLocationX", move |_, a| {
        let gm = g.borrow();
        let v = loc_of(&gm, a).map_or(F32::ZERO, |l| l.x);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetLocationY", move |_, a| {
        let gm = g.borrow();
        let v = loc_of(&gm, a).map_or(F32::ZERO, |l| l.y);
        NativeResult::Return(Value::Real(v))
    });

    // UNSURE: the original reads the terrain height here; there is no terrain yet.
    reg.register("GetLocationZ", |_, _| {
        NativeResult::Return(Value::Real(F32::ZERO))
    });

    let g = Rc::clone(game);
    reg.register("Rect", move |_, a| {
        let mut gm = g.borrow_mut();
        let playable = gm.playable();
        let r = make_rect(
            playable,
            (real_at(a, 0), real_at(a, 1)),
            (real_at(a, 2), real_at(a, 3)),
        );
        let (_, id) = gm.add_object(Object::Rect(r), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("RectFromLoc", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(p), Some(q)) = (loc_of(&gm, &a[..1]), loc_of(&gm, &a[1..2])) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let playable = gm.playable();
        let r = make_rect(playable, (p.x, p.y), (q.x, q.y));
        let (_, id) = gm.add_object(Object::Rect(r), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("RemoveRect", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = obj_of(&gm, a, HandleKind::Rect) {
            gm.kill(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetRect", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = obj_of(&gm, a, HandleKind::Rect) {
            let playable = gm.playable();
            let r = make_rect(
                playable,
                (real_at(a, 1), real_at(a, 2)),
                (real_at(a, 3), real_at(a, 4)),
            );
            *rect_mut(&mut gm, k) = r;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetRectFromLoc", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(k) = obj_of(&gm, a, HandleKind::Rect) else {
            return NativeResult::Return(Value::Null);
        };
        if let (Some(p), Some(q)) = (loc_of(&gm, &a[1..2]), loc_of(&gm, &a[2..3])) {
            let playable = gm.playable();
            let r = make_rect(playable, (p.x, p.y), (q.x, q.y));
            *rect_mut(&mut gm, k) = r;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetRectCenterX", move |_, a| {
        let gm = g.borrow();
        let v = rect_of(&gm, a).map_or(F32::ZERO, |r| center(&r).0);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetRectCenterY", move |_, a| {
        let gm = g.borrow();
        let v = rect_of(&gm, a).map_or(F32::ZERO, |r| center(&r).1);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("MoveRectTo", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = obj_of(&gm, a, HandleKind::Rect) {
            move_rect_to(rect_mut(&mut gm, k), real_at(a, 1), real_at(a, 2));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("MoveRectToLoc", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = obj_of(&gm, a, HandleKind::Rect) {
            if let Some(p) = loc_of(&gm, &a[1..2]) {
                move_rect_to(rect_mut(&mut gm, k), p.x, p.y);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetRectMinX", move |_, a| {
        let gm = g.borrow();
        let v = rect_of(&gm, a).map_or(F32::ZERO, |r| r.min_x);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetRectMinY", move |_, a| {
        let gm = g.borrow();
        let v = rect_of(&gm, a).map_or(F32::ZERO, |r| r.min_y);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetRectMaxX", move |_, a| {
        let gm = g.borrow();
        let v = rect_of(&gm, a).map_or(F32::ZERO, |r| r.max_x);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetRectMaxY", move |_, a| {
        let gm = g.borrow();
        let v = rect_of(&gm, a).map_or(F32::ZERO, |r| r.max_y);
        NativeResult::Return(Value::Real(v))
    });
}

/// Builds a rect from two corners: each corner is clamped to the playable area, then the edges are
/// sorted (see the module notes).
fn make_rect(playable: Option<Playable>, p: (F32, F32), q: (F32, F32)) -> RectObj {
    let (x1, y1) = clamp_corner(playable, p.0, p.1);
    let (x2, y2) = clamp_corner(playable, q.0, q.1);
    let (min_x, max_x) = sort_pair(x1, x2);
    let (min_y, max_y) = sort_pair(y1, y2);
    RectObj {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

/// Clamps the corner (x, y) into the playable area (0x6F469490). Without a playable area it is kept.
fn clamp_corner(playable: Option<Playable>, x: F32, y: F32) -> (F32, F32) {
    match playable {
        None => (x, y),
        Some(p) => (
            clamp_axis(x, p.min_x, p.max_x),
            clamp_axis(y, p.min_y, p.max_y),
        ),
    }
}

/// One axis of the clamp. The lower bound is applied first; the upper bound only when the value was
/// not raised. The upper bound is `hi - C` with C = 0.0 (CONFIRMED structure, LIKELY constant).
/// The compares are IEEE, as the original's x87 `fcom`.
fn clamp_axis(v: F32, lo: F32, hi: F32) -> F32 {
    let hi = arith::sub(hi, F32::ZERO);
    if lo.to_f32() > v.to_f32() {
        return lo;
    }
    if hi.to_f32() < v.to_f32() {
        return hi;
    }
    v
}

/// The (min, max) of two values as `SetRect` picks them: `min = (b <= a) ? b : a` and
/// `max = (a <= b) ? b : a`, so a NaN keeps the first operand.
fn sort_pair(a: F32, b: F32) -> (F32, F32) {
    let (fa, fb) = (a.to_f32(), b.to_f32());
    let min = if fb <= fa { b } else { a };
    let max = if fa <= fb { b } else { a };
    (min, max)
}

/// The center of a rect: `(max + min) * 0.5` on each axis, as softfloat add then mul (0x6F3B1FB0).
fn center(r: &RectObj) -> (F32, F32) {
    let cx = arith::mul(arith::add(r.max_x, r.min_x), HALF);
    let cy = arith::mul(arith::add(r.max_y, r.min_y), HALF);
    (cx, cy)
}

/// Moves the rect so that its center is at (x, y), without clamping (0x6F3B2360 and 0x6F3B1F10).
fn move_rect_to(r: &mut RectObj, x: F32, y: F32) {
    let (cx, cy) = center(r);
    let dx = arith::sub(x, cx);
    let dy = arith::sub(y, cy);
    r.min_y = arith::add(r.min_y, dy);
    r.min_x = arith::add(r.min_x, dx);
    r.max_y = arith::add(r.max_y, dy);
    r.max_x = arith::add(r.max_x, dx);
}

/// The object of the handle in argument 0, if it is a live object of `kind`.
fn obj_of(gm: &Game, a: &[Value], kind: HandleKind) -> Option<ObjKey> {
    let id = a.first()?.as_handle()?;
    gm.handle_of_kind(id, kind)
}

/// The location in argument 0 (or in `a` when it is the only argument), if it is live.
fn loc_of(gm: &Game, a: &[Value]) -> Option<LocationObj> {
    let k = obj_of(gm, a, HandleKind::Location)?;
    match gm.object(k) {
        Object::Location(l) => Some(*l),
        _ => None,
    }
}

fn rect_of(gm: &Game, a: &[Value]) -> Option<RectObj> {
    let k = obj_of(gm, a, HandleKind::Rect)?;
    match gm.object(k) {
        Object::Rect(r) => Some(*r),
        _ => None,
    }
}

fn rect_mut(gm: &mut Game, k: ObjKey) -> &mut RectObj {
    match gm.object_mut(k) {
        Object::Rect(r) => r,
        _ => unreachable!("rect_mut on a non-rect key"),
    }
}
