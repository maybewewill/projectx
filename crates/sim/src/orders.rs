//! The order system: per-unit order lists, the dispatcher, the ability lookup, the move sub-task and the
//! per-step mover. Spec: `docs/spec/orders.md`, `orders-2.md`, `movement*.md`, `pathing-2.md`.
//!
//! Labels as elsewhere. CONFIRMED = read in the spec's disassembly notes; LIKELY = inferred from the
//! spec's structure; UNVERIFIED = a model choice that the spec does not settle (marked where used).
//!
//! What is modelled:
//!
//! * Per-unit lists (CONFIRMED names, LIKELY structure). `list` holds the started head and the orders
//!   queued behind it; `pending` holds orders accepted while a start is running (never reached by the
//!   scripted natives, which cannot re-enter a start). The total count (`+0x1B4`) is `list + pending`.
//!   An order is refused when the total is already above [`ORDER_LIMIT`] (CONFIRMED: 500, so 501 orders
//!   at most). A locked unit (`+0x5C` bit `0x100`, host-set here) refuses every order.
//! * Dispatcher (CONFIRMED structure): `replace = true` clears everything (the stop notification `0xD0144`
//!   when there was something to clear, then `0xD02A6` for each cleared order) and then enqueues.
//!   `replace = false` appends behind the current order. An order that starts runs its class handler and
//!   the ability's start, then fires `0xD02A6`.
//! * Ability lookup (CONFIRMED values, spec orders-2 section 1.8): an ability accepts an id by its family;
//!   among the accepting abilities the lowest rank wins, the first one on a tie. Ranks: move and attack 1,
//!   `0xD0002` 3, smart 16, `0xD00BD` 13, anything else 0.
//!   Families: the move family (CONFIRMED from the slot-192 check of CAbilityMove) is `0xD0002`, `0xD0003`,
//!   `0xD000F`, `0xD0012`, `0xD0014`, `0xD0016`, `0xD0017`; for immediate orders the move ability also takes
//!   hold position (`0xD0019`, the slot-135 accept list). The attack family is `{0xD000F}` (UNVERIFIED: the
//!   attack family set was not read).
//! * Movement (see `docs/spec/movement-6.md`, CONFIRMED kernels in [`crate::mover`]). A point order with a move or
//!   attack ability plans a route through the pathing search when the host set pathing data ([`PathingMap`]);
//!   without it, or when the search start is outside the pyramid, the route is the straight line to the goal.
//!   The move issue gives the unit's mover its speed cap and a zero velocity; every movement walk (the repeating
//!   timer of message 0x8026A, period 0.03 s of agile clock, lockstep.md section 11.2; [`Game::tick_movement`])
//!   steers the mover with the original's walk (`mover::walk`: acceleration, turn limit, arrival test). The
//!   position is **evaluated** from the velocity and the game time, not stepped (`crate::movement`). A route
//!   waypoint counts as reached inside 0.49 cells (LIKELY). Without tables (the softfloat division, square root
//!   and arccosine need them) the mover does not move anything.
//!
//! Attack orders of a unit with the attack ability are run by the attack cycle (`combat.rs`): it decides the
//! chase and the swing, and completes the order. Other target orders complete at once (no target following).
//! Not modelled here: build orders, patrol return, stop-by-owner details, the item drop orders (they stay in
//! the item model, `natives/item.rs`), and the path smoothing and separation of the original.

use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, Tables, F32};

use crate::abilities::{Ability, BaseKind};
use crate::mover::{self, PathState};
use crate::natives::game::{Game, ObjKey, Object};
use crate::pathing::grid::scale_by_1_32;
use crate::pathing::search::{Request, Search, POP_BUDGET};

/// Maximum number of orders a unit holds (CONFIRMED: refused when the count is above 500).
pub const ORDER_LIMIT: u32 = 500;

/// `OrderId("stop")` (CONFIRMED, orders.md section 1).
pub const ORDER_STOP: u32 = 0x000D_0004;
/// `OrderId("smart")`.
pub const ORDER_SMART: u32 = 0x000D_0003;
/// `OrderId("move")`.
pub const ORDER_MOVE: u32 = 0x000D_0012;
/// `OrderId("attack")`.
pub const ORDER_ATTACK: u32 = 0x000D_000F;
/// `OrderId("AImove")` (orders.md section 1; the correction against pathing-2 section 6.2).
pub const ORDER_AIMOVE: u32 = 0x000D_0014;
/// `OrderId("patrol")`.
pub const ORDER_PATROL: u32 = 0x000D_0016;
/// `OrderId("holdposition")`.
pub const ORDER_HOLD: u32 = 0x000D_0019;
/// The id the immediate validator always accepts (orders.md 4.1).
pub const ORDER_ALWAYS_ACCEPTED: u32 = 0x000D_0005;

/// Internal notification: stop (`0xD0144`, orders.md 6.3).
pub const NOTE_STOP: u32 = 0x000D_0144;
/// Internal notification: queued (`0xD02A5`).
pub const NOTE_QUEUED: u32 = 0x000D_02A5;
/// Internal notification: started or cleared (`0xD02A6`).
pub const NOTE_STARTED_OR_CLEARED: u32 = 0x000D_02A6;

mod names;
pub use names::NAMED_ORDERS;

/// Original ASCII names match without case: Storm hash ordinal 590 folds case and ordinal 509 calls `_strnicmp`.
pub fn order_id_by_name(name: &str) -> Option<u32> {
    NAMED_ORDERS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|&(_, id)| id)
}

/// The name of an order id (`OrderId2String`), if it has one.
pub fn order_name(id: u32) -> Option<&'static str> {
    NAMED_ORDERS
        .iter()
        .find(|&&(_, i)| i == id)
        .map(|&(n, _)| n)
}

/// The order classes (orders.md section 0 and orders-2 C1). The class decides the event kind and the
/// argument checks, not the validator (orders-2 1.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderClass {
    /// No target: the immediate class.
    Immediate,
    /// A point: `IssuePointOrder*`.
    Point,
    /// A widget target: `IssueTargetOrder*`. Also the class of a zero-flag id (orders-2 C1).
    Target,
}

/// The abilities a unit type can have (only the two the order model uses). A type lists them in its order data;
/// creating a unit attaches the matching ability objects (`Amov` / `Aatk`, see `crate::abilities`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbilityKind {
    Move,
    Attack,
}

impl AbilityKind {
    /// The base code of the ability object for this kind.
    pub fn base_code(self) -> u32 {
        match self {
            AbilityKind::Move => crate::abilities::MOVE,
            AbilityKind::Attack => crate::abilities::ATTACK,
        }
    }
}

/// What the order model needs from a unit type: its abilities. The movement speed is the unit's own
/// `move_speed` field (see `UnitObj::move_speed`), so there is one source of truth for it.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeOrderData {
    pub abilities: Vec<AbilityKind>,
}

/// One order object (the fields the model uses).
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub id: u32,
    pub class: OrderClass,
    /// The point of a point order (`None` for an immediate or target order without one).
    pub point: Option<(F32, F32)>,
    /// The handle id of the target widget, or 0.
    pub target: u32,
}

/// An order that a native issued and the unit accepted. `GetIssuedOrderId` and the other event getters
/// read it through the trigger thread's context (`Ctx::Trigger { order }`).
#[derive(Clone, Debug, PartialEq)]
pub struct IssuedOrder {
    pub unit: ObjKey,
    pub order: Order,
}

/// The pathing data of a map: the region search over its pyramid and the world origin of its cell grid.
/// Cells are 32 world units (CONFIRMED), so a world point `p` is cell `(p - origin) / 32`.
#[derive(Clone, Debug)]
pub struct PathingMap {
    pub search: Search,
    pub origin: (F32, F32),
}

/// One entry of a unit's list.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    order: Order,
    /// Set when the order has started (the head of the list is the active order).
    started: bool,
}

/// A planned route: world points to walk to in order (start side first). `next` is the index of the
/// waypoint being walked to.
#[derive(Clone, Debug, PartialEq)]
struct Route {
    points: Vec<(F32, F32)>,
    next: usize,
}

/// The order state of one unit.
#[derive(Clone, Debug, Default, PartialEq)]
struct UnitOrders {
    list: Vec<Entry>,
    pending: Vec<Order>,
    route: Option<Route>,
    /// Set while a start runs. Orders dispatched meanwhile go to `pending`.
    starting: bool,
}

/// The order subsystem of a game.
#[derive(Clone, Debug, Default)]
pub struct OrderSystem {
    types: BTreeMap<u32, TypeOrderData>,
    units: BTreeMap<ObjKey, UnitOrders>,
    locked: BTreeSet<ObjKey>,
    issued: Vec<IssuedOrder>,
    notices: Vec<(ObjKey, u32)>,
    pathing: Option<PathingMap>,
    pub(crate) tables: Option<Tables>,
}

/// Softfloat constants used by the order system (bit patterns).
const CELL_SIZE: F32 = F32::from_bits(0x4200_0000);

/// Whether ability `kind` takes the point or target order `id` by its family (CONFIRMED for the move family).
fn point_family(kind: BaseKind, id: u32) -> bool {
    match kind {
        BaseKind::Move => matches!(
            id,
            0x000D_0002
                | 0x000D_0003
                | 0x000D_000F
                | 0x000D_0012
                | 0x000D_0014
                | 0x000D_0016
                | 0x000D_0017
        ),
        BaseKind::Attack => id == ORDER_ATTACK,
        BaseKind::Build => is_build_order_id(id),
        _ => false,
    }
}

/// Whether `id` is a build order id: the order id of a build order is the raw code of the unit type to place
/// (`IssueBuildOrderById`, `0x6F3C8BE0` passes the type id as the order id). The internal orders all live in the
/// `0x000Dxxxx` range, so a raw code has a non-zero top byte. UNSURE: the original's build ability checks the id
/// against the unit's build list (`natives/lane_f.rs` notes); the model accepts any raw code.
pub fn is_build_order_id(id: u32) -> bool {
    id >> 24 != 0
}

/// Whether ability `kind` accepts the immediate order `id` (stop is handled before this check).
fn immediate_family(kind: BaseKind, id: u32) -> bool {
    kind == BaseKind::Move && id == ORDER_HOLD
}

/// Rank of an ability for an order id (lower wins, orders-2 1.8). The table is per order id, as recorded: move and
/// attack abilities use it. Abilities without an order family (`BaseKind` other than move or attack) rank 0.
fn rank(kind: BaseKind, id: u32) -> u32 {
    if !matches!(kind, BaseKind::Move | BaseKind::Attack) {
        return 0;
    }
    match id {
        0x000D_0002 => 3,
        0x000D_0003 => 16,
        0x000D_000F | 0x000D_0012 => 1,
        0x000D_00BD => 13,
        _ => 0,
    }
}

/// Whether a point order id is point-capable (flags bit `0x2`). UNVERIFIED table: the flag word of the
/// original is not read; these are the ids that take a point in the spec (move, smart, attack, AImove,
/// patrol). Other ids have flags 0 and build the target class (orders-2 C1).
pub fn point_capable(id: u32) -> bool {
    matches!(
        id,
        ORDER_MOVE | ORDER_SMART | ORDER_ATTACK | ORDER_AIMOVE | ORDER_PATROL
    )
}

impl OrderSystem {
    /// An empty order system with no pathing data and no tables.
    pub fn new() -> Self {
        Self::default()
    }

    /// The order data of a unit type (`None` for an unknown type: the units have no abilities and no speed).
    pub fn type_data(&self, type_id: u32) -> Option<&TypeOrderData> {
        self.types.get(&type_id)
    }

    /// The accepted-order records, oldest first. The index of a record is the `order` of its trigger context.
    pub fn issued(&self) -> &[IssuedOrder] {
        &self.issued
    }

    /// The internal notifications fired so far, `(unit, id)`, oldest first.
    pub fn notices(&self) -> &[(ObjKey, u32)] {
        &self.notices
    }

    /// The ability lookup of the order system: among the unit's ability objects, the lowest-rank one whose family
    /// accepts the order wins, the first one on a tie (`FUN_6F279A90`, orders-2 1.8). `immediate` selects the
    /// immediate family. Returns the kind of the winning ability.
    fn ability_for(&self, abilities: &[Ability], id: u32, immediate: bool) -> Option<BaseKind> {
        let mut best: Option<(u32, BaseKind)> = None;
        for ability in abilities {
            let kind = ability.kind;
            let ok = if immediate {
                immediate_family(kind, id)
            } else {
                point_family(kind, id)
            };
            if !ok {
                continue;
            }
            let r = rank(kind, id);
            if best.is_none_or(|(br, _)| r < br) {
                best = Some((r, kind));
            }
        }
        best.map(|(_, k)| k)
    }

    fn total(&self, unit: ObjKey) -> u32 {
        self.units
            .get(&unit)
            .map_or(0, |u| (u.list.len() + u.pending.len()) as u32)
    }
}

impl Game {
    /// Sets the softfloat tables the mover needs (from the user's `Game.dll`). Without them no unit moves.
    pub fn set_tables(&mut self, tables: Option<Tables>) {
        self.orders.tables = tables;
    }

    /// Sets (or clears) the pathing data used to plan routes.
    pub fn set_pathing(&mut self, pathing: Option<PathingMap>) {
        self.orders.pathing = pathing;
    }

    /// Sets the order data of a unit type: move speed (world units per second) and abilities.
    pub fn set_type_orders(&mut self, type_id: u32, data: TypeOrderData) {
        self.orders.types.insert(type_id, data);
    }

    /// Locks or unlocks a unit (`+0x5C` bit `0x100`). A locked unit refuses every order.
    pub fn set_unit_locked(&mut self, unit: ObjKey, locked: bool) {
        if locked {
            self.orders.locked.insert(unit);
        } else {
            self.orders.locked.remove(&unit);
        }
    }

    /// The order ids in the unit's list, head (the active order) first. Pending orders are not included.
    pub fn unit_order_ids(&self, unit: ObjKey) -> Vec<u32> {
        self.orders
            .units
            .get(&unit)
            .map_or_else(Vec::new, |u| u.list.iter().map(|e| e.order.id).collect())
    }

    /// The order system (for inspection: issued orders, notifications).
    pub fn orders(&self) -> &OrderSystem {
        &self.orders
    }

    /// Takes the internal notifications (`(unit, id)`), oldest first.
    pub fn take_order_notices(&mut self) -> Vec<(ObjKey, u32)> {
        std::mem::take(&mut self.orders.notices)
    }

    /// The validator of an issued order (orders.md section 4, simplified): stop and the always-accepted id are
    /// special, other ids need an ability of the unit whose family takes them. A locked unit refuses stop.
    pub(crate) fn order_validates(&self, unit: ObjKey, order: &Order) -> bool {
        let abilities = self.unit_abilities(unit);
        if order.class == OrderClass::Immediate {
            if order.id == ORDER_ALWAYS_ACCEPTED {
                return true;
            }
            if order.id == ORDER_STOP {
                return !self.orders.locked.contains(&unit);
            }
            return self
                .orders
                .ability_for(&abilities, order.id, true)
                .is_some();
        }
        self.orders
            .ability_for(&abilities, order.id, false)
            .is_some()
    }

    /// The ability objects of a unit (its fixed slots and list, see `crate::abilities`).
    fn unit_abilities(&self, unit: ObjKey) -> Vec<Ability> {
        match self.object(unit) {
            Object::Unit(u) => u.abilities.iter().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// The dispatcher (orders.md section 5, CONFIRMED structure). Returns the index of the issued-order
    /// record when the unit accepted the order, `None` when it dropped it (locked, or the count limit).
    /// The scripted natives always call this with `replace = true`, except the append path.
    pub(crate) fn dispatch_order(
        &mut self,
        unit: ObjKey,
        order: Order,
        replace: bool,
    ) -> Option<u32> {
        if self.orders.locked.contains(&unit) {
            return None;
        }
        // An order at an enemy unit the issuer's player cannot see is dropped (`invisibility.rs`).
        if self.order_target_unseen(unit, &order) {
            return None;
        }
        if order.id == ORDER_STOP && order.class == OrderClass::Immediate {
            // Stop clears everything and is not queued (its ability is not found: UNVERIFIED).
            self.note_order_replaced(unit, order.id);
            self.clear_orders(unit);
            return None;
        }
        if replace {
            // Message `0xD0144` reaches the unit's spell: a channel ends unless this is its own order (cast.rs).
            self.note_order_replaced(unit, order.id);
            self.clear_orders(unit);
        }
        if self.orders.total(unit) > ORDER_LIMIT {
            return None;
        }
        let index = self.orders.issued.len() as u32;
        self.orders.issued.push(IssuedOrder {
            unit,
            order: order.clone(),
        });
        let st = self.orders.units.entry(unit).or_default();
        if st.starting {
            st.pending.push(order);
            return Some(index);
        }
        let idle = st.list.is_empty();
        st.list.push(Entry {
            order,
            started: false,
        });
        if idle {
            self.start_head(unit);
        } else {
            self.orders.notices.push((unit, NOTE_QUEUED));
        }
        Some(index)
    }

    /// Clears the unit's orders: the stop notification (when there was something), then the cleared
    /// notification for each order, then the lists and the route are emptied (orders.md section 5, clear-all).
    fn clear_orders(&mut self, unit: ObjKey) {
        self.combat_end(unit);
        self.mover_halt(unit);
        let Some(st) = self.orders.units.get_mut(&unit) else {
            return;
        };
        if st.list.is_empty() && st.pending.is_empty() {
            st.route = None;
            return;
        }
        let cleared = st.list.len();
        st.list.clear();
        st.pending.clear();
        st.route = None;
        self.orders.notices.push((unit, NOTE_STOP));
        for _ in 0..cleared {
            self.orders.notices.push((unit, NOTE_STARTED_OR_CLEARED));
        }
    }

    /// Starts the head of the unit's list (the drain step of orders.md 5 and 6.1). Its class decides what
    /// happens: an immediate order completes at once except hold position; a point order plans a route for a
    /// move or attack ability and completes at once when the route is empty; a target order completes at once.
    fn start_head(&mut self, unit: ObjKey) {
        let abilities = self.unit_abilities(unit);
        let Some(head) = self
            .orders
            .units
            .get(&unit)
            .and_then(|u| u.list.first())
            .map(|e| e.order.clone())
        else {
            return;
        };
        self.orders.units.entry(unit).or_default().starting = true;
        self.orders.notices.push((unit, NOTE_STARTED_OR_CLEARED));
        // Attack orders of a unit with the attack ability belong to the attack cycle (`combat.rs`), which
        // plans the walk and completes the order.
        let combat = self.combat_kind_of(unit, &head);
        let mut complete = true;
        let mut route = None;
        match head.class {
            OrderClass::Immediate => {
                if head.id == ORDER_HOLD {
                    complete = false;
                }
            }
            OrderClass::Point => {
                if let (None, Some(kind), Some(goal)) = (
                    combat,
                    self.orders.ability_for(&abilities, head.id, false),
                    head.point,
                ) {
                    if matches!(kind, BaseKind::Move | BaseKind::Attack) {
                        let points = self.plan_route(unit, goal);
                        if !points.is_empty() {
                            route = Some(Route { points, next: 0 });
                            complete = false;
                        }
                    }
                }
            }
            OrderClass::Target => {}
        }
        let routed = route.is_some();
        let st = self.orders.units.entry(unit).or_default();
        st.starting = false;
        st.route = route;
        if let Some(e) = st.list.first_mut() {
            e.started = true;
        }
        let pending = std::mem::take(&mut st.pending);
        if routed {
            self.issue_route_motion(unit);
        } else {
            self.mover_halt(unit);
        }
        if combat.is_some() {
            self.combat_begin(unit);
        } else if complete {
            self.complete_head(unit);
        }
        // Orders that were dispatched during the start join the list behind the current one.
        for order in pending {
            self.orders.units.entry(unit).or_default().list.push(Entry {
                order,
                started: false,
            });
        }
    }

    /// Removes the finished head and starts the next order, if any.
    fn complete_head(&mut self, unit: ObjKey) {
        if self.combat_head(unit).is_some() {
            self.combat_end(unit);
        }
        let st = self.orders.units.entry(unit).or_default();
        if !st.list.is_empty() {
            st.list.remove(0);
        }
        st.route = None;
        let more = !st.list.is_empty();
        if more {
            self.start_head(unit);
        } else {
            self.mover_halt(unit);
        }
    }

    /// Plans the route from the unit's position to `goal` (world points, start side first, the start itself
    /// excluded). Uses the pathing search when present (orders-2 / pathing-2 dispatcher), else the straight
    /// line. Returns an empty list when the unit is already at the goal or the search stays on the start.
    fn plan_route(&mut self, unit: ObjKey, goal: (F32, F32)) -> Vec<(F32, F32)> {
        let (ux, uy) = {
            let u = self.unit(unit);
            (u.x, u.y)
        };
        let straight = vec![goal];
        let Some(map) = self.orders.pathing.as_mut() else {
            return straight;
        };
        let (ox, oy) = map.origin;
        let start = (
            scale_by_1_32(arith::sub(ux, ox)),
            scale_by_1_32(arith::sub(uy, oy)),
        );
        let goal_cell = (
            scale_by_1_32(arith::sub(goal.0, ox)),
            scale_by_1_32(arith::sub(goal.1, oy)),
        );
        let req = Request {
            path_type: 0,
            flag_on: false,
            start,
            goal: goal_cell,
            budget: POP_BUDGET,
            size_class: 0,
        };
        // A request that ends on its own start (code 0, one point) is returned as it is: the original does not retry with a
        // larger budget (`0x6F49B9C0` calls the dispatcher once; a failed re-plan makes the follower wait 20 walks).
        let found = map.search.dispatch(&req);
        let Some(d) = found else {
            return straight;
        };
        let to_world = |p: (F32, F32)| {
            (
                arith::add(ox, arith::mul(p.0, CELL_SIZE)),
                arith::add(oy, arith::mul(p.1, CELL_SIZE)),
            )
        };
        let n = d.points.len();
        if n >= 2 {
            // The output is goal side first and ends with the start: walk it reversed, minus the start.
            d.points[..n - 1]
                .iter()
                .rev()
                .map(|&p| to_world(p))
                .collect()
        } else if d.code == 1 {
            d.points.iter().map(|&p| to_world(p)).collect()
        } else {
            Vec::new()
        }
    }

    /// The active order of `unit` (the head of its list), or `None` when it has none.
    pub(crate) fn order_head(&self, unit: ObjKey) -> Option<Order> {
        self.orders
            .units
            .get(&unit)
            .and_then(|u| u.list.first())
            .map(|e| e.order.clone())
    }

    /// Completes the active order of `unit` and starts the next one (for the attack cycle).
    pub(crate) fn complete_order_head(&mut self, unit: ObjKey) {
        self.complete_head(unit);
    }

    /// Whether `unit` is walking a route.
    pub(crate) fn route_active(&self, unit: ObjKey) -> bool {
        self.orders
            .units
            .get(&unit)
            .is_some_and(|u| u.route.is_some())
    }

    /// Stops `unit` where it stands (drops its route).
    pub(crate) fn stop_route(&mut self, unit: ObjKey) {
        if let Some(u) = self.orders.units.get_mut(&unit) {
            u.route = None;
        }
        self.mover_halt(unit);
    }

    /// Plans a route from `unit` to `goal` and makes it the unit's route. Returns `false`, with no route, when the
    /// planner finds no way.
    pub(crate) fn chase_route(&mut self, unit: ObjKey, goal: (F32, F32)) -> bool {
        let points = self.plan_route(unit, goal);
        let st = self.orders.units.entry(unit).or_default();
        if points.is_empty() {
            st.route = None;
            self.mover_halt(unit);
            return false;
        }
        st.route = Some(Route { points, next: 0 });
        self.issue_route_motion(unit);
        true
    }

    /// The move issue of a freshly planned route: the mover takes the unit's speed and a zero velocity
    /// (`movement-6.md` section 5; `Game::mover_issue`). Without tables there is no mover and the unit stays put.
    fn issue_route_motion(&mut self, unit: ObjKey) {
        let Some(t) = self.orders.tables.take() else {
            return;
        };
        self.mover_issue(unit, F32::ZERO, &t);
        self.orders.tables = Some(t);
        // CAbilityMove's start reaches the mover start 0x6F474D40 (docs/spec/lockstep.md section 17)
        self.pr_mover_start(unit);
    }

    /// The softfloat tables of the order system, when the host set them.
    pub(crate) fn order_tables(&self) -> Option<&Tables> {
        self.orders.tables.as_ref()
    }

    /// A route ended: an attack-cycle order keeps its order (the cycle decides the next step), any other order
    /// completes.
    fn route_finished(&mut self, unit: ObjKey) {
        if self.combat_head(unit).is_some() {
            self.stop_route(unit);
        } else {
            self.complete_head(unit);
        }
    }

    /// The stop-everything step of the order system (`0x6F2964E0`): the stop notification, then every order of the unit
    /// is cleared (the unit teardown `0x6F2A4A10`, the unit-suspend-decay native and the corpse creation use it).
    pub(crate) fn stop_all_orders(&mut self, unit: ObjKey) {
        self.clear_orders(unit);
    }

    /// Issues an order from a native: validation, then dispatch. Returns `(validated, record index)`:
    /// the native returns `validated` (true even when the dispatcher drops the order), and fires the
    /// issued-order events only when `record index` is set.
    pub(crate) fn issue_order(
        &mut self,
        unit: ObjKey,
        order: Order,
        replace: bool,
    ) -> (bool, Option<u32>) {
        if !self.order_validates(unit, &order) {
            return (false, None);
        }
        (true, self.dispatch_order(unit, order, replace))
    }

    /// Queues a point order behind the unit's current orders (the shift path: `replace = false`, orders.md 5).
    /// Validated like the natives. Returns whether the order was accepted into the unit (not locked, under the
    /// limit). The scripts cannot reach this path: their natives always replace.
    pub fn queue_point_order(&mut self, unit: ObjKey, id: u32, x: F32, y: F32) -> bool {
        let class = if point_capable(id) {
            OrderClass::Point
        } else {
            OrderClass::Target
        };
        let order = Order {
            id,
            class,
            point: Some((x, y)),
            target: 0,
        };
        if !self.order_validates(unit, &order) {
            return false;
        }
        self.dispatch_order(unit, order, false).is_some()
    }

    /// The combat tick of every unit with an order. Called once per batch by the script host (cadence
    /// UNSURE, lockstep.md section 11.7). It needs no tables (the chase walk does).
    pub fn tick_combat(&mut self) {
        let busy: Vec<ObjKey> = self
            .orders
            .units
            .iter()
            .filter(|(_, st)| !st.list.is_empty())
            .map(|(&k, _)| k)
            .collect();
        for unit in busy {
            self.combat_tick(unit);
        }
    }

    /// Runs one movement walk: one step of every unit with a route (the mover of `movement.md` section 1,
    /// per unit in key order). Called by the movement timer (message 0x8026A), every
    /// [`crate::lockstep::MOVE_PERIOD_BITS`] = 0.03 s of agile clock (lockstep.md section 11.2-11.3).
    pub fn tick_movement(&mut self) {
        self.movement_calls += 1;
        self.pr_walk_release();
        self.tick_turns();
        let Some(tables) = self.orders.tables.take() else {
            return;
        };
        let keys: Vec<ObjKey> = self
            .orders
            .units
            .iter()
            .filter(|(_, st)| st.route.is_some())
            .map(|(&k, _)| k)
            .collect();
        for unit in keys {
            self.step_unit(unit, &tables);
        }
        // 0x6F483300 runs the facing smoother over the listed movers after the walks
        self.tick_facing_smoothers(&tables);
        // then the push-apart records (0x6F483300 calls the kernel for every second record)
        self.push_walk(&tables);
        self.orders.tables = Some(tables);
    }

    /// One movement walk of a unit with a route (`mover::walk`, `docs/spec/movement-6.md`): the walk steers (speed,
    /// heading) and re-bases the position at the current game time; the position itself is evaluated, not stepped.
    fn step_unit(&mut self, unit: ObjKey, t: &Tables) {
        let live = {
            let Object::Unit(u) = self.object(unit) else {
                return;
            };
            !(u.dead || u.removed)
        };
        if !live || self.is_stunned(unit) {
            // A dead or removed unit stops where it is (its velocity would otherwise carry it on); so does a stunned one
            // (`CBuffStun` counts a disable on the unit, `crate::auras`), and its route stays for when the stun ends.
            self.mover_halt_with(unit, t);
            return;
        }
        if self.is_stunned(unit) || self.is_rooted(unit) {
            // A rooted unit (Ensnare, Web: the buff calls the mover's movement-disable `0x6F2031D0`) does not move either.
            self.mover_halt_with(unit, t);
            return;
        }
        let Some(route) = self.orders.units.get(&unit).and_then(|s| s.route.clone()) else {
            return;
        };
        if route.next >= route.points.len() {
            self.route_finished(unit);
            return;
        }
        let g = self.layer();
        let mut kin = self.kin_of(unit);
        let pos = kin.position(&g);
        // Intermediate waypoints count as reached inside 0.49 cells (0x6F49C3B0, squared compare, LIKELY).
        let last = route.points.len() - 1;
        let mut next = route.next;
        while next < last {
            let wp = self.cells_of(route.points[next]);
            // 0x6F49C3B0: the fused distance and an inclusive compare (CONFIRMED, Unicorn `WR`)
            if mover::waypoint_reached(pos, wp) {
                next += 1;
            } else {
                break;
            }
        }
        if next != route.next {
            if let Some(rt) = self.orders.units.get_mut(&unit).and_then(|s| s.route.as_mut()) {
                rt.next = next;
            }
            // a new leg starts the mover again (UNSURE: that the original does this per leg, section 17.4)
            self.pr_mover_leg(unit);
        }
        let goal = self.cells_of(route.points[last]);
        let waypoint = self.cells_of(route.points[next]);
        let pilot = self.pilot_of(unit);
        let trace = mover::walk(&mut kin, &pilot, &g, goal, PathState::Open(waypoint), t);
        // The speed writer's velocity notification (0x6F494620 -> 0x6F4A74A0, CONFIRMED by `WN` vectors): the facing
        // of a unit in motion is the heading of its velocity. The mover joins the smoother list.
        kin.notify_facing(t);
        self.commit_kin(unit, &kin);
        if let Some(rt) = self.movers.get_mut(&unit) {
            rt.listed = true;
        }
        if trace.arrived {
            self.route_finished(unit);
        }
    }
}
