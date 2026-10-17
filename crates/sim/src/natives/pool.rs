//! Unit and item pools: `CreateUnitPool`, `DestroyUnitPool`, `UnitPoolAddUnitType`, `UnitPoolRemoveUnitType`,
//! `PlaceRandomUnit`, and the item twins `CreateItemPool`, `DestroyItemPool`, `ItemPoolAddItemType`,
//! `ItemPoolRemoveItemType` and `PlaceRandomItem`.
//!
//! Addresses are the 1.26a `Game.dll` functions in `docs/spec/natives-1.26a.tsv`; the bodies were read in the
//! decompile and the disassembly (not in the repo). Labels as in `unit.rs`.
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | CreateUnitPool / CreateItemPool | 0x6F3D2ED0 / 0x6F3D2FC0 | LIKELY | the handle is allocated last with flag 0; the global list the original links the pool into is not modelled |
//! | DestroyUnitPool / DestroyItemPool | 0x6F3CB5C0 / 0x6F3CB6A0 | LIKELY | the creation reference is released; the id is freed when no other holder keeps it |
//! | UnitPoolAddUnitType / ItemPoolAddItemType | 0x6F3CB5E0 / 0x6F3CB6C0 | CONFIRMED | `FUN_6f3efe00`: an id that is present gets its weight replaced, otherwise it is appended |
//! | UnitPoolRemoveUnitType / ItemPoolRemoveItemType | 0x6F3CB600 / 0x6F3CB6E0 | CONFIRMED | `FUN_6f3efeb0` / `FUN_6f3f0a50`: the first entry with the id is removed, the later ones shift down |
//! | PlaceRandomUnit | 0x6F3CB620 | UNSURE | the selection is CONFIRMED (`FUN_6f3e17b0`); the builder's register inputs are not decoded |
//! | PlaceRandomItem | 0x6F3CB700 | CONFIRMED selection | `FUN_6f3e0280`, the item builder `FUN_6f2b9940` |
//!
//! Layout (CONFIRMED): the pool keeps a count (`+0x24`) and an array of 8-byte entries: the raw type id, then the
//! weight as a float32 (`+0x28`).
//!
//! Random placement (CONFIRMED, `FUN_6f3e17b0` and `FUN_6f3e0280`, the same shape for units and items):
//! 1. A pool with no entries returns null without a draw.
//! 2. `total` is the sum of the weights, added in order in soft-float from 0.0.
//! 3. One step of the **global** generator (the one `GetRandomReal` uses; `FUN_6f4b1390` takes the pointer stored at
//!    `0x6FAB73D8`) gives `U` in `[0, 1)`. The per-category streams are not used here.
//! 4. `r = total * U` in soft-float.
//! 5. The entries are walked with running sums `cum += weight` (soft-float). The first entry with `cum > r` (an exact
//!    compare, `fcompp`) is picked. If no entry qualifies, entry 0 is picked.
//! 6. A picked id of `-1` (0xFFFFFFFF) is replaced by the id of entry 0.
//! 7. The unit or item is built at the position and the handle is allocated last. A failed build returns null.
//!
//! The unit selection helper forwards owner in ECX, selected rawcode in EDX, then X/Y/facing pointers on the stack.
//! The item helper forwards selected rawcode in ECX, X in EDX and Y on the stack (Unicorn-checked).
//! Native facing conversion and the downstream creation routines remain outside this check.
//! The unit is built through the `CreateUnit` path with the player as owner and the facing in degrees, as `CreateUnit`
//! does. The order of the checks follows the disassembly: pool, then player, then the draw.
//! `tools/emu/pool_selection_emu.py` runs both original selection helpers and bounded add/remove operations.
//! The module's vector tests compare selected ids, final global RNG state and exact entry order/weight bits:
//! 2,000 pool inputs through each original helper and 2,000 mutation steps (zero mismatches).
//! Creation, handle allocation and array growth beyond the preallocated capacity are not covered by these vectors.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, RunStatus, Value};
use softfloat::{arith, F32};

use super::game::{Game, HandleKind, ObjKey, Object, PoolObj};
use super::hashtable::real_at;
use super::item::new_item;
use super::player::raw_at;
use super::unit::create as create_unit;
use super::unit_attr::{NativeEntry, Support};

/// The raw id that marks a picked entry as "no type" (`-1`): entry 0's id is used instead (CONFIRMED).
const NO_TYPE: u32 = 0xFFFF_FFFF;

/// The support table of this module: every native of the pool work, with its level.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "CreateUnitPool",
        support: Support::Likely,
    },
    NativeEntry {
        name: "DestroyUnitPool",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitPoolAddUnitType",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "UnitPoolRemoveUnitType",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "PlaceRandomUnit",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "CreateItemPool",
        support: Support::Likely,
    },
    NativeEntry {
        name: "DestroyItemPool",
        support: Support::Likely,
    },
    NativeEntry {
        name: "ItemPoolAddItemType",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "ItemPoolRemoveItemType",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "PlaceRandomItem",
        support: Support::Confirmed,
    },
];

/// Registers the pool natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateUnitPool", move |_, _| {
        let id = create_pool(&mut g.borrow_mut(), false);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("CreateItemPool", move |_, _| {
        let id = create_pool(&mut g.borrow_mut(), true);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("DestroyUnitPool", move |_, a| {
        destroy_pool(&mut g.borrow_mut(), a, false);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("DestroyItemPool", move |_, a| {
        destroy_pool(&mut g.borrow_mut(), a, true);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitPoolAddUnitType", move |_, a| {
        add_native(&mut g.borrow_mut(), a, false);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ItemPoolAddItemType", move |_, a| {
        add_native(&mut g.borrow_mut(), a, true);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitPoolRemoveUnitType", move |_, a| {
        remove_native(&mut g.borrow_mut(), a, false);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ItemPoolRemoveItemType", move |_, a| {
        remove_native(&mut g.borrow_mut(), a, true);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("PlaceRandomUnit", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(k) = pool_at(&gm, a, 0, false) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let owner = raw_at(a, 1) as u32;
        if gm.player_index_of_handle(owner).is_none() {
            return NativeResult::Return(Value::Handle(0));
        }
        match pick(&mut gm, k) {
            Ok(Some(type_id)) => {
                let id = create_unit(
                    &mut gm,
                    owner,
                    type_id,
                    real_at(a, 2),
                    real_at(a, 3),
                    real_at(a, 4),
                );
                NativeResult::Return(Value::Handle(id))
            }
            Ok(None) => NativeResult::Return(Value::Handle(0)),
            Err(status) => NativeResult::Fail(status),
        }
    });

    let g = Rc::clone(game);
    reg.register("PlaceRandomItem", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(k) = pool_at(&gm, a, 0, true) else {
            return NativeResult::Return(Value::Handle(0));
        };
        match pick(&mut gm, k) {
            Ok(Some(type_id)) => {
                let id = match new_item(&gm, type_id, real_at(a, 1), real_at(a, 2)) {
                    Some(obj) => gm.add_object(Object::Item(obj), 0).1,
                    None => 0,
                };
                NativeResult::Return(Value::Handle(id))
            }
            Ok(None) => NativeResult::Return(Value::Handle(0)),
            Err(status) => NativeResult::Fail(status),
        }
    });
}

/// Creates an empty pool and returns its handle (allocated last, flag 0).
fn create_pool(gm: &mut Game, item: bool) -> u32 {
    gm.add_object(
        Object::Pool(PoolObj {
            item,
            entries: Vec::new(),
        }),
        0,
    )
    .1
}

/// `DestroyUnitPool` / `DestroyItemPool`: releases the creation reference of a live pool of the kind.
fn destroy_pool(gm: &mut Game, a: &[Value], item: bool) {
    if let Some(k) = pool_at(gm, a, 0, item) {
        gm.kill(k);
    }
}

/// `UnitPoolAddUnitType` / `ItemPoolAddItemType`: the weight is the real argument `2`.
fn add_native(gm: &mut Game, a: &[Value], item: bool) {
    if let Some(k) = pool_at(gm, a, 0, item) {
        add_entry(gm, k, raw_at(a, 1) as u32, real_at(a, 2));
    }
}

/// `UnitPoolRemoveUnitType` / `ItemPoolRemoveItemType`.
fn remove_native(gm: &mut Game, a: &[Value], item: bool) {
    if let Some(k) = pool_at(gm, a, 0, item) {
        remove_entry(gm, k, raw_at(a, 1) as u32);
    }
}

/// The pool in handle argument `i`, if it is a live pool of the requested kind.
fn pool_at(gm: &Game, a: &[Value], i: usize, item: bool) -> Option<ObjKey> {
    let kind = if item {
        HandleKind::ItemPool
    } else {
        HandleKind::UnitPool
    };
    let id = a.get(i).and_then(|v| v.as_handle()).unwrap_or(0);
    gm.handle_of_kind(id, kind)
}

fn pool_mut(gm: &mut Game, k: ObjKey) -> &mut PoolObj {
    match gm.object_mut(k) {
        Object::Pool(p) => p,
        _ => panic!("not a pool"),
    }
}

/// Adds `id` with `weight`: an id that is present gets the new weight in place, a new one is appended
/// (`FUN_6f3efe00`).
fn add_entry(gm: &mut Game, k: ObjKey, id: u32, weight: F32) {
    let entries = &mut pool_mut(gm, k).entries;
    match entries.iter_mut().find(|(t, _)| *t == id) {
        Some(entry) => entry.1 = weight,
        None => entries.push((id, weight)),
    }
}

/// Removes the first entry with `id`, shifting the later entries down (`FUN_6f3efeb0`, `FUN_6f3f0a50`).
fn remove_entry(gm: &mut Game, k: ObjKey, id: u32) {
    let entries = &mut pool_mut(gm, k).entries;
    if let Some(i) = entries.iter().position(|(t, _)| *t == id) {
        entries.remove(i);
    }
}

/// The weighted pick of the random placement (module notes, steps 1 to 6). Returns the type id to build, or `None`
/// for an empty pool, which draws nothing.
fn pick(gm: &mut Game, k: ObjKey) -> Result<Option<u32>, RunStatus> {
    let entries = match gm.object(k) {
        Object::Pool(p) => p.entries.clone(),
        _ => Vec::new(),
    };
    if entries.is_empty() {
        return Ok(None);
    }
    let total = entries
        .iter()
        .fold(F32::ZERO, |acc, &(_, w)| arith::add(acc, w));
    let (table, streams) = gm.rng_parts().ok_or(RunStatus::MissingTables)?;
    crate::rng::trace::note(|| "pool".into());
    let u = streams.global.random_fraction(table);
    let r = arith::mul(total, u);
    let mut cum = F32::ZERO;
    for &(id, w) in &entries {
        cum = arith::add(cum, w);
        if arith::gt(cum, r) {
            return Ok(Some(resolve_no_type(&entries, id)));
        }
    }
    Ok(Some(entries[0].0))
}

/// A picked `-1` gives the id of entry 0 (CONFIRMED: the `cmp edx, -1` branch of `FUN_6f3e17b0`).
fn resolve_no_type(entries: &[(u32, F32)], id: u32) -> u32 {
    if id == NO_TYPE {
        entries[0].0
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_type_entry_resolves_to_the_first_entry() {
        let entries = [(7u32, F32::ONE), (9, F32::ONE)];
        assert_eq!(resolve_no_type(&entries, NO_TYPE), 7);
        assert_eq!(resolve_no_type(&entries, 9), 9);
    }

    #[test]
    fn add_replaces_weight_and_remove_shifts() {
        let mut g = Game::new();
        let (_, id) = g.add_object(
            Object::Pool(PoolObj {
                item: false,
                entries: Vec::new(),
            }),
            0,
        );
        let k = g.handle_object(id).unwrap();
        add_entry(&mut g, k, 1, F32::ONE);
        add_entry(&mut g, k, 2, F32::ONE);
        add_entry(&mut g, k, 1, F32::ZERO);
        let entries = |g: &Game| match g.object(k) {
            Object::Pool(p) => p.entries.clone(),
            _ => unreachable!(),
        };
        assert_eq!(entries(&g), vec![(1, F32::ZERO), (2, F32::ONE)]);
        remove_entry(&mut g, k, 1);
        assert_eq!(entries(&g), vec![(2, F32::ONE)]);
        remove_entry(&mut g, k, 5);
        assert_eq!(entries(&g), vec![(2, F32::ONE)]);
    }

    fn hex(text: &str) -> u32 {
        u32::from_str_radix(text, 16).expect("vector hex")
    }

    #[test]
    #[ignore = "requires GAME_DLL; run explicitly to compare with the original RNG table"]
    fn selection_matches_original_pool_vectors() {
        let path = std::env::var("GAME_DLL").expect("GAME_DLL is required for the original-table check");
        let dll = std::fs::read(path).expect("Game.dll");
        let table = crate::rng::RngTable::from_game_dll(&dll).expect("original RNG table");
        let mut game = Game::new();
        game.set_rng_table(Some(table));
        let keys = [false, true].map(|item| {
            game.add_object(Object::Pool(PoolObj { item, entries: Vec::new() }), 0).0
        });
        let mut checked = 0;
        for (line_no, line) in include_str!("../../tests/data/pools/selection.txt").lines().enumerate() {
            let mut fields = line.split_whitespace();
            if fields.next() != Some("P") {
                continue;
            }
            let before = crate::rng::Rng { a: hex(fields.next().unwrap()), b: hex(fields.next().unwrap()) };
            let count: usize = fields.next().unwrap().parse().unwrap();
            let entries: Vec<_> = (0..count).map(|_| {
                (hex(fields.next().unwrap()), F32::from_bits(hex(fields.next().unwrap())))
            }).collect();
            let expected = match fields.next().unwrap() {
                "none" => None,
                value => Some(hex(value)),
            };
            let after = crate::rng::Rng { a: hex(fields.next().unwrap()), b: hex(fields.next().unwrap()) };
            let draws: usize = fields.next().unwrap().parse().unwrap();
            assert_eq!(draws, usize::from(count != 0));
            assert!(fields.next().is_none());
            for key in keys {
                pool_mut(&mut game, key).entries = entries.clone();
                game.rng_parts().unwrap().1.global = before;
                assert_eq!(pick(&mut game, key).expect("tables"), expected, "selection line {}", line_no + 1);
                assert_eq!(game.rng_parts().unwrap().1.global, after, "RNG line {}", line_no + 1);
            }
            checked += 1;
        }
        assert!(checked >= 1000, "original selection vectors must execute");
    }

    #[test]
    fn mutation_matches_original_pool_vectors() {
        let mut game = Game::new();
        let key = game.add_object(Object::Pool(PoolObj { item: false, entries: Vec::new() }), 0).0;
        let mut checked = 0;
        for (line_no, line) in include_str!("../../tests/data/pools/selection.txt").lines().enumerate() {
            let mut fields = line.split_whitespace();
            if fields.next() != Some("M") {
                continue;
            }
            let operation = fields.next().unwrap();
            let id = hex(fields.next().unwrap());
            let weight = F32::from_bits(hex(fields.next().unwrap()));
            match operation {
                "add" => add_entry(&mut game, key, id, weight),
                "remove" => remove_entry(&mut game, key, id),
                _ => panic!("unknown mutation"),
            }
            let count: usize = fields.next().unwrap().parse().unwrap();
            let expected: Vec<_> = (0..count).map(|_| (hex(fields.next().unwrap()), hex(fields.next().unwrap()))).collect();
            assert!(fields.next().is_none());
            let actual: Vec<_> = pool_mut(&mut game, key).entries.iter().map(|&(id, weight)| (id, weight.to_bits())).collect();
            assert_eq!(actual, expected, "mutation line {}", line_no + 1);
            checked += 1;
        }
        assert!(checked >= 1000, "original mutation vectors must execute");
    }
}
