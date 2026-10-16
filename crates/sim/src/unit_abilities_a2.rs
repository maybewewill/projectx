//! Unit abilities, batch A part 2 (S51). Spec: `docs/spec/abilities-unit-a2.md`.
//!
//! The codes of `docs/spec/abilities-unit-a.md` that S46 left without a model. Only codes with behaviour are in
//! [`ABILITIES`] (class and parent from the ability atlas `docs/research/ability-atlas/factory.tsv`, level fields from
//! the 1.26a `AbilityData.slk` labelled by `AbilityMetaData.slk` / `WorldEditStrings.txt`, order ids from the original
//! getters `docs/research/ability-orders/getters.tsv` or the order-name table `orders/names.rs`). Labels as in
//! `unit_abilities_a.rs`: CONFIRMED = machine code, LIKELY = class parent / editor field meaning, UNSURE = a stand-in.
//!
//! Hooks (called from `unit_abilities_a.rs`, which `cast.rs` calls): [`register_orders`], [`order_alias`],
//! [`takes_no_target`], [`ignores_range`], [`target_error`], [`run_effect`], [`on_timer`] and
//! [`Game::sync_unit_abilities_a2`].

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

use crate::abilities::code;
use crate::cast::{CastError, CastRec, CastTarget};
use crate::natives::game::{Game, ObjKey};
use crate::natives::ScriptRuntime;
use crate::production::{
    ACOLYTE_HARVEST_ABILITY, ENTANGLE_ABILITY, GOLD_MINE_ABILITY, HARVEST_ABILITY, LUMBER_HARVEST_ABILITY,
    ORDER_ACOLYTE_HARVEST, ORDER_HARVEST,
};
use crate::unit_abilities_a::{Conf, Info, Sim};

/// The registry of this part.
pub const ABILITIES: &[Info] = &[
    Info { code: code(b"Agld"), class: "CAbilityGoldMine", parent: "CAbilityGoldMineBase", sim: Sim::Economy, conf: Conf::Likely, note: "a unit with Agld is a gold mine: DataA gold at creation (the ability class reads its level-1 DataA, economy.md)" },
    Info { code: code(b"Ahar"), class: "CAbilityHarvest", parent: "CAbilityHarvestLumber", sim: Sim::Economy, conf: Conf::Likely, note: "gold and lumber harvest from the ability record (DataA per hit, DataB lumber cap, DataC gold cap); order harvest 852018 on a mine or a tree point; returnresources 852020" },
    Info { code: code(b"Ahrl"), class: "CAbilityHarvestLumber", parent: "CAbilityHarvestReturn", sim: Sim::Economy, conf: Conf::Likely, note: "lumber harvest (DataA per hit, DataB cap); same orders as Ahar" },
    Info { code: code(b"Aaha"), class: "CAbilityAcolyteHarvest", parent: "CPower", sim: Sim::Economy, conf: Conf::Likely, note: "the acolyte works a haunted mine (attached, the mine pays); order acolyteharvest 852185" },
    Info { code: code(b"Abgm"), class: "CAbilityBlightedGoldMine", parent: "CAbilityPossessedGoldMine", sim: Sim::Economy, conf: Conf::Likely, note: "haunted mine: DataA gold per DataB seconds scaled by workers/DataC (max 8)" },
    Info { code: code(b"Aegm"), class: "CAbilityEntangledGoldMine", parent: "CAbilityPossessedGoldMine", sim: Sim::Economy, conf: Conf::Likely, note: "entangled mine: DataA gold per DataB seconds, default 5 miners" },
    Info { code: code(b"Aent"), class: "CAbilityEntangle", parent: "CPower", sim: Sim::Economy, conf: Conf::Likely, note: "the Tree of Life turns a neutral gold mine into UnitID after Cast seconds; order entangle 852147, autocast scan 1 s within Rng" },
];

/// State of this part (owned by the batch-A state).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Units that carry `Aent` (so the production tick scans for mines even without a type row).
    entanglers: BTreeSet<ObjKey>,
    /// Units that carry `Agld` and were made a resource node by this module.
    mine_nodes: BTreeSet<ObjKey>,
}

impl State {
    /// Whether some unit carries the entangle ability.
    pub fn has_entanglers(&self) -> bool {
        !self.entanglers.is_empty()
    }
}

/// The entry of `base` in this part's registry.
pub fn info(base: u32) -> Option<&'static Info> {
    ABILITIES.iter().find(|i| i.code == base)
}

/// The order ids of this part: `(order id, ability)`.
pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (id, base) in [
        (ORDER_HARVEST, HARVEST_ABILITY),
        (852_020, HARVEST_ABILITY),
        (ORDER_ACOLYTE_HARVEST, ACOLYTE_HARVEST_ABILITY),
        (852_147, ENTANGLE_ABILITY),
    ] {
        ids.insert(id, base);
    }
}

/// A shared order id resolves to the ability the unit actually has: `harvest` / `returnresources` belong to `Ahar` for the
/// peasant and `Ahrl` for the ghoul.
pub(crate) fn order_alias(gm: &Game, unit: ObjKey, order_id: u32, base: u32) -> u32 {
    if (order_id == ORDER_HARVEST || order_id == 852_020) && base == HARVEST_ABILITY {
        let has = |c: u32| gm.unit(unit).abilities.get(c).is_some_and(|a| !a.is_removed());
        if !has(HARVEST_ABILITY) && has(LUMBER_HARVEST_ABILITY) {
            return LUMBER_HARVEST_ABILITY;
        }
    }
    base
}

fn is_harvest(base: u32) -> bool {
    matches!(base, HARVEST_ABILITY | LUMBER_HARVEST_ABILITY | ACOLYTE_HARVEST_ABILITY)
}

/// Abilities of this part cast without a target (`returnresources`).
pub(crate) fn takes_no_target(base: u32) -> bool {
    is_harvest(base)
}

/// Abilities whose order the unit walks to (the worker approaches the mine or tree itself): no range gate at the cast.
pub(crate) fn ignores_range(base: u32) -> bool {
    is_harvest(base) || base == ENTANGLE_ABILITY
}

/// Extra target rules of this part.
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    match base {
        HARVEST_ABILITY | LUMBER_HARVEST_ABILITY | ACOLYTE_HARVEST_ABILITY => {
            // A mine to work, or a hall to return the load to.
            let mine = gm.resource_amount(target).is_some_and(|a| a > 0) && !gm.unit(target).removed;
            let hall = gm.unit(target).owner == gm.unit(caster).owner
                && gm.unit(target).class_flags & crate::production::CLASS_TOWNHALL != 0;
            (!mine && !hall).then_some(CastError::BadTarget)
        }
        ENTANGLE_ABILITY => {
            let neutral = gm.resource_amount(target).is_some_and(|a| a > 0)
                && gm.unit(target).type_id == crate::ai_towns::GOLD_MINE
                && !gm.unit(target).removed;
            (!neutral).then_some(CastError::BadTarget)
        }
        _ => None,
    }
}

/// The class effects of this part. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, _rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        HARVEST_ABILITY | LUMBER_HARVEST_ABILITY | ACOLYTE_HARVEST_ABILITY => {
            let mut gm = game.borrow_mut();
            match rec.target {
                CastTarget::Unit(t) if gm.resource_amount(t).is_some() => {
                    gm.order_harvest(rec.caster, Some(t), None);
                }
                CastTarget::Point(x, y) => {
                    // A tree under the clicked point (the destructable target of the original is a point here).
                    if let Some(i) = gm.tree_near(x.to_f32(), y.to_f32(), TREE_PICK_RADIUS) {
                        gm.order_harvest(rec.caster, None, Some(i));
                    }
                }
                _ => {
                    gm.order_return_resources(rec.caster);
                }
            }
            true
        }
        ENTANGLE_ABILITY => {
            if let CastTarget::Unit(t) = rec.target {
                game.borrow_mut().order_entangle(rec.caster, t);
            }
            true
        }
        _ => false,
    }
}

/// Timer dispatch for the kinds of this part (none yet).
pub(crate) fn on_timer(_game: &RefCell<Game>, _rt: &mut dyn ScriptRuntime, _kind: u8, _unit: ObjKey, _base: u32, _aux: u32) {}

/// How far from the clicked point a tree still counts as the target.
const TREE_PICK_RADIUS: f32 = 128.0;

fn has(gm: &Game, unit: ObjKey, base: u32) -> bool {
    gm.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed())
}

impl Game {
    /// Keeps the state of this part in step with the abilities of `unit` (called from `sync_unit_abilities_a`).
    pub(crate) fn sync_unit_abilities_a2(&mut self, unit: ObjKey) {
        // Entangle: the production tick scans for mines while some unit has the ability.
        if has(self, unit, ENTANGLE_ABILITY) {
            self.spells.unit_a.b.entanglers.insert(unit);
        } else {
            self.spells.unit_a.b.entanglers.remove(&unit);
        }
        // Gold mine: a unit with `Agld` owns a resource node of `DataA` gold (the ability class reads its level-1
        // `DataA` when it is created); a mine the type table already made a node is left alone.
        if has(self, unit, GOLD_MINE_ABILITY) {
            if self.resource_amount(unit).is_none() {
                let amount = self
                    .ability_level_record(unit, GOLD_MINE_ABILITY)
                    .map(|l| conv::f2i(l.data[0]))
                    .or_else(|| self.ability_resource.get(&GOLD_MINE_ABILITY).copied())
                    .unwrap_or(0);
                self.add_resource_node(unit, amount);
                self.spells.unit_a.b.mine_nodes.insert(unit);
            }
        } else if self.spells.unit_a.b.mine_nodes.remove(&unit) {
            self.mines.remove(&unit);
        }
    }
}

#[allow(dead_code)]
fn _unused(_: F32) {
    let _ = arith::add;
}
