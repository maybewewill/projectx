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

use jass::vm::{NativeRegistry, NativeResult};
use softfloat::F32;

mod ability;
pub mod ai;
pub mod animation;
pub mod defeat;
mod destructable;
pub mod economy;
pub mod event;
pub mod fog;
mod force;
pub mod game;
mod gamecache;
pub mod gamestate;
pub(crate) mod group;
mod hashtable;
pub mod hero;
pub mod item;
pub mod lane_e;
pub mod lane_f;
mod location;
pub mod melee_start;
pub mod misc;
pub mod object;
pub mod order;
pub mod pending;
mod player;
pub mod pool;
pub mod presentation;
pub mod shop;
mod region;
mod rng;
mod timer;
mod trigger;
pub(crate) mod unit;
pub mod unit_attr;
mod unit_event;

pub use ability::expire_timed_life;
pub use game::{
    Ctx, Game, HandleKind, ObjKey, Payload, PickRecord, Playable, ScriptRuntime, UnitEvent, UnitObj, UnitType,
    UnitTypes,
};
pub(crate) use group::circle_candidates;
pub use presentation::{PresentationEvent, PresentationLog, PresentationState};
pub use timer::expire as expire_timer;
pub use trigger::fire as fire_trigger;
pub(crate) use unit::set_life;
pub(crate) use unit_event::{dispatch_unit_death_by, fire_observers_in, KEY_BASE};

/// Registers the sleep, timer and trigger natives on `reg`, bound to `game`.
///
/// `TriggerSleepAction` is replaced: the wake time is also queued in `game.timers`, so the host can
/// resume the thread at that time (CONFIRMED structure: sleeps are timers, game-loop-2 section 4.5).
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("TriggerSleepAction", move |ctx, a| {
        let seconds = crate::timer::sleep_seconds(a.first().and_then(|v| v.as_real()).unwrap_or(F32::ZERO));
        let wake = ctx.wake_time(seconds);
        g.borrow_mut()
            .timers
            .start_at(wake.to_f32(), Payload::Sleep(ctx.thread()));
        NativeResult::Sleep(seconds)
    });
    timer::register(reg, game);
    trigger::register(reg, game);
    lane_e::register_sleeping(reg, game);
    lane_f::register_sleeping(reg, game);
    register_world(reg, game);
}

/// Registers the world natives that do not queue timers or triggers: hashtables, game caches, locations, players,
/// forces, the random natives, presentation, units, groups, items, destructables, orders and the rest. The main VM
/// gets them from [`register`]; the AI VMs (`ai.rs`) get them without the timer and trigger natives, because their
/// expiries run on the main host's queue.
pub fn register_world(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    hashtable::register(reg, game);
    gamecache::register(reg, game);
    location::register(reg, game);
    player::register(reg, game);
    force::register(reg, game);
    rng::register(reg, game);
    presentation::register(reg, game);
    unit::register(reg, game);
    unit_event::register(reg, game);
    event::register(reg, game);
    region::register(reg, game);
    group::register(reg, game);
    item::register(reg, game);
    destructable::register(reg, game);
    unit_attr::register(reg, game);
    ability::register(reg, game);
    hero::register(reg, game);
    gamestate::register(reg, game);
    fog::register(reg, game);
    misc::register(reg, game);
    order::register(reg, game);
    pool::register(reg, game);
    defeat::register(reg, game);
    economy::register(reg, game);
    object::register(reg, game);
    animation::register(reg, game);
    shop::register(reg, game);
    melee_start::register(reg, game);
    lane_e::register(reg, game);
    lane_f::register(reg, game);
}
