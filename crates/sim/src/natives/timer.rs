//! Timer natives: `CreateTimer`, `DestroyTimer`, `TimerStart`, the getters, `PauseTimer`,
//! `ResumeTimer` and `GetExpiredTimer`, and the expiry of a timer (handler, then listeners).
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions read in `natives.c` and the decompile
//! chunks (the scratchpad, not in the repo). Labels: CONFIRMED = read directly, LIKELY = strong
//! inference from the code shape, UNSURE = guessed or not resolved.
//!
//! * `CreateTimer` (0x6F3D0500): a new timer with a handle (allocator flag 1). It is not running,
//!   its timeout is 0 and its remaining time is 0. CONFIRMED.
//! * Every timer native first resolves the handle (0x6F3BD710). A null, freed or destroyed handle
//!   does nothing, and the getters return 0. CONFIRMED.
//! * `DestroyTimer` (0x6F3C1350): the handle's object gets its death call (virtual slot 0x5C). The
//!   running queue entry is cancelled and the handle is released, so the id may be reused (LIFO,
//!   handles-2 section 5). LIKELY for the cancel, CONFIRMED for the death call.
//! * `TimerStart(t, timeout, periodic, handler)` (0x6F3C1370 -> 0x6F3F6890): the timeout is stored
//!   raw (CONFIRMED by the store through the object's slot 1), the handler is registered, then message
//!   0x80204 starts the agent timer (0x6F477FF0). A running timer is cancelled first (0x6F477D30 marks
//!   the old request cancelled). The expiry is `queue_now + clamp(timeout)`, where the clamp turns
//!   negative values into 0 (0x6F477F80, CONFIRMED). The periodic flag re-arms the timer with the same
//!   clamped timeout as the period (LIKELY). The queue's FIFO tie-break is the global insertion order
//!   (UNSURE; the per-timer counter at +0x50 might be the tie-break key, see game-loop-2 3.4).
//! * `TimerGetTimeout` (0x6F3C13E0): the stored raw timeout. CONFIRMED.
//! * `TimerGetRemaining` (0x6F3C13C0 -> 0x6F3E8E30 -> slot 6 at 0x6F4779F0): while the timer runs,
//!   `expiry - now` (soft-float sub, CONFIRMED that the sub is applied to the request's expiry and the
//!   queue's time at +0x40). While paused, expired or never started, the stored paused value (0 at
//!   the start). LIKELY.
//! * `TimerGetElapsed` (0x6F3C13A0 -> 0x6F3ECD30): `timeout - remaining`, both soft-float. While the
//!   timer runs the timeout is the clamped request value; while inactive it is the raw timeout. The
//!   operand order is inferred from the sub helper's argument order (LIKELY). After a one-shot expiry
//!   the elapsed time is the whole timeout (LIKELY, follows from the paused value 0).
//! * `PauseTimer` (0x6F3C1400 -> 0x6F3E8F00): stores the remaining time as the paused value and
//!   cancels the queue entry (0x6F477D30). A timer that is not running is left as it is. CONFIRMED
//!   for the call order, LIKELY for the store.
//! * `ResumeTimer` (0x6F3C1420 -> 0x6F3E8F30): when the timer does not run, starts it with the paused
//!   value as its timeout (message 0x80272, periodic flag from the stored timer flags). A running
//!   timer is not affected. CONFIRMED for the condition, LIKELY for the value used. Consequence (UNSURE):
//!   a one-shot timer that has expired, or was never started, restarts with 0 and fires at the next
//!   step.
//! * Expiry (0x6F3E8F30 path and the pump 0x6F46E370): the expiry is delivered to the handler, then to
//!   each trigger that listens to the timer, in registration order (LIKELY for the listener order; the
//!   handler-before-listeners order is UNSURE). A one-shot timer stops running and its paused value
//!   becomes 0. A periodic timer re-arms at `expiry + period`, and its handler runs at every expiry.
//! * `GetExpiredTimer` (0x6F3C1D80): the timer whose handler runs in the calling thread's context, or
//!   the null handle outside an expiry (LIKELY). Within a thread that a sleep resumed, the context is
//!   kept (UNSURE that the original does the same).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{Ctx, Game, ObjKey, Object, Payload, TimerObj};
use super::ScriptRuntime;

/// Registers the timer natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateTimer", move |_, _| {
        let mut gm = g.borrow_mut();
        let (_, id) = gm.add_object(Object::Timer(TimerObj::new()), 1);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("DestroyTimer", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = timer_of(&gm, a) {
            cancel(&mut gm, k);
            gm.kill(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("TimerStart", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(k) = timer_of(&gm, a) else {
            return NativeResult::Return(Value::Null);
        };
        let timeout = real_arg(a, 1);
        let periodic = a.get(2).and_then(|v| v.as_boolean()).unwrap_or(false);
        let handler = code_arg(a, 3);
        cancel(&mut gm, k);
        let id = gm
            .timers
            .start(timeout.to_f32(), periodic, Payload::Timer(k));
        if let Object::Timer(t) = gm.object_mut(k) {
            t.timeout = timeout;
            t.periodic = periodic;
            t.handler = handler;
            t.active = Some(id);
            t.paused_remaining = F32::ZERO;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("TimerGetTimeout", move |_, a| {
        let gm = g.borrow();
        let v = timer_of(&gm, a).map_or(F32::ZERO, |k| timer_ref(&gm, k).timeout);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("TimerGetElapsed", move |_, a| {
        let gm = g.borrow();
        let v = timer_of(&gm, a).map_or(F32::ZERO, |k| elapsed(&gm, timer_ref(&gm, k)));
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("TimerGetRemaining", move |_, a| {
        let gm = g.borrow();
        let v = timer_of(&gm, a).map_or(F32::ZERO, |k| remaining(&gm, timer_ref(&gm, k)));
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("PauseTimer", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = timer_of(&gm, a) {
            if timer_ref(&gm, k).active.is_some() {
                let rem = remaining(&gm, timer_ref(&gm, k));
                cancel(&mut gm, k);
                if let Object::Timer(t) = gm.object_mut(k) {
                    t.paused_remaining = rem;
                }
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ResumeTimer", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = timer_of(&gm, a) {
            if timer_ref(&gm, k).active.is_none() {
                let t = timer_ref(&gm, k);
                let (rem, periodic) = (t.paused_remaining, t.periodic);
                let id = gm.timers.start(rem.to_f32(), periodic, Payload::Timer(k));
                if let Object::Timer(t) = gm.object_mut(k) {
                    t.active = Some(id);
                }
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetExpiredTimer", move |ctx, _| {
        let gm = g.borrow();
        let id = match gm.context(ctx.thread_key()) {
            Some(Ctx::Timer(k)) => gm.handle_of(k),
            _ => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });
}

/// Runs the expiry of timer `k` at the queue's current time: the handler, then the listening triggers.
/// `rt` runs the script code. The queue must not be borrowed by the caller.
pub fn expire(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, k: ObjKey) {
    let (handler, listeners, key) = {
        let mut gm = game.borrow_mut();
        let Object::Timer(t) = gm.object_mut(k) else {
            return;
        };
        if !t.periodic {
            t.active = None;
            t.paused_remaining = F32::ZERO;
        }
        let (handler, listeners) = (t.handler, t.listeners.clone());
        let key = gm.context_key(Ctx::Timer(k));
        (handler, listeners, key)
    };
    if let Some(code) = handler {
        rt.chain_keyed(vec![code], key);
    }
    for (trigger, event) in listeners {
        super::trigger::fire(game, rt, trigger, Some(event));
    }
}

/// Cancels the queue entry of a running timer (if any) and marks it not running.
fn cancel(gm: &mut Game, k: ObjKey) {
    let active = match gm.object_mut(k) {
        Object::Timer(t) => t.active.take(),
        _ => None,
    };
    if let Some(id) = active {
        gm.timers.cancel(id);
    }
}

/// The timer object behind the first argument (a handle), if it is a live timer.
fn timer_of(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    let id = a.first()?.as_handle()?;
    let k = gm.handle_object(id)?;
    matches!(gm.object(k), Object::Timer(_)).then_some(k)
}

fn timer_ref(gm: &Game, k: ObjKey) -> &TimerObj {
    match gm.object(k) {
        Object::Timer(t) => t,
        _ => unreachable!("timer_ref on a non-timer key"),
    }
}

/// Seconds left on a timer. A running timer's expiry minus the queue time; otherwise the paused value.
fn remaining(gm: &Game, t: &TimerObj) -> F32 {
    let expiry = t.active.and_then(|id| gm.timers.expiry_of(id));
    match expiry {
        Some(e) if e.is_infinite() => F32::from_bits(f32::INFINITY.to_bits()),
        Some(e) => arith::sub(
            F32::from_bits(e.to_bits()),
            F32::from_bits(gm.timers.now().to_bits()),
        ),
        None => t.paused_remaining,
    }
}

/// Seconds since the timer started (or since its last expiry, when periodic): `timeout - remaining`.
fn elapsed(gm: &Game, t: &TimerObj) -> F32 {
    let rem = remaining(gm, t);
    if t.active.is_some() {
        let timeout = if arith::lt(t.timeout, F32::ZERO) {
            F32::ZERO
        } else {
            t.timeout
        };
        arith::sub(timeout, rem)
    } else {
        arith::sub(t.timeout, t.paused_remaining)
    }
}

fn real_arg(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

/// A code argument: `None` for the null code.
fn code_arg(a: &[Value], i: usize) -> Option<u32> {
    match a.get(i) {
        Some(Value::Code(f)) => Some(*f),
        _ => None,
    }
}
