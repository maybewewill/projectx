//! Pathing-cell occupant lists and the neighbour gather of the push-apart record
//! (`docs/spec/movement-7.md` section 8).
//!
//! The original keeps, per pathing cell, a chain of 8-byte pool entries `{next: 24 bits, kind: 8 bits; node}`
//! (`grid+0x28` cell words whose low 24 bits are the chain head, `0xFFFFFF` = empty; pool at `grid+0x78`). A node is a
//! mover's occupant record (stamp at `+0x38`, entry reference count at `+0x3C`, footprint rect at `+0x1C`).
//!
//! * Insert (`0x6F49FCD0`): a new entry goes to the **head** of the cell's chain; a free entry (`grid+0xAC`) is reused,
//!   otherwise the pool grows. Kind `0` entries (tombstones) also set the cell's bit in the dirty bitmap.
//! * Footprint update (`0x6F49FF90`): the old rect minus the overlap gets tombstones, the new rect minus the overlap
//!   gets live entries (kind `0x01`), pieces in the order of the rect difference `0x6F4BC060`, last piece first, each
//!   piece row by row (outer index = the first rect coordinate).
//! * Clean (`0x6F49F1E0` -> `0x6F49F030`): dirty cells ascending, chain head to tail; tombstones and duplicate or dead
//!   entries are unlinked onto the free list, live entries are kept and stamped.
//! * Gather (`0x6F496170` -> `0x6F495CA0` -> `0x6F495B00` -> `0x6F493B60`): the box is clamped to the grid bounds, the
//!   outer loop runs over the first coordinate (`index = pitch * outer + inner`), within a cell head to tail; a node is
//!   taken once per walk (stamp compare), after the mover filter.
//!
//! All of this is CONFIRMED by Unicorn runs of the real routines (`tools/emu/push_gather_emu.py`,
//! `tools/emu/push_occupancy_emu.py`). What is not known is when the original runs the clean and how it sizes a mover's
//! footprint (see the spec): those are the hosts' choices, flagged UNSURE where they are made.

use softfloat::{arith, conv, math, F32};

/// Chain end / empty cell (low 24 bits of a cell word).
pub const NIL: u32 = 0x00FF_FFFF;
/// Entry kinds (the entry's high byte).
pub const KIND_TOMBSTONE: u32 = 0;
pub const KIND_UNIT: u32 = 0x0100_0000;
pub const KIND_STATIC: u32 = 0x0200_0000;
/// A node whose stamp is `-1` is dead.
pub const DEAD_STAMP: u32 = 0xFFFF_FFFF;

/// One pool entry: word 0 (`next | kind`) and the node index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub w0: u32,
    pub node: u32,
}

/// The bookkeeping of one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeRec {
    /// `+0x38`.
    pub stamp: u32,
    /// `+0x3C`: how many pool entries refer to the node.
    pub refs: u32,
    /// `+0x1C`: the footprint rect `[row0, col0, row1, col1]` (end exclusive) as last registered.
    pub rect: [i32; 4],
}

/// The facts of a node's mover that the filter `0x6F493B60` reads.
#[derive(Clone, Copy, Debug)]
pub struct NodeFacts {
    /// Node `+0x34`: any bit in the top byte rejects.
    pub flags: u32,
    /// The mover is the query's own unit.
    pub is_self: bool,
    /// Mover `+0x10` is the unit tag.
    pub tag_ok: bool,
    /// Mover `+0x14` bit 31.
    pub bit31: bool,
    /// Mover `+0x90` (the footprint radius, cells): must be strictly positive.
    pub radius: F32,
    /// Mover `+0xC0` (the last walk's speed): must compare equal to zero (`+0` or `-0`).
    pub last_speed: F32,
    /// The mover's push record flags `R+0x20`, if it has one.
    pub rec: Option<u32>,
}

/// The query: the filter byte (record bits 20..27) and nibble (bits 28..31) of the asking record.
#[derive(Clone, Copy, Debug)]
pub struct Query {
    pub filter: u32,
    pub nibble: u32,
}

/// `0x6F493B60`: whether a node's mover is a neighbour for the query.
pub fn node_passes(f: &NodeFacts, q: &Query) -> bool {
    if f.flags & 0xFF00_0000 != 0 || f.is_self || !f.tag_ok || f.bit31 {
        return false;
    }
    if !arith::gt(f.radius, F32::ZERO) || !arith::eq(f.last_speed, F32::ZERO) {
        return false;
    }
    let Some(rec) = f.rec else {
        return false;
    };
    ((rec >> 20) & 0xFF) == q.filter && ((rec >> 28) & 0xF) >= q.nibble
}

/// The parts of rect `a` outside rect `b` (`b` overlaps `a`), in the original's order (`0x6F4BC060`).
pub fn rect_diff(a: [i32; 4], b: [i32; 4]) -> Vec<[i32; 4]> {
    let mut out = Vec::new();
    let mut start0 = a[0];
    if b[0] > a[0] {
        out.push([a[0], a[1], b[0], a[3]]);
        start0 = b[0];
    }
    let mut end0 = a[2];
    if b[2] < a[2] {
        out.push([b[2], a[1], a[2], a[3]]);
        end0 = b[2];
    }
    if b[1] > a[1] {
        out.push([start0, a[1], end0, b[1]]);
    }
    if b[3] < a[3] {
        out.push([start0, b[3], end0, a[3]]);
    }
    out
}

/// The cell box of a centre `(x, y)` (cells) and a radius, scaled by `scale` (`[grid+0x68]`) and floored:
/// `[y - r, x - r, y + r + 1, x + r + 1]` per component (`0x6F495D60` for the gather box with the table radius,
/// `0x6F4A6D70` for a mover's footprint with its own radius). CONFIRMED (`S` and `F` vectors).
pub fn cell_box(x: F32, y: F32, r: F32, scale: F32) -> [i32; 4] {
    let lo_y = arith::mul(arith::sub(y, r), scale);
    let lo_x = arith::mul(arith::sub(x, r), scale);
    let hi_y = arith::mul(arith::add(y, r), scale);
    let hi_x = arith::mul(arith::add(x, r), scale);
    let cell = |v: F32| conv::f2i(math::floor(v));
    [cell(lo_y), cell(lo_x), cell(hi_y).wrapping_add(1), cell(hi_x).wrapping_add(1)]
}

/// The occupant grid.
#[derive(Clone, Debug)]
pub struct Occupancy {
    pub pitch: i32,
    pub rows: i32,
    /// `grid+0x54..+0x60`: `[row0, col0, row1, col1]`, the clamp of every walk.
    pub bounds: [i32; 4],
    /// One word per cell: pathing bits in the top byte, chain head in the low 24 bits.
    pub cells: Vec<u32>,
    pub pool: Vec<Entry>,
    /// `grid+0xAC`: head of the free entries.
    pub free: u32,
    /// `grid+0xB0`: live entries.
    pub live: u32,
    /// `grid+0xB4`: the stamp counter.
    pub stamp: u32,
    /// One bit per cell: set by tombstones, cleared by the clean.
    pub dirty: Vec<u32>,
    pub nodes: Vec<NodeRec>,
    /// Nodes whose last reference went away while dead (the vtable `+0x10` destructor call).
    pub destroyed: Vec<u32>,
}

impl Occupancy {
    /// An empty grid of `rows * pitch` cells, bounds covering all of it.
    pub fn new(pitch: i32, rows: i32) -> Self {
        let n = (pitch * rows).max(0) as usize;
        Occupancy {
            pitch,
            rows,
            bounds: [0, 0, rows, pitch],
            cells: vec![NIL; n],
            pool: Vec::new(),
            free: NIL,
            live: 0,
            stamp: 0,
            dirty: vec![0; n.div_ceil(32)],
            nodes: Vec::new(),
            destroyed: Vec::new(),
        }
    }

    /// Registers a node (stamp 0, no references, empty rect) and returns its index.
    pub fn add_node(&mut self) -> u32 {
        self.nodes.push(NodeRec { stamp: 0, refs: 0, rect: [0; 4] });
        (self.nodes.len() - 1) as u32
    }

    /// Marks a node dead (`stamp = -1`): the clean drops its entries and destroys it when the last one is gone.
    pub fn kill_node(&mut self, node: u32) {
        self.nodes[node as usize].stamp = DEAD_STAMP;
    }

    /// `0x6F49FCD0`: inserts one entry at the head of the cell `(inner, outer)`.
    fn insert_cell(&mut self, inner: i32, outer: i32, node: u32, kind: u32) {
        let idx = (self.pitch * outer + inner) as usize;
        if kind == KIND_TOMBSTONE {
            self.dirty[idx >> 5] |= 1 << (idx & 31);
        }
        let cell = self.cells[idx];
        let w0 = (cell & 0x00FF_FFFF) | kind;
        let e = if self.free == NIL {
            self.pool.push(Entry { w0, node });
            (self.pool.len() - 1) as u32
        } else {
            let e = self.free;
            self.free = self.pool[e as usize].w0 & 0x00FF_FFFF;
            self.pool[e as usize] = Entry { w0, node };
            e
        };
        self.live += 1;
        self.nodes[node as usize].refs += 1;
        self.cells[idx] = (cell & 0xFF00_0000) | e;
    }

    /// `0x6F49FDA0`: inserts into every cell of `rect` inside the bounds, outer index ascending then inner ascending.
    fn insert_rect(&mut self, rect: [i32; 4], node: u32, kind: u32) {
        let b = self.bounds;
        let (hi3, hi2) = (rect[3].min(b[3]), rect[2].min(b[2]));
        let (lo1, lo0) = (rect[1].max(b[1]), rect[0].max(b[0]));
        if lo0 < hi2 && lo1 < hi3 {
            for o in lo0..hi2 {
                for i in lo1..hi3 {
                    self.insert_cell(i, o, node, kind);
                }
            }
        }
    }

    /// `0x6F49FF90`: moves a node's footprint to `new`.
    pub fn move_footprint(&mut self, node: u32, new: [i32; 4]) {
        let old = self.nodes[node as usize].rect;
        if old == new {
            return;
        }
        let x = [old[0].max(new[0]), old[1].max(new[1]), old[2].min(new[2]), old[3].min(new[3])];
        if x[0] < x[2] && x[1] < x[3] {
            for r in rect_diff(old, x).into_iter().rev() {
                self.insert_rect(r, node, KIND_TOMBSTONE);
            }
            for r in rect_diff(new, x).into_iter().rev() {
                self.insert_rect(r, node, KIND_UNIT);
            }
        } else {
            self.insert_rect(old, node, KIND_TOMBSTONE);
            self.insert_rect(new, node, KIND_UNIT);
        }
        self.nodes[node as usize].rect = new;
    }

    /// `0x6F49E850`: `true` for a live node; a dead node with no references left is destroyed.
    fn check_node(&mut self, node: u32) -> bool {
        let n = self.nodes[node as usize];
        if n.stamp != DEAD_STAMP {
            return true;
        }
        if n.refs & 0x00FF_FFFF == 0 {
            self.destroyed.push(node);
        }
        false
    }

    /// `0x6F49F030`: cleans one cell.
    fn clean_cell(&mut self, idx: usize) {
        let mut cur = self.cells[idx] & 0x00FF_FFFF;
        if cur == NIL {
            return;
        }
        self.stamp = self.stamp.wrapping_add(1);
        let st = self.stamp;
        let mut prev = NIL;
        loop {
            let Entry { w0, node } = self.pool[cur as usize];
            let next = w0 & 0x00FF_FFFF;
            let kind = w0 & 0xFF00_0000;
            let nd = if kind == KIND_STATIC { None } else { Some(node) };
            let keep = kind == KIND_UNIT && {
                let s = self.nodes[node as usize].stamp;
                s != DEAD_STAMP && s != st
            };
            if keep {
                prev = cur;
            } else {
                if prev == NIL {
                    self.cells[idx] = (self.cells[idx] & 0xFF00_0000) | next;
                } else {
                    let p = &mut self.pool[prev as usize];
                    p.w0 = (p.w0 & 0xFF00_0000) | next;
                }
                self.pool[cur as usize].w0 = (w0 & 0xFF00_0000) | self.free;
                self.free = cur;
                self.live = self.live.wrapping_sub(1);
                if let Some(n) = nd {
                    let r = &mut self.nodes[n as usize].refs;
                    *r = r.wrapping_sub(1);
                }
            }
            if let Some(n) = nd {
                if self.check_node(n) {
                    self.nodes[n as usize].stamp = st;
                }
            }
            if next == NIL {
                break;
            }
            cur = next;
        }
    }

    /// `0x6F49F1E0`: cleans every dirty cell, ascending.
    pub fn clean_dirty(&mut self) {
        if self.stamp > 0x7FFF_FFFF {
            self.stamp = 0;
        }
        for w in 0..self.dirty.len() {
            let mut bits = self.dirty[w];
            let mut k = 0usize;
            while bits != 0 {
                if bits & 1 != 0 {
                    self.clean_cell(w * 32 + k);
                }
                bits >>= 1;
                k += 1;
            }
            self.dirty[w] = 0;
        }
    }

    /// `0x6F496170` -> `0x6F495CA0`: the nodes of the cells of `rect` for which `accept(node)` holds, in walk order.
    pub fn gather(&mut self, r: [i32; 4], mut accept: impl FnMut(u32) -> bool) -> Vec<u32> {
        let mut out = Vec::new();
        let b = self.bounds;
        if !(r[0] <= r[2] && r[1] <= r[3]) {
            return out;
        }
        let (hi3, hi2) = (r[3].min(b[3]), r[2].min(b[2]));
        let (lo1, lo0) = (r[1].max(b[1]), r[0].max(b[0]));
        if !(lo0 < hi2 && lo1 < hi3) {
            return out;
        }
        self.stamp = self.stamp.wrapping_add(1);
        let walk = self.stamp;
        for o in lo0..hi2 {
            for i in lo1..hi3 {
                let head = self.cells[(self.pitch * o + i) as usize] & 0x00FF_FFFF;
                if head == NIL {
                    continue;
                }
                self.stamp = self.stamp.wrapping_add(1);
                let cell_stamp = self.stamp;
                let mut cur = head;
                loop {
                    let Entry { w0, node } = self.pool[cur as usize];
                    let next = w0 & 0x00FF_FFFF;
                    let kind = w0 & 0xFF00_0000;
                    if kind != KIND_STATIC {
                        let s = self.nodes[node as usize].stamp;
                        if !(s == walk || s == cell_stamp || s == DEAD_STAMP) {
                            if kind == KIND_UNIT {
                                if accept(node) {
                                    out.push(node);
                                }
                                self.nodes[node as usize].stamp = walk;
                            } else {
                                self.nodes[node as usize].stamp = cell_stamp;
                            }
                        }
                    }
                    if next == NIL {
                        break;
                    }
                    cur = next;
                }
            }
        }
        out
    }
}
