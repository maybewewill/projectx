//! Region-level A* search (pathing-2.md sections 4 and 5).
//!
//! STATUS: UNVERIFIED against the original. The loop, the heap, the cost, the expansion
//! order and the corner rule follow the decompile reading in `docs/spec/pathing-2.md`, but
//! the region labelling that produces the regions is NOT located in the original (the flood
//! fill is open item 1 of that spec). The regions here come from the caller
//! ([`RegionMap::from_labels`]) or from a plain 4-connected flood fill
//! ([`RegionMap::label_connected`]), which is a stand-in and not the original's shapes.
//! Only the search itself is ported; the level pyramid, the large-unit footprint checks and
//! the second neighbour rule (flag byte at node +0x22) are not ported.
//!
//! Model (per the spec):
//! * A node is a region. Its id is its node index, and its seed is one cell of the region
//!   (the first cell in row-major order for the stand-in labelling).
//! * Step cost from parent P to neighbour N is `isqrt(576 * D)`, with `D` the squared seed
//!   distance; written as `(24 dx)^2 + (24 dy)^2` in 32-bit wrapping arithmetic. The
//!   heuristic is the same expression for the seed of N and the goal cell.
//! * Key `f = g + h` (unsigned, wrapping). Relax only on a strictly smaller `g`.
//! * Expansion probes N, E, S, W, then NE, SE, SW, NW. A diagonal is probed only when both
//!   orthogonal cells it cuts past are passable (N and E, E and S, S and W, W and N).
//! * At most `budget` pops per call (400 by default). Stale entries count as pops.
//! * If the goal is not reached, the path goes to the touched node with the smallest
//!   squared cell distance to the goal (first found wins ties), or straight to the goal
//!   when no touched node beats the start.
//!
//! Open consequence: edges come only from the eight neighbours of each region's seed. When a
//! region is bigger than one cell, most of its boundary is never probed, so the result
//! depends strongly on the labelling. The original's flood fill decides that, and it is not
//! located yet. [`RegionMap::label_connected`] therefore gives mostly straight results.
//!
//! Deviations kept on purpose, both UNVERIFIED:
//! * A probe that finds the node's own region is not relaxed. The decompile would make a
//!   node its own parent in that case, and the parent walk would never end.
//! * The start node's seed is its region seed, not the start cell.

use super::grid::Grid;
use super::heap::Heap;
use super::isqrt::isqrt;
use softfloat::F32;

/// Pops allowed per search call (the path's default expansion budget, pathing-2.md 5.2).
pub const POP_BUDGET: u32 = 400;

/// Region id of a blocked cell.
pub const NO_REGION: u32 = u32::MAX;

/// A partition of the grid into regions, with one seed cell per region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionMap {
    width: u32,
    height: u32,
    labels: Vec<u32>,
    seeds: Vec<(i32, i32)>,
}

impl RegionMap {
    /// Builds a map from one label per cell (row-major). [`NO_REGION`] marks blocked cells.
    /// The seed of each region is its first cell in row-major order.
    ///
    /// Panics if `labels.len() != width * height`.
    pub fn from_labels(width: u32, height: u32, labels: Vec<u32>) -> Self {
        assert_eq!(
            labels.len(),
            width as usize * height as usize,
            "one label per cell"
        );
        let count = labels
            .iter()
            .filter(|&&l| l != NO_REGION)
            .map(|&l| l as usize + 1)
            .max()
            .unwrap_or(0);
        let mut seeds: Vec<Option<(i32, i32)>> = vec![None; count];
        for (i, &l) in labels.iter().enumerate() {
            if l == NO_REGION {
                continue;
            }
            let slot = &mut seeds[l as usize];
            if slot.is_none() {
                let x = (i % width as usize) as i32;
                let y = (i / width as usize) as i32;
                *slot = Some((x, y));
            }
        }
        RegionMap {
            width,
            height,
            labels,
            seeds: seeds.into_iter().map(|s| s.unwrap_or((0, 0))).collect(),
        }
    }

    /// Stand-in labelling: 4-connected components of the cells that have no type bit in
    /// `mask` set. Ids are handed out in row-major order of each component's first cell.
    pub fn label_connected(grid: &Grid, mask: u8) -> Self {
        let (w, h) = (grid.width() as i32, grid.height() as i32);
        let mut labels = vec![NO_REGION; (w * h) as usize];
        let mut next = 0u32;
        let mut stack = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let start = (y * w + x) as usize;
                if labels[start] != NO_REGION || grid.is_blocked(x, y, mask) != Some(false) {
                    continue;
                }
                labels[start] = next;
                stack.push((x, y));
                while let Some((cx, cy)) = stack.pop() {
                    for (nx, ny) in [(cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)] {
                        if nx < 0 || ny < 0 || nx >= w || ny >= h {
                            continue;
                        }
                        let i = (ny * w + nx) as usize;
                        if labels[i] == NO_REGION && grid.is_blocked(nx, ny, mask) == Some(false) {
                            labels[i] = next;
                            stack.push((nx, ny));
                        }
                    }
                }
                next += 1;
            }
        }
        RegionMap::from_labels(w as u32, h as u32, labels)
    }

    /// Region id at a cell, or `None` when the cell is blocked or outside the map.
    pub fn region_at(&self, x: i32, y: i32) -> Option<u32> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return None;
        }
        let l = self.labels[y as usize * self.width as usize + x as usize];
        (l != NO_REGION).then_some(l)
    }

    /// Number of region ids (one more than the largest id in use).
    pub fn region_count(&self) -> usize {
        self.seeds.len()
    }

    /// Seed cell of a region.
    pub fn seed(&self, region: u32) -> (i32, i32) {
        self.seeds[region as usize]
    }
}

/// How a search ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Start and goal share a cell or a region: the caller draws the straight segment.
    SameRegion,
    /// The goal region was reached. `cells` lists seed cells from the goal back to the start.
    Reached,
    /// Not reached. `cells` leads from the closest touched node back to the start.
    Partial,
    /// Not reached and no touched node beats the start. The caller draws the straight segment.
    Straight,
    /// Start or goal is outside the map or blocked. The caller draws the straight segment.
    Failed,
}

/// Result of [`find_path`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    /// How the search ended.
    pub status: Status,
    /// Seed cells (`(x, y)`), goal side first. Empty for the straight-segment statuses.
    pub cells: Vec<(i32, i32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    /// Never touched, or popped and being expanded (the original's `-1`).
    Untouched,
    /// In the open list (the original's `-2`).
    Open,
    /// Closed (the original keeps a linked closed list; only its existence matters here).
    Closed,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    seed: (i32, i32),
    stamp: u32,
    link: Link,
    g: u32,
    h: u32,
    parent: Option<u32>,
}

/// `576 * D` in 32-bit wrapping arithmetic, as `(24 dx)^2 + (24 dy)^2`.
fn scaled_d2(a: (i32, i32), b: (i32, i32)) -> u32 {
    let dx = (a.0.wrapping_sub(b.0).wrapping_mul(24)) as u32;
    let dy = (a.1.wrapping_sub(b.1).wrapping_mul(24)) as u32;
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}

/// Plain squared cell distance (no scale), 32-bit wrapping.
fn d2(a: (i32, i32), b: (i32, i32)) -> u32 {
    let dx = a.0.wrapping_sub(b.0) as u32;
    let dy = a.1.wrapping_sub(b.1) as u32;
    dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy))
}

struct Search<'a> {
    regions: &'a RegionMap,
    nodes: Vec<Node>,
    heap: Heap,
    goal_cell: (i32, i32),
    goal_region: u32,
    best_d2: u32,
    best_node: u32,
    pops: u32,
}

impl Search<'_> {
    fn push_node(&mut self, node: u32) {
        let n = &mut self.nodes[node as usize];
        n.stamp = n.stamp.wrapping_add(1);
        n.link = Link::Open;
        let (key, stamp) = (n.g.wrapping_add(n.h), n.stamp);
        self.heap.push(key, node, stamp);
    }

    fn relax(&mut self, node: u32, parent: u32) {
        let p = self.nodes[parent as usize];
        let n = self.nodes[node as usize];
        let g_new = p.g.wrapping_add(isqrt(scaled_d2(p.seed, n.seed)));
        let h_new = isqrt(scaled_d2(n.seed, self.goal_cell));

        if n.link == Link::Untouched {
            // First touch: track the node closest to the goal (strict, so the first wins).
            let dist = d2(n.seed, self.goal_cell);
            if dist < self.best_d2 {
                self.best_d2 = dist;
                self.best_node = node;
            }
        } else if n.g <= g_new {
            return;
        } else {
            // Re-open an open or closed node: the stamp change invalidates its old entry.
            let nm = &mut self.nodes[node as usize];
            nm.stamp = nm.stamp.wrapping_add(1);
            nm.link = Link::Untouched;
        }

        let nm = &mut self.nodes[node as usize];
        nm.parent = Some(parent);
        nm.g = g_new;
        nm.h = h_new;
        self.push_node(node);
    }

    /// Probes one neighbour cell from `from`. Returns whether the cell is passable.
    fn probe(&mut self, cell: (i32, i32), from: u32) -> bool {
        match self.regions.region_at(cell.0, cell.1) {
            None => false,
            Some(region) => {
                if region != from {
                    self.relax(region, from);
                }
                true
            }
        }
    }

    fn expand(&mut self, node: u32) {
        let (x, y) = self.nodes[node as usize].seed;
        let n = self.probe((x, y - 1), node);
        let e = self.probe((x + 1, y), node);
        let s = self.probe((x, y + 1), node);
        let w = self.probe((x - 1, y), node);
        if n && e {
            self.probe((x + 1, y - 1), node);
        }
        if e && s {
            self.probe((x + 1, y + 1), node);
        }
        if s && w {
            self.probe((x - 1, y + 1), node);
        }
        if w && n {
            self.probe((x - 1, y - 1), node);
        }
    }

    /// Main loop (original `0x6F49A170`). Returns true when the goal region is popped.
    fn run(&mut self, budget: u32) -> bool {
        while !self.heap.is_empty() {
            let count = self.pops;
            self.pops = count.wrapping_add(1);
            if count >= budget {
                break;
            }
            let Some(entry) = self.heap.pop() else { break };
            let node = entry.node;
            let n = &mut self.nodes[node as usize];
            if n.stamp != entry.stamp {
                continue;
            }
            n.stamp = n.stamp.wrapping_add(1);
            n.link = Link::Untouched;
            if node == self.goal_region {
                return true;
            }
            self.expand(node);
            let n = &mut self.nodes[node as usize];
            n.stamp = n.stamp.wrapping_add(1);
            n.link = Link::Closed;
        }
        false
    }

    /// Seed cells from `node` back to the start (the parent chain), bounded by the node count.
    fn chain(&self, node: u32) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        let mut cur = Some(node);
        let mut steps = 0usize;
        while let Some(c) = cur {
            out.push(self.nodes[c as usize].seed);
            cur = self.nodes[c as usize].parent;
            steps += 1;
            if steps > self.nodes.len() {
                break;
            }
        }
        out
    }
}

/// Finds a region path from `start` to `goal` (world points) with the default budget.
pub fn find_path(grid: &Grid, regions: &RegionMap, start: (F32, F32), goal: (F32, F32)) -> Path {
    find_path_with_budget(grid, regions, start, goal, POP_BUDGET)
}

/// [`find_path`] with an explicit pop budget.
pub fn find_path_with_budget(
    grid: &Grid,
    regions: &RegionMap,
    start: (F32, F32),
    goal: (F32, F32),
    budget: u32,
) -> Path {
    let start_cell = grid.to_cell(start.0, start.1);
    let goal_cell = grid.to_cell(goal.0, goal.1);
    let (Some(start_region), Some(goal_region)) = (
        regions.region_at(start_cell.0, start_cell.1),
        regions.region_at(goal_cell.0, goal_cell.1),
    ) else {
        return Path {
            status: Status::Failed,
            cells: Vec::new(),
        };
    };
    if start_cell == goal_cell || start_region == goal_region {
        return Path {
            status: Status::SameRegion,
            cells: Vec::new(),
        };
    }

    let nodes: Vec<Node> = (0..regions.region_count() as u32)
        .map(|r| Node {
            seed: regions.seed(r),
            stamp: 0,
            link: Link::Untouched,
            g: 0,
            h: 0,
            parent: None,
        })
        .collect();
    let mut s = Search {
        regions,
        nodes,
        heap: Heap::new(),
        goal_cell,
        goal_region,
        best_d2: d2(start_cell, goal_cell),
        best_node: start_region,
        pops: 0,
    };
    let seed = s.nodes[start_region as usize].seed;
    s.nodes[start_region as usize].h = isqrt(scaled_d2(seed, goal_cell));
    s.push_node(start_region);

    if s.run(budget) {
        return Path {
            status: Status::Reached,
            cells: s.chain(goal_region),
        };
    }
    if s.best_node != start_region {
        return Path {
            status: Status::Partial,
            cells: s.chain(s.best_node),
        };
    }
    Path {
        status: Status::Straight,
        cells: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    /// World point of cell `(x, y)` (its corner, origin at zero).
    fn at(x: i32, y: i32) -> (F32, F32) {
        (f(x as f32 * 32.0 + 1.0), f(y as f32 * 32.0 + 1.0))
    }

    /// A `w` by `h` grid whose labels come from `label(x, y)`.
    fn map(w: u32, h: u32, label: impl Fn(i32, i32) -> u32) -> (Grid, RegionMap) {
        let grid = Grid::new(w, h, F32::ZERO, F32::ZERO);
        let mut labels = Vec::new();
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                labels.push(label(x, y));
            }
        }
        (grid, RegionMap::from_labels(w, h, labels))
    }

    #[test]
    fn same_cell_and_same_region_are_straight() {
        let (g, r) = map(4, 1, |_, _| 0);
        assert_eq!(
            find_path(&g, &r, at(0, 0), at(0, 0)).status,
            Status::SameRegion
        );
        assert_eq!(
            find_path(&g, &r, at(0, 0), at(3, 0)).status,
            Status::SameRegion
        );
    }

    #[test]
    fn out_of_map_and_blocked_endpoints_fail() {
        let (g, r) = map(3, 1, |x, _| if x == 1 { NO_REGION } else { x as u32 });
        assert_eq!(find_path(&g, &r, at(0, 0), at(1, 0)).status, Status::Failed);
        assert_eq!(find_path(&g, &r, at(0, 0), at(9, 0)).status, Status::Failed);
        assert_eq!(
            find_path(&g, &r, at(-1, 0), at(2, 0)).status,
            Status::Failed
        );
    }

    #[test]
    fn corridor_is_walked_in_order() {
        let (g, r) = map(6, 1, |x, _| x as u32);
        let p = find_path(&g, &r, at(0, 0), at(5, 0));
        assert_eq!(p.status, Status::Reached);
        assert_eq!(
            p.cells,
            vec![(5, 0), (4, 0), (3, 0), (2, 0), (1, 0), (0, 0)]
        );
    }

    #[test]
    fn search_is_deterministic() {
        // One region per cell in an 8 by 8 map, with a wall column at x = 4 that has one gap
        // at y = 5. Every probe then lands on a different region, so the search is a cell
        // level 8-direction search with the corner rule.
        let label = |x: i32, y: i32| {
            if x == 4 && y != 5 {
                NO_REGION
            } else {
                (y * 8 + x) as u32
            }
        };
        let (g, r) = map(8, 8, label);
        let first = find_path(&g, &r, at(0, 0), at(7, 7));
        for _ in 0..5 {
            assert_eq!(find_path(&g, &r, at(0, 0), at(7, 7)), first);
        }
        assert_eq!(first.status, Status::Reached);
        assert_eq!(first.cells.first(), Some(&(7, 7)));
        assert_eq!(first.cells.last(), Some(&(0, 0)));
        // The path must pass the gap.
        assert!(first.cells.contains(&(4, 5)));
    }

    #[test]
    fn multi_cell_region_probes_only_from_its_seed() {
        // Model rule (pathing-2 5.4, UNVERIFIED): edges come from the eight neighbours of a
        // region's seed cell only. Region 0 spans (0, 0) and (1, 0); its seed (0, 0) sees
        // only its own cell to the east, so region 1 at (2, 0) is never reached.
        let (g, r) = map(3, 1, |x, _| if x < 2 { 0 } else { 1 });
        assert_eq!(
            find_path(&g, &r, at(0, 0), at(2, 0)).status,
            Status::Straight
        );
    }

    #[test]
    fn pop_cap_limits_the_search() {
        // A one-cell corridor of 600 regions: reaching the far end needs about 600 pops.
        let (g, r) = map(600, 1, |x, _| x as u32);
        let capped = find_path(&g, &r, at(0, 0), at(599, 0));
        assert_eq!(capped.status, Status::Partial);
        assert!(capped.cells.len() <= POP_BUDGET as usize + 1);
        let full = find_path_with_budget(&g, &r, at(0, 0), at(599, 0), 1000);
        assert_eq!(full.status, Status::Reached);
        assert_eq!(full.cells.len(), 600);
    }

    #[test]
    fn unreachable_goal_falls_back_to_the_closest_node() {
        // Regions 0, 1, 2 form a corridor, a wall cell at x = 3 cuts it, and region 3 holds
        // the goal. Touched nodes are 1 and 2; node 2 (seed (2, 0)) is the closest to the goal.
        let label = |x: i32, _y: i32| match x {
            0..=2 => x as u32,
            3 => NO_REGION,
            _ => 3,
        };
        let (g, r) = map(6, 1, label);
        let p = find_path(&g, &r, at(0, 0), at(5, 0));
        assert_eq!(p.status, Status::Partial);
        assert_eq!(p.cells, vec![(2, 0), (1, 0), (0, 0)]);
    }

    #[test]
    fn fallback_is_straight_when_nothing_beats_the_start() {
        // The start is the closest touched node (its neighbours are farther from the goal),
        // so the search reports a straight segment.
        let label = |x: i32, y: i32| match (x, y) {
            (0, 0) => 0,
            (0, 1) | (1, 0) => NO_REGION,
            _ => 1,
        };
        let (g, r) = map(4, 2, label);
        let p = find_path(&g, &r, at(0, 0), at(3, 1));
        assert_eq!(p.status, Status::Straight);
        assert!(p.cells.is_empty());
    }

    #[test]
    fn connected_labelling_separates_walls() {
        let mut g = Grid::new(3, 3, F32::ZERO, F32::ZERO);
        g.set_type_bits(1, 0, 0x02);
        g.set_type_bits(1, 1, 0x02);
        g.set_type_bits(1, 2, 0x02);
        let r = RegionMap::label_connected(&g, 0x02);
        assert_eq!(r.region_at(0, 0), Some(0));
        assert_eq!(r.region_at(1, 0), None);
        assert_ne!(r.region_at(2, 0), r.region_at(0, 0));
        assert_eq!(r.seed(0), (0, 0));
    }
}
