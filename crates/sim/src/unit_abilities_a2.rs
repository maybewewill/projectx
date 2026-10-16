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

/// Timer dispatch for the kinds of this part.
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, _base: u32, _aux: u32) {
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
        let rings = [(level.data[3], F32::ONE), (level.data[4], level.data[1]), (level.area, level.data[2])];
        let mut victims = vec![target];
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, level.area) {
            if k != target && self.alive(k) && !self.unit(k).hidden && crate::orc::area_predicate(self, attacker, k, word) {
                victims.push(k);
            }
        }
        for k in victims {
            let d2 = dist2(self, at, k);
            let factor = if k == target {
                F32::ONE
            } else {
                match rings.iter().find(|(r, _)| within(d2, *r)) {
                    Some((_, f)) => *f,
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
        let mut r = radii;
        r.sort_by(|a, b| a.to_f32().total_cmp(&b.to_f32()));
        let mut a = amounts;
        a.sort_by(|x, y| y.to_f32().total_cmp(&x.to_f32()));
        let at = (self.unit(target).x, self.unit(target).y);
        let _ = level;
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, r[2]) {
            if k == target || !self.alive(k) || self.unit(k).hidden || !crate::orc::area_predicate(self, attacker, k, word) {
                continue;
            }
            let d2 = dist2(self, at, k);
            let Some(i) = (0..3).find(|&i| within(d2, r[i])) else { continue };
            if !arith::lt(F32::ZERO, a[i]) {
                continue;
            }
            if let Some(d) = self.unit_damage_target(attacker, k, a[i], true, true, 0, DT_ENHANCED) {
                died.push(d);
            }
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
        let outer = if arith::lt(level.data[0], level.data[2]) { level.data[2] } else { level.data[0] };
        let mut died = Vec::new();
        for k in crate::natives::group::circle_candidates(self, at.0, at.1, outer) {
            if k == unit || !self.alive(k) || self.unit(k).hidden || !crate::orc::area_predicate(self, unit, k, word) {
                continue;
            }
            let d2 = dist2(self, at, k);
            let amount = if within(d2, level.data[0]) {
                level.data[1]
            } else if within(d2, level.data[2]) {
                level.data[3]
            } else {
                continue;
            };
            if let Some(d) = self.unit_damage_target(unit, k, amount, false, false, 0, DT_DEMOLITION) {
                died.push(d);
            }
        }
        died
    }
}
