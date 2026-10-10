//! Force natives: `CreateForce`, `DestroyForce`, `ForceAddPlayer`, `ForceRemovePlayer`, `ForceClear`,
//! `ForceEnumPlayers`, `ForceEnumPlayersCounted`, `ForceEnumAllies`, `ForceEnumEnemies`, `ForForce`,
//! `IsPlayerInForce`, `GetEnumPlayer` and `GetFilterPlayer`.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions read in `natives.c`, with the helpers
//! checked by disassembly (capstone). Labels as in `player.rs`.
//!
//! Model. A force is a 16-bit set of player slots (bit `i` = `Player(i)`), stored at `force + 0x24`
//! (CONFIRMED from `ForceClear` and the `ForForce` loop). Membership is read with the word helper each
//! time it is needed, so a change made by a script during an enumeration is seen by the rest of it.
//!
//! * `CreateForce` (0x6F3D29F0): a new empty force with its own handle (flag 0, LIKELY). CONFIRMED
//!   for the empty set (the helper clears it).
//! * `DestroyForce` (0x6F3C44C0): the object's death call (virtual slot 0x5C), so the handle is
//!   released. CONFIRMED for the call, LIKELY for the release. The set is not cleared (no handle can
//!   reach it any more).
//! * `ForceAddPlayer` / `ForceRemovePlayer` (0x6F3C44E0 / 0x6F3C4510): set / clear the player's bit. A
//!   null force or player does nothing. LIKELY (the helpers 0x6F3DFF80 / 0x6F3DFFA0 take the player
//!   object; their bodies were read as "set / clear bit of the player's index" but not traced).
//! * `ForceClear` (0x6F3C4540): the set becomes empty. CONFIRMED.
//! * `IsPlayerInForce` (0x6F3C95D0): true when the player's bit is set in the force. CONFIRMED.
//! * `ForceEnumPlayers` (0x6F3C4560), `ForceEnumPlayersCounted` (0x6F3C4590), `ForceEnumAllies`
//!   (0x6F3C45C0) and `ForceEnumEnemies` (0x6F3C4600) all run one helper (0x6F3E7250, CONFIRMED
//!   structure). The helper first clears the force, then goes over the candidate slots in ascending
//!   order. For each candidate:
//!   - the candidate must be an active slot (slot state not 0), otherwise it is skipped without a
//!     filter call;
//!   - the candidate's handle becomes `GetFilterPlayer` and the filter runs (its result is always
//!     computed, even when the relation test fails, LIKELY from the order of the calls);
//!   - for Allies the candidate is added when the candidate's own passive target set contains the
//!     source player; for Enemies when it does not. A null source adds nobody (CONFIRMED for the
//!     test, see the notes of `IsPlayerAlly`);
//!   - the candidate is added when the filter passed (or there is no filter) and the relation holds.
//!
//!   The loop stops before the next candidate once the number added reaches the limit. The limit is
//!   unsigned, so a negative count means no limit (CONFIRMED for the unsigned compare).
//!   The candidate slots are 0 to 11 only (CONFIRMED: the loop exits when the index passes 11).
//!   So players 12 to 15 are never visited by these four natives (LIKELY: a loop bound that was read,
//!   not a test that was run in the game; check it against a real game with a player in slot 12 to 15).
//! * A null filter (or a handle that is not a boolexpr) accepts every candidate (CONFIRMED: the filter
//!   is checked for null before the call).
//! * `ForForce` (0x6F3C4640 -> 0x6F3E7450, CONFIRMED structure): for each slot `0` to `15` in ascending
//!   order whose bit is set in the force at that moment, `GetEnumPlayer` becomes that player and the
//!   code runs with no arguments. Its return value is ignored. Empty slots are visited too, since
//!   this loop has no active-slot test (CONFIRMED). The call result is inspected by the original in
//!   a way that was not traced (UNSURE): here a failed call simply moves on to the next slot.
//! * `GetEnumPlayer` (0x6F3BB320) and `GetFilterPlayer` (0x6F3BB340): the value of the innermost running
//!   `ForForce` (or enumeration candidate) scope, or null outside one. CONFIRMED. The scopes are a stack
//!   (the setter pushes and the reset pops one entry, 0x6F430DB0 / 0x6F42DEE0), so a nested enumeration
//!   gives its own value and the outer one returns after it ends. Two separate stacks: a filter call
//!   does not change `GetEnumPlayer`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{ForceObj, Game, HandleKind, ObjKey, Object, ScriptRuntime};
use super::player::{alliance_bit, index_at, raw_at, PASSIVE};

/// The candidate slots of the four `ForceEnum*` natives: 0 to 11 (CONFIRMED loop bound, see above).
const ENUM_CANDIDATES: u32 = 12;
/// The slots `ForForce` visits: 0 to 15 (CONFIRMED loop bound).
const FORFORCE_SLOTS: u32 = 16;

/// Which players the enumeration accepts besides the filter.
#[derive(Clone, Copy)]
enum Relation {
    /// Every active candidate.
    Any,
    /// Candidates whose passive target set contains the source (`None`: nobody).
    Ally(Option<u32>),
    /// Candidates whose passive target set does not contain the source (`None`: nobody).
    Enemy(Option<u32>),
}

/// Registers the force natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateForce", move |_, _| {
        let (_, id) = g
            .borrow_mut()
            .add_object(Object::Force(ForceObj::default()), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("DestroyForce", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = force_at(&gm, a, 0) {
            gm.kill(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ForceAddPlayer", move |_, a| {
        let mut gm = g.borrow_mut();
        if let (Some(k), Some(p)) = (force_at(&gm, a, 0), index_at(&gm, a, 1)) {
            gm.force_mut(k).members |= 1 << p;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ForceRemovePlayer", move |_, a| {
        let mut gm = g.borrow_mut();
        if let (Some(k), Some(p)) = (force_at(&gm, a, 0), index_at(&gm, a, 1)) {
            gm.force_mut(k).members &= !(1 << p);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ForceClear", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = force_at(&gm, a, 0) {
            gm.force_mut(k).members = 0;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("IsPlayerInForce", move |_, a| {
        let gm = g.borrow();
        let v = match (index_at(&gm, a, 0), force_at(&gm, a, 1)) {
            (Some(p), Some(k)) => gm.force(k).members & (1 << p) != 0,
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("ForceEnumPlayers", move |ctx, a| {
        let (force, filter) = {
            let gm = g.borrow();
            (force_at(&gm, a, 0), boolexpr_at(&gm, a, 1))
        };
        if let Some(k) = force {
            let key = ctx.thread_key();
            enum_players(&g, ctx, key, k, Relation::Any, filter, u32::MAX);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ForceEnumPlayersCounted", move |ctx, a| {
        let (force, filter) = {
            let gm = g.borrow();
            (force_at(&gm, a, 0), boolexpr_at(&gm, a, 1))
        };
        let limit = raw_at(a, 2) as u32;
        if let Some(k) = force {
            let key = ctx.thread_key();
            enum_players(&g, ctx, key, k, Relation::Any, filter, limit);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ForceEnumAllies", move |ctx, a| {
        let (force, filter, src) = {
            let gm = g.borrow();
            (
                force_at(&gm, a, 0),
                boolexpr_at(&gm, a, 2),
                index_at(&gm, a, 1),
            )
        };
        if let Some(k) = force {
            let key = ctx.thread_key();
            enum_players(&g, ctx, key, k, Relation::Ally(src), filter, u32::MAX);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ForceEnumEnemies", move |ctx, a| {
        let (force, filter, src) = {
            let gm = g.borrow();
            (
                force_at(&gm, a, 0),
                boolexpr_at(&gm, a, 2),
                index_at(&gm, a, 1),
            )
        };
        if let Some(k) = force {
            let key = ctx.thread_key();
            enum_players(&g, ctx, key, k, Relation::Enemy(src), filter, u32::MAX);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ForForce", move |ctx, a| {
        let code = match a.get(1) {
            Some(Value::Code(f)) => *f,
            _ => return NativeResult::Return(Value::Null),
        };
        let force = {
            let gm = g.borrow();
            force_at(&gm, a, 0)
        };
        if let Some(k) = force {
            let key = ctx.thread_key();
            for_force(&g, ctx, key, k, code);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetEnumPlayer", move |_, _| {
        NativeResult::Return(Value::Handle(g.borrow().enum_player()))
    });

    let g = Rc::clone(game);
    reg.register("GetFilterPlayer", move |_, _| {
        NativeResult::Return(Value::Handle(g.borrow().filter_player()))
    });
}

/// The live force of a handle argument.
fn force_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.force_of_handle(a.get(i)?.as_handle()?)
}

/// The boolexpr of a handle argument, if it is a live boolexpr.
fn boolexpr_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.handle_of_kind(a.get(i)?.as_handle()?, HandleKind::BoolExpr)
}

/// The shared core of the `ForceEnum*` natives. Runs without a borrow of the game during each script
/// call, so the filter may use any native.
fn enum_players(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    key: u32,
    force: ObjKey,
    relation: Relation,
    filter: Option<ObjKey>,
    limit: u32,
) {
    game.borrow_mut().force_mut(force).members = 0;
    let mut count: u32 = 0;
    for i in 0..ENUM_CANDIDATES {
        if limit <= count {
            return;
        }
        if game.borrow().player(i).slot_state == 0 {
            continue;
        }
        let handle = game.borrow_mut().player_handle(i);
        game.borrow_mut().push_filter_player(handle);
        let passed = match filter {
            None => true,
            Some(b) => super::trigger::eval_boolexpr(game, rt, b, key),
        };
        game.borrow_mut().pop_filter_player();
        let related = {
            let gm = game.borrow();
            match relation {
                Relation::Any => true,
                Relation::Ally(Some(src)) => alliance_bit(&gm, i, src, PASSIVE),
                Relation::Enemy(Some(src)) => !alliance_bit(&gm, i, src, PASSIVE),
                Relation::Ally(None) | Relation::Enemy(None) => false,
            }
        };
        if passed && related {
            game.borrow_mut().force_mut(force).members |= 1 << i;
            count += 1;
        }
    }
}

/// `ForForce`: runs `code` once for each slot that is in the force, in ascending order. The membership
/// is read again before each slot.
fn for_force(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, key: u32, force: ObjKey, code: u32) {
    for i in 0..FORFORCE_SLOTS {
        let member = game.borrow().force(force).members & (1 << i) != 0;
        if !member {
            continue;
        }
        let handle = game.borrow_mut().player_handle(i);
        game.borrow_mut().push_enum_player(handle);
        let _ = rt.call_keyed(code, key);
        game.borrow_mut().pop_enum_player();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_bounds_match_the_original_loops() {
        assert_eq!(ENUM_CANDIDATES, 12);
        assert_eq!(FORFORCE_SLOTS, 16);
    }
}
