//! The path follower's walk step (`docs/spec/movement-8.md`): the straight-line check `0x6F49D8D0`, the blocker scan
//! `0x6F49D770` and the blocked branch `0x6F49DE90` with the detour interval `0x6F49B340`.
//!
//! Every walk of a moving unit asks its follower `P` (`U+0xA8`, `0x6F49E020`) for the next waypoint. After the reach
//! test, the walk step `0x6F49DF00` checks whether the **one-cell step** from the unit's position toward the waypoint
//! is free: the direction to the waypoint is shortened to at most one cell, the cell of `pos + step` is traced
//! (`0x6F4A2070` for footprint class 0, `0x6F4A2180/2360/2550` for the larger classes) together with the cells a
//! diagonal step cuts, and every blocked cell and every foreign occupant found puts an entry on a list. An empty list
//! means a clear step. A non-empty list goes to the blocker scan, which marks the first moving occupant as the thing
//! to wait for. If no occupant was marked (terrain, a standing unit, a dead one), the unit asks for a detour:
//! `7 + rand(2)` is drawn from the **global generator** (`0x6F49B340`, one draw per blocked walk) and the waypoint
//! array is dropped.
//!
//! All of it is CONFIRMED against the real code in Unicorn (`tools/emu/follow_emu.py`, vectors replayed by
//! `crates/sim/tests/follow_vectors.rs`). What the host answers (movers, handles, units) is the [`FollowHost`] trait.

use softfloat::{arith, conv, math, F32, Tables};

use crate::occupancy::{Occupancy, DEAD_STAMP, KIND_STATIC, KIND_UNIT, NIL};

/// The mover tag (`U+0x10`) a list entry must carry (`0x60706375`).
pub const MOVER_TAG: u32 = 0x6070_6375;
/// `0.49` (`0x6FAAE4CC`).
const REACH: F32 = F32(0x3EFA_E148);
/// `144.0` (`0x6FAB775C`): the squared distance from the reissue point beyond which the detour always draws.
const FAR2: F32 = F32(0x4310_0000);
/// Footprint class thresholds (`0x6FAAE4D4` = 0.5, `0x6FAAE4F4` = 1.0, `0x6FAAE500` = 1.5).
const HALF: F32 = F32(0x3F00_0000);
const ONE: F32 = F32(0x3F80_0000);
const ONE_HALF: F32 = F32(0x3FC0_0000);
/// The list holds at most this many entries (`0x6F4A1E70`, `0x6F4A1EA0`).
const LIST_MAX: usize = 0x20;
/// `P+0x88` flag: the follower waits for a blocker (cleared by every walk).
pub const FLAG_WAITING: u32 = 0x0010_0000;

/// Footprint class of a radius (`0x6F38ADA0`): 0 below 0.5, 1 below 1.0, 3 from 1.5 up, else 2 (a NaN gives 2).
pub fn size_class(radius: F32) -> u8 {
    if arith::gt(HALF, radius) {
        0
    } else if arith::gt(ONE, radius) {
        1
    } else if arith::le(ONE_HALF, radius) {
        3
    } else {
        2
    }
}

/// The side data of an occupant node that the trace reads (`+0x34`, `+0x40`, `+0x30`); the stamp (`+0x38`) is
/// [`crate::occupancy::NodeRec::stamp`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeExtra {
    /// `+0x34`: bit 24 must be set to count; the low 24 bits are path-type bits tested against the follower's mask.
    pub flags: u32,
    /// `+0x40`: any bit outside `0x70000000` rejects the node.
    pub word40: u32,
    /// `+0x30`: the mover the node belongs to.
    pub owner: Option<u32>,
}

/// The follower fields of the walk step (`P`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follower {
    /// `+0x88`.
    pub flags: u32,
    /// `+0x94`: walks to wait.
    pub wait: u32,
    /// `+0x98`: the detour counter.
    pub retry: u32,
    /// `+0xA8` / `+0xAC`: the handle of the thing waited for (`-1, -1` for none).
    pub blocker: (u32, u32),
    /// `+0x9C`: the pathing mask of the unit.
    pub mask: u32,
    /// `+0xB4`: the footprint radius (cells).
    pub radius: F32,
    /// `+0x24` / `+0x28`.
    pub reissue: (F32, F32),
    /// `+0xA4` is non-zero (a request token: UNSURE, the sim has none).
    pub token: bool,
    /// `+0xA0`: the walker's own footprint node.
    pub own_node: Option<u32>,
    /// `+0x74`: the waypoint index (`0xFFFFFFFF` after a detour request).
    pub index: u32,
    /// `+0x50`: the number of waypoints.
    pub count: u32,
}

impl Follower {
    /// A follower with no state.
    pub fn new(mask: u32, radius: F32) -> Self {
        Follower {
            flags: 0,
            wait: 0,
            retry: 0,
            blocker: (u32::MAX, u32::MAX),
            mask,
            radius,
            reissue: (F32::ZERO, F32::ZERO),
            token: false,
            own_node: None,
            index: 0,
            count: 0,
        }
    }

    /// `0x6F49B3E0`: clears the waiting state.
    fn clear_wait(&mut self) {
        self.flags &= !FLAG_WAITING;
        self.retry = 0;
        self.wait = 0;
    }
}

/// What the walk step asks of the world besides the grid.
pub trait FollowHost {
    /// The follower of mover `m`.
    fn follower(&mut self, m: u32) -> &mut Follower;
    /// Mover `m + 0x10` is the mover tag.
    fn tag_ok(&self, m: u32) -> bool;
    /// `m + 0x80 / +0x84`: the velocity (cells per second).
    fn velocity(&self, m: u32) -> (F32, F32);
    /// `0x6F495940`: the unit of mover `m`, if its handle is valid.
    fn unit_of(&self, m: u32) -> Option<u32>;
    /// Unit `+0x80` bit 3.
    fn unit_flag8(&self, unit: u32) -> bool;
    /// Unit `+0x38`.
    fn unit_stamp(&self, unit: u32) -> u32;
    /// The handle of mover `m` (`+0x14`, `+0x18`).
    fn handle_of(&self, m: u32) -> (u32, u32);
    /// `0x6F03FA30`: whether the handle names a live mover.
    fn handle_valid(&self, h: (u32, u32)) -> bool;
    /// One step of the global generator.
    fn draw(&mut self) -> u32;
}

/// Walks between two requests of a kind (`dword_6F953340`): 0 = the cell-level detour search (`0x6F49BB90`), 1 = the
/// region path request (`0x6F49B9C0`), 2 = (`0x40`, unused here).
pub const REQUEST_INTERVAL: [u32; 3] = [10, 10, 0x40];

/// `0x6F49ACA0`: the per-follower request budget. `last` is the walk counter of the last grant (`P+0x7C + 4k`), `now`
/// the world's walk counter (`world+0x538`); returns the grant, and the elapsed walks.
pub fn request_gate(last: &mut u32, now: u32, interval: u32) -> (bool, u32) {
    if *last > now {
        *last = now.wrapping_sub(interval);
    }
    let elapsed = now.wrapping_sub(*last);
    if elapsed < interval {
        return (false, elapsed);
    }
    *last = now;
    (true, elapsed)
}

/// The trace list: `None` is a blocked cell or an occupant without the mover tag (a null entry).
type List = Vec<Option<u32>>;

fn push(list: &mut List, v: Option<u32>) {
    if list.len() < LIST_MAX {
        list.push(v);
    }
}

/// `0x6F4A1EA0`: visits one cell.
fn visit(
    occ: &mut Occupancy,
    extra: &[NodeExtra],
    mask: u32,
    host: &dyn FollowHost,
    list: &mut List,
    x: i32,
    y: i32,
) {
    let inside = (x as u32) < occ.pitch as u32 && (y as u32) < occ.rows as u32;
    if inside {
        let word = occ.cells[(occ.pitch * y + x) as usize];
        if (mask & word) & 0xFF00_0000 == 0 {
            let mut cur = word & 0x00FF_FFFF;
            if cur == NIL {
                return;
            }
            occ.stamp = occ.stamp.wrapping_add(1);
            let counter = occ.stamp;
            loop {
                let e = occ.pool[cur as usize];
                let next = e.w0 & 0x00FF_FFFF;
                let kind = e.w0 & 0xFF00_0000;
                if kind != KIND_STATIC {
                    let n = e.node as usize;
                    let nx = extra[n];
                    let st = occ.nodes[n].stamp;
                    if st != DEAD_STAMP && nx.flags & 0x0100_0000 != 0 && st != counter {
                        occ.nodes[n].stamp = counter;
                        if kind == KIND_UNIT && nx.word40 & 0x8FFF_FFFF == 0 && mask & nx.flags & 0x00FF_FFFF != 0 {
                            let owner = nx.owner.filter(|&o| host.tag_ok(o));
                            push(list, owner);
                        }
                    }
                }
                if next == NIL {
                    break;
                }
                cur = next;
            }
            return;
        }
    }
    push(list, None);
}

fn row(
    occ: &mut Occupancy,
    ex: &[NodeExtra],
    mask: u32,
    host: &dyn FollowHost,
    list: &mut List,
    x0: i32,
    y: i32,
    n: i32,
) {
    // 0x6F4A1FA0: n cells along x
    for x in x0..x0.wrapping_add(n) {
        visit(occ, ex, mask, host, list, x, y);
    }
}

fn col(
    occ: &mut Occupancy,
    ex: &[NodeExtra],
    mask: u32,
    host: &dyn FollowHost,
    list: &mut List,
    x: i32,
    y0: i32,
    n: i32,
) {
    // 0x6F4A1FE0: n cells along y
    for y in y0..y0.wrapping_add(n) {
        visit(occ, ex, mask, host, list, x, y);
    }
}

fn rect(
    occ: &mut Occupancy,
    ex: &[NodeExtra],
    mask: u32,
    host: &dyn FollowHost,
    list: &mut List,
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
) {
    // 0x6F4A2020: h rows of w cells
    for y in y0..y0.wrapping_add(h) {
        row(occ, ex, mask, host, list, x0, y, w);
    }
}

/// `0x6F49ABB0`: the direction code of a step from `start` to `end` (cells).
fn direction_code(start: (i32, i32), end: (i32, i32)) -> u32 {
    let mut r = 0;
    if end.0 < start.0 {
        r = 8;
    } else if end.0 > start.0 {
        r = 2;
    }
    if end.1 < start.1 {
        return r | 1;
    }
    if end.1 > start.1 {
        return r | 4;
    }
    r
}

/// The trace of the size class `class` around the end cell (`0x6F4A2070`, `0x6F4A2180`, `0x6F4A2360`, `0x6F4A2550`).
fn trace_class(
    occ: &mut Occupancy,
    ex: &[NodeExtra],
    mask: u32,
    host: &dyn FollowHost,
    list: &mut List,
    class: u8,
    end: (i32, i32),
    code: u32,
) {
    let (x, y) = end;
    macro_rules! v {
        ($x:expr, $y:expr) => {
            visit(occ, ex, mask, host, list, $x, $y)
        };
    }
    macro_rules! r {
        ($x:expr, $y:expr, $n:expr) => {
            row(occ, ex, mask, host, list, $x, $y, $n)
        };
    }
    macro_rules! c {
        ($x:expr, $y:expr, $n:expr) => {
            col(occ, ex, mask, host, list, $x, $y, $n)
        };
    }
    match class {
        0 => {
            v!(x, y);
            match code {
                3 => {
                    v!(x - 1, y);
                    v!(x, y + 1);
                }
                6 => {
                    v!(x - 1, y);
                    v!(x, y - 1);
                }
                9 => {
                    v!(x + 1, y);
                    v!(x, y + 1);
                }
                12 => {
                    v!(x + 1, y);
                    v!(x, y - 1);
                }
                _ => {}
            }
        }
        1 => match code {
            1 => r!(x - 1, y - 1, 2),
            2 => c!(x, y - 1, 2),
            3 => {
                r!(x - 2, y - 1, 3);
                c!(x, y, 2);
            }
            4 => r!(x - 1, y, 2),
            6 => {
                r!(x - 2, y, 3);
                c!(x, y - 2, 2);
            }
            8 => c!(x - 1, y - 1, 2),
            9 => {
                r!(x - 1, y - 1, 3);
                c!(x - 1, y, 2);
            }
            12 => {
                r!(x - 1, y, 3);
                c!(x - 1, y - 2, 2);
            }
            _ => rect(occ, ex, mask, host, list, x - 1, y - 1, 2, 2),
        },
        2 => match code {
            1 => r!(x - 1, y - 1, 3),
            2 => c!(x + 1, y - 1, 3),
            3 => {
                r!(x - 2, y - 1, 4);
                c!(x + 1, y, 3);
            }
            4 => r!(x - 1, y + 1, 3),
            6 => {
                r!(x - 2, y + 1, 4);
                c!(x + 1, y - 2, 3);
            }
            8 => c!(x - 1, y - 1, 3),
            9 => {
                r!(x - 1, y - 1, 4);
                c!(x - 1, y, 3);
            }
            12 => {
                r!(x - 1, y + 1, 4);
                c!(x - 1, y - 2, 3);
            }
            _ => rect(occ, ex, mask, host, list, x - 1, y - 1, 3, 3),
        },
        _ => match code {
            1 => r!(x - 2, y - 2, 4),
            2 => c!(x + 1, y - 2, 4),
            3 => {
                r!(x - 3, y - 2, 5);
                c!(x + 1, y - 1, 4);
            }
            4 => r!(x - 2, y + 1, 4),
            6 => {
                r!(x - 3, y + 1, 5);
                c!(x + 1, y - 3, 4);
            }
            8 => c!(x - 2, y - 2, 4),
            9 => {
                r!(x - 2, y - 2, 5);
                c!(x - 2, y - 1, 4);
            }
            12 => {
                r!(x - 2, y + 1, 5);
                c!(x - 2, y - 3, 4);
            }
            _ => rect(occ, ex, mask, host, list, x - 2, y - 2, 4, 4),
        },
    }
}

/// The squared speed of a velocity (`0x6F49D770`): `vx * vx + vy * vy` with the soft helpers.
fn speed2(v: (F32, F32)) -> F32 {
    arith::add(arith::mul(v.0, v.0), arith::mul(v.1, v.1))
}

/// `0x6F49B430`: `owner` waits for the mover `who` (`None` for nobody) for at least `wait` walks.
fn set_blocker(host: &mut dyn FollowHost, owner: u32, who: Option<u32>, wait: u32) {
    let h = who.map_or((u32::MAX, u32::MAX), |w| host.handle_of(w));
    let p = host.follower(owner);
    p.blocker = h;
    if p.wait < wait {
        p.wait = wait;
    }
}

/// `0x6F49D2F0`: whether the follower of `m` waits for a live mover.
fn waits_for_live(host: &dyn FollowHost, m: u32, p: &Follower) -> bool {
    let _ = m;
    host.handle_valid(p.blocker)
}

/// `0x6F49D770`: marks the first moving occupant as the blocker of `me`.
fn scan_blockers(host: &mut dyn FollowHost, me: u32, list: &List) {
    {
        let p = host.follower(me);
        p.blocker = (u32::MAX, u32::MAX);
    }
    let mine = speed2(host.velocity(me));
    let my_unit = host.unit_of(me);
    let my_nibble = (host.follower(me).flags >> 16) & 0xF;
    for entry in list {
        let Some(m) = *entry else {
            continue;
        };
        let Some(unit) = host.unit_of(m) else {
            continue;
        };
        let other = speed2(host.velocity(m));
        if arith::eq(other, F32::ZERO) {
            continue;
        }
        let op = *host.follower(m);
        if waits_for_live(host, m, &op) {
            continue;
        }
        let mark = if Some(unit) == my_unit && !my_unit.is_some_and(|u| host.unit_flag8(u)) {
            true
        } else if !(arith::lt(other, mine) || other.to_f32().is_nan() || mine.to_f32().is_nan()) {
            true
        } else {
            (op.flags >> 16) & 0xF != my_nibble
        };
        if mark {
            set_blocker(host, me, Some(m), 4);
            return;
        }
        set_blocker(host, m, Some(me), 20);
    }
}

/// `0x6F49D8D0`: `true` when the one-cell step from `pos` toward `wp` is free. Fills the blocker state of `me`.
pub fn step_is_clear(
    occ: &mut Occupancy,
    ex: &mut [NodeExtra],
    host: &mut dyn FollowHost,
    me: u32,
    pos: (F32, F32),
    wp: (F32, F32),
    t: &Tables,
) -> (bool, Vec<Option<u32>>) {
    let mut dx = arith::sub(wp.0, pos.0);
    let mut dy = arith::sub(wp.1, pos.1);
    // 0x6F49B050: the direction is shortened to one cell
    let len = math::sqrt(arith::add(arith::mul(dx, dx), arith::mul(dy, dy)), t);
    if arith::lt(ONE, len) {
        let inv = arith::recip(len, t);
        dx = arith::mul(dx, inv);
        dy = arith::mul(dy, inv);
    }
    let cell = |v: F32| conv::f2i(math::floor(v));
    let start = (cell(pos.0), cell(pos.1));
    let end = (cell(arith::add(pos.0, dx)), cell(arith::add(pos.1, dy)));
    let code = direction_code(start, end);
    let p = *host.follower(me);
    let mut list: List = Vec::new();
    let class = size_class(p.radius);
    // the walker's own footprint node is counted up for the length of the trace (`P+0xA0`, `node+0x40`), which makes
    // the trace skip it
    if let Some(n) = p.own_node {
        ex[n as usize].word40 = ex[n as usize].word40.wrapping_add(1);
    }
    trace_class(occ, ex, p.mask, host, &mut list, class, end, code);
    if let Some(n) = p.own_node {
        ex[n as usize].word40 = ex[n as usize].word40.wrapping_sub(1);
    }
    scan_blockers(host, me, &list);
    (list.is_empty(), list)
}

/// What a walk step answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepOut {
    /// The step's return code of `0x6F49DF00`: 0 free, 1 wait, 3 token blocked, 4 end of the path.
    pub code: u32,
    /// The trace list (for tests).
    pub list: Vec<Option<u32>>,
    /// The detour draw happened.
    pub drew: bool,
    /// The blocked branch `0x6F49DE90` ran: the caller's waypoint copy is restored to its old value.
    pub restored: bool,
}

/// `0x6F49DF00` for a waypoint that is not reached yet: the reach test, the trace and the blocked branch.
/// Returns `None` when the waypoint is reached (the caller advances the path: `0x6F49C3B0` reached branch).
pub fn walk_step(
    occ: &mut Occupancy,
    ex: &mut [NodeExtra],
    host: &mut dyn FollowHost,
    me: u32,
    pos: (F32, F32),
    wp: (F32, F32),
    t: &Tables,
) -> Option<StepOut> {
    // 0x6F49C3B0
    let d2 = arith::dist2(wp.0, wp.1, pos.0, pos.1);
    let c = arith::mul(REACH, REACH);
    if arith::le(d2, c) {
        return None;
    }
    host.follower(me).clear_wait();
    let (clear, list) = step_is_clear(occ, ex, host, me, pos, wp, t);
    if clear {
        return Some(StepOut { code: 0, list, drew: false, restored: false });
    }
    let p = *host.follower(me);
    if !host.handle_valid(p.blocker) && p.wait == 0 {
        // 0x6F49DE90
        if p.token {
            // 0x6F49C460: the request token test (UNSURE: the sim has no token)
        }
        let mut drew = false;
        if host.follower(me).retry == 0 {
            // 0x6F49B340
            let d = arith::dist2(p.reissue.0, p.reissue.1, pos.0, pos.1);
            let far = arith::lt(FAR2, d);
            let stamp = host.unit_of(me).map_or(0, |u| host.unit_stamp(u));
            if far || stamp > 1 {
                let r = host.draw();
                host.follower(me).retry = 7 + ((r >> 22) & 1);
                drew = true;
            } else {
                host.follower(me).retry = 2;
            }
        }
        let f = host.follower(me);
        if f.retry == 1 {
            return Some(StepOut { code: 4, list, drew, restored: true });
        }
        // 0x6F49DBE0(0, 0, 0, 0)
        f.flags &= !FLAG_WAITING;
        f.count = 0;
        f.index = u32::MAX;
        f.retry -= 1;
        return Some(StepOut { code: 1, list, drew, restored: true });
    }
    Some(StepOut { code: 1, list, drew: false, restored: false })
}
