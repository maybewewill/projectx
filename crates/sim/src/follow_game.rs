//! The game side of the path follower's walk step (`crate::follow`, `docs/spec/movement-8.md`).
//!
//! Every ground unit has a **footprint node** in the pathing grid's occupant lists (`0x6F4A6E40`): the cells
//! `floor(pos - lo) .. floor(pos + hi) + 1` with `(lo, hi) = (0, 0), (1, 0), (1, 1), (2, 1)` cells for the footprint
//! classes 0 to 3 (the class of the radius, `0x6F38ADA0`; the trace shapes of `0x6F4A2070/2180/2360/2550` are the same
//! boxes). A walking unit traces the cells of its next one-cell step before it moves; a blocked step stops it for a
//! walk, and a step blocked by something that is not a moving unit makes the follower draw its detour interval from
//! the global generator.
//!
//! UNSURE (flagged where it is decided): the footprint radius (`P+0xB4`, here collision / 32), the node flags and
//! the mask (ground = `0x01000001`), when a node is moved (here: at the walk of its unit, idle units at the start of
//! each walk), the detour search `0x6F49BB90` -> `0x6F4A3CC0` (stand-in: the region planner toward the waypoint), the
//! request token `P+0xA4` (none) and the unit stamp `unit+0x38` (always above 1), static structures (not in the grid).

use std::collections::BTreeMap;

use softfloat::{F32, Tables};

use crate::follow::{
    request_gate, size_class, walk_step, FollowHost, Follower, NodeExtra, REQUEST_INTERVAL,
};
use crate::mover::{self, PathState};
use crate::natives::game::{Game, ObjKey, Object};
use crate::occupancy::{Occupancy, NIL};
use crate::pathing::grid::scale_by_1_32;

/// Node flags of a ground unit: bit 24 (counts) and the type-0 mask bits (`0x6FA84E84` = 0x06000006, LIKELY).
const GROUND_FLAGS: u32 = 0x0100_0006;
/// The follower mask of a ground unit: the path-type-0 mask of the cell test (`pathing-build.md` 3.2).
const GROUND_MASK: u32 = 0x0600_0006;
const MOVE_FLY: u32 = 0x2;

/// The follower state of a game.
#[derive(Debug, Default)]
pub struct FollowState {
    occ: Option<Occupancy>,
    extra: Vec<NodeExtra>,
    /// Unit -> footprint node.
    nodes: BTreeMap<ObjKey, u32>,
    /// Units that were looked at (a unit gets a node once, or none).
    seen: BTreeMap<ObjKey, bool>,
    fol: BTreeMap<ObjKey, Follower>,
    /// `P+0x7C`: the walk counter of the last detour grant.
    last_grant: BTreeMap<ObjKey, u32>,
    /// `world+0x538`: counts walks.
    walks: u32,
    /// Diagnostics: detour draws made, blocked steps seen.
    pub draws: u32,
    pub blocked_steps: u32,
}

impl FollowState {
    /// The detour draws made so far.
    pub fn draws(&self) -> u32 {
        self.draws
    }
}

struct Host<'a> {
    game: &'a mut Game,
    fol: &'a mut BTreeMap<ObjKey, Follower>,
    draws: &'a mut u32,
}

impl Host<'_> {
    fn live(&self, m: u32) -> bool {
        if m as usize >= self.game.object_count() {
            return false;
        }
        match self.game.object(ObjKey(m)) {
            Object::Unit(u) => !(u.dead || u.removed),
            _ => false,
        }
    }
}

impl FollowHost for Host<'_> {
    fn follower(&mut self, m: u32) -> &mut Follower {
        self.fol.entry(ObjKey(m)).or_insert_with(|| Follower::new(GROUND_MASK, F32::ZERO))
    }
    fn tag_ok(&self, m: u32) -> bool {
        self.live(m)
    }
    fn velocity(&self, m: u32) -> (F32, F32) {
        self.game.movers.get(&ObjKey(m)).map_or((F32::ZERO, F32::ZERO), |r| (r.vx, r.vy))
    }
    fn unit_of(&self, m: u32) -> Option<u32> {
        self.live(m).then_some(m)
    }
    fn unit_flag8(&self, _unit: u32) -> bool {
        false
    }
    fn unit_stamp(&self, _unit: u32) -> u32 {
        // unit+0x38 is the group-query stamp (UNSURE: above 1 after the first range query)
        2
    }
    fn handle_of(&self, m: u32) -> (u32, u32) {
        (m, 1)
    }
    fn handle_valid(&self, h: (u32, u32)) -> bool {
        h.0 != u32::MAX && self.live(h.0)
    }
    fn draw(&mut self) -> u32 {
        *self.draws += 1;
        match self.game.rng_parts() {
            Some((table, streams)) => {
                crate::rng::trace::note(|| "follow detour".into());
                streams.global.next_u32(table)
            }
            None => 0,
        }
    }
}

/// The footprint box of a class around a position (cells): `[y0, x0, y1, x1)`.
pub fn footprint_box(class: u8, x: F32, y: F32) -> [i32; 4] {
    use softfloat::{arith, conv, math};
    let (lo, hi) = match class {
        0 => (0.0f32, 0.0f32),
        1 => (1.0, 0.0),
        2 => (1.0, 1.0),
        _ => (2.0, 1.0),
    };
    let f = |v: f32| F32(v.to_bits());
    let cell = |v: F32| conv::f2i(math::floor(v));
    let (xl, yl) = (arith::sub(x, f(lo)), arith::sub(y, f(lo)));
    let (xh, yh) = (arith::add(x, f(hi)), arith::add(y, f(hi)));
    [cell(yl), cell(xl), cell(yh).wrapping_add(1), cell(xh).wrapping_add(1)]
}

impl Game {
    /// Turns the follower on for a pathing grid: `words` are the cell words (pathing type bits in the top byte).
    pub fn enable_follow(&mut self, width: i32, height: i32, words: &[u32]) {
        let mut occ = Occupancy::new(width, height);
        for (i, w) in words.iter().enumerate().take(occ.cells.len()) {
            occ.cells[i] = (w & 0xFF00_0000) | NIL;
        }
        self.follow.occ = Some(occ);
    }

    /// Whether the follower is on.
    pub fn follow_enabled(&self) -> bool {
        self.follow.occ.is_some()
    }

    /// The follower diagnostics (draws, blocked steps).
    pub fn follow_stats(&self) -> (u32, u32) {
        (self.follow.draws, self.follow.blocked_steps)
    }

    fn follow_radius(&self, type_id: u32) -> F32 {
        let c = self.unit_types().get(type_id).map_or(F32::ZERO, |t| t.collision);
        let r = scale_by_1_32(c);
        match std::env::var("SIM_FOLLOW_RADIUS_DIV").ok().and_then(|s| s.parse::<f32>().ok()) {
            Some(d) => F32((r.to_f32() / d).to_bits()),
            None => r,
        }
    }

    /// Whether a unit takes part in the ground footprint grid.
    fn follow_ground(&self, key: ObjKey) -> bool {
        let Object::Unit(u) = self.object(key) else {
            return false;
        };
        if u.dead || u.removed {
            return false;
        }
        if self.unit_types().is_building(u.type_id) {
            return false;
        }
        !self.unit_types().get(u.type_id).is_some_and(|t| t.movement & MOVE_FLY != 0)
    }

    /// Registers a unit not seen before (a node for a live ground unit).
    fn follow_register(&mut self, occ: &mut Occupancy, key: ObjKey) {
        if self.follow.seen.contains_key(&key) {
            return;
        }
        let ground = self.follow_ground(key);
        self.follow.seen.insert(key, ground);
        if !ground {
            return;
        }
        let node = occ.add_node();
        self.follow.extra.push(NodeExtra { flags: std::env::var("SIM_FOLLOW_FLAGS").ok().and_then(|s| u32::from_str_radix(&s, 16).ok()).unwrap_or(GROUND_FLAGS), word40: 0, owner: Some(key.0) });
        debug_assert_eq!(node as usize + 1, self.follow.extra.len());
        self.follow.nodes.insert(key, node);
        let radius = {
            let Object::Unit(u) = self.object(key) else { return };
            self.follow_radius(u.type_id)
        };
        let mut f = Follower::new(GROUND_MASK, radius);
        f.own_node = Some(node);
        self.follow.fol.insert(key, f);
    }

    /// Moves the footprint node of a unit to its position at the game time.
    fn follow_update_node(&mut self, occ: &mut Occupancy, key: ObjKey) {
        let Some(&node) = self.follow.nodes.get(&key) else {
            return;
        };
        if !self.follow_ground(key) {
            occ.kill_node(node);
            self.follow.nodes.remove(&key);
            self.follow.fol.remove(&key);
            return;
        }
        let g = self.layer();
        let (x, y) = self.kin_of(key).position(&g);
        let class = self.follow.fol.get(&key).map_or(0, |f| size_class(f.radius));
        occ.move_footprint(node, footprint_box(class, x, y));
    }

    /// The pass before the unit walks of one movement walk: new units get nodes, idle units' nodes follow their
    /// positions, dirty cells are cleaned.
    pub(crate) fn follow_begin_walk(&mut self) {
        let Some(mut occ) = self.follow.occ.take() else {
            return;
        };
        self.follow.walks = self.follow.walks.wrapping_add(1);
        for i in 0..self.object_count() {
            let key = ObjKey(i as u32);
            if !matches!(self.object(key), Object::Unit(_)) {
                continue;
            }
            self.follow_register(&mut occ, key);
            // units with a route update their node at their own walk
            if self.follow.nodes.contains_key(&key) && !(self.route_active(key) && self.follow_ground(key)) {
                self.follow_update_node(&mut occ, key);
            }
        }
        occ.clean_dirty();
        self.follow.occ = Some(occ);
    }

    /// A route was issued: the follower starts clean.
    pub(crate) fn follow_reset(&mut self, unit: ObjKey) {
        if let Some(f) = self.follow.fol.get_mut(&unit) {
            f.wait = 0;
            f.retry = 0;
            f.blocker = (u32::MAX, u32::MAX);
            f.flags &= !crate::follow::FLAG_WAITING;
            f.index = 0;
        }
        self.follow.last_grant.remove(&unit);
    }

    /// The follower's answer for one walk of `unit` toward `wp` (cells): `Open(wp)` or `Blocked`. `detour` is set when
    /// the follower drops its path and needs a new one; the caller then re-plans through [`Game::follow_detour_plan`].
    pub(crate) fn follow_path_state(
        &mut self,
        unit: ObjKey,
        pos: (F32, F32),
        wp: (F32, F32),
        t: &Tables,
    ) -> PathState {
        let Some(mut occ) = self.follow.occ.take() else {
            return PathState::Open(wp);
        };
        let mut extra = std::mem::take(&mut self.follow.extra);
        let mut fol = std::mem::take(&mut self.follow.fol);
        let mut draws = 0u32;
        let state = 'step: {
            let Some(p) = fol.get_mut(&unit) else {
                break 'step PathState::Open(wp);
            };
            // 0x6F49E020: a waiting follower counts down and reports "blocked"
            if p.wait != 0 {
                p.wait -= 1;
                break 'step PathState::Blocked;
            }
            let p_radius = p.radius;
            let mut host = Host { game: self, fol: &mut fol, draws: &mut draws };
            match walk_step(&mut occ, &mut extra, &mut host, unit.0, pos, wp, t) {
                None => PathState::Open(wp),
                Some(out) if out.code == 0 => PathState::Open(wp),
                Some(out) => {
                    if std::env::var_os("SIM_FOLLOW_DEBUG").is_some() && out.drew {
                        eprintln!(
                            "  list {:?} code {} radius {} class {}",
                            out.list,
                            out.code,
                            p_radius.to_f32(),
                            size_class(p_radius)
                        );
                        let (cx, cy) = (
                            softfloat::conv::f2i(softfloat::math::floor(pos.0)),
                            softfloat::conv::f2i(softfloat::math::floor(pos.1)),
                        );
                        for dy in -3..=3 {
                            let mut s = String::new();
                            for dx in -3..=3 {
                                let (x, y) = (cx + dx, cy + dy);
                                let w = if x >= 0 && y >= 0 && x < occ.pitch && y < occ.rows {
                                    occ.cells[(occ.pitch * y + x) as usize] >> 24
                                } else {
                                    0xEE
                                };
                                s.push_str(&format!("{w:02x} "));
                            }
                            eprintln!("    {s}");
                        }
                    }
                    PathState::Blocked
                }
            }
        };
        self.follow.draws += draws;
        if std::env::var_os("SIM_FOLLOW_DEBUG").is_some() && draws > 0 {
            let ty = match self.object(unit) {
                Object::Unit(u) => u.type_id,
                _ => 0,
            };
            eprintln!(
                "follow draw: unit {} type {:08x} t {:.2} pos ({:.2},{:.2}) wp ({:.2},{:.2})",
                unit.0,
                ty,
                self.game_time().to_f32(),
                pos.0.to_f32(),
                pos.1.to_f32(),
                wp.0.to_f32(),
                wp.1.to_f32()
            );
        }
        if matches!(state, PathState::Blocked) {
            self.follow.blocked_steps += 1;
        }
        self.follow.occ = Some(occ);
        self.follow.extra = extra;
        self.follow.fol = fol;
        state
    }

    /// Whether the follower of `unit` dropped its path (index -1) and wants a detour.
    pub(crate) fn follow_wants_detour(&self, unit: ObjKey) -> bool {
        self.follow.fol.get(&unit).is_some_and(|f| f.index == u32::MAX)
    }

    /// The detour request gate (`0x6F49BB90`, `0x6F49ACA0` kind 0): `true` when the unit may ask this walk.
    pub(crate) fn follow_detour_granted(&mut self, unit: ObjKey) -> bool {
        let now = self.follow.walks;
        let last = self.follow.last_grant.entry(unit).or_insert(0);
        let (ok, _) = request_gate(last, now, REQUEST_INTERVAL[0]);
        if ok {
            if let Some(f) = self.follow.fol.get_mut(&unit) {
                f.index = 0;
            }
        }
        ok
    }

    /// The detour of `unit` from `pos` toward `goal` (cells): the cells of the local search that stands in for
    /// `0x6F4A3CC0`, as cell-centre positions, or `None` when the search found no way off the spot.
    pub(crate) fn follow_detour(&mut self, unit: ObjKey, pos: (F32, F32), goal: (F32, F32)) -> Option<Vec<(F32, F32)>> {
        use softfloat::{conv, math};
        let mut occ = self.follow.occ.take()?;
        let mut extra = std::mem::take(&mut self.follow.extra);
        let mut fol = std::mem::take(&mut self.follow.fol);
        let mut draws = 0u32;
        let cell = |v: F32| conv::f2i(math::floor(v));
        let from = (cell(pos.0), cell(pos.1));
        let to = (cell(goal.0), cell(goal.1));
        let path = {
            let mut host = Host { game: self, fol: &mut fol, draws: &mut draws };
            crate::follow::find_detour(&mut occ, &mut extra, &mut host, unit.0, from, to, 400)
        };
        self.follow.occ = Some(occ);
        self.follow.extra = extra;
        self.follow.fol = fol;
        path.map(|cells| {
            cells
                .into_iter()
                .map(|(x, y)| (F32((x as f32 + 0.5).to_bits()), F32((y as f32 + 0.5).to_bits())))
                .collect()
        })
    }

    /// Updates the footprint node of a walking unit after its walk.
    pub(crate) fn follow_after_walk(&mut self, unit: ObjKey) {
        let Some(mut occ) = self.follow.occ.take() else {
            return;
        };
        self.follow_update_node(&mut occ, unit);
        self.follow.occ = Some(occ);
    }
}

#[allow(dead_code)]
fn _uses(_: &mover::Pilot) {}
