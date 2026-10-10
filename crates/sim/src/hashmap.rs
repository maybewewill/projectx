//! Bucket-chained map that reproduces the iteration order of the JASS hashtable's maps in `Game.dll`
//! 1.26a (`TSHashTable<..., HASHKEY_DWORD>`). The order matters because flushing a hashtable releases
//! the saved handle references in that order, and the release order decides which freed handle id is
//! reused first.
//!
//! Structure (addresses are 1.26a VAs; the facts marked VERIFIED were run in Unicorn against the
//! original by `tools/emu/hashmap_vectors.py`):
//!
//! * Table: vftable `0x6F941DB4` (handle kind; the other kinds share the template). Constructor
//!   `0x6F3ADD60` sets the global sentinel and leaves the mask at -1 (no buckets yet). VERIFIED.
//! * Buckets: created lazily by `0x6F3FD680` with 4 buckets (mask 3). Bucket index = `key & mask`.
//!   VERIFIED (4 buckets, mask 3 on first insert; `key & mask` for placement).
//! * Chain order: a bucket is a circular list with a sentinel in the bucket record. A new element is
//!   appended at the tail (the bucket's last-element field `E+8` moves to it). VERIFIED.
//! * Growth trigger (`0x6F400800`, the parent version `0x6F400940`): before an insert into bucket
//!   `i`, the score decays by 3 (to 0 at least), then one point is added per element already in
//!   bucket `i`; if the score exceeds 13 the table doubles and the insert retries with the new mask.
//!   Nothing grows once the mask is `>= 0x1FFF`. VERIFIED: the grow point of 2064 random sequences
//!   matched the model exactly. The score is not changed by removals or by updates of existing keys.
//! * Destroy order (`0x6F3ABF90` with param 0, the bucket walk used by the node destructor
//!   `0x6F4000D0`): buckets ascending, and inside a bucket the tail first (newest first). VERIFIED for
//!   tables that have not grown (1936 random add/remove/destroy sequences, 0 mismatches).
//! * Removal (`0x6F3ADE10`, the element base destructor, unlinks the element from its chain and the
//!   global list): the order of the remaining elements is kept. VERIFIED for the chain order.
//!
//! NOT VERIFIED: the order after a growth. The original rehashes through `0x6F3FE240` (child) and
//! `0x6F3FE5E0` (parent) with a stack-local temporary list and a bucket array reallocation. The
//! Unicorn harness did not reproduce a correct rehash (elements stayed in their old buckets), so this
//! port uses a documented guess: elements are taken from the old buckets in the original's pop order
//! (buckets ascending, tail first) and each is put back at the head of its new bucket. Once a table
//! has grown, the destroy order of this port is therefore unverified. The divergence can begin at the
//! first insert that reports [`Insert::Grew`]; before that, the order is exact.
//!
//! The global list (sentinel at `H+8`, newest first) affects only the link updates of the removal path
//! and has no effect on the destroy order, so it is not modelled.

/// Result of an insert: whether the table grew on this insert (see the module notes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert {
    /// The element went into an existing bucket; no growth.
    Placed,
    /// The table doubled on this insert (the rehash order is unverified).
    Grew,
}

/// Growth is decided when the chain score exceeds this value (VERIFIED).
pub const GROW_SCORE: u32 = 13;
/// The score decays by this much per insert (VERIFIED).
pub const DECAY: u32 = 3;
/// No growth once the mask reaches this value (VERIFIED).
pub const MAX_MASK_GROW: u32 = 0x1fff;
/// Bucket count of a table that has just received its first element (VERIFIED).
pub const INITIAL_BUCKETS: usize = 4;

/// A map keyed by a 32-bit integer, with the original's bucket layout and iteration order.
///
/// Each bucket keeps its elements in insertion order (the last one is the tail). The iteration order
/// used by the original's flush is [`BucketMap::destroy_order`].
#[derive(Debug, Clone, Default)]
pub struct BucketMap<V> {
    /// Empty until the first insert (the original's mask of -1).
    buckets: Vec<Vec<(u32, V)>>,
    score: u32,
}

impl<V> BucketMap<V> {
    /// An empty map (no buckets yet).
    pub fn new() -> Self {
        BucketMap {
            buckets: Vec::new(),
            score: 0,
        }
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.buckets.iter().map(Vec::len).sum()
    }

    /// True when the map has no element.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn mask(&self) -> u32 {
        (self.buckets.len() as u32).wrapping_sub(1)
    }

    fn bucket_of(&self, key: u32) -> usize {
        (key & self.mask()) as usize
    }

    /// The value stored under `key`, if any. Lookups do not change the order.
    pub fn get(&self, key: u32) -> Option<&V> {
        if self.buckets.is_empty() {
            return None;
        }
        self.buckets[self.bucket_of(key)]
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }

    /// Mutable access to the value under `key`. Updating in place keeps the element's position.
    pub fn get_mut(&mut self, key: u32) -> Option<&mut V> {
        if self.buckets.is_empty() {
            return None;
        }
        let b = self.bucket_of(key);
        self.buckets[b]
            .iter_mut()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v)
    }

    /// Appends a new element for `key` (the caller checks that the key is absent). Applies the
    /// growth rule of the original before the append and reports whether the table grew.
    pub fn insert_new(&mut self, key: u32, value: V) -> Insert {
        if self.buckets.is_empty() {
            self.buckets = (0..INITIAL_BUCKETS).map(|_| Vec::new()).collect();
        }
        let mut grew = Insert::Placed;
        if self.mask() < MAX_MASK_GROW {
            self.score = self.score.saturating_sub(DECAY);
            let b = self.bucket_of(key);
            for _ in 0..self.buckets[b].len() {
                self.score += 1;
                if self.score > GROW_SCORE {
                    self.grow();
                    grew = Insert::Grew;
                    break;
                }
            }
        }
        let b = self.bucket_of(key);
        self.buckets[b].push((key, value));
        grew
    }

    /// Doubles the bucket count (UNVERIFIED rehash order, see the module notes).
    fn grow(&mut self) {
        let newcount = self.buckets.len() * 2;
        self.score = 0;
        // Pop every element: buckets ascending, tail first.
        let mut popped: Vec<(u32, V)> = Vec::new();
        for bucket in &mut self.buckets {
            while let Some(e) = bucket.pop() {
                popped.push(e);
            }
        }
        self.buckets = (0..newcount).map(|_| Vec::new()).collect();
        // Put each back at the head of its new bucket.
        for e in popped {
            let b = (e.0 & self.mask()) as usize;
            self.buckets[b].insert(0, e);
        }
    }

    /// Removes the element under `key` and returns its value. The other elements keep their order.
    pub fn remove(&mut self, key: u32) -> Option<V> {
        self.remove_where(key, |_| true)
    }

    /// Removes the first element under `key` whose value satisfies `f`. Used when the element is
    /// known by its value (the map may hold several elements with the same key).
    pub fn remove_where(&mut self, key: u32, mut f: impl FnMut(&V) -> bool) -> Option<V> {
        if self.buckets.is_empty() {
            return None;
        }
        let b = self.bucket_of(key);
        let pos = self.buckets[b]
            .iter()
            .position(|(k, v)| *k == key && f(v))?;
        Some(self.buckets[b].remove(pos).1)
    }

    /// Removes every element and returns the values in the original's destroy order: buckets
    /// ascending, and inside a bucket the tail first. The map is left empty with no buckets.
    pub fn destroy_order(&mut self) -> Vec<V> {
        let mut out = Vec::with_capacity(self.len());
        for bucket in self.buckets.drain(..) {
            out.extend(bucket.into_iter().rev().map(|(_, v)| v));
        }
        self.score = 0;
        out
    }

    /// The keys in destroy order, without changing the map.
    pub fn keys_in_destroy_order(&self) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.len());
        for bucket in &self.buckets {
            out.extend(bucket.iter().rev().map(|(k, _)| *k));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_and_tail_first_destroy() {
        let mut m = BucketMap::new();
        for (k, v) in [(0u32, 'a'), (4, 'b'), (8, 'c'), (1, 'd'), (5, 'e')] {
            assert_eq!(m.insert_new(k, v), Insert::Placed);
        }
        // Bucket 0 holds 0,4,8 and bucket 1 holds 1,5: tail first gives 8,4,0,5,1.
        assert_eq!(m.keys_in_destroy_order(), vec![8, 4, 0, 5, 1]);
        assert_eq!(m.destroy_order(), vec!['c', 'b', 'a', 'e', 'd']);
        assert!(m.is_empty());
    }

    #[test]
    fn removal_keeps_order_and_update_keeps_position() {
        let mut m = BucketMap::new();
        for k in [0u32, 4, 8] {
            m.insert_new(k, k);
        }
        *m.get_mut(4).unwrap() = 40;
        assert_eq!(m.remove(4), Some(40));
        assert_eq!(m.keys_in_destroy_order(), vec![8, 0]);
        assert_eq!(m.get(8), Some(&8));
        assert_eq!(m.remove(4), None);
    }

    #[test]
    fn growth_trigger_matches_score_rule() {
        // Keys 0,4,...,28 all sit in bucket 0. The score after insert i is the decayed score plus i:
        // 0,1,2,3,4,6,9,13 for i = 0..7, so eight keys do not grow the table.
        let mut m = BucketMap::new();
        for i in 0..8u32 {
            assert_eq!(m.insert_new(4 * i, i), Insert::Placed);
        }
        assert_eq!(m.buckets.len(), 4);
        assert_eq!(m.score, 13);
        // The ninth collision exceeds the threshold after four walked elements.
        assert_eq!(m.insert_new(32, 8), Insert::Grew);
        assert_eq!(m.buckets.len(), 8);
        assert_eq!(
            m.score, 0,
            "the score is reset by the growth and the insert does not walk again"
        );
    }
}
