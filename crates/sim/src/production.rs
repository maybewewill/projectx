//! Unit production for a player: training queues, building construction, research and worker harvesting.
//!
//! This is the part of the game that the AI manager drives through its production and order natives
//! (`ai_manager.rs`, `docs/spec/ai-natives.md` section 7), but nothing here is AI specific: the same entry points
//! can serve a player command.
//!
//! Labels as elsewhere: CONFIRMED = read in the original and checked, LIKELY = read, a detail inferred, UNSURE = a
//! model choice that stands in for a part that is not decoded. The stand-ins are listed in the spec section 7.5.
//!
//! What is modelled, and from where:
//!
//! * **Training** (`CAbilityTrain`, `economy.rs` [`TrainQueue`], CONFIRMED pieces): seven slots, the cost is paid when
//!   a unit is queued, the food is taken when the head starts, the next head starts after a finish. The length of the
//!   timer is the type's `bldtm` (UNSURE, `economy-4.md`: the progress timer was not found). A queued unit counts as
//!   an *in progress* unit of its type for the owner's counters (LIKELY: the AI scripts rely on it, otherwise
//!   `SetBuildUnit` would queue the same unit again every cycle). A finished unit appears next to its building.
//! * **Construction** (`construction.rs`, CONFIRMED registration and refund pieces): the worker walks to the site, the
//!   cost is paid when it arrives (the original's placement payment `0x6F0EDA70`), the building is created as an
//!   in-progress unit (its food made applies at the finish, CONFIRMED order of `0x6F29F850`), grows linearly from 10 % to
//!   its full life (UNSURE law), and completes after `bldtm`. The builder of a night elf or undead building is
//!   consumed (LIKELY).
//! * **Research** (UNSURE timer, as training): a building with the tech in its `Researches` list, one at a time, the
//!   cost `base + mod * level` of `UpgradeData.slk` paid when the research starts, the player's tech level raised at the
//!   finish.
//! * **Harvesting** (`economy.rs` [`Worker`], CONFIRMED carry, trip and deposit arithmetic): a worker walks to the
//!   source, works for a fixed time, walks to the nearest completed town hall of its owner and deposits through the
//!   income changer (tax and share). The trip times and the choice of the hall are stand-ins (UNSURE).
//! * **Placement** (stand-in, UNSURE): the original's build spots (`0x6F2D2880`) are not decoded. The model scans
//!   rings around the town hall for a free square on the pathing grid, away from mines, trees and structures.
//!
//! Time. [`Production::tick`] is called once per game tick with the agile clock; the timers (training, construction,
//! research) fire in tick order. [`Game::production_batch`] runs the walking logic once per batch (0.1 s).

use std::collections::{BTreeMap, BTreeSet};

use softfloat::F32;

use crate::economy::{
    credit, Flags, Res, TrainError, TrainItem, TrainOutcome, TrainQueue,
    TrainTimer, Worker,
};
use crate::natives::game::{Game, ObjKey, Object};
use crate::orders::{Order, OrderClass, ORDER_MOVE};
use crate::timer::TimerQueue;
use crate::unit_counts::{class_of, REGISTERED_COMPLETED, REGISTERED_IN_PROGRESS};

/// Class bit of a town hall in the unit classification word (`UNIT_TYPE_TOWNHALL`).
pub const CLASS_TOWNHALL: u32 = 0x40;

/// The food ceiling (`FoodCeiling`, Misc default 100; UNSURE for a map that changes it).
pub const FOOD_CEILING: i32 = 100;

/// The share of the life a new building starts with (`0.1`, CONFIRMED in `construction::percent_life`).
const START_LIFE: f32 = 0.1;

/// Seconds a worker spends inside a gold mine per trip (UNSURE stand-in).
pub const MINE_SECONDS: f32 = 1.0;
/// Seconds between two lumber hits (UNSURE stand-in; the swing interval of the original is the unit's attack cooldown
/// data, `Worker::swing_interval`).
pub const HIT_SECONDS: f32 = 1.0;
/// Gold carried per trip (`Ahar` data, default 10, UNSURE for a changed table).
pub const GOLD_TRIP: i32 = 10;
/// Lumber carried per trip (`Ahar` data, default 10).
pub const LUMBER_TRIP: i32 = 10;
/// Lumber a standing tree holds (UNSURE stand-in; the tree's life stands for it in the original).
pub const TREE_WOOD: i32 = 50;

/// How near (world units) a worker must be to a mine to enter it.
const MINE_REACH: f32 = 230.0;
/// How near a worker must be to a hall to deposit.
const HALL_REACH: f32 = 260.0;
/// How near a worker must be to a tree to chop it.
const TREE_REACH: f32 = 120.0;
/// How near a builder must be to its site to place the building.
const SITE_REACH: f32 = 200.0;
/// A move order that ended farther than this many times its reach counts as blocked (the worker proceeds anyway after
/// a few retries, UNSURE).
const RETRY_LIMIT: u8 = 4;

/// What a unit type can train, build and research, and what it needs. Filled by the host from the object text
/// (`Trains`, `Builds`, `Researches`, `Requires`, `Upgrade` of the `*UnitFunc.txt` profiles).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeTech {
    pub trains: Vec<u32>,
    pub builds: Vec<u32>,
    pub researches: Vec<u32>,
    /// `Requires` entries: (required type, amount). All are needed.
    pub requires: Vec<(u32, i32)>,
    /// A hero type's numbered requirements: entry `n` (`Requires`, `Requires1`, ...) is what the player needs to own
    /// before it may get its `n + 1`-th hero (the first hero needs nothing in the retail data).
    pub hero_requires: Vec<Vec<u32>>,
    /// `Upgrade`: the types this one upgrades into (a town hall into the next tier).
    pub upgrade_to: Vec<u32>,
    /// The unit has the harvest ability for gold (`Ahar` or the acolyte's).
    pub harvest_gold: bool,
    /// The unit has the harvest ability for lumber.
    pub harvest_wood: bool,
}

/// One row of `UpgradeData.slk` (the columns the AI needs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpgradeType {
    pub gold_base: i32,
    pub gold_mod: i32,
    pub lumber_base: i32,
    pub lumber_mod: i32,
    pub time_base: i32,
    pub time_mod: i32,
    pub max_level: i32,
}

impl UpgradeType {
    /// Gold of the research that takes the tech from `level` to `level + 1`: `base + mod * level` (CONFIRMED shape for
    /// the AI cost natives: "base plus increment times (level minus 1)" for the target level).
    pub fn gold_for(&self, level: i32) -> i32 {
        self.gold_base + self.gold_mod * level
    }
    pub fn lumber_for(&self, level: i32) -> i32 {
        self.lumber_base + self.lumber_mod * level
    }
    pub fn seconds_for(&self, level: i32) -> i32 {
        self.time_base + self.time_mod * level
    }
}

/// A tree that workers can chop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tree {
    pub x: f32,
    pub y: f32,
    pub wood: i32,
}

/// The buildable-area grid of the map: the pathing type byte of every 32-unit cell (`0x02` unwalkable, `0x08`
/// unbuildable), row-major from the cell at (`origin_x`, `origin_y`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BuildGrid {
    pub width: i32,
    pub height: i32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub types: Vec<u8>,
}

impl BuildGrid {
    fn cell_free(&self, cx: i32, cy: i32) -> bool {
        if cx < 0 || cy < 0 || cx >= self.width || cy >= self.height {
            return false;
        }
        self.types[(cy * self.width + cx) as usize] & 0x0A == 0
    }

    /// Whether the square of half-size `half` around (x, y) lies on buildable, walkable cells.
    pub fn square_free(&self, x: f32, y: f32, half: f32) -> bool {
        let x0 = ((x - half - self.origin_x) / 32.0).floor() as i32;
        let x1 = ((x + half - self.origin_x) / 32.0).ceil() as i32 - 1;
        let y0 = ((y - half - self.origin_y) / 32.0).floor() as i32;
        let y1 = ((y + half - self.origin_y) / 32.0).ceil() as i32 - 1;
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                if !self.cell_free(cx, cy) {
                    return false;
                }
            }
        }
        true
    }
}

/// Why a production request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProdError {
    /// The unit is gone, dead, not completed, or not the owner's.
    BadUnit,
    /// The building cannot train / the worker cannot build / the building cannot research this.
    NotAvailable,
    /// A requirement of the type is not met.
    Requirement,
    /// The training queue or the research slot is full.
    Busy,
    /// Not enough gold or lumber.
    Resources,
    /// No food room.
    Food,
    /// No free build spot was found.
    NoSite,
    /// The tech is at its maximum level.
    MaxLevel,
}

impl From<TrainError> for ProdError {
    fn from(e: TrainError) -> Self {
        match e {
            TrainError::QueueFull => ProdError::Busy,
            TrainError::FoodBlocked => ProdError::Food,
            TrainError::NotEnoughResources => ProdError::Resources,
        }
    }
}

/// What the production system did, in order (for tests and the boot report).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProdEvent {
    Queued { building: ObjKey, type_id: u32 },
    Trained { building: ObjKey, unit: ObjKey, type_id: u32 },
    SiteChosen { builder: ObjKey, type_id: u32, x: i32, y: i32 },
    BuildStarted { builder: ObjKey, building: ObjKey, type_id: u32 },
    BuildFailed { builder: ObjKey, type_id: u32, reason: ProdError },
    Built { building: ObjKey, type_id: u32 },
    ResearchStarted { building: ObjKey, tech: u32 },
    UnitUpgradeStarted { unit: ObjKey, to: u32 },
    UnitUpgraded { unit: ObjKey, to: u32 },
    Researched { building: ObjKey, tech: u32, level: i32 },
    Deposit { worker: ObjKey, resource: Res, amount: i32 },
}

/// Totals for the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProdStats {
    pub gold_deposited: i64,
    pub lumber_deposited: i64,
    pub gold_trips: u32,
    pub lumber_trips: u32,
    pub trained: u32,
    pub built: u32,
    pub researched: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Walking to the source (mine or tree).
    ToSource,
    /// Inside the mine / chopping, until `until`.
    Working { until: f32 },
    /// Walking to the hall.
    ToHall,
}

/// A worker's harvest cycle.
#[derive(Debug, Clone)]
pub struct Harvester {
    pub resource: Res,
    phase: Phase,
    worker: Worker,
    /// The mine of a gold cycle.
    mine: Option<ObjKey>,
    /// The tree of a lumber cycle (index into [`Production::trees`]).
    tree: Option<usize>,
    /// Where the cycle is anchored (the town's hall position): the nearest source is chosen from here.
    anchor: (f32, f32),
    retries: u8,
}

impl Harvester {
    /// Gold or lumber.
    pub fn resource(&self) -> Res {
        self.resource
    }
    /// The carried amount.
    pub fn carry(&self) -> i32 {
        self.worker.carry
    }
}

/// The AI-visible bookkeeping flags of a worker (`unit AI record +0x28`: bit 1 gold, 2 wood this cycle; bit 4 gold,
/// 8 wood in the previous cycle, CONFIRMED in `ClearHarvestAI`, `0x6F2DD5B0`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiUnit {
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum JobPhase {
    Moving,
    Building { site: ObjKey },
}

/// A worker going to place (or placing) a building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildJob {
    pub type_id: u32,
    pub x: f32,
    pub y: f32,
    phase: JobPhase,
    retries: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Construction {
    builder: ObjKey,
    start: f32,
    seconds: f32,
    consumed: bool,
    /// The life modifier of the construction tick (`0x6F045600`): start life and rate per second, when the
    /// soft-float tables are loaded (`construction::life_ramp`).
    ramp: Option<(F32, F32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Research {
    tech: u32,
    done_at: f32,
    gold: i32,
    lumber: i32,
}

/// A unit that is turning into another type in place (a town hall upgrade).
#[derive(Debug, Clone, Copy, PartialEq)]
struct UnitUpgrade {
    to: u32,
    done_at: f32,
}

/// What the AI build-spot search reads of a unit type (`docs/spec/ai-natives.md` section 8): the `buffType` enum
/// (record `+0x1B8`), `buffRadius` (`+0x1BC`) and the size of the pathing texture (`pPathingFootprint`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SiteType {
    /// 0 none, 1 townhall, 2 resource, 4 factory, 8 buffer (`0x6F262E60`).
    pub buff_type: u32,
    pub buff_radius: F32,
    /// The pathing texture size in texels; `None` for a type without one (`pathTex` `_`).
    pub footprint: Option<crate::ai_site::Footprint>,
}

/// The production state of a game.
#[derive(Debug, Default)]
pub struct Production {
    pub tech: BTreeMap<u32, TypeTech>,
    pub upgrades: BTreeMap<u32, UpgradeType>,
    pub trees: Vec<Tree>,
    pub grid: Option<BuildGrid>,
    /// Per type: the data of the AI build-spot search (`ai_site.rs`).
    pub site_types: BTreeMap<u32, SiteType>,
    /// The Misc `BuildingAngle` after `i2r(f2i(..))` (`0x6F020D30`): selects the footprint orientation.
    pub building_angle: F32,
    /// `bldtm` of each type, seconds.
    pub build_seconds: BTreeMap<u32, i32>,
    queues: BTreeMap<ObjKey, TrainQueue>,
    train_timers: TimerQueue<TrainTimer>,
    now: f32,
    harvesters: BTreeMap<ObjKey, Harvester>,
    ai_units: BTreeMap<ObjKey, AiUnit>,
    jobs: BTreeMap<ObjKey, BuildJob>,
    constructions: BTreeMap<ObjKey, Construction>,
    research: BTreeMap<ObjKey, Research>,
    unit_upgrades: BTreeMap<ObjKey, UnitUpgrade>,
    pub events: Vec<ProdEvent>,
    pub stats: ProdStats,
}

impl Production {
    /// The game time of the last tick.
    pub fn now(&self) -> f32 {
        self.now
    }

    /// Whether `unit` has a harvest cycle.
    pub fn harvester(&self, unit: ObjKey) -> Option<&Harvester> {
        self.harvesters.get(&unit)
    }

    /// Number of workers with a harvest cycle of `resource`.
    pub fn harvesting(&self, resource: Res) -> usize {
        self.harvesters.values().filter(|h| h.resource == resource).count()
    }

    /// The queued types of a building's training queue, head first.
    pub fn queued(&self, building: ObjKey) -> Vec<u32> {
        self.queues
            .get(&building)
            .map(|q| q.items().iter().map(|i| i.type_id).collect())
            .unwrap_or_default()
    }

    /// The AI flags of a unit.
    pub fn ai_unit(&self, unit: ObjKey) -> AiUnit {
        self.ai_units.get(&unit).copied().unwrap_or_default()
    }

    /// Sets the AI flags of a unit.
    pub fn set_ai_flags(&mut self, unit: ObjKey, flags: u32) {
        self.ai_units.entry(unit).or_default().flags = flags;
    }

    /// Whether the worker is on a build job.
    pub fn is_building(&self, unit: ObjKey) -> bool {
        self.jobs.contains_key(&unit)
    }

    /// The type a worker is on its way to build (jobs that have not placed yet), the AI's "ordered to build" count.
    pub fn pending_build_type(&self, unit: ObjKey) -> Option<u32> {
        self.jobs.get(&unit).and_then(|j| match j.phase {
            JobPhase::Moving => Some(j.type_id),
            JobPhase::Building { .. } => None,
        })
    }

    /// Whether the building is researching.
    pub fn researching(&self, building: ObjKey) -> Option<u32> {
        self.research.get(&building).map(|r| r.tech)
    }

    /// The type a unit is upgrading into, if any.
    pub fn upgrading_to(&self, unit: ObjKey) -> Option<u32> {
        self.unit_upgrades.get(&unit).map(|u| u.to)
    }

    /// The in-place upgrades running into `type_id`.
    pub fn upgrades_into(&self, type_id: u32) -> usize {
        self.unit_upgrades.values().filter(|u| u.to == type_id).count()
    }

    /// How many units of `type_id` are queued at `building`.
    pub fn queued_total(&self, building: ObjKey, type_id: u32) -> usize {
        self.queues
            .get(&building)
            .map_or(0, |q| q.items().iter().filter(|i| i.type_id == type_id).count())
    }

    /// Whether the building is under construction by this system.
    pub fn is_constructing(&self, building: ObjKey) -> bool {
        self.constructions.contains_key(&building)
    }

    /// Tree wood left in total (for the report).
    pub fn wood_left(&self) -> i64 {
        self.trees.iter().map(|t| i64::from(t.wood)).sum()
    }
}

fn fl(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// Seconds until the countdown property of a timed object reaches 0: `R / D` with `R = I2R(bldtm)` stored by
/// `0x6F023A30` and `D` the speed factor (1.0; the cheat constant of `0x6F281530` is not applied), both through
/// the soft float (`0x6F030920` computes `R / D` for the modifier horizon, `construction::timed_start`). Without
/// tables the plain integer is used. Whether the listener fires exactly at `R / D` is the agile engine's
/// modifier evaluation, which is not decoded (UNSURE).
fn countdown_seconds(tables: Option<&softfloat::Tables>, bldtm: i32) -> f32 {
    match tables {
        Some(t) => {
            softfloat::arith::div(softfloat::conv::i2r(bldtm), F32::ONE, t).to_f32()
        }
        None => bldtm as f32,
    }
}

fn square_gap(ax: f32, ay: f32, ar: f32, bx: f32, by: f32, br: f32) -> bool {
    (ax - bx).abs() >= ar + br || (ay - by).abs() >= ar + br
}

impl Game {
    /// The production state.
    pub fn production(&self) -> &Production {
        &self.production
    }

    /// The production state, for the host that installs the tech data.
    pub fn production_mut(&mut self) -> &mut Production {
        &mut self.production
    }

    fn unit_xy(&self, k: ObjKey) -> (f32, f32) {
        let u = self.unit(k);
        (u.x.to_f32(), u.y.to_f32())
    }

    fn unit_live(&self, k: ObjKey) -> bool {
        matches!(self.object(k), Object::Unit(u) if !u.dead && !u.removed)
    }

    /// The live units of `owner`, in creation order.
    pub fn owned_units(&self, owner: u32) -> Vec<ObjKey> {
        self.unit_objects()
            .filter(|(_, u)| u.owner == owner && !u.dead && !u.removed)
            .map(|(k, _)| k)
            .collect()
    }

    fn type_class(&self, type_id: u32) -> u32 {
        class_of(self.unit_types().is_building(type_id))
    }

    /// Whether the unit is a completed one (counted as completed, not an in-progress building).
    pub fn unit_completed(&self, k: ObjKey) -> bool {
        self.unit_live(k) && self.unit(k).count_state & REGISTERED_IN_PROGRESS == 0
    }

    /// The half-size of the square a building of this type blocks (UNSURE stand-in for the pathing footprint).
    pub fn footprint_half(&self, type_id: u32) -> f32 {
        let Some(t) = self.unit_types().get(type_id) else { return 64.0 };
        if t.classification & CLASS_TOWNHALL != 0 {
            128.0
        } else if t.gold_cost + t.wood_cost >= 250 {
            96.0
        } else {
            64.0
        }
    }

    fn food_cap_of(&self, p: u32) -> i32 {
        let made = self.lane_f.food.get(&p).map_or(0, |f| f.made);
        made.min(FOOD_CEILING)
    }


    /// The count of completed units of `type_id`, or of any type it upgrades into, owned by `owner`.
    pub fn completed_chain(&self, owner: u32, type_id: u32) -> i32 {
        let mut set = BTreeSet::new();
        let mut stack = vec![type_id];
        while let Some(t) = stack.pop() {
            if !set.insert(t) {
                continue;
            }
            if let Some(tech) = self.production.tech.get(&t) {
                stack.extend(tech.upgrade_to.iter().copied());
            }
        }
        self.owned_units(owner)
            .into_iter()
            .filter(|&k| {
                let u = self.unit(k);
                set.contains(&u.type_id) && u.count_state & REGISTERED_IN_PROGRESS == 0
            })
            .count() as i32
    }

    /// Whether `owner` meets the `Requires` entries of `type_id` (completed units, tier upgrades included).
    pub fn requirements_met(&self, owner: u32, type_id: u32) -> bool {
        let Some(tech) = self.production.tech.get(&type_id) else { return true };
        if crate::natives::hero::is_hero_id(type_id) && !tech.hero_requires.is_empty() {
            // The tier follows the number of heroes the player already owns (any state, not removed).
            let owned = self
                .unit_objects()
                .filter(|(_, u)| u.owner == owner && !u.removed && crate::natives::hero::is_hero_id(u.type_id))
                .count();
            let tier = owned.min(tech.hero_requires.len() - 1);
            return tech.hero_requires[tier].iter().all(|&req| self.completed_chain(owner, req) >= 1);
        }
        tech.requires
            .iter()
            .all(|&(req, amount)| self.completed_chain(owner, req) >= amount.max(1))
    }

    fn can_pay(&self, owner: u32, gold: i32, lumber: i32) -> bool {
        let p = &self.economy[owner as usize];
        i64::from(p.stored(Res::Gold)) >= i64::from(gold) * 10
            && i64::from(p.stored(Res::Lumber)) >= i64::from(lumber) * 10
    }

    fn pay(&mut self, owner: u32, gold: i32, lumber: i32) {
        credit(&mut self.economy, owner as usize, Res::Gold, gold.wrapping_neg(), Flags::NONE);
        credit(&mut self.economy, owner as usize, Res::Lumber, lumber.wrapping_neg(), Flags::NONE);
    }

    // ------------------------------------------------------------------------------------------------------
    // Training

    /// Whether `building` can train `type_id` (its `Trains` list, completed, not busy upgrading, owner meets the
    /// requirements). Does not look at the queue length or the money.
    pub fn can_train(&self, building: ObjKey, type_id: u32) -> bool {
        if !self.unit_completed(building) || self.production.constructions.contains_key(&building) {
            return false;
        }
        let owner = self.unit(building).owner;
        self.production
            .tech
            .get(&self.unit(building).type_id)
            .is_some_and(|t| t.trains.contains(&type_id))
            && self.requirements_met(owner, type_id)
    }

    /// Queues `type_id` at `building`. The cost is paid now; the unit counts as in progress for its owner.
    pub fn train_unit(&mut self, building: ObjKey, type_id: u32) -> Result<(), ProdError> {
        if !self.unit_live(building) {
            return Err(ProdError::BadUnit);
        }
        if !self.can_train(building, type_id) {
            return Err(ProdError::NotAvailable);
        }
        let owner = self.unit(building).owner;
        let Some(ty) = self.unit_types().get(type_id).cloned() else {
            return Err(ProdError::NotAvailable);
        };
        let bldtm = self.production.build_seconds.get(&type_id).copied().unwrap_or(0);
        let seconds = countdown_seconds(self.order_tables(), bldtm);
        let item = TrainItem {
            type_id,
            gold: ty.gold_cost,
            lumber: ty.wood_cost,
            food: ty.food_used,
            food_exempt: false,
            build_seconds: seconds,
        };
        let cap = self.food_cap_of(owner);
        let mut used = self.food_used_of(owner);
        let before = used;
        let mut queue = self.production.queues.remove(&building).unwrap_or_default();
        let result = queue.enqueue(&mut self.economy, owner as usize, item, used, cap);
        if result.is_ok() {
            // The head starts at once when nothing runs (the food is taken then).
            let mut timers = std::mem::take(&mut self.production.train_timers);
            let _ = queue.try_start(&mut timers, &mut used, cap, building.0 as usize);
            self.production.train_timers = timers;
        }
        self.production.queues.insert(building, queue);
        if used != before {
            self.food_add_used(owner, used.wrapping_sub(before));
        }
        result?;
        let class = self.type_class(type_id);
        if let Some(p) = self.player_mut(owner) {
            p.counts.begin_in_progress(type_id, class, false);
        }
        self.production.events.push(ProdEvent::Queued { building, type_id });
        Ok(())
    }

    fn spawn_point(&self, building: ObjKey) -> (f32, f32) {
        let (x, y) = self.unit_xy(building);
        let half = self.footprint_half(self.unit(building).type_id);
        (x, y - half - 48.0)
    }

    fn on_train_finished(&mut self, building: ObjKey, type_id: u32) {
        let owner = self.unit(building).owner;
        let class = self.type_class(type_id);
        if let Some(p) = self.player_mut(owner) {
            p.counts.remove_in_progress(type_id, class, false);
        }
        if !self.unit_live(building) {
            return;
        }
        let (x, y) = self.spawn_point(building);
        let food = self.unit_types().get(type_id).map_or(0, |t| t.food_used);
        let face = F32::from_bits(0x4387_0000); // 270 degrees, facing away from the building
        if let Some((k, _)) = crate::natives::unit::build(self, owner, type_id, fl(x), fl(y), face, true) {
            // The food of the trained unit was taken when its head started; the creation applied it again.
            self.food_add_used(owner, food.wrapping_neg());
            self.production.stats.trained += 1;
            self.production.events.push(ProdEvent::Trained { building, unit: k, type_id });
        }
    }

    // ------------------------------------------------------------------------------------------------------
    // Construction

    /// The AI's build spot for a building of `type_id` around `anchor` (the town's hall position), for `owner`:
    /// the search of `0x6F2E50E0` ([`crate::ai_site`], `docs/spec/ai-natives.md` section 8). When the map has no
    /// pathing grid, the type has no pathing texture or the playable area is unknown, the ring stand-in below is
    /// used instead.
    pub fn find_build_site(&self, owner: u32, type_id: u32, anchor: (f32, f32)) -> Option<(f32, f32)> {
        if let Some(r) = self.find_build_site_ai(owner, type_id, anchor) {
            return r;
        }
        self.find_build_site_rings(owner, type_id, anchor)
    }

    /// `Some(result)` when the original's search could be run, `None` when its inputs are missing.
    fn find_build_site_ai(&self, owner: u32, type_id: u32, anchor: (f32, f32)) -> Option<Option<(f32, f32)>> {
        use crate::ai_site::{self, MarkUnit, Params};
        let grid = self.production.grid.as_ref()?;
        let playable = self.playable()?;
        let footprint = self.production.site_types.get(&type_id)?.footprint?;
        let rot = ai_site::rotation_class(self.production.building_angle);
        let center = (F32::from_bits(anchor.0.to_bits()), F32::from_bits(anchor.1.to_bits()));
        // The structures that mark the window: live units and pending jobs with a pathing texture, within 1600 of the
        // centre (the range query of `0x6F2DF650`, flag 4 = a live unit; the footprint test stands in for the
        // flag word check of `0x6F26E490`, UNSURE).
        let origin = (playable.min_x, playable.min_y);
        let mut units: Vec<MarkUnit> = Vec::new();
        for (_, u) in self.unit_objects() {
            if u.dead || u.removed {
                continue;
            }
            let Some(st) = self.production.site_types.get(&u.type_id) else { continue };
            let Some(fp) = st.footprint else { continue };
            if !crate::natives::group::in_range(u.x, u.y, center.0, center.1, ai_site::HALF_WINDOW, origin) {
                continue;
            }
            units.push(MarkUnit { buff_type: st.buff_type, buff_radius: st.buff_radius, x: u.x, y: u.y, footprint: fp });
        }
        for job in self.production.jobs.values() {
            let Some(st) = self.production.site_types.get(&job.type_id) else { continue };
            let Some(fp) = st.footprint else { continue };
            let (jx, jy) = (F32::from_bits(job.x.to_bits()), F32::from_bits(job.y.to_bits()));
            if crate::natives::group::in_range(jx, jy, center.0, center.1, ai_site::HALF_WINDOW, origin) {
                units.push(MarkUnit { buff_type: st.buff_type, buff_radius: st.buff_radius, x: jx, y: jy, footprint: fp });
            }
        }
        let count = crate::ai_manager::unit_count(self, owner, type_id, false);
        let params = Params {
            type_id,
            player: owner,
            count_ge_two: count >= 2,
            center,
            playable,
            footprint,
            rot,
        };
        let host = SiteHostImpl::new(self, grid, &units, rot, center, footprint);
        Some(ai_site::find_site(&host, &params, &units).map(|(x, y)| (x.to_f32(), y.to_f32())))
    }

    /// Finds a free square for a building of `type_id` near `anchor`, for `owner` (UNSURE stand-in, see the module
    /// notes). Rings of 64 units from 192 out to 1700.
    pub fn find_build_site_rings(&self, owner: u32, type_id: u32, anchor: (f32, f32)) -> Option<(f32, f32)> {
        let grid = self.production.grid.as_ref()?;
        let half = self.footprint_half(type_id);
        let mut structures: Vec<(f32, f32, f32)> = Vec::new();
        for (k, u) in self.unit_objects() {
            if u.removed || u.dead {
                continue;
            }
            if self.unit_types().is_building(u.type_id) && !self.mines.contains_key(&k) {
                let (x, y) = self.unit_xy(k);
                structures.push((x, y, self.footprint_half(u.type_id)));
            }
        }
        for job in self.production.jobs.values() {
            structures.push((job.x, job.y, self.footprint_half(job.type_id)));
        }
        let mines: Vec<(f32, f32)> = self
            .mines
            .keys()
            .filter(|&&k| self.unit_live(k))
            .map(|&k| self.unit_xy(k))
            .collect();
        let _ = owner;
        let step = 64.0f32;
        let mut r = 192.0f32;
        while r <= 1700.0 {
            let n = (r / step) as i32;
            // The ring of the square at Chebyshev distance r, in a fixed order (rows bottom to top, then columns).
            for iy in -n..=n {
                for ix in -n..=n {
                    if ix.abs() != n && iy.abs() != n {
                        continue;
                    }
                    let x = anchor.0 + ix as f32 * step;
                    let y = anchor.1 + iy as f32 * step;
                    if !grid.square_free(x, y, half) {
                        continue;
                    }
                    if structures.iter().any(|&(sx, sy, sr)| !square_gap(x, y, half + 32.0, sx, sy, sr)) {
                        continue;
                    }
                    if mines.iter().any(|&(mx, my)| !square_gap(x, y, half, mx, my, 144.0 + 96.0)) {
                        continue;
                    }
                    if self
                        .production
                        .trees
                        .iter()
                        .any(|t| t.wood > 0 && !square_gap(x, y, half, t.x, t.y, 48.0))
                    {
                        continue;
                    }
                    return Some((x, y));
                }
            }
            r += step;
        }
        None
    }

    /// Whether `builder` can build `type_id` (its `Builds` list, alive, completed, not on another job).
    pub fn can_build(&self, builder: ObjKey, type_id: u32) -> bool {
        self.unit_completed(builder)
            && !self.production.jobs.contains_key(&builder)
            && self
                .production
                .tech
                .get(&self.unit(builder).type_id)
                .is_some_and(|t| t.builds.contains(&type_id))
            && self.requirements_met(self.unit(builder).owner, type_id)
    }

    /// Sends `builder` to place a building of `type_id` at (x, y). The cost is paid on arrival.
    pub fn start_build_job(&mut self, builder: ObjKey, type_id: u32, x: f32, y: f32) -> Result<(), ProdError> {
        if !self.can_build(builder, type_id) {
            return Err(ProdError::NotAvailable);
        }
        let ty = self.unit_types().get(type_id).ok_or(ProdError::NotAvailable)?;
        let (gold, wood) = (ty.gold_cost, ty.wood_cost);
        let owner = self.unit(builder).owner;
        if !self.can_pay(owner, gold, wood) {
            return Err(ProdError::Resources);
        }
        // A worker taken for a job leaves its harvest cycle (the carried load goes with it, UNSURE).
        self.production.harvesters.remove(&builder);
        self.production.jobs.insert(
            builder,
            BuildJob { type_id, x, y, phase: JobPhase::Moving, retries: 0 },
        );
        self.production.events.push(ProdEvent::SiteChosen { builder, type_id, x: x as i32, y: y as i32 });
        self.order_move(builder, x, y);
        Ok(())
    }

    fn order_move(&mut self, unit: ObjKey, x: f32, y: f32) {
        let order = Order {
            id: ORDER_MOVE,
            class: OrderClass::Point,
            point: Some((fl(x), fl(y))),
            target: 0,
        };
        let _ = self.issue_order(unit, order, true);
    }

    fn order_idle(&self, unit: ObjKey) -> bool {
        self.unit_order_ids(unit).is_empty()
    }

    fn place_building(&mut self, builder: ObjKey, job: BuildJob) -> Result<ObjKey, ProdError> {
        let owner = self.unit(builder).owner;
        let type_id = job.type_id;
        let ty = self.unit_types().get(type_id).cloned().ok_or(ProdError::NotAvailable)?;
        if !self.can_pay(owner, ty.gold_cost, ty.wood_cost) {
            return Err(ProdError::Resources);
        }
        // The spot may have been taken since it was chosen.
        let half = self.footprint_half(type_id);
        let blocked = self.unit_objects().any(|(k, u)| {
            !u.removed
                && !u.dead
                && self.unit_types().is_building(u.type_id)
                && !self.mines.contains_key(&k)
                && {
                    let (sx, sy) = self.unit_xy(k);
                    !square_gap(job.x, job.y, half, sx, sy, self.footprint_half(u.type_id))
                }
        });
        if blocked {
            return Err(ProdError::NoSite);
        }
        self.pay(owner, ty.gold_cost, ty.wood_cost);
        let (k, _) = crate::natives::unit::build(self, owner, type_id, fl(job.x), fl(job.y), F32::ZERO, true)
            .ok_or(ProdError::NotAvailable)?;
        // The creation registered a completed unit with its food; an in-progress building is registered as such and
        // gets its food made only at the finish.
        let class = self.type_class(type_id);
        if let Some(p) = self.player_mut(owner) {
            p.counts.remove_completed(type_id, class);
            p.counts.begin_in_progress(type_id, class, false);
        }
        self.unit_mut(k).count_state = REGISTERED_IN_PROGRESS;
        self.food_release(k);
        let max = ty.max_life;
        let start_life = fl(max.to_f32() * START_LIFE);
        self.unit_mut(k).life = start_life;
        let bldtm = self.production.build_seconds.get(&type_id).copied().unwrap_or(0);
        let seconds = countdown_seconds(self.order_tables(), bldtm);
        // The construction tick creates a life modifier from the 10 % start value (`0x6F05FF50`: A * 0.1) with
        // rate (A * 0.9 / total) * k (`construction::life_ramp`, CONFIRMED in Unicorn).
        let ramp = self.order_tables().and_then(|t| {
            let total = softfloat::conv::i2r(bldtm);
            let a = ty.max_life;
            let start_life = softfloat::arith::mul(a, F32::from_bits(0x3DCC_CCCD));
            crate::construction::life_ramp(a, total, total, F32::ONE, t).map(|(rate, _)| (start_life, rate))
        });
        let race = self.player(owner).race;
        let consumed = race == 3 || race == 4;
        let start = self.production.now;
        self.production.constructions.insert(k, Construction { builder, start, seconds, consumed, ramp });
        self.production.events.push(ProdEvent::BuildStarted { builder, building: k, type_id });
        Ok(k)
    }

    fn finish_building(&mut self, k: ObjKey) {
        let Some(c) = self.production.constructions.remove(&k) else { return };
        let (owner, type_id, max) = {
            let u = self.unit(k);
            (u.owner, u.type_id, u.max_life)
        };
        let class = self.type_class(type_id);
        if let Some(p) = self.player_mut(owner) {
            p.counts.finish(type_id, class, false);
        }
        {
            let u = self.unit_mut(k);
            u.count_state = REGISTERED_COMPLETED;
            u.life = max;
        }
        self.food_apply_creation(k);
        // The builder: a worker that stays goes back to being idle; a consumed one is gone already.
        self.production.jobs.remove(&c.builder);
        self.production.stats.built += 1;
        self.production.events.push(ProdEvent::Built { building: k, type_id });
    }

    // ------------------------------------------------------------------------------------------------------
    // Research

    /// The level of `tech` the owner has (0 when none).
    pub fn tech_level(&self, owner: u32, tech: u32) -> i32 {
        self.misc
            .players
            .get(owner as usize)
            .and_then(|p| p.tech.get(&tech))
            .map_or(0, |t| t.researched)
    }

    /// Whether `building` could research `tech` now (apart from money): completed, not busy, tech in its list, below
    /// the maximum level.
    pub fn can_research(&self, building: ObjKey, tech: u32) -> Result<(), ProdError> {
        if !self.unit_completed(building) {
            return Err(ProdError::BadUnit);
        }
        let owner = self.unit(building).owner;
        let listed = self
            .production
            .tech
            .get(&self.unit(building).type_id)
            .is_some_and(|t| t.researches.contains(&tech));
        if !listed {
            return Err(ProdError::NotAvailable);
        }
        if self.production.research.contains_key(&building) {
            return Err(ProdError::Busy);
        }
        // The same tech may be in progress elsewhere.
        if self.production.research.values().any(|r| r.tech == tech) {
            return Err(ProdError::Busy);
        }
        let up = self.production.upgrades.get(&tech).ok_or(ProdError::NotAvailable)?;
        if self.tech_level(owner, tech) >= up.max_level.max(1) {
            return Err(ProdError::MaxLevel);
        }
        Ok(())
    }

    /// Starts `tech` at `building`; the cost is paid now.
    pub fn start_research(&mut self, building: ObjKey, tech: u32) -> Result<(), ProdError> {
        self.can_research(building, tech)?;
        let owner = self.unit(building).owner;
        let level = self.tech_level(owner, tech);
        let up = self.production.upgrades[&tech];
        let (gold, lumber) = (up.gold_for(level), up.lumber_for(level));
        if !self.can_pay(owner, gold, lumber) {
            return Err(ProdError::Resources);
        }
        self.pay(owner, gold, lumber);
        let done_at = self.production.now + up.seconds_for(level) as f32;
        self.production.research.insert(building, Research { tech, done_at, gold, lumber });
        self.production.events.push(ProdEvent::ResearchStarted { building, tech });
        Ok(())
    }

    fn finish_research(&mut self, building: ObjKey) {
        let Some(r) = self.production.research.remove(&building) else { return };
        let owner = self.unit(building).owner;
        let level = self.tech_level(owner, r.tech) + 1;
        if let Some(p) = self.misc.players.get_mut(owner as usize) {
            let t = p.tech.entry(r.tech).or_insert(crate::natives::misc::TechLevel {
                researched: 0,
                max_allowed: i32::MAX,
            });
            t.researched = level;
        }
        self.production.stats.researched += 1;
        self.production.events.push(ProdEvent::Researched { building, tech: r.tech, level });
    }

    // ------------------------------------------------------------------------------------------------------
    // In-place upgrades (town hall tiers)

    /// Whether the building is idle: completed, nothing queued, no research, no upgrade running. The AI orders only
    /// idle buildings (LIKELY: the usable test `0x6F2D5940` skips a building whose queue ability reports busy).
    pub fn building_usable(&self, k: ObjKey) -> bool {
        self.unit_completed(k)
            && !self.production.constructions.contains_key(&k)
            && !self.production.research.contains_key(&k)
            && !self.production.unit_upgrades.contains_key(&k)
            && self.production.queues.get(&k).is_none_or(|q| q.is_empty())
    }

    /// Whether `unit` can upgrade into `to` (its `Upgrade` list) and the owner meets the requirements.
    pub fn can_upgrade_unit(&self, unit: ObjKey, to: u32) -> bool {
        self.building_usable(unit)
            && self
                .production
                .tech
                .get(&self.unit(unit).type_id)
                .is_some_and(|t| t.upgrade_to.contains(&to))
            && self.requirements_met(self.unit(unit).owner, to)
    }

    /// Starts the upgrade of `unit` into `to`. The cost of the target type is paid now.
    pub fn start_unit_upgrade(&mut self, unit: ObjKey, to: u32) -> Result<(), ProdError> {
        if !self.can_upgrade_unit(unit, to) {
            return Err(ProdError::NotAvailable);
        }
        let owner = self.unit(unit).owner;
        let ty = self.unit_types().get(to).ok_or(ProdError::NotAvailable)?;
        let (gold, wood) = (ty.gold_cost, ty.wood_cost);
        if !self.can_pay(owner, gold, wood) {
            return Err(ProdError::Resources);
        }
        self.pay(owner, gold, wood);
        let class = self.type_class(to);
        if let Some(p) = self.player_mut(owner) {
            p.counts.begin_in_progress(to, class, false);
        }
        let seconds = self.production.build_seconds.get(&to).copied().unwrap_or(0) as f32;
        let done_at = self.production.now + seconds;
        self.production.unit_upgrades.insert(unit, UnitUpgrade { to, done_at });
        self.production.events.push(ProdEvent::UnitUpgradeStarted { unit, to });
        Ok(())
    }

    fn finish_unit_upgrade(&mut self, unit: ObjKey) {
        let Some(up) = self.production.unit_upgrades.remove(&unit) else { return };
        if !self.unit_live(unit) {
            return;
        }
        let (owner, old) = {
            let u = self.unit(unit);
            (u.owner, u.type_id)
        };
        let to = up.to;
        let class = self.type_class(to);
        let old_class = self.type_class(old);
        if let Some(p) = self.player_mut(owner) {
            p.counts.finish(to, class, false);
            p.counts.remove_completed(old, old_class);
        }
        self.food_release(unit);
        if let Some(ty) = self.unit_types().get(to).cloned() {
            let u = self.unit_mut(unit);
            let frac = if u.max_life.to_f32() > 0.0 { u.life.to_f32() / u.max_life.to_f32() } else { 1.0 };
            u.type_id = to;
            u.max_life = ty.max_life;
            u.life = fl(ty.max_life.to_f32() * frac);
            u.class_flags = ty.classification;
        }
        self.food_apply_creation(unit);
        self.production.events.push(ProdEvent::UnitUpgraded { unit, to });
    }

    // ------------------------------------------------------------------------------------------------------
    // Harvesting

    /// Whether `unit` can harvest `resource`.
    pub fn can_harvest(&self, unit: ObjKey, resource: Res) -> bool {
        if !self.unit_completed(unit) {
            return false;
        }
        self.production.tech.get(&self.unit(unit).type_id).is_some_and(|t| match resource {
            Res::Gold => t.harvest_gold,
            Res::Lumber => t.harvest_wood,
        })
    }

    /// Starts (or keeps) a harvest cycle of `resource` for `unit`, anchored at `anchor` (the town hall), with `mine` the
    /// gold mine of a gold cycle. A worker already on the same resource keeps its cycle (no new order, CONFIRMED:
    /// `ClearHarvestAI` re-marks the previous workers without ordering them again).
    pub fn assign_harvest(&mut self, unit: ObjKey, resource: Res, anchor: (f32, f32), mine: Option<ObjKey>) {
        if let Some(h) = self.production.harvesters.get_mut(&unit) {
            if h.resource == resource {
                h.anchor = anchor;
                if resource == Res::Gold && mine.is_some() {
                    h.mine = mine;
                }
                return;
            }
        }
        self.production.jobs.remove(&unit);
        let owner = self.unit(unit).owner;
        let mut worker = Worker::new(owner as usize, GOLD_TRIP, LUMBER_TRIP, 1);
        // A load of the other resource is deposited first (CONFIRMED `carry_add` rule, simplified to a deposit now).
        if let Some(old) = self.production.harvesters.remove(&unit) {
            let mut w = old.worker;
            self.deposit_worker(&mut w);
        }
        worker.mining = false;
        self.production.harvesters.insert(
            unit,
            Harvester { resource, phase: Phase::ToSource, worker, mine, tree: None, anchor, retries: 0 },
        );
        self.unit_mut(unit).hidden = false;
        let target = self.source_point(unit);
        if let Some((x, y)) = target {
            self.order_move(unit, x, y);
        }
    }

    /// Whether the worker has a harvest cycle.
    pub fn is_harvesting(&self, unit: ObjKey) -> bool {
        self.production.harvesters.contains_key(&unit)
    }

    fn pick_mine(&self, from: (f32, f32)) -> Option<ObjKey> {
        self.mines
            .iter()
            .filter(|(&k, m)| m.remaining > 0 && self.unit_live(k))
            .map(|(&k, _)| {
                let (x, y) = self.unit_xy(k);
                (k, (x - from.0).powi(2) + (y - from.1).powi(2))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(k, _)| k)
    }

    fn pick_tree(&self, from: (f32, f32)) -> Option<usize> {
        self.production
            .trees
            .iter()
            .enumerate()
            .filter(|(_, t)| t.wood > 0)
            .map(|(i, t)| (i, (t.x - from.0).powi(2) + (t.y - from.1).powi(2)))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(i, _)| i)
    }

    /// The point a harvesting worker walks to (choosing the mine or tree when it has none), None when no source is left.
    fn source_point(&mut self, unit: ObjKey) -> Option<(f32, f32)> {
        let (resource, anchor, mine, tree) = {
            let h = self.production.harvesters.get(&unit)?;
            (h.resource, h.anchor, h.mine, h.tree)
        };
        match resource {
            Res::Gold => {
                let m = mine
                    .filter(|&m| self.mines.get(&m).is_some_and(|x| x.remaining > 0) && self.unit_live(m))
                    .or_else(|| self.pick_mine(anchor))?;
                self.production.harvesters.get_mut(&unit)?.mine = Some(m);
                Some(self.unit_xy(m))
            }
            Res::Lumber => {
                let t = tree
                    .filter(|&i| self.production.trees.get(i).is_some_and(|t| t.wood > 0))
                    .or_else(|| self.pick_tree(anchor))?;
                self.production.harvesters.get_mut(&unit)?.tree = Some(t);
                let tr = self.production.trees[t];
                Some((tr.x, tr.y))
            }
        }
    }

    fn nearest_hall(&self, owner: u32, from: (f32, f32)) -> Option<(ObjKey, f32, f32)> {
        self.unit_objects()
            .filter(|(k, u)| {
                u.owner == owner
                    && !u.dead
                    && !u.removed
                    && u.class_flags & CLASS_TOWNHALL != 0
                    && u.count_state & REGISTERED_IN_PROGRESS == 0
                    && !self.mines.contains_key(k)
            })
            .map(|(k, _)| {
                let (x, y) = self.unit_xy(k);
                (k, x, y, (x - from.0).powi(2) + (y - from.1).powi(2))
            })
            .min_by(|a, b| a.3.total_cmp(&b.3).then(a.0.cmp(&b.0)))
            .map(|(k, x, y, _)| (k, x, y))
    }

    fn step_harvester(&mut self, unit: ObjKey) {
        let now = self.production.now;
        let Some(h) = self.production.harvesters.get(&unit).cloned() else { return };
        let owner = self.unit(unit).owner;
        let pos = self.unit_xy(unit);
        match h.phase {
            Phase::ToSource => {
                let Some((sx, sy)) = self.source_point(unit) else {
                    return;
                };
                let (reach, d) = match h.resource {
                    Res::Gold => (MINE_REACH, ((pos.0 - sx).powi(2) + (pos.1 - sy).powi(2)).sqrt()),
                    Res::Lumber => (TREE_REACH, ((pos.0 - sx).powi(2) + (pos.1 - sy).powi(2)).sqrt()),
                };
                let arrived = d <= reach;
                if !arrived && self.order_idle(unit) {
                    let hh = self.production.harvesters.get_mut(&unit).unwrap();
                    hh.retries += 1;
                    if hh.retries >= RETRY_LIMIT {
                        hh.retries = 0;
                        // Blocked: the cycle goes on as if the worker had arrived (UNSURE).
                    } else {
                        self.order_move(unit, sx, sy);
                        return;
                    }
                } else if !arrived {
                    return;
                }
                let hh = self.production.harvesters.get_mut(&unit).unwrap();
                hh.retries = 0;
                let secs = match h.resource {
                    Res::Gold => MINE_SECONDS,
                    Res::Lumber => HIT_SECONDS * LUMBER_TRIP as f32,
                };
                hh.phase = Phase::Working { until: now + secs };
                if h.resource == Res::Gold {
                    self.unit_mut(unit).hidden = true;
                }
            }
            Phase::Working { until } => {
                if now < until {
                    return;
                }
                let mut worker = h.worker.clone();
                match h.resource {
                    Res::Gold => {
                        worker.mining = true;
                        if let Some(m) = h.mine {
                            if self.gold_trip(&mut worker, m, false).is_some() {
                                self.production.stats.gold_trips += 1;
                            }
                        }
                        worker.mining = false;
                        self.unit_mut(unit).hidden = false;
                    }
                    Res::Lumber => {
                        let mut take = 0;
                        if let Some(i) = h.tree {
                            if let Some(t) = self.production.trees.get_mut(i) {
                                take = t.wood.min(LUMBER_TRIP);
                                t.wood -= take;
                            }
                        }
                        worker.carry_add(&mut self.economy, take, true);
                        self.production.stats.lumber_trips += 1;
                    }
                }
                let hh = self.production.harvesters.get_mut(&unit).unwrap();
                hh.worker = worker;
                hh.phase = Phase::ToHall;
                hh.retries = 0;
                if let Some((_, hx, hy)) = self.nearest_hall(owner, pos) {
                    self.order_move(unit, hx, hy);
                }
            }
            Phase::ToHall => {
                let Some((_, hx, hy)) = self.nearest_hall(owner, pos) else { return };
                let d = ((pos.0 - hx).powi(2) + (pos.1 - hy).powi(2)).sqrt();
                let arrived = d <= HALL_REACH;
                if !arrived && self.order_idle(unit) {
                    let hh = self.production.harvesters.get_mut(&unit).unwrap();
                    hh.retries += 1;
                    if hh.retries < RETRY_LIMIT {
                        self.order_move(unit, hx, hy);
                        return;
                    }
                } else if !arrived {
                    return;
                }
                let mut worker = h.worker.clone();
                let resource = if worker.carrying_lumber { Res::Lumber } else { Res::Gold };
                let credited = self.deposit_worker(&mut worker);
                if credited != 0 {
                    match resource {
                        Res::Gold => self.production.stats.gold_deposited += i64::from(credited),
                        Res::Lumber => self.production.stats.lumber_deposited += i64::from(credited),
                    }
                    self.production.events.push(ProdEvent::Deposit { worker: unit, resource, amount: credited });
                }
                let hh = self.production.harvesters.get_mut(&unit).unwrap();
                hh.worker = worker;
                hh.phase = Phase::ToSource;
                hh.retries = 0;
                if let Some((sx, sy)) = self.source_point(unit) {
                    self.order_move(unit, sx, sy);
                }
            }
        }
    }

    fn step_job(&mut self, builder: ObjKey) {
        let Some(job) = self.production.jobs.get(&builder).copied() else { return };
        if !self.unit_live(builder) {
            self.production.jobs.remove(&builder);
            return;
        }
        match job.phase {
            JobPhase::Moving => {
                let pos = self.unit_xy(builder);
                let d = ((pos.0 - job.x).powi(2) + (pos.1 - job.y).powi(2)).sqrt();
                let arrived = d <= SITE_REACH;
                if !arrived && self.order_idle(builder) {
                    let j = self.production.jobs.get_mut(&builder).unwrap();
                    j.retries += 1;
                    if j.retries < RETRY_LIMIT {
                        self.order_move(builder, job.x, job.y);
                        return;
                    }
                } else if !arrived {
                    return;
                }
                match self.place_building(builder, job) {
                    Ok(site) => {
                        let consumed = self.production.constructions[&site].consumed;
                        if consumed {
                            // The builder becomes the building (night elf wisp, undead acolyte).
                            self.production.jobs.remove(&builder);
                            crate::natives::unit::remove_unit(self, builder);
                        } else if let Some(j) = self.production.jobs.get_mut(&builder) {
                            j.phase = JobPhase::Building { site };
                        }
                    }
                    Err(reason) => {
                        self.production.jobs.remove(&builder);
                        self.production
                            .events
                            .push(ProdEvent::BuildFailed { builder, type_id: job.type_id, reason });
                    }
                }
            }
            JobPhase::Building { site } => {
                if !self.production.constructions.contains_key(&site) {
                    self.production.jobs.remove(&builder);
                }
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------
    // Ticking

    /// The per-tick work: advances the production clock to `now` and fires the due timers (training heads, building
    /// completions, research completions) in tick order. Cheap when nothing is queued.
    pub fn production_tick(&mut self, now: f32) {
        self.production.now = now;
        if !self.production.train_timers.is_empty() {
            loop {
                let fired = self.production.train_timers.pop_due(now);
                let Some(fired) = fired else { break };
                let building = ObjKey(fired.payload.building as u32);
                self.train_head_done(building);
            }
        }
        if !self.production.constructions.is_empty() {
            let done: Vec<ObjKey> = self
                .production
                .constructions
                .iter()
                .filter(|(_, c)| now >= c.start + c.seconds)
                .map(|(&k, _)| k)
                .collect();
            for k in done {
                self.finish_building(k);
            }
        }
        if !self.production.unit_upgrades.is_empty() {
            let done: Vec<ObjKey> = self
                .production
                .unit_upgrades
                .iter()
                .filter(|(_, u)| now >= u.done_at)
                .map(|(&k, _)| k)
                .collect();
            for k in done {
                self.finish_unit_upgrade(k);
            }
        }
        if !self.production.research.is_empty() {
            let done: Vec<ObjKey> = self
                .production
                .research
                .iter()
                .filter(|(_, r)| now >= r.done_at)
                .map(|(&k, _)| k)
                .collect();
            for k in done {
                self.finish_research(k);
            }
        }
    }

    fn train_head_done(&mut self, building: ObjKey) {
        let Some(mut queue) = self.production.queues.remove(&building) else { return };
        if !self.unit_live(building) {
            // The building died: the queue is dropped (the refund is not modelled, UNSURE).
            return;
        }
        let owner = self.unit(building).owner;
        let cap = self.food_cap_of(owner);
        let mut used = self.food_used_of(owner);
        let before = used;
        let mut timers = std::mem::take(&mut self.production.train_timers);
        let outcomes = queue.complete(&mut timers, &mut used, cap, building.0 as usize);
        self.production.train_timers = timers;
        self.production.queues.insert(building, queue);
        if used != before {
            self.food_add_used(owner, used.wrapping_sub(before));
        }
        for o in outcomes {
            if let TrainOutcome::Finished { type_id } = o {
                self.on_train_finished(building, type_id);
            }
        }
    }

    /// The per-batch work (0.1 s): construction growth, build jobs and the harvest cycles.
    pub fn production_batch(&mut self) {
        let p = &self.production;
        if p.constructions.is_empty() && p.jobs.is_empty() && p.harvesters.is_empty() && p.queues.is_empty() {
            return;
        }
        let now = self.production.now;
        // Construction growth: life from 10 % to 100 % (UNSURE law).
        let sites: Vec<(ObjKey, Construction)> =
            self.production.constructions.iter().map(|(&k, &c)| (k, c)).collect();
        for (k, c) in sites {
            let frac = if c.seconds <= 0.0 { 1.0 } else { ((now - c.start) / c.seconds).clamp(0.0, 1.0) };
            let max = self.unit(k).max_life.to_f32();
            let life = match c.ramp {
                Some((start_life, rate)) => {
                    let elapsed = fl((now - c.start).clamp(0.0, c.seconds.max(0.0)));
                    let l = softfloat::arith::add(start_life, softfloat::arith::mul(rate, elapsed));
                    if l.to_f32() > max { fl(max) } else { l }
                }
                None => fl(max * (START_LIFE + (1.0 - START_LIFE) * frac)),
            };
            self.unit_mut(k).life = life;
        }
        let jobs: Vec<ObjKey> = self.production.jobs.keys().copied().collect();
        for b in jobs {
            self.step_job(b);
        }
        let workers: Vec<ObjKey> = self.production.harvesters.keys().copied().collect();
        for w in workers {
            if !self.unit_live(w) {
                self.production.harvesters.remove(&w);
                continue;
            }
            self.step_harvester(w);
        }
        // A blocked head (no food) is retried each batch (the original waits for a food-change notice, LIKELY).
        let blocked: Vec<ObjKey> = self
            .production
            .queues
            .iter()
            .filter(|(_, q)| !q.is_empty() && !q.is_running())
            .map(|(&k, _)| k)
            .collect();
        for b in blocked {
            if !self.unit_live(b) {
                continue;
            }
            let owner = self.unit(b).owner;
            let cap = self.food_cap_of(owner);
            let mut used = self.food_used_of(owner);
            let before = used;
            let mut queue = self.production.queues.remove(&b).unwrap_or_default();
            let mut timers = std::mem::take(&mut self.production.train_timers);
            let _ = queue.retry(&mut timers, &mut used, cap, b.0 as usize);
            self.production.train_timers = timers;
            self.production.queues.insert(b, queue);
            if used != before {
                self.food_add_used(owner, used.wrapping_sub(before));
            }
        }
    }
}

impl Game {
    /// A one-line summary of what `owner` has: stored gold and lumber, food, live units by type name, harvesters,
    /// and the production totals. For the boot report.
    pub fn production_summary(&self, owner: u32) -> String {
        let mut counts: BTreeMap<String, i32> = BTreeMap::new();
        for (_, u) in self.unit_objects() {
            if u.owner == owner && !u.dead && !u.removed {
                let name = self
                    .unit_types()
                    .get(u.type_id)
                    .map_or_else(|| format!("{:08x}", u.type_id), |t| t.name.clone());
                let tag = if u.count_state & REGISTERED_IN_PROGRESS != 0 { format!("{name}*") } else { name };
                *counts.entry(tag).or_default() += 1;
            }
        }
        let e = &self.economy[owner as usize];
        let st = &self.production.stats;
        format!(
            "gold {} lumber {} food {}/{} | all players: gold_h {} wood_h {} deposited gold {} lumber {} trained {} built {} researched {} | {:?}",
            e.stored(Res::Gold) / 10,
            e.stored(Res::Lumber) / 10,
            self.food_used_of(owner),
            self.food_cap_of(owner),
            self.production.harvesting(Res::Gold),
            self.production.harvesting(Res::Lumber),
            st.gold_deposited,
            st.lumber_deposited,
            st.trained,
            st.built,
            st.researched,
            counts
        )
    }
}

impl Game {
    /// The live structures of `owner` as `(type name, x, y, in progress)`, in creation order. For the boot report
    /// (`w3boot --buildings`), to check where the AI places its buildings.
    pub fn building_positions(&self, owner: u32) -> Vec<(String, f32, f32, bool)> {
        let mut out = Vec::new();
        for (k, u) in self.unit_objects() {
            if u.owner != owner || u.dead || u.removed || !self.unit_types().is_building(u.type_id) {
                continue;
            }
            let name = self
                .unit_types()
                .get(u.type_id)
                .map_or_else(|| format!("{:08x}", u.type_id), |t| t.name.clone());
            let (x, y) = self.unit_xy(k);
            out.push((name, x, y, u.count_state & REGISTERED_IN_PROGRESS != 0));
        }
        out
    }
}

/// The engine predicates of the build-spot search, over the model's pathing grid (`SiteHost` of [`crate::ai_site`]).
///
/// * `unwalkable`: the cell's pathing byte has the unwalkable bit (`0x02`) or a tree stands on it (a tree blocks the
///   2 x 2 cells around its position, UNSURE).
/// * `cell_level`: always 0 (the model has no cliff levels yet; UNSURE).
/// * `reachable`: the cell is connected to the centre's cell over unwalkable-free cells, 8-neighbours, a diagonal
///   only past two free orthogonals (the model of the pathing query `0x6F475560`, UNSURE).
/// * `placement_ok`: every texel of the building's pathing texture lies on a buildable, walkable cell that no other
///   structure texture, mine texture or tree covers (the model of `0x6F2A50C0`, UNSURE; the textures that matter
///   are solid over their whole area).
struct SiteHostImpl<'a> {
    grid: &'a BuildGrid,
    /// Cells covered by structures, mines and jobs (their footprint walks).
    occupied: BTreeSet<(i32, i32)>,
    trees: BTreeSet<(i32, i32)>,
    reach: BTreeSet<(i32, i32)>,
    footprint: crate::ai_site::Footprint,
    rot: u32,
}

impl<'a> SiteHostImpl<'a> {
    fn new(
        gm: &'a Game,
        grid: &'a BuildGrid,
        units: &[crate::ai_site::MarkUnit],
        rot: u32,
        center: (F32, F32),
        footprint: crate::ai_site::Footprint,
    ) -> Self {
        let mut me = SiteHostImpl {
            grid,
            occupied: BTreeSet::new(),
            trees: BTreeSet::new(),
            reach: BTreeSet::new(),
            footprint,
            rot,
        };
        for t in gm.production.trees.iter().filter(|t| t.wood > 0) {
            let (cx, cy) = me.cell(t.x, t.y);
            for dy in -1..=0 {
                for dx in -1..=0 {
                    me.trees.insert((cx + dx, cy + dy));
                }
            }
        }
        for u in units {
            let mut cells = Vec::new();
            crate::ai_site::footprint_walk(u.footprint, rot, u.x, u.y, |px, py| {
                cells.push((px.to_f32(), py.to_f32()));
            });
            for (px, py) in cells {
                let c = me.cell(px, py);
                me.occupied.insert(c);
            }
        }
        me.flood(center);
        me
    }

    fn cell(&self, x: f32, y: f32) -> (i32, i32) {
        (((x - self.grid.origin_x) / 32.0).floor() as i32, ((y - self.grid.origin_y) / 32.0).floor() as i32)
    }

    fn walkable_cell(&self, c: (i32, i32)) -> bool {
        c.0 >= 0
            && c.1 >= 0
            && c.0 < self.grid.width
            && c.1 < self.grid.height
            && self.grid.types[(c.1 * self.grid.width + c.0) as usize] & 0x02 == 0
            && !self.trees.contains(&c)
    }

    /// Flood fill of the walkable cells from the centre, inside the search window.
    fn flood(&mut self, center: (F32, F32)) {
        let start = self.cell(center.0.to_f32(), center.1.to_f32());
        let lim = 55;
        let mut stack = vec![start];
        self.reach.insert(start);
        while let Some((x, y)) = stack.pop() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let n = (x + dx, y + dy);
                if (n.0 - start.0).abs() > lim || (n.1 - start.1).abs() > lim || self.reach.contains(&n) {
                    continue;
                }
                if !self.walkable_cell(n) {
                    continue;
                }
                if dx != 0 && dy != 0 && (!self.walkable_cell((x + dx, y)) || !self.walkable_cell((x, y + dy))) {
                    continue;
                }
                self.reach.insert(n);
                stack.push(n);
            }
        }
    }
}

impl crate::ai_site::SiteHost for SiteHostImpl<'_> {
    fn cell_level(&self, _cell_x: i32, _cell_y: i32) -> i32 {
        0
    }

    fn unwalkable(&self, x: F32, y: F32) -> bool {
        !self.walkable_cell(self.cell(x.to_f32(), y.to_f32()))
    }

    fn reachable(&self, _from: (F32, F32), to: (F32, F32)) -> bool {
        self.reach.contains(&self.cell(to.0.to_f32(), to.1.to_f32()))
    }

    fn placement_ok(&self, x: F32, y: F32) -> bool {
        let mut ok = true;
        crate::ai_site::footprint_walk(self.footprint, self.rot, x, y, |px, py| {
            let c = self.cell(px.to_f32(), py.to_f32());
            let in_grid = c.0 >= 0 && c.1 >= 0 && c.0 < self.grid.width && c.1 < self.grid.height;
            if !in_grid
                || self.grid.types[(c.1 * self.grid.width + c.0) as usize] & 0x0A != 0
                || self.trees.contains(&c)
                || self.occupied.contains(&c)
            {
                ok = false;
            }
        });
        ok
    }
}
