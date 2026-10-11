//! Mover state of the game's units: continuous-time positions (`docs/spec/movement-6.md`).
//!
//! A unit that walks a route has a [`MoverRt`]: the velocity `V` in cells per second, the time snapshot of its last
//! re-base, the speed cap and the steering values of `crate::mover`. Its cells (`UnitObj::cell_x/cell_y`) are the
//! position **at the snapshot**; the position at the game time `t` is `cell + V * (t - snapshot)` (softfloat), which
//! is what `GetUnitX/Y` return. The cached world position `UnitObj::x/y` of a moving unit is refreshed whenever the
//! game time changes ([`Game::set_game_time`]) and after every walk.
//!
//! The clock layer is the game time with the snapshot rows fixed at zero. The original's layers are timer queues that
//! restart their float at the 300 s horizon (`0x6FAAE594`) and count a row; with a single growing clock the
//! elapsed time is the same difference, so only the rounding of very late times differs (UNSURE, not modelled).

use std::collections::BTreeMap;

use softfloat::{arith, F32, Tables};

use crate::heroes::GridCell;
use crate::mover::{self, Kin};
use crate::natives::game::{Game, ObjKey, Object};
use crate::unit_position::{cell_to_world, world_to_cell};

/// The queue horizon `300.0` (`0x6FAAE594`): the layer scale of `Kin::elapsed`.
pub(crate) const LAYER_HORIZON: F32 = F32::from_bits(0x4396_0000);

/// `0.49 * 0.49 = 0.2401`: the squared distance within which a route waypoint counts as reached (`0x6F49C3B0`).
pub(crate) fn waypoint_radius_sq() -> F32 {
    let r = F32::from_bits(0x3EFA_E148);
    arith::mul(r, r)
}

/// The mover state of one unit (the fields of `U` that a still unit does not need).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MoverRt {
    pub snap_t: F32,
    pub snap_row: i32,
    pub vx: F32,
    pub vy: F32,
    pub cap: F32,
    /// The acceleration step `U+0xB4`: the speed cap at the last move issue.
    pub accel: F32,
    /// The arrival radius `U+0xB0` (cells).
    pub radius: F32,
    pub moving: bool,
    pub last_speed: F32,
    pub face_error: F32,
}

impl MoverRt {
    fn new(now: F32) -> Self {
        MoverRt {
            snap_t: now,
            snap_row: 0,
            vx: F32::ZERO,
            vy: F32::ZERO,
            cap: F32::ZERO,
            accel: F32::ZERO,
            radius: mover::arrival_radius(F32::ZERO),
            moving: false,
            last_speed: F32::ZERO,
            face_error: F32::ZERO,
        }
    }

    /// Whether the unit is in motion (a non-zero velocity).
    pub fn in_motion(&self) -> bool {
        self.vx.to_bits() << 1 != 0 || self.vy.to_bits() << 1 != 0
    }
}

/// The movers of a game, keyed by unit.
pub(crate) type Movers = BTreeMap<ObjKey, MoverRt>;

impl Game {
    /// The game time the positions are evaluated at (the queue time of the running timer callback).
    pub fn game_time(&self) -> F32 {
        self.game_time
    }

    /// Sets the game time and refreshes the cached world position of every unit in motion. Called by the host
    /// before each timer callback and when a tick ends (`script.rs`).
    pub fn set_game_time(&mut self, t: F32) {
        if self.game_time == t {
            return;
        }
        self.game_time = t;
        let moving: Vec<ObjKey> = self
            .movers
            .iter()
            .filter(|(_, m)| m.in_motion())
            .map(|(&k, _)| k)
            .collect();
        for k in moving {
            self.refresh_cached_position(k);
        }
    }

    /// The clock layer positions are evaluated against.
    pub(crate) fn layer(&self) -> GridCell {
        GridCell {
            origin: self.game_time,
            row: 0,
            scale: LAYER_HORIZON,
        }
    }

    /// The mover of `unit` as a [`Kin`] (a unit without a mover is still, with its snapshot at the current time).
    pub(crate) fn kin_of(&self, unit: ObjKey) -> Kin {
        let u = self.unit(unit);
        let rt = self
            .movers
            .get(&unit)
            .copied()
            .unwrap_or_else(|| MoverRt::new(self.game_time));
        Kin {
            snap_t: rt.snap_t,
            snap_row: rt.snap_row,
            cx: u.cell_x,
            cy: u.cell_y,
            vx: rt.vx,
            vy: rt.vy,
            cap: rt.cap,
            moving: rt.moving,
            facing: u.facing,
            last_speed: rt.last_speed,
            face_error: rt.face_error,
        }
    }

    /// Writes a [`Kin`] back to the unit and its mover, and refreshes the cached world position.
    pub(crate) fn commit_kin(&mut self, unit: ObjKey, kin: &Kin) {
        let now = self.game_time;
        let rt = self.movers.entry(unit).or_insert_with(|| MoverRt::new(now));
        rt.snap_t = kin.snap_t;
        rt.snap_row = kin.snap_row;
        rt.vx = kin.vx;
        rt.vy = kin.vy;
        rt.cap = kin.cap;
        rt.moving = kin.moving;
        rt.last_speed = kin.last_speed;
        rt.face_error = kin.face_error;
        let u = self.unit_mut(unit);
        u.cell_x = kin.cx;
        u.cell_y = kin.cy;
        u.facing = kin.facing;
        self.refresh_cached_position(unit);
    }

    /// Recomputes the cached world position of `unit` from its mover at the current game time (the normaliser
    /// `0x6F4743A0` with the cell term, `unit-position.md`).
    pub(crate) fn refresh_cached_position(&mut self, unit: ObjKey) {
        if !matches!(self.object(unit), Object::Unit(_)) {
            return;
        }
        let g = self.layer();
        let kin = self.kin_of(unit);
        let (fx, fy) = kin.position(&g);
        let (ox, oy) = self.world_origin();
        let u = self.unit_mut(unit);
        u.x = cell_to_world(fx, ox);
        u.y = cell_to_world(fy, oy);
    }

    /// The move issue of `0x6F474D40`: the speed cap and the acceleration step become `min(speed, bound) / 32`, the
    /// velocity is zeroed (re-based), and the arrival radius is set. `radius` is the radius argument of the issue
    /// (world units; zero for a plain move, UNSURE).
    pub(crate) fn mover_issue(&mut self, unit: ObjKey, radius: F32, t: &Tables) {
        let g = self.layer();
        let bound = mover::speed_bound(t);
        let speed = self.unit(unit).move_speed;
        let mut kin = self.kin_of(unit);
        let cap = mover::speed_to_cap(speed, bound);
        kin.set_cap(&g, cap, t);
        kin.set_velocity(&g, (F32::ZERO, F32::ZERO), t);
        self.commit_kin(unit, &kin);
        let rt = self.movers.get_mut(&unit).expect("committed above");
        rt.accel = cap;
        rt.radius = mover::arrival_radius(radius);
        rt.last_speed = F32::ZERO;
        rt.face_error = F32::ZERO;
    }

    /// Stops the unit where it is: the position is re-based at the current time and the velocity becomes zero (the
    /// stop of `0x6F4956C0`). Does nothing for a unit that is not in motion.
    pub(crate) fn mover_halt(&mut self, unit: ObjKey) {
        if !self.movers.get(&unit).is_some_and(|m| m.in_motion()) {
            return;
        }
        let Some(t) = self.orders.tables.take() else {
            return;
        };
        let g = self.layer();
        let mut kin = self.kin_of(unit);
        kin.set_velocity(&g, (F32::ZERO, F32::ZERO), &t);
        self.orders.tables = Some(t);
        self.commit_kin(unit, &kin);
    }

    /// `SetUnitMoveSpeed` on a unit with a mover: the speed sink `0x6F4747B0` sets the cap (a lower cap rescales the
    /// velocity). The acceleration step keeps its value from the last move issue.
    pub fn mover_speed_changed(&mut self, unit: ObjKey) {
        if !self.movers.contains_key(&unit) {
            return;
        }
        let Some(t) = self.orders.tables.take() else {
            return;
        };
        let g = self.layer();
        let cap = mover::speed_to_cap(self.unit(unit).move_speed, mover::speed_bound(&t));
        let mut kin = self.kin_of(unit);
        kin.set_cap(&g, cap, &t);
        self.orders.tables = Some(t);
        self.commit_kin(unit, &kin);
    }

    /// The pilot values of `unit` (turn cap, propulsion window, acceleration, arrival radius).
    pub(crate) fn pilot_of(&self, unit: ObjKey) -> mover::Pilot {
        let u = self.unit(unit);
        let rt = self
            .movers
            .get(&unit)
            .copied()
            .unwrap_or_else(|| MoverRt::new(self.game_time));
        mover::Pilot {
            steer: mover::SteerParams {
                accel: rt.accel,
                turn_cap: mover::turn_cap(u.turn_speed),
                window: mover::prop_window(u.prop_window),
            },
            radius: rt.radius,
        }
    }

    /// A world point in cells (the setter's arithmetic, `unit_position::world_to_cell`).
    pub(crate) fn cells_of(&self, p: (F32, F32)) -> (F32, F32) {
        let (ox, oy) = self.world_origin();
        (world_to_cell(p.0, ox), world_to_cell(p.1, oy))
    }
}
