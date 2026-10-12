//! The computer AI's build-spot search (`docs/spec/ai-natives.md` section 8).
//!
//! A port of the placement routine behind `SetProduce` for a structure: `0x6F2E50E0` (wrapper) with its work
//! buffer builder `0x6F2E1630`, the blocker marking (`0x6F2DF650` -> callback `0x6F2DF4A0` -> footprint walk
//! `0x6F3E0D30` -> cell callback `0x6F2DF420` -> cell marker `0x6F2DF380`), the position snap `0x6F3E0B50` and the
//! breadth-first search `0x6F2DF6F0` with its neighbour push `0x6F2DF680`. All coordinates go through the
//! softfloat library in the operand order of the original, so the results are bit for bit those of the DLL
//! (Unicorn vectors: `tools/emu/ai_site_emu.py`, `crates/sim/tests/ai_site_vectors.rs`).
//!
//! What is decoded (CONFIRMED where checked by those vectors, otherwise LIKELY/UNSURE as marked):
//!
//! * The work buffer is a 101 x 101 window of 32-unit cells centred on the town (the hall position): its origin is
//!   `center - 1600.0`. Two byte grids: the *value* grid and the *state* grid. A value cell is `5` (free of
//!   everything), `0x9C` (unwalkable), `0x9E` (another cliff level than the centre) or, after the marking, `0..3`
//!   (inside the buffer radius of a structure of that tier) or `0x9D` (under a structure footprint on a level cell).
//! * Every structure with a pathing footprint within 1600 of the centre marks every cell of its footprint (the
//!   pathing texture walked in the orientation of the Misc `BuildingAngle`) and a square of Chebyshev radius `r`
//!   around each footprint cell: a cell keeps the smaller of its value and the tier of the marker. The tier and `r`
//!   come from the type's `buffType` (`townhall` 0, `resource` 1, `factory` 2, `buffer` 3, anything else 3 with
//!   `r = 6`) and `buffRadius` (`r = trunc(buffRadius)`; a `buffer` of the tower/ziggurat family gets 15 or 5).
//! * The search is a queue walk from the centre cell over the 8 neighbours (in the order W, SW, NW, E, SE, NE, S,
//!   N), at most once per cell. Each popped cell is turned into a world point, snapped to the building grid by the
//!   texture size, and tested if its value cell is `5` (take the first that passes the host's placement test) or
//!   `0..3` (remember the first passing candidate per tier). Without a `5` candidate the highest tier wins.
//!
//! The four host predicates are the parts of the engine this module does not own ([`SiteHost`]): the cliff level of
//! a cell, the unwalkable test of the pathing map, the reachability query and the general placement test
//! (`0x6F0128C0`, `0x6F468F90`, `0x6F475560`, `0x6F2A50C0`).

use softfloat::{arith, conv, F32};

use crate::natives::game::Playable;
use crate::unit_position::{clamp_to_playable, world_to_cell};

/// Cells per side of the work window.
pub const GRID: usize = 101;
/// Capacity of the queue ring (`0x27D9`).
pub const RING: usize = 10201;

/// `1600.0` (`0x6FAB4EB0`): half the window width in world units.
pub const HALF_WINDOW: F32 = F32::from_bits(0x44C8_0000);
/// `1/32` (`0x6FAB4EB4`).
pub const INV32: F32 = F32::from_bits(0x3D00_0000);
/// `-99999.0` (`0x6FAB4EB8`): the empty-bucket sentinel.
pub const SENTINEL: F32 = F32::from_bits(0xC7C3_4F80);
/// `32.0` (`0x6FADB3F8`, `0x6FAB68A4`).
pub const C32: F32 = F32::from_bits(0x4200_0000);
/// `16.0` (`0x6FAB68A8`).
pub const C16: F32 = F32::from_bits(0x4180_0000);
/// `-1.0`: the status of a failed search.
pub const NEG_ONE: F32 = F32::from_bits(0xBF80_0000);

/// Value cell: free.
pub const CELL_FREE: u8 = 5;
/// Value cell: unwalkable.
pub const CELL_BLOCKED: u8 = 0x9C;
/// Value cell: under a structure footprint.
pub const CELL_UNDER: u8 = 0x9D;
/// Value cell: another cliff level than the centre.
pub const CELL_OTHER_LEVEL: u8 = 0x9E;

/// `'hwtw'` (scout tower) and `'uzig'` (ziggurat): the tower family that gets the special buffer radii.
const RAW_HWTW: u32 = 0x6877_7477;
const RAW_UZIG: u32 = 0x757A_6967;

/// The size of a pathing texture in texels (a 32-unit square each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footprint {
    pub w: u32,
    pub h: u32,
}

/// The engine predicates the search needs.
pub trait SiteHost {
    /// `0x6F0128C0(cell_x, cell_y, 0)`: the terrain level of a cell (cells are relative to the playable minimum).
    fn cell_level(&self, cell_x: i32, cell_y: i32) -> i32;
    /// `0x6F468F90(&x, &y, 2)`: the pathing cell under the world point has the unwalkable bit.
    fn unwalkable(&self, x: F32, y: F32) -> bool;
    /// `0x6F475560` is not `-1`: the builder's movement type can path from `from` to `to`.
    fn reachable(&self, from: (F32, F32), to: (F32, F32)) -> bool;
    /// `0x6F2A50C0` returns 0: the building may stand at the point.
    fn placement_ok(&self, x: F32, y: F32) -> bool;
}

/// What the search is asked.
#[derive(Debug, Clone)]
pub struct Params {
    /// Raw code of the building type.
    pub type_id: u32,
    /// The AI player.
    pub player: u32,
    /// The wrapper's `a6`: the AI already has two or more of the type (owned plus in progress, `0x6F2E50E0`).
    pub count_ge_two: bool,
    /// The search centre: the position of the town's hall.
    pub center: (F32, F32),
    /// The playable area (the world object's `+0x6C..+0x78`): origin of the cell numbers and clamp rectangle.
    pub playable: Playable,
    /// The pathing texture of the building type.
    pub footprint: Footprint,
    /// Orientation class (`0..3`) of the Misc `BuildingAngle` ([`rotation_class`]).
    pub rot: u32,
}

/// One structure that marks the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkUnit {
    /// `buffType` enum of the type record (`+0x1B8`): 0 none, 1 townhall, 2 resource, 4 factory, 8 buffer.
    pub buff_type: u32,
    /// `buffRadius` of the type record (`+0x1BC`).
    pub buff_radius: F32,
    /// The unit's position.
    pub x: F32,
    pub y: F32,
    /// The pathing texture of the unit's type.
    pub footprint: Footprint,
}

/// `near(x)`: `|x| < 2e-7`, as the x87 compare of `0x6F2DF6F0` (`2e-7 <= |x|` is false, ordered).
fn near_zero(x: F32) -> bool {
    let a = f32::from_bits(x.0 & 0x7FFF_FFFF);
    !a.is_nan() && (a as f64) < f32::from_bits(0x3456_BF95) as f64
}

/// The orientation class of an angle in degrees: 90 -> 1, 180 -> 2, 270 -> 3, anything else 0 (the compare chain at
/// the top of `0x6F2DF6F0`, `0x6F2A50C0`, `0x6F2DF4A0`). The angle is the Misc `BuildingAngle` through
/// `i2r(f2i(value))` (`0x6F020D30`).
pub fn rotation_class(angle: F32) -> u32 {
    if near_zero(arith::sub(angle, F32::from_bits(0x42B4_0000))) {
        1
    } else if near_zero(arith::sub(angle, F32::from_bits(0x4334_0000))) {
        2
    } else if near_zero(arith::sub(angle, F32::from_bits(0x4387_0000))) {
        3
    } else {
        0
    }
}

/// `0x6F020D30`: the building angle from the Misc real (`i2r` of its truncation).
pub fn building_angle(misc_value: F32) -> F32 {
    conv::i2r(conv::f2i(misc_value))
}

/// The work buffer of `0x6F2E1630` (the DLL keeps it as a flat struct; the fields are named by their offsets).
#[derive(Clone)]
pub struct Work {
    /// `+0x00`: radius scratch of the marking callback.
    pub radius: i32,
    /// `+0x04`: tier scratch of the marking callback.
    pub tier: i32,
    /// `+0x08`, `+0x0C`: the window origin in cell units (`origin * 1/32`).
    pub cell_origin: (F32, F32),
    /// `+0x14`: tower/ziggurat type and the AI has two or more.
    pub flag_a: bool,
    /// `+0x18`: tower/ziggurat type and fewer than two.
    pub flag_b: bool,
    /// `+0x20`: a reachability query failed.
    pub reach_failed: bool,
    /// The value grid, `[y][x]`.
    pub value: Vec<[u8; GRID]>,
    /// The state grid, `[y][x]`: 5 untouched, 6 queued, 7 done, 4 taken.
    pub state: Vec<[u8; GRID]>,
    /// `+0x5564`, `+0x5568`: world origin of the window.
    pub origin: (F32, F32),
    /// `+0x556C`, `+0x5570`: the centre.
    pub center: (F32, F32),
    /// Candidate x/y per tier (`+0x5580`, `+0x5594`).
    pub bucket_x: [F32; 4],
    pub bucket_y: [F32; 4],
    head: usize,
    tail: usize,
    queue_x: Vec<u32>,
    queue_y: Vec<u32>,
}

impl std::fmt::Debug for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Work").field("origin", &self.origin).field("center", &self.center).finish()
    }
}

fn cell_of(v: F32, origin: F32) -> i32 {
    conv::f2i(world_to_cell(v, origin))
}

impl Work {
    /// `0x6F2E1630`: builds the window around the centre.
    pub fn new(host: &dyn SiteHost, p: &Params) -> Work {
        let (cx, cy) = p.center;
        let origin = (arith::sub(cx, HALF_WINDOW), arith::sub(cy, HALF_WINDOW));
        let cell_origin = (arith::mul(origin.0, INV32), arith::mul(origin.1, INV32));
        let tower = p.type_id == RAW_HWTW || p.type_id == RAW_UZIG;
        let mut w = Work {
            radius: 0,
            tier: 0,
            cell_origin,
            flag_a: tower && p.count_ge_two,
            flag_b: tower && !p.count_ge_two,
            reach_failed: false,
            value: vec![[0; GRID]; GRID],
            state: vec![[CELL_FREE; GRID]; GRID],
            origin,
            center: p.center,
            bucket_x: [SENTINEL; 4],
            bucket_y: [F32::ZERO; 4],
            head: 0,
            tail: 1,
            queue_x: vec![0; RING],
            queue_y: vec![0; RING],
        };
        w.queue_x[0] = 50;
        w.queue_y[0] = 50;
        let level0 = host.cell_level(cell_of(cx, p.playable.min_x), cell_of(cy, p.playable.min_y));
        let mut y_run = origin.1;
        for j in 0..GRID {
            let mut x_run = origin.0;
            for i in 0..GRID {
                let ix = cell_of(x_run, p.playable.min_x);
                let iy = cell_of(y_run, p.playable.min_y);
                w.value[j][i] = if host.cell_level(ix, iy) == level0 {
                    if host.unwalkable(x_run, y_run) {
                        CELL_BLOCKED
                    } else {
                        CELL_FREE
                    }
                } else {
                    CELL_OTHER_LEVEL
                };
                w.state[j][i] = CELL_FREE;
                x_run = arith::add(x_run, C32);
            }
            y_run = arith::add(y_run, C32);
        }
        w
    }

    /// `0x6F2DF380(this, x, y)`: marks one footprint cell and the square around it.
    pub fn mark_cell(&mut self, x: i32, y: i32) {
        if (x as u32) <= 100 && (y as u32) <= 100 {
            let c = &mut self.value[y as usize][x as usize];
            if (*c as i8) > (CELL_UNDER as i8) {
                *c = CELL_UNDER;
            }
        }
        let r = self.radius;
        let n = r.wrapping_mul(2).wrapping_add(1);
        if n > 0 {
            let mut yy = y.wrapping_sub(r);
            for _ in 0..n {
                let mut xx = x.wrapping_sub(r);
                for _ in 0..n {
                    if (xx as u32) <= 100 && (yy as u32) <= 100 {
                        let c = &mut self.value[yy as usize][xx as usize];
                        if (*c as i8 as i32) > self.tier {
                            *c = self.tier as u8;
                        }
                    }
                    xx = xx.wrapping_add(1);
                }
                yy = yy.wrapping_add(1);
            }
        }
    }

    /// `0x6F2DF420`: the cell callback of the footprint walk: texel position -> cell -> [`Work::mark_cell`].
    fn mark_texel(&mut self, px: F32, py: F32) {
        let cx = conv::f2i(arith::sub(arith::mul(px, INV32), self.cell_origin.0));
        let cy = conv::f2i(arith::sub(arith::mul(py, INV32), self.cell_origin.1));
        self.mark_cell(cx, cy);
    }

    /// `0x6F2DF4A0` for one structure.
    pub fn mark_unit(&mut self, u: &MarkUnit, rot: u32) {
        let trunc = conv::f2i(u.buff_radius);
        let (tier, radius) = match u.buff_type {
            1 => (0, trunc),
            2 => (1, trunc),
            4 => (2, trunc),
            8 => {
                let r = if self.flag_a {
                    15
                } else if self.flag_b {
                    5
                } else {
                    trunc
                };
                (3, r)
            }
            _ => (3, 6),
        };
        self.radius = radius;
        self.tier = tier;
        footprint_walk(u.footprint, rot, u.x, u.y, |px, py| self.mark_texel(px, py));
    }

    /// `0x6F2DF680`: queues a cell if it is untouched.
    fn push(&mut self, x: u32, y: u32) {
        let s = &mut self.state[y as usize][x as usize];
        if *s == CELL_FREE {
            self.queue_x[self.tail] = x;
            self.queue_y[self.tail] = y;
            self.tail += 1;
            if self.tail == RING {
                self.tail = 0;
            }
            *s = 6;
        }
    }

    /// `0x6F2DF6F0`: the search. Returns the chosen point. `check_reach` is the third argument of the DLL routine
    /// (1 on the first pass, 0 on the retry).
    pub fn search(&mut self, host: &dyn SiteHost, p: &Params, check_reach: bool) -> Option<(F32, F32)> {
        let rot = p.rot;
        while self.head != self.tail {
            let x = self.queue_x[self.head];
            let y = self.queue_y[self.head];
            self.head += 1;
            if self.head == RING {
                self.head = 0;
            }
            let mut px = arith::add(self.origin.0, arith::mul(conv::i2r(x as i32), C32));
            let mut py = arith::add(self.origin.1, arith::mul(conv::i2r(y as i32), C32));
            let (sx, sy) = snap(p.footprint, rot, p.playable, px, py);
            px = sx;
            py = sy;
            let ix = conv::f2i(arith::mul(INV32, arith::sub(px, self.origin.0))) as u32;
            let iy = conv::f2i(arith::mul(INV32, arith::sub(py, self.origin.1))) as u32;
            if ix < 101 && iy < 101 {
                let state = self.state[iy as usize][ix as usize];
                let value = self.value[iy as usize][ix as usize];
                if value == CELL_FREE {
                    if state != 7 {
                        if check_reach && !host.reachable(self.center, (px, py)) {
                            self.reach_failed = true;
                        } else if host.placement_ok(px, py) {
                            self.value[iy as usize][ix as usize] = 4;
                            self.state[iy as usize][ix as usize] = 4;
                            return Some((px, py));
                        }
                    }
                } else if (value as i8) >= 0 && value <= 3 {
                    let t = value as usize;
                    if self.bucket_x[t].0 == SENTINEL.0 && state != 7 {
                        if check_reach && !host.reachable(self.center, (px, py)) {
                            self.reach_failed = true;
                        } else if host.placement_ok(px, py) {
                            self.bucket_x[t] = px;
                            self.bucket_y[t] = py;
                        }
                    }
                }
            }
            let (qx, qy) = (x, y);
            let xm = qx.wrapping_sub(1);
            if xm <= 99 {
                self.push(xm, qy);
                if qy.wrapping_sub(1) <= 99 {
                    self.push(xm, qy - 1);
                }
                if qy <= 99 {
                    self.push(xm, qy + 1);
                }
            }
            if qx <= 99 {
                self.push(qx + 1, qy);
                if qy.wrapping_sub(1) <= 99 {
                    self.push(qx + 1, qy - 1);
                }
                if qy <= 99 {
                    self.push(qx + 1, qy + 1);
                }
            }
            if qy.wrapping_sub(1) <= 99 {
                self.push(qx, qy - 1);
            }
            if qy <= 99 {
                self.push(qx, qy + 1);
            }
            if ix < 101 && iy < 101 {
                self.state[iy as usize][ix as usize] = 7;
            }
        }
        // No cell of value 5 passed: the best tier that found a candidate.
        let mut t = 3i32;
        loop {
            if self.bucket_x[t as usize].0 != SENTINEL.0 {
                break;
            }
            t -= 1;
            if t < 0 {
                return None;
            }
        }
        let t = t as usize;
        let bx = conv::f2i(arith::mul(arith::sub(self.bucket_x[t], self.origin.0), INV32));
        let by = conv::f2i(arith::mul(arith::sub(self.bucket_y[t], self.origin.1), INV32));
        if (bx as u32) < 101 && (by as u32) < 101 {
            self.value[by as usize][bx as usize] = 4;
            self.state[by as usize][bx as usize] = 4;
        }
        Some((self.bucket_x[t], self.bucket_y[t]))
    }
}

/// `0x6F3E0D30`: walks the texels of a footprint of size `fp` centred on (px, py) in orientation `rot`, calling `f`
/// with the world point of each texel in the DLL's order (rows, then columns; the position advances by softfloat
/// adds, and the coordinate that the column step moves is reset after each row).
pub fn footprint_walk(fp: Footprint, rot: u32, px: F32, py: F32, mut f: impl FnMut(F32, F32)) {
    let hx = arith::mul(conv::i2r(fp.w.wrapping_sub(1) as i32), C16);
    let hy = arith::mul(conv::i2r(fp.h.wrapping_sub(1) as i32), C16);
    let neg = |v: F32| F32(v.0 ^ 0x8000_0000);
    let z = F32::ZERO;
    // (x0, y0), column step, row step
    let ((x0, y0), col, row) = match rot {
        0 => ((arith::sub(px, hx), arith::sub(py, hy)), (C32, z), (z, C32)),
        1 => ((arith::add(px, hy), arith::sub(py, hx)), (z, C32), (neg(C32), z)),
        2 => ((arith::add(px, hx), arith::add(py, hy)), (neg(C32), z), (z, neg(C32))),
        _ => ((arith::sub(px, hy), arith::add(py, hx)), (z, neg(C32)), (C32, z)),
    };
    let (mut cx, mut cy) = (x0, y0);
    for _ in 0..fp.h {
        for _ in 0..fp.w {
            f(cx, cy);
            cx = arith::add(cx, col.0);
            cy = arith::add(cy, col.1);
        }
        cx = arith::add(cx, row.0);
        cy = arith::add(cy, row.1);
        if rot == 0 || rot == 2 {
            cx = x0;
        } else {
            cy = y0;
        }
    }
}

/// `0x6F3E0B50`: snaps a cell corner to the building grid for a footprint of this size and orientation.
pub fn snap(fp: Footprint, rot: u32, playable: Playable, x: F32, y: F32) -> (F32, F32) {
    let (xdim, ydim) = if rot == 0 || rot == 2 { (fp.w, fp.h) } else { (fp.h, fp.w) };
    let x_even = xdim & 1 == 0;
    let y_even = ydim & 1 == 0;
    let mut bx = ((xdim >> 1) & 1) as i32;
    let mut by = ((ydim >> 1) & 1) as i32;
    let (x, y) = clamp_to_playable(playable, x, y);
    let neg_x = x.to_f32() < 0.0;
    let neg_y = y.to_f32() < 0.0;
    let xi = conv::f2i(x);
    let yi = conv::f2i(y);
    let mut xs = xi - xi % 64;
    let mut ys = yi - yi % 64;
    if neg_x {
        bx = -bx;
    }
    xs += bx << 5;
    if neg_y {
        by = -by;
    }
    ys += by << 5;
    let sx = conv::i2r(xs);
    let sy = conv::i2r(ys);
    let neg = |v: F32| F32(v.0 ^ 0x8000_0000);
    let ax = if x_even {
        F32::ZERO
    } else if neg_x {
        neg(C16)
    } else {
        C16
    };
    let ay = if y_even {
        F32::ZERO
    } else if neg_y {
        neg(C16)
    } else {
        C16
    };
    (arith::add(sx, ax), arith::add(sy, ay))
}

/// `0x6F2E50E0` after the buffer is built and marked: the first pass with the reachability test, and, when it found
/// nothing because a reachability query failed, a second pass without it on the copy taken before the first.
pub fn run_search(mut work: Work, host: &dyn SiteHost, p: &Params) -> Option<(F32, F32)> {
    let backup = work.clone();
    let first = work.search(host, p, true);
    if first.is_some() || !work.reach_failed {
        return first;
    }
    let mut again = backup;
    again.search(host, p, false)
}

/// The whole routine: build the window, mark the structures, search.
pub fn find_site(host: &dyn SiteHost, p: &Params, units: &[MarkUnit]) -> Option<(F32, F32)> {
    let mut w = Work::new(host, p);
    for u in units {
        w.mark_unit(u, p.rot);
    }
    run_search(w, host, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Open;
    impl SiteHost for Open {
        fn cell_level(&self, _: i32, _: i32) -> i32 {
            0
        }
        fn unwalkable(&self, _: F32, _: F32) -> bool {
            false
        }
        fn reachable(&self, _: (F32, F32), _: (F32, F32)) -> bool {
            true
        }
        fn placement_ok(&self, _: F32, _: F32) -> bool {
            true
        }
    }

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    fn playable() -> Playable {
        Playable { min_x: f(-4096.0), min_y: f(-4096.0), max_x: f(4096.0), max_y: f(4096.0) }
    }

    fn params(w: u32, h: u32) -> Params {
        Params {
            type_id: 0x6862_6172,
            player: 0,
            count_ge_two: false,
            center: (f(0.0), f(0.0)),
            playable: playable(),
            footprint: Footprint { w, h },
            rot: 3,
        }
    }

    #[test]
    fn rotation_classes_follow_the_angle_compare_chain() {
        assert_eq!(rotation_class(f(0.0)), 0);
        assert_eq!(rotation_class(f(90.0)), 1);
        assert_eq!(rotation_class(f(180.0)), 2);
        assert_eq!(rotation_class(f(270.0)), 3);
        assert_eq!(rotation_class(f(45.0)), 0);
        assert_eq!(rotation_class(building_angle(f(270.7))), 3);
    }

    #[test]
    fn snap_aligns_to_the_building_grid_by_footprint_parity() {
        // An even footprint stands on a 64-grid line, an odd one is moved by 16.
        let (x, y) = snap(Footprint { w: 4, h: 4 }, 3, playable(), f(100.0), f(200.0));
        assert_eq!((x.to_f32(), y.to_f32()), (64.0, 192.0));
        let (x, y) = snap(Footprint { w: 3, h: 3 }, 3, playable(), f(100.0), f(200.0));
        // Size 3: bit 1 is set (+32) and the size is odd (+16): 64 + 32 + 16 and 192 + 32 + 16.
        assert_eq!((x.to_f32(), y.to_f32()), (112.0, 240.0));
    }

    #[test]
    fn footprint_walk_visits_every_texel_once_on_the_32_lattice() {
        let mut seen = Vec::new();
        footprint_walk(Footprint { w: 3, h: 2 }, 3, f(0.0), f(0.0), |x, y| seen.push((x.to_f32(), y.to_f32())));
        assert_eq!(seen.len(), 6);
        seen.sort_by(|a, b| a.partial_cmp(b).unwrap());
        seen.dedup();
        assert_eq!(seen.len(), 6);
    }

    #[test]
    fn an_empty_map_gives_the_first_cell_in_queue_order() {
        let p = params(4, 4);
        let got = find_site(&Open, &p, &[]).unwrap();
        // The centre cell is popped first; its corner (-32, -32) snaps to the building grid.
        let want = snap(p.footprint, p.rot, p.playable, f(-32.0), f(-32.0));
        assert_eq!(got, want);
    }

    #[test]
    fn a_town_hall_buffer_pushes_the_site_out_of_its_radius() {
        let p = params(4, 4);
        let hall = MarkUnit {
            buff_type: 1,
            buff_radius: f(8.0),
            x: f(0.0),
            y: f(0.0),
            footprint: Footprint { w: 16, h: 16 },
        };
        let (x, y) = find_site(&Open, &p, &[hall]).unwrap();
        // 8 texels either side of the centre plus the 8-cell buffer.
        assert!(x.to_f32().abs().max(y.to_f32().abs()) >= 16.0 * 16.0 + 8.0 * 32.0 - 64.0);
    }

    #[test]
    fn tiers_are_used_when_no_free_cell_passes() {
        struct OnlyNearBuffer;
        impl SiteHost for OnlyNearBuffer {
            fn cell_level(&self, _: i32, _: i32) -> i32 {
                0
            }
            fn unwalkable(&self, _: F32, _: F32) -> bool {
                false
            }
            fn reachable(&self, _: (F32, F32), _: (F32, F32)) -> bool {
                true
            }
            fn placement_ok(&self, x: F32, _: F32) -> bool {
                x.to_f32() < 0.0
            }
        }
        // A buffer covers the whole window: every cell has tier 3, so the first passing cell in queue order is the
        // candidate of bucket 3 and is returned after the queue is exhausted.
        let big = MarkUnit { buff_type: 8, buff_radius: f(60.0), x: f(0.0), y: f(0.0), footprint: Footprint { w: 1, h: 1 } };
        let p = params(2, 2);
        let got = find_site(&OnlyNearBuffer, &p, &[big]).unwrap();
        assert!(got.0.to_f32() < 0.0);
    }

    #[test]
    fn no_candidate_at_all_is_none() {
        struct Never;
        impl SiteHost for Never {
            fn cell_level(&self, _: i32, _: i32) -> i32 {
                0
            }
            fn unwalkable(&self, _: F32, _: F32) -> bool {
                false
            }
            fn reachable(&self, _: (F32, F32), _: (F32, F32)) -> bool {
                true
            }
            fn placement_ok(&self, _: F32, _: F32) -> bool {
                false
            }
        }
        assert_eq!(find_site(&Never, &params(2, 2), &[]), None);
    }
}
