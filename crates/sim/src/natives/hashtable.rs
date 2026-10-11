//! Hashtable natives: `InitHashtable`, the `Save*`, `Load*`, `HaveSaved*` and `RemoveSaved*` families,
//! `FlushChildHashtable` and `FlushParentHashtable`.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions read in `natives.c`, and the helpers
//! were checked by disassembly (capstone) of the same binary. Labels: CONFIRMED = read directly,
//! LIKELY = strong inference from the code shape, UNSURE = guessed or not resolved.
//!
//! * Storage (CONFIRMED). A hashtable holds one storage object per table (the manager at game+0x404
//!   maps the table's data pointer to it). Inside, each parent key has a node, and each node has one
//!   map per value kind keyed by child key: integer (node+0x1c), real (+0x48), boolean (+0x74), string
//!   (+0xa0) and handle (+0xcc). Here the same thing is one map per kind keyed by (parent, child).
//! * Missing keys read as the default of the kind: 0, 0.0, false, "" and the null handle (CONFIRMED
//!   for integer, boolean and handle; string default LIKELY, real default LIKELY).
//! * Every native first resolves the table handle (0x6F3BED90). A null or freed table does nothing,
//!   the save natives return false and the load natives return the default. CONFIRMED.
//! * `SaveInteger` / `SaveReal` / `SaveBoolean` / `SaveStr` (0x6F3CA200 / 0x6F3CA240 / 0x6F3CA280 /
//!   0x6F3CA2C0): plain overwrite of the slot of the kind. No reference counts. CONFIRMED.
//!   `SaveStr` and the handle saves return a boolean whose value the original leaves in a register;
//!   this port returns true for a stored value and false for a rejected one (UNSURE).
//! * Handle saves (`SaveUnitHandle` 0x6F3CA380 and the others) type-check the handle first with the
//!   kind's resolver (timer 0x6F3BD710, location 0x6F3BD3B0, rect 0x6F3BDB90, unit 0x6F3BDCB0, ...).
//!   A null handle or one of the wrong type stores nothing and the old value stays. CONFIRMED.
//!   A stored handle is written with flag 1: the table holds one reference, taken with AddRef
//!   (0x6F428080) after the old owned reference is released (0x6F42DD40). CONFIRMED (0x6F402D80).
//!   The handle id keeps its number after the object is destroyed, because the table still holds it.
//! * Handle types the simulation does not model (unit, item, ...) have no objects here, so every
//!   handle of those types is null and the typed save fails. That is the same result the original
//!   gives for a null handle.
//! * `LoadXHandle` (`LoadUnitHandle` 0x6F3CAC10 -> 0x6F3BF680) returns the saved id only if the id
//!   still passes the typed check (0x6F3BDCB0 for units, read from the entry's object pointer, which
//!   the object's death clears). Otherwise the result is the null handle. CONFIRMED for the unit
//!   path; the other kinds are assumed to check the same way (LIKELY).
//! * `RemoveSaved*` (0x6F3CB410..0x6F3CB510) deletes the slot. For a handle slot the node destructor
//!   (0x6F3EFC30) drops the owned reference. CONFIRMED for the destructor, LIKELY for the path.
//! * `FlushChildHashtable(t, p)` (0x6F3CB590 -> 0x6F402A80) erases the parent node `p` with all its
//!   values. Owned handles are released through the node destructors. LIKELY.
//! * `FlushParentHashtable(t)` (0x6F3CB550 -> 0x6F3F4C90) erases the storage of the whole table,
//!   releasing every owned handle, and the table stays usable. LIKELY. The original also makes a
//!   virtual call on the table object afterwards whose effect was not traced (UNSURE).
//! * Order of the releases during a flush follows the original's hash maps, modelled in
//!   `crate::hashmap`: parents in their map order, then each parent's handle children in its map order
//!   (buckets ascending, newest first inside a bucket). The child-level order was checked in Unicorn
//!   (`tools/emu/hashmap_vectors.py`) for tables that have not grown; the parent level uses the same
//!   algorithm (LIKELY). After a table has grown (more than 13 chain points in one bucket) the release
//!   order is not verified (see the module notes of `crate::hashmap`).
//! * `InitHashtable` (0x6F3D2DF0): a new table with a handle of flag 0. CONFIRMED.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeCtx, NativeRegistry, NativeResult, Value};
use softfloat::F32;

use super::game::{Game, HandleKind, ObjKey, Object, SavedHandle, Slots};
use crate::hashmap::BucketMap;

/// The handle kind of each typed `Save*Handle` / `Load*Handle` native. `None` for the handle types the
/// simulation does not model (no object of that type can exist, so the typed check always fails).
const HANDLE_NATIVES: &[(&str, Option<HandleKind>)] = &[
    ("PlayerHandle", Some(HandleKind::Player)),
    // TODO: a unit is also a widget; the widget type check is not modelled yet.
    ("WidgetHandle", None),
    ("DestructableHandle", None),
    ("ItemHandle", None),
    ("UnitHandle", Some(HandleKind::Unit)),
    ("AbilityHandle", None),
    ("TimerHandle", Some(HandleKind::Timer)),
    ("RegionHandle", Some(HandleKind::Region)),
    ("TriggerHandle", Some(HandleKind::Trigger)),
    ("TriggerConditionHandle", Some(HandleKind::Condition)),
    ("TriggerActionHandle", Some(HandleKind::Action)),
    ("TriggerEventHandle", Some(HandleKind::Event)),
    ("ForceHandle", Some(HandleKind::Force)),
    ("GroupHandle", None),
    ("LocationHandle", Some(HandleKind::Location)),
    ("RectHandle", Some(HandleKind::Rect)),
    ("BooleanExprHandle", Some(HandleKind::BoolExpr)),
    ("SoundHandle", None),
    ("EffectHandle", None),
    ("UnitPoolHandle", None),
    ("ItemPoolHandle", None),
    ("QuestHandle", None),
    ("QuestItemHandle", None),
    ("DefeatConditionHandle", None),
    ("TimerDialogHandle", None),
    ("LeaderboardHandle", None),
    ("MultiboardHandle", None),
    ("MultiboardItemHandle", None),
    ("TrackableHandle", None),
    ("DialogHandle", None),
    ("ButtonHandle", None),
    ("TextTagHandle", None),
    ("LightningHandle", None),
    ("ImageHandle", None),
    ("UbersplatHandle", None),
    ("FogStateHandle", None),
    ("FogModifierHandle", None),
    ("AgentHandle", None),
    ("HashtableHandle", Some(HandleKind::Hashtable)),
];

/// Registers the hashtable natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("InitHashtable", move |_, _| {
        let (_, id) = g
            .borrow_mut()
            .add_object(Object::Hashtable(HashtableObj::default()), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("SaveInteger", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            ensure_node(&mut gm, k, a);
            slots_mut(&mut gm, k).ints.insert(keys(a), int_at(a, 3));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SaveReal", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            ensure_node(&mut gm, k, a);
            slots_mut(&mut gm, k).reals.insert(keys(a), real_at(a, 3));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SaveBoolean", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            ensure_node(&mut gm, k, a);
            slots_mut(&mut gm, k).bools.insert(keys(a), bool_at(a, 3));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SaveStr", move |ctx, a| {
        let text = str_at(ctx, a, 3);
        let mut gm = g.borrow_mut();
        let stored = match table_of(&gm, a) {
            Some(k) => {
                ensure_node(&mut gm, k, a);
                slots_mut(&mut gm, k).strings.insert(keys(a), text);
                true
            }
            None => false,
        };
        NativeResult::Return(Value::Boolean(stored))
    });

    for (suffix, kind) in HANDLE_NATIVES {
        let g = Rc::clone(game);
        reg.register(&format!("Save{suffix}"), move |_, a| {
            save_handle(&g, a, *kind)
        });
        let g = Rc::clone(game);
        reg.register(&format!("Load{suffix}"), move |_, a| {
            load_handle(&g, a, *kind)
        });
    }

    let g = Rc::clone(game);
    reg.register("LoadInteger", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a).map_or(0, |k| {
            slots(&gm, k).ints.get(&keys(a)).copied().unwrap_or(0)
        });
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("LoadReal", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a).map_or(F32::ZERO, |k| {
            slots(&gm, k)
                .reals
                .get(&keys(a))
                .copied()
                .unwrap_or(F32::ZERO)
        });
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("LoadBoolean", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a)
            .is_some_and(|k| slots(&gm, k).bools.get(&keys(a)).copied().unwrap_or(false));
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("LoadStr", move |ctx, a| {
        let text = {
            let gm = g.borrow();
            table_of(&gm, a).and_then(|k| slots(&gm, k).strings.get(&keys(a)).cloned())
        }
        .unwrap_or_default();
        NativeResult::Return(Value::String(ctx.intern(&text)))
    });

    let g = Rc::clone(game);
    reg.register("HaveSavedInteger", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a).is_some_and(|k| slots(&gm, k).ints.contains_key(&keys(a)));
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("HaveSavedReal", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a).is_some_and(|k| slots(&gm, k).reals.contains_key(&keys(a)));
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("HaveSavedBoolean", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a).is_some_and(|k| slots(&gm, k).bools.contains_key(&keys(a)));
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("HaveSavedString", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a).is_some_and(|k| slots(&gm, k).strings.contains_key(&keys(a)));
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("HaveSavedHandle", move |_, a| {
        let gm = g.borrow();
        let v = table_of(&gm, a).is_some_and(|k| slots(&gm, k).handles.contains_key(&keys(a)));
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("RemoveSavedInteger", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            slots_mut(&mut gm, k).ints.remove(&keys(a));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RemoveSavedReal", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            slots_mut(&mut gm, k).reals.remove(&keys(a));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RemoveSavedBoolean", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            slots_mut(&mut gm, k).bools.remove(&keys(a));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RemoveSavedString", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            slots_mut(&mut gm, k).strings.remove(&keys(a));
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RemoveSavedHandle", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            if let Some(old) = slots_mut(&mut gm, k).handles.remove(&keys(a)) {
                order_mut(&mut gm, k).remove_child(int_at(a, 1), int_at(a, 2));
                if old.owned {
                    gm.release(old.id);
                }
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("FlushChildHashtable", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            let parent = int_at(a, 1);
            let children = order_mut(&mut gm, k).take_parent(parent);
            let mut released = release_children(slots_mut(&mut gm, k), parent, &children);
            released.extend(slots_mut(&mut gm, k).remove_parent(&parent));
            gm.release_all(&released);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("FlushParentHashtable", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = table_of(&gm, a) {
            let mut released = Vec::new();
            for (parent, children) in order_mut(&mut gm, k).take_all() {
                released.extend(release_children(slots_mut(&mut gm, k), parent, &children));
            }
            released.extend(slots_mut(&mut gm, k).clear());
            gm.release_all(&released);
        }
        NativeResult::Return(Value::Null)
    });
}

/// The saved handles of one table, plus the order in which a flush releases them.
///
/// The original keeps one node per parent key (created by the first save under that key) and, in each
/// node, the handle values in a hash map keyed by child. A flush walks the parents and then the
/// children in the map's bucket order (see `crate::hashmap`). Each parent is `(key, child keys)`.
#[derive(Debug, Clone, Default)]
pub struct FlushOrder {
    nodes: BucketMap<(i32, BucketMap<i32>)>,
}

impl FlushOrder {
    fn ensure_parent(&mut self, parent: i32) {
        if self.nodes.get(parent as u32).is_none() {
            let _ = self
                .nodes
                .insert_new(parent as u32, (parent, BucketMap::new()));
        }
    }

    fn add_child(&mut self, parent: i32, child: i32) {
        self.ensure_parent(parent);
        if let Some((_, children)) = self.nodes.get_mut(parent as u32) {
            if children.get(child as u32).is_none() {
                let _ = children.insert_new(child as u32, child);
            }
        }
    }

    fn remove_child(&mut self, parent: i32, child: i32) {
        if let Some((_, children)) = self.nodes.get_mut(parent as u32) {
            children.remove(child as u32);
        }
    }

    /// Takes the node of `parent` and returns its children in flush order.
    fn take_parent(&mut self, parent: i32) -> Vec<i32> {
        match self.nodes.remove(parent as u32) {
            Some((_, children)) => children
                .keys_in_destroy_order()
                .into_iter()
                .map(|k| k as i32)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Takes every node in flush order, each with its children in flush order.
    fn take_all(&mut self) -> Vec<(i32, Vec<i32>)> {
        self.nodes
            .destroy_order()
            .into_iter()
            .map(|(parent, mut children)| (parent, children.destroy_order()))
            .collect()
    }
}

/// The contents of a hashtable object: its values and the flush order of its handles.
#[derive(Debug, Clone, Default)]
pub struct HashtableObj {
    pub slots: Slots<i32, i32>,
    pub order: FlushOrder,
}

/// `Save*Handle`: stores handle argument 3 under (parent, child) when it passes the typed check of
/// `kind`. The old owned reference is released first, then the new one is taken (CONFIRMED order).
fn save_handle(game: &RefCell<Game>, a: &[Value], kind: Option<HandleKind>) -> NativeResult {
    let mut gm = game.borrow_mut();
    let Some(table) = table_of(&gm, a) else {
        return NativeResult::Return(Value::Boolean(false));
    };
    let value = a.get(3).and_then(|v| v.as_handle()).unwrap_or(0);
    let Some(kind) = kind else {
        return NativeResult::Return(Value::Boolean(false));
    };
    if gm.handle_of_kind(value, kind).is_none() {
        return NativeResult::Return(Value::Boolean(false));
    }
    let old = slots_mut(&mut gm, table).handles.insert(
        keys(a),
        SavedHandle {
            id: value,
            owned: true,
        },
    );
    if old.is_none() {
        order_mut(&mut gm, table).add_child(int_at(a, 1), int_at(a, 2));
    }
    if let Some(old) = old {
        if old.owned {
            gm.release(old.id);
        }
    }
    gm.add_ref(value);
    NativeResult::Return(Value::Boolean(true))
}

/// `Load*Handle`: the saved id if it still passes the typed check of `kind`, else the null handle.
fn load_handle(game: &RefCell<Game>, a: &[Value], kind: Option<HandleKind>) -> NativeResult {
    let gm = game.borrow();
    let id = table_of(&gm, a).map_or(0, |k| {
        slots(&gm, k).handles.get(&keys(a)).map_or(0, |h| h.id)
    });
    let ok = kind.is_some_and(|kind| gm.handle_of_kind(id, kind).is_some());
    NativeResult::Return(Value::Handle(if ok { id } else { 0 }))
}

/// The hashtable object behind argument 0, if it is a live hashtable.
fn table_of(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    let id = a.first()?.as_handle()?;
    gm.handle_of_kind(id, HandleKind::Hashtable)
}

fn slots(gm: &Game, k: ObjKey) -> &Slots<i32, i32> {
    &table(gm, k).slots
}

fn slots_mut(gm: &mut Game, k: ObjKey) -> &mut Slots<i32, i32> {
    &mut table_mut(gm, k).slots
}

fn order_mut(gm: &mut Game, k: ObjKey) -> &mut FlushOrder {
    &mut table_mut(gm, k).order
}

fn table(gm: &Game, k: ObjKey) -> &HashtableObj {
    match gm.object(k) {
        Object::Hashtable(t) => t,
        _ => unreachable!("table on a non-hashtable key"),
    }
}

fn table_mut(gm: &mut Game, k: ObjKey) -> &mut HashtableObj {
    match gm.object_mut(k) {
        Object::Hashtable(t) => t,
        _ => unreachable!("table_mut on a non-hashtable key"),
    }
}

/// A save under parent `a[1]` creates the parent's node in the flush order if it does not exist.
fn ensure_node(gm: &mut Game, k: ObjKey, a: &[Value]) {
    order_mut(gm, k).ensure_parent(int_at(a, 1));
}

/// Removes the saved handles of `children` (in the order given) from `slots`, returning the owned
/// ids in that order. The handles are released by the caller.
fn release_children(slots: &mut Slots<i32, i32>, parent: i32, children: &[i32]) -> Vec<u32> {
    let mut released = Vec::new();
    for &child in children {
        if let Some(h) = slots.handles.remove(&(parent, child)) {
            if h.owned {
                released.push(h.id);
            }
        }
    }
    released
}

/// The (parent, child) pair of arguments 1 and 2.
fn keys(a: &[Value]) -> (i32, i32) {
    (int_at(a, 1), int_at(a, 2))
}

pub(super) fn int_at(a: &[Value], i: usize) -> i32 {
    a.get(i).and_then(|v| v.as_integer()).unwrap_or(0)
}

pub(super) fn real_at(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

pub(super) fn bool_at(a: &[Value], i: usize) -> bool {
    a.get(i).and_then(|v| v.as_boolean()).unwrap_or(false)
}

/// The text of the string argument `i`. Strings are interned by the VM, so the text is the key.
pub(super) fn str_at(ctx: &NativeCtx<'_>, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(id)) => ctx.string(*id).to_string(),
        _ => String::new(),
    }
}
