//! Regenerates the random cases of `tools/emu/pyramid_vectors.py` and `search_vectors.py` from
//! their seeds, and runs them through the Rust port. The generators draw in the same order as
//! the scripts (`make_case`, `make_search_case`), so the digests recorded from the original apply.

use sim::pathing::pyramid::{Labels, Level, Pyramid};
use sim::pathing::search::{FlagLink, Request, Search};
use softfloat::F32;

/// One pyramid lookup: `(level, x, y)`.
type PyramidOp = (usize, i32, i32);
/// One pyramid request: whether the generation is bumped, and its lookups.
pub type PyramidRequest = (bool, Vec<PyramidOp>);
/// A cell `(x, y)`.
pub type Cell = (i32, i32);
/// One search request: path type, flag, start cell, goal cell, budget.
pub type SearchRequest = (u32, bool, Cell, Cell, u32);
/// The status name and the path cells of one request.
pub type PathReport = (String, Vec<Cell>);

const M64: u64 = u64::MAX;
const DIGEST_INIT: u64 = 0xCBF2_9CE4_8422_2325;
const DIGEST_MUL: u64 = 0x0000_0100_0000_01B3;

/// The LCG of the vector scripts: 64-bit state, `u32` is the top 32 bits.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) & M64)
    }

    pub fn u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        self.u32() % n
    }
}

/// FNV-style fold of 32-bit words (the scripts' `fnv_words`).
pub fn fnv(mut h: u64, words: &[u32]) -> u64 {
    for &w in words {
        h = (h ^ u64::from(w)).wrapping_mul(DIGEST_MUL);
    }
    h
}

/// Two-bit state of `level` for path type `t`, placed in the word as the scripts do.
fn word_with_state(hi16: u32, state: u32, t: u32) -> u32 {
    let mut word = hi16 << 16;
    word &= !((1u32 << (31 - t)) | (1u32 << (30 - t)));
    word |= ((state >> 1) << (31 - t)) | ((state & 1) << (30 - t));
    word
}

/// A state draw: 0 (50%), 1 (15%), 2 (20%), 3 (15%).
fn draw_state(r: u32) -> u32 {
    if r < 50 {
        0
    } else if r < 65 {
        1
    } else if r < 85 {
        2
    } else {
        3
    }
}

/// One pyramid lookup test case (`make_case` in `pyramid_vectors.py`).
pub struct PyramidCase {
    pub path_type: u32,
    pub flag_on: bool,
    pub grow: usize,
    pub cap0: usize,
    pub pyramid: Pyramid,
    /// The requests of the case.
    pub requests: Vec<PyramidRequest>,
}

pub fn pyramid_case(seed: u64) -> PyramidCase {
    let mut g = Lcg::new(seed);
    let w0 = 8 * (1 + g.below(3));
    let h0 = 8 * (1 + g.below(3));
    let t = 2 * g.below(8);
    let flag_on = g.below(2) == 1;
    let grow = [0usize, 1, 2, 3, 4, 8][g.below(6) as usize];
    let cap0 = [0usize, 1, 2, 5][g.below(4) as usize];
    let mut levels = Vec::new();
    for k in 0..4 {
        let (w, h) = (w0 >> k, h0 >> k);
        let mut words = Vec::new();
        for _ in 0..w * h {
            let state = draw_state(g.below(100));
            let hi16 = g.u32() & 0xFFFF;
            words.push(word_with_state(hi16, state, t));
        }
        levels.push(Level::new(w, h, &words));
    }
    let mut requests = Vec::new();
    for _ in 0..1 + g.below(4) {
        let bump = g.below(5) != 0;
        let mut ops = Vec::new();
        for _ in 0..1 + g.below(10) {
            let level = if g.below(3) == 0 { g.below(4) } else { 0 };
            let x = g.below(w0 + 4) as i32 - 2;
            let y = g.below(h0 + 4) as i32 - 2;
            ops.push((level as usize, x, y));
        }
        requests.push((bump, ops));
    }
    let [l0, l1, l2, l3]: [Level; 4] = levels.try_into().expect("four levels");
    PyramidCase {
        path_type: t,
        flag_on,
        grow,
        cap0,
        pyramid: Pyramid::new([l0, l1, l2, l3]),
        requests,
    }
}

/// Runs a pyramid case; returns the digest the script computes (one digest per lookup).
pub fn run_pyramid_case(case: PyramidCase) -> u64 {
    let mut pyramid = case.pyramid;
    let mut labels = Labels::new(case.path_type, case.flag_on, case.grow, case.cap0);
    let mut dig = DIGEST_INIT;
    for (bump, ops) in case.requests {
        if bump {
            labels.gen = labels.gen.wrapping_add(1);
        }
        labels.nodes.clear();
        for (level, x, y) in ops {
            let ret = pyramid.lookup(&mut labels, level, x, y);
            let mut words = vec![ret as u32, labels.gen, labels.nodes.len() as u32];
            for k in 0..4 {
                for r in pyramid.level(k).records() {
                    words.push(r.stamp);
                    words.push(r.word1);
                }
            }
            for n in &labels.nodes {
                words.extend([
                    n.seed.0 as u32,
                    n.seed.1 as u32,
                    u32::from(n.flag),
                    u32::from(n.level),
                ]);
            }
            dig = fnv(dig, &words);
        }
    }
    dig
}

/// One search case (`make_search_case` in `search_vectors.py`).
pub struct SearchCase {
    pub grow: usize,
    pub cap0: usize,
    pub pyramid: Pyramid,
    pub links: Vec<FlagLink>,
    /// The requests of the case.
    pub requests: Vec<SearchRequest>,
}

pub fn search_case(seed: u64) -> SearchCase {
    let mut g = Lcg::new(seed.wrapping_add(0x200_0000));
    let w0 = 8 * (1 + g.below(3));
    let h0 = 8 * (1 + g.below(3));
    let cell_only = g.below(3) == 0;
    let grow = [1usize, 2, 4, 8][g.below(4) as usize];
    let cap0 = [0usize, 3][g.below(2) as usize];
    let _hgrow = [1u32, 4, 16][g.below(3) as usize];
    let mut levels = Vec::new();
    for k in 0..4 {
        let (w, h) = (w0 >> k, h0 >> k);
        let mut words = Vec::new();
        for _ in 0..w * h {
            let r = g.below(100);
            let state = if k > 0 && cell_only { 3 } else { draw_state(r) };
            let hi16 = g.u32() & 0xFFFF;
            // The search pyramid is built with path type 0 (as the script does).
            words.push(word_with_state(hi16, state, 0));
        }
        levels.push(Level::new(w, h, &words));
    }
    let mut links = Vec::new();
    for k in 0..256 {
        let bit0 = k != 0 && g.below(3) == 0;
        let tx = g.below(w0) as i32;
        let ty = g.below(h0) as i32;
        links.push(FlagLink {
            enabled: bit0,
            x: tx,
            y: ty,
        });
    }
    let mut requests = Vec::new();
    for _ in 0..1 + g.below(3) {
        let t = 2 * g.below(8);
        let flag = g.below(2) == 1;
        let scx = g.below(w0) as i32;
        let scy = g.below(h0) as i32;
        let gcx = g.below(w0 + 2) as i32 - 1;
        let gcy = g.below(h0 + 2) as i32 - 1;
        let budget = [5u32, 40, 400][g.below(3) as usize];
        requests.push((t, flag, (scx, scy), (gcx, gcy), budget));
    }
    let [l0, l1, l2, l3]: [Level; 4] = levels.try_into().expect("four levels");
    SearchCase {
        grow,
        cap0,
        pyramid: Pyramid::new([l0, l1, l2, l3]),
        links,
        requests,
    }
}

/// A cell as the request takes it: a float holding the cell number.
fn point(cell: (i32, i32)) -> (F32, F32) {
    (
        F32::from_bits((cell.0 as f32).to_bits()),
        F32::from_bits((cell.1 as f32).to_bits()),
    )
}

/// Runs a search case; returns the digest and the path status of each request.
pub fn run_search_case(case: SearchCase) -> (u64, Vec<PathReport>) {
    let mut search = Search::new(case.pyramid, case.links, case.grow, case.cap0);
    let mut dig = DIGEST_INIT;
    let mut paths = Vec::new();
    for (path_type, flag_on, start, goal, budget) in case.requests {
        let req = Request {
            path_type,
            flag_on,
            start: point(start),
            goal: point(goal),
            budget,
        };
        let path = search.find_path(&req);
        dig = fnv(dig, &search.observable_words());
        let name = format!("{:?}", path.status).to_lowercase();
        paths.push((name, path.cells));
    }
    (dig, paths)
}
