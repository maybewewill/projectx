//! Player unit counters: the counts behind `GetPlayerUnitCount`, `GetPlayerStructureCount` and
//! `GetPlayerTypedUnitCount` (docs/spec/natives-pending.md section 1).
//!
//! The original keeps, per player, a counter object at `player+0x2D0`. It has four totals (the structure
//! pair at `+0x54` in progress and `+0x58` completed, the unit pair at `+0x5C` in progress and `+0x60`
//! completed), and one record per unit type (created on first touch) with four values: `+0x1C` completed
//! (entries with the flag clear), `+0x20` in progress (flag clear), `+0x24` in progress (flag set), `+0x28`
//! total of completed and finished units. The record's `+0x30` class word is 2, 4 or 8 and is set once, at
//! creation: 4 for a structure, 8 for another unit type, 2 for a type of a third class that is not counted in
//! the totals. Every change goes through the five methods below. The rules are read from the disassembly of
//! `0x6F403770`, `0x6F4037B0`, `0x6F403810`, `0x6F403860` and `0x6F403890`, and checked against the original
//! in Unicorn (`tools/emu/unit_counts_emu.py`, 1,900 operations and 3,800 typed walks, 0 mismatches).
//!
//! The typed count walks the type tree (`0x6F4038D0`): the record's value for the query (`+0x28`, plus
//! `+0x20` and `+0x24` when in-progress units are included), then the children, then (when the upgrades flag
//! is set) the second list with the in-progress bit forced on. The lists come from the unit data through
//! [`TypeLinks`]. The column each list is read from is not identified (UNSURE).
//!
//! Which unit events call the updates is in [`crate::natives::unit`]: creation adds a completed unit, removal
//! takes it out, and an owner change moves it. Nothing creates an in-progress unit yet (no construction
//! model), so the in-progress methods are exercised only by the tests.

use std::collections::BTreeMap;

/// Class word 2: a type that the totals do not count (the original's predicate `0x6F2B0000`, not identified).
pub const CLASS_OTHER: u32 = 2;
/// Class word 4: a structure. Counted in the structure pair.
pub const CLASS_STRUCTURE: u32 = 4;
/// Class word 8: any other unit type. Counted in the unit pair.
pub const CLASS_UNIT: u32 = 8;

/// The unit registration word of a unit (`unit+0x64`, bits 0x8 and 0x10): what the unit currently adds to
/// the counters. The original sets it when a unit is registered and clears it when the registration is undone.
pub const REGISTERED_IN_PROGRESS: u32 = 0x8;
/// See [`REGISTERED_IN_PROGRESS`].
pub const REGISTERED_COMPLETED: u32 = 0x10;

/// The per-type record of a player's counter (the object created by `0x6F4019F0`, the fields used by the
/// counter updates).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TypeRecord {
    /// The class word (record `+0x30`): [`CLASS_OTHER`], [`CLASS_STRUCTURE`] or [`CLASS_UNIT`].
    pub class: u32,
    /// `+0x1C`: completed units added with the flag clear (never decremented by the removal path).
    pub completed_flag_clear: i32,
    /// `+0x20`: units in progress with the flag clear.
    pub in_progress_flag_clear: i32,
    /// `+0x24`: units in progress with the flag set.
    pub in_progress_flag_set: i32,
    /// `+0x28`: completed and finished units (the value the typed count uses by default).
    pub total: i32,
}

/// The type tree the typed count walks. The lists are read from the unit data by the host.
pub trait TypeLinks {
    /// The first list of a type (walked whenever the query does not have the 0x20 flag, which the natives never
    /// set). UNSURE: the data column.
    fn children(&self, type_id: u32) -> &[u32];
    /// The second list (walked only when the query has the upgrades flag). UNSURE: the data column (the
    /// upgrade list is the likely source).
    fn upgrades(&self, type_id: u32) -> &[u32];
}

/// A tree with no links (every type counts only itself).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLinks;

impl TypeLinks for NoLinks {
    fn children(&self, _: u32) -> &[u32] {
        &[]
    }
    fn upgrades(&self, _: u32) -> &[u32] {
        &[]
    }
}

/// The counters of one player.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerCounts {
    /// `+0x54`: structures in progress.
    pub structures_in_progress: i32,
    /// `+0x58`: structures completed.
    pub structures_completed: i32,
    /// `+0x5C`: units in progress.
    pub units_in_progress: i32,
    /// `+0x60`: units completed.
    pub units_completed: i32,
    records: BTreeMap<u32, TypeRecord>,
}

impl PlayerCounts {
    /// An empty counter (all zero, no records).
    pub fn new() -> Self {
        Self::default()
    }

    /// The record of `type_id`, if one was created.
    pub fn record(&self, type_id: u32) -> Option<&TypeRecord> {
        self.records.get(&type_id)
    }

    /// The record of `type_id`, created with `class` when it is first touched (the original creates it in
    /// the same call that first updates it).
    fn record_mut(&mut self, type_id: u32, class: u32) -> &mut TypeRecord {
        self.records.entry(type_id).or_insert(TypeRecord {
            class,
            ..TypeRecord::default()
        })
    }

    /// A completed unit is added (`0x6F403770`, wrapper `0x6F40FD30`). `flag` is the argument that splits the
    /// record's counts (the original passes the creation flag `0x20` of the unit builder here).
    pub fn add_completed(&mut self, type_id: u32, class: u32, flag: bool) {
        let r = self.record_mut(type_id, class);
        r.total = r.total.wrapping_add(1);
        if !flag {
            r.completed_flag_clear = r.completed_flag_clear.wrapping_add(1);
        }
        let class = r.class;
        self.bump_completed(class, 1);
    }

    /// An in-progress unit is removed (`0x6F4037B0`, wrapper `0x6F40FD70`).
    pub fn remove_in_progress(&mut self, type_id: u32, class: u32, flag: bool) {
        let r = self.record_mut(type_id, class);
        if flag {
            r.in_progress_flag_set = r.in_progress_flag_set.wrapping_sub(1);
        } else {
            r.in_progress_flag_clear = r.in_progress_flag_clear.wrapping_sub(1);
        }
        let class = r.class;
        self.bump_in_progress(class, -1);
    }

    /// An in-progress unit is finished (`0x6F403810`, wrapper `0x6F40FDB0`): it leaves the in-progress count and
    /// joins the completed one. The total is incremented as well.
    pub fn finish(&mut self, type_id: u32, class: u32, flag: bool) {
        let r = self.record_mut(type_id, class);
        r.total = r.total.wrapping_add(1);
        if flag {
            r.in_progress_flag_set = r.in_progress_flag_set.wrapping_sub(1);
        } else {
            r.in_progress_flag_clear = r.in_progress_flag_clear.wrapping_sub(1);
            r.completed_flag_clear = r.completed_flag_clear.wrapping_add(1);
        }
        let class = r.class;
        self.bump_in_progress(class, -1);
        self.bump_completed(class, 1);
    }

    /// A completed unit is removed (`0x6F403860`, wrapper `0x6F40FDF0`). The record's `+0x1C` is not changed.
    pub fn remove_completed(&mut self, type_id: u32, class: u32) {
        let r = self.record_mut(type_id, class);
        r.total = r.total.wrapping_sub(1);
        let class = r.class;
        self.bump_completed(class, -1);
    }

    /// An in-progress unit starts (`0x6F403890`, wrapper `0x6F40FE30`). The total is not changed.
    pub fn begin_in_progress(&mut self, type_id: u32, class: u32, flag: bool) {
        let r = self.record_mut(type_id, class);
        if flag {
            r.in_progress_flag_set = r.in_progress_flag_set.wrapping_add(1);
        } else {
            r.in_progress_flag_clear = r.in_progress_flag_clear.wrapping_add(1);
        }
        let class = r.class;
        self.bump_in_progress(class, 1);
    }

    /// `GetPlayerUnitCount`: the completed units, plus the units in progress when `include_in_progress`
    /// (`0x6F3DC410`).
    pub fn unit_count(&self, include_in_progress: bool) -> i32 {
        let mut n = self.units_completed;
        if include_in_progress {
            n = n.wrapping_add(self.units_in_progress);
        }
        n
    }

    /// `GetPlayerStructureCount`: the completed structures, plus those in progress when asked (`0x6F3DC430`).
    pub fn structure_count(&self, include_in_progress: bool) -> i32 {
        let mut n = self.structures_completed;
        if include_in_progress {
            n = n.wrapping_add(self.structures_in_progress);
        }
        n
    }

    /// `GetPlayerTypedUnitCount` for the type `type_id` (`0x6F4038D0`, flags as the native sets them: 0x10,
    /// plus 0x2 and 0x4 for in-progress units, plus 0x8 for the upgrades list). A type with no record counts 0
    /// for itself but its links are still walked, as in the original.
    pub fn typed_count<L: TypeLinks>(
        &self,
        links: &L,
        type_id: u32,
        include_in_progress: bool,
        include_upgrades: bool,
    ) -> i32 {
        let mut flags = 0x10u32;
        if include_in_progress {
            flags |= 0x16;
        }
        if include_upgrades {
            flags |= 0x8;
        }
        self.typed_walk(links, type_id, flags)
    }

    /// The typed walk with the raw flags of the original (`0x6F4038D0`): 0x1 completed (flag clear), 0x10 total,
    /// 0x2 and 0x4 in progress (flag clear and set), 0x8 the upgrades list, 0x20 no children. Used by the
    /// natives through [`PlayerCounts::typed_count`] and by the vector test.
    pub fn typed_walk<L: TypeLinks>(&self, links: &L, type_id: u32, flags: u32) -> i32 {
        let mut total = 0i32;
        if let Some(r) = self.records.get(&type_id) {
            if flags & 1 != 0 {
                total = r.completed_flag_clear;
            } else if flags & 0x10 != 0 {
                total = r.total;
            }
            if flags & 2 != 0 {
                total = total.wrapping_add(r.in_progress_flag_clear);
            }
            if flags & 4 != 0 {
                total = total.wrapping_add(r.in_progress_flag_set);
            }
        }
        if flags & 0x20 == 0 {
            for &c in links.children(type_id) {
                total = total.wrapping_add(self.typed_walk(links, c, flags));
            }
        }
        if flags & 8 != 0 {
            for &c in links.upgrades(type_id) {
                total = total.wrapping_add(self.typed_walk(links, c, flags | 2));
            }
        }
        total
    }

    fn bump_completed(&mut self, class: u32, d: i32) {
        if class & CLASS_STRUCTURE != 0 {
            self.structures_completed = self.structures_completed.wrapping_add(d);
        } else if class & CLASS_UNIT != 0 {
            self.units_completed = self.units_completed.wrapping_add(d);
        }
    }

    fn bump_in_progress(&mut self, class: u32, d: i32) {
        if class & CLASS_STRUCTURE != 0 {
            self.structures_in_progress = self.structures_in_progress.wrapping_add(d);
        } else if class & CLASS_UNIT != 0 {
            self.units_in_progress = self.units_in_progress.wrapping_add(d);
        }
    }
}

/// The class word of a unit type for the counters: 4 for a building, 8 for any other unit type.
/// `is_building` is the `isbldg` column (LIKELY, see the module notes; the original's structure test
/// `0x6F32CC80` reads a flag of its own type table, not identified).
pub fn class_of(is_building: bool) -> u32 {
    if is_building {
        CLASS_STRUCTURE
    } else {
        CLASS_UNIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Links {
        children: BTreeMap<u32, Vec<u32>>,
        upgrades: BTreeMap<u32, Vec<u32>>,
    }

    impl TypeLinks for Links {
        fn children(&self, t: u32) -> &[u32] {
            self.children.get(&t).map(Vec::as_slice).unwrap_or(&[])
        }
        fn upgrades(&self, t: u32) -> &[u32] {
            self.upgrades.get(&t).map(Vec::as_slice).unwrap_or(&[])
        }
    }

    #[test]
    fn completed_unit_counts_in_the_unit_pair_and_structure_in_the_structure_pair() {
        let mut c = PlayerCounts::new();
        c.add_completed(1, CLASS_UNIT, false);
        c.add_completed(2, CLASS_STRUCTURE, false);
        assert_eq!((c.units_completed, c.structures_completed), (1, 1));
        assert_eq!(c.record(1).unwrap().completed_flag_clear, 1);
        assert_eq!(c.record(1).unwrap().total, 1);
    }

    #[test]
    fn other_class_is_not_in_the_totals_but_has_a_record() {
        let mut c = PlayerCounts::new();
        c.add_completed(9, CLASS_OTHER, false);
        assert_eq!(c.units_completed, 0);
        assert_eq!(c.structures_completed, 0);
        assert_eq!(c.record(9).unwrap().total, 1);
        assert_eq!(c.typed_count(&NoLinks, 9, false, false), 1);
    }

    #[test]
    fn in_progress_is_counted_only_when_asked() {
        let mut c = PlayerCounts::new();
        c.begin_in_progress(1, CLASS_UNIT, false);
        c.begin_in_progress(1, CLASS_UNIT, true);
        assert_eq!(c.units_in_progress, 2);
        assert_eq!(c.unit_count(false), 0);
        assert_eq!(c.unit_count(true), 2);
        assert_eq!(c.typed_count(&NoLinks, 1, false, false), 0);
        assert_eq!(c.typed_count(&NoLinks, 1, true, false), 2);
    }

    #[test]
    fn finish_moves_an_in_progress_unit_to_completed_and_bumps_the_total() {
        let mut c = PlayerCounts::new();
        c.begin_in_progress(3, CLASS_STRUCTURE, false);
        assert_eq!(c.structures_in_progress, 1);
        c.finish(3, CLASS_STRUCTURE, false);
        assert_eq!(c.structures_in_progress, 0);
        assert_eq!(c.structures_completed, 1);
        let r = c.record(3).unwrap();
        assert_eq!((r.in_progress_flag_clear, r.completed_flag_clear, r.total), (0, 1, 1));
    }

    #[test]
    fn finish_with_the_flag_set_does_not_touch_the_completed_record_count() {
        let mut c = PlayerCounts::new();
        c.begin_in_progress(4, CLASS_UNIT, true);
        c.finish(4, CLASS_UNIT, true);
        let r = c.record(4).unwrap();
        assert_eq!((r.in_progress_flag_set, r.completed_flag_clear, r.total), (0, 0, 1));
        assert_eq!(c.units_completed, 1);
    }

    #[test]
    fn remove_completed_takes_the_total_back_but_not_the_completed_record_count() {
        let mut c = PlayerCounts::new();
        c.add_completed(1, CLASS_UNIT, false);
        c.remove_completed(1, CLASS_UNIT);
        assert_eq!(c.units_completed, 0);
        let r = c.record(1).unwrap();
        assert_eq!((r.total, r.completed_flag_clear), (0, 1));
    }

    #[test]
    fn remove_in_progress_undoes_begin() {
        let mut c = PlayerCounts::new();
        c.begin_in_progress(5, CLASS_UNIT, false);
        c.remove_in_progress(5, CLASS_UNIT, false);
        assert_eq!(c.units_in_progress, 0);
        assert_eq!(c.record(5).unwrap().in_progress_flag_clear, 0);
    }

    #[test]
    fn class_is_fixed_by_the_first_touch() {
        let mut c = PlayerCounts::new();
        c.add_completed(7, CLASS_STRUCTURE, false);
        // A later call with another class uses the record's class (the original stores it once).
        c.add_completed(7, CLASS_UNIT, false);
        assert_eq!(c.structures_completed, 2);
        assert_eq!(c.units_completed, 0);
    }

    #[test]
    fn typed_walk_adds_children_and_upgrades_as_flagged() {
        let mut links = Links::default();
        links.children.insert(1, vec![2, 3]);
        links.upgrades.insert(1, vec![4]);
        let mut c = PlayerCounts::new();
        for _ in 0..2 {
            c.add_completed(1, CLASS_UNIT, false);
        }
        c.add_completed(2, CLASS_UNIT, false);
        c.add_completed(3, CLASS_UNIT, false);
        c.begin_in_progress(4, CLASS_UNIT, false);
        // Type 1: its own total 2, children 2 and 3 (1 each). Type 4 (upgrade) has one unit in progress,
        // which counts only when both the upgrades flag and the in-progress flag are set.
        assert_eq!(c.typed_count(&links, 1, false, false), 4);
        assert_eq!(c.typed_count(&links, 1, true, false), 4);
        assert_eq!(c.typed_count(&links, 1, false, true), 5);
        assert_eq!(c.typed_count(&links, 1, true, true), 5);
    }

    #[test]
    fn typed_walk_of_an_unknown_type_is_zero_without_links() {
        let c = PlayerCounts::new();
        assert_eq!(c.typed_count(&NoLinks, 42, true, true), 0);
    }

    #[test]
    fn class_of_building_is_structure() {
        assert_eq!(class_of(true), CLASS_STRUCTURE);
        assert_eq!(class_of(false), CLASS_UNIT);
    }
}
