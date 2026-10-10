//! Open list of the region search: a 1-based binary min-heap of 12-byte entries
//! (Game.dll 1.26a: push `0x6F498580`, pop `0x6F4985F0`).
//!
//! Layout as in the original: slot 0 is a sentinel that is never read as an entry, the
//! root is slot 1, the parent of `k` is `k >> 1` and the children are `2k` and `2k + 1`.
//! Each entry is `(key, node, stamp)`; keys are unsigned and compared as unsigned.
//!
//! Tie behaviour (the part that decides pop order for equal keys):
//! * Push: the new entry climbs while its parent's key is greater than or equal to its
//!   own key, so a parent with an equal key moves down and the newer entry ends above it.
//! * Pop: the last entry is sifted down from the root. The right child is chosen when it
//!   exists and its key is less than or equal to the left child's key (ties go right).
//!   The sift stops when the last key is less than or equal to the chosen child's key.
//!
//! Checked against the original by `tools/emu/pathing_vectors.py`: identical pop results
//! and identical array contents after every operation (see `tests/pathing`).

/// One heap entry: 12 bytes in the original.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// Priority (`g + h` in the region search). Smaller pops first.
    pub key: u32,
    /// Node index (the region id in the region search).
    pub node: u32,
    /// Stamp of the node when the entry was pushed. Stale entries are skipped by the caller.
    pub stamp: u32,
}

/// The heap. `slots[0]` is the sentinel; `slots[1..]` are the live entries.
#[derive(Clone, Debug)]
pub struct Heap {
    slots: Vec<Entry>,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    /// An empty heap.
    pub fn new() -> Self {
        Heap {
            slots: vec![Entry::default()],
        }
    }

    /// Number of live entries (the original size counter minus the sentinel).
    pub fn len(&self) -> usize {
        self.slots.len() - 1
    }

    /// True when no entry is live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every entry.
    pub fn clear(&mut self) {
        self.slots.truncate(1);
    }

    /// The raw slots including the sentinel at index 0 (for checks and digests).
    pub fn slots(&self) -> &[Entry] {
        &self.slots
    }

    /// Adds an entry and restores the heap order (original push).
    pub fn push(&mut self, key: u32, node: u32, stamp: u32) {
        self.slots.push(Entry::default());
        let mut i = self.slots.len() - 1;
        while i > 1 {
            let parent = i >> 1;
            if self.slots[parent].key < key {
                break;
            }
            self.slots[i] = self.slots[parent];
            i = parent;
        }
        self.slots[i] = Entry { key, node, stamp };
    }

    /// Removes and returns the smallest entry (original pop), or `None` when empty.
    pub fn pop(&mut self) -> Option<Entry> {
        if self.is_empty() {
            return None;
        }
        let root = self.slots[1];
        // The last slot leaves the array first; it is sifted down from the root.
        let last = self.slots.pop()?;
        if self.slots.len() > 1 {
            let last_index = self.slots.len() - 1;
            let half = last_index >> 1;
            let mut hole = 1;
            if half != 0 {
                loop {
                    let mut child = hole << 1;
                    if child < last_index && self.slots[child + 1].key <= self.slots[child].key {
                        child += 1;
                    }
                    if last.key <= self.slots[child].key {
                        break;
                    }
                    self.slots[hole] = self.slots[child];
                    hole = child;
                    if child > half {
                        break;
                    }
                }
            }
            self.slots[hole] = last;
        }
        Some(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small deterministic generator for the tests (not the game RNG).
    struct Lcg(u64);

    impl Lcg {
        fn next_u32(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 32) as u32
        }
    }

    fn is_heap(h: &Heap) -> bool {
        let s = h.slots();
        (2..s.len()).all(|k| s[k >> 1].key <= s[k].key)
    }

    #[test]
    fn pops_in_key_order() {
        let mut h = Heap::new();
        for (i, key) in [5u32, 1, 4, 1, 9, 0, 7].into_iter().enumerate() {
            h.push(key, i as u32, 0);
            assert!(is_heap(&h));
        }
        let mut keys = Vec::new();
        while let Some(e) = h.pop() {
            keys.push(e.key);
            assert!(is_heap(&h));
        }
        assert_eq!(keys, vec![0, 1, 1, 4, 5, 7, 9]);
        assert!(h.pop().is_none());
    }

    #[test]
    fn random_sequences_keep_the_heap_property() {
        let mut rng = Lcg(7);
        for _ in 0..200 {
            let mut h = Heap::new();
            let mut model: Vec<u32> = Vec::new();
            for _ in 0..120 {
                if !rng.next_u32().is_multiple_of(3) || h.is_empty() {
                    let key = rng.next_u32() % 8;
                    h.push(key, 0, 0);
                    model.push(key);
                } else {
                    let e = h.pop().expect("non-empty");
                    model.sort_unstable();
                    assert_eq!(e.key, model.remove(0));
                }
                assert!(is_heap(&h));
                assert_eq!(h.len(), model.len());
            }
        }
    }

    #[test]
    fn clear_empties_the_heap() {
        let mut h = Heap::new();
        h.push(3, 1, 2);
        h.push(1, 4, 5);
        h.clear();
        assert!(h.is_empty());
        assert_eq!(h.slots().len(), 1);
    }
}
