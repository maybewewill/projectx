//! Random natives: `GetRandomInt`, `GetRandomReal` and `SetRandomSeed`, on the global generator
//! and the 45 streams of [`crate::rng`].
//!
//! Behaviour notes (addresses are the 1.26a `Game.dll` natives; labels as in `timer.rs`).
//!
//! * `GetRandomInt(lo, hi)` (0x6F3B3560): one draw from the global generator. Equal bounds return
//!   `lo` without a draw; `lo > hi` runs upward from `lo` (`rng.md` 3.1). CONFIRMED.
//! * `GetRandomReal(a, b)` (0x6F3B35A0): always one draw, the range is `|b - a|` in soft-float, the
//!   result is `a + range * U` (`rng.md` 3.3). CONFIRMED structure; the constants of the helpers are
//!   LIKELY (`rng.md` section 9).
//! * `SetRandomSeed(s)` (0x6F3B36D0): reseeds the global generator and all 45 streams (`rng.md` 4.3).
//!   CONFIRMED.
//! * Without the RNG table (no `Game.dll` given to the host) all three stop the calling thread with
//!   `RunStatus::MissingTables`, like the other table-based natives. The table is not reconstructed.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, RunStatus, Value};

use super::game::Game;
use super::hashtable::{int_at, real_at};

/// Registers the random natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("GetRandomInt", move |_, a| {
        let (lo, hi) = (int_at(a, 0), int_at(a, 1));
        let mut gm = g.borrow_mut();
        match gm.rng_parts() {
            Some((table, streams)) => {
                crate::rng::trace::note(|| format!("GetRandomInt({lo},{hi})"));
                NativeResult::Return(Value::Integer(streams.global.random_int(table, lo, hi)))
            }
            None => NativeResult::Fail(RunStatus::MissingTables),
        }
    });

    let g = Rc::clone(game);
    reg.register("GetRandomReal", move |_, a| {
        let (lo, hi) = (real_at(a, 0), real_at(a, 1));
        let mut gm = g.borrow_mut();
        match gm.rng_parts() {
            Some((table, streams)) => {
                crate::rng::trace::note(|| "GetRandomReal".to_string());
                let r = streams.global.random_real_soft(table, lo, hi);
                NativeResult::Return(Value::Real(r))
            }
            None => NativeResult::Fail(RunStatus::MissingTables),
        }
    });

    let g = Rc::clone(game);
    reg.register("SetRandomSeed", move |_, a| {
        let seed = int_at(a, 0) as u32;
        let mut gm = g.borrow_mut();
        match gm.rng_parts() {
            Some((table, streams)) => {
                streams.set_random_seed(table, seed);
                NativeResult::Return(Value::Null)
            }
            None => NativeResult::Fail(RunStatus::MissingTables),
        }
    });
}
