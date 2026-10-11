//! Game state behind the timer and trigger natives: an arena of game objects (timers, triggers,
//! boolexprs, trigger conditions and actions, events), the handle table that gives them ids, the
//! timer queue and the context keys of running script threads.
//!
//! Objects are never removed from the arena. A destroyed object keeps its [`ObjKey`] but has no live
//! handle any more, so every native that receives a handle goes through [`Game::handle_object`] and
//! sees `None` for a destroyed or stale id.
//!
//! Context keys: each script thread started by a timer handler or a trigger carries a key (see
//! `jass::vm::NativeCtx::thread_key`). The key names a [`Ctx`] (which timer or which trigger and
//! event the thread runs for), so natives such as `GetTriggeringTrigger` can answer per thread.
//! Keys are interned in creation order (1, 2, ...), so the same `Ctx` always gets the same key.

use std::collections::{BTreeMap, BTreeSet};

use jass::vm::{RunStatus, ThreadId, Value};
use softfloat::F32;

use super::presentation::{PresObj, PresentationState};
use crate::abilities::{Buff, UnitAbilities};
use crate::shop::{StockList, TypeStock};
use crate::handles::HandleTable;
use crate::rng::{RngTable, Streams};
use crate::timer::{TimerId, TimerQueue};

pub use crate::abilities::{AbilityType, AbilityTypes};

/// The shop stock lists of the game and the type data they read. Lists are created on first use for a unit that has
/// the sell ability; the host fills the type data (`stockMax`, `stockRegen` of the item and unit type rows).
#[derive(Debug, Clone, Default)]
pub struct ShopState {
    /// Item shop lists (`Asid`) by unit.
    pub items: BTreeMap<ObjKey, StockList>,
    /// Unit shop lists (`Asud`) by unit.
    pub units: BTreeMap<ObjKey, StockList>,
    /// Stock data of item types; a missing type reads as `stockMax` 0, regen 1.0.
    pub item_stock: BTreeMap<u32, TypeStock>,
    /// Stock data of unit types; a missing type reads as `stockMax` 0, regen 0.0.
    pub unit_stock: BTreeMap<u32, TypeStock>,
}

/// Index of a game object in the arena. The handle table keys objects by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjKey(pub u32);

/// The observer agent of game-level events (game events, game states, global variables). It is not an
/// arena object: its table lives in `Game::observers` under this key (see `event.rs`).
pub const GAME_AGENT: ObjKey = ObjKey(u32::MAX);

/// What a queued timer entry is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// A `TriggerSleepAction`: the script thread resumes at the expiry.
    Sleep(ThreadId),
    /// A timer handle expires: its handler runs and the triggers that listen to it fire.
    Timer(ObjKey),
    /// A timer event registered with `TriggerRegisterTimerEvent` fires its trigger.
    Event(ObjKey),
    /// The duration of a timed life buff ends: the unit dies (`natives/ability.rs`). `serial` names the buff.
    TimedLife { unit: ObjKey, serial: u32 },
    /// An attack timer of the unit's attack object (`combat.rs`).
    Attack {
        unit: ObjKey,
        msg: crate::combat::AttackMsg,
    },
    /// The unit movement walk (message 0x8026A): a repeating timer with period
    /// [`crate::lockstep::MOVE_PERIOD_BITS`] (0.03 s), lockstep.md section 11.2.
    Movement,
    /// The vision refresh (message 0x80269): a repeating timer with period
    /// [`crate::lockstep::VISION_PERIOD_BITS`] (0.4 s), `docs/spec/vision.md` section 11.
    Vision,
    /// A delayed `UnitDamagePoint` comes due (`combat.rs`, the record is kept in the combat system under `id`).
    DamagePoint(u32),
}

/// The timer or trigger that a script thread runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ctx {
    /// A trigger's condition or action chain, with the event that started it (if any) and the unit
    /// of a unit event (`GetTriggerUnit`, `GetDyingUnit`), if any.
    Trigger {
        trigger: ObjKey,
        event: Option<ObjKey>,
        unit: Option<ObjKey>,
        /// Index of the issued-order record of an order event (`GetIssuedOrderId`), if any.
        order: Option<u32>,
        /// Index of the [`super::event::Firing`] whose response slots the getters read, if the event was
        /// fired through `event::fire_event`.
        firing: Option<u32>,
    },
    /// A timer handler started at the timer's expiry.
    Timer(ObjKey),
}

/// A timer handle (`timer` in JASS).
#[derive(Debug, Clone)]
pub struct TimerObj {
    /// The timeout as it was given to `TimerStart` (not clamped). Returned by `TimerGetTimeout`.
    pub timeout: F32,
    pub periodic: bool,
    /// The handler given to `TimerStart`, if any.
    pub handler: Option<u32>,
    /// The queue entry while the timer runs. `None` when paused, expired or never started.
    pub active: Option<TimerId>,
    /// Seconds left while paused (and the value a `ResumeTimer` restarts with).
    pub paused_remaining: F32,
    /// Triggers registered with `TriggerRegisterTimerExpireEvent`: `(trigger, event)`, in
    /// registration order.
    pub listeners: Vec<(ObjKey, ObjKey)>,
}

impl Default for TimerObj {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerObj {
    pub fn new() -> Self {
        TimerObj {
            timeout: F32::ZERO,
            periodic: false,
            handler: None,
            active: None,
            paused_remaining: F32::ZERO,
            listeners: Vec::new(),
        }
    }
}

/// A `trigger` handle with its lists.
#[derive(Debug, Clone)]
pub struct TriggerObj {
    /// Set by `EnableTrigger` and `DisableTrigger`. New triggers are enabled.
    pub enabled: bool,
    /// Storage for `TriggerWaitOnSleeps`. It does not change the execution path yet.
    pub wait_on_sleeps: bool,
    /// `TriggerCondition` objects, in the order they were added.
    pub conditions: Vec<ObjKey>,
    /// `TriggerAction` objects, in the order they were added.
    pub actions: Vec<ObjKey>,
    /// `Event` objects registered on this trigger.
    pub events: Vec<ObjKey>,
    /// Evaluation count (`GetTriggerEvalCount`). Wraps like a JASS integer.
    pub evals: i32,
    /// Execution count (`GetTriggerExecCount`). Wraps like a JASS integer.
    pub execs: i32,
}

impl Default for TriggerObj {
    fn default() -> Self {
        Self::new()
    }
}

impl TriggerObj {
    pub fn new() -> Self {
        TriggerObj {
            enabled: true,
            wait_on_sleeps: false,
            conditions: Vec::new(),
            actions: Vec::new(),
            events: Vec::new(),
            evals: 0,
            execs: 0,
        }
    }
}

/// The kind of a `boolexpr` (also the `conditionfunc` and `filterfunc` handle types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolExpr {
    /// `Condition(code)` or `Filter(code)`: calls the code, which returns a boolean.
    Leaf(u32),
    /// `And(a, b)`.
    And(ObjKey, ObjKey),
    /// `Or(a, b)`.
    Or(ObjKey, ObjKey),
    /// `Not(a)`.
    Not(ObjKey),
}

/// A condition of a trigger: one `boolexpr`, referenced by the trigger.
#[derive(Debug, Clone, Copy)]
pub struct Condition {
    pub trigger: ObjKey,
    pub boolexpr: ObjKey,
}

/// An action of a trigger: one function, run by the trigger's action chain.
#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub trigger: ObjKey,
    pub code: u32,
}

/// Where an event comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    /// `TriggerRegisterTimerEvent`: a timer of its own, which the queue re-arms when periodic.
    Timer(Option<TimerId>),
    /// `TriggerRegisterTimerExpireEvent`: fires when this timer handle expires.
    Expire(ObjKey),
    /// `TriggerRegisterUnitEvent`: fires for the unit whose observer table holds it. `key` is the
    /// registration key (event id + 0x80200).
    UnitEvent { key: u32 },
    /// `TriggerRegisterPlayerUnitEvent`: fires for units of player slot `player`. `key` is the
    /// registration key (event id + 0x80200).
    PlayerUnitEvent { key: u32, player: u32 },
    /// `TriggerRegisterEnterRegion` / `TriggerRegisterLeaveRegion`: fires for a unit that crosses the
    /// edge of `region`. `key` is the registration key (`0x80205` enter, `0x80206` leave).
    RegionEvent { key: u32, region: ObjKey },
    /// `TriggerRegisterDeathEvent` (`natives/destructable.rs`): fires when the widget `widget` dies. `key`
    /// is the registration key (`0x80259`). The widget is what `GetTriggerDestructable` returns.
    WidgetDeath { key: u32, widget: ObjKey },
    /// A keyed event with no extra matching rule (`natives/event.rs`): game events, filtered unit events,
    /// alliance changes, dialogs, buttons and trackables. `value` is the registration argument that the
    /// rule of the event uses (the alliance type for alliance changes, 0 otherwise).
    Keyed { key: u32, value: i32 },
    /// A player or unit state event (`TriggerRegisterPlayerStateEvent`, `TriggerRegisterUnitStateEvent`).
    /// The operator is `key - 0x80101`. It fires for state `state` when the new value compares to `limit`.
    Limit { key: u32, state: u32, limit: F32 },
    /// `TriggerRegisterPlayerChatEvent`: `text` indexes [`Game::strings`]; `exact` selects the match rule.
    Chat { key: u32, exact: bool, text: u32 },
    /// `TriggerRegisterUnitInRange`: fires for a unit that comes within `range` of the registering unit.
    InRange { key: u32, range: F32 },
    /// `TriggerRegisterVariableEvent`: fires when the global `name` (index into [`Game::strings`]) changes and
    /// the new value compares to `limit` with the operator `op` (`0..=5`, the JASS limit operators).
    Variable { name: u32, op: u32, limit: F32 },
}

/// A destructable type as the simulation needs it: the template values a new destructable starts with.
///
/// The fields follow the destructable record offsets listed in `natives/destructable.rs`. Only the fields the
/// natives read are here. TODO(gamedata): the engine should fill the registry from
/// the destructable rows of the SLK data (`crates/gamedata`) at map load. That parsing is not implemented.
#[derive(Debug, Clone, PartialEq)]
pub struct DestructableType {
    /// Maximum life. A new destructable starts at this value (LIKELY: the record's max life field, `+0x90`).
    pub max_life: F32,
    /// Number of variations (`+0x58`). A requested variation at or above this count is clamped to the last
    /// one (CONFIRMED structure, see `natives/destructable.rs`).
    pub variations: u32,
}

/// The destructable type registry, keyed by the raw type id (the four-character code as an integer).
///
/// The host fills it. TODO(gamedata): the engine should load it from the destructable rows of the SLK data at
/// map load. Tests insert synthetic types.
#[derive(Debug, Clone, Default)]
pub struct DestructableTypes {
    by_id: BTreeMap<u32, DestructableType>,
}

impl DestructableTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces the type `id`. Returns the type it replaced.
    pub fn insert(&mut self, id: u32, ty: DestructableType) -> Option<DestructableType> {
        self.by_id.insert(id, ty)
    }

    /// The type with raw id `id`.
    pub fn get(&self, id: u32) -> Option<&DestructableType> {
        self.by_id.get(&id)
    }

    /// Number of registered types.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether no type is registered.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// A destructable (`destructable` in JASS, a widget with a handle). Its position, facing, scale and life are
/// stored as the natives see them; the original keeps them in sub-objects (see `natives/destructable.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct DestructableObj {
    /// The raw type id (`GetDestructableTypeId`, object field `+0x30`).
    pub type_id: u32,
    pub x: F32,
    pub y: F32,
    /// Height. `CreateDestructable` takes it from the terrain (not modelled: 0.0 there, see
    /// `natives/destructable.rs`); `CreateDestructableZ` stores the given value.
    pub z: F32,
    /// Facing in radians (the degree argument times pi/180, as for units).
    pub facing: F32,
    /// Scale, clamped to `[0.1, 10.0]` at creation.
    pub scale: F32,
    /// The variation index after the clamp.
    pub variation: u32,
    /// Current life. Never negative: a life at or below zero is stored as `+0.0`.
    pub life: F32,
    pub max_life: F32,
    /// Occluder height (`GetDestructableOccluderHeight`, object `+0x40`, default 0.0).
    pub occluder_height: F32,
    /// Set while the life is zero or below (the original's `IsDead` test, `+0x13C` slot): the corpse state.
    pub dead: bool,
    /// Set by `ShowDestructable(false)`, and by `RemoveDestructable`.
    pub hidden: bool,
    /// The invulnerable bit (object flag word `+0x20`, bit 3). Stored only: no damage path reads it yet.
    pub invulnerable: bool,
    /// Set by `RemoveDestructable`. A removed destructable has no live handle for the natives.
    pub removed: bool,
    /// Whether the destructable currently blocks pathing. Set while it is alive, cleared by its death or
    /// removal. The pathing write itself is not implemented (see `natives/destructable.rs`, the hook).
    /// The field is kept so that the hook has its state when the pathing map is written.
    pub blocks_pathing: bool,
}

/// A trigger event (`event` in JASS).
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub trigger: ObjKey,
    pub source: EventSource,
}

/// A handle type the simulation models. Typed natives (`SaveTimerHandle`, `LoadLocationHandle`, ...)
/// accept a handle only when its object has this kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    Timer,
    Trigger,
    BoolExpr,
    Event,
    Condition,
    Action,
    Location,
    Rect,
    /// A region (see [`RegionObj`]).
    Region,
    Hashtable,
    GameCache,
    Player,
    Force,
    /// A unit (see [`UnitObj`]).
    Unit,
    /// A unit group (see [`GroupObj`]).
    Group,
    /// An item (see [`ItemObj`]).
    Item,
    /// A destructable (see [`DestructableObj`]).
    Destructable,
    /// A fog modifier (see `natives/fog.rs`).
    FogModifier,
    /// Presentation handle types (see `presentation.rs`): camera setups, sounds, effects, quests,
    /// quest items, multiboards and their items, leaderboards, dialogs, buttons and timer dialogs.
    CameraSetup,
    Sound,
    Effect,
    Quest,
    QuestItem,
    Multiboard,
    MultiboardItem,
    Leaderboard,
    Dialog,
    Button,
    TimerDialog,
    /// A unit pool (`unitpool`, see `natives/pool.rs`).
    UnitPool,
    /// An item pool (`itempool`, see `natives/pool.rs`).
    ItemPool,
    /// A defeat condition (see `natives/defeat.rs`).
    DefeatCondition,
    /// A trackable (see `natives/defeat.rs`).
    Trackable,
}

/// A `location` handle: a point with raw coordinates (no clamping, no terrain height).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationObj {
    pub x: F32,
    pub y: F32,
}

/// A `rect` handle. The four edges are stored raw; `SetRect` and friends sort and clamp them first
/// (see `natives/location.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectObj {
    pub min_x: F32,
    pub min_y: F32,
    pub max_x: F32,
    pub max_y: F32,
}

/// A `region` handle: the set of 32-unit cells it covers (see `natives/region.rs`). A cell is
/// `(x, y)` relative to the playable area's origin; a cell in the set is inside the region.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionObj {
    pub cells: BTreeSet<(i32, i32)>,
}

/// A unit type as the simulation needs it: the template values that a new unit starts with.
///
/// The attribute fields follow the unit-type record offsets read by the attribute natives (see
/// `natives/unit_attr.rs`). They are plain values here; TODO(gamedata): the engine fills them from the unit rows
/// of the SLK data at map load, like the other fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitType {
    /// The name that `CreateUnitByName` looks up.
    pub name: String,
    /// Maximum life. A new unit starts at this value (LIKELY: the original takes it from the template).
    pub max_life: F32,
    /// Maximum mana.
    pub max_mana: F32,
    /// Mana of a new unit (UNSURE: the original copies the template's starting mana).
    pub start_mana: F32,
    /// Number of item slots (`UnitInventorySize`). 0 for a unit without an inventory: the original has
    /// no pack object then, and every pack native fails (`natives/item.rs`).
    pub inventory_size: u32,
    /// Default move speed (record `+0x1D4`). A new unit starts at this value (LIKELY).
    pub move_speed: F32,
    /// Default turn speed (record `+0x1E0`).
    pub turn_speed: F32,
    /// Default prop window (record `+0x1E4`).
    pub prop_window: F32,
    /// Default fly height (record `+0x1F8`).
    pub fly_height: F32,
    /// Default acquire range (record `+0x194`).
    pub acquire_range: F32,
    /// Point value (record `+0x1D0`), read by `GetUnitPointValue`.
    pub point_value: i32,
    /// Food used (record `+0x5C`), read by `GetUnitFoodUsed`.
    pub food_used: i32,
    /// Food made (record `+0x60`), read by `GetUnitFoodMade`.
    pub food_made: i32,
    /// The abilities a new unit gets, in list order, at level 1 (LIKELY). TODO(gamedata): from the type's ability
    /// list.
    pub abilities: Vec<u32>,
    /// Gold cost, read by `GetUnitGoldCost` (CONFIRMED: the cost native returns the type record's gold field, `+0x20`
    /// of the type object, through `FUN_6f29b800`). A registry input: the host fills it from the unit data.
    pub gold_cost: i32,
    /// Wood cost, read by `GetUnitWoodCost` (CONFIRMED: the type record's wood field, `+0x24`, `FUN_6f29b830`).
    pub wood_cost: i32,
    /// Sight radius (record `+0x18C`, UnitBalance `sight`), the day sight of a new unit. 0 when the host has not
    /// filled it (TODO(gamedata): the engine loads it from the unit rows).
    pub sight: F32,
    /// Night sight radius (record `+0x190`, UnitBalance `nsight`). 0 when not filled.
    pub night_sight: F32,
    /// Classification bits (record `+0x1C8`, UnitBalance `type`), copied to the unit's `+0x248` at creation.
    pub classification: u32,
    /// Movement type bits (record `+0x1A8`, `movetp`), copied to the unit's `+0x1FC` at creation.
    pub movement: u32,
    /// `weapsOn` (bit 0 weapon 1, bit 1 weapon 2).
    pub weapons_on: u32,
    /// `weapTp` enum per weapon (record `+0xF8`).
    pub weapon_kind: [u32; 2],
    /// Target flag word per weapon (record `+0xA0`).
    pub weapon_targs: [u32; 2],
}

/// The unit type registry, keyed by the raw type id (the four-character code as an integer).
///
/// The host fills it. TODO(gamedata): the engine should load it from the unit rows of the SLK data
/// (`crates/gamedata`) at map load. That parsing is not implemented here; tests insert synthetic types.
#[derive(Debug, Clone, Default)]
pub struct UnitTypes {
    by_id: BTreeMap<u32, UnitType>,
    by_name: BTreeMap<String, u32>,
    /// Types with the `isbldg` column set (LIKELY: counted as structures, see `unit_counts.rs`). A side table so that
    /// [`UnitType`] keeps its fields. Filled by the host.
    buildings: BTreeSet<u32>,
    /// The typed-count lists of each type: (first list, upgrade list). UNSURE: the data columns (see
    /// `unit_counts::TypeLinks`). Filled by the host.
    count_links: BTreeMap<u32, (Vec<u32>, Vec<u32>)>,
    /// The fields the random creep and neutral building picks test, per type, with the types in the order the original
    /// inserted them into its global list (UnitUI row order, verified by `tools/emu/unit_order_growth.py`). Filled by
    /// the host with [`UnitTypes::set_pick_record`].
    picks: Vec<(u32, PickRecord)>,
}

/// The unit type fields that `ChooseRandomCreep` (`0x6F29FD90`) and `ChooseRandomNPBuilding` (`0x6F294740`) test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickRecord {
    /// The first byte of the raw code is an uppercase ASCII letter (the creep walk skips those).
    pub code_upper_first: bool,
    /// The race enum (record `+0x1C0`): creeps 8, `other` 7.
    pub race: i32,
    /// `level` (record `+0x68`).
    pub level: i32,
    /// `isbldg` (record `+0x28C`).
    pub is_building: bool,
    /// `special` (record `+0x290`).
    pub special: i32,
    /// `hostilePal` (record `+0x294`).
    pub hostile_pal: i32,
    /// `nbrandom` (record `+0x298`).
    pub nb_random: i32,
    /// The tilesets text (record `+0x29C`).
    pub tilesets: String,
}

impl crate::unit_counts::TypeLinks for UnitTypes {
    fn children(&self, type_id: u32) -> &[u32] {
        self.count_links.get(&type_id).map(|l| l.0.as_slice()).unwrap_or(&[])
    }
    fn upgrades(&self, type_id: u32) -> &[u32] {
        self.count_links.get(&type_id).map(|l| l.1.as_slice()).unwrap_or(&[])
    }
}

impl UnitTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces the type `id`. Returns the type it replaced. The name index follows the new name.
    pub fn insert(&mut self, id: u32, ty: UnitType) -> Option<UnitType> {
        if let Some(old) = self.by_id.get(&id) {
            if self.by_name.get(&old.name) == Some(&id) {
                self.by_name.remove(&old.name);
            }
        }
        self.by_name.insert(ty.name.clone(), id);
        self.by_id.insert(id, ty)
    }

    /// Registers the pick fields of type `id`. The first call for an id appends it to the pick order; a later call
    /// replaces the fields and keeps the position (the original inserts a record only when its code is absent).
    pub fn set_pick_record(&mut self, id: u32, record: PickRecord) {
        match self.picks.iter_mut().find(|(i, _)| *i == id) {
            Some(entry) => entry.1 = record,
            None => self.picks.push((id, record)),
        }
    }

    /// The pick records in insertion order.
    pub fn pick_records(&self) -> &[(u32, PickRecord)] {
        &self.picks
    }

    /// Sets whether type `id` is a building (the `isbldg` column). Counted in the structure pair of the counters.
    pub fn set_building(&mut self, id: u32, is_building: bool) {
        if is_building {
            self.buildings.insert(id);
        } else {
            self.buildings.remove(&id);
        }
    }

    /// Whether type `id` is a building (false for an unknown type).
    pub fn is_building(&self, id: u32) -> bool {
        self.buildings.contains(&id)
    }

    /// Sets the typed-count lists of type `id`: `children` (the first list) and `upgrades` (the second list).
    pub fn set_count_links(&mut self, id: u32, children: Vec<u32>, upgrades: Vec<u32>) {
        self.count_links.insert(id, (children, upgrades));
    }

    /// The type with raw id `id`.
    pub fn get(&self, id: u32) -> Option<&UnitType> {
        self.by_id.get(&id)
    }

    /// The raw id of the type named `name` (exact, case-sensitive match: UNSURE, see
    /// `natives/unit.rs`).
    pub fn id_by_name(&self, name: &str) -> Option<u32> {
        self.by_name.get(name).copied()
    }

    /// Number of registered types.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether no type is registered.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// A unit (`unit` in JASS), an agent with a handle.
///
/// Position, facing and the stat values are stored as the natives see them. The original keeps them in
/// sub-objects (movement and stats, see `docs/spec/unit-core.md` section 4); this model does not.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitObj {
    /// The raw type id (`GetUnitTypeId`).
    pub type_id: u32,
    /// The owning player's slot index, 0 to 15 (the original's owner index at `+0x58`).
    pub owner: u32,
    /// Cell coordinates of the position (unit+0x78 / +0x7C, `docs/spec/unit-position.md`). Written only by
    /// [`Game::store_unit_position`].
    pub cell_x: F32,
    pub cell_y: F32,
    /// World x, cached: the normaliser's output for `cell_x` at the current origin (what `GetUnitX` returns).
    /// Written only through [`Game::store_unit_position`] (or [`Game::refresh_unit_positions`]).
    pub x: F32,
    /// World y, cached as `x` (see there).
    pub y: F32,
    /// Facing in radians, as the mover stores it. The natives convert to and from degrees.
    pub facing: F32,
    /// Current life. Never negative: a life at or below zero is stored as `+0.0`.
    pub life: F32,
    pub max_life: F32,
    /// Current mana. Stored as given (no clamp in the original's mana path).
    pub mana: F32,
    pub max_mana: F32,
    /// Set when life reaches zero (through `KillUnit` or a life change to zero). The unit stays in the
    /// game and keeps its handle; nothing here revives it.
    pub dead: bool,
    /// Set by `ShowUnit(false)`.
    pub hidden: bool,
    /// Set by `PauseUnit`. The effect of the pause is not modelled yet (UNSURE, see the spec).
    pub paused: bool,
    /// Set by `RemoveUnit`. A removed unit has no live handle, so every native treats it as invalid.
    pub removed: bool,
    /// Move speed (`SetUnitMoveSpeed`). Stored as given (UNSURE: the original's mover may clamp it).
    pub move_speed: F32,
    /// Turn speed (`SetUnitTurnSpeed`, the mover's field at object `+0xB8`).
    pub turn_speed: F32,
    /// Prop window (`SetUnitPropWindow`, object `+0xBC`).
    pub prop_window: F32,
    /// Fly height (object `+0x208`). Set by `SetUnitFlyHeight`, which keeps a non-positive value at 0.01.
    pub fly_height: F32,
    /// Acquire range (`SetUnitAcquireRange`, read through the acquisition sub-object).
    pub acquire_range: F32,
    /// Scale (`SetUnitScale`). Only the first scale value is stored (see `natives/unit_attr.rs`).
    pub scale: F32,
    /// Time scale (`SetUnitTimeScale`, object `+0x300`).
    pub time_scale: F32,
    /// Vertex colour as `0xAARRGGBB` (`SetUnitVertexColor`, object `+0x2D4`).
    pub vertex_color: u32,
    /// The user data integer (`SetUnitUserData`, object `+0x27C`).
    pub user_data: i32,
    /// Creep guard flag (`SetUnitCreepGuard`, object `+0x60` bit 2).
    pub creep_guard: bool,
    /// What the unit adds to the owner's counters: 0, `REGISTERED_IN_PROGRESS` or `REGISTERED_COMPLETED`
    /// (`unit+0x64` bits 0x8 and 0x10 in the original; see `unit_counts.rs`).
    pub count_state: u32,
    /// Exploded flag (`SetUnitExploded`, object `+0x5C` bit 13).
    pub exploded: bool,
    /// Illusion flag (object `+0x5C` bit 30). Read by `IsUnitIllusion`; no native sets it yet.
    pub illusion: bool,
    /// Hero armor bonus, the total of the derived armor (`heroes::DerivedBonuses::armor`, section 10 of
    /// `docs/spec/heroes.md`). Added to the type armor by `combat::unit_defense`. Zero for non-heroes.
    pub hero_armor: F32,
    /// Hero life regeneration bonus (total, stored only: the simulation has no regeneration tick, LIKELY).
    pub hero_regen: F32,
    /// Hero attack speed bonus (total, stored only: its use in the attack cycle is not decoded, section 10).
    pub hero_attack_speed: F32,
    /// Hero attack damage bonus (total). Added to the dice bonus of the attacker's strikes by `damage::SpecDamage`.
    pub hero_attack: F32,
    /// Day sight radius (object `+0x18C`), copied from the type at creation (see [`UnitType::sight`]).
    pub sight: F32,
    /// Night sight radius (object `+0x190`).
    pub night_sight: F32,
    /// Players that the unit is revealed to (object `+0x148`, one bit per slot; detection and reveal). Read by the
    /// visibility queries (`natives/fog.rs`, `unit_fog_test`).
    pub revealed: u16,
    /// Extra vision bits of the unit (object `+0x14C`): players whose vision the unit's sight also gives (UNSURE what
    /// sets them; read by the refresh).
    pub vision_extra: u16,
    /// Loaded flag (object `+0x5C` bit 4). Read by `IsUnitLoaded`; no transport system sets it yet.
    pub loaded: bool,
    /// The classification word (object `+0x248`): the type's `type` bits at creation, changed by `UnitAddType` /
    /// `UnitRemoveType`. Read by `IsUnitType` (see `crate::unitclass`).
    pub class_flags: u32,
    /// Alternate icon flag (object `+0x280` bit 0x200, `UnitSetUsesAltIcon`); presentation only.
    pub alt_icon: bool,
    /// The food bits of the registration word (object `+0x64`): `0x300` = the type's food used is applied to the
    /// owner, `0xC00` = the food made is applied (see `natives/lane_f.rs`). Set at creation (flags `0x502`).
    pub food_reg: u32,
    /// Ethereal counter (object `+0x158`): `IsUnitType(ETHEREAL)` is `> 0`. No writer in the model yet.
    pub ethereal: i32,
    /// Magic immunity counter (object `+0x15C`): `IsUnitType(MAGIC_IMMUNE)` is `> 0`. No writer in the model yet.
    pub magic_immune: i32,
    /// The unit's abilities: the fixed slots and the ability list (see [`crate::abilities`]).
    pub abilities: UnitAbilities,
    /// The unit's buffs in the order they were added (see [`crate::abilities`]).
    pub buffs: Vec<Buff>,
    /// Hero state, present for a hero type (see `natives/hero.rs`).
    pub hero: Option<HeroObj>,
}

/// The hero state of a unit (the hero sub-object of the original, reached through `+0x1F0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroObj {
    /// The hero level. A new hero starts at 1 (LIKELY).
    pub level: i32,
    /// Experience points (`+0x8C`).
    pub xp: i32,
    /// Unspent skill points (`+0x90`).
    pub skill_points: i32,
    /// Set by `SuspendHeroXP` (`+0xEC`): experience is not gained while set (LIKELY).
    pub xp_suspended: bool,
    /// Base strength (`+0x94`) and per-level strength gain (`+0xD4`, real), see `crate::heroes::attribute`.
    pub str_base: i32,
    pub str_plus: softfloat::F32,
    /// Base agility (`+0xA8`) and per-level agility gain (`+0xE4`, real).
    pub agi_base: i32,
    pub agi_plus: softfloat::F32,
    /// Intelligence base (the counter behind the reference at `+0x7C`, written from type `+0x28` at creation) and
    /// its per-level gain (`+0xDC`, real), see `crate::heroes::intelligence` (`docs/spec/heroes.md`, section 6).
    pub int_base: i32,
    pub int_plus: softfloat::F32,
    /// The primary attribute (`+0xCC`): `1` strength, `2` intelligence, otherwise agility. Read by the attack bonus
    /// (`crate::heroes::primary_total`). The source in the unit record is UNSURE; the default is agility.
    pub primary: i32,
}

impl Default for HeroObj {
    fn default() -> Self {
        HeroObj {
            level: 1,
            xp: 0,
            skill_points: 0,
            xp_suspended: false,
            str_base: 0,
            str_plus: softfloat::F32::ZERO,
            agi_base: 0,
            agi_plus: softfloat::F32::ZERO,
            int_base: 0,
            int_plus: softfloat::F32::ZERO,
            primary: 0,
        }
    }
}

/// An item type as the simulation needs it: the template values a new item starts with, and the record
/// fields the random item selection reads (`natives/item.rs`).
///
/// The field names follow the item-type record offsets of `docs/spec/items.md` section 3.4. The meaning of
/// the flag word bits is in [`item_flags`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemType {
    /// Class of the type (record `+0x3C`). `ChooseRandomItemEx` compares its category with it.
    pub class: u32,
    /// Level read by `ChooseRandomItemEx` when its category is not 7 (record `+0x38`).
    pub level_ex: i32,
    /// Level read by `ChooseRandomItem`, and by `ChooseRandomItemEx` with category 7 (record `+0x40`).
    pub level_plain: i32,
    /// Record `+0x48`. The random selection requires it to be nonzero (its flag5 is 0 in both natives).
    pub flag_48: u32,
    /// Charges a new item starts with (template value copied to `+0x84`).
    pub charges: i32,
    /// Record `+0x4C`: the type is a powerup (`IsItemIdPowerup`).
    pub powerup: bool,
    /// Record `+0x50`: the type can be sold (`IsItemIdSellable`).
    pub sellable: bool,
    /// Record `+0x54`: the type can be pawned (`IsItemIdPawnable`).
    pub pawnable: bool,
    /// Flag bits a new item starts with (the type-record bits of `+0x20`, see [`item_flags`]).
    pub flags: u32,
}

/// The item type registry, keyed by the raw type id. Its candidate order is the registration order (the
/// first `insert` of an id fixes its position). The original's list order is LIKELY load-history based
/// (`docs/spec/items.md` 9.3, UNSURE), so this order is a model, not a verified one.
///
/// TODO(gamedata): the engine should load it from the item rows of the SLK data (`crates/gamedata`) at map
/// load, like [`UnitTypes`]. Tests insert synthetic types.
#[derive(Debug, Clone, Default)]
pub struct ItemTypes {
    order: Vec<u32>,
    by_id: BTreeMap<u32, ItemType>,
}

impl ItemTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces the type `id`. A new id goes last in the registration order; a replaced id keeps its
    /// position. Returns the type it replaced.
    pub fn insert(&mut self, id: u32, ty: ItemType) -> Option<ItemType> {
        if !self.by_id.contains_key(&id) {
            self.order.push(id);
        }
        self.by_id.insert(id, ty)
    }

    /// The type with raw id `id`.
    pub fn get(&self, id: u32) -> Option<&ItemType> {
        self.by_id.get(&id)
    }

    /// The registered ids in registration order.
    pub fn ids(&self) -> &[u32] {
        &self.order
    }

    /// Number of registered types.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether no type is registered.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Bits of the item flag word (`+0x20`, `docs/spec/items.md` section 2). Only the bits the natives use are
/// named. Bit 0 is the hidden bit that `SetItemVisible(false)` and `RemoveItem` set (the spec's reading).
pub mod item_flags {
    /// Hidden (`IsItemVisible` is its negation).
    pub const HIDDEN: u32 = 0x001;
    /// Invulnerable (`SetItemInvulnerable`).
    pub const INVULNERABLE: u32 = 0x008;
    /// Dropped on the owner's death (`SetItemDropOnDeath`).
    pub const DROP_ON_DEATH: u32 = 0x080;
    /// Set by `UnitAddItemById` on the item it creates. Its meaning is not known (UNSURE).
    pub const ADDED_BY_ID: u32 = 0x100;
    /// Droppable (`SetItemDroppable`).
    pub const DROPPABLE: u32 = 0x400;
    /// Powerup: consumed when a unit takes it, never placed in a slot (`IsItemPowerup`).
    pub const POWERUP: u32 = 0x800;
    /// No charges. Set when the charges are 0 or below (`SetItemCharges`).
    pub const NO_CHARGES: u32 = 0x1000;
    /// Pawnable (`SetItemPawnable`, `IsItemPawnable`).
    pub const PAWNABLE: u32 = 0x2000;
}

/// An item (`item` in JASS), an object with a handle. Its position is raw, as the natives see it (the
/// original keeps it in a sub-object, `docs/spec/items.md` section 2).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemObj {
    /// The raw type id (`GetItemTypeId`).
    pub type_id: u32,
    pub x: F32,
    pub y: F32,
    /// The flag word (see [`item_flags`]).
    pub flags: u32,
    /// Charges (`GetItemCharges`). Stored as given by `SetItemCharges` (0 when it was 0 or below).
    pub charges: i32,
    /// The owning player's slot index (`+0x74`). A new item starts at 0 (UNSURE: the original copies it from
    /// the type record).
    pub owner: u32,
    /// The unit whose pack holds the item, or `None` while the item is on the ground.
    pub holder: Option<ObjKey>,
    /// The user data integer (`SetItemUserData`, object `+0x80`).
    pub user_data: i32,
    /// The drop id (`SetItemDropID`, object `+0xCC`). Stored only: no drop system reads it yet.
    pub drop_id: i32,
    /// Set by `RemoveItem` and by a powerup consumed on pickup. A removed item has no live handle for the
    /// natives, although its id is released only when its reference is dropped (see `natives/item.rs`).
    pub removed: bool,
}

/// What an item order asks a unit to do with its item (the order system is not modelled, so the order is
/// only recorded: see [`Game::item_orders`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ItemOrderKind {
    /// `UnitDropItemPoint`: order id `0xD0021`, to the point `(x, y)`.
    DropPoint { x: F32, y: F32 },
    /// `UnitDropItemSlot`: order id `0xD0022 + slot`.
    DropSlot { slot: u32 },
    /// `UnitDropItemTarget`: the target order (its id is not read, UNSURE). The target is the handle id of
    /// the widget argument.
    DropTarget { target: u32 },
    /// `UnitUseItem`: an immediate order whose id is the item's ability order (`0xD0028` + the ability order
    /// index, data driven: the item ability tables are not modelled, so the id is not stored).
    UseImmediate,
    /// `UnitUseItemPoint`: a point order of the item's ability, to `(x, y)`.
    UsePoint { x: F32, y: F32 },
    /// `UnitUseItemTarget`: a target order of the item's ability. The target is the widget's handle id.
    UseTarget { target: u32 },
}

impl ItemOrderKind {
    /// The order id the original queues for this kind: `0xD0021` for a point drop, `0xD0022 + slot` for a
    /// slot drop (`docs/spec/items.md` 5.6). `None` for the target form, whose id is not read.
    pub fn order_id(&self) -> Option<u32> {
        match *self {
            ItemOrderKind::DropPoint { .. } => Some(0x000D_0021),
            ItemOrderKind::DropSlot { slot } => Some(0x000D_0022 + slot),
            ItemOrderKind::DropTarget { .. }
            | ItemOrderKind::UseImmediate
            | ItemOrderKind::UsePoint { .. }
            | ItemOrderKind::UseTarget { .. } => None,
        }
    }
}

/// An item order a native queued, in the order the scripts issued it. The natives return the value the
/// original returns when the order is queued (`true`), so the move itself happens later, in the order system
/// that is not implemented yet (`docs/spec/items.md` 5.6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemOrder {
    pub unit: ObjKey,
    pub item: ObjKey,
    pub kind: ItemOrderKind,
}

/// A unit event, in the order the natives produced it. Drained by the host with
/// [`Game::take_unit_events`]. The queue is plain data, so its order is deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitEvent {
    /// The unit was removed (its removed flag was set). Comes first in the removal sequence.
    Removed(ObjKey),
    /// The unit's life reached zero. Recorded by `KillUnit` and by any life change to zero of a unit that
    /// was alive. The original fires the life-changed event (`Notify`) for the same change; whether its
    /// handler is what detects the death is UNSURE (see the spec, section 11).
    Death(ObjKey),
    /// The owner changed from `from` to `to` (player slot indices). Not recorded when they are equal.
    OwnerChanged { unit: ObjKey, from: u32, to: u32 },
    /// A CEvent with id `id` is fired on the unit (`0xD01E6` for a life change, or a removal-sequence id).
    Notify { unit: ObjKey, id: u32 },
}

/// A reference held by a saved handle value. `owned` is the original's entry flag: when it is set the
/// table holds one reference to `id` (taken with `add_ref`, dropped with `release`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedHandle {
    pub id: u32,
    pub owned: bool,
}

/// The saved values of one hashtable or game cache. Every value kind has its own map, keyed by the
/// pair (parent key, child key). Missing keys read as the default of the kind (0, 0.0, false, "").
#[derive(Debug, Clone)]
pub struct Slots<P: Ord + Clone, C: Ord + Clone> {
    pub ints: BTreeMap<(P, C), i32>,
    pub reals: BTreeMap<(P, C), F32>,
    pub bools: BTreeMap<(P, C), bool>,
    pub strings: BTreeMap<(P, C), String>,
    pub handles: BTreeMap<(P, C), SavedHandle>,
}

impl<P: Ord + Clone, C: Ord + Clone> Slots<P, C> {
    pub fn new() -> Self {
        Slots {
            ints: BTreeMap::new(),
            reals: BTreeMap::new(),
            bools: BTreeMap::new(),
            strings: BTreeMap::new(),
            handles: BTreeMap::new(),
        }
    }

    /// Removes every value of parent `parent` (all kinds). Returns the ids whose references the
    /// table held, in key order; the caller releases them.
    pub fn remove_parent(&mut self, parent: &P) -> Vec<u32> {
        let released = self
            .handles
            .iter()
            .filter(|((p, _), h)| p == parent && h.owned)
            .map(|(_, h)| h.id)
            .collect();
        self.ints.retain(|(p, _), _| p != parent);
        self.reals.retain(|(p, _), _| p != parent);
        self.bools.retain(|(p, _), _| p != parent);
        self.strings.retain(|(p, _), _| p != parent);
        self.handles.retain(|(p, _), _| p != parent);
        released
    }

    /// Removes every value. Returns the ids whose references the table held, in key order.
    pub fn clear(&mut self) -> Vec<u32> {
        let released = self
            .handles
            .values()
            .filter(|h| h.owned)
            .map(|h| h.id)
            .collect();
        self.ints.clear();
        self.reals.clear();
        self.bools.clear();
        self.strings.clear();
        self.handles.clear();
        released
    }
}

impl<P: Ord + Clone, C: Ord + Clone> Default for Slots<P, C> {
    fn default() -> Self {
        Self::new()
    }
}

/// The playable area, as the original keeps it in its game-state struct (min x, min y, max x, max y).
/// Rect coordinates are clamped to it. The map loader does not fill it in yet, so until a host sets
/// it ([`Game::set_playable`]) rects are not clamped (UNSURE, see `natives/location.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playable {
    pub min_x: F32,
    pub min_y: F32,
    pub max_x: F32,
    pub max_y: F32,
}

/// Number of player slots: `Player(0)` to `Player(15)`.
pub const PLAYER_SLOTS: usize = 16;
/// Number of alliance types, `ALLIANCE_PASSIVE` (0) to `ALLIANCE_SHARED_VISION_FORCED` (9).
pub const ALLIANCE_TYPES: usize = 10;
/// Number of player state slots, `PLAYER_STATE_*` 0 to 25. Other values are not valid states.
pub const PLAYER_STATES: usize = 26;

/// A player slot (`player` in JASS). The 16 slots exist from [`Game::new`] on; their handles are
/// created when a script first asks for one (see [`Game::player_handle`]).
///
/// The raw values (race, color, controller, slot state) are the integers behind the JASS handle
/// types, because `Convert*` returns them raw. Their defaults are placeholders until a host sets
/// them from the map (UNSURE, see `natives/player.rs`).
#[derive(Debug, Clone)]
pub struct PlayerObj {
    /// The slot index, 0 to 15. Fixed at creation.
    pub index: u32,
    /// `GetPlayerName` / `SetPlayerName`.
    pub name: String,
    /// The race value (`+0x260`). Read as `RACE_HUMAN` (1) when the player is null.
    pub race: u32,
    /// The color value (`+0x264`).
    pub color: u32,
    /// The controller value (`+0x268`).
    pub controller: u32,
    /// The slot state (`+0x270`): 0 empty, 1 playing, 2 left. Empty slots are skipped by the force
    /// enumerations that test it (see `natives/force.rs`).
    pub slot_state: u32,
    /// The start location index (`+0x274`).
    pub start_location: i32,
    /// The team (`+0x278`).
    pub team: i32,
    /// `IsPlayerObserver`. The original keeps it as a word at `+0xf0`; a host sets it.
    pub observer: bool,
    /// Per alliance type, the set of target players as a bit mask (bit `q` = this player has the
    /// type with player `q`). Only the row of this player is stored: alliances are one-directional.
    pub alliances: [u32; ALLIANCE_TYPES],
    /// The stored state words (see `natives/player.rs` for the scaling of gold and lumber). Entries 1 (gold) and 2
    /// (lumber) are not used: those live in `Game::economy`.
    pub states: [i32; PLAYER_STATES],
    /// The unit counters of this player (the counter object at `player+0x2D0`, see `unit_counts.rs`).
    pub counts: crate::unit_counts::PlayerCounts,
}

impl PlayerObj {
    /// A slot with the placeholder defaults. Only the index is meaningful before a host fills the slot.
    pub fn new(index: u32) -> Self {
        let mut alliances = [0; ALLIANCE_TYPES];
        // A player is passively allied with itself (UNSURE, see `natives/player.rs`).
        alliances[0] = 1 << index;
        PlayerObj {
            index,
            name: String::new(),
            race: 1,
            color: index,
            controller: 0,
            slot_state: 0,
            start_location: 0,
            team: 0,
            observer: false,
            alliances,
            states: [0; PLAYER_STATES],
            counts: crate::unit_counts::PlayerCounts::new(),
        }
    }
}

/// A reference held by a group: the unit and the handle id whose reference the group owns (so the id
/// stays reserved while the unit is in the group, see `natives/group.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupMember {
    pub unit: ObjKey,
    pub id: u32,
}

/// A unit group (`group` in JASS): the members in insertion order, plus the query state the
/// `GroupEnum*` collector reads (see `natives/group.rs`, spec section 3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupObj {
    /// Members in insertion order, without duplicates. Removed units stay here as ghosts until the
    /// group is cleared or enumerated.
    pub members: Vec<GroupMember>,
    /// The filter boolexpr of the running enumeration, or `None`.
    pub filter: Option<ObjKey>,
    /// Unit type filter of the running enumeration (`None` for any type).
    pub type_id: Option<u32>,
    /// Owner filter of the running enumeration (`None` for any player).
    pub player: Option<u32>,
    /// Limit of the running enumeration. 0 means no limit (the non-counted natives use 0).
    pub limit: u32,
    /// Counter of the running enumeration (spec `groups-events.md` 3.3: it counts failed inserts).
    pub counter: u32,
}

/// One entry of an observer table: the trigger listening for a unit or player-unit event key, the
/// event object it registered, and the optional filter boolexpr (player-unit events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observer {
    pub key: u32,
    pub trigger: ObjKey,
    pub event: ObjKey,
    pub filter: Option<ObjKey>,
}

/// A force (`force` in JASS): a set of player slots, one bit per slot (bit `i` = `Player(i)`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ForceObj {
    pub members: u32,
}

/// One game object of the arena.
#[derive(Debug, Clone)]
pub enum Object {
    Timer(TimerObj),
    Trigger(TriggerObj),
    BoolExpr(BoolExpr),
    Condition(Condition),
    Action(Action),
    Event(Event),
    Location(LocationObj),
    Rect(RectObj),
    /// A region (see [`RegionObj`]).
    Region(RegionObj),
    /// A hashtable: saved values keyed by (parent, child) integers.
    Hashtable(super::hashtable::HashtableObj),
    /// A game cache: saved values keyed by (mission, key) strings.
    GameCache(Slots<String, String>),
    /// A player slot. Its arena key is its slot index.
    Player(PlayerObj),
    /// A force.
    Force(ForceObj),
    /// A unit.
    Unit(UnitObj),
    /// A unit group.
    Group(GroupObj),
    /// An item.
    Item(ItemObj),
    /// A destructable (see [`DestructableObj`]).
    Destructable(DestructableObj),
    /// A fog modifier (see `natives/fog.rs`).
    Fog(super::fog::FogModifierObj),
    /// A presentation object with a handle (see `presentation.rs`).
    Pres(PresObj),
    /// A unit or item pool (see `natives/pool.rs`).
    Pool(PoolObj),
    /// A defeat condition (see `natives/defeat.rs`).
    DefeatCondition(DefeatObj),
    /// A trackable (see `natives/defeat.rs`).
    Trackable(TrackableObj),
}

/// A unit pool or an item pool (`unitpool` / `itempool`): weighted entries in insertion order. An entry is a raw
/// type id and its weight (a float32 in the engine's soft-float). The entries follow the original's pool layout
/// (`natives/pool.rs`, spec in the module notes).
#[derive(Debug, Clone, PartialEq)]
pub struct PoolObj {
    /// `true` for an item pool, `false` for a unit pool.
    pub item: bool,
    /// `(raw type id, weight)` in insertion order. An entry is never added twice: adding an existing id updates its
    /// weight in place.
    pub entries: Vec<(u32, F32)>,
}

/// A defeat condition (`defeatcondition`). Only its description is kept: the victory and defeat checks that read
/// the list are not modelled (`natives/defeat.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefeatObj {
    /// The text set by `DefeatConditionSetDescription`.
    pub description: String,
}

/// A trackable (`trackable`): a model path and a position. Its events are not fired (`natives/defeat.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackableObj {
    /// The model path given to `CreateTrackable`.
    pub model: String,
    pub x: F32,
    pub y: F32,
    pub z: F32,
}

impl Object {
    /// The handle type of this object.
    pub fn handle_kind(&self) -> HandleKind {
        match self {
            Object::Timer(_) => HandleKind::Timer,
            Object::Trigger(_) => HandleKind::Trigger,
            Object::BoolExpr(_) => HandleKind::BoolExpr,
            Object::Condition(_) => HandleKind::Condition,
            Object::Action(_) => HandleKind::Action,
            Object::Event(_) => HandleKind::Event,
            Object::Location(_) => HandleKind::Location,
            Object::Rect(_) => HandleKind::Rect,
            Object::Region(_) => HandleKind::Region,
            Object::Hashtable(_) => HandleKind::Hashtable,
            Object::GameCache(_) => HandleKind::GameCache,
            Object::Player(_) => HandleKind::Player,
            Object::Force(_) => HandleKind::Force,
            Object::Unit(_) => HandleKind::Unit,
            Object::Group(_) => HandleKind::Group,
            Object::Item(_) => HandleKind::Item,
            Object::Destructable(_) => HandleKind::Destructable,
            Object::Fog(_) => HandleKind::FogModifier,
            Object::Pres(p) => p.handle_kind(),
            Object::Pool(p) => {
                if p.item {
                    HandleKind::ItemPool
                } else {
                    HandleKind::UnitPool
                }
            }
            Object::DefeatCondition(_) => HandleKind::DefeatCondition,
            Object::Trackable(_) => HandleKind::Trackable,
        }
    }
}

/// The timer and trigger state of one game. Shared between the natives and the host through an
/// `Rc<RefCell<Game>>`. Natives must not keep a borrow of it while they run script code.
#[derive(Debug)]
pub struct Game {
    objects: Vec<Object>,
    handles: HandleTable<ObjKey>,
    /// Pending timers: sleeps, timer expiries and trigger events. Its time is the last expiry or
    /// pump target.
    pub timers: TimerQueue<Payload>,
    /// `Ctx` of key `i + 1` is `contexts[i]`.
    contexts: Vec<Ctx>,
    context_keys: BTreeMap<Ctx, u32>,
    /// Game caches by name. A cache is created by `InitGameCache` and never destroyed.
    caches: BTreeMap<String, ObjKey>,
    /// The playable area that rect coordinates are clamped to, or `None` (no clamp) until a host sets it.
    playable: Option<Playable>,
    /// The whole-map rectangle `GetWorldBounds` returns (the terrain getter `0x6F73F500`), or `None` until a host
    /// sets it. See `natives/melee_start.rs`.
    world_bounds: Option<Playable>,
    /// The map's tileset character (the byte at `+0x1D0` of the map object, read by `0x6F00F520`); 0 skips the
    /// tileset test of the random creep and neutral building picks.
    map_tileset: u8,
    /// Whether the movement timer (0x8026A, [`Payload::Movement`]) has been armed. Armed once per game.
    pub movement_armed: bool,
    /// Movement walks run so far (one per firing of the movement timer).
    pub movement_calls: u64,
    /// The game time positions are evaluated at (see `crate::movement`).
    pub(crate) game_time: F32,
    /// The movers of units that have walked (see `crate::movement`).
    pub(crate) movers: crate::movement::Movers,
    /// Whether the vision refresh timer (0x80269, [`Payload::Vision`]) has been armed. Armed once per game.
    pub vision_armed: bool,
    /// The softfloat tables that the playable-area grid needs (set by the host, see [`Game::set_vision_tables`]).
    vision_tables: Option<softfloat::Tables>,
    /// The player index of `GetLocalPlayer`. Set by the host (see [`Game::set_local_player`]).
    local_player: u32,
    /// Player handles of the running `GetEnumPlayer` scopes, innermost last. Empty outside a scope.
    enum_players: Vec<u32>,
    /// Player handles of the running `GetFilterPlayer` scopes, innermost last.
    filter_players: Vec<u32>,
    /// The global generator and the 45 streams (`GetRandomInt`, `SetRandomSeed`, ...).
    rng: Streams,
    /// The permutation table of the generator, from the user's `Game.dll`. `None` until a host sets it;
    /// the random natives then stop their thread with `MissingTables`.
    rng_table: Option<RngTable>,
    /// Presentation state: the event log, the camera and the non-handle pools (see `presentation.rs`).
    pres: PresentationState,
    /// State of the selection, string, player-removal and timed-turn natives (`lane_e.rs`).
    pub lane: super::lane_e::LaneE,
    /// State of the food-use, corpse, build-order, hotkey and sound-wait natives (`lane_f.rs`).
    pub lane_f: super::lane_f::LaneF,
    /// The unit type registry the host fills (see [`UnitTypes`]).
    unit_types: UnitTypes,
    /// Unit events in the order they were produced, until the host takes them (see [`UnitEvent`]).
    unit_events: Vec<UnitEvent>,
    /// The observer table of each agent (a unit, or a player slot for player-unit events), keyed by
    /// the agent's arena key. Each list is in registration order, oldest first.
    observers: BTreeMap<ObjKey, Vec<Observer>>,
    /// Unit handles of the running `GetEnumUnit` scopes (group callbacks), innermost last.
    enum_units: Vec<u32>,
    /// Unit handles of the running `GetFilterUnit` scopes, innermost last.
    filter_units: Vec<u32>,
    /// The item type registry the host fills (see [`ItemTypes`]).
    item_types: ItemTypes,
    /// Shop stock data and lists (`crate::shop`, `natives/shop.rs`).
    pub shops: ShopState,
    /// The pack of each unit that has one: one entry per slot, `None` for an empty slot. A pack grows to the
    /// slot it is first used at and never has more slots than the unit's capacity (the type's
    /// `inventory_size`).
    pub(crate) packs: BTreeMap<ObjKey, Vec<Option<ObjKey>>>,
    /// Item orders in the order they were queued, until the host takes them (see [`ItemOrder`]).
    item_orders: Vec<ItemOrder>,
    /// Item handles of the running `GetEnumItem` scopes (`EnumItemsInRect`), innermost last.
    enum_items: Vec<u32>,
    /// Item handles of the running `GetFilterItem` scopes, innermost last.
    filter_items: Vec<u32>,
    /// The destructable type registry the host fills (see [`DestructableTypes`]).
    destructable_types: DestructableTypes,
    /// Destructable handles of the running `GetEnumDestructable` scopes (`EnumDestructablesInRect`), innermost
    /// last.
    enum_destructables: Vec<u32>,
    /// Destructable handles of the running `GetFilterDestructable` scopes, innermost last.
    filter_destructables: Vec<u32>,
    /// The ability type registry the host fills (see [`AbilityTypes`]).
    ability_types: AbilityTypes,
    /// Game-state values (speed, difficulty, flags, campaign, tournament; see `natives/gamestate.rs`).
    pub(crate) game_state: super::gamestate::GameState,
    /// Fog of war: fog modifiers and the per-player cell state (see `natives/fog.rs`).
    pub(crate) fog: super::fog::FogState,
    /// Blight, terrain overlay, start locations, player tech and misc player state, and the native log
    /// (see `natives/misc.rs`).
    pub(crate) misc: super::misc::MiscState,
    /// The order system: unit order lists, the issued-order records and the mover (see `orders.rs`).
    pub(crate) orders: crate::orders::OrderSystem,
    /// The last buff serial handed out (see [`Buff::serial`]).
    pub(crate) buff_serial: u32,
    /// Strings that event records refer to by index: chat patterns and global variable names (see
    /// `event.rs`). Never removed.
    pub strings: Vec<String>,
    /// The response slots of every event fired through `event::fire_event`, by index (see `Ctx::Trigger`).
    pub(crate) firings: Vec<super::event::Firing>,
    /// Waygate state of the units that have the waygate ability (see `natives/economy.rs`).
    pub(crate) waygates: BTreeMap<ObjKey, super::economy::WaygateState>,
    /// The gold mine of each resource node (a unit with the gold mine flag). A unit has an entry only when it is a
    /// resource node (see `natives/economy.rs`).
    pub(crate) mines: BTreeMap<ObjKey, crate::economy::Mine>,
    /// The gold a unit gets at creation for each resource ability it carries (ability raw code -> amount), filled by
    /// the host from the ability data (`Agld`, level 1 `DataA`). See [`Game::set_ability_resource_amount`].
    pub(crate) ability_resource: BTreeMap<u32, i32>,
    /// Gold and lumber of each player slot, the x10 stored amounts and the upkeep rates (see `economy.rs`). Index =
    /// slot. Gold and lumber are read and written only here (`SetPlayerState`, `GetPlayerState` and the changer).
    pub(crate) economy: Vec<crate::economy::PlayerEconomy>,
    /// The economy constants read from Misc at boot (or the built-in defaults), see [`crate::economy::EconomyConstants`].
    pub(crate) economy_constants: crate::economy::EconomyConstants,
    /// The attack cycle: type data, Misc values, attack objects and the damage model (see `combat.rs`).
    pub(crate) combat: crate::combat::CombatSystem,
    /// The hero experience tables built from the Misc values (see `crate::heroes`). Empty until the engine sets them,
    /// in which case the heroes have no thresholds (`MaxHeroLevel` 0).
    pub(crate) hero_xp: crate::heroes::XpTables,
    /// The derived attribute bonus rates (`[Misc]`, see `crate::heroes::derived_bonuses`). All zero until the engine
    /// sets them, so the attributes have no derived effect.
    pub(crate) hero_rates: crate::heroes::BonusRates,
    /// Hero attributes of each hero type (base and per-level gain), copied into a new hero (see `heroes`).
    pub(crate) hero_attributes: BTreeMap<u32, crate::heroes::HeroAttributes>,
    /// The day cycle (`[Misc]` DayLength, DayHours, Dawn, Dusk; `crate::daynight`). The default is the usual cycle until
    /// the engine installs the map's Misc values.
    pub(crate) day_cycle: crate::daynight::DayCycle,
    /// Training queues, constructions, research and worker cycles (see `production.rs`).
    pub(crate) production: crate::production::Production,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        // The player slots are the first objects of the arena: `Player(i)` has key `i`.
        let objects = (0..PLAYER_SLOTS as u32)
            .map(|i| Object::Player(PlayerObj::new(i)))
            .collect();
        Game {
            objects,
            handles: HandleTable::new(),
            timers: TimerQueue::new(),
            movement_armed: false,
            movement_calls: 0,
            game_time: F32::ZERO,
            movers: crate::movement::Movers::new(),
            vision_armed: false,
            vision_tables: None,
            contexts: Vec::new(),
            context_keys: BTreeMap::new(),
            caches: BTreeMap::new(),
            playable: None,
            world_bounds: None,
            map_tileset: 0,
            local_player: 0,
            enum_players: Vec::new(),
            filter_players: Vec::new(),
            rng: Streams::default(),
            rng_table: None,
            pres: PresentationState::default(),
            lane: super::lane_e::LaneE::default(),
            lane_f: super::lane_f::LaneF::default(),
            unit_types: UnitTypes::new(),
            shops: ShopState::default(),
            unit_events: Vec::new(),
            observers: BTreeMap::new(),
            enum_units: Vec::new(),
            filter_units: Vec::new(),
            item_types: ItemTypes::new(),
            packs: BTreeMap::new(),
            item_orders: Vec::new(),
            enum_items: Vec::new(),
            filter_items: Vec::new(),
            destructable_types: DestructableTypes::new(),
            enum_destructables: Vec::new(),
            filter_destructables: Vec::new(),
            ability_types: AbilityTypes::new(),
            game_state: super::gamestate::GameState::new(),
            fog: super::fog::FogState::default(),
            misc: super::misc::MiscState::default(),
            orders: crate::orders::OrderSystem::new(),
            buff_serial: 0,
            strings: Vec::new(),
            firings: Vec::new(),
            waygates: BTreeMap::new(),
            mines: BTreeMap::new(),
            ability_resource: BTreeMap::new(),
            economy: (0..PLAYER_SLOTS).map(crate::economy::PlayerEconomy::new).collect(),
            economy_constants: crate::economy::EconomyConstants::default(),
            combat: crate::combat::CombatSystem::default(),
            hero_xp: crate::heroes::XpTables::default(),
            hero_rates: crate::heroes::BonusRates::default(),
            hero_attributes: BTreeMap::new(),
            day_cycle: crate::daynight::DayCycle::default(),
            production: crate::production::Production::default(),
        }
    }

    /// The item type registry (read only). TODO(gamedata): filled by the engine from SLK data.
    pub fn item_types(&self) -> &ItemTypes {
        &self.item_types
    }

    /// Mutable access to the item type registry, for the host and the tests.
    pub fn item_types_mut(&mut self) -> &mut ItemTypes {
        &mut self.item_types
    }

    /// The item behind `key`. Panics if `key` is not an item.
    pub fn item(&self, key: ObjKey) -> &ItemObj {
        match self.object(key) {
            Object::Item(i) => i,
            _ => panic!("not an item"),
        }
    }

    /// Mutable access to the item behind `key`. Panics if `key` is not an item.
    pub fn item_mut(&mut self, key: ObjKey) -> &mut ItemObj {
        match self.object_mut(key) {
            Object::Item(i) => i,
            _ => panic!("not an item"),
        }
    }

    /// The item behind handle `id`, or `None` for a null, freed, destroyed, wrong-type or removed handle.
    /// Every item native resolves its item argument through this.
    pub fn item_of_handle(&self, id: u32) -> Option<ObjKey> {
        let k = self.handle_of_kind(id, HandleKind::Item)?;
        (!self.item(k).removed).then_some(k)
    }

    /// The item orders queued so far, oldest first.
    pub fn item_orders(&self) -> &[ItemOrder] {
        &self.item_orders
    }

    /// Takes the queued item orders (oldest first) and empties the queue.
    pub fn take_item_orders(&mut self) -> Vec<ItemOrder> {
        std::mem::take(&mut self.item_orders)
    }

    /// Appends an item order to the queue.
    pub(crate) fn push_item_order(&mut self, order: ItemOrder) {
        self.item_orders.push(order);
    }

    /// The item handle that `GetEnumItem` returns: the innermost running `EnumItemsInRect` callback, or 0.
    pub fn enum_item(&self) -> u32 {
        self.enum_items.last().copied().unwrap_or(0)
    }

    /// The item handle that `GetFilterItem` returns: the innermost running filter call, or 0.
    pub fn filter_item(&self) -> u32 {
        self.filter_items.last().copied().unwrap_or(0)
    }

    /// Opens a `GetEnumItem` scope with item handle `id`. Pair with [`Game::pop_enum_item`].
    pub fn push_enum_item(&mut self, id: u32) {
        self.enum_items.push(id);
    }

    /// Closes the innermost `GetEnumItem` scope.
    pub fn pop_enum_item(&mut self) {
        self.enum_items.pop();
    }

    /// Opens a `GetFilterItem` scope with item handle `id`. Pair with [`Game::pop_filter_item`].
    pub fn push_filter_item(&mut self, id: u32) {
        self.filter_items.push(id);
    }

    /// Closes the innermost `GetFilterItem` scope.
    pub fn pop_filter_item(&mut self) {
        self.filter_items.pop();
    }

    /// The destructable type registry (read only). TODO(gamedata): filled by the engine from SLK data.
    pub fn destructable_types(&self) -> &DestructableTypes {
        &self.destructable_types
    }

    /// The destructable type registry, for the host and the tests.
    pub fn destructable_types_mut(&mut self) -> &mut DestructableTypes {
        &mut self.destructable_types
    }

    /// The destructable behind `key`. Panics if `key` is not a destructable.
    pub fn destructable(&self, key: ObjKey) -> &DestructableObj {
        match self.object(key) {
            Object::Destructable(d) => d,
            _ => panic!("not a destructable"),
        }
    }

    /// Mutable access to the destructable behind `key`. Panics if `key` is not a destructable.
    pub fn destructable_mut(&mut self, key: ObjKey) -> &mut DestructableObj {
        match self.object_mut(key) {
            Object::Destructable(d) => d,
            _ => panic!("not a destructable"),
        }
    }

    /// The destructable behind handle `id`, or `None` for a null, freed, destroyed, wrong-type or removed handle.
    /// Every destructable native resolves its argument through this.
    pub fn destructable_of_handle(&self, id: u32) -> Option<ObjKey> {
        let k = self.handle_of_kind(id, HandleKind::Destructable)?;
        (!self.destructable(k).removed).then_some(k)
    }

    /// The destructable handle that `GetEnumDestructable` returns: the innermost running enum callback, or 0.
    pub fn enum_destructable(&self) -> u32 {
        self.enum_destructables.last().copied().unwrap_or(0)
    }

    /// The destructable handle that `GetFilterDestructable` returns: the innermost running filter call, or 0.
    pub fn filter_destructable(&self) -> u32 {
        self.filter_destructables.last().copied().unwrap_or(0)
    }

    /// Opens a `GetEnumDestructable` scope with destructable handle `id`. Pair with
    /// [`Game::pop_enum_destructable`].
    pub fn push_enum_destructable(&mut self, id: u32) {
        self.enum_destructables.push(id);
    }

    /// Closes the innermost `GetEnumDestructable` scope.
    pub fn pop_enum_destructable(&mut self) {
        self.enum_destructables.pop();
    }

    /// Opens a `GetFilterDestructable` scope with destructable handle `id`. Pair with
    /// [`Game::pop_filter_destructable`].
    pub fn push_filter_destructable(&mut self, id: u32) {
        self.filter_destructables.push(id);
    }

    /// Closes the innermost `GetFilterDestructable` scope.
    pub fn pop_filter_destructable(&mut self) {
        self.filter_destructables.pop();
    }

    /// The ability type registry (read only). TODO(gamedata): filled by the engine from SLK data.
    pub fn ability_types(&self) -> &AbilityTypes {
        &self.ability_types
    }

    /// Mutable access to the ability type registry, for the host and the tests.
    pub fn ability_types_mut(&mut self) -> &mut AbilityTypes {
        &mut self.ability_types
    }

    /// The unit type registry (read only). TODO(gamedata): filled by the engine from SLK data.
    pub fn unit_types(&self) -> &UnitTypes {
        &self.unit_types
    }

    /// Mutable access to the unit type registry, for the host and the tests.
    pub fn unit_types_mut(&mut self) -> &mut UnitTypes {
        &mut self.unit_types
    }

    /// The unit behind `key`. Panics if `key` is not a unit.
    /// Installs the hero experience tables (built by `crate::heroes::build` from the Misc values).
    pub fn set_hero_xp(&mut self, tables: crate::heroes::XpTables) {
        self.hero_xp = tables;
    }

    /// Installs the day cycle (`[Misc]` DayLength, DayHours, Dawn and Dusk, `crate::daynight`).
    pub fn set_day_cycle(&mut self, cycle: crate::daynight::DayCycle) {
        self.day_cycle = cycle;
    }

    /// The day cycle in force.
    pub fn day_cycle(&self) -> &crate::daynight::DayCycle {
        &self.day_cycle
    }

    /// Installs the derived attribute bonus rates (`[Misc]`, see `crate::heroes::derived_bonuses`).
    pub fn set_hero_rates(&mut self, rates: crate::heroes::BonusRates) {
        self.hero_rates = rates;
    }

    /// The derived attribute bonus rates in force.
    pub fn hero_rates(&self) -> &crate::heroes::BonusRates {
        &self.hero_rates
    }

    /// Sets the hero attributes of a hero type (the engine fills them from the unit records).
    pub fn set_hero_attributes(&mut self, type_id: u32, attributes: crate::heroes::HeroAttributes) {
        self.hero_attributes.insert(type_id, attributes);
    }

    /// The hero state a new hero of `type_id` starts with (level 1, no experience, the type's attributes).
    pub fn new_hero(&self, type_id: u32) -> HeroObj {
        let mut h = HeroObj::default();
        if let Some(a) = self.hero_attributes.get(&type_id) {
            h.str_base = a.str_base;
            h.str_plus = a.str_plus;
            h.agi_base = a.agi_base;
            h.agi_plus = a.agi_plus;
            h.int_base = a.int_base;
            h.int_plus = a.int_plus;
        }
        h
    }

    /// The hero experience tables in force.
    pub fn hero_xp(&self) -> &crate::heroes::XpTables {
        &self.hero_xp
    }

    pub fn unit(&self, key: ObjKey) -> &UnitObj {
        match self.object(key) {
            Object::Unit(u) => u,
            _ => panic!("not a unit"),
        }
    }

    /// Mutable access to the unit behind `key`. Panics if `key` is not a unit.
    pub fn unit_mut(&mut self, key: ObjKey) -> &mut UnitObj {
        match self.object_mut(key) {
            Object::Unit(u) => u,
            _ => panic!("not a unit"),
        }
    }

    /// The unit behind handle `id`, or `None` for a null, freed, destroyed, wrong-type or removed handle.
    /// Every unit native resolves its unit argument through this.
    pub fn unit_of_handle(&self, id: u32) -> Option<ObjKey> {
        let k = self.handle_of_kind(id, HandleKind::Unit)?;
        (!self.unit(k).removed).then_some(k)
    }

    /// The unit events recorded so far, oldest first.
    pub fn unit_events(&self) -> &[UnitEvent] {
        &self.unit_events
    }

    /// Takes the recorded unit events (oldest first) and empties the queue.
    pub fn take_unit_events(&mut self) -> Vec<UnitEvent> {
        std::mem::take(&mut self.unit_events)
    }

    /// Appends a unit event to the queue.
    pub(crate) fn push_unit_event(&mut self, event: UnitEvent) {
        self.unit_events.push(event);
    }

    /// The group object behind `key`. Panics if `key` is not a group.
    pub fn group(&self, key: ObjKey) -> &GroupObj {
        match self.object(key) {
            Object::Group(g) => g,
            _ => panic!("not a group"),
        }
    }

    /// Mutable access to the group object behind `key`. Panics if `key` is not a group.
    pub fn group_mut(&mut self, key: ObjKey) -> &mut GroupObj {
        match self.object_mut(key) {
            Object::Group(g) => g,
            _ => panic!("not a group"),
        }
    }

    /// The unit handle that `GetEnumUnit` returns: the innermost running group callback, or 0.
    pub fn enum_unit(&self) -> u32 {
        self.enum_units.last().copied().unwrap_or(0)
    }

    /// The unit handle that `GetFilterUnit` returns: the innermost running filter call, or 0.
    pub fn filter_unit(&self) -> u32 {
        self.filter_units.last().copied().unwrap_or(0)
    }

    /// Opens a `GetEnumUnit` scope with unit handle `id`. Pair with [`Game::pop_enum_unit`].
    pub fn push_enum_unit(&mut self, id: u32) {
        self.enum_units.push(id);
    }

    /// Closes the innermost `GetEnumUnit` scope.
    pub fn pop_enum_unit(&mut self) {
        self.enum_units.pop();
    }

    /// Opens a `GetFilterUnit` scope with unit handle `id`. Pair with [`Game::pop_filter_unit`].
    pub fn push_filter_unit(&mut self, id: u32) {
        self.filter_units.push(id);
    }

    /// Closes the innermost `GetFilterUnit` scope.
    pub fn pop_filter_unit(&mut self) {
        self.filter_units.pop();
    }

    /// The handle id of object `key`: its live id, or a new one (flag 0) when it has none. This is the
    /// original's object-to-handle conversion (spec `groups-events.md` 2.7).
    pub fn handle_or_create(&mut self, key: ObjKey) -> u32 {
        self.handles.create(key, 0)
    }

    /// The observer table of `agent` (oldest first). Empty when the agent has none.
    pub fn observers(&self, agent: ObjKey) -> &[Observer] {
        self.observers.get(&agent).map_or(&[], Vec::as_slice)
    }

    /// Mutable access to the observer table of `agent`, created on demand.
    pub fn observers_mut(&mut self, agent: ObjKey) -> &mut Vec<Observer> {
        self.observers.entry(agent).or_default()
    }

    /// Removes every observer entry that belongs to trigger `t` (used when the trigger is destroyed).
    pub fn forget_observers_of(&mut self, t: ObjKey) {
        for list in self.observers.values_mut() {
            list.retain(|o| o.trigger != t);
        }
    }

    /// The agents that have an observer table, in key (creation) order. A region's enter and leave
    /// registrations live in its own table, so this is the list the region hook scans.
    pub fn observer_agents(&self) -> Vec<ObjKey> {
        self.observers.keys().copied().collect()
    }

    /// The presentation state: the event log that a renderer reads, the camera and the pools.
    pub fn presentation(&self) -> &PresentationState {
        &self.pres
    }

    /// Mutable access to the presentation state (for the host and the natives).
    pub fn presentation_mut(&mut self) -> &mut PresentationState {
        &mut self.pres
    }

    /// Sets (or clears) the RNG permutation table. Without it the random natives fail with
    /// `MissingTables`. See [`crate::rng`].
    pub fn set_rng_table(&mut self, table: Option<RngTable>) {
        self.rng_table = table;
    }

    /// The generator state: the global generator and the streams.
    pub fn rng(&self) -> &Streams {
        &self.rng
    }

    /// The RNG table and the mutable generator state together, for a draw. `None` without a table.
    pub fn rng_parts(&mut self) -> Option<(&RngTable, &mut Streams)> {
        self.rng_table.as_ref().map(|table| (table, &mut self.rng))
    }

    /// Sets the player index that `GetLocalPlayer` returns. The host provides it (UNSURE: the original
    /// takes it from the game state, see `natives/player.rs`).
    pub fn set_local_player(&mut self, index: u32) {
        self.local_player = index;
    }

    /// The index of the local player.
    pub fn local_player(&self) -> u32 {
        self.local_player
    }

    /// The slot `index` (0 to 15). Panics for an index outside the table.
    pub fn player(&self, index: u32) -> &PlayerObj {
        match self.object(ObjKey(index)) {
            Object::Player(p) => p,
            _ => unreachable!("player slots are the first objects of the arena"),
        }
    }

    /// Mutable access to slot `index`, for the host (map setup). `None` for an index outside 0 to 15.
    pub fn player_mut(&mut self, index: u32) -> Option<&mut PlayerObj> {
        if index as usize >= PLAYER_SLOTS {
            return None;
        }
        match self.object_mut(ObjKey(index)) {
            Object::Player(p) => Some(p),
            _ => unreachable!("player slots are the first objects of the arena"),
        }
    }

    /// The handle of slot `index`. The handle is created on the first request (flag 0) and the same
    /// id is returned afterwards. Returns 0 for an index outside 0 to 15. LIKELY: the original creates
    /// it lazily, see `natives/player.rs`.
    pub fn player_handle(&mut self, index: u32) -> u32 {
        if index as usize >= PLAYER_SLOTS {
            return 0;
        }
        let key = ObjKey(index);
        match self.handles.id_of(&key) {
            Some(id) => id,
            None => self.handles.create(key, 0),
        }
    }

    /// The slot index of a live player handle, or `None` for any other id.
    pub fn player_index_of_handle(&self, id: u32) -> Option<u32> {
        self.handle_of_kind(id, HandleKind::Player).map(|k| k.0)
    }

    /// The force behind a live force handle, or `None` for any other id.
    pub fn force_of_handle(&self, id: u32) -> Option<ObjKey> {
        self.handle_of_kind(id, HandleKind::Force)
    }

    /// The force object behind `key`. Panics if `key` is not a force.
    pub fn force(&self, key: ObjKey) -> &ForceObj {
        match self.object(key) {
            Object::Force(f) => f,
            _ => panic!("not a force"),
        }
    }

    /// Mutable access to the force object behind `key`. Panics if `key` is not a force.
    pub fn force_mut(&mut self, key: ObjKey) -> &mut ForceObj {
        match self.object_mut(key) {
            Object::Force(f) => f,
            _ => panic!("not a force"),
        }
    }

    /// The player handle that `GetEnumPlayer` returns: the innermost running enumeration, or 0.
    pub fn enum_player(&self) -> u32 {
        self.enum_players.last().copied().unwrap_or(0)
    }

    /// The player handle that `GetFilterPlayer` returns: the innermost running filter call, or 0.
    pub fn filter_player(&self) -> u32 {
        self.filter_players.last().copied().unwrap_or(0)
    }

    /// Opens a `GetEnumPlayer` scope with player handle `id`. Pair with [`Game::pop_enum_player`].
    pub fn push_enum_player(&mut self, id: u32) {
        self.enum_players.push(id);
    }

    /// Closes the innermost `GetEnumPlayer` scope.
    pub fn pop_enum_player(&mut self) {
        self.enum_players.pop();
    }

    /// Opens a `GetFilterPlayer` scope with player handle `id`. Pair with [`Game::pop_filter_player`].
    pub fn push_filter_player(&mut self, id: u32) {
        self.filter_players.push(id);
    }

    /// Closes the innermost `GetFilterPlayer` scope.
    pub fn pop_filter_player(&mut self) {
        self.filter_players.pop();
    }

    /// Sets the playable area (`None` disables the rect clamp). See [`Playable`]. The fog grid is built from it
    /// (`vision::VisionGrid::from_playable`) when the softfloat tables are set; otherwise the grid keeps its fallback
    /// size until the tables arrive.
    pub fn set_playable(&mut self, playable: Option<Playable>) {
        self.playable = playable;
        self.refresh_unit_positions();
        if let (Some(p), Some(t)) = (playable, self.vision_tables.as_ref()) {
            self.fog.grid = crate::vision::VisionGrid::from_playable(p.min_x, p.min_y, p.max_x, p.max_y, t);
        }
    }

    /// Sets the softfloat tables the fog grid needs (from the user's `Game.dll`). If a playable area is already set,
    /// the grid is rebuilt from it.
    pub fn set_vision_tables(&mut self, tables: Option<softfloat::Tables>) {
        self.vision_tables = tables;
        if let (Some(p), Some(t)) = (self.playable, self.vision_tables.as_ref()) {
            self.fog.grid = crate::vision::VisionGrid::from_playable(p.min_x, p.min_y, p.max_x, p.max_y, t);
        }
    }

    /// The unit objects of the game with their keys, in creation order (removed and dead units included; the callers
    /// filter).
    pub fn unit_objects(&self) -> impl Iterator<Item = (ObjKey, &UnitObj)> + '_ {
        self.objects.iter().enumerate().filter_map(|(i, o)| match o {
            Object::Unit(u) => Some((ObjKey(i as u32), u)),
            _ => None,
        })
    }

    /// The playable area, if one is set.
    pub fn playable(&self) -> Option<Playable> {
        self.playable
    }

    /// Sets the whole-map rectangle returned by `GetWorldBounds` (`None` clears it).
    pub fn set_world_bounds(&mut self, bounds: Option<Playable>) {
        self.world_bounds = bounds;
    }

    /// The whole-map rectangle, if one is set.
    pub fn world_bounds(&self) -> Option<Playable> {
        self.world_bounds
    }

    /// Sets the map tileset character (0 = none) used by the random creep and neutral building picks.
    pub fn set_map_tileset(&mut self, tileset: u8) {
        self.map_tileset = tileset;
    }

    /// The map tileset character.
    pub fn map_tileset(&self) -> u8 {
        self.map_tileset
    }

    /// The game cache named `name`, if `InitGameCache` created it and it is still alive.
    pub fn cache_named(&self, name: &str) -> Option<ObjKey> {
        self.caches.get(name).copied().filter(|&k| self.is_alive(k))
    }

    /// Records `key` as the game cache named `name`.
    pub fn set_cache_named(&mut self, name: &str, key: ObjKey) {
        self.caches.insert(name.to_string(), key);
    }

    /// Number of objects ever created (destroyed ones included).
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// The handle table (for inspection and tests).
    pub fn handles(&self) -> &HandleTable<ObjKey> {
        &self.handles
    }

    /// The object behind `key`. Panics for a key this game did not create.
    pub fn object(&self, key: ObjKey) -> &Object {
        &self.objects[key.0 as usize]
    }

    /// Mutable access to the object behind `key`.
    pub fn object_mut(&mut self, key: ObjKey) -> &mut Object {
        &mut self.objects[key.0 as usize]
    }

    /// Adds an object without a handle. The caller gives it one later with [`Game::handle_or_create`]: the
    /// original allocates an item's handle after its placement (`docs/spec/items.md` 5.4).
    pub(crate) fn push_object(&mut self, object: Object) -> ObjKey {
        let key = ObjKey(self.objects.len() as u32);
        self.objects.push(object);
        key
    }

    /// Adds an object and gives it a handle with the given flag (see `HandleTable::create`). Returns
    /// the object's key and its handle id.
    pub fn add_object(&mut self, object: Object, flag: u32) -> (ObjKey, u32) {
        let key = ObjKey(self.objects.len() as u32);
        self.objects.push(object);
        let id = self.handles.create(key, flag);
        (key, id)
    }

    /// Adds an object without a handle (the original builds units without one; only the natives allocate it).
    pub fn add_object_unhandled(&mut self, object: Object) -> ObjKey {
        let key = ObjKey(self.objects.len() as u32);
        self.objects.push(object);
        key
    }

    /// The live object of a handle id, or `None` for the null handle, a freed id, or an object that
    /// was destroyed.
    pub fn handle_object(&self, id: u32) -> Option<ObjKey> {
        self.handles.get(id).copied()
    }

    /// The live object of handle `id` if it has the handle type `kind` (the typed check of the
    /// original's natives: a null, freed or wrong-type handle gives `None`).
    pub fn handle_of_kind(&self, id: u32, kind: HandleKind) -> Option<ObjKey> {
        let k = self.handle_object(id)?;
        (self.object(k).handle_kind() == kind).then_some(k)
    }

    /// Adds one reference to handle `id` (the original's AddRef, used by hashtable saves). Null and
    /// unknown ids are ignored.
    pub fn add_ref(&mut self, id: u32) {
        self.handles.add_ref(id);
    }

    /// Drops one reference to handle `id`. The id is freed when its count reaches 0. Null and unknown
    /// ids are ignored.
    pub fn release(&mut self, id: u32) {
        self.handles.release(id);
    }

    /// Drops one reference to each id in `ids`, in order.
    pub fn release_all(&mut self, ids: &[u32]) {
        for &id in ids {
            self.handles.release(id);
        }
    }

    /// The handle id of a live object, or 0 when it has none.
    pub fn handle_of(&self, key: ObjKey) -> u32 {
        self.handles.id_of(&key).unwrap_or(0)
    }

    /// Whether the object still has a live handle.
    pub fn is_alive(&self, key: ObjKey) -> bool {
        self.handles.id_of(&key).is_some()
    }

    /// Releases the object's creation reference (its handle is freed unless other holders keep the
    /// id reserved). The object's data stays in the arena for the scripts that still hold it.
    pub fn kill(&mut self, key: ObjKey) {
        self.handles.object_died(&key);
    }

    /// The context key of `ctx`, interned: the same context always gets the same key.
    pub fn context_key(&mut self, ctx: Ctx) -> u32 {
        if let Some(&k) = self.context_keys.get(&ctx) {
            return k;
        }
        self.contexts.push(ctx);
        let k = self.contexts.len() as u32;
        self.context_keys.insert(ctx, k);
        k
    }

    /// The context of a thread key, or `None` for key 0 and unknown keys.
    pub fn context(&self, key: u32) -> Option<Ctx> {
        let i = key.checked_sub(1)? as usize;
        self.contexts.get(i).copied()
    }
}

/// How a native or the host runs script code: synchronously (conditions), or as a chain that runs
/// until its first stop (trigger actions and timer handlers). Implemented for the native context
/// (where a chain starts when the native returns) and for the VM (where it starts at once).
pub trait ScriptRuntime {
    /// Runs the no-argument function `func` to completion and returns its value.
    fn call_keyed(&mut self, func: u32, key: u32) -> Result<Value, RunStatus>;
    /// Starts a thread that runs `chain` one function after another, with context key `key`.
    fn chain_keyed(&mut self, chain: Vec<u32>, key: u32);
}

impl ScriptRuntime for jass::vm::NativeCtx<'_> {
    fn call_keyed(&mut self, func: u32, key: u32) -> Result<Value, RunStatus> {
        self.call_func_keyed(func, &[], key)
    }

    fn chain_keyed(&mut self, chain: Vec<u32>, key: u32) {
        self.spawn_chain(chain, key);
    }
}

impl ScriptRuntime for jass::vm::Vm {
    fn call_keyed(&mut self, func: u32, key: u32) -> Result<Value, RunStatus> {
        self.call_func(func, &[], key)
    }

    fn chain_keyed(&mut self, chain: Vec<u32>, key: u32) {
        let now = self.now();
        self.run_chain(&chain, key, now);
    }
}
