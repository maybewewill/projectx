//! Fog of war natives: `CreateFogModifierRadius` / `...RadiusLoc` / `...Rect`, `DestroyFogModifier`,
//! `FogModifierStart` / `FogModifierStop`, `SetFogStateRect` / `SetFogStateRadius` / `SetFogStateRadiusLoc`, and the
//! visibility queries `IsVisibleToPlayer`, `IsFoggedToPlayer`, `IsMaskedToPlayer` and their location forms.
//!
//! Model. The original keeps a fog grid per player (a bit per player slot in the grid cells, read through the
//! helpers `0x6f00e7a0` and `0x6f26d0c0`). A point query returns one of three states: visible (4), fogged (2) or
//! masked (1). The grid itself and its sight counts are not modelled: vision comes from fog modifiers and from
//! the explicit states the natives set. The model keeps the state per player and cell as follows.
//!
//! * A cell is 128 world units wide (UNSURE: the original's fog cell size was not read; it is a single constant,
//!   [`FOG_CELL`]). Cell `(cx, cy)` covers `[cx * 128, cx * 128 + 128)` on each axis.
//! * Each player has, per cell, the number of active modifiers of each state (visible, fogged, masked), an
//!   explicit state (what `SetFogState*` wrote, if anything), and an "explored" bit (a visible modifier covered the
//!   cell at some time).
//! * The state of a point is the first that applies, in this order: an active visible modifier gives visible; an
//!   active masked modifier gives masked; an active fogged modifier gives fogged; the explicit state; the explored
//!   bit gives fogged; otherwise masked. The order is UNSURE: it is the model's reading of the three states.
//!
//! Handles (CONFIRMED). The three creators (`0x6f3d0f90`, `0x6f3d10f0`, `0x6f3d0e70`) resolve the player first and
//! return null when the player is not live. A location or rect argument that does not resolve also returns null,
//! without allocating anything. Otherwise the modifier is created and its handle is allocated last with flag 0 (the
//! `FUN_6f430c80(obj, 0)` call). A modifier starts inactive: `FogModifierStart` sets the active bit (`+0x20` bit 6)
//! and `FogModifierStop` clears it (CONFIRMED, `0x6f3c1bc0` and `0x6f3c1be0`).
//!
//! Shared vision (LIKELY). With `useShared` true the modifier also covers the players that the owner shares vision
//! with: the players in the owner's row of alliance type [`SHARED_VISION_ALLIANCE`] (UNSURE: the index of the type and
//! the original's shared mask at `+0x2e0` were not read).
//!
//! Addresses (CONFIRMED values from `natives.c` unless marked):
//!
//! | Native | Address | Status | Notes |
//! |---|---|---|---|
//! | `CreateFogModifierRadius` | 0x6f3d0f90 | CONFIRMED handle order, UNSURE cells | returns null for a dead player |
//! | `CreateFogModifierRadiusLoc` | 0x6f3d10f0 | CONFIRMED handle order | null for a dead location |
//! | `CreateFogModifierRect` | 0x6f3d0e70 | CONFIRMED handle order | null for a dead rect |
//! | `DestroyFogModifier` | 0x6f3c1ba0 | CONFIRMED structure (destroy slot), LIKELY release | deactivates, then releases the creation reference |
//! | `FogModifierStart` | 0x6f3c1bc0 | CONFIRMED | sets active |
//! | `FogModifierStop` | 0x6f3c1be0 | CONFIRMED | clears active |
//! | `SetFogStateRect` | 0x6f3c1a30 | CONFIRMED structure, UNSURE store | writes the explicit states of the cells |
//! | `SetFogStateRadius` | 0x6f3c1ab0 | CONFIRMED structure, UNSURE store | |
//! | `SetFogStateRadiusLoc` | 0x6f3c1b20 | CONFIRMED structure, UNSURE store | |
//! | `IsVisibleToPlayer` | 0x6f3c98f0 | CONFIRMED (grid state 4) | model state 4 |
//! | `IsFoggedToPlayer` | 0x6f3c9980 | CONFIRMED (grid state 2) | model state 2 |
//! | `IsMaskedToPlayer` | 0x6f3c9a10 | CONFIRMED (grid state 1) | model state 1 |
//! | `IsLocationVisibleToPlayer` | 0x6f3c9940 | CONFIRMED | point form |
//! | `IsLocationFoggedToPlayer` | 0x6f3c99d0 | CONFIRMED | point form |
//! | `IsLocationMaskedToPlayer` | 0x6f3c9a60 | CONFIRMED | point form |
//!
//! Not modelled: the sight counts that units add (units are not in this module), the `afterUnits` flag (stored only,
//! UNSURE), and the rendering of the fog. The tests are in `tests/fog_natives.rs`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use jass::vm::{NativeRegistry, Value};
use softfloat::{arith, conv, F32};

use super::game::{Game, HandleKind, LocationObj, ObjKey, Object, RectObj};
use super::gamestate::native;
use super::hashtable::{bool_at, int_at, real_at};
use super::player::index_at;
use crate::vision::VisionGrid;

pub use crate::vision::{FOG_FOGGED, FOG_MASKED, FOG_VISIBLE};

/// The width of a fog cell in world units (UNSURE: the grid's scale, see `vision::VisionGrid::scale`).
pub const FOG_CELL: i32 = 128;

/// The alliance type whose row gives the players that a shared-vision modifier covers (UNSURE, see the module notes).
pub const SHARED_VISION_ALLIANCE: usize = 5;


/// A fog modifier (`fogmodifier` in JASS). Its cells and players are fixed at creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FogModifierObj {
    /// The fog state it applies while active (`FOG_VISIBLE`, `FOG_FOGGED` or `FOG_MASKED`).
    pub state: i32,
    /// The player slots whose grids it writes, as a bit mask (the owner, plus shared-vision players when asked).
    pub players: u32,
    /// The cells it covers.
    pub cells: BTreeSet<(i32, i32)>,
    /// Set by `FogModifierStart`, cleared by `FogModifierStop`. A modifier starts inactive.
    pub active: bool,
    /// The `afterUnits` argument (stored only, UNSURE).
    pub after_units: bool,
    /// Set by `DestroyFogModifier`. A destroyed modifier has no live handle for the natives.
    pub destroyed: bool,
}

/// The fog-of-war state of the game: the cell grid of `vision` and the active fog modifiers, in the order they
/// were started (the order the rebuild re-applies them in, UNSURE).
#[derive(Debug, Clone, Default)]
pub struct FogState {
    /// Cell words per player (see `vision.rs`).
    pub grid: VisionGrid,
    /// Active fog modifiers, in start order.
    active: Vec<ObjKey>,
}

/// The cells that a rect covers (both ends inclusive).
pub(super) fn rect_cells(r: &RectObj, cell: i32) -> BTreeSet<(i32, i32)> {
    let mut out = BTreeSet::new();
    let (x0, x1) = (cell_of_size(r.min_x, cell), cell_of_size(r.max_x, cell));
    let (y0, y1) = (cell_of_size(r.min_y, cell), cell_of_size(r.max_y, cell));
    for cx in x0..=x1 {
        for cy in y0..=y1 {
            out.insert((cx, cy));
        }
    }
    out
}

/// The cells whose centre lies within `radius` of `(x, y)` (squared distance compared with `radius * radius`, the
/// same test as the unit queries). The bounding box of the search is the radius on each side.
pub(super) fn circle_cells(x: F32, y: F32, radius: F32, cell: i32) -> BTreeSet<(i32, i32)> {
    let mut out = BTreeSet::new();
    let r2 = arith::mul(radius, radius);
    let x0 = cell_of_size(arith::sub(x, radius), cell);
    let x1 = cell_of_size(arith::add(x, radius), cell);
    let y0 = cell_of_size(arith::sub(y, radius), cell);
    let y1 = cell_of_size(arith::add(y, radius), cell);
    let half = conv::i2r(cell / 2);
    for cx in x0..=x1 {
        for cy in y0..=y1 {
            let centre_x = arith::add(conv::i2r(cx * cell), half);
            let centre_y = arith::add(conv::i2r(cy * cell), half);
            let d = arith::dist2(centre_x, centre_y, x, y);
            if arith::le(d, r2) {
                out.insert((cx, cy));
            }
        }
    }
    out
}

/// The cell index of a coordinate for cells of `cell` units.
pub(super) fn cell_of_size(v: F32, cell: i32) -> i32 {
    (v.to_f32() / cell as f32).floor() as i32
}

impl Game {
    /// The fog state of point `(x, y)` for player slot `player`: [`FOG_VISIBLE`], [`FOG_FOGGED`] or [`FOG_MASKED`].
    pub fn fog_state_at(&self, player: u32, x: F32, y: F32) -> i32 {
        self.fog.grid.state_at(x, y, player)
    }

    /// The fog state of a cell for player slot `player` (the table of `vision::table_state`).
    pub fn fog_state_of_cell(&self, player: u32, cell: (i32, i32)) -> i32 {
        self.fog.grid.state(cell, player)
    }

    /// Rebuilds the visible layer: clears it, then re-applies every active modifier in start order. The
    /// explored layer is kept, as in the original's refresh (UNSURE: the original clears the visible layer only on
    /// some refreshes, `DAT_6fab6a34 & 3`, and that counter's writer is not found yet).
    fn fog_refresh(&mut self) {
        self.fog.grid.clear_visible();
        let active = self.fog.active.clone();
        for k in active {
            let m = self.fog_modifier(k).clone();
            self.fog.grid.write_cells(m.state, (m.players & 0xFFFF) as u16, &m.cells);
        }
    }

    /// The fog modifier behind `key`. Panics if `key` is not a fog modifier.
    pub fn fog_modifier(&self, key: ObjKey) -> &FogModifierObj {
        match self.object(key) {
            Object::Fog(f) => f,
            _ => panic!("not a fog modifier"),
        }
    }

    /// The live fog modifier behind handle `id`, or `None` for a null, freed, destroyed or wrong-type handle.
    pub fn fog_modifier_of_handle(&self, id: u32) -> Option<ObjKey> {
        let k = self.handle_of_kind(id, HandleKind::FogModifier)?;
        (!self.fog_modifier(k).destroyed).then_some(k)
    }

    /// The player slots whose grids a modifier with `useShared` writes: the owner, and the players the owner shares
    /// vision with (UNSURE, see [`SHARED_VISION_ALLIANCE`]).
    fn fog_player_mask(&self, owner: u32, use_shared: bool) -> u32 {
        let own = 1u32 << owner;
        if !use_shared {
            return own;
        }
        own | self.player(owner).alliances[SHARED_VISION_ALLIANCE]
    }

    /// Starts modifier `k`: writes its shape with its mode for each player in its mask. The original writes the
    /// shape at its next refresh; the write is immediate here (UNSURE).
    fn fog_apply(&mut self, k: ObjKey) {
        let m = self.fog_modifier(k).clone();
        self.fog.grid.write_cells(m.state, (m.players & 0xFFFF) as u16, &m.cells);
    }

    /// Writes an explicit state (`SetFogState*`) for each player in `players`, with the modifier writer
    /// (CONFIRMED: `0x6f3c1a30` and its siblings reach the same writers as the modifiers).
    fn fog_set_explicit(&mut self, players: u32, cells: &BTreeSet<(i32, i32)>, state: i32) {
        self.fog.grid.write_cells(state, (players & 0xFFFF) as u16, cells);
    }

    /// Creates a fog modifier (inactive) and returns its handle, or 0 when the player is not live.
    /// `cells` is computed by the caller from the shape it was given.
    fn fog_create(
        &mut self,
        owner: u32,
        state: i32,
        cells: BTreeSet<(i32, i32)>,
        use_shared: bool,
        after_units: bool,
    ) -> u32 {
        let players = self.fog_player_mask(owner, use_shared);
        let obj = Object::Fog(FogModifierObj {
            state,
            players,
            cells,
            active: false,
            after_units,
            destroyed: false,
        });
        self.add_object(obj, 0).1
    }
}

/// Registers the fog natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    native(reg, game, "CreateFogModifierRadius", |gm, _, a| {
        // (player, fogstate, x, y, radius, useShared, afterUnits)
        let Some(p) = index_at(gm, a, 0) else {
            return Value::Handle(0);
        };
        let (x, y, r) = (real_at(a, 2), real_at(a, 3), real_at(a, 4));
        let cells = gm.fog.grid.circle_shape(x, y, r);
        Value::Handle(gm.fog_create(p, int_at(a, 1), cells, bool_at(a, 5), bool_at(a, 6)))
    });

    native(reg, game, "CreateFogModifierRadiusLoc", |gm, _, a| {
        // (player, fogstate, location, radius, useShared, afterUnits)
        let Some(p) = index_at(gm, a, 0) else {
            return Value::Handle(0);
        };
        let Some(loc) = location_of(gm, a, 2) else {
            return Value::Handle(0);
        };
        let cells = gm.fog.grid.circle_shape(loc.x, loc.y, real_at(a, 3));
        Value::Handle(gm.fog_create(p, int_at(a, 1), cells, bool_at(a, 4), bool_at(a, 5)))
    });

    native(reg, game, "CreateFogModifierRect", |gm, _, a| {
        // (player, fogstate, rect, useShared, afterUnits)
        let Some(p) = index_at(gm, a, 0) else {
            return Value::Handle(0);
        };
        let Some(r) = rect_of(gm, a, 2) else {
            return Value::Handle(0);
        };
        let cells = gm.fog.grid.rect_shape((r.min_x, r.min_y), (r.max_x, r.max_y));
        Value::Handle(gm.fog_create(p, int_at(a, 1), cells, bool_at(a, 3), bool_at(a, 4)))
    });

    native(reg, game, "DestroyFogModifier", |gm, _, a| {
        if let Some(k) = handle_fog(gm, a, 0) {
            gm.fog_stop(k);
            gm.fog_obj_mut(k).destroyed = true;
            gm.kill(k);
        }
        Value::Null
    });

    native(reg, game, "FogModifierStart", |gm, _, a| {
        if let Some(k) = handle_fog(gm, a, 0) {
            gm.fog_start(k);
        }
        Value::Null
    });

    native(reg, game, "FogModifierStop", |gm, _, a| {
        if let Some(k) = handle_fog(gm, a, 0) {
            gm.fog_stop(k);
        }
        Value::Null
    });

    native(reg, game, "SetFogStateRect", |gm, _, a| {
        // (player, fogstate, rect, useShared)
        let Some(p) = index_at(gm, a, 0) else {
            return Value::Null;
        };
        let Some(r) = rect_of(gm, a, 2) else {
            return Value::Null;
        };
        let players = gm.fog_player_mask(p, bool_at(a, 3));
        let cells = gm.fog.grid.rect_shape((r.min_x, r.min_y), (r.max_x, r.max_y));
        gm.fog_set_explicit(players, &cells, int_at(a, 1));
        Value::Null
    });

    native(reg, game, "SetFogStateRadius", |gm, _, a| {
        // (player, fogstate, x, y, radius, useShared)
        let Some(p) = index_at(gm, a, 0) else {
            return Value::Null;
        };
        let cells = gm.fog.grid.circle_shape(real_at(a, 2), real_at(a, 3), real_at(a, 4));
        let players = gm.fog_player_mask(p, bool_at(a, 5));
        gm.fog_set_explicit(players, &cells, int_at(a, 1));
        Value::Null
    });

    native(reg, game, "SetFogStateRadiusLoc", |gm, _, a| {
        // (player, fogstate, location, radius, useShared)
        let Some(p) = index_at(gm, a, 0) else {
            return Value::Null;
        };
        let Some(loc) = location_of(gm, a, 2) else {
            return Value::Null;
        };
        let cells = gm.fog.grid.circle_shape(loc.x, loc.y, real_at(a, 3));
        let players = gm.fog_player_mask(p, bool_at(a, 4));
        gm.fog_set_explicit(players, &cells, int_at(a, 1));
        Value::Null
    });

    // Point queries: (x, y, player) and (location, player).
    native(reg, game, "IsVisibleToPlayer", |gm, _, a| {
        point_state(gm, real_at(a, 0), real_at(a, 1), a, 2, FOG_VISIBLE)
    });
    native(reg, game, "IsFoggedToPlayer", |gm, _, a| {
        point_state(gm, real_at(a, 0), real_at(a, 1), a, 2, FOG_FOGGED)
    });
    native(reg, game, "IsMaskedToPlayer", |gm, _, a| {
        point_state(gm, real_at(a, 0), real_at(a, 1), a, 2, FOG_MASKED)
    });
    native(reg, game, "IsLocationVisibleToPlayer", |gm, _, a| {
        loc_state(gm, a, FOG_VISIBLE)
    });
    native(reg, game, "IsLocationFoggedToPlayer", |gm, _, a| {
        loc_state(gm, a, FOG_FOGGED)
    });
    native(reg, game, "IsLocationMaskedToPlayer", |gm, _, a| {
        loc_state(gm, a, FOG_MASKED)
    });
}

/// Whether the point's state is `want`. A null player gives false (the original returns before the query).
fn point_state(gm: &Game, x: F32, y: F32, a: &[Value], player_at: usize, want: i32) -> Value {
    match index_at(gm, a, player_at) {
        Some(p) => Value::Boolean(gm.fog_state_at(p, x, y) == want),
        None => Value::Boolean(false),
    }
}

/// The location query: the point of location argument 0, player argument 1.
fn loc_state(gm: &Game, a: &[Value], want: i32) -> Value {
    let Some(loc) = location_of(gm, a, 0) else {
        return Value::Boolean(false);
    };
    point_state(gm, loc.x, loc.y, a, 1, want)
}

/// The live fog modifier in handle argument `i`.
fn handle_fog(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.fog_modifier_of_handle(a.get(i)?.as_handle()?)
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
fn rect_of(gm: &Game, a: &[Value], i: usize) -> Option<RectObj> {
    let k = gm.handle_of_kind(a.get(i)?.as_handle()?, HandleKind::Rect)?;
    match gm.object(k) {
        Object::Rect(r) => Some(*r),
        _ => None,
    }
}

impl Game {
    /// Starts modifier `k` (no effect when it is already active): it joins the active list and its shape is written.
    fn fog_start(&mut self, k: ObjKey) {
        if self.fog_modifier(k).active {
            return;
        }
        self.fog.active.push(k);
        self.fog_apply(k);
        self.fog_obj_mut(k).active = true;
    }

    /// Stops modifier `k` (no effect when it is inactive) and rebuilds the visible layer from the remaining sources.
    fn fog_stop(&mut self, k: ObjKey) {
        if !self.fog_modifier(k).active {
            return;
        }
        self.fog.active.retain(|&x| x != k);
        self.fog_obj_mut(k).active = false;
        self.fog_refresh();
    }

    /// The fog modifier behind `key`, mutable. Panics if `key` is not a fog modifier.
    fn fog_obj_mut(&mut self, key: ObjKey) -> &mut FogModifierObj {
        match self.object_mut(key) {
            Object::Fog(f) => f,
            _ => panic!("not a fog modifier"),
        }
    }
}
