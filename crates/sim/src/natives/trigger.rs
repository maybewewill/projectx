//! Trigger natives: create and destroy, enable state, conditions, actions, the timer events,
//! `TriggerEvaluate`, `TriggerExecute`, the counters and the triggering-object queries.
//!
//! Behaviour notes (addresses are 1.26a `Game.dll`; labels as in `timer.rs`).
//!
//! * `CreateTrigger` (0x6F3D11F0): a new trigger, enabled, with no conditions and no actions. Its
//!   handle has allocator flag 0. Enabled by default is LIKELY (the event path tests the enable flag
//!   and the common case must fire).
//! * `DestroyTrigger` (0x6F3C1C00): death call on the trigger (slot 0x5C). Here it also cancels its
//!   timer events, removes its expire-event listeners, and releases its conditions, actions and events.
//!   LIKELY for the list releases.
//! * `ResetTrigger` (0x6F3C1C20): sets the evaluation count (+0x58) and the execution count (+0x5C) to
//!   0. Nothing else changes. CONFIRMED.
//! * `EnableTrigger` / `DisableTrigger` (0x6F3C1C40 / 0x6F3C1C60) set the flag that
//!   `IsTriggerEnabled` (0x6F3C1C80) reads. CONFIRMED for the pair, LIKELY that the flag is the enable
//!   state (the event path checks it, see below).
//! * Event path (0x6F4482A0, called by every event): the evaluation count goes up first, even for a
//!   disabled trigger. Only if the trigger is enabled are its conditions evaluated. If they pass, the
//!   execution count goes up and the actions run. CONFIRMED structure.
//! * Conditions (0x6F4480F0 / 0x6F447BA0): every condition is called, in list order, with no early
//!   exit. The result is the AND of all of them, and an empty list is true. The loop computes an OR
//!   as well, and the mode value picks AND or OR (0x6F473170). The AND is taken for ordinary triggers
//!   (LIKELY: an empty list must be true for the common case). Edge-triggered event modes are not
//!   modelled. A condition whose call fails (for example a divide by zero) counts as false (UNSURE).
//! * `TriggerEvaluate` (0x6F3C3F80 -> 0x6F448320): the condition result, with no counter change and
//!   no enable check. CONFIRMED.
//! * `TriggerExecute` (0x6F3C3F40 -> 0x6F4482D0): the evaluation count and the execution count both go
//!   up, then the actions run. It runs even when the trigger is disabled and without its conditions.
//!   CONFIRMED. The actions run as one chain: the first action starts at once and runs until its first
//!   sleep; the next action starts when the sleep ends (LIKELY, from 0x6F447E10 and 0x6F447340).
//! * `TriggerAddCondition` (0x6F3C3DF0): a null boolexpr gives the null handle and adds nothing.
//!   Otherwise a condition is appended. Its handle has flag 0. CONFIRMED for the null case.
//! * `TriggerAddAction` (0x6F3C3E90): a null code gives the null handle. The action is appended.
//!   CONFIRMED for the null case.
//! * `TriggerRegisterTimerEvent(t, timeout, periodic)` (0x6F3D1420): an event whose own queue entry
//!   fires the trigger's event path at each expiry (re-armed when periodic). The event handle has
//!   flag 1. LIKELY.
//! * `TriggerRegisterTimerExpireEvent(t, timer)` (0x6F3D1550): the trigger is added to the timer's
//!   listeners. It fires at each expiry of the timer, in registration order (LIKELY, see
//!   `timer.rs`).
//! * `GetTriggeringTrigger` (0x6F3BB2A0): the trigger whose conditions, actions or handler the calling
//!   thread runs (per-thread context, see `game.rs`). The null handle otherwise. LIKELY.
//! * `GetTriggerEventId` (0x6F3BB2C0 -> 0x6F4280B0): the event that started the calling thread's
//!   trigger, or the null handle (UNSURE: the native calls a current-event getter, its handle type is
//!   not resolved).
//! * `GetTriggerEvalCount` / `GetTriggerExecCount` (0x6F3C1CE0 / 0x6F3C1D00): the counters, 0 for a
//!   null or destroyed trigger. CONFIRMED.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{
    Action, BoolExpr, Condition, Ctx, Event, EventSource, Game, ObjKey, Object, Payload, TriggerObj,
};
use super::ScriptRuntime;

/// Registers the trigger natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateTrigger", move |_, _| {
        let (_, id) = g
            .borrow_mut()
            .add_object(Object::Trigger(TriggerObj::new()), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("DestroyTrigger", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(t) = trigger_of(&gm, a) {
            destroy(&mut gm, t);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ResetTrigger", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(t) = trigger_of(&gm, a) {
            if let Object::Trigger(tr) = gm.object_mut(t) {
                tr.evals = 0;
                tr.execs = 0;
            }
        }
        NativeResult::Return(Value::Null)
    });

    for (name, enable) in [("EnableTrigger", true), ("DisableTrigger", false)] {
        let g = Rc::clone(game);
        reg.register(name, move |_, a| {
            let mut gm = g.borrow_mut();
            if let Some(t) = trigger_of(&gm, a) {
                if let Object::Trigger(tr) = gm.object_mut(t) {
                    tr.enabled = enable;
                }
            }
            NativeResult::Return(Value::Null)
        });
    }

    let g = Rc::clone(game);
    reg.register("IsTriggerEnabled", move |_, a| {
        let gm = g.borrow();
        let on = trigger_of(&gm, a).is_some_and(|t| trigger_ref(&gm, t).enabled);
        NativeResult::Return(Value::Boolean(on))
    });

    let g = Rc::clone(game);
    reg.register("TriggerWaitOnSleeps", move |_, a| {
        let mut gm = g.borrow_mut();
        let flag = a.get(1).and_then(|v| v.as_boolean()).unwrap_or(false);
        if let Some(t) = trigger_of(&gm, a) {
            if let Object::Trigger(tr) = gm.object_mut(t) {
                tr.wait_on_sleeps = flag;
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsTriggerWaitOnSleeps", move |_, a| {
        let gm = g.borrow();
        let on = trigger_of(&gm, a).is_some_and(|t| trigger_ref(&gm, t).wait_on_sleeps);
        NativeResult::Return(Value::Boolean(on))
    });

    let g = Rc::clone(game);
    reg.register("Condition", move |_, a| new_leaf(&g, a));
    let g = Rc::clone(game);
    reg.register("Filter", move |_, a| new_leaf(&g, a));

    for name in ["DestroyCondition", "DestroyFilter", "DestroyBoolExpr"] {
        let g = Rc::clone(game);
        reg.register(name, move |_, a| {
            let mut gm = g.borrow_mut();
            if let Some(b) = boolexpr_of(&gm, a) {
                gm.kill(b);
            }
            NativeResult::Return(Value::Null)
        });
    }

    let g = Rc::clone(game);
    reg.register("And", move |_, a| combine(&g, a, BoolExpr::And));
    let g = Rc::clone(game);
    reg.register("Or", move |_, a| combine(&g, a, BoolExpr::Or));
    let g = Rc::clone(game);
    reg.register("Not", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(x) = boolexpr_of(&gm, &a[..1]) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let id = add_boolexpr(&mut gm, BoolExpr::Not(x));
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("TriggerAddCondition", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(b)) = (trigger_of(&gm, a), boolexpr_of(&gm, &a[1..])) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let (_, id) = gm.add_object(
            Object::Condition(Condition {
                trigger: t,
                boolexpr: b,
            }),
            0,
        );
        push_list(&mut gm, t, |tr| &mut tr.conditions, id);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRemoveCondition", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(t) = trigger_of(&gm, a) {
            if let Some(c) = item_of(&gm, a, 1, is_condition, t) {
                remove_item(&mut gm, t, c, |tr| &mut tr.conditions);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("TriggerClearConditions", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(t) = trigger_of(&gm, a) {
            let items = std::mem::take(list_mut(&mut gm, t, |tr| &mut tr.conditions));
            for c in items {
                gm.kill(c);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("TriggerAddAction", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(code)) = (trigger_of(&gm, a), a.get(1).and_then(code_of)) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let (_, id) = gm.add_object(Object::Action(Action { trigger: t, code }), 0);
        push_list(&mut gm, t, |tr| &mut tr.actions, id);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRemoveAction", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(t) = trigger_of(&gm, a) {
            if let Some(c) = item_of(&gm, a, 1, is_action, t) {
                remove_item(&mut gm, t, c, |tr| &mut tr.actions);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("TriggerClearActions", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(t) = trigger_of(&gm, a) {
            let items = std::mem::take(list_mut(&mut gm, t, |tr| &mut tr.actions));
            for c in items {
                gm.kill(c);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterTimerEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(t) = trigger_of(&gm, a) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let timeout = a
            .get(1)
            .and_then(|v| v.as_real())
            .unwrap_or(softfloat::F32::ZERO);
        let periodic = a.get(2).and_then(|v| v.as_boolean()).unwrap_or(false);
        let (ev, id) = gm.add_object(
            Object::Event(Event {
                trigger: t,
                source: EventSource::Timer(None),
            }),
            1,
        );
        let timer = gm
            .timers
            .start(timeout.to_f32(), periodic, Payload::Event(ev));
        if let Object::Event(e) = gm.object_mut(ev) {
            e.source = EventSource::Timer(Some(timer));
        }
        push_list(&mut gm, t, |tr| &mut tr.events, id);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("TriggerRegisterTimerExpireEvent", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(t), Some(timer)) = (trigger_of(&gm, a), timer_arg(&gm, a)) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let (ev, id) = gm.add_object(
            Object::Event(Event {
                trigger: t,
                source: EventSource::Expire(timer),
            }),
            1,
        );
        if let Object::Timer(tm) = gm.object_mut(timer) {
            tm.listeners.push((t, ev));
        }
        push_list(&mut gm, t, |tr| &mut tr.events, id);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("TriggerEvaluate", move |ctx, a| {
        let t = trigger_of(&g.borrow(), a);
        let Some(t) = t else {
            return NativeResult::Return(Value::Boolean(false));
        };
        let key = ctx_key(&g, t, None);
        let ok = eval_conditions(&g, ctx, t, key);
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("TriggerExecute", move |ctx, a| {
        let t = trigger_of(&g.borrow(), a);
        if let Some(t) = t {
            // TriggerExecute counts an evaluation too (0x6F4482D0 bumps both counters).
            if let Object::Trigger(tr) = g.borrow_mut().object_mut(t) {
                tr.evals = tr.evals.wrapping_add(1);
            }
            execute(&g, t, None, ctx);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetTriggerEvalCount", move |_, a| {
        let gm = g.borrow();
        let n = trigger_of(&gm, a).map_or(0, |t| trigger_ref(&gm, t).evals);
        NativeResult::Return(Value::Integer(n))
    });

    let g = Rc::clone(game);
    reg.register("GetTriggerExecCount", move |_, a| {
        let gm = g.borrow();
        let n = trigger_of(&gm, a).map_or(0, |t| trigger_ref(&gm, t).execs);
        NativeResult::Return(Value::Integer(n))
    });

    let g = Rc::clone(game);
    reg.register("GetTriggeringTrigger", move |ctx, _| {
        let gm = g.borrow();
        let id = match gm.context(ctx.thread_key()) {
            Some(Ctx::Trigger { trigger, .. }) => gm.handle_of(trigger),
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetTriggerEventId", move |ctx, _| {
        let gm = g.borrow();
        let id = match gm.context(ctx.thread_key()) {
            Some(Ctx::Trigger { event: Some(e), .. }) => gm.handle_of(e),
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });
}

/// Runs the event path of trigger `t` (see the module notes): evaluation count, enable check,
/// conditions, execution count and the action chain. `event` is the event that fired, if any.
pub fn fire(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, t: ObjKey, event: Option<ObjKey>) {
    let enabled = {
        let mut gm = game.borrow_mut();
        if !gm.is_alive(t) {
            return;
        }
        let Object::Trigger(tr) = gm.object_mut(t) else {
            return;
        };
        tr.evals = tr.evals.wrapping_add(1);
        tr.enabled
    };
    if !enabled {
        return;
    }
    let key = ctx_key(game, t, event);
    if eval_conditions(game, rt, t, key) {
        execute(game, t, event, rt);
    }
}

/// The condition result of trigger `t` in context `key`: AND of all conditions, every one is called.
fn eval_conditions(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, t: ObjKey, key: u32) -> bool {
    let exprs: Vec<ObjKey> = {
        let gm = game.borrow();
        trigger_ref(&gm, t)
            .conditions
            .iter()
            .filter_map(|&c| match gm.object(c) {
                Object::Condition(cd) => Some(cd.boolexpr),
                _ => None,
            })
            .collect()
    };
    let mut all = true;
    for b in exprs {
        all &= eval_boolexpr(game, rt, b, key);
    }
    all
}

/// Runs the actions of trigger `t` as one chain and counts an execution.
fn execute(game: &RefCell<Game>, t: ObjKey, event: Option<ObjKey>, rt: &mut dyn ScriptRuntime) {
    let (chain, key) = {
        let mut gm = game.borrow_mut();
        let codes: Vec<u32> = trigger_ref(&gm, t)
            .actions
            .iter()
            .filter_map(|&a| match gm.object(a) {
                Object::Action(ac) => Some(ac.code),
                _ => None,
            })
            .collect();
        if let Object::Trigger(tr) = gm.object_mut(t) {
            tr.execs = tr.execs.wrapping_add(1);
        }
        let key = gm.context_key(Ctx::Trigger { trigger: t, event });
        (codes, key)
    };
    if !chain.is_empty() {
        rt.chain_keyed(chain, key);
    }
}

/// Evaluates one boolexpr. A leaf calls its code. `And` and `Or` evaluate both sides (no early exit,
/// like the condition list; UNSURE for the original's and/or). A failed call counts as false.
pub(super) fn eval_boolexpr(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    b: ObjKey,
    key: u32,
) -> bool {
    let kind = match game.borrow().object(b) {
        Object::BoolExpr(k) => *k,
        _ => return false,
    };
    match kind {
        BoolExpr::Leaf(code) => {
            matches!(rt.call_keyed(code, key), Ok(v) if v.as_boolean() == Some(true))
        }
        BoolExpr::And(x, y) => {
            let a = eval_boolexpr(game, rt, x, key);
            let c = eval_boolexpr(game, rt, y, key);
            a && c
        }
        BoolExpr::Or(x, y) => {
            let a = eval_boolexpr(game, rt, x, key);
            let c = eval_boolexpr(game, rt, y, key);
            a || c
        }
        BoolExpr::Not(x) => !eval_boolexpr(game, rt, x, key),
    }
}

fn destroy(gm: &mut Game, t: ObjKey) {
    let (conds, actions, events) = {
        let tr = trigger_ref(gm, t);
        (tr.conditions.clone(), tr.actions.clone(), tr.events.clone())
    };
    for ev in events {
        let source = match gm.object(ev) {
            Object::Event(e) => e.source,
            _ => continue,
        };
        match source {
            EventSource::Timer(Some(id)) => {
                gm.timers.cancel(id);
            }
            EventSource::Timer(None) => {}
            EventSource::Expire(timer) => {
                if let Object::Timer(tm) = gm.object_mut(timer) {
                    tm.listeners.retain(|&(trig, _)| trig != t);
                }
            }
        }
        gm.kill(ev);
    }
    // Expire listeners are stored on the timer, so every timer is searched.
    for i in 0..gm.object_count() {
        let key = ObjKey(i as u32);
        if let Object::Timer(tm) = gm.object_mut(key) {
            tm.listeners.retain(|&(trig, _)| trig != t);
        }
    }
    for c in conds.into_iter().chain(actions) {
        gm.kill(c);
    }
    gm.kill(t);
}

/// The context key for trigger `t` (with `event`), interned in `game`.
fn ctx_key(game: &RefCell<Game>, t: ObjKey, event: Option<ObjKey>) -> u32 {
    game.borrow_mut()
        .context_key(Ctx::Trigger { trigger: t, event })
}

/// Adds a new boolexpr object with a handle (flag 0) and returns its id.
fn add_boolexpr(gm: &mut Game, b: BoolExpr) -> u32 {
    gm.add_object(Object::BoolExpr(b), 0).1
}

fn new_leaf(g: &Rc<RefCell<Game>>, a: &[Value]) -> NativeResult {
    let Some(code) = a.first().and_then(code_of) else {
        return NativeResult::Return(Value::Handle(0));
    };
    let id = add_boolexpr(&mut g.borrow_mut(), BoolExpr::Leaf(code));
    NativeResult::Return(Value::Handle(id))
}

fn combine(
    g: &Rc<RefCell<Game>>,
    a: &[Value],
    make: impl Fn(ObjKey, ObjKey) -> BoolExpr,
) -> NativeResult {
    let mut gm = g.borrow_mut();
    let (Some(x), Some(y)) = (boolexpr_of(&gm, &a[..1]), boolexpr_of(&gm, &a[1..2])) else {
        return NativeResult::Return(Value::Handle(0));
    };
    let id = add_boolexpr(&mut gm, make(x, y));
    NativeResult::Return(Value::Handle(id))
}

/// Adds `id` (an object of this trigger's list) to the list chosen by `list`.
fn push_list(
    gm: &mut Game,
    t: ObjKey,
    list: impl Fn(&mut TriggerObj) -> &mut Vec<ObjKey>,
    id: u32,
) {
    let Some(key) = gm.handle_object(id) else {
        return;
    };
    if let Object::Trigger(tr) = gm.object_mut(t) {
        list(tr).push(key);
    }
}

fn list_mut(
    gm: &mut Game,
    t: ObjKey,
    list: impl Fn(&mut TriggerObj) -> &mut Vec<ObjKey>,
) -> &mut Vec<ObjKey> {
    match gm.object_mut(t) {
        Object::Trigger(tr) => list(tr),
        _ => unreachable!("list_mut on a non-trigger"),
    }
}

/// Removes `item` from the trigger's list and releases its handle.
fn remove_item(
    gm: &mut Game,
    t: ObjKey,
    item: ObjKey,
    list: impl Fn(&mut TriggerObj) -> &mut Vec<ObjKey>,
) {
    let l = list_mut(gm, t, list);
    if let Some(i) = l.iter().position(|&k| k == item) {
        l.remove(i);
        gm.kill(item);
    }
}

fn trigger_of(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    typed_handle(gm, a.first()?, |o| matches!(o, Object::Trigger(_)))
}

fn trigger_ref(gm: &Game, t: ObjKey) -> &TriggerObj {
    match gm.object(t) {
        Object::Trigger(tr) => tr,
        _ => unreachable!("trigger_ref on a non-trigger key"),
    }
}

fn timer_arg(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    typed_handle(gm, a.get(1)?, |o| matches!(o, Object::Timer(_)))
}

fn boolexpr_of(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    typed_handle(gm, a.first()?, |o| matches!(o, Object::BoolExpr(_)))
}

/// A condition or action of trigger `t` given as argument `i`, if it belongs to `t`.
fn item_of(
    gm: &Game,
    a: &[Value],
    i: usize,
    kind: fn(&Object) -> bool,
    t: ObjKey,
) -> Option<ObjKey> {
    let k = typed_handle(gm, a.get(i)?, kind)?;
    let owner = match gm.object(k) {
        Object::Condition(c) => c.trigger,
        Object::Action(ac) => ac.trigger,
        _ => return None,
    };
    (owner == t).then_some(k)
}

fn is_condition(o: &Object) -> bool {
    matches!(o, Object::Condition(_))
}

fn is_action(o: &Object) -> bool {
    matches!(o, Object::Action(_))
}

/// The object of a handle value when it is live and its kind matches.
fn typed_handle(gm: &Game, v: &Value, kind: impl Fn(&Object) -> bool) -> Option<ObjKey> {
    let id = v.as_handle()?;
    let k = gm.handle_object(id)?;
    kind(gm.object(k)).then_some(k)
}

fn code_of(v: &Value) -> Option<u32> {
    match v {
        Value::Code(f) => Some(*f),
        _ => None,
    }
}
