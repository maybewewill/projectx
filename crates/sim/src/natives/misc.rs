//! Miscellaneous natives that need neither units, combat nor orders: blight, the terrain queries and setters, the
//! preload natives (logged no-ops), the start locations and their priorities, and the player settings that do not
//! depend on units: race preference and selectable flag, score screen, handicap, tax rate, the tech tree levels,
//! and the availability of abilities for a player.
//!
//! Model. Blight and the terrain setters write into the model's grids, which are kept per cell so that a script can
//! read back what it wrote. The terrain queries answer from an optional [`TerrainProvider`] (a host adapter over the
//! map's terrain data) first, then from the values the script wrote, then from defaults. The defaults are UNSURE: the
//! map data was not read in this module (see the table below). Start locations and player settings are stored as the
//! natives write them.
//!
//! Addresses are the 1.26a `Game.dll` natives (`docs/spec/natives-1.26a.tsv`). Labels: CONFIRMED = read in the
//! decompile of the native (`natives.c`) or of the helper it calls, LIKELY = strong inference from the code shape,
//! UNSURE = guessed or not read.
//!
//! | Native | Address | Status | Notes |
//! |---|---|---|---|
//! | `SetBlight` | 0x6f3cd4c0 | CONFIRMED (point clamp), UNSURE store | the blight cells are 32 units wide (the pathing cell); the player only gates the call |
//! | `SetBlightLoc` | 0x6f3cd5d0 | CONFIRMED (point clamp), UNSURE store | |
//! | `SetBlightRect` | 0x6f3cd520 | CONFIRMED gate, UNSURE store | |
//! | `SetBlightPoint` | 0x6f3cd580 | CONFIRMED (point clamp), UNSURE store | |
//! | `IsPointBlighted` | 0x6f3b4550 | CONFIRMED (pathing flag `0x20`), UNSURE store | reads the cell of the point |
//! | `GetTerrainType` | 0x6f3bced0 | CONFIRMED read, UNSURE default | provider, then overlay, then 0 |
//! | `GetTerrainVariance` | 0x6f3b4250 | CONFIRMED read, UNSURE default | provider, then overlay, then 0 |
//! | `GetTerrainCliffLevel` | 0x6f3b4190 | CONFIRMED (point clamp), UNSURE default | provider, then 0 |
//! | `SetTerrainType` | 0x6f3bcf20 | CONFIRMED read, UNSURE store | stores type and variation per 128-unit tile; area and shape are not stored |
//! | `IsTerrainPathable` | 0x6f3b42d0 | UNSURE | the pathing type argument was not read in the body; the model uses the argument. Default true. |
//! | `SetTerrainPathable` | 0x6f3b42f0 | CONFIRMED structure, UNSURE store | stores the flag per type and 32-unit cell |
//! | `Preload` | 0x6f3b5170 | CONFIRMED structure | logged; the file is not read |
//! | `PreloadEnd` | 0x6f3b5190 | CONFIRMED structure | logged |
//! | `PreloadStart` | 0x6f3b51f0 | CONFIRMED structure | logged |
//! | `PreloadRefresh` | 0x6f3b5210 | CONFIRMED structure | logged |
//! | `PreloadEndEx` | 0x6f3b5220 | CONFIRMED structure | logged |
//! | `PreloadGenStart` | 0x6f3b5290 | CONFIRMED structure | logged (the original records the tick count: not modelled) |
//! | `PreloadGenClear` | 0x6f3b52b0 | CONFIRMED structure | logged |
//! | `PreloadGenEnd` | 0x6f3b52d0 | CONFIRMED structure | logged |
//! | `Preloader` | 0x6f3b5310 | CONFIRMED structure | logged; no file is written (the model has no disk) |
//! | `DefineStartLocation` | 0x6f3baf70 | LIKELY | stores the x and y of start location `i` (the index range is not checked in the body; the model keeps `i < 12`) |
//! | `DefineStartLocationLoc` | 0x6f3c0ea0 | CONFIRMED structure | same, from a location |
//! | `GetStartLocationX` | 0x6f3bb060 | CONFIRMED (index 11 is the last; 0 otherwise) | |
//! | `GetStartLocationY` | 0x6f3bb080 | CONFIRMED | |
//! | `GetStartLocationLoc` | 0x6f3d04b0 | CONFIRMED (flag 1 location) | |
//! | `SetStartLocPrioCount` | 0x6f3baf90 | CONFIRMED range check (`< 12`) | |
//! | `SetStartLocPrio` | 0x6f3bafc0 | CONFIRMED range check (`< 12`, type `< 3`) | |
//! | `GetStartLocPrioSlot` | 0x6f3bb000 | CONFIRMED range check | the other start location index |
//! | `GetStartLocPrio` | 0x6f3bb030 | CONFIRMED range check | the priority type |
//! | `ForcePlayerStartLocation` | 0x6f3c0fa0 | CONFIRMED structure (`+0x274`), LIKELY store | sets the player's start location index |
//! | `GetPlayerStartLocationX` | 0x6f3c0ff0 | CONFIRMED | x of the player's start location |
//! | `GetPlayerStartLocationY` | 0x6f3c1020 | CONFIRMED | y of the player's start location |
//! | `SetPlayerRacePreference` | 0x6f3c11a0 | CONFIRMED (flag word `+0x25c`) | keeps the selectable bit `0x40`, sets the other bits |
//! | `IsPlayerRacePrefSet` | 0x6f3c1260 | CONFIRMED | bit test of the preference, without `0x40` |
//! | `SetPlayerRaceSelectable` | 0x6f3c11d0 | CONFIRMED (bit `0x40` of `+0x25c`) | |
//! | `GetPlayerSelectable` | 0x6f3c1290 | CONFIRMED (bit `0x40`) | default set (UNSURE) |
//! | `SetPlayerOnScoreScreen` | 0x6f3c9800 | CONFIRMED (`+0x31c`) | stored flag |
//! | `GetPlayerHandicap` | 0x6f3c9e10 | CONFIRMED (`+0x29c`, 1.0 for a null player) | default 1.0 (UNSURE) |
//! | `SetPlayerHandicap` | 0x6f3c9e30 | CONFIRMED gate (`abs >= 0.001`) | |
//! | `GetPlayerHandicapXP` | 0x6f3c9ec0 | CONFIRMED (`+0x2a4`, 1.0 for a null player) | default 1.0 (UNSURE) |
//! | `SetPlayerHandicapXP` | 0x6f3c9ee0 | LIKELY store | the helper (`0x6f3b7760`) was not read |
//! | `SetPlayerTaxRate` | 0x6f3c10e0 | CONFIRMED gate (map flag `0x100` blocks it), UNSURE key | stores the rate for `(to, state)`; state 1 gold, 2 lumber |
//! | `GetPlayerTaxRate` | 0x6f3c1150 | CONFIRMED gate, UNSURE key | 0 unless set |
//! | `GetPlayerTechMaxAllowed` | 0x6f3c9690 | CONFIRMED structure, UNSURE default | the global cap (`0x6f2afca0`) is not read; default unlimited |
//! | `SetPlayerTechMaxAllowed` | 0x6f3c9660 | CONFIRMED structure | stored |
//! | `AddPlayerTechResearched` | 0x6f3c96d0 | CONFIRMED loop (each step needs the max), LIKELY values | adds one level per step while below the max |
//! | `SetPlayerTechResearched` | 0x6f3c9730 | CONFIRMED (adds only the positive difference) | |
//! | `GetPlayerTechResearched` | 0x6f3c9770 | CONFIRMED structure | true when the level is above 0 |
//! | `GetPlayerTechCount` | 0x6f3c97a0 | CONFIRMED structure, UNSURE values for the non-specific form | the researched level of the tech |
//! | `SetPlayerAbilityAvailable` | 0x6f3c97d0 | CONFIRMED structure | stores the set of unavailable abilities |
//!
//! Not in this module: `GetPlayerScore` (its counters are combat and unit statistics), the unit and item pools and
//! `PlaceRandom*` (they create units or items), `CreateBlightedGoldmine` (a unit), the object-data string lookups
//! `UnitId`, `AbilityId`, `OrderId` and their `2String` forms, `GetObjectName`, `GetAbilityEffect*`, `GetAbilitySound*`
//! and `GetDestructableName` (the object tables are not loaded), `GetLocalizedString` and `GetLocalizedHotkey` (the string
//! tables are not loaded), `SetPlayers`, `GetPlayers`, `SetTeams`, `GetTeams` (slot setup), `RemovePlayer`, `CripplePlayer`,
//! `SetPlayerUnitsOwner`, the unit and structure counts, `CachePlayerHeroData` and `IsSuspendedXP` (hero data), the
//! animation, selection and sync natives, `Cheat`, `ExecuteFunc`, `PauseCompAI` and the AI start natives.
//!
//! The tests are in `tests/misc_natives.rs`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use jass::vm::{NativeRegistry, Value};
use softfloat::{arith, F32};

use super::fog::{circle_cells, rect_cells};
use super::game::{Game, HandleKind, LocationObj, ObjKey, Object, Playable, PLAYER_SLOTS};
use super::gamestate::native;
use super::hashtable::{bool_at, int_at, real_at, str_at};
use super::player::{index_at, raw_at};

/// The blight and pathing cell width (32 world units, as the pathing grid).
pub const PATH_CELL: i32 = 32;
/// The terrain tile width (128 world units, UNSURE: the terrain query's cell was not read).
pub const TILE: i32 = 128;
/// The clamp margin subtracted from the playable area's upper bound (`0x6FAAE564` = 32.0, CONFIRMED in `location.rs`).
const CLAMP_MARGIN: F32 = F32::from_bits(0x4200_0000);
/// The smallest handicap that `SetPlayerHandicap` accepts (`0x6FAAE47C` = 0.001).
const HANDICAP_MIN: F32 = F32::from_bits(0x3A83_126F);
/// The default handicap (1.0, `0x6FAAE4F4`).
const ONE: F32 = F32::ONE;
/// The number of start locations and of priority slots (`< 12`, CONFIRMED range checks).
pub const START_LOCATIONS: usize = 12;
/// The player flag word bit that marks a race as selectable (`+0x25c` bit 6).
pub const RACE_SELECTABLE: u32 = 0x40;
/// The tax-lock map flag (`+0x38` bit 8): tax rates cannot be set while it is set.
const FLAG_TAX_LOCKED: u32 = 0x100;

/// A source of terrain data for the terrain queries. A host implements it over the map's terrain (the `w3e` tile
/// and cliff data and the pathing map). `None` means "no answer": the model then uses what the script wrote, and its
/// defaults. Coordinates are the world coordinates, already clamped to the playable area.
pub trait TerrainProvider {
    /// The terrain type of the tile at `(x, y)`.
    fn tile_type(&self, x: F32, y: F32) -> Option<u32>;
    /// The variation of the tile at `(x, y)`.
    fn tile_variance(&self, x: F32, y: F32) -> Option<u32>;
    /// The cliff level at `(x, y)`.
    fn cliff_level(&self, x: F32, y: F32) -> Option<u32>;
    /// Whether the point is pathable for the pathing type (raw value).
    fn pathable(&self, x: F32, y: F32, pathing: u32) -> Option<bool>;
}

/// One event that a native logged. The host reads them with [`Game::misc_events`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiscEvent {
    /// `CopySaveGame` was called (the model has no save files, so it returns false).
    CopySaveGame,
    /// `EndGame` with its result argument.
    EndGame { result: bool },
    /// `RestartGame` with its argument (keep the settings).
    RestartGame { keep_settings: bool },
    /// `ReloadGame`.
    ReloadGame,
    /// `PauseGame` with its argument.
    PauseGame { paused: bool },
    /// `ForceCampaignSelectScreen`.
    ForceCampaignSelect,
    /// A preload native: its name and the text argument (empty when it has none).
    Preload { native: &'static str, text: String },
}

/// The tech levels of one player and tech: the researched level and the maximum allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TechLevel {
    pub researched: i32,
    /// The maximum allowed level. Default: unlimited (UNSURE: the original's default was not read).
    pub max_allowed: i32,
}

/// The per-player settings that the misc natives write.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerMisc {
    /// The race flag word (`+0x25c`): bit `0x40` is the selectable bit, the other bits the race preference.
    pub race_flags: u32,
    /// `SetPlayerHandicap` value (`+0x29c`).
    pub handicap: F32,
    /// `SetPlayerHandicapXP` value (`+0x2a4`).
    pub handicap_xp: F32,
    /// `SetPlayerOnScoreScreen` (`+0x31c`).
    pub on_score_screen: bool,
    /// Tax rates keyed by `(receiver slot, state)`, state 1 gold and 2 lumber.
    pub tax: BTreeMap<(u32, u32), i32>,
    /// Tech levels keyed by the raw tech id.
    pub tech: BTreeMap<u32, TechLevel>,
    /// Abilities that `SetPlayerAbilityAvailable(p, a, false)` made unavailable (raw ids).
    pub unavailable_abilities: BTreeSet<u32>,
}

impl Default for PlayerMisc {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerMisc {
    /// The placeholder defaults (UNSURE, see the module table): selectable, handicap 1.0.
    pub fn new() -> Self {
        PlayerMisc {
            race_flags: RACE_SELECTABLE,
            handicap: ONE,
            handicap_xp: ONE,
            on_score_screen: false,
            tax: BTreeMap::new(),
            tech: BTreeMap::new(),
            unavailable_abilities: BTreeSet::new(),
        }
    }
}

/// The blight, terrain, start-location, player and log state of the game (see the module notes).
pub struct MiscState {
    /// Events that the natives logged, oldest first.
    pub log: Vec<MiscEvent>,
    /// Blight cells (32-unit cells). A cell in the set is blighted.
    pub blight: BTreeSet<(i32, i32)>,
    /// The terrain provider, if a host set one.
    terrain: Option<Box<dyn TerrainProvider>>,
    /// Terrain written by `SetTerrainType`: `(type, variation)` per 128-unit tile.
    tiles: BTreeMap<(i32, i32), (u32, u32)>,
    /// Pathability written by `SetTerrainPathable`, per `(pathing type, 32-unit cell)`.
    pathing: BTreeMap<(u32, i32, i32), bool>,
    /// Start location positions, indexed by start location (0 to 11).
    pub start_locs: [(F32, F32); START_LOCATIONS],
    /// The number of priority slots of each start location.
    pub start_prio_count: [u32; START_LOCATIONS],
    /// The priority slots: `(other start location, priority type)` per `(start location, slot)`.
    pub start_prio: [[(u32, u32); START_LOCATIONS]; START_LOCATIONS],
    /// Per player slot.
    pub players: Vec<PlayerMisc>,
}

impl Default for MiscState {
    fn default() -> Self {
        Self::new()
    }
}

impl MiscState {
    /// The state with the defaults of each field: no blight, no terrain, start locations at (0, 0), default player
    /// settings.
    pub fn new() -> Self {
        MiscState {
            log: Vec::new(),
            blight: BTreeSet::new(),
            terrain: None,
            tiles: BTreeMap::new(),
            pathing: BTreeMap::new(),
            start_locs: [(F32::ZERO, F32::ZERO); START_LOCATIONS],
            start_prio_count: [0; START_LOCATIONS],
            start_prio: [[(0, 0); START_LOCATIONS]; START_LOCATIONS],
            players: (0..PLAYER_SLOTS).map(|_| PlayerMisc::new()).collect(),
        }
    }
}

impl fmt::Debug for MiscState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MiscState")
            .field("log", &self.log)
            .field("blight", &self.blight.len())
            .field("terrain_provider", &self.terrain.is_some())
            .field("tiles", &self.tiles.len())
            .field("pathing", &self.pathing.len())
            .field("start_locs", &self.start_locs)
            .field("players", &self.players.len())
            .finish()
    }
}

impl Game {
    /// Sets (or clears) the terrain provider that the terrain queries consult first.
    pub fn set_terrain_provider(&mut self, provider: Option<Box<dyn TerrainProvider>>) {
        self.misc.terrain = provider;
    }

    /// The events logged by the misc natives, oldest first.
    pub fn misc_events(&self) -> &[MiscEvent] {
        &self.misc.log
    }

    /// Takes the logged events (oldest first) and empties the log.
    pub fn take_misc_events(&mut self) -> Vec<MiscEvent> {
        std::mem::take(&mut self.misc.log)
    }

    /// The player settings of slot `index` (0 to 15). Panics for an index outside the table.
    pub fn player_misc(&self, index: u32) -> &PlayerMisc {
        &self.misc.players[index as usize]
    }

    /// Whether the cell of point `(x, y)` is blighted (`IsPointBlighted`).
    pub fn is_blighted(&self, x: F32, y: F32) -> bool {
        self.misc.blight.contains(&(cell32(x), cell32(y)))
    }
}

/// The 32-unit cell of a coordinate.
fn cell32(v: F32) -> i32 {
    super::fog::cell_of_size(v, PATH_CELL)
}

/// The 128-unit tile of a coordinate.
fn tile_of(v: F32) -> i32 {
    super::fog::cell_of_size(v, TILE)
}

/// One axis of the playable-area clamp (`0x6F469490`): the lower bound first, then the upper bound minus the margin.
fn clamp_axis(v: F32, lo: F32, hi: F32) -> F32 {
    let hi = arith::sub(hi, CLAMP_MARGIN);
    if lo.to_f32() > v.to_f32() {
        return lo;
    }
    if hi.to_f32() < v.to_f32() {
        return hi;
    }
    v
}

/// Clamps `(x, y)` into the playable area. Without a playable area the point is kept.
fn clamp_point(playable: Option<Playable>, x: F32, y: F32) -> (F32, F32) {
    match playable {
        None => (x, y),
        Some(p) => (
            clamp_axis(x, p.min_x, p.max_x),
            clamp_axis(y, p.min_y, p.max_y),
        ),
    }
}

/// The terrain value at `(x, y)` (clamped): the script's write, then the provider, then `default`.
fn terrain_value<T>(
    gm: &Game,
    x: F32,
    y: F32,
    written: impl Fn(&MiscState, (i32, i32)) -> Option<T>,
    provided: impl Fn(&dyn TerrainProvider, F32, F32) -> Option<T>,
    default: T,
) -> T {
    let (x, y) = clamp_point(gm.playable(), x, y);
    if let Some(v) = written(&gm.misc, (tile_of(x), tile_of(y))) {
        return v;
    }
    if let Some(p) = gm.misc.terrain.as_deref() {
        if let Some(v) = provided(p, x, y) {
            return v;
        }
    }
    default
}

/// The location in handle argument `i`.
fn location_of(gm: &Game, a: &[Value], i: usize) -> Option<LocationObj> {
    let k = gm.handle_of_kind(a.get(i)?.as_handle()?, HandleKind::Location)?;
    match gm.object(k) {
        Object::Location(l) => Some(*l),
        _ => None,
    }
}

/// The rect in handle argument `i`.
fn rect_of(gm: &Game, a: &[Value], i: usize) -> Option<super::game::RectObj> {
    let k: ObjKey = gm.handle_of_kind(a.get(i)?.as_handle()?, HandleKind::Rect)?;
    match gm.object(k) {
        Object::Rect(r) => Some(*r),
        _ => None,
    }
}

/// The start location index argument `i` if it is below [`START_LOCATIONS`].
fn start_index(a: &[Value], i: usize) -> Option<usize> {
    let n = int_at(a, i);
    usize::try_from(n).ok().filter(|&n| n < START_LOCATIONS)
}

/// Adds (`add` true) or removes `cells` from the blight set.
fn blight_cells(gm: &mut Game, cells: &BTreeSet<(i32, i32)>, add: bool) {
    for &c in cells {
        if add {
            gm.misc.blight.insert(c);
        } else {
            gm.misc.blight.remove(&c);
        }
    }
}

/// Registers the misc natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    register_blight(reg, game);
    register_terrain(reg, game);
    register_preload(reg, game);
    register_start_locations(reg, game);
    register_players(reg, game);
}

fn register_blight(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native(reg, game, "SetBlight", |gm, _, a| {
        // (player, x, y, radius, add)
        if index_at(gm, a, 0).is_some() {
            let (x, y) = clamp_point(gm.playable(), real_at(a, 1), real_at(a, 2));
            let cells = circle_cells(x, y, real_at(a, 3), PATH_CELL);
            blight_cells(gm, &cells, bool_at(a, 4));
        }
        Value::Null
    });
    native(reg, game, "SetBlightLoc", |gm, _, a| {
        // (player, location, radius, add)
        if index_at(gm, a, 0).is_some() {
            if let Some(loc) = location_of(gm, a, 1) {
                let (x, y) = clamp_point(gm.playable(), loc.x, loc.y);
                let cells = circle_cells(x, y, real_at(a, 2), PATH_CELL);
                blight_cells(gm, &cells, bool_at(a, 3));
            }
        }
        Value::Null
    });
    native(reg, game, "SetBlightRect", |gm, _, a| {
        // (player, rect, add)
        if index_at(gm, a, 0).is_some() {
            if let Some(r) = rect_of(gm, a, 1) {
                let cells = rect_cells(&r, PATH_CELL);
                blight_cells(gm, &cells, bool_at(a, 2));
            }
        }
        Value::Null
    });
    native(reg, game, "SetBlightPoint", |gm, _, a| {
        // (player, x, y, add)
        if index_at(gm, a, 0).is_some() {
            let (x, y) = clamp_point(gm.playable(), real_at(a, 1), real_at(a, 2));
            let cell = BTreeSet::from([(cell32(x), cell32(y))]);
            blight_cells(gm, &cell, bool_at(a, 3));
        }
        Value::Null
    });
    native(reg, game, "IsPointBlighted", |gm, _, a| {
        Value::Boolean(gm.is_blighted(real_at(a, 0), real_at(a, 1)))
    });
}

fn register_terrain(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native(reg, game, "GetTerrainType", |gm, _, a| {
        let (x, y) = (real_at(a, 0), real_at(a, 1));
        let v = terrain_value(
            gm,
            x,
            y,
            |m, c| m.tiles.get(&c).map(|t| t.0),
            |p, x, y| p.tile_type(x, y),
            0,
        );
        Value::Integer(v as i32)
    });
    native(reg, game, "GetTerrainVariance", |gm, _, a| {
        let (x, y) = (real_at(a, 0), real_at(a, 1));
        let v = terrain_value(
            gm,
            x,
            y,
            |m, c| m.tiles.get(&c).map(|t| t.1),
            |p, x, y| p.tile_variance(x, y),
            0,
        );
        Value::Integer(v as i32)
    });
    native(reg, game, "GetTerrainCliffLevel", |gm, _, a| {
        let (x, y) = (real_at(a, 0), real_at(a, 1));
        let v = terrain_value(gm, x, y, |_, _| None, |p, x, y| p.cliff_level(x, y), 0);
        Value::Integer(v as i32)
    });
    native(reg, game, "SetTerrainType", |gm, _, a| {
        // (x, y, type, variation, area, shape); area and shape are not stored.
        let (x, y) = clamp_point(gm.playable(), real_at(a, 0), real_at(a, 1));
        let key = (tile_of(x), tile_of(y));
        gm.misc
            .tiles
            .insert(key, (raw_at(a, 2) as u32, raw_at(a, 3) as u32));
        Value::Null
    });
    native(reg, game, "IsTerrainPathable", |gm, _, a| {
        // (x, y, pathing type)
        let (x, y) = (real_at(a, 0), real_at(a, 1));
        let pathing = raw_at(a, 2) as u32;
        let (cx, cy) = (cell32(x), cell32(y));
        if let Some(&v) = gm.misc.pathing.get(&(pathing, cx, cy)) {
            return Value::Boolean(v);
        }
        let (x, y) = clamp_point(gm.playable(), x, y);
        let provided = gm
            .misc
            .terrain
            .as_deref()
            .and_then(|p| p.pathable(x, y, pathing));
        Value::Boolean(provided.unwrap_or(true))
    });
    native(reg, game, "SetTerrainPathable", |gm, _, a| {
        // (x, y, pathing type, flag)
        let pathing = raw_at(a, 2) as u32;
        let key = (pathing, cell32(real_at(a, 0)), cell32(real_at(a, 1)));
        gm.misc.pathing.insert(key, bool_at(a, 3));
        Value::Null
    });
}

fn register_preload(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native(reg, game, "Preload", |gm, ctx, a| {
        log_preload(gm, "Preload", str_at(ctx, a, 0));
        Value::Null
    });
    native(reg, game, "PreloadEnd", |gm, _, a| {
        log_preload(gm, "PreloadEnd", format!("{}", real_at(a, 0).to_f32()));
        Value::Null
    });
    native(reg, game, "PreloadStart", |gm, _, _| {
        log_preload(gm, "PreloadStart", String::new());
        Value::Null
    });
    native(reg, game, "PreloadRefresh", |gm, _, _| {
        log_preload(gm, "PreloadRefresh", String::new());
        Value::Null
    });
    native(reg, game, "PreloadEndEx", |gm, _, _| {
        log_preload(gm, "PreloadEndEx", String::new());
        Value::Null
    });
    native(reg, game, "PreloadGenStart", |gm, _, _| {
        log_preload(gm, "PreloadGenStart", String::new());
        Value::Null
    });
    native(reg, game, "PreloadGenClear", |gm, _, _| {
        log_preload(gm, "PreloadGenClear", String::new());
        Value::Null
    });
    native(reg, game, "PreloadGenEnd", |gm, ctx, a| {
        log_preload(gm, "PreloadGenEnd", str_at(ctx, a, 0));
        Value::Null
    });
    native(reg, game, "Preloader", |gm, ctx, a| {
        log_preload(gm, "Preloader", str_at(ctx, a, 0));
        Value::Null
    });
}

fn log_preload(gm: &mut Game, native: &'static str, text: String) {
    gm.misc.log.push(MiscEvent::Preload { native, text });
}

fn register_start_locations(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native(reg, game, "DefineStartLocation", |gm, _, a| {
        if let Some(i) = start_index(a, 0) {
            gm.misc.start_locs[i] = (real_at(a, 1), real_at(a, 2));
        }
        Value::Null
    });
    native(reg, game, "DefineStartLocationLoc", |gm, _, a| {
        if let (Some(i), Some(loc)) = (start_index(a, 0), location_of(gm, a, 1)) {
            gm.misc.start_locs[i] = (loc.x, loc.y);
        }
        Value::Null
    });
    native(reg, game, "GetStartLocationX", |gm, _, a| {
        let v = start_index(a, 0).map_or(F32::ZERO, |i| gm.misc.start_locs[i].0);
        Value::Real(v)
    });
    native(reg, game, "GetStartLocationY", |gm, _, a| {
        let v = start_index(a, 0).map_or(F32::ZERO, |i| gm.misc.start_locs[i].1);
        Value::Real(v)
    });
    native(reg, game, "GetStartLocationLoc", |gm, _, a| {
        let Some(i) = start_index(a, 0) else {
            return Value::Handle(0);
        };
        let (x, y) = gm.misc.start_locs[i];
        Value::Handle(gm.add_object(Object::Location(LocationObj { x, y }), 1).1)
    });
    native(reg, game, "SetStartLocPrioCount", |gm, _, a| {
        if let (Some(loc), Some(count)) = (start_index(a, 0), start_index(a, 1)) {
            gm.misc.start_prio_count[loc] = count as u32;
        }
        Value::Null
    });
    native(reg, game, "SetStartLocPrio", |gm, _, a| {
        // (location, slot, other location, priority type)
        let (Some(loc), Some(slot), Some(other)) =
            (start_index(a, 0), start_index(a, 1), start_index(a, 2))
        else {
            return Value::Null;
        };
        let ty = int_at(a, 3);
        if (0..3).contains(&ty) {
            gm.misc.start_prio[loc][slot] = (other as u32, ty as u32);
        }
        Value::Null
    });
    native(reg, game, "GetStartLocPrioSlot", |gm, _, a| {
        let v = match (start_index(a, 0), start_index(a, 1)) {
            (Some(loc), Some(slot)) => gm.misc.start_prio[loc][slot].0 as i32,
            _ => 0,
        };
        Value::Integer(v)
    });
    native(reg, game, "GetStartLocPrio", |gm, _, a| {
        let v = match (start_index(a, 0), start_index(a, 1)) {
            (Some(loc), Some(slot)) => gm.misc.start_prio[loc][slot].1 as i32,
            _ => 0,
        };
        Value::Integer(v)
    });
    native(reg, game, "ForcePlayerStartLocation", |gm, _, a| {
        if let Some(p) = index_at(gm, a, 0) {
            if let Some(player) = gm.player_mut(p) {
                player.start_location = int_at(a, 1);
            }
        }
        Value::Null
    });
    native(reg, game, "GetPlayerStartLocationX", |gm, _, a| {
        Value::Real(player_start(gm, a).map_or(F32::ZERO, |(x, _)| x))
    });
    native(reg, game, "GetPlayerStartLocationY", |gm, _, a| {
        Value::Real(player_start(gm, a).map_or(F32::ZERO, |(_, y)| y))
    });
}

/// The position of the start location of the player in argument 0.
fn player_start(gm: &Game, a: &[Value]) -> Option<(F32, F32)> {
    let p = index_at(gm, a, 0)?;
    let loc = gm.player(p).start_location;
    usize::try_from(loc)
        .ok()
        .filter(|&i| i < START_LOCATIONS)
        .map(|i| gm.misc.start_locs[i])
}

fn register_players(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native(reg, game, "SetPlayerRacePreference", |gm, _, a| {
        // The selectable bit is kept; the other bits are the preference.
        if let Some(p) = index_at(gm, a, 0) {
            let pref = raw_at(a, 1) as u32;
            let w = &mut gm.misc.players[p as usize].race_flags;
            *w = (*w & RACE_SELECTABLE) | (pref & !RACE_SELECTABLE);
        }
        Value::Null
    });
    native(reg, game, "IsPlayerRacePrefSet", |gm, _, a| {
        let pref = raw_at(a, 1) as u32;
        let v = index_at(gm, a, 0)
            .is_some_and(|p| gm.misc.players[p as usize].race_flags & pref & !RACE_SELECTABLE != 0);
        Value::Boolean(v)
    });
    native(reg, game, "SetPlayerRaceSelectable", |gm, _, a| {
        if let Some(p) = index_at(gm, a, 0) {
            let w = &mut gm.misc.players[p as usize].race_flags;
            if bool_at(a, 1) {
                *w |= RACE_SELECTABLE;
            } else {
                *w &= !RACE_SELECTABLE;
            }
        }
        Value::Null
    });
    native(reg, game, "GetPlayerSelectable", |gm, _, a| {
        let v = index_at(gm, a, 0)
            .is_some_and(|p| gm.misc.players[p as usize].race_flags & RACE_SELECTABLE != 0);
        Value::Boolean(v)
    });
    native(reg, game, "SetPlayerOnScoreScreen", |gm, _, a| {
        if let Some(p) = index_at(gm, a, 0) {
            gm.misc.players[p as usize].on_score_screen = bool_at(a, 1);
        }
        Value::Null
    });
    native(reg, game, "GetPlayerHandicap", |gm, _, a| {
        let v = index_at(gm, a, 0).map_or(ONE, |p| gm.misc.players[p as usize].handicap);
        Value::Real(v)
    });
    native(reg, game, "SetPlayerHandicap", |gm, _, a| {
        let h = real_at(a, 1);
        let mag = if arith::lt(h, F32::ZERO) {
            arith::neg(h)
        } else {
            h
        };
        if let Some(p) = index_at(gm, a, 0) {
            if arith::le(HANDICAP_MIN, mag) {
                gm.misc.players[p as usize].handicap = h;
            }
        }
        Value::Null
    });
    native(reg, game, "GetPlayerHandicapXP", |gm, _, a| {
        let v = index_at(gm, a, 0).map_or(ONE, |p| gm.misc.players[p as usize].handicap_xp);
        Value::Real(v)
    });
    native(reg, game, "SetPlayerHandicapXP", |gm, _, a| {
        if let Some(p) = index_at(gm, a, 0) {
            gm.misc.players[p as usize].handicap_xp = real_at(a, 1);
        }
        Value::Null
    });
    native(reg, game, "SetPlayerTaxRate", |gm, _, a| {
        // (giver, receiver, state, rate). The rate is stored under the giver for the receiver and the state.
        if gm.game_state().map_flags & FLAG_TAX_LOCKED != 0 {
            return Value::Null;
        }
        let (Some(giver), Some(receiver)) = (index_at(gm, a, 0), index_at(gm, a, 1)) else {
            return Value::Null;
        };
        let state = int_at(a, 2);
        if state == 1 || state == 2 {
            gm.misc.players[giver as usize]
                .tax
                .insert((receiver, state as u32), int_at(a, 3));
        }
        Value::Null
    });
    native(reg, game, "GetPlayerTaxRate", |gm, _, a| {
        let (Some(giver), Some(receiver)) = (index_at(gm, a, 0), index_at(gm, a, 1)) else {
            return Value::Integer(0);
        };
        let state = int_at(a, 2);
        let v = if state == 1 || state == 2 {
            gm.misc.players[giver as usize]
                .tax
                .get(&(receiver, state as u32))
                .copied()
                .unwrap_or(0)
        } else {
            0
        };
        Value::Integer(v)
    });
    native(reg, game, "GetPlayerTechMaxAllowed", |gm, _, a| {
        let v = match (index_at(gm, a, 0), raw_at(a, 1) as u32) {
            (Some(p), tech) => tech_of(gm, p, tech).max_allowed,
            _ => 0,
        };
        Value::Integer(v)
    });
    native(reg, game, "SetPlayerTechMaxAllowed", |gm, _, a| {
        if let Some(p) = index_at(gm, a, 0) {
            let tech = raw_at(a, 1) as u32;
            gm.misc.players[p as usize]
                .tech
                .entry(tech)
                .or_insert_with(default_tech)
                .max_allowed = int_at(a, 2);
        }
        Value::Null
    });
    native(reg, game, "AddPlayerTechResearched", |gm, _, a| {
        if let Some(p) = index_at(gm, a, 0) {
            add_tech(gm, p, raw_at(a, 1) as u32, int_at(a, 2));
        }
        Value::Null
    });
    native(reg, game, "SetPlayerTechResearched", |gm, _, a| {
        // Only raises the level: the positive difference to the target is added.
        if let Some(p) = index_at(gm, a, 0) {
            let tech = raw_at(a, 1) as u32;
            let diff = int_at(a, 2).wrapping_sub(tech_of(gm, p, tech).researched);
            if diff >= 1 {
                add_tech(gm, p, tech, diff);
            }
        }
        Value::Null
    });
    native(reg, game, "GetPlayerTechResearched", |gm, _, a| {
        let v =
            index_at(gm, a, 0).is_some_and(|p| tech_of(gm, p, raw_at(a, 1) as u32).researched > 0);
        Value::Boolean(v)
    });
    native(reg, game, "GetPlayerTechCount", |gm, _, a| {
        let v = index_at(gm, a, 0).map_or(0, |p| tech_of(gm, p, raw_at(a, 1) as u32).researched);
        Value::Integer(v)
    });
    native(reg, game, "SetPlayerAbilityAvailable", |gm, _, a| {
        if let Some(p) = index_at(gm, a, 0) {
            let ability = raw_at(a, 1) as u32;
            let set = &mut gm.misc.players[p as usize].unavailable_abilities;
            if bool_at(a, 2) {
                set.remove(&ability);
            } else {
                set.insert(ability);
            }
        }
        Value::Null
    });
}

fn default_tech() -> TechLevel {
    TechLevel {
        researched: 0,
        max_allowed: i32::MAX,
    }
}

/// The tech level of player `p` for `tech` (the default when it was never written).
fn tech_of(gm: &Game, p: u32, tech: u32) -> TechLevel {
    gm.misc.players[p as usize]
        .tech
        .get(&tech)
        .copied()
        .unwrap_or_else(default_tech)
}

/// Adds `count` levels, one at a time, while the level is below the maximum (`AddPlayerTechResearched`).
fn add_tech(gm: &mut Game, p: u32, tech: u32, count: i32) {
    let mut level = tech_of(gm, p, tech);
    let mut left = count;
    while left > 0 && level.researched < level.max_allowed {
        level.researched = level.researched.wrapping_add(1);
        left -= 1;
    }
    gm.misc.players[p as usize].tech.insert(tech, level);
}
