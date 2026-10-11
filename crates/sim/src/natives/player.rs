//! Player natives: `Player`, `GetLocalPlayer`, `GetPlayerId`, the name, race, controller, slot state,
//! team, start location and color getters and setters, `SetPlayerAlliance` / `GetPlayerAlliance`,
//! `IsPlayerAlly`, `IsPlayerEnemy`, `GetPlayerState` / `SetPlayerState` and `IsPlayerObserver`.
//!
//! `IsPlayerInForce` and the force natives are in `force.rs`, and `GetEnumPlayer` / `GetFilterPlayer`
//! are there too, because they belong to the force enumerations.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions read in `natives.c` and checked by
//! disassembly (capstone) of the same binary. Labels: CONFIRMED = read directly, LIKELY = strong
//! inference from the code shape, UNSURE = guessed or not resolved.
//!
//! Model. The 16 player slots are objects of the game arena (`Game::player`). The original keeps them
//! in a table of 16 pointers at the world object (`FUN_6f3a1650(i)` = `world[+0x58 + 4*i]`, no bounds
//! check there). The `Player` native is the only one that takes a raw index; it returns null for an
//! index outside 0 to 15 (UNSURE: the original reads the table without a check).
//!
//! * Handles (LIKELY). `Player(i)` calls the handle allocator (`FUN_6f430c80`) with flag 0, which
//!   returns the existing id of the slot or creates one. So the id of a slot is created by the first
//!   script access (`Player`, `GetLocalPlayer`, a `ForForce` or filter enumeration), not at map start.
//!   Whether the engine also creates the ids at map start is UNSURE (see docs/spec/handles-2.md 8.2).
//! * Player lookup from a handle (`FUN_6f3bd4d0`, CONFIRMED): the handle must be a live player; any
//!   other id (0, a freed id, another type) gives null, and every getter then returns its default.
//! * `GetLocalPlayer` (0x6F3BBB60, CONFIRMED structure): the local index is a 16-bit field of the world
//!   object (`+0x28`, or `+0x2a` when a flag at `+0x610` of a sub-object is 1). The host provides the
//!   index here. The native is desync-prone by design: each machine can answer a different player,
//!   so a script that branches on it changes the state on some clients only (the original has the
//!   same property; it is documented, not fixed).
//! * `GetPlayerId` (0x6F3C9640, CONFIRMED): the slot index, byte at `+0x30`. Null gives 0.
//! * Alliances (CONFIRMED structure). Each player keeps one 32-bit target set per alliance type,
//!   at `alliance-object + 0x38 + 16 * type`, read with the word helper `FUN_6f473170`. Bit `q` set
//!   means "this player has this type with player `q`". The relation is one-directional: setting the
//!   row of `p` does not touch the row of `q`. Types outside 0 to 9 are rejected (UNSURE).
//! * Defaults of the state that a map sets (team, start location, color, race, controller, slot state,
//!   names, player states, the self alliance): placeholders until the host fills the slot from the map.
//!   Each is marked where it is defined in `game.rs` (UNSURE).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{Game, PlayerObj, ALLIANCE_TYPES, PLAYER_STATES};

/// The alliance type that `IsPlayerAlly` and `IsPlayerEnemy` read (`ALLIANCE_PASSIVE`, 0).
pub(super) const PASSIVE: usize = 0;

/// `GetPlayerRace` of a null player: `RACE_HUMAN` (CONFIRMED).
const NULL_RACE: u32 = 1;
/// `GetPlayerController` of a null player: `MAP_CONTROL_NONE` (CONFIRMED).
const NULL_CONTROLLER: u32 = 5;

/// The largest value of a gold or lumber state word after the clamp (`0x989680`, CONFIRMED).
const GOLD_LUMBER_MAX: u32 = 0x0098_9680;

/// Registers the player natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("Player", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = match index_arg(a, 0) {
            Some(i) => gm.player_handle(i),
            None => 0,
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetLocalPlayer", move |_, _| {
        let mut gm = g.borrow_mut();
        let i = gm.local_player();
        NativeResult::Return(Value::Handle(gm.player_handle(i)))
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerId", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).map_or(0, |p| p.index as i32);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerName", move |ctx, a| {
        let name = {
            let gm = g.borrow();
            player_at(&gm, a, 0)
                .map(|p| p.name.clone())
                .unwrap_or_default()
        };
        NativeResult::Return(Value::String(ctx.intern(&name)))
    });

    let g = Rc::clone(game);
    reg.register("SetPlayerName", move |ctx, a| {
        let name = match a.get(1) {
            Some(Value::String(id)) => ctx.string(*id).to_string(),
            _ => String::new(),
        };
        let mut gm = g.borrow_mut();
        if let Some(p) = player_mut_at(&mut gm, a, 0) {
            p.name = name;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerRace", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).map_or(NULL_RACE, |p| p.race);
        NativeResult::Return(Value::Handle(v))
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerController", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).map_or(NULL_CONTROLLER, |p| p.controller);
        NativeResult::Return(Value::Handle(v))
    });

    // UNSURE: the original also calls a helper (0x6F416700) that is not traced; here only the value is set.
    let g = Rc::clone(game);
    reg.register("SetPlayerController", move |_, a| {
        let v = raw_at(a, 1) as u32;
        let mut gm = g.borrow_mut();
        if let Some(p) = player_mut_at(&mut gm, a, 0) {
            p.controller = v;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerSlotState", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).map_or(0, |p| p.slot_state);
        NativeResult::Return(Value::Handle(v))
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerTeam", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).map_or(0, |p| p.team);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetPlayerTeam", move |_, a| {
        let v = raw_at(a, 1);
        let mut gm = g.borrow_mut();
        if let Some(p) = player_mut_at(&mut gm, a, 0) {
            p.team = v;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerStartLocation", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).map_or(0, |p| p.start_location);
        NativeResult::Return(Value::Integer(v))
    });

    // CONFIRMED: writes the raw location index (`+0x274`), no range check. The start-location
    // coordinates (GetPlayerStartLocationX/Y) and ForcePlayerStartLocation are not implemented yet.
    let g = Rc::clone(game);
    reg.register("SetPlayerStartLocation", move |_, a| {
        let v = raw_at(a, 1);
        let mut gm = g.borrow_mut();
        if let Some(p) = player_mut_at(&mut gm, a, 0) {
            p.start_location = v;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerColor", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).map_or(0, |p| p.color);
        NativeResult::Return(Value::Handle(v))
    });

    let g = Rc::clone(game);
    reg.register("SetPlayerColor", move |_, a| {
        let v = raw_at(a, 1) as u32;
        let mut gm = g.borrow_mut();
        if let Some(p) = player_mut_at(&mut gm, a, 0) {
            p.color = v;
        }
        NativeResult::Return(Value::Null)
    });

    // UNSURE: the original calls helpers for the alliance types 0, 5, 6, 7 and 9 (vision, control and
    // the pass-through of the flag to the UI). Only the bit is stored here.
    let g = Rc::clone(game);
    reg.register("SetPlayerAlliance", move |_, a| {
        let t = raw_at(a, 2);
        let flag = bool_at(a, 3);
        let mut gm = g.borrow_mut();
        if let (Some(p), Some(q), Some(t)) =
            (index_at(&gm, a, 0), index_at(&gm, a, 1), alliance_type(t))
        {
            let mask = &mut gm.player_mut(p).expect("slot").alliances[t];
            if flag {
                *mask |= 1 << q;
            } else {
                *mask &= !(1 << q);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerAlliance", move |_, a| {
        let gm = g.borrow();
        let v = match (
            index_at(&gm, a, 0),
            index_at(&gm, a, 1),
            alliance_type(raw_at(a, 2)),
        ) {
            (Some(p), Some(q), Some(t)) => alliance_bit(&gm, p, q, t),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsPlayerAlly", move |_, a| {
        let gm = g.borrow();
        let v = match (index_at(&gm, a, 0), index_at(&gm, a, 1)) {
            (Some(p), Some(q)) => alliance_bit(&gm, p, q, PASSIVE),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsPlayerEnemy", move |_, a| {
        let gm = g.borrow();
        let v = match (index_at(&gm, a, 0), index_at(&gm, a, 1)) {
            (Some(p), Some(q)) => !alliance_bit(&gm, p, q, PASSIVE),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsPlayerObserver", move |_, a| {
        let gm = g.borrow();
        let v = player_at(&gm, a, 0).is_some_and(|p| p.observer);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerState", move |_, a| {
        let gm = g.borrow();
        let v = match (index_at(&gm, a, 0), state_at(a, 1)) {
            (Some(p), Some(s)) => state_load(s, stored_state(&gm, p, s)),
            _ => 0,
        };
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetPlayerState", move |ctx, a| {
        let v = raw_at(a, 2);
        let changed = {
            let mut gm = g.borrow_mut();
            match (index_at(&gm, a, 0), state_at(a, 1)) {
                (Some(p), Some(s)) => {
                    let stored = state_store(s, v);
                    set_stored_state(&mut gm, p, s, stored);
                    Some((p, s, stored))
                }
                _ => None,
            }
        };
        if let Some((p, s, stored)) = changed {
            let value = softfloat::conv::i2r(state_load(s, stored));
            super::event::fire_player_state(&g, ctx, p, s, value);
        }
        NativeResult::Return(Value::Null)
    });
}

/// The raw integer of an integer or handle-typed argument. Enum values (`playercolor`, `racetype`, ...)
/// are handles in JASS, and `Convert*` makes them from raw integers, so both kinds are accepted.
pub(super) fn raw_at(a: &[Value], i: usize) -> i32 {
    match a.get(i) {
        Some(Value::Integer(n)) => *n,
        Some(Value::Handle(n)) => *n as i32,
        _ => 0,
    }
}

fn bool_at(a: &[Value], i: usize) -> bool {
    match a.get(i) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(n)) => *n != 0,
        _ => false,
    }
}

/// A slot index argument of `Player(i)`: `Some` for 0 to 15, `None` otherwise.
fn index_arg(a: &[Value], i: usize) -> Option<u32> {
    let n = raw_at(a, i);
    (0..super::game::PLAYER_SLOTS as i32)
        .contains(&n)
        .then_some(n as u32)
}

/// The slot index of a player handle argument, or `None` for a null or invalid handle.
pub(super) fn index_at(gm: &Game, a: &[Value], i: usize) -> Option<u32> {
    gm.player_index_of_handle(a.get(i)?.as_handle()?)
}

/// The slot of a player handle argument.
pub(super) fn player_at<'g>(gm: &'g Game, a: &[Value], i: usize) -> Option<&'g PlayerObj> {
    index_at(gm, a, i).map(|p| gm.player(p))
}

fn player_mut_at<'g>(gm: &'g mut Game, a: &[Value], i: usize) -> Option<&'g mut PlayerObj> {
    let p = index_at(gm, a, i)?;
    gm.player_mut(p)
}

/// An alliance type argument: 0 to 9 are valid, anything else is rejected (UNSURE).
pub(super) fn alliance_type(t: i32) -> Option<usize> {
    (0..ALLIANCE_TYPES as i32)
        .contains(&t)
        .then_some(t as usize)
}

/// A player state argument: 0 to 25 are valid.
fn state_at(a: &[Value], i: usize) -> Option<u32> {
    let s = raw_at(a, i);
    (0..PLAYER_STATES as i32).contains(&s).then_some(s as u32)
}

/// Whether bit `q` is set in the target set of `p` for alliance type `t`.
pub(super) fn alliance_bit(gm: &Game, p: u32, q: u32, t: usize) -> bool {
    gm.player(p).alliances[t] & (1 << q) != 0
}

/// The stored form of a player state value, as `SetPlayerState` writes it.
///
/// Gold and lumber (states 1 and 2) are kept times 10 and clamped to `[0, 0x989680]` (CONFIRMED from
/// `FUN_6f3a0eb0`: the clamp takes 0 for a negative value and the maximum above it). States 15 and 16
/// get their own factor in the caller (1 and 10, CONFIRMED from the body of `SetPlayerState`).
pub(super) fn state_store(state: u32, value: i32) -> i32 {
    let v = match state {
        16 => value.wrapping_mul(10),
        _ => value,
    };
    if matches!(state, 1 | 2) {
        let x = v.wrapping_mul(10);
        if x as u32 > GOLD_LUMBER_MAX {
            return if x < 0 { 0 } else { GOLD_LUMBER_MAX as i32 };
        }
        return x;
    }
    v
}

/// The stored value of state `s` of slot `p`. Gold and lumber (states 1 and 2) live in the economy of the slot
/// (`Game::economy`, shared with the changer); the other states are the slot's state words.
pub(super) fn stored_state(gm: &Game, p: u32, s: u32) -> i32 {
    match s {
        1 | 2 => gm.player_economy(p).map_or(0, |e| e.stock[(s - 1) as usize]),
        _ => gm.player(p).states[s as usize],
    }
}

/// Writes the stored value of state `s` of slot `p` (the inverse of [`stored_state`]). The value is already in the
/// stored form (see [`state_store`]).
pub(super) fn set_stored_state(gm: &mut Game, p: u32, s: u32, stored: i32) {
    match s {
        1 | 2 => {
            if let Some(e) = gm.economy.get_mut(p as usize) {
                e.stock[(s - 1) as usize] = stored;
            }
        }
        _ => gm.player_mut(p).expect("slot").states[s as usize] = stored,
    }
}

/// The value of a stored player state, as `GetPlayerState` reads it (the inverse of [`state_store`]):
/// states 1 and 2 are divided by 10, state 15 is unchanged and state 16 is divided by 10 as unsigned.
pub(super) fn state_load(state: u32, stored: i32) -> i32 {
    match state {
        1 | 2 => stored.wrapping_div(10),
        15 => stored,
        16 => (stored as u32 / 10) as i32,
        _ => stored,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_round_trip_and_clamp() {
        assert_eq!(state_load(1, state_store(1, 500)), 500);
        assert_eq!(state_store(1, 500), 5000);
        assert_eq!(state_store(1, -3), 0);
        // i32::MAX * 10 wraps to -10 in the 32-bit multiply, and a negative result clamps to 0.
        assert_eq!(state_store(2, i32::MAX), 0);
        // 10^9 * 10 wraps to 1410065408, which is above the maximum, so it clamps to the maximum.
        assert_eq!(state_store(2, 1_000_000_000), GOLD_LUMBER_MAX as i32);
        assert_eq!(state_load(1, GOLD_LUMBER_MAX as i32), 1_000_000);
    }

    #[test]
    fn gathered_states() {
        assert_eq!(state_store(15, 7), 7);
        assert_eq!(state_load(15, 7), 7);
        assert_eq!(state_store(16, 7), 70);
        assert_eq!(state_load(16, 70), 7);
        assert_eq!(state_load(16, -10), ((-10i32 as u32) / 10) as i32);
    }
}
