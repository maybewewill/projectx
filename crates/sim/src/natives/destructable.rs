//! Destructable natives: `CreateDestructable` and its Z, dead and Z-dead forms, `RemoveDestructable`,
//! `KillDestructable`, the life pair with `DestructableRestoreLife` and the max life pair, the position getters,
//! the type id, invulnerability, `ShowDestructable`, the occluder height, `EnumDestructablesInRect` with
//! `GetEnumDestructable` / `GetFilterDestructable`, `GetTriggerDestructable` and `TriggerRegisterDeathEvent`
//! (for destructables and units).
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions (`natives.c` bodies and the callees in the
//! decompile export; the core is `0x6F26A830`, the life setter `0x6F26B100`). Labels as in `unit.rs`: CONFIRMED =
//! read in the decompile, LIKELY = strong inference from the decompile, UNSURE = guessed or not read.
//! Vtable slots are the byte offsets of `CDestructable` (`0x6F92EAF4`): slot `N` is at `4 * N`.
//!
//! * Natives and their bodies (CONFIRMED, `natives.c`): `CreateDestructable` `0x6F3BB790` calls the core
//!   `0x6F26AD20` and then allocates the handle (`0x6F430C80`) last. `CreateDestructableZ` `0x6F3BB7E0`,
//!   `CreateDeadDestructable` `0x6F3BB840` and `CreateDeadDestructableZ` `0x6F3BB890` follow the same pattern. The
//!   face argument is multiplied by pi/180 before the core call (CONFIRMED that a multiply by the `0x6FAAE5E8`
//!   constant happens; LIKELY that it is the face).
//! * Handle resolution (CONFIRMED): every native resolves its argument through `0x6F3BE010`, which returns null
//!   for a null, freed, wrong-type or removed handle. The natives then return their default with no side effect
//!   (`GetDestructableLife` and friends return 0).
//! * Creation order (CONFIRMED structure, LIKELY details). The core looks the type up first in the destructable
//!   data table (`0x6F2686C0`, a hash table of the `DestructableDataNode` records keyed by the four-character
//!   code). When there is no record the core creates nothing, so the native gets no object and allocates no
//!   handle id (LIKELY, see [`create`]).
//! * Variation (CONFIRMED clamp, `0x6F26A830`): the record's variation count (`+0x58`) is read and the requested
//!   variation is clamped to `count - 1` when it is at or above it (unsigned compare, so a negative argument
//!   becomes the last variation). No random draw is found in the creation path: the core's direct callees do not
//!   call the RNG step or the GetRandom natives (LIKELY no draw; a call graph walk was not completed).
//! * Scale (CONFIRMED values): the scale is clamped to `[0.1, 10.0]` (constants at `0x6F92ED04/08/10/18`, read
//!   from the binary: 0.1 as f32, 0.1 and 10.0 as the clamp bounds). The clamp uses the float comparisons.
//! * Position (LIKELY): `CreateDestructableZ` stores the given z. `CreateDestructable` takes its z from the
//!   terrain height at `(x, y)` (LIKELY: the wrapper `0x6F266210` calls the terrain query `0x6F0126F0`). The
//!   terrain is not loaded here, so z is 0.0 for that form (TODO(terrain)).
//! * Life (CONFIRMED for the vtable slots, LIKELY for the values): `SetDestructableLife` calls the life setter
//!   (vtable slot `0x124`, `0x6F26B100`). A value at or below zero is stored as `+0.0`, the same clamp as units
//!   (LIKELY). The dead test is `life <= 0` (slot `0x13C`, `0x6F266BB0`, CONFIRMED). A change from a positive life
//!   to zero is a death: the corpse flag is set, the pathing blocker is removed, and the death observers run (see
//!   below). A new destructable starts at the type's max life (LIKELY); a dead creation starts at zero.
//! * Revive (LIKELY): the setter calls its "alive" path when the new life is above `0.4` (`0x6FAAE4C4`, read as
//!   0.4 f32 at the start-up) on a corpse. That path clears the corpse flag and puts the blocker back. The exact
//!   body of the path is not read (UNSURE: the other effects of that call).
//! * `KillDestructable` sets the life to zero through the same setter (CONFIRMED). `RemoveDestructable`
//!   (`0x6F26A7E0`, CONFIRMED structure): hides, removes the blocker and releases the object through the
//!   destroy slot. No death event is fired by a removal (LIKELY). The creation reference is released, so the id
//!   is freed when no saved handle keeps it (LIKELY, the same as `RemoveUnit`).
//! * `DestructableRestoreLife(d, life, birth)` (`0x6F26A330`, CONFIRMED structure): it acts only on a dead
//!   destructable (life `<= 0`). The target life is `life` when `|life| >= 0.001` and `0.5` when that is at or
//!   below `0.5`; a `life` with `|life| < 0.001` restores the max life (CONFIRMED from the comparisons with the
//!   constants `0x6FAAE47C` = 0.001 and `0x6FAAE4D0` = 0.5). The blocker is put back, and the birth flag chooses
//!   between two animation calls (animations are not modelled).
//! * `SetDestructableMaxLife` stores the max life as given (UNSURE: the original also calls a helper that may
//!   clamp the current life to the new maximum, not read).
//! * Invulnerability (CONFIRMED bit, `+0x20` bit 3): `SetDestructableInvulnerable` stores the flag and
//!   `IsDestructableInvulnerable` returns it. No damage path reads it yet, and JASS life changes ignore it
//!   (LIKELY: a JASS life change is not damage).
//! * `ShowDestructable(false)` calls vtable slot `0x84` (hide), `ShowDestructable(true)` slot `0x80` (show)
//!   (CONFIRMED). Hiding does not change the blocker (LIKELY).
//! * Occluder height (CONFIRMED, object `+0x40`, default 0.0): the getter and the setter read and write the field.
//! * `EnumDestructablesInRect` (`0x6F3C4A90`, CONFIRMED structure): the rect is copied, the filter scope of
//!   `GetFilterDestructable` is pushed for every candidate before the boolexpr runs (the collector `0x6F3BB8F0`
//!   calls `0x6F430DB0(9, ...)` first), the boolexpr decides, and the enum scope of `GetEnumDestructable` and the
//!   action run only when the filter passed and an action is given. The walk order is not read (`0x6F4693D0`,
//!   UNSURE): the candidates are the arena in creation order, as the item walk does ([`destructable_walk`]). The
//!   rect test is inclusive (LIKELY, the unit query's test). Removed destructables are not visited (LIKELY);
//!   hidden and dead ones are (UNSURE).
//! * `GetTriggerDestructable` (CONFIRMED, `0x6F3C1D20`): the widget of the running dispatch, when it is a live
//!   destructable (LIKELY the widget slot of the dispatch; here the event's widget, see below).
//! * `TriggerRegisterDeathEvent(trigger, widget)` (`0x6F3D22C0`, CONFIRMED structure): it resolves the widget, adds
//!   a new event object (flag 1, as the unit events do) and registers it on the widget with key `0x80259`
//!   (CONFIRMED value, `0x6F4364E0`). The model accepts a live unit or a live destructable as the widget (items
//!   are not modelled as widgets here: UNSURE). The death of a destructable fires the key `0x80259` observers,
//!   oldest first, with `GetTriggerDestructable` set (LIKELY: the original's destructable death dispatch is
//!   not read). The death of a unit fires them too (LIKELY, see `unit_event.rs`).
//! * Not modelled: the animation natives (`SetDestructableAnimation`, `QueueDestructableAnimation`,
//!   `SetDestructableAnimationSpeed`), `GetDestructableName` (the type name lives in the data table), the
//!   pathing write itself ([`set_pathing_blocker`] is the hook), the terrain height for `CreateDestructable`, and
//!   the destructable death path's effects other than the event (the corpse model and the animation).
//!
//! The death event key `0x80259` is a widget key: [`WIDGET_DEATH_KEY`]. The tests are in
//! `tests/destructable_natives.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{Ctx, DestructableObj, EventSource, Game, HandleKind, ObjKey, Object, RectObj};
use super::group::in_rect;
use super::player::raw_at;
use super::trigger::{eval_boolexpr, fire_with_unit};
use super::unit_event::{register_observer, KEY_BASE};
use super::ScriptRuntime;

/// The widget death event key (`TriggerRegisterDeathEvent`, CONFIRMED value `0x80259`).
pub const WIDGET_DEATH_KEY: u32 = KEY_BASE + 0x59;

/// Degrees to radians (`0x6FAAE5E8`, see `unit.rs`).
const DEG_TO_RAD: F32 = F32::from_bits(0x3C8E_FA35);
/// The scale clamp bounds: `0.1` (`0x6F92ED04`, `0x6F92ED08`) and `10.0` (`0x6F92ED10`, `0x6F92ED18`).
const MIN_SCALE: F32 = F32::from_bits(0x3DCC_CCCD);
const MAX_SCALE: F32 = F32::from_bits(0x4120_0000);
/// The life that a corpse must exceed to be revived (`0x6FAAE4C4` = 0.4).
const REVIVE_LIFE: F32 = F32::from_bits(0x3ECC_CCCD);
/// The restore-life threshold on the given life (`0x6FAAE47C` = 0.001).
const RESTORE_EPSILON: F32 = F32::from_bits(0x3A83_126F);
/// The restore-life floor (`0x6FAAE4D0` = 0.5).
const RESTORE_FLOOR: F32 = F32::from_bits(0x3F00_0000);

/// Registers the destructable natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateDestructable", move |_, a| {
        // (objid, x, y, face, scale, variation)
        let mut gm = g.borrow_mut();
        let id = create(
            &mut gm,
            raw_at(a, 0) as u32,
            real_at(a, 1),
            real_at(a, 2),
            F32::ZERO,
            real_at(a, 3),
            real_at(a, 4),
            raw_at(a, 5),
            false,
        );
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("CreateDestructableZ", move |_, a| {
        // (objid, x, y, z, face, scale, variation)
        let mut gm = g.borrow_mut();
        let id = create(
            &mut gm,
            raw_at(a, 0) as u32,
            real_at(a, 1),
            real_at(a, 2),
            real_at(a, 3),
            real_at(a, 4),
            real_at(a, 5),
            raw_at(a, 6),
            false,
        );
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("CreateDeadDestructable", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = create(
            &mut gm,
            raw_at(a, 0) as u32,
            real_at(a, 1),
            real_at(a, 2),
            F32::ZERO,
            real_at(a, 3),
            real_at(a, 4),
            raw_at(a, 5),
            true,
        );
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("CreateDeadDestructableZ", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = create(
            &mut gm,
            raw_at(a, 0) as u32,
            real_at(a, 1),
            real_at(a, 2),
            real_at(a, 3),
            real_at(a, 4),
            real_at(a, 5),
            raw_at(a, 6),
            true,
        );
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("RemoveDestructable", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = destructable_at(&gm, a, 0) {
            remove(&mut gm, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("KillDestructable", move |ctx, a| {
        let died = {
            let mut gm = g.borrow_mut();
            destructable_at(&gm, a, 0).filter(|&k| set_life(&mut gm, k, F32::ZERO))
        };
        if let Some(k) = died {
            dispatch_death(&g, ctx, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetDestructableLife", move |ctx, a| {
        let died = {
            let mut gm = g.borrow_mut();
            destructable_at(&gm, a, 0).filter(|&k| set_life(&mut gm, k, real_at(a, 1)))
        };
        if let Some(k) = died {
            dispatch_death(&g, ctx, k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetDestructableLife", move |_, a| {
        let gm = g.borrow();
        let v = destructable_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.destructable(k).life);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("DestructableRestoreLife", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = destructable_at(&gm, a, 0) {
            restore_life(&mut gm, k, real_at(a, 1));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetDestructableMaxLife", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = destructable_at(&gm, a, 0) {
            gm.destructable_mut(k).max_life = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetDestructableMaxLife", move |_, a| {
        let gm = g.borrow();
        let v = destructable_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.destructable(k).max_life);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetDestructableX", move |_, a| {
        let gm = g.borrow();
        let v = destructable_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.destructable(k).x);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetDestructableY", move |_, a| {
        let gm = g.borrow();
        let v = destructable_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.destructable(k).y);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetDestructableTypeId", move |_, a| {
        let gm = g.borrow();
        let v = destructable_at(&gm, a, 0).map_or(0, |k| gm.destructable(k).type_id as i32);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetDestructableInvulnerable", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = destructable_at(&gm, a, 0) {
            gm.destructable_mut(k).invulnerable = bool_arg(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsDestructableInvulnerable", move |_, a| {
        let gm = g.borrow();
        let v = destructable_at(&gm, a, 0).is_some_and(|k| gm.destructable(k).invulnerable);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("ShowDestructable", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = destructable_at(&gm, a, 0) {
            gm.destructable_mut(k).hidden = !bool_arg(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetDestructableOccluderHeight", move |_, a| {
        let gm = g.borrow();
        let v =
            destructable_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.destructable(k).occluder_height);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetDestructableOccluderHeight", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = destructable_at(&gm, a, 0) {
            gm.destructable_mut(k).occluder_height = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("EnumDestructablesInRect", move |ctx, a| {
        let key = ctx.thread_key();
        let rect = rect_at(&g.borrow(), a, 0);
        if let Some(rect) = rect {
            let filter = boolexpr_at(&g, a, 1);
            let action = code_at(a, 2);
            enum_destructables_in_rect(&g, ctx, key, rect, filter, action);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetEnumDestructable", move |_, _| {
        let id = g.borrow().enum_destructable();
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetFilterDestructable", move |_, _| {
        let id = g.borrow().filter_destructable();
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetTriggerDestructable", move |ctx, _| {
        let gm = g.borrow();
        let id = trigger_widget_handle(&gm, ctx.thread_key());
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterDeathEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let trigger = gm.handle_of_kind(handle_arg(a, 0), HandleKind::Trigger);
        let widget = widget_at(&gm, a, 1);
        let (Some(t), Some(w)) = (trigger, widget) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let source = EventSource::WidgetDeath {
            key: WIDGET_DEATH_KEY,
            widget: w,
        };
        let handle = register_observer(&mut gm, w, WIDGET_DEATH_KEY, t, None, source);
        NativeResult::Return(Value::Handle(handle))
    });
}

/// The core of the create natives (CONFIRMED order: the handle is allocated last; LIKELY for the details).
///
/// `face` is in degrees and `scale` as given. A type without a record gives the null handle and allocates no
/// id. `dead` makes a corpse: life zero and no pathing blocker.
#[allow(clippy::too_many_arguments)]
fn create(
    gm: &mut Game,
    type_id: u32,
    x: F32,
    y: F32,
    z: F32,
    face: F32,
    scale: F32,
    variation: i32,
    dead: bool,
) -> u32 {
    let Some(ty) = gm.destructable_types().get(type_id).cloned() else {
        return 0;
    };
    let scale = clamp_scale(scale);
    let variation = clamp_variation(ty.variations, variation as u32);
    let life = if dead { F32::ZERO } else { ty.max_life };
    let obj = DestructableObj {
        type_id,
        x,
        y,
        z,
        facing: arith::mul(face, DEG_TO_RAD),
        scale,
        variation,
        life,
        max_life: ty.max_life,
        occluder_height: F32::ZERO,
        dead,
        hidden: false,
        invulnerable: false,
        removed: false,
        blocks_pathing: false,
    };
    let (key, id) = gm.add_object(Object::Destructable(obj), 0);
    set_pathing_blocker(gm, key, !dead);
    id
}

/// The scale clamp (LIKELY from the float comparisons at the core's start): above 10.0 becomes 10.0, below 0.1
/// becomes 0.1, and anything else stays.
fn clamp_scale(scale: F32) -> F32 {
    if scale.to_f32() > MAX_SCALE.to_f32() {
        MAX_SCALE
    } else if scale.to_f32() < MIN_SCALE.to_f32() {
        MIN_SCALE
    } else {
        scale
    }
}

/// The variation clamp (CONFIRMED structure): `last = count - 1` with unsigned wrap; a variation at or above
/// `last` becomes `last`. For a count of 0 only the value `u32::MAX` is changed (by nothing).
fn clamp_variation(count: u32, variation: u32) -> u32 {
    let last = count.wrapping_sub(1);
    if last <= variation {
        last
    } else {
        variation
    }
}

/// The life setter (CONFIRMED values at `0x6F26B100`, LIKELY details). A value at or below zero is stored as
/// `+0.0`. A change from a positive life to zero is a death: the corpse flag is set and the blocker removed.
/// A value above `0.4` on a corpse revives it. Returns `true` when the destructable died now.
fn set_life(gm: &mut Game, k: ObjKey, value: F32) -> bool {
    let stored = if value.to_f32() <= 0.0 {
        F32::ZERO
    } else {
        value
    };
    let (newly_dead, revived) = {
        let d = gm.destructable_mut(k);
        let was_positive = d.life.to_f32() > 0.0;
        d.life = stored;
        let newly_dead = stored.to_f32() == 0.0 && was_positive;
        let revived = d.dead && stored.to_f32() > REVIVE_LIFE.to_f32();
        if newly_dead {
            d.dead = true;
        }
        if revived {
            d.dead = false;
        }
        (newly_dead, revived)
    };
    if newly_dead {
        set_pathing_blocker(gm, k, false);
    }
    if revived {
        set_pathing_blocker(gm, k, true);
    }
    newly_dead
}

/// `DestructableRestoreLife` (CONFIRMED structure, see the module notes). Acts only when the life is zero or
/// below. The birth flag is not used: it only chooses an animation, which is not modelled.
fn restore_life(gm: &mut Game, k: ObjKey, life: F32) {
    if gm.destructable(k).life.to_f32() > 0.0 {
        return;
    }
    let target = if life.to_f32().abs() >= RESTORE_EPSILON.to_f32() {
        if life.to_f32() <= RESTORE_FLOOR.to_f32() {
            RESTORE_FLOOR
        } else {
            life
        }
    } else {
        gm.destructable(k).max_life
    };
    // The setter revives the corpse when the target is above 0.4 (the floor is 0.5).
    set_life(gm, k, target);
    set_pathing_blocker(gm, k, true);
}

/// `RemoveDestructable` (LIKELY, see the module notes): the destructable is hidden, its blocker removed, marked
/// removed, and its creation reference released. No death event.
fn remove(gm: &mut Game, k: ObjKey) {
    {
        let d = gm.destructable_mut(k);
        d.hidden = true;
        d.removed = true;
    }
    set_pathing_blocker(gm, k, false);
    gm.kill(k);
}

/// The pathing hook. The pathing map is not written yet (TODO(pathing): the original writes the blocker bits of the
/// type's footprint into the pathing cells when the blocker is set, and clears them when it is removed; the
/// footprint and the cell rule are not read). The flag is kept so that the write can be added here.
fn set_pathing_blocker(gm: &mut Game, k: ObjKey, on: bool) {
    gm.destructable_mut(k).blocks_pathing = on;
}

/// Runs the death observers of destructable `k`, oldest first. Each entry fires with `GetTriggerDestructable` set
/// to `k` (through its event's widget) and no unit. The natives call this after their borrow of the game is
/// released.
pub(crate) fn dispatch_death(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, k: ObjKey) {
    let nodes: Vec<(ObjKey, ObjKey)> = game
        .borrow()
        .observers(k)
        .iter()
        .filter(|o| o.key == WIDGET_DEATH_KEY)
        .map(|o| (o.trigger, o.event))
        .collect();
    for (trigger, event) in nodes {
        fire_with_unit(game, rt, trigger, Some(event), None);
    }
}

/// The widget of the running trigger dispatch, when it is a live destructable (the event's widget, see
/// [`EventSource::WidgetDeath`]). 0 otherwise.
pub(crate) fn trigger_widget_handle(gm: &Game, thread: u32) -> u32 {
    let Some(Ctx::Trigger { event: Some(e), .. }) = gm.context(thread) else {
        return 0;
    };
    match gm.object(e) {
        Object::Event(ev) => match ev.source {
            EventSource::WidgetDeath { widget, .. } => match gm.object(widget) {
                Object::Destructable(d) if !d.removed => gm.handle_of(widget),
                _ => 0,
            },
            _ => 0,
        },
        _ => 0,
    }
}

/// The candidates of `EnumDestructablesInRect`, in creation order (UNSURE: the walker is not read). Only the arena
/// is walked, so this is the one place that defines the order.
fn destructable_walk(gm: &Game) -> Vec<ObjKey> {
    (0..gm.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(gm.object(k), Object::Destructable(_)))
        .collect()
}

/// `EnumDestructablesInRect` (see the module notes): per candidate, the filter scope is pushed, the boolexpr
/// decides, and the enum scope and the action run only when it passed and an action is given.
fn enum_destructables_in_rect(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    key: u32,
    rect: RectObj,
    filter: Option<ObjKey>,
    action: Option<u32>,
) {
    let candidates = destructable_walk(&game.borrow());
    for d in candidates {
        let id = {
            let mut gm = game.borrow_mut();
            if !candidate_in_rect(&gm, d, &rect) {
                continue;
            }
            let id = gm.handle_or_create(d);
            gm.push_filter_destructable(id);
            id
        };
        let passed = match filter {
            None => true,
            Some(b) => eval_boolexpr(game, rt, b, key),
        };
        if passed {
            if let Some(code) = action {
                game.borrow_mut().push_enum_destructable(id);
                let _ = rt.call_keyed(code, key);
                game.borrow_mut().pop_enum_destructable();
            }
        }
        game.borrow_mut().pop_filter_destructable();
    }
}

/// The candidate test: a live (not removed, handle-holding) destructable inside the rect (inclusive).
fn candidate_in_rect(gm: &Game, d: ObjKey, rect: &RectObj) -> bool {
    let obj = gm.destructable(d);
    !obj.removed && gm.is_alive(d) && in_rect(obj.x, obj.y, rect)
}

/// The destructable in handle argument `i`, if it is a live destructable.
fn destructable_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.destructable_of_handle(handle_arg(a, i))
}

/// The widget in handle argument `i` for `TriggerRegisterDeathEvent`: a live unit or a live destructable.
fn widget_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    let id = handle_arg(a, i);
    gm.unit_of_handle(id)
        .or_else(|| gm.destructable_of_handle(id))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_clamped_to_the_bounds_and_kept_otherwise() {
        assert_eq!(
            clamp_scale(F32::from_bits(0x41A0_0000)).to_bits(),
            MAX_SCALE.to_bits(),
            "20 -> 10"
        );
        assert_eq!(
            clamp_scale(F32::ZERO).to_bits(),
            MIN_SCALE.to_bits(),
            "0 -> 0.1"
        );
        assert_eq!(
            clamp_scale(F32::from_bits(0x3F80_0000)).to_bits(),
            0x3F80_0000,
            "1 stays"
        );
    }

    #[test]
    fn variation_is_clamped_to_the_last_one_with_unsigned_compare() {
        assert_eq!(clamp_variation(3, 0), 0);
        assert_eq!(clamp_variation(3, 2), 2);
        assert_eq!(clamp_variation(3, 9), 2);
        assert_eq!(
            clamp_variation(3, u32::MAX),
            2,
            "a negative request is the last one"
        );
        assert_eq!(
            clamp_variation(0, 5),
            5,
            "no variations: the request is kept"
        );
    }
}
