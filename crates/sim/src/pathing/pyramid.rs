//! The pathing pyramid and its lazy region labeller (Game.dll 1.26a: lookup `0x6F498A60`,
//! labeller `0x6F498A00`, list append `0x6F4984E0`, record copy `0x6F498010`).
//!
//! Spec: `docs/spec/pathing-3.md`. Verified against the original by
//! `tools/emu/pyramid_vectors.py` (Unicorn, 2,000 random cases and 27,871 lookups: return
//! values, records, list entries, counts and capacities compared after every lookup, 0 mismatches).
//!
//! Model:
//! * Four levels, level 0 the finest. Level `k` has records for aligned blocks of `2^k` cells per
//!   side. A record is 8 bytes in the original: a generation stamp and a 32-bit word. The low 16
//!   bits of the word are the region id; the high 16 bits hold the per-path-type 2-bit states.
//! * The state of path type `T` is the pair of bits `(31 - T, 30 - T)` of the word; the lookup
//!   reads `word << T` and keeps bits 30 and 31. State `01` blocks at every level, state `10`
//!   blocks at level 0 and asks for a finer look above it, `00` and `11` are walkable.
//! * [`Pyramid::lookup`] walks up from the query's record while the coarser records have state
//!   00 or 11. The record it labels and returns is the one just below the first coarser record
//!   with a non-zero state (or the level-3 record when none has one), so a uniform coarse block
//!   gives its cell-level or block-level record, not the coarse block's own state.
//! * A record is labelled the first time it is looked up in a generation (stamp != generation):
//!   its id is the current length of the node list, the seed is the query cell, and the node
//!   list entry gets the record's flag byte (bits 16..23 of the word) when flags are enabled.
//!   Ids restart at 0 for every request, because the request resets the list.
//! * The list has a logical capacity and a growth step. An append beyond the capacity with a zero
//!   growth step fails: the record keeps its new stamp and id, and no entry is added.

/// Lookup result for a blocked cell (state `01` at any level, state `10` at level 0).
pub const BLOCKED: i32 = -1;
/// Lookup result for state `10` above level 0: the caller descends one level and asks again.
pub const REFINE: i32 = -2;

/// Link value of a node that is not in the open or closed list (the original's `-1`).
pub const UNTOUCHED: u32 = u32::MAX;
/// Link value of a node in the open list (the original's `-2`).
pub const OPEN: u32 = u32::MAX - 1;
/// Head of an empty closed list (the original's `-3`).
pub const LIST_END: u32 = u32::MAX - 2;
/// Parent of a node that has none.
pub const NO_NODE: u32 = u32::MAX;

/// One pyramid record (8 bytes in the original).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    /// Generation in which the record was last labelled.
    pub stamp: u32,
    /// Low 16 bits: region id. Bits 16..31: per-path-type states (and the flag byte).
    pub word1: u32,
}

/// One pyramid level: `width` by `height` records, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    width: u32,
    height: u32,
    records: Vec<Record>,
}

impl Level {
    /// A level from its words (stamps are 0, as the level loader leaves them).
    ///
    /// Panics if `words.len() != width * height`.
    pub fn new(width: u32, height: u32, words: &[u32]) -> Self {
        assert_eq!(
            words.len(),
            width as usize * height as usize,
            "one word per record"
        );
        Level {
            width,
            height,
            records: words
                .iter()
                .map(|&word1| Record { stamp: 0, word1 })
                .collect(),
        }
    }

    /// Width in records.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in records.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The record array (row-major).
    pub fn records(&self) -> &[Record] {
        &self.records
    }
}

/// A node-list entry as the labeller leaves it, plus the fields the region search uses.
///
/// In the original the entry is 36 bytes. The labeller writes the seed, the flag byte and the
/// level; the other fields start at the values the entry template holds in the executable's
/// data (visit 0, link [`UNTOUCHED`], prev [`UNTOUCHED`], g 0, h 0, parent [`NO_NODE`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// Seed cell (the query cell that labelled the region).
    pub seed: (i32, i32),
    /// Visit counter; heap entries whose stamp differs from it are stale.
    pub visit: u32,
    /// Open or closed list link (see [`UNTOUCHED`], [`OPEN`], [`LIST_END`]).
    pub link: u32,
    /// Previous node in the closed list.
    pub prev: u32,
    /// Cost from the start.
    pub g: u32,
    /// Heuristic to the goal cell.
    pub h: u32,
    /// Parent node, or [`NO_NODE`].
    pub parent: u32,
    /// Flag byte of the record (see [`Labels::flag_on`]).
    pub flag: u8,
    /// Pyramid level of the record (0 means a single cell).
    pub level: u8,
    /// Set on a node reached through a flag link (the parent's flag byte).
    pub via_flag: u8,
}

impl Node {
    /// The entry the labeller appends.
    pub fn labelled(seed: (i32, i32), flag: u8, level: u8) -> Self {
        Node {
            seed,
            visit: 0,
            link: UNTOUCHED,
            prev: UNTOUCHED,
            g: 0,
            h: 0,
            parent: NO_NODE,
            flag,
            level,
            via_flag: 0,
        }
    }
}

/// The state the lookup reads and writes besides the pyramid: the search object's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
    /// Path type `T` (search object `+0xD4`); only its low 5 bits select the shift.
    pub path_type: u32,
    /// Whether labelled nodes take the record's flag byte (search object `+0xD8`).
    pub flag_on: bool,
    /// Current generation (`+0x2C`). Records stamped with it are already labelled.
    pub gen: u32,
    /// The node list (`W`, the region ids are its indices).
    pub nodes: Vec<Node>,
    /// Logical capacity of the list (`W+0x18`).
    pub capacity: usize,
    /// Growth step (`W+0x14`); zero means a full list cannot grow.
    pub grow: usize,
}

impl Labels {
    /// Empty labelling state: generation 1 and no nodes.
    pub fn new(path_type: u32, flag_on: bool, grow: usize, capacity: usize) -> Self {
        Labels {
            path_type,
            flag_on,
            gen: 1,
            nodes: Vec::new(),
            capacity,
            grow,
        }
    }

    /// Appends one entry (`0x6F4984E0` with count 1). Returns false when the list is full and
    /// cannot grow (the entry is then not added).
    fn append(&mut self, node: Node) -> bool {
        let needed = self.nodes.len() + 1;
        if needed > self.capacity {
            if self.grow == 0 {
                return false;
            }
            let short = needed - self.capacity;
            self.capacity += self.grow.max(short);
        }
        self.nodes.push(node);
        true
    }
}

/// The four pyramid levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pyramid {
    levels: [Level; 4],
}

/// The 2-bit state of `word` for shift `t`, in the top two bits (what the lookup tests).
fn state(word: u32, shift: u32) -> u32 {
    (word << shift) & 0xC000_0000
}

impl Pyramid {
    /// A pyramid from its four levels (level 0 first).
    pub fn new(levels: [Level; 4]) -> Self {
        Pyramid { levels }
    }

    /// Level `k` (0 is the finest).
    pub fn level(&self, k: usize) -> &Level {
        &self.levels[k]
    }

    /// Index of the record that covers cell `(x, y)` at `level`, or `None` outside the level.
    pub fn record_index(&self, level: usize, x: i32, y: i32) -> Option<usize> {
        let lv = &self.levels[level];
        let ux = x >> level;
        let uy = y >> level;
        if ux < 0 || uy < 0 || ux as u32 >= lv.width || uy as u32 >= lv.height {
            return None;
        }
        Some(uy as usize * lv.width as usize + ux as usize)
    }

    /// The lookup `0x6F498A60`: returns the region id of cell `(x, y)` seen at `level`
    /// (`0..=3`), [`BLOCKED`] or [`REFINE`].
    ///
    /// Labels the chosen record when it is not yet labelled in `labels.gen`.
    pub fn lookup(&mut self, labels: &mut Labels, level: usize, x: i32, y: i32) -> i32 {
        let shift = labels.path_type & 0x1F;
        let Some(first) = self.record_index(level, x, y) else {
            return BLOCKED;
        };
        match state(self.levels[level].records[first].word1, shift) {
            0x4000_0000 => return BLOCKED,
            0x8000_0000 => return if level == 0 { BLOCKED } else { REFINE },
            _ => {}
        }
        // Walk up while the coarser records are uniform (state 00 or 11); the chosen record is
        // the one just below the first record with a non-zero state, or the top-level record.
        let (mut chosen_level, mut chosen) = (level, first);
        for k in level + 1..self.levels.len() {
            let Some(index) = self.record_index(k, x, y) else {
                // Not reachable with a pyramid whose levels cover the level below them.
                break;
            };
            if state(self.levels[k].records[index].word1, shift) != 0 {
                break;
            }
            (chosen_level, chosen) = (k, index);
        }
        let record = self.levels[chosen_level].records[chosen];
        if record.stamp != labels.gen {
            self.label(labels, chosen_level, chosen, x, y);
        }
        (self.levels[chosen_level].records[chosen].word1 & 0xFFFF) as i32
    }

    /// The labeller `0x6F498A00`: stamps the record, gives it the current list length as its id
    /// and appends a node seeded at the query cell.
    fn label(&mut self, labels: &mut Labels, level: usize, index: usize, x: i32, y: i32) {
        let id = (labels.nodes.len() as u32) & 0xFFFF;
        let record = &mut self.levels[level].records[index];
        record.stamp = labels.gen;
        record.word1 = (record.word1 & 0xFFFF_0000) | id;
        let flag = if labels.flag_on {
            ((record.word1 >> 16) & 0xFF) as u8
        } else {
            0
        };
        // A failed append leaves the record stamped with an id that no entry carries (the
        // original does not check the result either).
        labels.append(Node::labelled((x, y), flag, level as u8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Word with state `s` for path type 0 and a flag byte `flag`.
    fn word(s: u32, flag: u32) -> u32 {
        (s << 30) | (flag << 16)
    }

    /// An 8 by 8 pyramid whose level-0 records are `base` and whose coarser levels are `coarse`.
    fn pyramid(base: u32, coarse: u32) -> Pyramid {
        Pyramid::new([
            Level::new(8, 8, &[base; 64]),
            Level::new(4, 4, &[coarse; 16]),
            Level::new(2, 2, &[coarse; 4]),
            Level::new(1, 1, &[coarse]),
        ])
    }

    #[test]
    fn blocked_states_return_the_documented_codes() {
        let mut labels = Labels::new(0, false, 4, 0);
        let mut p = pyramid(word(1, 0), word(1, 0));
        assert_eq!(p.lookup(&mut labels, 0, 0, 0), BLOCKED);
        assert_eq!(p.lookup(&mut labels, 1, 0, 0), BLOCKED);
        // State 10 blocks at level 0 and asks for a finer look above it.
        let mut p = pyramid(word(2, 0), word(2, 0));
        assert_eq!(p.lookup(&mut labels, 0, 0, 0), BLOCKED);
        assert_eq!(p.lookup(&mut labels, 1, 0, 0), REFINE);
        assert!(labels.nodes.is_empty());
    }

    #[test]
    fn uniform_coarse_block_labels_the_cell_record_as_the_top_record() {
        // Level 0 is walkable, the coarser levels are walkable: the walk reaches level 3.
        let mut labels = Labels::new(0, false, 4, 0);
        let mut p = pyramid(0, 0);
        assert_eq!(p.lookup(&mut labels, 0, 5, 5), 0);
        assert_eq!(labels.nodes.len(), 1);
        assert_eq!(labels.nodes[0].seed, (5, 5));
        assert_eq!(labels.nodes[0].level, 3);
        // Same generation: the record is already labelled and nothing is appended.
        assert_eq!(p.lookup(&mut labels, 0, 0, 0), 0);
        assert_eq!(labels.nodes.len(), 1);
    }

    #[test]
    fn non_zero_coarse_state_stops_the_walk() {
        // Level-1 records in state 01: the walk stops below them, so each cell keeps its own record.
        let mut labels = Labels::new(0, false, 4, 0);
        let mut p = Pyramid::new([
            Level::new(8, 8, &[0; 64]),
            Level::new(4, 4, &[word(1, 0); 16]),
            Level::new(2, 2, &[0; 4]),
            Level::new(1, 1, &[0]),
        ]);
        assert_eq!(p.lookup(&mut labels, 0, 0, 0), 0);
        assert_eq!(p.lookup(&mut labels, 0, 1, 0), 1);
        assert_eq!(labels.nodes[1].level, 0);
    }

    #[test]
    fn full_list_without_growth_keeps_the_stamp_and_adds_nothing() {
        // Level 0 walkable, level 1 in state 11: every cell is its own record.
        let mut labels = Labels::new(0, false, 0, 1);
        let mut p = pyramid(0, word(3, 0));
        assert_eq!(p.lookup(&mut labels, 0, 0, 0), 0);
        assert_eq!(p.lookup(&mut labels, 0, 1, 0), 1);
        assert_eq!(labels.nodes.len(), 1);
        assert_eq!(labels.capacity, 1);
        // The failed record keeps id 1 (the length at labelling) and its stamp.
        assert_eq!(p.lookup(&mut labels, 0, 1, 0), 1);
        assert_eq!(labels.nodes.len(), 1);
    }

    #[test]
    fn flag_byte_is_copied_only_when_enabled() {
        // Coarse state 11 stops the walk, so the level-0 record (flag 7) is the one labelled.
        let mut labels = Labels::new(0, true, 4, 0);
        let mut p = pyramid(word(0, 7), word(3, 0));
        assert_eq!(p.lookup(&mut labels, 0, 2, 3), 0);
        assert_eq!(labels.nodes[0].flag, 7);
        let mut labels = Labels::new(0, false, 4, 0);
        let mut p = pyramid(word(0, 7), word(3, 0));
        p.lookup(&mut labels, 0, 2, 3);
        assert_eq!(labels.nodes[0].flag, 0);
    }

    #[test]
    fn cells_outside_the_pyramid_are_blocked() {
        let mut labels = Labels::new(0, false, 4, 0);
        let mut p = pyramid(0, 0);
        assert_eq!(p.lookup(&mut labels, 0, -1, 0), BLOCKED);
        assert_eq!(p.lookup(&mut labels, 0, 8, 0), BLOCKED);
        assert_eq!(p.lookup(&mut labels, 0, 0, 8), BLOCKED);
        assert!(labels.nodes.is_empty());
    }
}
