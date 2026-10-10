//! JASS handle table: id allocation, reference counts and the object-to-id map, as in the original
//! `Game.dll` 1.26a (docs/spec/handles-2.md, emulated in `tools/emu/handle_alloc_emu.py`).
//!
//! Rules (CONFIRMED unless marked):
//!
//! * A handle value is its id. Ids start at [`HANDLE_BASE`] (`0x100000`); any value below it is the
//!   null handle and never refers to an object.
//! * A fresh id is `HANDLE_BASE + table length`, where the table length counts every entry ever
//!   pushed (freed entries are kept). Ids are therefore issued in order.
//! * A released id goes on a LIFO free stack and is the next id handed out. The stack is consulted
//!   before the table grows.
//! * Each entry has a reference count. Creating an object gives it count 1. [`HandleTable::add_ref`]
//!   adds one, [`HandleTable::release`] removes one, and the id is pushed on the free stack when the
//!   count reaches 0. A reserved id (count > 0) keeps its number after its object is gone.
//! * Creating an object that already has a live id returns that id and changes nothing.
//! * [`HandleTable::object_died`] releases the creation reference once (LIKELY: the original's agent
//!   death event 0x80267 does this). Afterwards the entry no longer names the object, so
//!   [`HandleTable::get`] returns `None` for that id while other holders keep the id reserved.
//! * [`HandleTable::reset`] (called when a game state is built) leaves a dummy entry in slot 0, so the
//!   first real id is `0x100001` (LIKELY: the reset routine 0x6f42d2e0 pre-sizes the table to 1).
//!
//! Determinism: the object-to-id map is a `BTreeMap`, so no iteration order depends on a random seed.
//! The generic parameter `O` is the object key (called `ObjectRef` in the design notes). It must be
//! totally ordered and cheap to clone, e.g. a `u32` index into the simulation's object arena.
//!
//! Limits: add_ref and release on an id that was never issued (beyond the table) are ignored; the
//! original reads past its vector there (undefined). Ids below `HANDLE_BASE` are always ignored.
//! The original's destroy notify (entry flag non-zero) is not modelled: [`HandleTable::flag`] only
//! reports the stored flag.

use std::collections::BTreeMap;

/// The first handle id is `HANDLE_BASE + 1` (slot 0 is the dummy entry). CONFIRMED base.
pub const HANDLE_BASE: u32 = 0x0010_0000;

/// One table entry. The original stores 12 bytes (count, agent pointer, flag); `object` is `None`
/// once the object is gone or the entry is free.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry<O> {
    refcount: u32,
    object: Option<O>,
    flag: u32,
}

/// Handle ids for objects of type `O` (see the module documentation).
#[derive(Debug, Clone)]
pub struct HandleTable<O> {
    /// Slot `i` holds id `HANDLE_BASE + i`. Slot 0 is the dummy entry after a reset.
    entries: Vec<Entry<O>>,
    /// Released ids, last released on top.
    free: Vec<u32>,
    /// Object to id. An entry can be stale (see `live_id`): the id was released and reused.
    by_object: BTreeMap<O, u32>,
}

impl<O: Ord + Clone> HandleTable<O> {
    /// A fresh table, the same state as after [`HandleTable::reset`].
    pub fn new() -> Self {
        let mut t = HandleTable {
            entries: Vec::new(),
            free: Vec::new(),
            by_object: BTreeMap::new(),
        };
        t.reset();
        t
    }

    /// Clears every id and leaves the dummy entry in slot 0 (LIKELY, see the module documentation).
    pub fn reset(&mut self) {
        self.entries.clear();
        self.free.clear();
        self.by_object.clear();
        self.entries.push(Entry {
            refcount: 0,
            object: None,
            flag: 0,
        });
    }

    /// Slot index of `id`, or `None` for null ids and ids beyond the table.
    fn slot(&self, id: u32) -> Option<usize> {
        let i = id.checked_sub(HANDLE_BASE)? as usize;
        (i < self.entries.len()).then_some(i)
    }

    /// The live id of `object`: its map entry, if that entry still names the object. A stale map
    /// entry (its id was released and handed to another object) does not count.
    fn live_id(&self, object: &O) -> Option<u32> {
        let &id = self.by_object.get(object)?;
        let i = self.slot(id)?;
        (self.entries[i].object.as_ref() == Some(object)).then_some(id)
    }

    /// Creates a handle for `object` with count 1 and the given flag, or returns the id the object
    /// already has (its count and flag are then unchanged).
    pub fn create(&mut self, object: O, flag: u32) -> u32 {
        if let Some(id) = self.live_id(&object) {
            return id;
        }
        let id = self
            .free
            .pop()
            .unwrap_or(HANDLE_BASE + self.entries.len() as u32);
        let entry = Entry {
            refcount: 1,
            object: Some(object.clone()),
            flag,
        };
        match self.slot(id) {
            Some(i) => self.entries[i] = entry,
            None => self.entries.push(entry),
        }
        self.by_object.insert(object, id);
        id
    }

    /// Adds one reference to `id` (a hash table save, for example). Null ids are ignored.
    pub fn add_ref(&mut self, id: u32) {
        if let Some(i) = self.slot(id) {
            let e = &mut self.entries[i];
            e.refcount = e.refcount.wrapping_add(1);
        }
    }

    /// Removes one reference from `id`. When the count reaches 0 the entry is cleared, its object
    /// link is dropped and the id is pushed on the free stack. Null ids are ignored, and so is a
    /// release of an id whose count is already 0 (the original does not decrement below zero and
    /// does not push again; CONFIRMED by the Unicorn vectors in `tests/handles.rs`).
    pub fn release(&mut self, id: u32) {
        let Some(i) = self.slot(id) else { return };
        let e = &mut self.entries[i];
        if e.refcount == 0 {
            return;
        }
        e.refcount -= 1;
        if e.refcount == 0 {
            if let Some(object) = e.object.take() {
                if self.by_object.get(&object) == Some(&id) {
                    self.by_object.remove(&object);
                }
            }
            self.free.push(id);
        }
    }

    /// The object died: releases its creation reference once and forgets the object. Returns
    /// `false` (and does nothing) if the object has no live id, for example because its id was
    /// already released to zero.
    pub fn object_died(&mut self, object: &O) -> bool {
        let Some(id) = self.live_id(object) else {
            return false;
        };
        self.by_object.remove(object);
        if let Some(i) = self.slot(id) {
            self.entries[i].object = None;
        }
        self.release(id);
        true
    }

    /// The object of `id`, or `None` for null ids, free ids and ids whose object died.
    pub fn get(&self, id: u32) -> Option<&O> {
        self.entries[self.slot(id)?].object.as_ref()
    }

    /// The live id of `object`, if it has one.
    pub fn id_of(&self, object: &O) -> Option<u32> {
        self.live_id(object)
    }

    /// Reference count of `id`, or `None` for null ids and ids beyond the table.
    pub fn refcount(&self, id: u32) -> Option<u32> {
        Some(self.entries[self.slot(id)?].refcount)
    }

    /// Flag stored at creation, or `None` for null ids and ids beyond the table.
    pub fn flag(&self, id: u32) -> Option<u32> {
        Some(self.entries[self.slot(id)?].flag)
    }

    /// The free stack, bottom first. The last element is the next id handed out.
    pub fn free_ids(&self) -> &[u32] {
        &self.free
    }

    /// Number of table entries, including the dummy and the freed ones.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Always false: the dummy entry is always present.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The id the next allocation would return if the free stack is empty.
    pub fn next_fresh_id(&self) -> u32 {
        HANDLE_BASE + self.entries.len() as u32
    }
}

impl<O: Ord + Clone> Default for HandleTable<O> {
    fn default() -> Self {
        Self::new()
    }
}
