//! The AI manager: what the production and harvest natives of a computer player do to the game.
//!
//! The AI scripts do not order units themselves. They call `SetProduce`, `SetUpgrade`, `HarvestGold`, `HarvestWood`
//! and `ClearHarvestAI`, and the engine's AI object turns each call into unit orders at once (CONFIRMED: the native
//! bodies call the order dispatcher before they return). The structure below is read from the bodies of
//! `0x6F2DD450` (`SetProduce`), `0x6F2DD4B0` (`SetUpgrade`), `0x6F2DD5E0` / `0x6F2DD630` (`HarvestGold` / `HarvestWood`),
//! `0x6F2DD5B0` (`ClearHarvestAI`) and their helpers; the spec is `docs/spec/ai-natives.md` section 7.
//!
//! * **SetProduce(count, type, town)** (CONFIRMED): zero returns true; otherwise `count` requests, the first failure
//!   returns false. A request for a structure type goes to the build path (`0x6F2D2B70`), anything else to the train
//!   path (`0x6F2D5B90`).
//! * **Train path** (CONFIRMED structure): a hero type that the player already owns (alive or dead) returns true at
//!   once (`0x6F2D2D80`, the revive path). Otherwise the AI player's units are walked in list order; the first *usable*
//!   building (completed, idle: `0x6F2D5940`) that can train the type, and that belongs to the requested town when a
//!   town is given, gets the train order and the walk ends with true. The order can be refused later (money, food,
//!   requirements): the native does not look.
//! * **Build path** (CONFIRMED structure): the tech gates (requirements), then the AI player's units are walked in list
//!   order for a unit that is idle or harvesting (AI modes 0, 5, 6) and that can build the type; the site is searched
//!   from the town's position, and the builder gets the build order. No builder, or no site, returns false. A town hall
//!   tier (a type that one of the player's buildings upgrades into) goes to that building as an upgrade order.
//! * **SetUpgrade(tech)** (CONFIRMED): the native always returns false (the value of the body is the constant 0); the
//!   effect is the order to the first usable building whose research list has the tech and that can start it (cost,
//!   level).
//! * **ClearHarvestAI** (CONFIRMED, `0x6F2DD5B0`): for every unit of the AI player, the "assigned this cycle" bits
//!   (1 gold, 2 wood) become the "previous cycle" bits (4 gold, 8 wood), and the other previous bits are cleared.
//! * **HarvestGold/HarvestWood(town, n)** (CONFIRMED structure, `0x6F2D9710`, `0x6F2D8940`, `0x6F2D8C20`): `n <= 0` does
//!   nothing; the town slot must be filled; a gold request also needs a completed hall (or a completed haunted or
//!   entangled mine). Wood on town 0 first sends *every* idle harvester to wood (the loop does not stop when the count
//!   reaches zero, CONFIRMED quirk). Then the workers of the previous cycle that are still harvesting keep their
//!   assignment without a new order (`n` drops by one each), and finally other workers that are idle or harvesting and
//!   not yet assigned this cycle take a new order, in list order, until `n` is used.
//!
//! Stand-ins (UNSURE, listed in the spec section 7.5): the build spot, the choice of the builder among equals, the town a
//! worker belongs to (the model does not keep the per-unit town reference, any worker may serve any town), the travel and
//! work times, and the hero revive.

use crate::economy::Res;
use crate::natives::game::{Game, ObjKey};
use crate::production::{ProdError, CLASS_TOWNHALL};

/// The AI-visible mode of a worker (unit AI record `+0x194`): 0 idle, 2 on its way to build, 5 gold, 6 lumber.
pub const MODE_IDLE: u32 = 0;
pub const MODE_BUILD: u32 = 2;
pub const MODE_GOLD: u32 = 5;
pub const MODE_WOOD: u32 = 6;

/// A town of the AI as the managers need it: its hall and mine when they resolve, and the anchor point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TownInfo {
    pub slot: usize,
    pub hall: Option<ObjKey>,
    pub mine: Option<ObjKey>,
    pub x: f32,
    pub y: f32,
}

/// The mode of a unit in the AI's view.
pub fn mode_of(gm: &Game, k: ObjKey) -> u32 {
    let p = gm.production();
    if p.is_building(k) {
        return MODE_BUILD;
    }
    match p.harvester(k).map(|h| h.resource()) {
        Some(Res::Gold) => MODE_GOLD,
        Some(Res::Lumber) => MODE_WOOD,
        None => MODE_IDLE,
    }
}

/// `ClearHarvestAI` (`0x6F2DD5B0`).
pub fn clear_harvest(gm: &mut Game, player: u32) {
    for k in gm.owned_units(player) {
        let mut f = gm.production().ai_unit(k).flags;
        if f & 1 != 0 {
            f = (f & 0xFFFF_FFFA) | 4;
        } else {
            f &= !4;
        }
        if f & 2 != 0 {
            f = (f & 0xFFFF_FFF5) | 8;
        } else {
            f &= !8;
        }
        gm.production_mut().set_ai_flags(k, f);
    }
}

fn type_is(gm: &Game, k: ObjKey, code: u32) -> bool {
    gm.unit(k).type_id == code
}

/// `HarvestGold` / `HarvestWood` after the native's own gate (`n > 0`): `0x6F2D9710`.
pub fn harvest(gm: &mut Game, player: u32, town: Option<TownInfo>, n: i32, gold: bool) {
    let Some(town) = town else { return };
    if gold {
        // A completed hall; failing that a completed mine of the kinds that stand without a hall.
        let hall_ok = town.hall.is_some_and(|h| gm.unit_completed(h));
        let mine_ok = town.mine.is_some_and(|m| {
            gm.unit_completed(m)
                && (type_is(gm, m, crate::ai_towns::HAUNTED_MINE) || type_is(gm, m, crate::ai_towns::ENTANGLED_MINE))
        });
        if !hall_ok && !mine_ok {
            return;
        }
    }
    let res = if gold { Res::Gold } else { Res::Lumber };
    let anchor = (town.x, town.y);
    let mine = if gold { town.mine } else { None };
    let mut left = n;
    // Wood on the first town: every idle harvester goes to wood, the count only informs (CONFIRMED quirk).
    if !gold && town.slot == 0 {
        for k in gm.owned_units(player) {
            if mode_of(gm, k) == MODE_IDLE
                && gm.can_harvest(k, res)
                && gm.production().ai_unit(k).flags & 3 == 0
            {
                let f = gm.production().ai_unit(k).flags | 2;
                gm.production_mut().set_ai_flags(k, f);
                gm.assign_harvest(k, res, anchor, mine);
                if left > 0 {
                    left -= 1;
                }
            }
        }
    }
    if left <= 0 {
        return;
    }
    // The previous cycle's workers that still harvest keep their assignment (no order).
    for k in gm.owned_units(player) {
        let rec = gm.production().ai_unit(k).flags;
        if rec & 3 != 0 {
            continue;
        }
        let mode = mode_of(gm, k);
        if mode != MODE_GOLD && mode != MODE_WOOD {
            continue;
        }
        let ok = if gold {
            gm.can_harvest(k, res) && rec & 4 != 0
        } else {
            gm.can_harvest(k, res) && rec & 8 != 0
        };
        if !ok {
            continue;
        }
        gm.production_mut().set_ai_flags(k, rec | if gold { 1 } else { 2 });
        // The worker keeps its cycle; only a worker that changed resource would be re-ordered, and it was previous.
        gm.assign_harvest(k, res, anchor, mine);
        left -= 1;
        if left <= 0 {
            return;
        }
    }
    // New assignments, list order.
    for k in gm.owned_units(player) {
        let rec = gm.production().ai_unit(k).flags;
        if rec & 3 != 0 || !gm.can_harvest(k, res) {
            continue;
        }
        let mode = mode_of(gm, k);
        if mode != MODE_IDLE && mode != MODE_GOLD && mode != MODE_WOOD {
            continue;
        }
        gm.production_mut().set_ai_flags(k, rec | if gold { 1 } else { 2 });
        gm.assign_harvest(k, res, anchor, mine);
        left -= 1;
        if left <= 0 {
            return;
        }
    }
}

fn default_anchor(gm: &Game, player: u32) -> Option<(f32, f32)> {
    gm.owned_units(player)
        .into_iter()
        .find(|&k| gm.unit(k).class_flags & CLASS_TOWNHALL != 0 && gm.unit_completed(k) && gm.resource_amount(k).is_none())
        .map(|k| {
            let u = gm.unit(k);
            (u.x.to_f32(), u.y.to_f32())
        })
}

/// One production request of `SetProduce` (`0x6F2D8130`): true when the request was accepted by a unit.
fn request(gm: &mut Game, player: u32, type_id: u32, town: Option<TownInfo>) -> bool {
    if gm.unit_types().is_building(type_id) {
        build_request(gm, player, type_id, town)
    } else {
        train_request(gm, player, type_id, town)
    }
}

/// The train path (`0x6F2D5B90`).
fn train_request(gm: &mut Game, player: u32, type_id: u32, town: Option<TownInfo>) -> bool {
    // A hero the player already has (alive or dead) is the revive path (`0x6F2D2D80`): an alive hero needs nothing; a dead
    // one is revived at an altar that can train the type (UNSURE model of the cost and time, see `Game::revive_hero_at`).
    if crate::natives::hero::is_hero_id(type_id) {
        if gm.owned_units(player).into_iter().any(|k| gm.unit(k).type_id == type_id) {
            return true;
        }
        if let Some(h) = gm.dead_hero_of(player, type_id) {
            for k in gm.owned_units(player) {
                if gm.building_usable(k) && gm.can_train(k, type_id) && gm.revive_hero_at(k, h).is_ok() {
                    return true;
                }
            }
            return true;
        }
        if gm.unit_objects().any(|(_, u)| u.owner == player && u.type_id == type_id && u.dead && !u.removed) {
            return true; // already being revived
        }
    }
    for k in gm.owned_units(player) {
        if !gm.building_usable(k) || !gm.can_train(k, type_id) {
            continue;
        }
        if let Some(t) = town {
            let u = gm.unit(k);
            let d2 = (u.x.to_f32() - t.x).powi(2) + (u.y.to_f32() - t.y).powi(2);
            if d2 > crate::ai_towns::SITE_RADIUS * crate::ai_towns::SITE_RADIUS {
                continue;
            }
        }
        // The order is issued whether or not it later succeeds (the native does not look).
        let _ = gm.train_unit(k, type_id);
        return true;
    }
    false
}

/// The build path (`0x6F2D2B70`, `0x6F2D2880`).
fn build_request(gm: &mut Game, player: u32, type_id: u32, town: Option<TownInfo>) -> bool {
    if !gm.requirements_met(player, type_id) {
        return false;
    }
    // A tier of an existing building: the building gets the upgrade order.
    let upgrader = gm
        .owned_units(player)
        .into_iter()
        .find(|&k| gm.can_upgrade_unit(k, type_id));
    if let Some(k) = upgrader {
        return gm.start_unit_upgrade(k, type_id).is_ok();
    }
    let builder = gm.owned_units(player).into_iter().find(|&k| {
        let mode = mode_of(gm, k);
        (mode == MODE_IDLE || mode == MODE_GOLD || mode == MODE_WOOD) && gm.can_build(k, type_id)
    });
    let Some(builder) = builder else { return false };
    let anchor = town
        .map(|t| (t.x, t.y))
        .or_else(|| default_anchor(gm, player))
        .unwrap_or_else(|| {
            let u = gm.unit(builder);
            (u.x.to_f32(), u.y.to_f32())
        });
    let Some((x, y)) = gm.find_build_site(player, type_id, anchor) else { return false };
    match gm.start_build_job(builder, type_id, x, y) {
        Ok(()) => true,
        Err(ProdError::Resources) | Err(_) => false,
    }
}

/// `SetProduce(count, type, town)` (`0x6F2DD450`).
pub fn set_produce(gm: &mut Game, player: u32, count: i32, type_id: u32, town: Option<TownInfo>) -> bool {
    if count == 0 {
        return true;
    }
    // A negative count loops until a request fails (CONFIRMED loop form); cap it at a large bound.
    let n = if count < 0 { 1000 } else { count };
    for _ in 0..n {
        if !request(gm, player, type_id, town) {
            return false;
        }
    }
    true
}

/// `SetUpgrade(tech)` (`0x6F2DD4B0` → `0x6F2D5A00`): the order goes to the first usable building that can take it.
pub fn set_upgrade(gm: &mut Game, player: u32, tech: u32) {
    for k in gm.owned_units(player) {
        if !gm.building_usable(k) || gm.can_research(k, tech).is_err() {
            continue;
        }
        if gm.start_research(k, tech).is_ok() {
            return;
        }
    }
}

/// `GetUnitCount` / `GetUnitCountDone` for the AI player: the completed units of the type, plus (for the first) the
/// buildings under construction, the units queued for training, the in-place upgrades into the type and the workers on
/// their way to build it (`0x6F2DD050` = typed count with mask `0x36` plus `0x6F2D1C10`; `0x6F2DD010` = mask `0x30`).
pub fn unit_count(gm: &Game, player: u32, type_id: u32, done_only: bool) -> i32 {
    let mut n = 0i32;
    for (k, u) in gm.unit_objects() {
        if u.owner != player || u.dead || u.removed {
            continue;
        }
        let building = u.count_state & crate::unit_counts::REGISTERED_IN_PROGRESS != 0;
        if u.type_id == type_id && (!building || !done_only) {
            n += 1;
        }
        if !done_only {
            n += gm.production().queued_total(k, type_id) as i32;
            if gm.production().pending_build_type(k) == Some(type_id) {
                n += 1;
            }
        }
    }
    if !done_only {
        n += gm.production().upgrades_into(type_id) as i32;
    }
    n
}
