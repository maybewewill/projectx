//! Wander (`Awan`, `CAbilityWander`, vtable `0x6F8E4EC4`): the random walk of critters (pigs, sheep, raccoons, ...).
//!
//! Source: the machine code of the class, read for docs/spec/lockstep.md section 20 (R3). The class takes the **global
//! generator** for its delay and **stream 8** for the offsets, so every wander cycle moves both hidden words of the sync
//! value (the generator hash `R` and the rand slot); the simulation had neither (the replay shows about one draw per
//! second of game time that the sim lacked).
//!
//! ```text
//! arm   (0x6F159420, called on messages 0xD0148 and 0xD0178 when the unit is idle):
//!           delay = 5.0 + 5.0 * U             U = one step of G (0x6F170D60, the GetRandomReal arithmetic, lo 0x6FAB1D7C = 5.0,
//!                                                hi 0x6FAB1D80 = 10.0); a dead unit takes 0.01 (0x6FAAE480) without a draw
//!           if 0.01 > delay: delay = 0.01      timer on the ability's own timer object, message 0xD01BF
//! fire  (0x6F162960, message 0xD01BF):
//!           nothing when the unit has a current order;
//!           x' = x + (500.0 * U1 - 250.0),  y' = y + (500.0 * U2 - 250.0)      U1, U2 = two steps of stream 8 (0x6F170DF0)
//!           clamp into the playable area, move order 0xD0012 to (x', y')        (0x6F294D40 / 0x6F2A4AB0)
//! ```
//!
//! The creation arm is UNSURE: the class registers for the idle messages `0xD0148` / `0xD014C` at `0x6F1593A0`, and a
//! new unit is idle, so the model arms once when the unit gets the ability and again whenever its order list empties.
//! The playable clamp of the fire (`0x6F00F560`) is taken as the SetUnitPosition clamp (UNSURE; the offsets are at most
//! 250 units, so it matters only at the map edge). A dead unit's branch (offsets 2000 * U - 1000) is not modelled.

use std::cell::RefCell;

use softfloat::{arith, F32};

use crate::abilities::code;
use crate::cast::SpellMsg;
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::ScriptRuntime;
use crate::orders::{Order, OrderClass, ORDER_MOVE};

/// `'Awan'`.
pub const AWAN: u32 = code(b"Awan");
/// The timer kind of the module (inside the `0x60..0x70` range that `neutral_heroes2::on_timer` serves).
pub const KIND_WANDER: u8 = 0x6E;

/// The stream of the offsets (`mov edx, 8` before `0x6F2853D0`, CONFIRMED by the disassembly of `0x6F170DF0`).
pub const WANDER_STREAM: usize = 8;

/// Delay draws when the unit gets the ability: the class arms at its own init (`0x6F162820`, 1) and the new unit's idle
/// notifications `0xD0148` and `0xD0178` each arm once more (`0x6F1628F0`, `0x6F162930`). UNSURE as a count, FITTED:
/// docs/spec/lockstep.md section 21 (the replay has 28 global draws by record 32; 3 per critter is the number that
/// brings the sim's cumulative count to 26/28 there, 103/104 at record 700).
pub const CREATE_ARMS: usize = 3;
/// Delay draws when the order list empties: the notifications `0xD0148` and `0xD0178` both arm (the ability has ONE
/// embedded timer at `+0x6C`, so the second arm replaces the first timer but still draws). FITTED on the burst of 12
/// draws (6 critters x 2) in records 103 to 149 of the replay.
pub const IDLE_ARMS: usize = 2;

const LO: u32 = 0x40A0_0000; // 5.0   (0x6FAB1D7C after start-up)
const HI: u32 = 0x4120_0000; // 10.0  (0x6FAB1D80)
const MIN_DELAY: u32 = 0x3C23_D70A; // 0.01 (0x6FAAE480)
const SPAN: u32 = 0x43FA_0000; // 500.0 (0x6FAAE59C)
const HALF: u32 = 0x437A_0000; // 250.0 (0x6FAAE588)

fn f(bits: u32) -> F32 {
    F32::from_bits(bits)
}

/// Gives or takes the wander timer with the ability (called from `sync_neutral_spells`).
pub(crate) fn sync(gm: &mut Game, unit: ObjKey) {
    let has = gm.unit(unit).abilities.get(AWAN).is_some_and(|a| !a.is_removed());
    let tracked = gm.spells.neutral.h2.misc.wander.contains_key(&unit);
    if has && !tracked {
        // The entry exists from now on, with or without a running timer.
        gm.spells.neutral.h2.misc.wander.insert(unit, None);
        let n = CREATE_ARMS;
        for _ in 0..n {
            arm(gm, unit);
        }
    } else if !has && tracked {
        if let Some(Some(id)) = gm.spells.neutral.h2.misc.wander.remove(&unit) {
            gm.timers.cancel(id);
        }
    }
}

/// The unit's order list emptied (the idle messages `0xD0148` / `0xD0178`): arm again.
pub(crate) fn on_idle(gm: &mut Game, unit: ObjKey) {
    if gm.spells.neutral.h2.misc.wander.contains_key(&unit) && gm.alive(unit) {
        let n = IDLE_ARMS;
        for _ in 0..n {
            arm(gm, unit);
        }
    }
}

/// `0x6F159420`: picks the delay and starts the timer (a running one is replaced).
fn arm(gm: &mut Game, unit: ObjKey) {
    if !gm.alive(unit) {
        return;
    }
    let Some((table, streams)) = gm.rng_parts() else {
        return;
    };
    crate::rng::trace::note(|| "wander delay".into());
    let delay = streams.global.random_real_soft(table, f(LO), f(HI));
    let delay = if arith::lt(delay, f(MIN_DELAY)) { f(MIN_DELAY) } else { delay };
    // One embedded timer per ability (`+0x6C`): a new arm replaces the running one.
    if let Some(Some(old)) = gm.spells.neutral.h2.misc.wander.get(&unit).copied() {
        gm.timers.cancel(old);
    }
    let id = gm.timers.start(
        delay.to_f32(),
        false,
        Payload::Spell(SpellMsg::UnitSpell { kind: KIND_WANDER, unit, base: AWAN, aux: 0 }),
    );
    gm.spells.neutral.h2.misc.wander.insert(unit, Some(id));
}

/// The timer (message `0xD01BF`): one step to a random point nearby, when the unit has nothing else to do.
pub(crate) fn on_timer(game: &RefCell<Game>, _rt: &mut dyn ScriptRuntime, unit: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.spells.neutral.h2.misc.wander.contains_key(&unit) || !gm.alive(unit) {
        return;
    }
    gm.spells.neutral.h2.misc.wander.insert(unit, None);
    if gm.order_head(unit).is_some() {
        return;
    }
    let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
    let Some((table, streams)) = gm.rng_parts() else {
        return;
    };
    let s = streams.stream_mut(WANDER_STREAM);
    let u1 = s.random_fraction(table);
    let u2 = s.random_fraction(table);
    let nx = arith::add(x, arith::sub(arith::mul(f(SPAN), u1), f(HALF)));
    let ny = arith::add(y, arith::sub(arith::mul(f(SPAN), u2), f(HALF)));
    let (nx, ny) = gm.clamp_unit_move(nx, ny);
    let order = Order { id: ORDER_MOVE, class: OrderClass::Point, point: Some((nx, ny)), target: 0 };
    gm.issue_order(unit, order, true);
}
