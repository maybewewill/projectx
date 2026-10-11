//! Ability and buff natives: `SetUnitInvulnerable`, `UnitApplyTimedLife`, `UnitPauseTimedLife`, `UnitHasBuffsEx`,
//! `UnitCountBuffsEx`, `UnitRemoveBuffs` and `UnitRemoveBuffsEx`, and the expiry of a timed life buff.
//!
//! The model is in `crate::abilities` (the notes there apply here). Addresses are the 1.26a `Game.dll` natives of
//! `docs/spec/abilities.md`: `SetUnitInvulnerable` 0x6F3C72F0, `UnitApplyTimedLife` 0x6F3C85D0, `UnitPauseTimedLife`
//! 0x6F3C8750, the buff natives 0x6F3C8360 to 0x6F3C8410. Labels: CONFIRMED = read in the spec's notes, LIKELY = strong
//! inference, UNSURE = guessed or not read.
//!
//! * Handle gate (CONFIRMED): an invalid or removed unit gives the native's default (nothing, `0` or `false`).
//! * `SetUnitInvulnerable` (CONFIRMED structure): adds or removes the ability `Avul` (see `Game::set_unit_invulnerable`).
//! * `UnitApplyTimedLife(unit, id, duration)` (CONFIRMED dispatch): adds the timed life buff `id` with a duration timer
//!   from the current queue time. The seven special ids and `BTLF` use the same timer here (UNSURE: the special ids'
//!   classes are not read, so their duration behaviour may differ).
//! * Timed life expiry (UNSURE, the spec only says the unit dies): when the timer fires, the buff is removed and the
//!   unit gets the life-zero path of `KillUnit` (the same `set_life` call and death dispatch). A removed unit is not
//!   killed. The expiry runs at the timer's expiry time, which is the game time during the callback.
//! * `UnitPauseTimedLife(unit, flag)` (LIKELY from the spec's slots 58 and 59): `true` pauses the running timed life
//!   timers and keeps the time left; `false` resumes them with that time.
//! * `UnitHasBuffsEx`, `UnitCountBuffsEx`, `UnitRemoveBuffsEx` take the seven selection booleans in WC3 order (CONFIRMED,
//!   see `BuffSelect`). `UnitRemoveBuffs(unit, removePositive, removeNegative)` uses the fixed filters the spec gives
//!   for it (timed life and aura included, magic, physical and dispellable not checked; CONFIRMED).
//! * Removing a buff cancels its duration timer, so a removed timed life buff does not kill the unit (UNSURE: the
//!   original's removal path is not read in full).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, ObjKey, ScriptRuntime};
use super::player::raw_at;
use super::unit::set_life;
use super::unit_attr::{bool_arg, real_at, unit_at};
use super::unit_event::dispatch_unit_death;
use crate::abilities::BuffSelect;

/// Registers the ability and buff natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("SetUnitInvulnerable", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.set_unit_invulnerable(k, bool_arg(a, 1));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitApplyTimedLife", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.apply_timed_life(k, raw_at(a, 1) as u32, real_at(a, 2));
        }
        NativeResult::Return(Value::Null)
    });

    // UnitResetCooldown: a no-op in the simulation (CONFIRMED by disassembly, abilities-2.md section 1.2). The
    // original only refreshes the local player's cooldown display.
    reg.register("UnitResetCooldown", |_, _| {
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitPauseTimedLife", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.set_timed_life_paused(k, bool_arg(a, 1));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitHasBuffsEx", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).is_some_and(|k| gm.count_buffs(k, select_ex(a)) > 0);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("UnitCountBuffsEx", move |_, a| {
        let gm = g.borrow();
        let n = unit_at(&gm, a, 0).map_or(0, |k| gm.count_buffs(k, select_ex(a)));
        NativeResult::Return(Value::Integer(i32::try_from(n).unwrap_or(i32::MAX)))
    });

    let g = Rc::clone(game);
    reg.register("UnitRemoveBuffs", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            let select = BuffSelect {
                remove_positive: bool_arg(a, 1),
                remove_negative: bool_arg(a, 2),
                magic: false,
                physical: false,
                timed_life: true,
                aura: true,
                dispellable: false,
            };
            gm.remove_buffs(k, select);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitRemoveBuffsEx", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.remove_buffs(k, select_ex(a));
        }
        NativeResult::Return(Value::Null)
    });
}

/// The seven selection booleans of the `Ex` natives, arguments 1 to 7 (after the unit).
fn select_ex(a: &[Value]) -> BuffSelect {
    BuffSelect {
        remove_positive: bool_arg(a, 1),
        remove_negative: bool_arg(a, 2),
        magic: bool_arg(a, 3),
        physical: bool_arg(a, 4),
        timed_life: bool_arg(a, 5),
        aura: bool_arg(a, 6),
        dispellable: bool_arg(a, 7),
    }
}

/// The timed life buff `serial` of unit `unit` has run out (the timer payload of `Payload::TimedLife`).
///
/// The buff is removed (CONFIRMED path: the handler of message `0xD01C4` calls the removal routine,
/// `docs/spec/abilities-2.md` section 3), and the unit's life is set to zero through the `KillUnit` path: the life
/// change, then the death dispatch when the unit was alive. The death step is UNSURE: the removal notify of the
/// timed life class does not call a kill itself (abilities-2.md section 3.3). Called by the host after its clock reaches the expiry, with the game time
/// set to that expiry. A removed unit, or a buff that is no longer on the unit, does nothing.
pub fn expire_timed_life(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    serial: u32,
) {
    let died = {
        let mut gm = game.borrow_mut();
        if gm.unit(unit).removed {
            return;
        }
        let buffs = &mut gm.unit_mut(unit).buffs;
        let Some(i) = buffs.iter().position(|b| b.serial == serial) else {
            return;
        };
        buffs.remove(i);
        set_life(&mut gm, unit, F32::ZERO)
    };
    if died {
        dispatch_unit_death(game, rt, unit);
    }
}
