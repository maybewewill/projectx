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
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use jass::vm::{NativeRegistry, Value};
use softfloat::{arith, conv, F32};

use super::game::{Game, HandleKind, LocationObj, ObjKey, Object, RectObj};
use super::gamestate::native;
use super::hashtable::{bool_at, int_at, real_at};
use super::player::index_at;

/// The fog state values of the model (CONFIRMED: the values the grid query returns).
pub const FOG_MASKED: i32 = 1;
pub const FOG_FOGGED: i32 = 2;
pub const FOG_VISIBLE: i32 = 4;

/// The width of a fog cell in world units (UNSURE, see the module notes).
pub const FOG_CELL: i32 = 128;

/// The alliance type whose row gives the players that a shared-vision modifier covers (UNSURE, see the module notes).
pub const SHARED_VISION_ALLIANCE: usize = 5;

/// A cell key: the player slot and the cell.
pub type CellKey = (u32, i32, i32);

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

/// The fog-of-war state of the game: per-cell counts and explicit states. See the module notes.
#[derive(Debug, Clone, Default)]
pub struct FogState {
    /// Number of active visible modifiers per cell.
    visible: BTreeMap<CellKey, u32>,
    /// Number of active masked modifiers per cell.
    masked: BTreeMap<CellKey, u32>,
    /// Number of active fogged modifiers per cell.
    fogged: BTreeMap<CellKey, u32>,
    /// The explicit state that `SetFogState*` wrote per cell.
    explicit: BTreeMap<CellKey, i32>,
    /// Cells that a visible modifier has covered.
    explored: BTreeSet<CellKey>,
}

/// The cell index of a world coordinate: `floor(v / FOG_CELL)`. The division is by a power of two, so it is exact.
pub(super) fn cell_of(v: F32) -> i32 {
    cell_of_size(v, FOG_CELL)
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
        self.fog_state_of_cell(player, (cell_of(x), cell_of(y)))
    }

    /// The fog state of a cell (see the module notes for the order of the checks).
    pub fn fog_state_of_cell(&self, player: u32, cell: (i32, i32)) -> i32 {
        let key = (player, cell.0, cell.1);
        let f = &self.fog;
        if f.visible.get(&key).copied().unwrap_or(0) > 0 {
            FOG_VISIBLE
        } else if f.masked.get(&key).copied().unwrap_or(0) > 0 {
            FOG_MASKED
        } else if f.fogged.get(&key).copied().unwrap_or(0) > 0 {
            FOG_FOGGED
        } else if let Some(&s) = f.explicit.get(&key) {
            s
        } else if f.explored.contains(&key) {
            FOG_FOGGED
        } else {
            FOG_MASKED
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

    /// Adds (`add` true) or removes the counts of modifier `k`, for each player in its mask.
    fn fog_apply(&mut self, k: ObjKey, add: bool) {
        let m = self.fog_modifier(k).clone();
        let f = &mut self.fog;
        for p in 0..16u32 {
            if m.players & (1 << p) == 0 {
                continue;
            }
            for &(cx, cy) in &m.cells {
                let key = (p, cx, cy);
                let map = match m.state {
                    FOG_VISIBLE => &mut f.visible,
                    FOG_FOGGED => &mut f.fogged,
                    _ => &mut f.masked,
                };
                if add {
                    *map.entry(key).or_insert(0) += 1;
                    if m.state == FOG_VISIBLE {
                        f.explored.insert(key);
                    }
                } else if let Some(n) = map.get_mut(&key) {
                    *n = n.saturating_sub(1);
                    if *n == 0 {
                        map.remove(&key);
                    }
                }
            }
        }
    }

    /// Sets the explicit state of the cells for each player in `players`.
    fn fog_set_explicit(&mut self, players: u32, cells: &BTreeSet<(i32, i32)>, state: i32) {
        for p in 0..16u32 {
            if players & (1 << p) == 0 {
                continue;
            }
            for &(cx, cy) in cells {
                self.fog.explicit.insert((p, cx, cy), state);
                if state == FOG_VISIBLE || state == FOG_FOGGED {
                    self.fog.explored.insert((p, cx, cy));
                }
            }
        }
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
        let cells = circle_cells(x, y, r, FOG_CELL);
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
        let cells = circle_cells(loc.x, loc.y, real_at(a, 3), FOG_CELL);
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
        let cells = rect_cells(&r, FOG_CELL);
        Value::Handle(gm.fog_create(p, int_at(a, 1), cells, bool_at(a, 3), bool_at(a, 4)))
    });

    native(reg, game, "DestroyFogModifier", |gm, _, a| {
        if let Some(k) = handle_fog(gm, a, 0) {
            gm.fog_apply(k, false);
            gm.fog_obj_mut(k).active = false;
            gm.fog_obj_mut(k).destroyed = true;
            gm.kill(k);
        }
        Value::Null
    });

    native(reg, game, "FogModifierStart", |gm, _, a| {
        if let Some(k) = handle_fog(gm, a, 0) {
            if !gm.fog_modifier(k).active {
                gm.fog_apply(k, true);
                gm.fog_obj_mut(k).active = true;
            }
        }
        Value::Null
    });

    native(reg, game, "FogModifierStop", |gm, _, a| {
        if let Some(k) = handle_fog(gm, a, 0) {
            if gm.fog_modifier(k).active {
                gm.fog_apply(k, false);
                gm.fog_obj_mut(k).active = false;
            }
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
        let cells = rect_cells(&r, FOG_CELL);
        gm.fog_set_explicit(players, &cells, int_at(a, 1));
        Value::Null
    });

    native(reg, game, "SetFogStateRadius", |gm, _, a| {
        // (player, fogstate, x, y, radius, useShared)
        let Some(p) = index_at(gm, a, 0) else {
            return Value::Null;
        };
        let cells = circle_cells(real_at(a, 2), real_at(a, 3), real_at(a, 4), FOG_CELL);
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
        let cells = circle_cells(loc.x, loc.y, real_at(a, 3), FOG_CELL);
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
    /// The fog modifier behind `key`, mutable. Panics if `key` is not a fog modifier.
    fn fog_obj_mut(&mut self, key: ObjKey) -> &mut FogModifierObj {
        match self.object_mut(key) {
            Object::Fog(f) => f,
            _ => panic!("not a fog modifier"),
        }
    }
}
