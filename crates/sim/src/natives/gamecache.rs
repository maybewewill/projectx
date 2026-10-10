//! Game cache natives: `InitGameCache`, `Store*`, `GetStored*`, `HaveStored*`, `Flush*` and the
//! `SyncStored*` calls. Only the in-memory part is modelled.
//!
//! Behaviour notes (addresses are 1.26a `Game.dll`; labels as in `hashtable.rs`).
//!
//! * Storage. A cache keeps one map per value kind keyed by (mission, key) strings, the same layout as
//!   a hashtable with string keys (`TSHashTable` with the `STRIPACKED` key type, CONFIRMED from the
//!   RTTI). Missing keys read as the default of the kind: 0, 0.0, false, "" (LIKELY).
//! * Keys are compared as exact text (UNSURE: the `STRIPACKED` hash may normalise the text, for
//!   example its case or its packing; the normalisation was not traced).
//! * `InitGameCache(name)` (0x6F3D2CC0): an empty name gives the null handle. The original also
//!   requires the name to exist in `Campaigns.w3v` (or a flag in the game state) and returns the null
//!   handle otherwise. That file check is not modelled, so every non-empty name succeeds (UNSURE).
//!   A cache name gives the same handle every time (LIKELY: the cache object is looked up by name).
//! * `SyncStored*` (0x6F3CA6E0..0x6F3CA820) send the value to the other players in multiplayer. They
//!   have no effect in a single player game and are no-ops here (LIKELY).
//! * `FlushGameCache` (0x6F3CB200) clears every value of the cache. `FlushStoredMission` (0x6F3CB240)
//!   clears one mission, `FlushStored<Kind>` (0x6F3CB280..0x6F3CB3C0) one value. LIKELY.
//! * UNSUPPORTED, no effect (the native returns the empty result): `StoreUnit`, `HaveStoredUnit`,
//!   `RestoreUnit`, `FlushStoredUnit` (units do not exist in the simulation yet), and every save or load
//!   to disk: `SaveGameCache`, `ReloadGameCachesFromDisk`, `RenameSaveDirectory`, `RemoveSaveDirectory`.
//!   The disk format and the campaign files are not implemented.
//! * `StoreString` and the other boolean-returning stores give true for a stored value (UNSURE: the
//!   original leaves the result in a register).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, HandleKind, ObjKey, Object, Slots};
use super::hashtable::{bool_at, int_at, real_at, str_at};

/// Registers the game cache natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("InitGameCache", move |ctx, a| {
        let name = str_at(ctx, a, 0);
        let mut gm = g.borrow_mut();
        let id = if name.is_empty() {
            0
        } else if let Some(k) = gm.cache_named(&name) {
            gm.handle_of(k)
        } else {
            let (k, id) = gm.add_object(Object::GameCache(Slots::new()), 0);
            gm.set_cache_named(&name, k);
            id
        };
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("StoreInteger", move |ctx, a| {
        let key = keys(ctx, a);
        let v = int_at(a, 3);
        let mut gm = g.borrow_mut();
        if let Some(k) = cache_of(&gm, a) {
            slots_mut(&mut gm, k).ints.insert(key, v);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("StoreReal", move |ctx, a| {
        let key = keys(ctx, a);
        let v = real_at(a, 3);
        let mut gm = g.borrow_mut();
        if let Some(k) = cache_of(&gm, a) {
            slots_mut(&mut gm, k).reals.insert(key, v);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("StoreBoolean", move |ctx, a| {
        let key = keys(ctx, a);
        let v = bool_at(a, 3);
        let mut gm = g.borrow_mut();
        if let Some(k) = cache_of(&gm, a) {
            slots_mut(&mut gm, k).bools.insert(key, v);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("StoreString", move |ctx, a| {
        let key = keys(ctx, a);
        let text = str_at(ctx, a, 3);
        let mut gm = g.borrow_mut();
        let stored = match cache_of(&gm, a) {
            Some(k) => {
                slots_mut(&mut gm, k).strings.insert(key, text);
                true
            }
            None => false,
        };
        NativeResult::Return(Value::Boolean(stored))
    });

    reg.register("StoreUnit", |_, _| {
        NativeResult::Return(Value::Boolean(false))
    });
    reg.register("RestoreUnit", |_, _| NativeResult::Return(Value::Handle(0)));
    reg.register("HaveStoredUnit", |_, _| {
        NativeResult::Return(Value::Boolean(false))
    });
    reg.register("FlushStoredUnit", |_, _| NativeResult::Return(Value::Null));

    for name in [
        "SyncStoredInteger",
        "SyncStoredReal",
        "SyncStoredBoolean",
        "SyncStoredUnit",
        "SyncStoredString",
    ] {
        reg.register(name, |_, _| NativeResult::Return(Value::Null));
    }

    let g = Rc::clone(game);
    reg.register("GetStoredInteger", move |ctx, a| {
        let key = keys(ctx, a);
        let gm = g.borrow();
        let v = cache_of(&gm, a).map_or(0, |k| slots(&gm, k).ints.get(&key).copied().unwrap_or(0));
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetStoredReal", move |ctx, a| {
        let key = keys(ctx, a);
        let gm = g.borrow();
        let v = cache_of(&gm, a).map_or(F32::ZERO, |k| {
            slots(&gm, k).reals.get(&key).copied().unwrap_or(F32::ZERO)
        });
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetStoredBoolean", move |ctx, a| {
        let key = keys(ctx, a);
        let gm = g.borrow();
        let v = cache_of(&gm, a)
            .is_some_and(|k| slots(&gm, k).bools.get(&key).copied().unwrap_or(false));
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("GetStoredString", move |ctx, a| {
        let key = keys(ctx, a);
        let text = {
            let gm = g.borrow();
            cache_of(&gm, a).and_then(|k| slots(&gm, k).strings.get(&key).cloned())
        }
        .unwrap_or_default();
        NativeResult::Return(Value::String(ctx.intern(&text)))
    });

    register_have(reg, game, "HaveStoredInteger", |s, k| {
        s.ints.contains_key(k)
    });
    register_have(reg, game, "HaveStoredReal", |s, k| s.reals.contains_key(k));
    register_have(reg, game, "HaveStoredBoolean", |s, k| {
        s.bools.contains_key(k)
    });
    register_have(reg, game, "HaveStoredString", |s, k| {
        s.strings.contains_key(k)
    });

    let g = Rc::clone(game);
    reg.register("FlushGameCache", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = cache_of(&gm, a) {
            slots_mut(&mut gm, k).clear();
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("FlushStoredMission", move |ctx, a| {
        let mission = str_at(ctx, a, 1);
        let mut gm = g.borrow_mut();
        if let Some(k) = cache_of(&gm, a) {
            slots_mut(&mut gm, k).remove_parent(&mission);
        }
        NativeResult::Return(Value::Null)
    });

    register_flush(reg, game, "FlushStoredInteger", |s, k| {
        s.ints.remove(k);
    });
    register_flush(reg, game, "FlushStoredReal", |s, k| {
        s.reals.remove(k);
    });
    register_flush(reg, game, "FlushStoredBoolean", |s, k| {
        s.bools.remove(k);
    });
    register_flush(reg, game, "FlushStoredString", |s, k| {
        s.strings.remove(k);
    });

    for name in [
        "SaveGameCache",
        "ReloadGameCachesFromDisk",
        "RenameSaveDirectory",
        "RemoveSaveDirectory",
    ] {
        reg.register(name, |_, _| NativeResult::Return(Value::Boolean(false)));
    }
}

/// A `HaveStored*` native: whether the (mission, key) pair is set, as seen by `has` for one kind.
fn register_have(
    reg: &mut NativeRegistry,
    game: &Rc<RefCell<Game>>,
    name: &str,
    has: fn(&Slots<String, String>, &(String, String)) -> bool,
) {
    let g = Rc::clone(game);
    reg.register(name, move |ctx, a| {
        let key = keys(ctx, a);
        let gm = g.borrow();
        let v = cache_of(&gm, a).is_some_and(|k| has(slots(&gm, k), &key));
        NativeResult::Return(Value::Boolean(v))
    });
}

/// A `FlushStored<Kind>` native: removes the (mission, key) pair from that kind's map only.
fn register_flush(
    reg: &mut NativeRegistry,
    game: &Rc<RefCell<Game>>,
    name: &str,
    remove: fn(&mut Slots<String, String>, &(String, String)),
) {
    let g = Rc::clone(game);
    reg.register(name, move |ctx, a| {
        let key = keys(ctx, a);
        let mut gm = g.borrow_mut();
        if let Some(k) = cache_of(&gm, a) {
            remove(slots_mut(&mut gm, k), &key);
        }
        NativeResult::Return(Value::Null)
    });
}

/// The game cache object behind argument 0, if it is a live cache.
fn cache_of(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    let id = a.first()?.as_handle()?;
    gm.handle_of_kind(id, HandleKind::GameCache)
}

fn slots(gm: &Game, k: ObjKey) -> &Slots<String, String> {
    match gm.object(k) {
        Object::GameCache(s) => s,
        _ => unreachable!("slots on a non-cache key"),
    }
}

fn slots_mut(gm: &mut Game, k: ObjKey) -> &mut Slots<String, String> {
    match gm.object_mut(k) {
        Object::GameCache(s) => s,
        _ => unreachable!("slots_mut on a non-cache key"),
    }
}

/// The (mission, key) pair of arguments 1 and 2.
fn keys(ctx: &jass::vm::NativeCtx<'_>, a: &[Value]) -> (String, String) {
    (str_at(ctx, a, 1), str_at(ctx, a, 2))
}
