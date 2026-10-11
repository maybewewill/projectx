//! Unit natives: creation (`CreateUnit`, `CreateUnitByName`, `CreateUnitAtLoc`, `CreateUnitAtLocByName`),
//! removal and death (`RemoveUnit`, `KillUnit`), position (`GetUnitX/Y`, `SetUnitX/Y`, `SetUnitPosition`,
//! `SetUnitPositionLoc`, `GetUnitLoc`, `GetWidgetX/Y`), facing, owner, type, state (life and mana), the
//! widget life pair, `ShowUnit` / `IsUnitHidden` and `PauseUnit` / `IsUnitPaused`.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions in `docs/spec/unit-core.md`. Labels as
//! in `player.rs`: CONFIRMED = read in the decompile or disassembly, LIKELY = strong inference, UNSURE =
//! guessed or not read.
//!
//! * Handle resolution (CONFIRMED, spec section 2): every native resolves its unit through
//!   [`Game::unit_of_handle`]. A null, freed, wrong-type or removed handle gives the native's default
//!   (0, 0.0, false, or nothing), with no side effect.
//! * Creation order (CONFIRMED, spec sections 3.1 and 3.5). `CreateUnitByName` looks the name up first.
//!   Then the player is checked (no side effect on failure). The type must exist in the registry (the
//!   original's type record lookup fails the whole call, spec 3.2 step 2). Only then is the unit built,
//!   and its handle is allocated last. So a failed creation consumes no handle id.
//!   `CreateUnitAtLoc*` resolves the location first (UNSURE: an invalid location is treated as a failed
//!   creation, the same as a bad player).
//! * Facing (CONFIRMED structure, LIKELY meaning): the degree argument is multiplied by pi/180 with a
//!   softfloat multiply when stored, and the getter multiplies the stored radians by 180/pi. The constants
//!   are the start-up values of the 1.26a image (`0x6FAAE5E8` = `0x3C8EFA35`, `0x6FAAE60C` = `0x42652EE1`,
//!   read in `docs/spec/unit-core.md` section 9 and in AGENTS.md section 4.3m).
//! * Life (CONFIRMED, spec 6.5 and 6.7). `SetLife` clamps: a value `<= 0.0` (including -0.0) is stored as
//!   `+0.0`. Then it fires event `0xD01E6` on the unit. `KillUnit`, `SetWidgetLife` and `SetUnitState`
//!   state 0 all use it. A change to zero of a unit that was alive also records a death
//!   ([`UnitEvent::Death`]) and runs the death trigger dispatch of `unit_event.rs` (player-unit key
//!   `0x80214`, then unit key `0x80235`). The spec leaves open whether the original detects death in the
//!   `0xD01E6` handler (UNSURE).
//! * Mana (CONFIRMED, spec 6.7): `SetUnitState` state 2 stores the value as it is, with no clamp and no
//!   event. States 1 and 3 (max life, max mana) are ignored, and the getters return the stored maxima.
//!   Any other state reads 0.0 and writes nothing.
//! * `KillUnit` never removes the unit and keeps its handle (CONFIRMED).
//! * `RemoveUnit` (CONFIRMED order, spec 6.9): the removed flag is set first, then the removal sequence
//!   events `0xD015A`, `0xD0164`, `0xD0156`, `0xD0178`, `0xD0144` are recorded in that order. Removing a
//!   removed unit does nothing. LIKELY: the creation reference of the handle is then released (the
//!   agent-death step), so the id is freed when its refcount reaches zero: at once when no saved handle
//!   holds it, otherwise when the last holder releases it. This is the one place where the model goes
//!   beyond the spec's own "does not release" note (see the report).
//! * Owner (CONFIRMED structure, spec 6.8). `SetUnitOwner` with the same owner does nothing. A new owner
//!   records [`UnitEvent::OwnerChanged`]. The `changeColor` argument has no effect here (UNSURE). Food
//!   accounting is not modelled (no food state yet).
//! * `ShowUnit` and `PauseUnit` store their flags. The original's show path (list enumeration, spec 6.10)
//!   and the meaning of the pause (spec 6.11) are not modelled yet (UNSURE).
//! * Unit events for triggers (death only) are in `unit_event.rs`. `IsUnitType` is not implemented (its
//!   jump table is not recovered, spec 5).
//! * Position (UNSURE): the stored x and y are the raw values. The original derives them from the mover
//!   and applies a placement adjustment for creation (spec 3.2 step 3, 5.1). `SetUnitPosition` calls the
//!   full move path in the original; here it only stores the values, with no move trigger yet.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{Game, LocationObj, ObjKey, Object, UnitEvent, UnitObj};
use super::player::raw_at;
use super::unit_event;

/// `SetLife`'s event: the life changed (CONFIRMED value `0xD01E6`).
pub const EVENT_LIFE_CHANGED: u32 = 0x000D_01E6;

/// The removal sequence, in the order the original fires them (CONFIRMED, spec 6.9).
const REMOVE_SEQUENCE: [u32; 5] = [
    0x000D_015A,
    0x000D_0164,
    0x000D_0156,
    0x000D_0178,
    0x000D_0144,
];

/// Degrees to radians: `0x6FAAE5E8` = pi/180 = `0x3C8EFA35` (AGENTS.md 4.3m, runtime start-up value).
const DEG_TO_RAD: F32 = F32::from_bits(0x3C8E_FA35);
/// Radians to degrees: `0x6FAAE60C` = 180/pi = `0x42652EE1` (AGENTS.md 4.3m, runtime start-up value).
const RAD_TO_DEG: F32 = F32::from_bits(0x4265_2EE1);

/// `UNIT_STATE_LIFE`.
const STATE_LIFE: u32 = 0;
/// `UNIT_STATE_MAX_LIFE`.
const STATE_MAX_LIFE: u32 = 1;
/// `UNIT_STATE_MANA`.
const STATE_MANA: u32 = 2;
/// `UNIT_STATE_MAX_MANA`.
const STATE_MAX_MANA: u32 = 3;

/// Registers the unit natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateUnit", move |_, a| {
        let id = create(
            &mut g.borrow_mut(),
            handle_arg(a, 0),
            raw_at(a, 1) as u32,
            real_at(a, 2),
            real_at(a, 3),
            real_at(a, 4),
        );
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("CreateUnitByName", move |ctx, a| {
        let name = string_arg(ctx, a, 1);
        let mut gm = g.borrow_mut();
        let id = match gm.unit_types().id_by_name(&name) {
            Some(type_id) => create(
                &mut gm,
                handle_arg(a, 0),
                type_id,
                real_at(a, 2),
                real_at(a, 3),
                real_at(a, 4),
            ),
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("CreateUnitAtLoc", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match loc_at(&gm, a, 2) {
            Some(loc) => create(
                &mut gm,
                handle_arg(a, 0),
                raw_at(a, 1) as u32,
                loc.x,
                loc.y,
                real_at(a, 3),
            ),
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("CreateUnitAtLocByName", move |ctx, a| {
        let name = string_arg(ctx, a, 1);
        let mut gm = g.borrow_mut();
        let id = match (gm.unit_types().id_by_name(&name), loc_at(&gm, a, 2)) {
            (Some(type_id), Some(loc)) => create(
                &mut gm,
                handle_arg(a, 0),
                type_id,
                loc.x,
                loc.y,
                real_at(a, 3),
            ),
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("RemoveUnit", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            remove(&mut gm, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("KillUnit", move |ctx, a| {
        let died = {
            let mut gm = g.borrow_mut();
            unit_at(&gm, a, 0).filter(|&k| set_life(&mut gm, k, F32::ZERO))
        };
        if let Some(k) = died {
            unit_event::dispatch_unit_death(&g, ctx, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitX", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).x);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitY", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).y);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitLoc", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match unit_at(&gm, a, 0) {
            Some(k) => {
                let (x, y) = (gm.unit(k).x, gm.unit(k).y);
                // A location from a getter has flag 1 (see `natives/location.rs`).
                gm.add_object(Object::Location(LocationObj { x, y }), 1).1
            }
            // UNSURE: the original's answer for an invalid unit is not traced; the null handle is used.
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitX", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).x = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitY", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).y = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitPosition", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            set_position(&mut gm, k, real_at(a, 1), real_at(a, 2));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitPositionLoc", move |_, a| {
        let mut gm = g.borrow_mut();
        if let (Some(k), Some(loc)) = (unit_at(&gm, a, 0), loc_at(&gm, a, 1)) {
            set_position(&mut gm, k, loc.x, loc.y);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitFacing", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| arith::mul(gm.unit(k).facing, RAD_TO_DEG));
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitFacing", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).facing = arith::mul(real_at(a, 1), DEG_TO_RAD);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitTypeId", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(0, |k| gm.unit(k).type_id as i32);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetOwningPlayer", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match unit_at(&gm, a, 0) {
            Some(k) => {
                let owner = gm.unit(k).owner;
                gm.player_handle(owner)
            }
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    // `changeColor` is read by nothing here (UNSURE, see the module notes).
    reg.register("SetUnitOwner", move |_, a| {
        let mut gm = g.borrow_mut();
        let new_owner = gm.player_index_of_handle(handle_arg(a, 1));
        if let (Some(k), Some(new_owner)) = (unit_at(&gm, a, 0), new_owner) {
            let from = gm.unit(k).owner;
            if from != new_owner {
                gm.unit_mut(k).owner = new_owner;
                gm.push_unit_event(UnitEvent::OwnerChanged {
                    unit: k,
                    from,
                    to: new_owner,
                });
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitState", move |_, a| {
        let gm = g.borrow();
        let state = raw_at(a, 1) as u32;
        let v = match unit_at(&gm, a, 0) {
            Some(k) => {
                let u = gm.unit(k);
                match state {
                    STATE_LIFE => u.life,
                    STATE_MAX_LIFE => u.max_life,
                    STATE_MANA => u.mana,
                    STATE_MAX_MANA => u.max_mana,
                    _ => F32::ZERO,
                }
            }
            None => F32::ZERO,
        };
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitState", move |ctx, a| {
        let died = {
            let mut gm = g.borrow_mut();
            unit_at(&gm, a, 0)
                .filter(|&k| set_state(&mut gm, k, raw_at(a, 1) as u32, real_at(a, 2)))
        };
        if let Some(k) = died {
            unit_event::dispatch_unit_death(&g, ctx, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetWidgetLife", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).life);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetWidgetLife", move |ctx, a| {
        let died = {
            let mut gm = g.borrow_mut();
            unit_at(&gm, a, 0).filter(|&k| set_life(&mut gm, k, real_at(a, 1)))
        };
        if let Some(k) = died {
            unit_event::dispatch_unit_death(&g, ctx, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetWidgetX", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).x);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetWidgetY", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).y);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("ShowUnit", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).hidden = !bool_arg(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsUnitHidden", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).is_some_and(|k| gm.unit(k).hidden);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("PauseUnit", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).paused = bool_arg(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsUnitPaused", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).is_some_and(|k| gm.unit(k).paused);
        NativeResult::Return(Value::Boolean(v))
    });
}

/// Creates a unit (the body of the `CreateUnit` natives, CONFIRMED order in the module notes). Returns
/// its handle id, or 0 when the owner handle is not a live player or the type is not registered. A
/// failed call allocates no id.
fn create(gm: &mut Game, owner_handle: u32, type_id: u32, x: F32, y: F32, face: F32) -> u32 {
    let Some(owner) = gm.player_index_of_handle(owner_handle) else {
        return 0;
    };
    // TODO(gamedata): the registry is filled from SLK data by the engine (see `Game::unit_types`).
    let Some(ty) = gm.unit_types().get(type_id).cloned() else {
        return 0;
    };
    let unit = UnitObj {
        type_id,
        owner,
        x,
        y,
        facing: arith::mul(face, DEG_TO_RAD),
        life: ty.max_life,
        max_life: ty.max_life,
        mana: ty.start_mana,
        max_mana: ty.max_mana,
        dead: false,
        hidden: false,
        paused: false,
        removed: false,
    };
    // The handle is allocated last, after the unit is built (CONFIRMED, spec section 7). The handle flag
    // of units is UNSURE; 0 is used, as for the other handle types the scripts create on demand.
    let (_, id) = gm.add_object(Object::Unit(unit), 0);
    id
}

/// Removes the unit (the body of `RemoveUnit`, see the module notes).
fn remove(gm: &mut Game, k: ObjKey) {
    gm.unit_mut(k).removed = true;
    gm.push_unit_event(UnitEvent::Removed(k));
    for id in REMOVE_SEQUENCE {
        gm.push_unit_event(UnitEvent::Notify { unit: k, id });
    }
    // Releases the creation reference: the id is freed once no other holder keeps it.
    gm.kill(k);
}

/// `SetLife` (CONFIRMED, spec 6.5): a value at or below zero is stored as `+0.0`, then the life-changed
/// event fires. A change to zero of a unit that was alive also records its death. Returns `true` when that
/// death happened, so the native can run the death dispatch (see `unit_event.rs`).
fn set_life(gm: &mut Game, k: ObjKey, value: F32) -> bool {
    let stored = if value.to_f32() <= 0.0 {
        F32::ZERO
    } else {
        value
    };
    let newly_dead = {
        let u = gm.unit_mut(k);
        u.life = stored;
        if stored.to_bits() == 0 && !u.dead {
            u.dead = true;
            true
        } else {
            false
        }
    };
    gm.push_unit_event(UnitEvent::Notify {
        unit: k,
        id: EVENT_LIFE_CHANGED,
    });
    if newly_dead {
        gm.push_unit_event(UnitEvent::Death(k));
    }
    newly_dead
}

/// `SetUnitState` (CONFIRMED, spec 6.7): life goes through [`set_life`], mana is stored as given, and
/// the maxima and unknown states are ignored. Returns `true` when the unit died.
fn set_state(gm: &mut Game, k: ObjKey, state: u32, value: F32) -> bool {
    match state {
        STATE_LIFE => set_life(gm, k, value),
        STATE_MANA => {
            gm.unit_mut(k).mana = value;
            false
        }
        _ => false,
    }
}

/// Stores a position as given (see the module notes on the position model).
fn set_position(gm: &mut Game, k: ObjKey, x: F32, y: F32) {
    let u = gm.unit_mut(k);
    u.x = x;
    u.y = y;
}

/// The unit in argument `i`, if it is a live unit.
fn unit_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.unit_of_handle(handle_arg(a, i))
}

/// The location in argument `i`, if it is a live location.
fn loc_at(gm: &Game, a: &[Value], i: usize) -> Option<LocationObj> {
    let k = gm.handle_of_kind(handle_arg(a, i), super::game::HandleKind::Location)?;
    match gm.object(k) {
        Object::Location(l) => Some(*l),
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

/// The string in argument `i` as text, or the empty string.
fn string_arg(ctx: &jass::vm::NativeCtx<'_>, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(id)) => ctx.string(*id).to_string(),
        _ => String::new(),
    }
}
