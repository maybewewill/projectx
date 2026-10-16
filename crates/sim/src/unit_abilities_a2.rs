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

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::timer::TimerId;
use crate::cast::{CastError, CastRec, CastTarget};
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::ScriptRuntime;
use crate::production::{
    ACOLYTE_HARVEST_ABILITY, ENTANGLE_ABILITY, GOLD_MINE_ABILITY, HARVEST_ABILITY, LUMBER_HARVEST_ABILITY,
    ORDER_ACOLYTE_HARVEST, ORDER_HARVEST,
};
use crate::cast::{SpellLevel, SpellMsg};
use crate::unit_abilities_a::{Conf, Info, Sim};
use crate::unit_spells::{targs_ok, targs_word};

/// The registry of this part.
pub const ABILITIES: &[Info] = &[
    Info { code: code(b"Agld"), class: "CAbilityGoldMine", parent: "CAbilityGoldMineBase", sim: Sim::Economy, conf: Conf::Likely, note: "a unit with Agld is a gold mine: DataA gold at creation (the ability class reads its level-1 DataA, economy.md)" },
    Info { code: code(b"Ahar"), class: "CAbilityHarvest", parent: "CAbilityHarvestLumber", sim: Sim::Economy, conf: Conf::Likely, note: "gold and lumber harvest from the ability record (DataA per hit, DataB lumber cap, DataC gold cap); order harvest 852018 on a mine or a tree point; returnresources 852020" },
    Info { code: code(b"Ahrl"), class: "CAbilityHarvestLumber", parent: "CAbilityHarvestReturn", sim: Sim::Economy, conf: Conf::Likely, note: "lumber harvest (DataA per hit, DataB cap); same orders as Ahar" },
    Info { code: code(b"Aaha"), class: "CAbilityAcolyteHarvest", parent: "CPower", sim: Sim::Economy, conf: Conf::Likely, note: "the acolyte works a haunted mine (attached, the mine pays); order acolyteharvest 852185" },
    Info { code: code(b"Abgm"), class: "CAbilityBlightedGoldMine", parent: "CAbilityPossessedGoldMine", sim: Sim::Economy, conf: Conf::Likely, note: "haunted mine: DataA gold per DataB seconds scaled by workers/DataC (max 8)" },
    Info { code: code(b"Aegm"), class: "CAbilityEntangledGoldMine", parent: "CAbilityPossessedGoldMine", sim: Sim::Economy, conf: Conf::Likely, note: "entangled mine: DataA gold per DataB seconds, default 5 miners" },
    Info { code: code(b"Acor"), class: "CAbilityCorrosiveBreath", parent: "CAbilityPassive", sim: Sim::Strike, conf: Conf::Likely, note: "on hit against a structure: buff Bcor (CBuffCorrosiveBreath < CBuffLifeDoT), DataA damage per second for Dur" },
    Info { code: code(b"Afak"), class: "CAbilityFlamingAttack", parent: "CAbilityFlamingArrows", sim: Sim::Strike, conf: Conf::Likely, note: "orb: Cost mana per hit, DataA fire damage to the target, splash rings DataD full / DataE x DataB / Area x DataC (labels: full radius, half radius, medium and small factor)" },
    Info { code: code(b"Aflk"), class: "CAbilityFlakCannon", parent: "CAbilityPassive", sim: Sim::Strike, conf: Conf::Likely, note: "on hit against an air target: splash damage DataC / DataD / DataE inside the radii Area / DataA / DataB" },
    Info { code: code(b"Afsh"), class: "CAbilityFragShards", parent: "CAbilityPassive", sim: Sim::Strike, conf: Conf::Likely, note: "on hit against a ground target: splash damage DataC / DataD / DataE inside the radii Area / DataA / DataB (sorted)" },
    Info { code: code(b"Afrb"), class: "CAbilityFrostBreath", parent: "CAbility", sim: Sim::Strike, conf: Conf::Likely, note: "on hit: the Frost Attack slow (Bfro) for Dur / HeroDur" },
    Info { code: code(b"Afrz"), class: "CAbilityFreezingBreath", parent: "CAbilityPassive", sim: Sim::Strike, conf: Conf::Likely, note: "on hit against a structure: Bfrz (CBuffFreezingBreath < CBuffStun) stuns the structure and the structures within Area for Dur" },
    Info { code: code(b"Aliq"), class: "CAbilityLiquidFire", parent: "CAbilityPassive", sim: Sim::Strike, conf: Conf::Likely, note: "on hit against a structure: Bliq (CBuffLiquidFire < CBuffPoisonAttack): DataA damage per second, attack speed -DataC, for Dur" },
    Info { code: code(b"Adda"), class: "CAbilityDeathDamageAoe", parent: "CAbility", sim: Sim::Strike, conf: Conf::Unsure, note: "when the unit dies: after Dur, DataB damage within DataA and DataD damage within DataC (labels: full / partial radius and amount)" },
    Info { code: code(b"Aeat"), class: "CAbilityEatTree", parent: "CAbilitySimpleSpell", sim: Sim::Terrain, conf: Conf::Likely, note: "order eattree 852146 at a tree: after DataA seconds the tree is felled, DataB seconds later buff Beat (CBuffEatTree < CBuffRejuvination) heals DataC over Dur" },
    Info { code: code(b"Agra"), class: "CAbilityGrabTree", parent: "CAbilityEatTree", sim: Sim::Terrain, conf: Conf::Unsure, note: "order grabtree 852511 at a tree: after DataA seconds the tree is felled and Bgra stays for DataE attacks, then DataB seconds more; the weapon switch (DataC/DataD) is not modelled" },
    Info { code: code(b"Acoa"), class: "CAbilityCoupleArcher", parent: "CAbilityCouple", sim: Sim::Convert, conf: Conf::Likely, note: "couple: order coupletarget 852507 on a unit of the partner type DataA (rawcode bits) merges both into UnitID" },
    Info { code: code(b"Acoh"), class: "CAbilityCoupleHippogryph", parent: "CAbilityCouple", sim: Sim::Convert, conf: Conf::Likely, note: "the same from the other side (partner DataA = the archer)" },
    Info { code: code(b"Acoi"), class: "CAbilityCoupleInstant", parent: "CAbilityClosestTargetSpell", sim: Sim::Convert, conf: Conf::Unsure, note: "order coupleinstant 852508: couples with the closest own unit of the partner type (the search radius is not decoded: unlimited)" },
    Info { code: code(b"Adec"), class: "CAbilityDecouple", parent: "CAbilitySimpleSpell", sim: Sim::Convert, conf: Conf::Likely, note: "order decouple 852509: the rider splits into the units DataA and DataB (life fraction kept)" },
    Info { code: code(b"Abtl"), class: "CAbilityBattlestations", parent: "CAbilitySpell", sim: Sim::Convert, conf: Conf::Unsure, note: "order battlestations 852099: own UnitID units within Area (busy ones when DataB != 0) go into the burrow's cargo" },
    Info { code: code(b"Alam"), class: "CAbilitySacrificialLamb", parent: "CAbilitySimpleSpell", sim: Sim::Convert, conf: Conf::Unsure, note: "order requestsacrifice 852201 on an own structure: the acolyte is consumed and a Shade appears (the pit's Asac train is not modelled)" },
    Info { code: code(b"Acan"), class: "CAbilityCannibalize", parent: "CAbilityClosestTargetSpell", sim: Sim::Terrain, conf: Conf::Unsure, note: "orders cannibalize 852188 (closest corpse within 500) and 852535 (target): the unit stands still and heals DataA per second for up to Dur, while below min(max life, DataB); the corpse is eaten when it ends" },
    Info { code: code(b"Aast"), class: "CAbilityAncestralSpirit", parent: "CAbilityClosestTargetSpell", sim: Sim::Terrain, conf: Conf::Unsure, note: "orders ancestralspirit 852490 (closest own corpse within Rng) and 852491 (target): the non-hero corpse comes back with DataA of its max life and DataB of its max mana" },
    Info { code: code(b"Advm"), class: "CAbilityDevourMagic", parent: "CAbilitySimpleSpell", sim: Sim::Terrain, conf: Conf::Unsure, note: "order devourmagic 852536 at a point: the Dispel Magic worker on every unit within Area (friends only when DataF is 0); each affected unit gives the caster DataA life and DataB mana, each buff removed DataC and DataD; summoned units take DataE" },
    Info { code: code(b"Agyd"), class: "CAbilityGraveyard", parent: "CAbility", sim: Sim::Terrain, conf: Conf::Unsure, note: "every Cool seconds a dead UnitID corpse is made on a ring of radius DataB when fewer than DataA lie within DataC" },
    Info { code: code(b"Afla"), class: "CAbilityFlare", parent: "CAbilitySimpleSpell", sim: Sim::Terrain, conf: Conf::Likely, note: "order flare 852060 at a point (Rng 99999): after DataB seconds a visible fog modifier of radius Area opens for the caster's player for Dur; DataA detection type is not modelled" },
    Info { code: code(b"Ablp"), class: "CAbilityBlightPlacement", parent: "CAbilitySimpleSpell", sim: Sim::Terrain, conf: Conf::Unsure, note: "once the unit is complete, blight grows around it by DataA every Dur up to Area (DataB != 0 creates, 0 removes)" },
    Info { code: code(b"Abli"), class: "CAbilityBlightGrowth", parent: "CAbility", sim: Sim::Terrain, conf: Conf::Unsure, note: "the same growth for the Necropolis and Graveyard (the stock row is the alias Abgs: Area 768)" },
    Info { code: code(b"Aamk"), class: "CAbilityAttributeModSkill", parent: "CAbilityPassive", sim: Sim::Passive, conf: Conf::Likely, note: "hero passive: DataA agility, DataB intelligence, DataC strength (the attribute bonus of `item_abilities`: F2I of the fields into the base counters)" },
    Info { code: code(b"Aent"), class: "CAbilityEntangle", parent: "CPower", sim: Sim::Economy, conf: Conf::Likely, note: "the Tree of Life turns a neutral gold mine into UnitID after Cast seconds; order entangle 852147, autocast scan 1 s within Rng" },
];

/// State of this part (owned by the batch-A state).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Units that carry `Aent` (so the production tick scans for mines even without a type row).
    entanglers: BTreeSet<ObjKey>,
    /// Units that carry `Agld` and were made a resource node by this module.
    mine_nodes: BTreeSet<ObjKey>,
    /// Units holding a grabbed tree: attacks left before it is dropped.
    grabbed: BTreeMap<ObjKey, i32>,
    /// Cannibalizing units: corpse, seconds left, the place they stand at.
    cannibals: BTreeMap<ObjKey, (ObjKey, f32, (F32, F32))>,
    /// Running graveyard timers.
    graveyards: BTreeMap<ObjKey, TimerId>,
    /// Flares waiting for their effect delay: id -> (x, y, area, duration).
    flares: BTreeMap<u32, (F32, F32, F32, F32)>,
    next_flare: u32,
    /// Blight growths in progress: (unit, ability) -> radius reached.
    blight: BTreeMap<(ObjKey, u32), F32>,
    /// Blight growths that reached their radius.
    blight_done: BTreeSet<(ObjKey, u32)>,
    /// The attribute totals `Aamk` has added to a hero: (agility, intelligence, strength).
    attrs: BTreeMap<ObjKey, (i32, i32, i32)>,
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
        (ORDER_EAT_TREE, EAT_TREE),
        (ORDER_GRAB_TREE, GRAB_TREE),
        (ORDER_COUPLE_TARGET, COUPLE_ARCHER),
        (ORDER_COUPLE_INSTANT, COUPLE_ARCHER),
        (ORDER_DECOUPLE, DECOUPLE),
        (ORDER_BATTLESTATIONS, BATTLESTATIONS),
        (ORDER_SACRIFICE, SACRIFICIAL_LAMB),
        (852_188, CANNIBALIZE),
        (852_535, CANNIBALIZE),
        (852_490, ANCESTRAL_SPIRIT),
        (852_491, ANCESTRAL_SPIRIT),
        (ORDER_DEVOUR_MAGIC, DEVOUR_MAGIC),
        (ORDER_FLARE, FLARE),
    ] {
        ids.insert(id, base);
    }
}

/// A shared order id resolves to the ability the unit actually has: `harvest` / `returnresources` belong to `Ahar` for the
/// peasant and `Ahrl` for the ghoul.
pub(crate) fn order_alias(gm: &Game, unit: ObjKey, order_id: u32, base: u32) -> u32 {
    if order_id == ORDER_COUPLE_TARGET || order_id == ORDER_COUPLE_INSTANT {
        // The order is shared by the three couple classes: the unit's own class answers it.
        let has = |c: u32| gm.unit(unit).abilities.get(c).is_some_and(|a| !a.is_removed());
        return [COUPLE_ARCHER, COUPLE_HIPPO, COUPLE_INSTANT].into_iter().find(|&c| has(c)).unwrap_or(base);
    }
    if (order_id == ORDER_HARVEST || order_id == 852_020) && base == HARVEST_ABILITY {
        let has = |c: u32| gm.unit(unit).abilities.get(c).is_some_and(|a| !a.is_removed());
        if !has(HARVEST_ABILITY) && has(LUMBER_HARVEST_ABILITY) {
            return LUMBER_HARVEST_ABILITY;
        }
        // `returnresources` is also the order of Return Resources (`Artn`, S52) on units without a harvest ability.
        if order_id == 852_020 && !has(HARVEST_ABILITY) && has(crate::unit_abilities_m::RETURN) {
            return crate::unit_abilities_m::RETURN;
        }
    }
    base
}

fn is_harvest(base: u32) -> bool {
    matches!(base, HARVEST_ABILITY | LUMBER_HARVEST_ABILITY | ACOLYTE_HARVEST_ABILITY)
}

/// Abilities of this part cast without a target (`returnresources`).
pub(crate) fn takes_no_target(base: u32) -> bool {
    is_harvest(base) || is_couple(base) || matches!(base, DECOUPLE | BATTLESTATIONS | CANNIBALIZE | ANCESTRAL_SPIRIT)
}

/// Abilities whose unit target is a corpse (the cast core would refuse a dead target otherwise).
pub(crate) fn wants_corpse(base: u32) -> bool {
    matches!(base, CANNIBALIZE | ANCESTRAL_SPIRIT)
}

/// Abilities whose order the unit walks to (the worker approaches the mine or tree itself): no range gate at the cast.
pub(crate) fn ignores_range(base: u32) -> bool {
    is_harvest(base) || is_couple(base) || base == ENTANGLE_ABILITY || matches!(base, EAT_TREE | GRAB_TREE | SACRIFICIAL_LAMB | FLARE)
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
        COUPLE_ARCHER | COUPLE_HIPPO | COUPLE_INSTANT => (!gm.couple_partner_ok(caster, target)).then_some(CastError::BadTarget),
        CANNIBALIZE => (!gm.corpse_ok(target, false)).then_some(CastError::BadTarget),
        ANCESTRAL_SPIRIT => (!(gm.corpse_ok(target, true) && gm.unit(target).owner == gm.unit(caster).owner)).then_some(CastError::BadTarget),
        SACRIFICIAL_LAMB => {
            let pit = targs_ok(gm, caster, target, targs_word("structure,player")) && gm.alive(target);
            (!pit).then_some(CastError::BadTarget)
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
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
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
        EAT_TREE | GRAB_TREE => {
            if let CastTarget::Point(x, y) = rec.target {
                game.borrow_mut().start_tree_cast(rec, x, y);
            }
            true
        }
        COUPLE_ARCHER | COUPLE_HIPPO | COUPLE_INSTANT => {
            let mut gm = game.borrow_mut();
            let partner = match rec.target {
                CastTarget::Unit(t) => Some(t),
                _ => gm.closest_partner(rec.caster),
            };
            if let Some(t) = partner {
                gm.couple(rec.caster, t);
            }
            true
        }
        DECOUPLE => {
            game.borrow_mut().decouple(rec.caster, &rec.level);
            true
        }
        BATTLESTATIONS => {
            game.borrow_mut().battlestations(rec.caster, &rec.level);
            true
        }
        SACRIFICIAL_LAMB => {
            if let CastTarget::Unit(pit) = rec.target {
                game.borrow_mut().sacrifice(rec.caster, pit);
            }
            true
        }
        CANNIBALIZE => {
            let mut gm = game.borrow_mut();
            let corpse = match rec.target {
                CastTarget::Unit(t) => Some(t),
                _ => gm.closest_corpse(rec.caster, F32::from_bits(CANNIBALIZE_SEARCH.to_bits()), false),
            };
            if let Some(c) = corpse {
                gm.start_cannibalize(rec.caster, c, &rec.level);
            }
            true
        }
        ANCESTRAL_SPIRIT => {
            let mut gm = game.borrow_mut();
            let corpse = match rec.target {
                CastTarget::Unit(t) => Some(t),
                _ => gm.closest_corpse(rec.caster, rec.level.rng, true),
            };
            if let Some(c) = corpse {
                gm.ancestral_spirit(rec.caster, c, &rec.level);
            }
            true
        }
        FLARE => {
            let mut gm = game.borrow_mut();
            let at = match rec.target {
                CastTarget::Point(x, y) => (x, y),
                CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
                CastTarget::None => (gm.unit(rec.caster).x, gm.unit(rec.caster).y),
            };
            gm.start_flare(rec, at);
            true
        }
        DEVOUR_MAGIC => {
            let at = {
                let gm = game.borrow();
                match rec.target {
                    CastTarget::Point(x, y) => (x, y),
                    CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
                    CastTarget::None => (gm.unit(rec.caster).x, gm.unit(rec.caster).y),
                }
            };
            let died = game.borrow_mut().devour_magic(rec.caster, &rec.level, at);
            for t in died {
                crate::natives::dispatch_unit_death_by(game, rt, t, Some(rec.caster));
            }
            true
        }
        _ => false,
    }
}

/// Timer dispatch for the kinds of this part.
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, base: u32, aux: u32) {
    if kind == KIND_TREE {
        game.borrow_mut().tree_stage(unit, base, aux);
    }
    if kind == KIND_CANNIBAL {
        game.borrow_mut().cannibalize_tick(unit);
    }
    if kind == KIND_GRAVEYARD {
        game.borrow_mut().graveyard_tick(unit);
    }
    if kind == KIND_FLARE {
        game.borrow_mut().flare_opens(unit, aux);
    }
    if kind == KIND_BLIGHT {
        game.borrow_mut().blight_step(unit, base);
    }
    if kind == KIND_DEATH_AOE {
        let died = game.borrow_mut().death_explosion(unit);
        for t in died {
            crate::natives::dispatch_unit_death_by(game, rt, t, Some(unit));
        }
    }
}

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
        // Graveyard: a periodic corpse maker while the unit has `Agyd`.
        let running = self.spells.unit_a.b.graveyards.contains_key(&unit);
        if has(self, unit, GRAVEYARD) && !running {
            let period = self.ability_level_record(unit, GRAVEYARD).map_or(15.0, |l| l.cool.to_f32()).max(0.1);
            let id = self.timers.start(
                period,
                true,
                Payload::Spell(SpellMsg::UnitSpell { kind: KIND_GRAVEYARD, unit, base: GRAVEYARD, aux: 0 }),
            );
            self.spells.unit_a.b.graveyards.insert(unit, id);
        } else if !has(self, unit, GRAVEYARD) && running {
            if let Some(id) = self.spells.unit_a.b.graveyards.remove(&unit) {
                self.timers.cancel(id);
            }
        }
        // Blight growth: starts once for a unit that has `Ablp` / `Abli`.
        for base in [BLIGHT_PLACEMENT, BLIGHT_GROWTH] {
            if has(self, unit, base) && !self.spells.unit_a.b.blight.contains_key(&(unit, base)) && !self.spells.unit_a.b.blight_done.contains(&(unit, base)) {
                self.spells.unit_a.b.blight.insert((unit, base), F32::ZERO);
                let step = self.ability_level_record(unit, base).map_or(0.08, |l| l.dur.to_f32()).max(0.01);
                self.timers.start(step, false, Payload::Spell(SpellMsg::UnitSpell { kind: KIND_BLIGHT, unit, base, aux: 0 }));
            }
        }
        self.sync_attribute_skill(unit);
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


// ---- on-hit and on-death effects (Acor, Afak, Aflk, Afsh, Afrb, Afrz, Aliq, Adda) ----

pub const CORROSIVE_BREATH: u32 = code(b"Acor");
pub const FLAMING_ATTACK: u32 = code(b"Afak");
pub const FLAK_CANNON: u32 = code(b"Aflk");
pub const FRAG_SHARDS: u32 = code(b"Afsh");
pub const FROST_BREATH: u32 = code(b"Afrb");
pub const FREEZING_BREATH: u32 = code(b"Afrz");
pub const LIQUID_FIRE: u32 = code(b"Aliq");
pub const DEATH_DAMAGE: u32 = code(b"Adda");
pub const BUFF_CORROSIVE: u32 = code(b"Bcor");
pub const BUFF_FREEZING: u32 = code(b"Bfrz");
pub const BUFF_LIQUID_FIRE: u32 = code(b"Bliq");
/// Timer kind of the delayed death explosion.
pub const KIND_DEATH_AOE: u8 = 0x50;

/// JASS damage types of the effects (`DAMAGE_TYPE_*`).
const DT_FIRE: u32 = 8;
const DT_ACID: u32 = 16;
const DT_ENHANCED: u32 = 5;
const DT_DEMOLITION: u32 = 22;

/// The three `{radius, amount}` rings Flak Cannon hands to the area body `0x6F2C31D0` (`CAbilityFlakCannon` slot `0x154`,
/// `0x6F177D00`): `(Area, DataC)`, `(DataA, DataD)`, `(DataB, DataE)`, in that order. Frag Shards uses the same triples
/// (`0x6F183B40`). CONFIRMED (V7, Unicorn `tools/emu/v7_hits_emu.py`, 300 cases). The body picks, for each unit, the amount of the
/// smallest ring that contains it (ring order only breaks ties).
pub fn tier_rings(area: F32, data: [F32; 5]) -> [(F32, F32); 3] {
    [(area, data[2]), (data[0], data[3]), (data[1], data[4])]
}

/// Whether the Flak Cannon hit handler calls the area body at all: the ability is enabled (`vf616`) and its pause counter
/// (`+0x3C`) is not positive (a signed compare). CONFIRMED (V7).
pub fn tier_hit_runs(enabled: bool, pause: u32) -> bool {
    enabled && (pause as i32) <= 0
}

/// The splash fields Flaming Attack writes into the launch record (`0x6F09DE00`): `Hfact` (`+0x80`) = `DataB`, `Qfact` (`+0x84`) =
/// `DataC`, `Farea` (`+0x88`) = `DataD`, `Harea` (`+0x8C`) = `DataE`, `Qarea` (`+0x90`) = `Area`. The handler runs only when the
/// ability flag `0x400000` is set and clears it. CONFIRMED (V7).
pub fn flaming_splash_fields(area: F32, data: [F32; 5]) -> [F32; 5] {
    [data[1], data[2], data[3], data[4], area]
}

/// The rings of the Death Damage explosion (`0x6F1E7200`): `(DataA, DataB)`, `(DataC, DataD)`; the delayed object gets the level's
/// `Dur`, count 2 and damage class `0x10`. CONFIRMED (V7).
pub fn death_damage_rings(data: [F32; 4]) -> [(F32, F32); 2] {
    [(data[0], data[1]), (data[2], data[3])]
}

/// Whether the Death Damage handler creates the explosion: the ability is enabled (`vf616`), its flag byte `+0x20` has bit 7
/// clear, the dying unit is no illusion (`+0x5C` bit `0x40000000`) and its flag byte `+0x20` has bit 0 clear. CONFIRMED (V7).
pub fn death_damage_runs(enabled: bool, ability_flags: u32, unit_flags: u32, unit_byte: u8) -> bool {
    enabled && ability_flags & 0x80 == 0 && unit_flags & 0x4000_0000 == 0 && unit_byte & 1 == 0
}

/// The damage-class masks the collector reads (`0x6FA6F2CC`, `0x6FA6F2D0`; set at start-up, runtime values).
pub const CLASS_MASK_A: u32 = 0x033E_E700;
pub const CLASS_MASK_B: u32 = 0x00C1_1830;

/// What the area body's per-candidate collector `0x6F2C2E80` sees about one candidate unit and the context it was built for.
#[derive(Debug, Clone, Copy)]
pub struct RingProbe {
    /// The context's damage class word (`ctx[20]`) and attack-type row (`ctx[22]`).
    pub class: u32,
    pub row: u32,
    /// Misc `MagicImmunesResistDamage`.
    pub misc_resist: bool,
    /// `ctx[24] != 0 && 0x6F26F2B0(ctx[23], unit)`: the max-amount mode instead of the ring lookup.
    pub max_mode: bool,
    /// The candidate's vtable slot `0x11C` (invulnerable), magic-immunity counter (`+0x15C`), ethereal counter (`+0x158`);
    /// `unit_known` = the owner object `0x6F282F60` resolved.
    pub invulnerable: bool,
    pub magic_immune: i32,
    pub ethereal: i32,
    pub unit_known: bool,
    /// The context's player filter (`ctx[25]`, -1 = none) and the relation answers for the unit's owner
    /// (`0x6F3A3400`, `0x6F3A36D0`).
    pub owner_filter: i32,
    pub rel_a: bool,
    pub rel_b: bool,
}

/// What the collector stores for a candidate: the amount and the bucket (0 = `rel_a` false, 1 = `rel_b`, 2 = the rest).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingEntry {
    pub amount: F32,
    pub bucket: u32,
}

/// The ring collector `0x6F2C2E80`. In ring mode it keeps the amount of the ring with the SMALLEST radius that contains the unit
/// (`member(radius)`; the scan starts from twice the largest radius, a ring replaces the kept one only when strictly smaller,
/// so equal radii keep the first); in max mode it takes the largest amount (an ordered compare, so NaN never wins; the start is
/// 0.0). A zero amount (the x87 `!=`, NaN counts as non-zero) stores nothing. The immunity gates follow: with the Misc rule on,
/// a Magic row (4) or a class in mask A is stopped by invulnerability or magic immunity (counter > 0), a row in 1, 2, 3, 5, 6
/// or a class in mask B by the ethereal counter (> 0); with it off every class but 16, 32, 0x4000000, 0x400000 is stopped by
/// invulnerability or magic immunity. CONFIRMED (V7, Unicorn `tools/emu/v7_ring_emu.py`, 600 cases, membership stubbed).
pub fn ring_collect(
    rings: &[(F32, F32)],
    max_radius: F32,
    p: &RingProbe,
    member: impl Fn(F32) -> bool,
) -> Option<RingEntry> {
    let amount = if p.max_mode {
        let mut best = F32::ZERO;
        for (_, a) in rings {
            if best.to_f32() < a.to_f32() {
                best = *a;
            }
        }
        best
    } else {
        let mut kept = arith::mul(max_radius, F32::from_bits(0x4000_0000));
        let mut amount = F32::ZERO;
        for (r, a) in rings {
            if kept.to_f32() > r.to_f32() && member(*r) {
                kept = *r;
                amount = *a;
            }
        }
        amount
    };
    // the x87 `0.0 != amount`: only an equal pair is "zero"
    if amount.to_f32() == 0.0 {
        return None;
    }
    let (check_a, check_b) = if p.misc_resist {
        let a = p.row == 4 || p.class & CLASS_MASK_A != 0;
        let b = matches!(p.row, 1 | 2 | 3 | 5 | 6) || p.class & CLASS_MASK_B != 0;
        (a, b)
    } else if matches!(p.class, 16 | 32 | 0x400_0000 | 0x40_0000) {
        (false, false)
    } else {
        (true, false)
    };
    if check_a {
        if p.invulnerable {
            return None;
        }
        if p.unit_known && p.magic_immune > 0 {
            return None;
        }
    } else if check_b && p.unit_known && p.ethereal > 0 {
        return None;
    }
    let bucket = if p.owner_filter != -1 && p.unit_known && !p.rel_a {
        0
    } else if p.owner_filter != -1 && p.unit_known && p.rel_b {
        1
    } else {
        2
    };
    Some(RingEntry { amount, bucket })
}

fn dist2(gm: &Game, a: (F32, F32), k: ObjKey) -> F32 {
    let u = gm.unit(k);
    let (dx, dy) = (arith::sub(u.x, a.0), arith::sub(u.y, a.1));
    arith::add(arith::mul(dx, dx), arith::mul(dy, dy))
}

/// Whether the squared distance `d2` is within radius `r` (inclusive, like the group range test).
fn within(d2: F32, r: F32) -> bool {
    !arith::lt(arith::mul(r, r), d2)
}

fn hero_dur(gm: &Game, target: ObjKey, level: &SpellLevel) -> F32 {
    if gm.target_hero(target) {
        level.hero_dur
    } else {
        level.dur
    }
}

impl Game {
    /// The on-hit effects of this part, called by the attack path after a hit that left `target` alive. Returns the
    /// units that died of the extra damage (the caller runs their death events).
    pub fn a2_on_hit(&mut self, attacker: ObjKey, target: ObjKey) -> Vec<ObjKey> {
        let mut died = Vec::new();
        self.grab_hit(attacker);
        for base in [FLAMING_ATTACK, FLAK_CANNON, FRAG_SHARDS, CORROSIVE_BREATH, FROST_BREATH, FREEZING_BREATH, LIQUID_FIRE] {
            if !self.alive(target) {
                break;
            }
            if !has(self, attacker, base) {
                continue;
            }
            let Some(level) = self.ability_level_record(attacker, base) else {
                continue;
            };
            match base {
                FLAMING_ATTACK => self.flaming_attack(attacker, target, &level, &mut died),
                FLAK_CANNON => self.tiered_splash(attacker, target, &level, "air,enemy,neutral", [level.area, level.data[0], level.data[1]], [level.data[2], level.data[3], level.data[4]], &mut died),
                FRAG_SHARDS => self.tiered_splash(attacker, target, &level, "ground,enemy,neutral", [level.area, level.data[0], level.data[1]], [level.data[2], level.data[3], level.data[4]], &mut died),
                CORROSIVE_BREATH => self.corrosive_hit(attacker, target, &level),
                FROST_BREATH => {
                    if targs_ok(self, attacker, target, targs_word("air,ground,enemy,neutral")) {
                        self.frost_apply(target, &level);
                    }
                }
                FREEZING_BREATH => self.freezing_hit(attacker, target, &level),
                _ => self.liquid_fire_hit(attacker, target, &level),
            }
        }
        died
    }

    /// Flaming Attack (`CAbilityFlamingAttack < CAbilityFlamingArrows`, an orb like Searing Arrows): each hit pays the level's
    /// `Cost` mana (skipped when the mana is short), the target takes `DataA` fire damage and the units around it take
    /// that damage scaled by `DataB` (inside `DataE`, "half damage radius") or `DataC` (inside `Area`); `DataD` is the full
    /// damage radius and `DataA` is the damage ("Damage Bonus"). The ring labels come from the editor; the target itself
    /// takes the full amount. UNSURE: the targs of the ring (the row's attack targs are used for all).
    fn flaming_attack(&mut self, attacker: ObjKey, target: ObjKey, level: &SpellLevel, died: &mut Vec<ObjKey>) {
        let word = targs_word("air,ground,structure,enemy,neutral");
        if !targs_ok(self, attacker, target, word) {
            return;
        }
        if level.cost > 0 {
            let (mana, c) = (self.unit(attacker).mana, conv::i2r(level.cost));
            if arith::lt(mana, c) {
                return;
            }
            self.write_mana(attacker, arith::sub(mana, c));
        }
        let at = (self.unit(target).x, self.unit(target).y);
        if !arith::lt(F32::ZERO, level.data[0]) {
            return;
        }
        // V7 (CONFIRMED): the handler `0x6F09DE00` fills the launch record's splash fields (`flaming_splash_fields`), which the
        // verified splash engine reads (`splash::ring_factor`, `splash::query_radius`, `bounce::launch_factors`).
        let [hfact, qfact, farea, harea, qarea] = flaming_splash_fields(level.area, [level.data[0], level.data[1], level.data[2], level.data[3], level.data[4]]);
        let (hfact, qfact) = crate::bounce::launch_factors(hfact, qfact);
        let query = crate::splash::query_radius(farea, harea, qarea);
        let mut victims = vec![target];
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, query) {
            if k != target && self.alive(k) && !self.unit(k).hidden && crate::orc::area_predicate(self, attacker, k, word) {
                victims.push(k);
            }
        }
        for k in victims {
            let d2 = dist2(self, at, k);
            let factor = if k == target {
                F32::ONE
            } else {
                match crate::splash::ring_factor(farea, harea, qarea, hfact, qfact, |r| within(d2, r)) {
                    Some(f) => f,
                    None => continue,
                }
            };
            let amount = arith::mul(level.data[0], factor);
            if let Some(d) = self.unit_damage_target(attacker, k, amount, false, false, 0, DT_FIRE) {
                died.push(d);
            }
        }
    }

    /// Flak Cannon and Frag Shards (`CAbilityPassive`, three radius tiers): a hit on a target the ability's `targs`
    /// accept damages the other units around the target. `radii` / `amounts` are the editor's full, medium and small
    /// tiers; radii are paired in ascending order with the amounts in descending order (the stock Frag Shards row lists
    /// medium 275 and small 250). The target already took the attack's damage. UNSURE: whether the target also takes the
    /// full tier (modelled: no).
    #[allow(clippy::too_many_arguments)]
    fn tiered_splash(&mut self, attacker: ObjKey, target: ObjKey, level: &SpellLevel, targs: &str, radii: [F32; 3], amounts: [F32; 3], died: &mut Vec<ObjKey>) {
        let word = targs_word(targs);
        if !targs_ok(self, attacker, target, word) {
            return;
        }
        // V7 (CONFIRMED, `0x6F177D00`, `0x6F2C2E80`): the rings are (Area, DataC), (DataA, DataD), (DataB, DataE); a unit takes
        // the amount of the smallest ring that contains it, subject to the collector's immunity gates. `radii` / `amounts`
        // are kept for the callers' signature; they are the same data in the editor's order.
        let _ = (radii, amounts);
        let rings = tier_rings(level.area, [level.data[0], level.data[1], level.data[2], level.data[3], level.data[4]]);
        let mut max_radius = F32::ZERO;
        for (r, _) in &rings {
            if max_radius.to_f32() < r.to_f32() {
                max_radius = *r;
            }
        }
        let at = (self.unit(target).x, self.unit(target).y);
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, max_radius) {
            if k == target || !self.alive(k) || self.unit(k).hidden || !crate::orc::area_predicate(self, attacker, k, word) {
                continue;
            }
            let d2 = dist2(self, at, k);
            let probe = self.ring_probe(k, 0x10, 1);
            let Some(e) = ring_collect(&rings, max_radius, &probe, |r| within(d2, r)) else { continue };
            if !arith::lt(F32::ZERO, e.amount) {
                continue;
            }
            if let Some(d) = self.unit_damage_target(attacker, k, e.amount, true, true, 0, DT_ENHANCED) {
                died.push(d);
            }
        }
    }

    /// The collector's view of candidate `k` for a record of damage class `class` and attack row `row` (`ring_collect`).
    fn ring_probe(&self, k: ObjKey, class: u32, row: u32) -> RingProbe {
        let u = self.unit(k);
        RingProbe {
            class,
            row,
            misc_resist: self.misc_magic_immunes_resist(),
            max_mode: false,
            invulnerable: self.unit_is_invulnerable(k),
            magic_immune: u.magic_immune,
            ethereal: u.ethereal,
            unit_known: true,
            owner_filter: -1,
            rel_a: true,
            rel_b: false,
        }
    }

    /// Corrosive Breath (`Acor`, buff `Bcor` over `CBuffLifeDoT`): a structure hit takes `DataA` damage per second for `Dur`.
    fn corrosive_hit(&mut self, attacker: ObjKey, target: ObjKey, level: &SpellLevel) {
        if !targs_ok(self, attacker, target, targs_word("structure,enemy,neutral")) {
            return;
        }
        let dur = hero_dur(self, target, level);
        if !arith::lt(F32::ZERO, dur) {
            return;
        }
        let preds = BuffPredicates { negative: true, physical: true, ..buff_predicates(BUFF_CORROSIVE) };
        let serial = self.apply_mods_buff(target, BUFF_CORROSIVE, preds, Default::default(), dur);
        self.register_tick(target, serial, attacker, level.data[0], false, DT_ACID, true);
    }

    /// Freezing Breath (`Afrz`, buff `Bfrz` over `CBuffStun`): the structure hit and every enemy structure within `Area` of
    /// it are stunned for `Dur`.
    fn freezing_hit(&mut self, attacker: ObjKey, target: ObjKey, level: &SpellLevel) {
        let word = targs_word("structure,enemy,neutral");
        if !targs_ok(self, attacker, target, word) {
            return;
        }
        let dur = hero_dur(self, target, level);
        if !arith::lt(F32::ZERO, dur) {
            return;
        }
        let at = (self.unit(target).x, self.unit(target).y);
        let mut victims = vec![target];
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, level.area) {
            if k != target && self.alive(k) && crate::orc::area_predicate(self, attacker, k, word) {
                victims.push(k);
            }
        }
        for k in victims {
            self.apply_stun_as(BUFF_FREEZING, k, dur);
        }
    }

    /// Liquid Fire (`Aliq`, buff `Bliq` over `CBuffPoisonAttack`): a structure hit burns for `Dur`: `DataA` damage per
    /// second and the structure's attack speed drops by `DataC` ("Attack Speed Reduction"); `DataB` slows movement (0 for the
    /// stock row). UNSURE: `DataD` "repairs allowed" (0: no repair while burning) has no sink.
    fn liquid_fire_hit(&mut self, attacker: ObjKey, target: ObjKey, level: &SpellLevel) {
        if !targs_ok(self, attacker, target, targs_word("structure,enemy,neutral")) {
            return;
        }
        let dur = hero_dur(self, target, level);
        if !arith::lt(F32::ZERO, dur) {
            return;
        }
        let preds = BuffPredicates { negative: true, physical: true, ..buff_predicates(BUFF_LIQUID_FIRE) };
        let mods = crate::unit_spells::UnitMods {
            move_delta: arith::neg(level.data[1]),
            attack_delta: arith::neg(level.data[2]),
            ..Default::default()
        };
        let serial = self.apply_mods_buff(target, BUFF_LIQUID_FIRE, preds, mods, dur);
        self.register_tick(target, serial, attacker, level.data[0], false, DT_FIRE, true);
    }

    /// The death hook (called from the unit death dispatch): a unit with `Adda` explodes after the level's `Dur`.
    pub(crate) fn a2_on_unit_died(&mut self, unit: ObjKey) {
        if !has(self, unit, DEATH_DAMAGE) {
            return;
        }
        // V7: the handler `0x6F1E7200` does nothing for an illusion or a removed unit (`death_damage_runs`).
        let (illusion, removed) = (self.unit(unit).illusion, self.unit(unit).removed);
        if !death_damage_runs(true, 0, if illusion { 0x4000_0000 } else { 0 }, u8::from(removed)) {
            return;
        }
        let delay = self.ability_level_record(unit, DEATH_DAMAGE).map_or(0.0, |l| l.dur.to_f32().max(0.0));
        self.timers.start(
            delay,
            false,
            Payload::Spell(SpellMsg::UnitSpell { kind: KIND_DEATH_AOE, unit, base: DEATH_DAMAGE, aux: 0 }),
        );
    }

    /// The explosion of a dead `Adda` unit: `DataB` damage inside `DataA`, `DataD` damage inside `DataC` (the partial ring).
    fn death_explosion(&mut self, unit: ObjKey) -> Vec<ObjKey> {
        let Some(level) = self.ability_level_record(unit, DEATH_DAMAGE) else {
            return Vec::new();
        };
        let word = targs_word("ground,structure,debris,enemy,ward");
        let at = (self.unit(unit).x, self.unit(unit).y);
        // V7 (CONFIRMED, `0x6F1E7200`, `0x6F2C2E80`): rings (DataA, DataB), (DataC, DataD); the smallest containing ring wins.
        let rings = death_damage_rings([level.data[0], level.data[1], level.data[2], level.data[3]]);
        let mut outer = F32::ZERO;
        for (r, _) in &rings {
            if outer.to_f32() < r.to_f32() {
                outer = *r;
            }
        }
        let mut died = Vec::new();
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, outer) {
            if k == unit || !self.alive(k) || self.unit(k).hidden || !crate::orc::area_predicate(self, unit, k, word) {
                continue;
            }
            let d2 = dist2(self, at, k);
            let probe = self.ring_probe(k, 1 << DT_DEMOLITION, 0);
            let Some(e) = ring_collect(&rings, outer, &probe, |r| within(d2, r)) else { continue };
            let amount = e.amount;
            if let Some(d) = self.unit_damage_target(unit, k, amount, false, false, 0, DT_DEMOLITION) {
                died.push(d);
            }
        }
        died
    }
}


// ---- trees: Eat Tree and Grab Tree (Aeat, Agra) ----

pub const EAT_TREE: u32 = code(b"Aeat");
pub const GRAB_TREE: u32 = code(b"Agra");
pub const BUFF_EAT_TREE: u32 = code(b"Beat");
pub const BUFF_GRAB_TREE: u32 = code(b"Bgra");
/// Order ids: the original cast getters (slot `0x348`, `docs/research/ability-orders/getters.tsv`).
pub const ORDER_EAT_TREE: u32 = 852_146;
pub const ORDER_GRAB_TREE: u32 = 852_511;
/// Timer kind of the tree stages.
pub const KIND_TREE: u8 = 0x51;
/// How far from the caster the tree may stand when the cast takes effect (the unit walks up to the tree in the original;
/// the model has no approach, so a farther tree makes the order a no-op: a stand-in).
const TREE_CAST_REACH: f32 = 160.0;

impl Game {
    /// The order on a tree: the tree under `(x, y)` that the caster can reach is torn out after `DataA` seconds (the "rip
    /// delay" / "attach delay" labels); nothing happens when no standing tree is there.
    fn start_tree_cast(&mut self, rec: &CastRec, x: F32, y: F32) {
        let Some(i) = self.tree_near(x.to_f32(), y.to_f32(), TREE_PICK_RADIUS) else { return };
        let t = self.production().trees[i];
        let (cx, cy) = (self.unit(rec.caster).x.to_f32(), self.unit(rec.caster).y.to_f32());
        if ((t.x - cx).powi(2) + (t.y - cy).powi(2)).sqrt() > TREE_CAST_REACH {
            return;
        }
        let delay = rec.level.data[0].to_f32().max(0.0);
        self.timers.start(
            delay,
            false,
            Payload::Spell(SpellMsg::UnitSpell { kind: KIND_TREE, unit: rec.caster, base: rec.base, aux: (i as u32) << 2 }),
        );
    }

    /// One stage of a tree ability (`aux` = tree index << 2 | stage). Eat Tree: stage 0 fells the tree and waits `DataB`;
    /// stage 1 gives `Beat`, which heals `DataC` over `Dur` as a Rejuvenation. Grab Tree: stage 0 fells the tree and puts
    /// `Bgra` on the unit for `DataE` attacks (`DataD` "enabled attack index" = 1, the weapon switch is not modelled);
    /// stage 2 (the remove delay after the last attack) takes the buff off.
    pub(crate) fn tree_stage(&mut self, unit: ObjKey, base: u32, aux: u32) {
        let (i, stage) = ((aux >> 2) as usize, aux & 3);
        if !self.alive(unit) {
            return;
        }
        let Some(level) = self.ability_level_record(unit, base) else { return };
        match (base, stage) {
            (EAT_TREE, 0) | (GRAB_TREE, 0) => {
                if self.production().trees.get(i).is_none_or(|t| t.wood <= 0) {
                    return;
                }
                self.fell_tree(i);
                if base == EAT_TREE {
                    let delay = level.data[1].to_f32().max(0.0);
                    self.timers.start(
                        delay,
                        false,
                        Payload::Spell(SpellMsg::UnitSpell { kind: KIND_TREE, unit, base, aux: (aux & !3) | 1 }),
                    );
                } else {
                    let attacks = conv::f2i(level.data[4]).max(1);
                    self.spells.unit_a.b.grabbed.insert(unit, attacks);
                    let preds = BuffPredicates { positive: true, ..buff_predicates(BUFF_GRAB_TREE) };
                    self.apply_mods_buff(unit, BUFF_GRAB_TREE, preds, Default::default(), F32::ZERO);
                }
            }
            (EAT_TREE, 1) => {
                let dur = hero_dur(self, unit, &level);
                if !arith::lt(F32::ZERO, dur) {
                    return;
                }
                let per_second = self.soft_div(level.data[2], dur);
                let preds = BuffPredicates { positive: true, magic: true, dispellable: true, ..buff_predicates(BUFF_EAT_TREE) };
                let total = arith::add(dur, F32::from_bits(0.01f32.to_bits()));
                let serial = self.apply_mods_buff(unit, BUFF_EAT_TREE, preds, Default::default(), total);
                self.register_tick(unit, serial, unit, per_second, true, 0, false);
            }
            (GRAB_TREE, 2) => {
                self.spells.unit_a.b.grabbed.remove(&unit);
                self.remove_buff_code(unit, BUFF_GRAB_TREE);
            }
            _ => {}
        }
    }

    /// An attack hit by a unit that holds a tree: one of its `DataE` attacks is used up; after the last one the tree is
    /// dropped `DataB` seconds later.
    fn grab_hit(&mut self, attacker: ObjKey) {
        let Some(left) = self.spells.unit_a.b.grabbed.get_mut(&attacker) else { return };
        *left -= 1;
        if *left > 0 {
            return;
        }
        *left = i32::MAX;
        let delay = self.ability_level_record(attacker, GRAB_TREE).map_or(0.0, |l| l.data[1].to_f32().max(0.0));
        self.timers.start(
            delay,
            false,
            Payload::Spell(SpellMsg::UnitSpell { kind: KIND_TREE, unit: attacker, base: GRAB_TREE, aux: 2 }),
        );
    }
}


// ---- units: Couple, Decouple, Battlestations, Sacrificial Lamb (Acoa, Acoh, Acoi, Adec, Abtl, Alam) ----

pub const COUPLE_ARCHER: u32 = code(b"Acoa");
pub const COUPLE_HIPPO: u32 = code(b"Acoh");
pub const COUPLE_INSTANT: u32 = code(b"Acoi");
pub const DECOUPLE: u32 = code(b"Adec");
pub const BATTLESTATIONS: u32 = code(b"Abtl");
pub const SACRIFICIAL_LAMB: u32 = code(b"Alam");
/// Order ids: the names `coupletarget`, `coupleinstant`, `decouple`, `battlestations`, `requestsacrifice` of the original
/// table; the cast getters of `Acoi` (852507 cast, 852508 primary), `Adec`, `Abtl` and `Alam` agree.
pub const ORDER_COUPLE_TARGET: u32 = 852_507;
pub const ORDER_COUPLE_INSTANT: u32 = 852_508;
pub const ORDER_DECOUPLE: u32 = 852_509;
pub const ORDER_BATTLESTATIONS: u32 = 852_099;
pub const ORDER_SACRIFICE: u32 = 852_201;
/// The Shade an acolyte becomes at the Sacrificial Pit (`ushd`; the type comes from the pit's train list in the original).
const SHADE: u32 = code(b"ushd");
/// Distance of the two units a rider splits into.
const SPLIT_OFFSET: f32 = 32.0;

fn is_couple(base: u32) -> bool {
    matches!(base, COUPLE_ARCHER | COUPLE_HIPPO | COUPLE_INSTANT)
}

impl Game {
    /// The couple data of `unit`: `(partner type, result type)`. `DataA` of the stock rows is a rawcode text; the level
    /// loader stores a rawcode field as the bits of the `F32` (the host's `spell_levels` hookup, as for `UnitID`).
    fn couple_data(&self, unit: ObjKey) -> Option<(u32, u32)> {
        [COUPLE_ARCHER, COUPLE_HIPPO].into_iter().filter(|&c| has(self, unit, c)).find_map(|c| {
            let l = self.ability_level_record(unit, c)?;
            Some((l.data[0].to_bits(), l.unit_id))
        })
    }

    /// Whether `other` can be coupled with `unit`: an own, living, free unit of the partner type.
    pub(crate) fn couple_partner_ok(&self, unit: ObjKey, other: ObjKey) -> bool {
        let Some((partner, _)) = self.couple_data(unit) else { return false };
        other != unit
            && self.alive(other)
            && !self.unit(other).loaded
            && self.unit(other).owner == self.unit(unit).owner
            && self.unit(other).type_id == partner
    }

    /// The closest eligible partner of `unit` (`coupleinstant`).
    fn closest_partner(&self, unit: ObjKey) -> Option<ObjKey> {
        let at = (self.unit(unit).x, self.unit(unit).y);
        self.unit_objects()
            .map(|(k, _)| k)
            .filter(|&k| self.couple_partner_ok(unit, k))
            .map(|k| (k, dist2(self, at, k)))
            .min_by(|a, b| a.1.to_f32().total_cmp(&b.1.to_f32()).then(a.0.cmp(&b.0)))
            .map(|(k, _)| k)
    }

    /// Merges `unit` and `partner` into one unit of the result type at `unit`'s place.
    fn couple(&mut self, unit: ObjKey, partner: ObjKey) -> Option<ObjKey> {
        let (_, result) = self.couple_data(unit)?;
        if !self.couple_partner_ok(unit, partner) || result == 0 || self.unit_types().get(result).is_none() {
            return None;
        }
        let (owner, x, y, face) = {
            let u = self.unit(unit);
            (u.owner, u.x, u.y, u.facing)
        };
        crate::natives::unit::remove_unit(self, partner);
        crate::natives::unit::remove_unit(self, unit);
        crate::natives::unit::build(self, owner, result, x, y, face, true).map(|(k, _)| k)
    }

    /// Splits the rider into the units `DataA` and `DataB`, either side of it; both get the rider's life fraction.
    fn decouple(&mut self, unit: ObjKey, level: &SpellLevel) {
        let (a, b) = (level.data[0].to_bits(), level.data[1].to_bits());
        if self.unit_types().get(a).is_none() || self.unit_types().get(b).is_none() {
            return;
        }
        let (owner, x, y, face, lf) = {
            let u = self.unit(unit);
            let lf = if arith::lt(F32::ZERO, u.max_life) { self.soft_div(u.life, u.max_life) } else { F32::ONE };
            (u.owner, u.x, u.y, u.facing, lf)
        };
        crate::natives::unit::remove_unit(self, unit);
        let off = F32::from_bits(SPLIT_OFFSET.to_bits());
        for (ty, dx) in [(a, arith::neg(off)), (b, off)] {
            if let Some((k, _)) = crate::natives::unit::build(self, owner, ty, arith::add(x, dx), y, face, true) {
                let max = self.unit(k).max_life;
                self.write_life(k, arith::mul(max, lf));
            }
        }
    }

    /// Battlestations: every own unit of the allowed type (`UnitID`) within `Area` goes into the burrow while it has room;
    /// units on a harvest cycle only when `DataB` ("Summon Busy Units") is set. The walk to the burrow is skipped.
    fn battlestations(&mut self, burrow: ObjKey, level: &SpellLevel) {
        let (owner, at) = (self.unit(burrow).owner, (self.unit(burrow).x, self.unit(burrow).y));
        let busy = level.data[1].to_bits() & 0x7FFF_FFFF != 0;
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, level.area) {
            if self.unit(k).owner != owner || self.unit(k).type_id != level.unit_id || !self.can_load(burrow, k) {
                continue;
            }
            if self.is_harvesting(k) {
                if !busy {
                    continue;
                }
                self.end_harvest(k);
            }
            self.load_unit(burrow, k);
        }
    }

    /// Sacrificial Lamb: the acolyte is consumed and a Shade stands at the pit.
    fn sacrifice(&mut self, acolyte: ObjKey, pit: ObjKey) {
        if !self.alive(pit) || self.unit_types().get(SHADE).is_none() {
            return;
        }
        let (owner, x, y, face) = {
            let p = self.unit(pit);
            (self.unit(acolyte).owner, p.x, p.y, p.facing)
        };
        crate::natives::unit::remove_unit(self, acolyte);
        crate::natives::unit::build(self, owner, SHADE, x, y, face, true);
    }
}


// ---- corpses: Cannibalize, Ancestral Spirit, Devour Magic, Graveyard (Acan, Aast, Advm, Agyd) ----

pub const CANNIBALIZE: u32 = code(b"Acan");
pub const ANCESTRAL_SPIRIT: u32 = code(b"Aast");
pub const DEVOUR_MAGIC: u32 = code(b"Advm");
pub const GRAVEYARD: u32 = code(b"Agyd");
/// `devourmagic` (cast getter of `Advm`, CONFIRMED in `getters.tsv`); the corpse orders are `cannibalize` 852188 /
/// 852535 and `ancestralspirit` 852490 / 852491.
pub const ORDER_DEVOUR_MAGIC: u32 = 852_536;
pub const KIND_CANNIBAL: u8 = 0x52;
pub const KIND_GRAVEYARD: u8 = 0x53;
/// How far the instant `cannibalize` order looks for a corpse (the closest-target search radius is not decoded: a
/// stand-in).
const CANNIBALIZE_SEARCH: f32 = 500.0;
/// The ring of the graveyard's corpses: eight directions.
const R: f32 = std::f32::consts::FRAC_1_SQRT_2;
const DIRS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (0.0, 1.0),
    (-1.0, 0.0),
    (0.0, -1.0),
    (R, R),
    (-R, R),
    (-R, -R),
    (R, -R),
];

impl Game {
    /// Whether `k` is a corpse the corpse abilities can use: dead, not removed, not an illusion, not a structure or a
    /// mechanical unit; `non_hero` also excludes heroes (they come back through the altar).
    pub(crate) fn corpse_ok(&self, k: ObjKey, non_hero: bool) -> bool {
        let u = self.unit(k);
        u.dead
            && !u.removed
            && !u.illusion
            && !crate::natives::unit_attr::is_unit_type(self, u, crate::unitclass::ty::STRUCTURE)
            && !crate::natives::unit_attr::is_unit_type(self, u, crate::unitclass::ty::MECHANICAL)
            && (!non_hero || !self.target_hero(k))
    }

    /// The closest corpse within `radius` of `unit` (`own` restricts it to the unit's player's).
    fn closest_corpse(&self, unit: ObjKey, radius: F32, own: bool) -> Option<ObjKey> {
        let at = (self.unit(unit).x, self.unit(unit).y);
        let owner = self.unit(unit).owner;
        crate::natives::group::circle_candidates(self, at.0, at.1, radius)
            .into_iter()
            .filter(|&k| k != unit && self.corpse_ok(k, own) && (!own || self.unit(k).owner == owner))
            .map(|k| (k, dist2(self, at, k)))
            .min_by(|a, b| a.1.to_f32().total_cmp(&b.1.to_f32()).then(a.0.cmp(&b.0)))
            .map(|(k, _)| k)
    }

    /// Cannibalize: the unit stands at its place and, once a second for up to `Dur`, heals `DataA` while its life is below
    /// the smaller of its maximum and `DataB` ("Max Hit Points"). Moving away or dying ends it; at the end the corpse is
    /// eaten. UNSURE: the stop rules (the original is a channel; the cast core's channel lifecycle is not used).
    fn start_cannibalize(&mut self, unit: ObjKey, corpse: ObjKey, level: &SpellLevel) {
        if !self.corpse_ok(corpse, false) {
            return;
        }
        let dur = hero_dur(self, unit, level).to_f32();
        let at = (self.unit(unit).x, self.unit(unit).y);
        self.spells.unit_a.b.cannibals.insert(unit, (corpse, dur, at));
        self.timers.start(1.0, false, Payload::Spell(SpellMsg::UnitSpell { kind: KIND_CANNIBAL, unit, base: CANNIBALIZE, aux: 0 }));
    }

    fn cannibalize_tick(&mut self, unit: ObjKey) {
        let Some(&(corpse, left, at)) = self.spells.unit_a.b.cannibals.get(&unit) else { return };
        let moved = self.alive(unit) && (self.unit(unit).x != at.0 || self.unit(unit).y != at.1);
        let level = self.ability_level_record(unit, CANNIBALIZE);
        if !self.alive(unit) || moved || level.is_none() || self.is_stunned(unit) || !self.corpse_ok(corpse, false) {
            self.spells.unit_a.b.cannibals.remove(&unit);
            return;
        }
        let level = level.unwrap_or_default();
        let (life, max) = (self.unit(unit).life, self.unit(unit).max_life);
        let cap = if arith::lt(F32::ZERO, level.data[1]) && arith::lt(level.data[1], max) { level.data[1] } else { max };
        let done = left <= 1.0 || !arith::lt(life, cap);
        if arith::lt(life, cap) {
            let next = arith::add(life, level.data[0]);
            self.write_life(unit, if arith::lt(cap, next) { cap } else { next });
        }
        if done {
            self.spells.unit_a.b.cannibals.remove(&unit);
            crate::natives::unit::remove_unit(self, corpse);
            return;
        }
        self.spells.unit_a.b.cannibals.insert(unit, (corpse, left - 1.0, at));
        self.timers.start(1.0, false, Payload::Spell(SpellMsg::UnitSpell { kind: KIND_CANNIBAL, unit, base: CANNIBALIZE, aux: 0 }));
    }

    /// Ancestral Spirit: the own corpse comes back with `DataA` ("Life Restored Factor") of its maximum life and `DataB`
    /// ("Mana Restored Factor") of its maximum mana.
    fn ancestral_spirit(&mut self, _caster: ObjKey, corpse: ObjKey, level: &SpellLevel) {
        if !self.corpse_ok(corpse, true) {
            return;
        }
        self.resurrect_unit(corpse);
        let (max_life, max_mana) = (self.unit(corpse).max_life, self.unit(corpse).max_mana);
        self.write_life(corpse, arith::mul(max_life, level.data[0]));
        self.write_mana(corpse, arith::mul(max_mana, level.data[1]));
    }

    /// Devour Magic: on every unit within `Area` of `at` the Dispel Magic worker runs in its smart mode (positive buffs of
    /// enemies, negative ones of the others; `DataE` damages summoned enemies); friends are left alone when `DataF` is set
    /// ("Ignore Friendly Buffs"). Every unit that lost a buff or took the summoned damage gives the caster `DataA` life and
    /// `DataB` mana, each buff removed `DataC` and `DataD` more. Returns the units killed.
    fn devour_magic(&mut self, caster: ObjKey, level: &SpellLevel, at: (F32, F32)) -> Vec<ObjKey> {
        let owner = self.unit(caster).owner;
        let skip_friends = level.data[5].to_bits() & 0x7FFF_FFFF != 0;
        let (mut life, mut mana) = (F32::ZERO, F32::ZERO);
        let mut died = Vec::new();
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, level.area) {
            if !self.alive(k) || self.unit(k).hidden {
                continue;
            }
            let enemy = self.is_enemy(owner, self.unit(k).owner);
            if !enemy && skip_friends && k != caster {
                continue;
            }
            let (before, hp) = (self.unit(k).buffs.len(), self.unit(k).life);
            let killed = crate::unit_spells::dispel_one(self, caster, k, level.data[4], F32::ZERO, true, 1);
            let removed = before.saturating_sub(self.unit(k).buffs.len());
            let hurt = killed.is_some() || self.unit(k).life.to_bits() != hp.to_bits();
            died.extend(killed);
            if removed == 0 && !hurt {
                continue;
            }
            let n = conv::i2r(removed as i32);
            life = arith::add(life, arith::add(level.data[0], arith::mul(level.data[2], n)));
            mana = arith::add(mana, arith::add(level.data[1], arith::mul(level.data[3], n)));
        }
        if self.alive(caster) {
            let (l, m) = (self.unit(caster).life, self.unit(caster).mana);
            let (ml, mm) = (self.unit(caster).max_life, self.unit(caster).max_mana);
            let nl = arith::add(l, life);
            let nm = arith::add(m, mana);
            self.write_life(caster, if arith::lt(ml, nl) { ml } else { nl });
            self.write_mana(caster, if arith::lt(mm, nm) { mm } else { nm });
        }
        died
    }

    /// The graveyard tick: while fewer than `DataA` corpses of the level's `UnitID` lie within `DataC` of the unit, a new one
    /// is made on the ring of radius `DataB` (the next free of eight directions). The corpse is a built unit made dead the
    /// way `CreateCorpse` does (life 0, no death event).
    fn graveyard_tick(&mut self, unit: ObjKey) {
        if !self.alive(unit) || !has(self, unit, GRAVEYARD) {
            if let Some(id) = self.spells.unit_a.b.graveyards.remove(&unit) {
                self.timers.cancel(id);
            }
            return;
        }
        let Some(level) = self.ability_level_record(unit, GRAVEYARD) else { return };
        if level.unit_id == 0 || self.unit_types().get(level.unit_id).is_none() {
            return;
        }
        let at = (self.unit(unit).x, self.unit(unit).y);
        let near = crate::natives::group::circle_candidates(self, at.0, at.1, level.data[2])
            .into_iter()
            .filter(|&k| self.unit(k).dead && !self.unit(k).removed && self.unit(k).type_id == level.unit_id)
            .count();
        if (near as i32) >= conv::f2i(level.data[0]) {
            return;
        }
        let (dx, dy) = DIRS[near % 8];
        let r = level.data[1];
        let x = arith::add(at.0, arith::mul(F32::from_bits(dx.to_bits()), r));
        let y = arith::add(at.1, arith::mul(F32::from_bits(dy.to_bits()), r));
        let owner = self.unit(unit).owner;
        let Some((k, _)) = crate::natives::unit::build(self, owner, level.unit_id, x, y, F32::ZERO, false) else { return };
        self.stop_all_orders(k);
        self.write_life(k, F32::ZERO);
        self.unit_mut(k).dead = true;
        self.stats_set_dead(k, true);
    }
}


// ---- area effects: Flare, Blight growth, Attribute Bonus (Afla, Ablp, Abli, Aamk) ----

pub const FLARE: u32 = code(b"Afla");
pub const BLIGHT_PLACEMENT: u32 = code(b"Ablp");
pub const BLIGHT_GROWTH: u32 = code(b"Abli");
pub const ATTRIBUTE_SKILL: u32 = code(b"Aamk");
/// `flare` (cast getter of `Afla`, CONFIRMED in `getters.tsv`).
pub const ORDER_FLARE: u32 = 852_060;
pub const KIND_FLARE: u8 = 0x54;
pub const KIND_BLIGHT: u8 = 0x55;

impl Game {
    /// Flare: after `DataB` seconds ("Effect Delay") the circle of radius `Area` around `at` is visible to the caster's
    /// player for `Dur` (a fog modifier, as the reveal of the item scrolls).
    fn start_flare(&mut self, rec: &CastRec, at: (F32, F32)) {
        self.spells.unit_a.b.next_flare += 1;
        let id = self.spells.unit_a.b.next_flare;
        let dur = rec.level.dur;
        self.spells.unit_a.b.flares.insert(id, (at.0, at.1, rec.level.area, dur));
        let delay = rec.level.data[1].to_f32().max(0.0);
        self.timers.start(delay, false, Payload::Spell(SpellMsg::UnitSpell { kind: KIND_FLARE, unit: rec.caster, base: FLARE, aux: id }));
    }

    fn flare_opens(&mut self, caster: ObjKey, id: u32) {
        if let Some((x, y, area, dur)) = self.spells.unit_a.b.flares.remove(&id) {
            crate::item_abilities::reveal(self, caster, x, y, area, dur);
        }
    }

    /// One growth step of the blight of `unit`: waits (one `Dur` at a time) until the building is complete, then the radius
    /// grows by `DataA` ("Expansion Amount") per step up to `Area`; `DataB` ("Creates Blight") decides whether the cells are
    /// blighted or cleared. UNSURE: the cadence (`Dur` 0.08 s per 64 units) is the stock row read as a step interval.
    fn blight_step(&mut self, unit: ObjKey, base: u32) {
        let key = (unit, base);
        let Some(&radius) = self.spells.unit_a.b.blight.get(&key) else { return };
        let level = self.ability_level_record(unit, base);
        let live = !self.unit(unit).removed && !self.unit(unit).dead && has(self, unit, base);
        let Some(level) = level.filter(|_| live) else {
            self.spells.unit_a.b.blight.remove(&key);
            return;
        };
        let step = level.dur.to_f32().max(0.01);
        if self.unit_completed(unit) {
            let next = arith::add(radius, level.data[0]);
            let next = if arith::lt(level.area, next) { level.area } else { next };
            let (x, y) = (self.unit(unit).x, self.unit(unit).y);
            self.blight_circle(x, y, next, level.data[1].to_bits() & 0x7FFF_FFFF != 0);
            if !arith::lt(next, level.area) || !arith::lt(radius, next) {
                self.spells.unit_a.b.blight.remove(&key);
                self.spells.unit_a.b.blight_done.insert(key);
                return;
            }
            self.spells.unit_a.b.blight.insert(key, next);
        }
        self.timers.start(step, false, Payload::Spell(SpellMsg::UnitSpell { kind: KIND_BLIGHT, unit, base, aux: 0 }));
    }

    /// Attribute Bonus: a hero with `Aamk` carries `F2I(DataA)` agility, `F2I(DataB)` intelligence and `F2I(DataC)` strength
    /// more in its base counters (`attribute_values`); a level change or the loss of the ability adjusts the difference.
    fn sync_attribute_skill(&mut self, unit: ObjKey) {
        if self.unit(unit).hero.is_none() {
            return;
        }
        let want = if has(self, unit, ATTRIBUTE_SKILL) {
            self.ability_level_record(unit, ATTRIBUTE_SKILL).map_or((0, 0, 0), |l| crate::item_abilities::attribute_values(&l.data))
        } else {
            (0, 0, 0)
        };
        let have = self.spells.unit_a.b.attrs.get(&unit).copied().unwrap_or((0, 0, 0));
        if want == have {
            return;
        }
        let (da, di, ds) = (want.0 - have.0, want.1 - have.1, want.2 - have.2);
        crate::natives::hero::change_hero_attributes(self, unit, false, |h| {
            h.agi_base = h.agi_base.wrapping_add(da);
            h.int_base = h.int_base.wrapping_add(di);
            h.str_base = h.str_base.wrapping_add(ds);
        });
        if want == (0, 0, 0) {
            self.spells.unit_a.b.attrs.remove(&unit);
        } else {
            self.spells.unit_a.b.attrs.insert(unit, want);
        }
    }
}
