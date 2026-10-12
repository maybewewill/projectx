//! Unit and player-unit events: `TriggerRegisterUnitEvent`, `TriggerRegisterPlayerUnitEvent`, the death
//! dispatch that `KillUnit`, `SetWidgetLife` and `SetUnitState` (state 0) run, and the event getters
//! `GetTriggerUnit`, `GetDyingUnit` and `GetTriggerPlayer` (`GetKillingUnit` is in `event.rs`).
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions in `docs/spec/groups-events.md`
//! (section 5 to 9). Labels as in `unit.rs`.
//!
//! * Registration keys (CONFIRMED, spec 5.1 and 5.2). A unit event key is the event id + `0x80200`. Accepted
//!   ids: `0x34`..=`0x58` and `0x119`..=`0x129` for units, `0x12`..=`0x33` and `0x10D`..=`0x118` for players.
//!   Anything else returns the null event with no side effect. The unit (or trigger, or player) must resolve.
//!   For a player-unit event the filter boolexpr may be null, and a non-null one must be a live boolexpr.
//! * Observer tables (CONFIRMED, spec 7.1 to 7.3). Each agent has one ordered table: a unit for unit events,
//!   a player slot for player-unit events. Entries are kept oldest first. Registering the same trigger on the
//!   same key again overwrites that entry's payload in place and keeps its position (the event handle that
//!   is returned is new). Destroying the trigger removes its entries.
//! * Dispatch (CONFIRMED structure, spec 7.3): every matching entry runs, oldest first, with no early exit.
//!   The entries are read before the first one runs, so a trigger registered by a running trigger is not
//!   visited in the same firing (UNVERIFIED for the original's walk, which the spec does not decide).
//! * Death order (CONFIRMED, spec 5.4 and 9): the player-unit key `0x80214` on the owner's table first, then
//!   the unit key `0x80235` on the dying unit's table. Each is run synchronously inside the native that
//!   killed the unit (conditions run at once, actions start as a chain when the native returns).
//! * Player-unit filter (LIKELY, spec 5.1 and 5.3): the filter boolexpr is evaluated with `GetFilterUnit` set to
//!   the dying unit, and the trigger does not run when it is false. The spec does not describe this path, so it
//!   is UNVERIFIED.
//! * Death detection (UNSURE, spec 9): the death is taken to happen when a unit's life changes to zero while it
//!   was alive (see `unit.rs`). The spec does not say whether every kill path runs this dispatch.
//! * Owner change: the queue records [`super::game::UnitEvent::OwnerChanged`], but no event id is known for it in
//!   the spec, so no trigger event is dispatched for it yet.
//! * Getters (CONFIRMED accepted ids, spec 8). `GetTriggerUnit` returns the unit of the running dispatch.
//!   `GetDyingUnit` does the same for event ids `0x14` and `0x35` only. `GetKillingUnit` returns the attacker of a
//!   combat kill (the killer is handed over by `combat.rs`, see `dispatch_unit_death_by`, LIKELY) and the null handle
//!   for `KillUnit`, `SetWidgetLife` and `SetUnitState` (no damage source). `GetTriggerPlayer` returns the player of a player-unit event
//!   (LIKELY; the null handle for unit events, UNSURE).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::event::{Datum, Field, Firing};
use super::game::{Ctx, Event, EventSource, Game, HandleKind, ObjKey, Object, Observer};
use super::player::raw_at;
use super::trigger::{eval_boolexpr, fire_with_slots, fire_with_unit};
use super::ScriptRuntime;

/// The base of the registration keys: key = event id + this value (CONFIRMED, spec 5.2).
pub const KEY_BASE: u32 = 0x0008_0200;
/// The player-unit death event id (`EVENT_PLAYER_UNIT_DEATH`, key `0x80214`).
pub const EVENT_PLAYER_UNIT_DEATH: u32 = 0x14;
/// The unit death event id (`EVENT_UNIT_DEATH`, key `0x80235`).
pub const EVENT_UNIT_DEATH: u32 = 0x35;

/// Registers the unit-event natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("TriggerRegisterUnitEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = event_id(a, 2);
        if !unit_event_accepted(id) {
            return NativeResult::Return(Value::Handle(0));
        }
        let trigger = gm.handle_of_kind(handle_arg(a, 0), HandleKind::Trigger);
        let unit = gm.unit_of_handle(handle_arg(a, 1));
        let (Some(t), Some(u)) = (trigger, unit) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let key = id + KEY_BASE;
        let handle = register_observer(&mut gm, u, key, t, None, EventSource::UnitEvent { key });
        NativeResult::Return(Value::Handle(handle))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterPlayerUnitEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = event_id(a, 2);
        if !player_unit_event_accepted(id) {
            return NativeResult::Return(Value::Handle(0));
        }
        let trigger = gm.handle_of_kind(handle_arg(a, 0), HandleKind::Trigger);
        let player = gm.player_index_of_handle(handle_arg(a, 1));
        let filter = match handle_arg(a, 3) {
            0 => None,
            id => match gm.handle_of_kind(id, HandleKind::BoolExpr) {
                Some(b) => Some(b),
                None => return NativeResult::Return(Value::Handle(0)),
            },
        };
        let (Some(t), Some(p)) = (trigger, player) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let key = id + KEY_BASE;
        let source = EventSource::PlayerUnitEvent { key, player: p };
        let handle = register_observer(&mut gm, ObjKey(p), key, t, filter, source);
        NativeResult::Return(Value::Handle(handle))
    });

    let g = Rc::clone(game);
    reg.register("GetTriggerUnit", move |ctx, _| {
        let gm = g.borrow();
        let id = trigger_unit_handle(&gm, ctx.thread_key());
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetDyingUnit", move |ctx, _| {
        let gm = g.borrow();
        let death = matches!(
            current_event_id(&gm, ctx.thread_key()),
            Some(EVENT_PLAYER_UNIT_DEATH | EVENT_UNIT_DEATH)
        );
        let id = if death {
            trigger_unit_handle(&gm, ctx.thread_key())
        } else {
            0
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetTriggerPlayer", move |ctx, _| {
        let mut gm = g.borrow_mut();
        let firing_player = match gm.context(ctx.thread_key()) {
            Some(Ctx::Trigger {
                firing: Some(i), ..
            }) => gm.firings.get(i as usize).and_then(|f| {
                f.data.iter().find_map(|(field, d)| match (field, d) {
                    (super::event::Field::Player, super::event::Datum::Player(p)) => Some(*p),
                    _ => None,
                })
            }),
            _ => None,
        };
        let player = firing_player.or(match gm.context(ctx.thread_key()) {
            Some(Ctx::Trigger { event: Some(e), .. }) => match gm.object(e) {
                Object::Event(ev) => match ev.source {
                    EventSource::PlayerUnitEvent { player, .. } => Some(player),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        });
        let id = player.map_or(0, |p| gm.player_handle(p));
        NativeResult::Return(Value::Handle(id))
    });
}

/// Runs the death event of `unit` (spec 9) with no known killer: the owner's player-unit table with key `0x80214`,
/// then the unit's own table with key `0x80235`. `GetKillingUnit` reads null in these events (the `KillUnit`,
/// `SetWidgetLife` and `SetUnitState` paths). Called by the natives that set the life to zero, after their borrow of
/// the game is released.
pub(crate) fn dispatch_unit_death(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey) {
    dispatch_unit_death_by(game, rt, unit, None);
}

/// [`dispatch_unit_death`] with the killer: the unit whose damage took the life to zero (combat damage passes the
/// attacker, see `combat.rs`). With a killer the events carry it as the firing's `Killer` field, which
/// `GetKillingUnit` reads for the event ids `0x14` and `0x35` (LIKELY the original's killer lookup, UNSURE which
/// global the original takes it from). Without one, nothing is recorded and the events behave as before.
pub(crate) fn dispatch_unit_death_by(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    killer: Option<ObjKey>,
) {
    let owner = game.borrow().unit(unit).owner;
    {
        let mut gm = game.borrow_mut();
        let killer_owner = killer.map_or(u32::MAX, |k| gm.unit(k).owner);
        let ty = gm.unit(unit).type_id;
        gm.kill_log.push((killer_owner, owner, ty));
    }
    // The death handler `0x6F2A4880` gives the kill experience first (UNSURE order against the JASS events).
    if let Some(k) = killer {
        crate::kill_xp::grant_kill_xp(&mut game.borrow_mut(), unit, k);
    }
    let firing = killer.map(|k| {
        let mut gm = game.borrow_mut();
        gm.firings.push(Firing {
            id: EVENT_UNIT_DEATH,
            data: vec![(Field::Killer, Datum::Object(k))],
        });
        (gm.firings.len() - 1) as u32
    });
    fire_observers_in(
        game,
        rt,
        ObjKey(owner),
        EVENT_PLAYER_UNIT_DEATH + KEY_BASE,
        unit,
        firing,
    );
    fire_observers_in(game, rt, unit, EVENT_UNIT_DEATH + KEY_BASE, unit, firing);
    // `TriggerRegisterDeathEvent` on a unit (LIKELY: the widget death key is shared by widgets, see
    // `destructable.rs`). The unit is the trigger unit of these entries, as for the unit death.
    fire_observers_in(
        game,
        rt,
        unit,
        super::destructable::WIDGET_DEATH_KEY,
        unit,
        firing,
    );
}

/// Fires the entries of `agent`'s table whose key is `key`, oldest first. Returns the OR of the results of
/// the fired triggers (see [`fire_with_unit`]). `firing` is the index of the [`Firing`] the triggers read (`None`
/// for no payload, which is the same run as [`fire_with_unit`]).
pub(crate) fn fire_observers_in(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    agent: ObjKey,
    key: u32,
    unit: ObjKey,
    firing: Option<u32>,
) -> bool {
    let nodes: Vec<Observer> = game
        .borrow()
        .observers(agent)
        .iter()
        .filter(|o| o.key == key)
        .copied()
        .collect();
    let mut any = false;
    for node in nodes {
        if let Some(b) = node.filter {
            if !filter_passes(game, rt, b, node, unit) {
                continue;
            }
        }
        any |= match firing {
            None => fire_with_unit(game, rt, node.trigger, Some(node.event), Some(unit)),
            Some(_) => fire_with_slots(
                game,
                rt,
                node.trigger,
                Some(node.event),
                Some(unit),
                None,
                firing,
            ),
        };
    }
    any
}

/// The filter of a player-unit event, run with `GetFilterUnit` set to the unit (UNVERIFIED, see the notes).
pub(crate) fn filter_passes(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    b: ObjKey,
    node: Observer,
    unit: ObjKey,
) -> bool {
    let id = game.borrow_mut().handle_or_create(unit);
    let key = game.borrow_mut().context_key(Ctx::Trigger {
        trigger: node.trigger,
        event: Some(node.event),
        unit: Some(unit),
        order: None,
        firing: None,
    });
    game.borrow_mut().push_filter_unit(id);
    let ok = eval_boolexpr(game, rt, b, key);
    game.borrow_mut().pop_filter_unit();
    ok
}

/// Adds an observer entry for `trigger` on `agent`'s table under `key`, with a new event object. An
/// existing entry for the same key and trigger keeps its place and takes the new payload (spec 7.2). Returns
/// the event's handle id.
pub(crate) fn register_observer(
    gm: &mut Game,
    agent: ObjKey,
    key: u32,
    trigger: ObjKey,
    filter: Option<ObjKey>,
    source: EventSource,
) -> u32 {
    let (ev, handle) = gm.add_object(Object::Event(Event { trigger, source }), 1);
    if let Object::Trigger(tr) = gm.object_mut(trigger) {
        tr.events.push(ev);
    }
    let table = gm.observers_mut(agent);
    match table
        .iter_mut()
        .find(|o| o.key == key && o.trigger == trigger)
    {
        Some(o) => {
            o.event = ev;
            o.filter = filter;
        }
        None => table.push(Observer {
            key,
            trigger,
            event: ev,
            filter,
        }),
    }
    handle
}

/// Whether `id` is an accepted unit event id (spec 5.1).
fn unit_event_accepted(id: u32) -> bool {
    matches!(id, 0x34..=0x58 | 0x119..=0x129)
}

/// Whether `id` is an accepted player-unit event id (spec 5.1).
fn player_unit_event_accepted(id: u32) -> bool {
    matches!(id, 0x12..=0x33 | 0x10D..=0x118)
}

/// The event id of the running dispatch (the key without its base), if the thread runs an event that has one.
pub(crate) fn current_event_id(gm: &Game, thread: u32) -> Option<u32> {
    // An event fired through `event::fire_event` carries its id in its firing record.
    if let Some(Ctx::Trigger {
        firing: Some(i), ..
    }) = gm.context(thread)
    {
        return gm.firings.get(i as usize).map(|f| f.id);
    }
    let Some(Ctx::Trigger { event: Some(e), .. }) = gm.context(thread) else {
        return None;
    };
    match gm.object(e) {
        Object::Event(ev) => match ev.source {
            EventSource::UnitEvent { key }
            | EventSource::PlayerUnitEvent { key, .. }
            | EventSource::RegionEvent { key, .. }
            | EventSource::WidgetDeath { key, .. }
            | EventSource::Keyed { key, .. }
            | EventSource::InRange { key, .. } => key.checked_sub(KEY_BASE),
            _ => None,
        },
        _ => None,
    }
}

/// The handle of the unit of the thread's dispatch (0 when it has none or the unit has no handle).
pub(crate) fn trigger_unit_handle(gm: &Game, thread: u32) -> u32 {
    match gm.context(thread) {
        Some(Ctx::Trigger { unit: Some(u), .. }) => gm.handle_of(u),
        _ => 0,
    }
}

/// The event id argument `i` (a raw integer or handle value).
fn event_id(a: &[Value], i: usize) -> u32 {
    raw_at(a, i) as u32
}

/// The handle id in argument `i`, or 0 (the null handle) when there is none.
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_ranges_match_the_spec() {
        assert!(unit_event_accepted(0x35));
        assert!(unit_event_accepted(0x34));
        assert!(unit_event_accepted(0x58));
        assert!(!unit_event_accepted(0x59));
        assert!(unit_event_accepted(0x119));
        assert!(!unit_event_accepted(0x118));
        assert!(player_unit_event_accepted(0x14));
        assert!(player_unit_event_accepted(0x118));
        assert!(!player_unit_event_accepted(0x34));
    }

    #[test]
    fn death_keys_match_the_spec() {
        assert_eq!(EVENT_PLAYER_UNIT_DEATH + KEY_BASE, 0x8_0214);
        assert_eq!(EVENT_UNIT_DEATH + KEY_BASE, 0x8_0235);
    }
}
