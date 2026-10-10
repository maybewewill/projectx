//! JASS natives that need game state: sleeps, timers and triggers.
//!
//! [`register`] adds them to a `jass::vm::NativeRegistry` (replacing the builtin `TriggerSleepAction`).
//! They share one [`Game`] through an `Rc<RefCell<_>>`, which the host also holds, so the host can run
//! expiries and trigger events against the same state (see `ScriptHost::step`).
//!
//! The notes for each native (addresses, CONFIRMED / LIKELY / UNSURE) are in `timer.rs`, `trigger.rs`,
//! `hashtable.rs`, `gamecache.rs` and `location.rs`. The semantics that the notes rely on are checked by the tests in
//! `tests/timer_natives.rs` and `tests/trigger_natives.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{clamp_sleep, NativeRegistry, NativeResult};
use softfloat::F32;

mod force;
pub mod game;
mod gamecache;
mod hashtable;
mod location;
mod player;
pub mod presentation;
mod rng;
mod timer;
mod trigger;

pub use game::{Ctx, Game, HandleKind, ObjKey, Payload, Playable, ScriptRuntime};
pub use presentation::{PresentationEvent, PresentationLog, PresentationState};
pub use timer::expire as expire_timer;
pub use trigger::fire as fire_trigger;

/// Registers the sleep, timer and trigger natives on `reg`, bound to `game`.
///
/// `TriggerSleepAction` is replaced: the wake time is also queued in `game.timers`, so the host can
/// resume the thread at that time (CONFIRMED structure: sleeps are timers, game-loop-2 section 4.5).
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("TriggerSleepAction", move |ctx, a| {
        let seconds = clamp_sleep(a.first().and_then(|v| v.as_real()).unwrap_or(F32::ZERO));
        let wake = ctx.wake_time(seconds);
        g.borrow_mut()
            .timers
            .start_at(wake.to_f32(), Payload::Sleep(ctx.thread()));
        NativeResult::Sleep(seconds)
    });
    timer::register(reg, game);
    trigger::register(reg, game);
    hashtable::register(reg, game);
    gamecache::register(reg, game);
    location::register(reg, game);
    player::register(reg, game);
    force::register(reg, game);
    rng::register(reg, game);
    presentation::register(reg, game);
}
