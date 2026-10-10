// Adaptive Huffman sector codec (MPQ compression mask bit 0x01).
//
// Ported from OpenRealm (MIT, (c) 2024 corepunch), common/mpq.c, which follows the StormLib
// description of the format. Only profile 7 (the WC3 voice profile) is implemented.
//
// The tree is a doubly linked list ordered by weight (heaviest first) plus parent/child links.
// Symbols 0..=255 are bytes, 256 is end of stream and 257 is the escape: a new byte follows as
// 8 raw bits and is inserted into the tree. Bits are read LSB first; a 0 bit selects the `low`
// child and a 1 bit selects its sibling (`low.prev`).
//
// Node links are indices into `Tree::nodes`; index 0 is the list head sentinel.

use crate::Error;

const HEAD: usize = 0;
const NIL: usize = usize::MAX;
const SYMBOL_COUNT: usize = 258;
const MAX_NODES: usize = 515;
const END_OF_STREAM: u32 = 256;
const ESCAPE: u32 = 257;
const PROFILE_7: u32 = 7;

/// Initial weights of the byte symbols for profile 7. Zero means "not in the initial tree".
const PROFILE7_WEIGHTS: [u8; 256] = {
    let high = [
        0xc3, 0xd9, 0xef, 0x3d, 0xf9, 0x7c, 0xe9, 0x1e, 0xfd, 0xab, 0xf1, 0x2c, 0xfc, 0x5b, 0xfe,
        0x17,
    ];
    let mid = [
        0xbd, 0xd9, 0xec, 0x3d, 0xf5, 0x7d, 0xe8, 0x1d, 0xfb, 0xae, 0xf0, 0x2c, 0xfb, 0x5c, 0xff,
        0x18,
    ];
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 16 {
        t[i] = high[i];
        t[64 + i] = mid[i];
        i += 1;
    }
    t[128] = 0x70;
    t[129] = 0x6c;
    t
};

#[derive(Clone, Copy)]
struct Node {
    next: usize,
    prev: usize,
    parent: usize,
    low: usize,
    value: u32,
    weight: u32,
}

struct Tree {
    nodes: Vec<Node>,
    symbols: [usize; SYMBOL_COUNT],
}

fn corrupt(msg: impl Into<String>) -> Error {
    Error::Decompress(format!("huffman: {}", msg.into()))
}

impl Tree {
    /// Builds the initial tree for a profile byte.
    fn new(profile: u32) -> Result<Self, Error> {
        if profile != PROFILE_7 {
            return Err(corrupt(format!("unsupported profile {profile}")));
        }
        let mut tree = Tree {
            nodes: vec![Node {
                next: HEAD,
                prev: HEAD,
                parent: NIL,
                low: NIL,
                value: 0,
                weight: 0,
            }],
            symbols: [NIL; SYMBOL_COUNT],
        };
        let mut max_weight = 0;
        for (i, &w) in PROFILE7_WEIGHTS.iter().enumerate() {
            if w == 0 {
                continue;
            }
            let node = tree.new_node(i as u32, u32::from(w), HEAD)?;
            tree.symbols[i] = node;
            max_weight = tree.fix(node, max_weight);
        }
        let tail = tree.nodes[HEAD].prev;
        tree.symbols[END_OF_STREAM as usize] = tree.new_node(END_OF_STREAM, 1, tail)?;
        let tail = tree.nodes[HEAD].prev;
        tree.symbols[ESCAPE as usize] = tree.new_node(ESCAPE, 1, tail)?;

        let mut low = tree.nodes[HEAD].prev;
        while low != HEAD {
            let high = tree.nodes[low].prev;
            if high == HEAD {
                break;
            }
            let weight = tree.nodes[high].weight + tree.nodes[low].weight;
            let parent = tree.new_node(0, weight, HEAD)?;
            tree.nodes[low].parent = parent;
            tree.nodes[high].parent = parent;
            tree.nodes[parent].low = low;
            max_weight = tree.fix(parent, max_weight);
            low = tree.nodes[high].prev;
        }
        Ok(tree)
    }

    fn new_node(&mut self, value: u32, weight: u32, at: usize) -> Result<usize, Error> {
        // `nodes[0]` is the head sentinel, which is not counted against the capacity.
        if self.nodes.len() > MAX_NODES {
            return Err(corrupt("tree is full"));
        }
        let id = self.nodes.len();
        self.nodes.push(Node {
            next: NIL,
            prev: NIL,
            parent: NIL,
            low: NIL,
            value,
            weight,
        });
        self.link(at, id);
        Ok(id)
    }

    /// Inserts `node` right after `at` in the list.
    fn link(&mut self, at: usize, node: usize) {
        let next = self.nodes[at].next;
        self.nodes[node].next = next;
        self.nodes[node].prev = at;
        self.nodes[next].prev = node;
        self.nodes[at].next = node;
    }

    fn remove(&mut self, node: usize) {
        let (prev, next) = (self.nodes[node].prev, self.nodes[node].next);
        if next == NIL {
            return;
        }
        self.nodes[prev].next = next;
        self.nodes[next].prev = prev;
        self.nodes[node].next = NIL;
        self.nodes[node].prev = NIL;
    }

    /// First node at or before `node` (walking towards the head) whose weight is at least `weight`.
    fn higher(&self, mut node: usize, weight: u32) -> usize {
        while node != HEAD {
            if self.nodes[node].weight >= weight {
                return node;
            }
            node = self.nodes[node].prev;
        }
        HEAD
    }

    /// Moves a node that is heavier than the current maximum into weight order.
    fn fix(&mut self, node: usize, max_weight: u32) -> u32 {
        let weight = self.nodes[node].weight;
        if weight >= max_weight {
            return weight;
        }
        let higher = self.higher(self.nodes[HEAD].prev, weight);
        self.remove(node);
        self.link(higher, node);
        max_weight
    }

    /// Increments the weights from `start` up to the root, swapping nodes to keep weight order.
    fn rebalance(&mut self, start: usize) {
        let mut node = start;
        while node != NIL {
            self.nodes[node].weight += 1;
            let higher = self.higher(self.nodes[node].prev, self.nodes[node].weight);
            let swap = self.nodes[higher].next;
            if swap != node {
                self.remove(swap);
                self.link(node, swap);
                self.remove(node);
                self.link(higher, node);
                let swap_parent = self.nodes[swap].parent;
                let low = self.nodes[swap_parent].low;
                let parent = self.nodes[node].parent;
                if self.nodes[parent].low == node {
                    self.nodes[parent].low = swap;
                }
                if low == swap {
                    self.nodes[swap_parent].low = node;
                }
                self.nodes[node].parent = swap_parent;
                self.nodes[swap].parent = parent;
            }
            node = self.nodes[node].parent;
        }
    }

    /// Splits the escape node (the list tail) and adds a leaf for `value`. Returns the new leaf.
    fn insert(&mut self, value: u32) -> Result<usize, Error> {
        let escape = self.nodes[HEAD].prev;
        let (escape_value, escape_weight) = (self.nodes[escape].value, self.nodes[escape].weight);
        let tail = self.nodes[HEAD].prev;
        let high = self.new_node(escape_value, escape_weight, tail)?;
        let tail = self.nodes[HEAD].prev;
        let low = self.new_node(value, 0, tail)?;
        self.nodes[high].parent = escape;
        self.nodes[low].parent = escape;
        self.nodes[escape].low = low;
        self.symbols[value as usize] = low;
        self.rebalance(low);
        Ok(low)
    }
}

/// LSB-first bit reader.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    count: u32,
}

impl BitReader<'_> {
    fn read(&mut self, n: u32) -> Result<u32, Error> {
        while self.count < n {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or_else(|| corrupt("truncated stream"))?;
            self.pos += 1;
            self.buf |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.count -= n;
        Ok(value)
    }
}

/// Decodes a Huffman stream (the bytes after the mask byte), producing at most `cap` bytes.
pub(crate) fn decompress(src: &[u8], cap: usize) -> Result<Vec<u8>, Error> {
    let mut bits = BitReader {
        data: src,
        pos: 0,
        buf: 0,
        count: 0,
    };
    let profile = bits.read(8)?;
    let mut tree = Tree::new(profile)?;
    let mut out = Vec::new();
    while out.len() < cap {
        let mut node = tree.nodes[HEAD].next;
        while tree.nodes[node].low != NIL {
            let low = tree.nodes[node].low;
            node = if bits.read(1)? == 1 {
                tree.nodes[low].prev
            } else {
                low
            };
        }
        let value = tree.nodes[node].value;
        if value == END_OF_STREAM {
            break;
        }
        if value == ESCAPE {
            let byte = bits.read(8)?;
            let leaf = tree.insert(byte)?;
            tree.rebalance(leaf);
            out.push(byte as u8);
        } else {
            out.push(value as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
impl Tree {
    /// Bits from the root to the first reachable leaf carrying `value`
    /// (0 for `low`, 1 for `low.prev`). `symbols[]` can point at split nodes, so search the tree.
    fn try_path(&self, value: u32) -> Option<Vec<bool>> {
        fn walk(t: &Tree, node: usize, value: u32, path: &mut Vec<bool>) -> bool {
            let low = t.nodes[node].low;
            if low == NIL {
                return t.nodes[node].value == value;
            }
            for (bit, child) in [(false, low), (true, t.nodes[low].prev)] {
                path.push(bit);
                if walk(t, child, value, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        walk(self, self.nodes[HEAD].next, value, &mut path).then_some(path)
    }

    /// Checks that every internal node's children point back at it.
    fn assert_consistent(&self) {
        for (i, n) in self.nodes.iter().enumerate().skip(1) {
            if n.low != NIL {
                let c1 = self.nodes[n.low].prev;
                assert_eq!(self.nodes[n.low].parent, i);
                assert_eq!(self.nodes[c1].parent, i);
            }
        }
    }
}

/// Test-only encoder for the same adaptive model, used to produce round-trip streams.
#[cfg(test)]
pub(crate) fn encode(data: &[u8]) -> Vec<u8> {
    struct BitWriter {
        out: Vec<u8>,
        acc: u8,
        count: u32,
    }
    impl BitWriter {
        fn put(&mut self, bits: &[bool]) {
            for &b in bits {
                self.acc |= u8::from(b) << self.count;
                self.count += 1;
                if self.count == 8 {
                    self.out.push(self.acc);
                    self.acc = 0;
                    self.count = 0;
                }
            }
        }
    }
    let mut w = BitWriter {
        out: Vec::new(),
        acc: 0,
        count: 0,
    };
    let profile: Vec<bool> = (0..8).map(|i| (PROFILE_7 >> i) & 1 == 1).collect();
    w.put(&profile);

    let mut tree = Tree::new(PROFILE_7).unwrap();
    for &byte in data {
        if let Some(path) = tree.try_path(u32::from(byte)) {
            // Like the decoder, only escapes update the model.
            w.put(&path);
        } else {
            w.put(&tree.try_path(ESCAPE).unwrap());
            let raw: Vec<bool> = (0..8).map(|i| (byte >> i) & 1 == 1).collect();
            w.put(&raw);
            let leaf = tree.insert(u32::from(byte)).unwrap();
            tree.rebalance(leaf);
        }
        tree.assert_consistent();
    }
    w.put(&tree.try_path(END_OF_STREAM).unwrap());
    if w.count > 0 {
        w.out.push(w.acc);
    }
    w.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg_bytes(seed: u32, n: usize) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (s >> 16) as u8
            })
            .collect()
    }

    /// Decodes with a cap larger than the data, so the end-of-stream marker is read too.
    #[track_caller]
    fn round_trip(data: &[u8]) {
        let packed = encode(data);
        let back = decompress(&packed, data.len() + 4096).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn empty_and_single_bytes() {
        round_trip(b"");
        round_trip(b"a");
        round_trip(&[0]);
        round_trip(&[255]);
    }

    #[test]
    fn text_round_trip() {
        round_trip(b"hello world, hello huffman, hello world again!");
        round_trip(&b"aaaaaaaaaaaaaaaaaaaabbbbbbbbbbcccccccc".repeat(20));
    }

    #[test]
    fn all_byte_values_round_trip() {
        let data: Vec<u8> = (0..=255u8).chain((0..=255u8).rev()).collect();
        round_trip(&data);
    }

    #[test]
    fn pseudo_random_round_trip() {
        round_trip(&lcg_bytes(1, 4000));
        // Restricted alphabet, so symbols repeat and the known-symbol path is exercised.
        let small: Vec<u8> = lcg_bytes(7, 4000).iter().map(|b| b & 0x1f).collect();
        round_trip(&small);
    }

    #[test]
    fn stops_at_capacity_without_end_marker() {
        let packed = encode(b"abcdef");
        assert_eq!(decompress(&packed, 3).unwrap(), b"abc");
    }

    #[test]
    fn rejects_other_profiles_and_truncation() {
        let mut packed = encode(b"xyz");
        // The first byte is the whole 8-bit profile; profile 6 is not supported.
        packed[0] = 6;
        assert!(decompress(&packed, 3).is_err());

        let packed = encode(&lcg_bytes(3, 200));
        assert!(decompress(&packed[..packed.len() / 2], 200).is_err());
    }
}
