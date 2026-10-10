//! Region search: the request setup, the A* loop and the path fallbacks of the 1.26a pathing
//! (Game.dll: request setup `0x6F49A640`, start and loop `0x6F49A240` / `0x6F49A170`, dispatcher
//! `0x6F49A800`, reset `0x6F49A3F0`, relax `0x6F498C70` / `0x6F498D90`, cell expander
//! `0x6F499E90`, block expander `0x6F499FA0` and its scanners).
//!
//! Spec: `docs/spec/pathing-3.md` (regions are the pyramid records, see [`super::pyramid`]).
//!
//! Verified against the original by `tools/emu/search_vectors.py` (Unicorn): random pyramids
//! with 1 to 3 requests each (400 embedded cases; 1,500 more fresh cases were run during
//! development). Every request goes through the original setup and loop and the whole state is
//! compared after it: return codes, every node entry, the heap entries, closed list, pops,
//! start/goal/best ids, best distance, pyramid records and list counts. 0 mismatches. The
//! coverage includes block-expander nodes (level 1 and up), edge splits, corner relaxations,
//! flag links, budget exhaustion and goal pops.
//!
//! What is checked and what is not:
//! * Mode 1 only (size class 0). The two-cell footprint mode of the original (`S+0x90 == 2`) is
//!   NOT ported: its extra probe tests are not modelled.
//! * The cell expander (level-0 nodes) is confirmed: probe order N, E, S, W, then the diagonals
//!   NE, SE, SW, NW, each gated by its two orthogonal neighbours, then the flag link.
//! * The block expander (level >= 1 nodes) is a reading of the edge and corner scanners
//!   (`0x6F4998F0`..`0x6F499BF0`, `0x6F499CF0`..`0x6F499E40`). It matched the original on the
//!   generated cases, but those cases do not exhaust the scanners' branches, so it is UNVERIFIED
//!   beyond them.
//! * The request setup converts its float inputs by floor and truncation. Only integral inputs
//!   (cells) were run through the original.
//! * The path-building step (`0x6F49A470`, waypoint output) is not ported: [`Path::cells`] is
//!   the chain of seed cells through the parent links, which is what the walk produces.
//! * An out-of-bounds start (setup returns 0) is reported as [`Status::Failed`] without a
//!   search. The original's dispatcher still calls the loop in that case; this is not ported
//!   and not verified.

use super::heap::Heap;
use super::isqrt::isqrt;
use super::pyramid::{Labels, Node, Pyramid, BLOCKED, LIST_END, NO_NODE, OPEN, REFINE, UNTOUCHED};
use softfloat::{conv, math, F32};

/// Default pop budget of a request (the original takes it from the caller; pathing-2.md 5.2).
pub const POP_BUDGET: u32 = 400;

/// One entry of the flag-link table (12 bytes in the original, at `S+0x3C`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlagLink {
    /// Bit 0 of the entry. When set, a node with this flag byte is linked to `(x, y)`.
    pub enabled: bool,
    /// Target cell x.
    pub x: i32,
    /// Target cell y.
    pub y: i32,
}

/// One path request (the arguments of the request setup).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    /// Path type `T` (even, 0 to 14 in practice).
    pub path_type: u32,
    /// Whether labelled nodes take the record's flag byte.
    pub flag_on: bool,
    /// Start point. The setup floors and truncates it to a cell (`0x6F6EF170`, `0x6F6EEC20`).
    pub start: (F32, F32),
    /// Goal point, converted the same way.
    pub goal: (F32, F32),
    /// Pop budget of the loop.
    pub budget: u32,
}

/// How a request ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Start and goal are the same cell, or one of them is blocked or the goal shares the start's
    /// region: the caller moves in a straight line (the original's direct move, return 1).
    Direct,
    /// The goal region was popped. `cells` runs from the goal back to the start.
    Reached,
    /// Not reached and the start is the closest node. The caller moves in a straight line.
    Straight,
    /// Not reached. `cells` runs from the closest node reached back to the start.
    Partial,
    /// The start cell is outside the pyramid. Nothing was searched.
    Failed,
}

/// Result of [`Search::find_path`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    /// How the request ended.
    pub status: Status,
    /// Seed cells of the nodes on the path, goal side first. Empty unless `Reached` or `Partial`.
    pub cells: Vec<(i32, i32)>,
}

/// Return code of the request setup (`0x6F49A640`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setup {
    Outside,
    Direct,
    Search,
}

/// Squared distance of `(24 dx, 24 dy)` in 32-bit wrapping arithmetic: the argument of the
/// step cost and the heuristic (`isqrt(576 * d^2)`, read from `0x6F498C70`).
fn scaled_d2(a: (i32, i32), b: (i32, i32)) -> u32 {
    let dx = a.0.wrapping_sub(b.0).unsigned_abs().wrapping_mul(24);
    let dy = a.1.wrapping_sub(b.1).unsigned_abs().wrapping_mul(24);
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}

/// Plain squared cell distance in 32-bit wrapping arithmetic (closest-node tracking).
fn plain_d2(a: (i32, i32), b: (i32, i32)) -> u32 {
    let dx = a.0.wrapping_sub(b.0) as u32;
    let dy = a.1.wrapping_sub(b.1) as u32;
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}

/// The cell of a point: floor, then truncation to an integer (the setup's conversion).
fn cell_of(v: F32) -> i32 {
    conv::f2i(math::floor(v))
}

/// Axis of an edge scan: `X` samples along x at a fixed row, `Y` along y at a fixed column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

/// Region search state: the pyramid, the labelling state, the open and closed lists and the
/// request fields of the original's search object.
#[derive(Clone, Debug)]
pub struct Search {
    pyramid: Pyramid,
    labels: Labels,
    links: Vec<FlagLink>,
    heap: Heap,
    closed_head: u32,
    pops: u32,
    budget: u32,
    start: (i32, i32),
    goal: (i32, i32),
    start_id: i32,
    goal_id: i32,
    best_d2: u32,
    best_id: i32,
    setup_code: i32,
    search_code: i32,
}

impl Search {
    /// A search over `pyramid`. `links` is the flag-link table (256 entries, indexed by the flag
    /// byte). `list_grow` and `list_capacity` are the node list's growth step and initial
    /// capacity (the list is erased, not freed, between requests).
    ///
    /// The request fields start at zero, as the original's zeroed search object has them.
    ///
    /// Panics if `links.len() != 256`.
    pub fn new(
        pyramid: Pyramid,
        links: Vec<FlagLink>,
        list_grow: usize,
        list_capacity: usize,
    ) -> Self {
        assert_eq!(links.len(), 256, "one link entry per flag byte");
        Search {
            pyramid,
            labels: Labels::new(0, false, list_grow, list_capacity),
            links,
            heap: Heap::new(),
            closed_head: LIST_END,
            pops: 0,
            budget: POP_BUDGET,
            start: (0, 0),
            goal: (0, 0),
            start_id: 0,
            goal_id: 0,
            best_d2: 0,
            best_id: 0,
            setup_code: 0,
            search_code: -1,
        }
    }

    /// The node list of the last request (region ids are its indices).
    pub fn nodes(&self) -> &[Node] {
        &self.labels.nodes
    }

    /// Return codes of the last request: the setup (0, 1 or 2) and the loop (goal id or -1).
    pub fn last_codes(&self) -> (i32, i32) {
        (self.setup_code, self.search_code)
    }

    /// Runs one request and returns how it ended. Each request resets the node list, the open
    /// list and the closed list, and bumps the generation when it labels.
    pub fn find_path(&mut self, req: &Request) -> Path {
        let setup = self.setup(req);
        self.setup_code = match setup {
            Setup::Outside => 0,
            Setup::Direct => 1,
            Setup::Search => 2,
        };
        self.search_code = -1;
        match setup {
            Setup::Outside => Path {
                status: Status::Failed,
                cells: Vec::new(),
            },
            Setup::Direct => Path {
                status: Status::Direct,
                cells: Vec::new(),
            },
            Setup::Search => {
                let found = self.start_search();
                self.search_code = found;
                if found >= 0 {
                    return Path {
                        status: Status::Reached,
                        cells: self.chain(found as u32),
                    };
                }
                if self.start_id == self.best_id {
                    return Path {
                        status: Status::Straight,
                        cells: Vec::new(),
                    };
                }
                Path {
                    status: Status::Partial,
                    cells: self.chain(self.best_id as u32),
                }
            }
        }
    }

    /// Reset (`0x6F49A3F0`) and request setup (`0x6F49A640`).
    fn setup(&mut self, req: &Request) -> Setup {
        self.pops = 0;
        self.closed_head = LIST_END;
        self.labels.nodes.clear();
        self.heap.clear();
        self.labels.path_type = req.path_type;
        self.labels.flag_on = req.flag_on;
        let start = (cell_of(req.start.0), cell_of(req.start.1));
        let goal = (cell_of(req.goal.0), cell_of(req.goal.1));
        self.start = start;
        self.goal = goal;
        if self.pyramid.record_index(0, start.0, start.1).is_none() {
            return Setup::Outside;
        }
        if start == goal {
            return Setup::Direct;
        }
        self.labels.gen = self.labels.gen.wrapping_add(1);
        let sid = self.pyramid.lookup(&mut self.labels, 0, start.0, start.1);
        self.start_id = sid;
        if sid == BLOCKED {
            self.goal_id = BLOCKED;
            return Setup::Direct;
        }
        let gid = self.pyramid.lookup(&mut self.labels, 0, goal.0, goal.1);
        self.goal_id = gid;
        if sid == gid {
            return Setup::Direct;
        }
        self.budget = req.budget;
        self.best_id = sid;
        self.best_d2 = plain_d2(start, goal);
        Setup::Search
    }

    /// Start node and loop (`0x6F49A240`).
    fn start_search(&mut self) -> i32 {
        let h = isqrt(scaled_d2(self.start, self.goal));
        let id = self.start_id as u32;
        self.labels.nodes[id as usize].h = h;
        self.push(id);
        self.run()
    }

    /// Adds `node` to the open list (`0x6F498B80`): visit +1, link open, key g + h.
    fn push(&mut self, node: u32) {
        let n = &mut self.labels.nodes[node as usize];
        n.visit = n.visit.wrapping_add(1);
        n.link = OPEN;
        n.prev = UNTOUCHED;
        let key = n.g.wrapping_add(n.h);
        let stamp = n.visit;
        self.heap.push(key, node, stamp);
    }

    /// Takes `node` out of the closed list (`0x6F498C20`).
    fn unlink(&mut self, node: u32) {
        let n = &mut self.labels.nodes[node as usize];
        n.visit = n.visit.wrapping_add(1);
        let (next, prev) = (n.link, n.prev);
        n.link = UNTOUCHED;
        n.prev = UNTOUCHED;
        if prev == UNTOUCHED {
            self.closed_head = next;
        } else {
            self.labels.nodes[prev as usize].link = next;
        }
        if next != LIST_END {
            self.labels.nodes[next as usize].prev = prev;
        }
    }

    /// Puts `node` at the head of the closed list (`0x6F498BD0`).
    fn close(&mut self, node: u32) {
        if self.closed_head != LIST_END {
            self.labels.nodes[self.closed_head as usize].prev = node;
        }
        let old_head = self.closed_head;
        let n = &mut self.labels.nodes[node as usize];
        n.visit = n.visit.wrapping_add(1);
        n.link = old_head;
        n.prev = UNTOUCHED;
        self.closed_head = node;
    }

    /// Relaxes `node` from `from` (`0x6F498C70`, and `0x6F498D90` when `flag_edge`).
    ///
    /// An untouched node is only tracked as the closest node to the goal. A touched node is
    /// re-labelled only when the new cost is strictly lower. A flag edge costs 1 instead of the
    /// distance step and records the parent's flag byte.
    fn relax(&mut self, node: u32, from: u32, flag_edge: bool) {
        let parent = self.labels.nodes[from as usize];
        let seed = self.labels.nodes[node as usize].seed;
        let step = if flag_edge {
            1
        } else {
            isqrt(scaled_d2(parent.seed, seed))
        };
        let g_new = parent.g.wrapping_add(step);
        let h_new = isqrt(scaled_d2(seed, self.goal));
        let n = self.labels.nodes[node as usize];
        if n.link == UNTOUCHED {
            let d = plain_d2(seed, self.goal);
            if d < self.best_d2 {
                self.best_d2 = d;
                self.best_id = node as i32;
            }
        } else {
            if g_new >= n.g {
                return;
            }
            if n.link < OPEN {
                self.unlink(node);
            } else {
                let m = &mut self.labels.nodes[node as usize];
                m.visit = m.visit.wrapping_add(1);
                m.link = UNTOUCHED;
                m.prev = UNTOUCHED;
            }
        }
        let m = &mut self.labels.nodes[node as usize];
        m.parent = from;
        m.via_flag = if flag_edge { parent.flag } else { 0 };
        m.g = g_new;
        m.h = h_new;
        self.push(node);
    }

    /// Lookup at level 0 of cell `(x, y)`, or `None` when it is blocked.
    fn walkable_region(&mut self, x: i32, y: i32) -> Option<u32> {
        let id = self.pyramid.lookup(&mut self.labels, 0, x, y);
        (id != BLOCKED).then_some(id as u32)
    }

    /// Probe of an orthogonal neighbour: relaxes its region when it is walkable.
    fn probe(&mut self, from: u32, x: i32, y: i32) -> bool {
        match self.walkable_region(x, y) {
            Some(id) => {
                self.relax(id, from, false);
                true
            }
            None => false,
        }
    }

    /// Probe of a diagonal neighbour (the gating is the caller's).
    fn diagonal(&mut self, from: u32, x: i32, y: i32) {
        if let Some(id) = self.walkable_region(x, y) {
            self.relax(id, from, false);
        }
    }

    /// Flag link of a node whose flag byte has an enabled table entry (`0x6F4998C0`).
    fn flag_link(&mut self, from: u32, flag: u8, x: i32, y: i32) {
        let link = self.links[flag as usize];
        if !link.enabled || (x == link.x && y == link.y) {
            return;
        }
        if let Some(id) = self.walkable_region(link.x, link.y) {
            self.relax(id, from, true);
        }
    }

    /// Cell expander (`0x6F499E90`): N, E, S, W, then NE, SE, SW, NW (each only when its two
    /// orthogonal neighbours were walkable), then the flag link.
    fn expand_cell(&mut self, node: u32) {
        let n = self.labels.nodes[node as usize];
        let (x, y) = n.seed;
        let north = self.probe(node, x, y - 1);
        let east = self.probe(node, x + 1, y);
        let south = self.probe(node, x, y + 1);
        let west = self.probe(node, x - 1, y);
        if north && east {
            self.diagonal(node, x + 1, y - 1);
        }
        if east && south {
            self.diagonal(node, x + 1, y + 1);
        }
        if south && west {
            self.diagonal(node, x - 1, y + 1);
        }
        if west && north {
            self.diagonal(node, x - 1, y - 1);
        }
        if n.flag != 0 {
            self.flag_link(node, n.flag, x, y);
        }
    }

    /// Lookup at `level` (the block scanners look up at their own level).
    fn lookup_at(&mut self, level: usize, x: i32, y: i32) -> i32 {
        self.pyramid.lookup(&mut self.labels, level, x, y)
    }

    /// Scans one edge of a level-`level` block (`0x6F4998F0` for [`Axis::X`], `0x6F4999F0` for
    /// [`Axis::Y`]). `split` is the coordinate along the edge that is halved when the sample is
    /// a [`REFINE`]; `fixed` is the coordinate across the edge.
    ///
    /// Returns the two flags the original writes to its out pointers: the first flag (the half
    /// with the smaller coordinate) and the second. A flag is set when the region found is at
    /// a level above 0 (the recursion's own first half writes the first flag).
    fn edge_scan(
        &mut self,
        node: u32,
        level: usize,
        axis: Axis,
        split: i32,
        fixed: i32,
    ) -> (bool, bool) {
        let at = |s: i32| match axis {
            Axis::X => (s, fixed),
            Axis::Y => (fixed, s),
        };
        let (mut out_first, mut out_second) = (false, false);
        let mut first_half_open = true;
        let mut lvl = level;
        let mut split = split;
        let (x, y) = at(split);
        let mut id = self.lookup_at(lvl, x, y);
        while id == BLOCKED || id == REFINE {
            if id != REFINE {
                return (out_first, out_second);
            }
            let block_level = lvl;
            lvl -= 1;
            let half = 1i32 << lvl;
            let mid = ((split >> block_level) << block_level) + half;
            let (first, second) = if mid <= split {
                (mid - 1, split)
            } else {
                (split, mid)
            };
            let (sub_first, _) = self.edge_scan(node, lvl, axis, first, fixed);
            if first_half_open && sub_first {
                out_first = true;
            }
            first_half_open = false;
            split = second;
            let (x, y) = at(split);
            id = self.lookup_at(lvl, x, y);
        }
        self.relax(id as u32, node, false);
        if lvl != 0 {
            out_second = true;
            if first_half_open {
                out_first = true;
            }
        }
        (out_first, out_second)
    }

    /// Corner probe (`0x6F499CF0` and its three siblings): the region at `(x, y)` seen at
    /// `level`, descending while it is a [`REFINE`].
    fn corner(&mut self, node: u32, level: usize, x: i32, y: i32) {
        let mut lvl = level;
        loop {
            let id = self.lookup_at(lvl, x, y);
            if id >= 0 {
                self.relax(id as u32, node, false);
                return;
            }
            if id != REFINE {
                return;
            }
            lvl -= 1;
        }
    }

    /// Block expander (`0x6F499FA0`, reading of the scanners): the four edges of the block (top,
    /// right, bottom, left) and then the corners that sit between two edges that both found
    /// a region (NE, SE, SW, NW).
    fn expand_block(&mut self, node: u32) {
        let n = self.labels.nodes[node as usize];
        let level = n.level as usize;
        let (x, y) = n.seed;
        let size = 1i32 << level;
        let ox = (x >> level) << level;
        let oy = (y >> level) << level;
        let top = self.edge_scan(node, level, Axis::X, x, oy - 1);
        let right = self.edge_scan(node, level, Axis::Y, y, ox + size);
        let bottom = self.edge_scan(node, level, Axis::X, x, oy + size);
        let left = self.edge_scan(node, level, Axis::Y, y, ox - 1);
        if top.1 && right.0 {
            self.corner(node, level, ox + size, oy - 1);
        }
        if right.1 && bottom.1 {
            self.corner(node, level, ox + size, oy + size);
        }
        if bottom.0 && left.1 {
            self.corner(node, level, ox - 1, oy + size);
        }
        if left.0 && top.0 {
            self.corner(node, level, ox - 1, oy - 1);
        }
    }

    /// Pop loop (`0x6F49A170`). Returns the goal id when the goal is popped, else -1.
    fn run(&mut self) -> i32 {
        loop {
            if self.heap.is_empty() {
                return -1;
            }
            let pops = self.pops;
            self.pops = pops.wrapping_add(1);
            if pops >= self.budget {
                return -1;
            }
            let Some(entry) = self.heap.pop() else {
                return -1;
            };
            let node = entry.node;
            let n = &mut self.labels.nodes[node as usize];
            if n.visit != entry.stamp {
                continue;
            }
            n.link = UNTOUCHED;
            n.prev = UNTOUCHED;
            n.visit = entry.stamp.wrapping_add(1);
            if node as i32 == self.goal_id {
                return node as i32;
            }
            if n.level == 0 {
                self.expand_cell(node);
            } else {
                self.expand_block(node);
            }
            self.close(node);
        }
    }

    /// Seed cells from `node` back to the start through the parent links.
    fn chain(&self, node: u32) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        let mut cur = node;
        for _ in 0..=self.labels.nodes.len() {
            let n = &self.labels.nodes[cur as usize];
            out.push(n.seed);
            if n.parent == NO_NODE {
                break;
            }
            cur = n.parent;
        }
        out
    }

    /// The observable state as words, in the order the differential test digests it
    /// (`tools/emu/search_vectors.py`, `state_words`): the two return codes, the generation,
    /// the list count, pops, closed head, start, goal and best ids, best distance, the pyramid
    /// records (stamp, word), every node (seed, visit, link, prev, g, h, parent, flag, level,
    /// via-flag), then the heap size (with the sentinel) and every live heap entry.
    pub fn observable_words(&self) -> Vec<u32> {
        let mut w = vec![
            self.setup_code as u32,
            self.search_code as u32,
            self.labels.gen,
            self.labels.nodes.len() as u32,
            self.pops,
            self.closed_head,
            self.start_id as u32,
            self.goal_id as u32,
            self.best_id as u32,
            self.best_d2,
        ];
        for k in 0..4 {
            for r in self.pyramid.level(k).records() {
                w.push(r.stamp);
                w.push(r.word1);
            }
        }
        for n in &self.labels.nodes {
            w.extend([
                n.seed.0 as u32,
                n.seed.1 as u32,
                n.visit,
                n.link,
                n.prev,
                n.g,
                n.h,
                n.parent,
                u32::from(n.flag),
                u32::from(n.level),
                u32::from(n.via_flag),
            ]);
        }
        let slots = self.heap.slots();
        w.push(slots.len() as u32);
        for e in &slots[1..] {
            w.extend([e.key, e.node, e.stamp]);
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathing::pyramid::Level;

    /// A level-0 record word with state `s` for path type 0.
    fn cell_word(s: u32) -> u32 {
        s << 30
    }

    /// An 8 by 8 pyramid: `cells[i]` gives the state of cell i (row-major); coarser levels are
    /// state 11, so every cell is its own region.
    fn pyramid(cells: &[u32]) -> Pyramid {
        let base: Vec<u32> = cells.iter().map(|&s| cell_word(s)).collect();
        let coarse = |n: usize| vec![cell_word(3); n];
        Pyramid::new([
            Level::new(8, 8, &base),
            Level::new(4, 4, &coarse(16)),
            Level::new(2, 2, &coarse(4)),
            Level::new(1, 1, &coarse(1)),
        ])
    }

    fn search(cells: &[u32]) -> Search {
        Search::new(pyramid(cells), vec![FlagLink::default(); 256], 8, 0)
    }

    fn at(x: i32, y: i32) -> (F32, F32) {
        (
            F32::from_bits((x as f32).to_bits()),
            F32::from_bits((y as f32).to_bits()),
        )
    }

    fn request(start: (i32, i32), goal: (i32, i32), budget: u32) -> Request {
        Request {
            path_type: 0,
            flag_on: false,
            start: at(start.0, start.1),
            goal: at(goal.0, goal.1),
            budget,
        }
    }

    #[test]
    fn same_cell_and_blocked_start_move_directly() {
        let mut s = search(&[0; 64]);
        assert_eq!(
            s.find_path(&request((2, 2), (2, 2), POP_BUDGET)).status,
            Status::Direct
        );
        let mut cells = [0u32; 64];
        cells[0] = 1;
        let mut s = search(&cells);
        assert_eq!(
            s.find_path(&request((0, 0), (5, 5), POP_BUDGET)).status,
            Status::Direct
        );
        assert_eq!(s.last_codes(), (1, -1));
    }

    #[test]
    fn start_outside_the_pyramid_fails() {
        let mut s = search(&[0; 64]);
        assert_eq!(
            s.find_path(&request((-1, 0), (5, 5), POP_BUDGET)).status,
            Status::Failed
        );
        assert_eq!(s.last_codes().0, 0);
    }

    #[test]
    fn open_grid_reaches_the_goal_with_adjacent_steps() {
        let mut s = search(&[0; 64]);
        let p = s.find_path(&request((0, 0), (7, 7), POP_BUDGET));
        assert_eq!(p.status, Status::Reached);
        assert_eq!(p.cells.first(), Some(&(7, 7)));
        assert_eq!(p.cells.last(), Some(&(0, 0)));
        for pair in p.cells.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                (a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1,
                "{a:?} -> {b:?}"
            );
        }
    }

    #[test]
    fn walled_off_goal_returns_the_closest_reached_cell() {
        // Column 4 is blocked (state 01) in every row: the goal (7, 0) is unreachable.
        let mut cells = [0u32; 64];
        for y in 0..8 {
            cells[y * 8 + 4] = 1;
        }
        let mut s = search(&cells);
        let p = s.find_path(&request((0, 0), (7, 0), POP_BUDGET));
        assert_eq!(p.status, Status::Partial);
        assert_eq!(p.cells.first(), Some(&(3, 0)));
        assert_eq!(p.cells.last(), Some(&(0, 0)));
    }

    #[test]
    fn budget_of_one_pop_stops_the_loop() {
        let mut s = search(&[0; 64]);
        let p = s.find_path(&request((0, 0), (7, 7), 1));
        assert_ne!(p.status, Status::Reached);
        assert_eq!(s.last_codes().1, -1);
    }

    #[test]
    fn requests_reset_the_node_list() {
        let mut s = search(&[0; 64]);
        s.find_path(&request((0, 0), (7, 7), POP_BUDGET));
        let first = s.nodes().len();
        s.find_path(&request((0, 0), (7, 7), POP_BUDGET));
        assert_eq!(s.nodes().len(), first);
        assert_eq!(s.nodes()[0].seed, (0, 0));
    }
}
