//! Unit abilities, batch M (S47). Spec: `docs/spec/abilities-unit-m.md`.
//!
//! The batch covers the 85 rawcodes of [`CODES`]. Every code is listed with its RTTI class and parent (atlas
//! `docs/research/ability-atlas/factory.tsv`) and the cast-getter order ids of `docs/research/ability-orders`
//! (`tools/analysis/ability_orders.py`). Codes that have a behaviour here are marked in [`Status`]; the rest are
//! recorded as structure so later passes can pick them up. Labels: CONFIRMED = read in the machine code, LIKELY =
//! strong inference from the RTTI parent / editor data, UNSURE = a stand-in. Nothing in this module is CONFIRMED by
//! Unicorn: the numbers are the level fields of the editor data (no new arithmetic constants were found).
//!
//! Behaviour modelled (all through the existing spell core, see `cast.rs`):
//! * Poison Sting / Poison Damage (`Apoi`, `Apo2`): the attack-hit poison of Envenomed Spears (`neutral_spells`):
//!   `DataA` damage per second (class 23), `Dur` / `HeroDur`. (`CAbilityPoisonAttack`, the parent of `Aven`.)
//! * Possession (`Apos`, `Aps2 < Apos`): the target changes owner, the caster is consumed.
//! * Spell Steal (`Asps`): one dispellable buff moves from the target to the caster (UNSURE: the original lets the
//!   player pick the receiving friend later).
//! * Spirit Link (`Aspl`): up to `DataA` units within `Area` share every hit equally (`Bspl`).
//! * Magic Leash / Control Magic (`Amls`): an enemy summoned unit changes owner.
//! * Call to Arms (`Amil` on the hall, `Amic` on a peasant): friendly units with `Amic` become militia for `Dur`.
//! * Queries for the invisibility system of S44: Permanent Invisibility (`Apiv`), Shadow Meld (`Ashm`), True Sight
//!   (`Atru`), Ultravision (`Ault`). No wiring: `crates/sim/src/invisibility.rs` owns the rules.

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, F32};

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::cast::{effect_duration, CastError, CastRec, CastTarget};
use crate::natives::game::{Game, ObjKey};
use crate::natives::ScriptRuntime;
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

pub const POISON_STING: u32 = code(b"Apoi");
pub const POISON_DAMAGE: u32 = code(b"Apo2");
pub const POSSESSION: u32 = code(b"Apos");
pub const POSSESSION_TWO: u32 = code(b"Aps2");
pub const SPELL_STEAL: u32 = code(b"Asps");
pub const SPIRIT_LINK: u32 = code(b"Aspl");
pub const MAGIC_LEASH: u32 = code(b"Amls");
pub const MILITIA: u32 = code(b"Amil");
pub const MILITIA_CONVERT: u32 = code(b"Amic");
pub const PERM_INVIS: u32 = code(b"Apiv");
pub const SHADOW_MELD: u32 = code(b"Ashm");
pub const TRUE_SIGHT: u32 = code(b"Atru");
pub const ULTRAVISION: u32 = code(b"Ault");
pub const BUFF_POISON: u32 = code(b"Bpoi");
pub const BUFF_SPIRIT_LINK: u32 = code(b"Bspl");
pub const BUFF_MILITIA: u32 = code(b"Bmil");
/// The default militia type (`hmil`), used when the ability level has no `UnitID`.
pub const MILITIA_TYPE: u32 = code(b"hmil");
/// The default `Dur` of Call to Arms (45 s) when the level record has none (UNSURE).
const MILITIA_DEFAULT_SECONDS: f32 = 45.0;
/// Damage class of the poison tick, as `neutral_spells`.
const POISON_DAMAGE_TYPE: u32 = 23;

/// How far a code is modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// An effect or passive hook exists in this module.
    Modelled,
    /// A query only (the rules live in another module).
    Query,
    /// Recorded structure only: no behaviour yet.
    Listed,
}

use Status::{Listed, Modelled, Query};

/// `(rawcode, RTTI class, RTTI parent, status)` of the whole batch (atlas `factory.tsv`).
pub const CODES: &[(&[u8; 4], &str, &str, Status)] = &[
    (b"Ambt", "CAbilityManaBattery", "CAbilityAutoTargetSpell", Listed),
    (b"Amdf", "CAbilityMagicDefense", "CAbilityDefend", Listed),
    (b"Amec", "CAbilityMechanicalCritter", "CAbilityWaterElemental", Listed),
    (b"Amed", "CAbilityMeatDrop", "CAbilityCargoDropInstant", Listed),
    (b"Amel", "CAbilityMeatLoad", "CAbilityAutoTargetSpell", Listed),
    (b"Amfl", "CAbilityManaFlare", "CAbilitySimpleSpell", Listed),
    (b"Amgl", "CAbilityMoonGlaive", "CAbilityPassive", Listed),
    (b"Amic", "CAbilityMilitiaConvert", "CAbilityCallToArms", Modelled),
    (b"Amil", "CAbilityMilitia", "CAbilityCallToArms", Modelled),
    (b"Amin", "CAbilityLandMine", "CAbility", Listed),
    (b"Amls", "CAbilityMagicLeash", "CAbilitySimpleSpell", Modelled),
    (b"Amou", "CAbilityMount", "CAbilityMorph", Listed),
    (b"Amtc", "CAbilityMeatCargo", "CAbilityCargoHold", Listed),
    (b"Aphx", "CAbilityPhoenix", "CAbilityMorph", Listed),
    (b"Apig", "CAbilityPandaImmolation", "CAbilityPermImmolation", Listed),
    (b"Apit", "CAbilityPurchaseItem", "CAbility", Listed),
    (b"Apiv", "CAbilityPermInvis", "CBonusBase", Query),
    (b"Apo2", "CAbilityPoisonDamage", "CAbilityPoisonAttack", Modelled),
    (b"Apoi", "CAbilityPoisonAttack", "CAbilityPassive", Modelled),
    (b"Apos", "CAbilityPossession", "CAbilitySimpleSpell", Modelled),
    (b"Aps2", "CAbilityPossessionTwo", "CAbilityPossession", Modelled),
    (b"Apsh", "CAbilityPhaseShift", "CAbilityModalSpell", Listed),
    (b"Apts", "CAbilityPlagueToss", "CAbilityPassive", Listed),
    (b"Apxf", "CAbilityPhoenixFire", "CAbilityPassive", Listed),
    (b"Arav", "CAbilityRavenForm", "CAbilityMorph", Listed),
    (b"Arbr", "CAbilityReinforce", "CAbilitySpiked", Listed),
    (b"Arem", "CAbilityRegenMana", "CAbilityRegenBase", Listed),
    (b"Aren", "CAbilityRenew", "CAbilityRepair", Listed),
    (b"Arep", "CAbilityRepair", "CAbilityAutoTargetSpell", Listed),
    (b"Aret", "CAbilityRetrain", "CAbilitySpell", Listed),
    (b"Arev", "CAbilityRevive", "CAbilityBuildQueue", Listed),
    (b"Arng", "CAbilityRevenge", "CAbilitySpell", Listed),
    (b"Aroc", "CAbilityBarrage", "CAbilityPassive", Listed),
    (b"Aroo", "CAbilityRoot", "CAbilityButton", Listed),
    (b"Arpb", "CAbilityReplenish", "CAbilityAutoTargetSpell", Listed),
    (b"Arpl", "CAbilityReplenishLife", "CAbilityReplenish", Listed),
    (b"Arpm", "CAbilityReplenishMana", "CAbilityReplenish", Listed),
    (b"Arst", "CAbilityRestoration", "CAbilityRepair", Listed),
    (b"Artn", "CAbilityReturn", "CAbility", Listed),
    (b"Asac", "CAbilitySacrifice", "CAbilityTrain", Listed),
    (b"Asal", "CAbilitySalvage", "CAbilityPassive", Listed),
    (b"Asds", "CAbilitySelfDestruct", "CAbilityAutoTargetSpell", Listed),
    (b"Ashm", "CAbilityShadowMeld", "CAbilityButton", Query),
    (b"Ashs", "CAbilityShadowSight", "CAbilitySimpleSpell", Listed),
    (b"Asla", "CAbilitySleepAlways", "CAbility", Listed),
    (b"Asod", "CAbilityDeathSpawn", "CAbility", Listed),
    (b"Aspa", "CAbilitySpiderAttack", "CAbility", Listed),
    (b"Aspb", "CAbilitySpellBook", "CAbilitySpell", Listed),
    (b"Aspd", "CAbilityDeathSpawnSpider", "CAbilityDeathSpawn", Listed),
    (b"Asph", "CAbilitySphere", "CBonusBase", Listed),
    (b"Aspi", "CAbilitySpiked", "CBonusBase", Listed),
    (b"Aspl", "CAbilitySpiritLink", "CAbilitySimpleSpell", Modelled),
    (b"Asps", "CAbilitySpellSteal", "CAbilityAutoTargetSpell", Modelled),
    (b"Assk", "CAbilityStoneSkin", "CAbilityPassive", Listed),
    (b"Assp", "CAbilityDeathSpawnSpiderling", "CAbilityDeathSpawn", Listed),
    (b"Astd", "CAbilityStandDown", "CAbilityCargoDropInstant", Listed),
    (b"Aste", "CAbilitySteal", "CAbilityDarkRitual", Listed),
    (b"Asth", "CAbilityStormHammers", "CAbilityPassive", Listed),
    (b"Astn", "CAbilityStoneForm", "CAbilityMorph", Listed),
    (b"Atau", "CAbilityTaunt", "CAbilitySimpleSpell", Listed),
    (b"Atdg", "CAbilityTornadoDamage", "CAbility", Listed),
    (b"Atdp", "CAbilityTankDropPilot", "CAbilitySpell", Listed),
    (b"Atlp", "CAbilityTankLoadPilot", "CAbilitySimpleSpell", Listed),
    (b"Atol", "CAbilityTreeOfLife", "CBonusBase", Listed),
    (b"Atpi", "CAbilityTankPilot", "CAbilitySimpleSpell", Listed),
    (b"Atru", "CAbilityTrueSight", "CAbilityDetector", Query),
    (b"Atsp", "CAbilityTornadoSpin", "CAbility", Listed),
    (b"Attu", "CAbilityTankTurret", "CAbility", Listed),
    (b"Atwa", "CAbilityTornadoWander", "CAbilityWander", Listed),
    (b"Auco", "CAbilityUnstableConcoction", "CAbilitySimpleSpell", Listed),
    (b"Ault", "CAbilityUltraVision", "CAbilityNightVision", Query),
    (b"Auns", "CAbilityUnsummon", "CAbilitySimpleSpell", Listed),
    (b"Avng", "CAbilityVengeance", "CAbilityCarrionScarabs", Listed),
    (b"Awan", "CAbilityWander", "CAbility", Listed),
    (b"Awha", "CAbilityWispHarvest", "CAbilityHarvestBase", Listed),
    (b"Awrp", "CAbilityWarp", "CAbility", Listed),
    (b"Andt", "CAbilityNeutralDetectAoe", "CAbilityNeutralSpell", Listed),
    (b"Aneu", "CAbilityNeutral", "CAbility", Listed),
    (b"Aoar", "CAbilityAuraRegenLife", "CAbilityAura", Listed),
];

/// `(order id, ability)`: the cast getters (slot `0x348`) of `docs/research/ability-orders/getters.tsv`. `Aps2`
/// shares the id of `Apos`, so only the first class is registered (a unit with `Aps2` alone is handled through the
/// alias row, see [`Game::set_spell_alias`]). `Amil` / `Amic` have no getter value in the table (their order
/// is issued by the host or the AI directly through [`crate::cast::begin_cast`]).
pub const SPELL_ORDERS: [(u32, u32); 4] = [
    (852_196, POSSESSION),
    (852_483, SPELL_STEAL),
    (852_499, SPIRIT_LINK),
    (852_480, MAGIC_LEASH),
];

/// Cast order ids of the codes that are only listed (cast getters, slot `0x348`).
pub const ORDER_IDS_LISTED: &[(&[u8; 4], u32)] = &[
    (b"Ambt", 852_157),
    (b"Amel", 852_050),
    (b"Amed", 852_054),
    (b"Amfl", 852_512),
    (b"Amec", 852_564),
    (b"Apsh", 852_514),
    (b"Aren", 852_161),
    (b"Arep", 852_024),
    (b"Arpb", 852_542),
    (b"Arpl", 852_545),
    (b"Arpm", 852_548),
    (b"Arst", 852_202),
    (b"Aret", 852_471),
    (b"Arng", 852_241),
    (b"Asds", 852_040),
    (b"Ashs", 852_570),
    (b"Astd", 852_113),
    (b"Aste", 852_574),
    (b"Atau", 852_520),
    (b"Atlp", 852_080),
    (b"Atpi", 852_081),
    (b"Auco", 852_500),
    (b"Auns", 852_210),
    (b"Avng", 852_521),
];

/// Spirit Link groups.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Link group of each linked unit (the group id is a counter).
    link_of: BTreeMap<ObjKey, u32>,
    groups: BTreeMap<u32, Vec<ObjKey>>,
    next_group: u32,
    /// `(unit, hit target)` of units whose life dropped to zero through a link share and still need their death
    /// dispatch (the host drains it).
    pub pending_deaths: Vec<(ObjKey, ObjKey)>,
}

pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (id, base) in SPELL_ORDERS {
        ids.insert(id, base);
    }
}

/// Abilities cast without a target (Call to Arms).
pub(crate) fn takes_no_target(base: u32) -> bool {
    base == MILITIA || base == MILITIA_CONVERT
}

pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    let bad = |w: &str| gm.unit(target).removed || !targs_ok(gm, caster, target, targs_word(w));
    match base {
        // targs (LIKELY, as Charm without the level rule): air,ground,nonhero,enemy,neutral,organic.
        POSSESSION | POSSESSION_TWO => bad("air,ground,nonhero,enemy,neutral,organic").then_some(CastError::BadTarget),
        SPELL_STEAL => bad("air,ground,enemy,friend,neutral,organic").then_some(CastError::BadTarget),
        SPIRIT_LINK => bad("air,ground,friend,self,organic").then_some(CastError::BadTarget),
        MAGIC_LEASH => {
            if bad("air,ground,enemy,neutral,nonhero") || !gm.unit(target).summoned {
                return Some(CastError::BadTarget);
            }
            None
        }
        _ => None,
    }
}

pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        POSSESSION | POSSESSION_TWO | MAGIC_LEASH => {
            if let CastTarget::Unit(t) = rec.target {
                take_over(game, rt, rec, t, rec.base != MAGIC_LEASH);
            }
            true
        }
        SPELL_STEAL => {
            if let CastTarget::Unit(t) = rec.target {
                spell_steal(game, rec, t);
            }
            true
        }
        SPIRIT_LINK => {
            if let CastTarget::Unit(t) = rec.target {
                spirit_link(game, rec, t);
            }
            true
        }
        MILITIA | MILITIA_CONVERT => {
            call_to_arms(game, rec);
            true
        }
        _ => false,
    }
}

/// Possession and Magic Leash (both `CAbilitySimpleSpell`): the target changes owner to the caster's owner for good
/// and its orders are cleared (the Charm path); Possession consumes the caster (LIKELY, the Banshee is removed).
fn take_over(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey, consume: bool) {
    let changed = {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        let to = gm.unit(rec.caster).owner;
        let from = crate::natives::unit::change_owner(&mut gm, target, to);
        if from.is_some() {
            let stop = crate::orders::Order {
                id: crate::orders::ORDER_STOP,
                class: crate::orders::OrderClass::Immediate,
                point: None,
                target: 0,
            };
            let _ = gm.issue_order(target, stop, true);
        }
        if consume && from.is_some() {
            crate::natives::unit::remove_unit(&mut gm, rec.caster);
        }
        from.map(|f| (f, to))
    };
    if let Some((from, to)) = changed {
        crate::natives::event::fire_owner_change(game, rt, target, from, to);
    }
}

/// Spell Steal (`CAbilitySpellSteal < CAbilityAutoTargetSpell`): the first dispellable buff of the polarity the
/// relation asks for (positive on an enemy, negative on a friend) moves to the caster as a marker. UNSURE: the
/// original moves the buff to a friendly unit the player picks afterwards, and copies its effects.
fn spell_steal(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) || gm.unit(target).magic_immune > 0 {
        return;
    }
    let enemy = gm.is_enemy(gm.unit(rec.caster).owner, gm.unit(target).owner);
    let pick = gm
        .unit(target)
        .buffs
        .iter()
        .find(|b| b.predicates.dispellable && if enemy { b.predicates.positive } else { b.predicates.negative })
        .map(|b| (b.base, b.predicates));
    let Some((base, preds)) = pick else {
        return;
    };
    gm.remove_buff_code(target, base);
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let dur = if arith::lt(F32::ZERO, dur) { dur } else { F32::from_bits(20.0f32.to_bits()) };
    gm.apply_mods_buff(rec.caster, base, preds, UnitMods::default(), dur);
}

/// Spirit Link (`CAbilitySpiritLink`, buff `Bspl`): the target and up to `DataA - 1` other friendly units within
/// `Area` of it are linked; the buff lasts `Dur` (UNSURE: 120 s when the level has none). The damage share is
/// [`Game::m_link_share`].
fn spirit_link(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let max = softfloat::conv::f2i(rec.level.data[0]).max(1) as usize;
    let (cx, cy) = (gm.unit(target).x, gm.unit(target).y);
    let word = targs_word("air,ground,friend,organic");
    let mut members = vec![target];
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        if members.len() >= max {
            break;
        }
        if k == target || members.contains(&k) || !gm.alive(k) || !targs_ok(&gm, rec.caster, k, word) {
            continue;
        }
        if gm.is_enemy(gm.unit(rec.caster).owner, gm.unit(k).owner) {
            continue;
        }
        members.push(k);
    }
    let hero = gm.target_hero(target);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let dur = if arith::lt(F32::ZERO, dur) { dur } else { F32::from_bits(120.0f32.to_bits()) };
    let preds = BuffPredicates { positive: true, magic: true, dispellable: true, ..buff_predicates(BUFF_SPIRIT_LINK) };
    gm.spells.m.next_group += 1;
    let id = gm.spells.m.next_group;
    for &k in &members {
        gm.apply_mods_buff(k, BUFF_SPIRIT_LINK, preds, UnitMods::default(), dur);
        gm.spells.m.link_of.insert(k, id);
    }
    gm.spells.m.groups.insert(id, members);
}

/// Call to Arms (`CAbilityMilitia`, `CAbilityMilitiaConvert`, both `CAbilityCallToArms`): friendly units within `Area`
/// of the caster that carry `Amic` (the caster itself for `Amic`) become the militia type (`UnitID`, default `hmil`)
/// for `Dur`; the buff `Bmil` holds the morph and its end restores the peasant (LIKELY structure, UNSURE numbers).
fn call_to_arms(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let militia = if rec.level.unit_id != 0 { rec.level.unit_id } else { MILITIA_TYPE };
    let dur = if arith::lt(F32::ZERO, rec.level.dur) {
        rec.level.dur
    } else {
        F32::from_bits(MILITIA_DEFAULT_SECONDS.to_bits())
    };
    let mut who: Vec<ObjKey> = Vec::new();
    if rec.base == MILITIA_CONVERT {
        who.push(rec.caster);
    } else {
        let (cx, cy) = (gm.unit(rec.caster).x, gm.unit(rec.caster).y);
        let owner = gm.unit(rec.caster).owner;
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
            let u = gm.unit(k);
            if u.owner == owner && !u.dead && !u.removed && u.abilities.contains(MILITIA_CONVERT) {
                who.push(k);
            }
        }
    }
    let preds = BuffPredicates { positive: true, ..buff_predicates(BUFF_MILITIA) };
    for k in who {
        if !gm.alive(k) || gm.is_polymorphed(k) {
            continue;
        }
        gm.morph_unit(k, militia);
        gm.apply_mods_buff(k, BUFF_MILITIA, preds, UnitMods { morph: true, ..UnitMods::default() }, dur);
    }
}

impl Game {
    /// Poison Sting / Poison Damage on-hit (the `Aven` path of `neutral_spells`): `DataA` damage per second, `Dur` /
    /// `HeroDur`. UNSURE: the stacking variant (`DataD`) is not modelled.
    pub(crate) fn m_on_hit(&mut self, attacker: ObjKey, target: ObjKey) {
        if !self.alive(target) {
            return;
        }
        for base in [POISON_STING, POISON_DAMAGE] {
            if self.unit(attacker).abilities.get(base).is_none_or(|a| a.is_removed()) {
                continue;
            }
            let Some(level) = self.ability_level_record(attacker, base) else {
                continue;
            };
            if !targs_ok(self, attacker, target, targs_word("air,ground,organic")) {
                continue;
            }
            let hero = self.target_hero(target);
            let dur = if hero { level.hero_dur } else { level.dur };
            if !arith::lt(F32::ZERO, dur) {
                continue;
            }
            let preds = BuffPredicates { negative: true, physical: true, ..buff_predicates(BUFF_POISON) };
            let serial = self.apply_mods_buff(target, BUFF_POISON, preds, UnitMods::default(), dur);
            self.register_tick(target, serial, attacker, level.data[0], false, POISON_DAMAGE_TYPE, true);
        }
    }

    /// The damage share of Spirit Link: a linked unit that takes `amount` takes `amount / n` itself and every other
    /// live linked unit loses the same share (`n` = live members). Returns the amount for `target`. LIKELY. The
    /// others are set through `set_life`; a unit that dies of its share is recorded in
    /// [`State::pending_deaths`] (UNSURE: the death dispatch of those is left to the host).
    pub(crate) fn m_link_share(&mut self, target: ObjKey, amount: F32) -> F32 {
        let Some(&g) = self.spells.m.link_of.get(&target) else {
            return amount;
        };
        if self.buff_position(target, BUFF_SPIRIT_LINK).is_none() {
            self.spells.m.link_of.remove(&target);
            return amount;
        }
        let members: Vec<ObjKey> = self
            .spells
            .m
            .groups
            .get(&g)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|&k| self.alive(k) && self.buff_position(k, BUFF_SPIRIT_LINK).is_some())
            .collect();
        if members.len() < 2 || !members.contains(&target) {
            return amount;
        }
        let n = F32::from_bits((members.len() as f32).to_bits());
        let share = self.soft_div(amount, n);
        for k in members {
            if k == target {
                continue;
            }
            let next = arith::sub(self.unit(k).life, share);
            if crate::natives::set_life(self, k, next) {
                self.spells.m.pending_deaths.push((k, target));
            }
        }
        share
    }

    /// Takes the units that died of a link share since the last call, with the unit whose hit caused it.
    pub fn m_take_link_deaths(&mut self) -> Vec<(ObjKey, ObjKey)> {
        std::mem::take(&mut self.spells.m.pending_deaths)
    }

    /// Permanent Invisibility (`Apiv`, `CAbilityPermInvis < CBonusBase`): the unit is always invisible. Query for the
    /// invisibility system (S44).
    pub fn m_perm_invisible(&self, unit: ObjKey) -> bool {
        self.unit(unit).abilities.get(PERM_INVIS).is_some_and(|a| !a.is_removed())
    }

    /// Shadow Meld (`Ashm`): the unit can hide at night while it stands still. `still` is supplied by the caller (the
    /// order system's idle test is not wired here). Query for S44.
    pub fn m_shadow_meld_active(&self, unit: ObjKey, still: bool) -> bool {
        still && self.is_night() && self.unit(unit).abilities.get(SHADOW_MELD).is_some_and(|a| !a.is_removed())
    }

    /// True Sight (`Atru`, `CAbilityTrueSight < CAbilityDetector`): whether `detector` reveals invisible `other`
    /// within the level's `Area` (UNSURE: the radius field is `Area`).
    pub fn m_true_sight_reveals(&self, detector: ObjKey, other: ObjKey) -> bool {
        let Some(level) = self.ability_level_record(detector, TRUE_SIGHT) else {
            return false;
        };
        let (a, b) = (self.unit(detector), self.unit(other));
        let dx = arith::sub(b.x, a.x);
        let dy = arith::sub(b.y, a.y);
        let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
        !arith::lt(arith::mul(level.area, level.area), d2)
    }
}
