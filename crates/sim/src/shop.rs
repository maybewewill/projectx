//! Shop stock lists: the dynamic sell-items / sell-units ability state (`docs/spec/shop-stock.md`).
//!
//! A shop unit holds one list per kind in its `Asid` (items) or `Asud` (units) ability: 12 slot records of which the
//! first `cap` (0 to 11) are usable. Each slot has a type id, a depleted flag, an age counter (least recently
//! added = largest age) and a stock property (current value and maximum, both reals). The natives only add, remove
//! and resize; buying and the restock expiry belong to systems this crate does not model, so a started restock timer
//! is recorded in the slot (`restock`) and nothing fires it.
//!
//! The rules are those of `0x6F036D60` (add), `0x6F036F50` (remove) and `0x6F036FB0` (set slots); the unit twins
//! `0x6F036890`, `0x6F036A80` and `0x6F036AE0` have the same instruction streams. Checked in Unicorn by
//! `tools/emu/shop_stock_emu.py` (vectors in `tests/shop_stock_vectors.txt`).

use softfloat::{arith, conv, F32};

/// Base code of the item shop ability (`Asid`).
pub const SELL_ITEMS: u32 = u32::from_be_bytes(*b"Asid");
/// Base code of the unit shop ability (`Asud`).
pub const SELL_UNITS: u32 = u32::from_be_bytes(*b"Asud");
/// Slot records per list.
pub const SLOTS: usize = 12;
/// Largest slot count `SetXTypeSlots` keeps (larger or negative requests become this).
pub const MAX_CAP: u32 = 11;
/// The restock threshold: a stock whose distance from its maximum is at least this (2.0e-7, the constant at
/// `0x6FAAE614`) starts a restock timer.
pub const RESTOCK_EPSILON_BITS: u32 = 0x3456_BF95;

/// What the type record gives a stock when the script does not (`vtable +0x330` / `+0x334`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeStock {
    /// The type's `stockMax` (0 when the type does not exist).
    pub stock_max: i32,
    /// The type's `stockRegen` seconds (1.0 for a missing item type, 0.0 for a missing unit type).
    pub regen: F32,
}

/// One slot record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// Type id, 0 when free.
    pub id: u32,
    /// Flag bit 0 of the slot word: the stock was added with a current value of 0.
    pub depleted: bool,
    /// Add counter: 0 for the newest slot, larger for older ones.
    pub age: u32,
    /// Current stock (a real).
    pub value: F32,
    /// Maximum stock (a real).
    pub max: F32,
    /// The restock interval of the timer started by the last add, if one is running (not fired by this crate).
    pub restock: Option<F32>,
}

/// One shop list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockList {
    /// Usable slots, 0 to 11.
    pub cap: u32,
    pub slots: [Slot; SLOTS],
}

impl Default for StockList {
    /// A fresh list: 11 slots, nothing in them. The constructor of the original is not read, so the capacity and the
    /// initial ages are UNSURE (the stock data of the ability definition would fill it).
    fn default() -> Self {
        let slot = Slot { id: 0, depleted: false, age: 0, value: F32::ZERO, max: F32::ZERO, restock: None };
        StockList { cap: MAX_CAP, slots: [slot; SLOTS] }
    }
}

impl StockList {
    /// `AddItemToStock` / `AddUnitToStock` on this list (`0x6F036D60`): `id` with `current` and `max_arg` (0 or -1
    /// means "the type's stockMax"). The slot is the one already holding `id` among the first `cap`, otherwise the
    /// first slot with the largest age among them (slot 0 when `cap` is 0). Returns the slot used.
    pub fn add(&mut self, id: u32, current: i32, max_arg: i32, ty: TypeStock) -> usize {
        let cap = self.cap as usize;
        let mut best = 0usize;
        let mut found = None;
        for i in 0..cap.min(SLOTS) {
            if (self.slots[i].age as i32) > (self.slots[best].age as i32) {
                best = i;
            }
            if self.slots[i].id == id {
                found = Some(i);
                break;
            }
        }
        let slot = found.unwrap_or(best);
        if found.is_none() {
            self.slots[slot].id = id;
        }
        let max = if max_arg == 0 || max_arg == -1 { conv::i2r(ty.stock_max) } else { conv::i2r(max_arg) };
        let value = conv::i2r(current);
        let s = &mut self.slots[slot];
        s.value = value;
        s.max = max;
        s.depleted = false;
        s.restock = None;
        let d = arith::sub(s.value, s.max);
        if f32::from_bits(RESTOCK_EPSILON_BITS) <= f32::from_bits(d.to_bits() & 0x7FFF_FFFF) {
            s.restock = Some(ty.regen);
        }
        if current == 0 {
            s.depleted = true;
        }
        for t in self.slots.iter_mut() {
            t.age = t.age.wrapping_add(1);
        }
        self.slots[slot].age = 0;
        slot
    }

    /// `RemoveItemFromStock` / `RemoveUnitFromStock` (`0x6F036F50`): the first of all 12 records holding `id` is
    /// freed (id 0, timer cancelled, depleted flag cleared); value, maximum and age stay. Returns the slot.
    pub fn remove(&mut self, id: u32) -> Option<usize> {
        let i = self.slots.iter().position(|s| s.id == id)?;
        let s = &mut self.slots[i];
        s.id = 0;
        s.restock = None;
        s.depleted = false;
        Some(i)
    }

    /// `SetItemTypeSlots` / `SetUnitTypeSlots` (`0x6F036FB0`): the count is `n` below 11 and 11 otherwise (negative
    /// counts included). Records from the new count up are cleared and get the age (largest age + 1).
    pub fn set_slots(&mut self, n: i32) {
        let cap = if (n as u32) < MAX_CAP { n as u32 } else { MAX_CAP };
        let age = self.slots.iter().map(|s| s.age).max().unwrap_or(0).wrapping_add(1);
        for s in self.slots.iter_mut().skip(cap as usize) {
            s.depleted = false;
            s.restock = None;
            s.id = 0;
            s.age = age;
        }
        self.cap = cap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TY: TypeStock = TypeStock { stock_max: 3, regen: F32::ONE };

    #[test]
    fn fills_in_order_then_replaces_the_oldest() {
        let mut l = StockList::default();
        l.set_slots(2);
        assert_eq!(l.add(10, 1, 1, TY), 0);
        assert_eq!(l.add(11, 1, 1, TY), 1);
        // slot 0 is older now
        assert_eq!(l.add(12, 1, 1, TY), 0);
        assert_eq!(l.slots[0].id, 12);
        // an existing id is updated in place
        assert_eq!(l.add(11, 2, 5, TY), 1);
        assert_eq!(l.slots[1].value, conv::i2r(2));
        assert_eq!(l.slots[1].max, conv::i2r(5));
        assert!(l.slots[1].restock.is_some());
    }

    #[test]
    fn default_max_and_depleted() {
        let mut l = StockList::default();
        let s = l.add(7, 0, 0, TY);
        assert_eq!(l.slots[s].max, conv::i2r(3));
        assert!(l.slots[s].depleted);
        let s = l.add(8, 3, -1, TY);
        assert_eq!(l.slots[s].restock, None);
    }

    #[test]
    fn remove_and_slots() {
        let mut l = StockList::default();
        l.add(7, 1, 1, TY);
        assert_eq!(l.remove(7), Some(0));
        assert_eq!(l.remove(7), None);
        l.set_slots(-5);
        assert_eq!(l.cap, MAX_CAP);
        l.set_slots(0);
        assert_eq!(l.cap, 0);
        // capacity 0 still writes slot 0
        assert_eq!(l.add(9, 1, 1, TY), 0);
    }
}
