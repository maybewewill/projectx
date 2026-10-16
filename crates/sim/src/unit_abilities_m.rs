//! Unit abilities, batch M (S47). Spec: `docs/spec/abilities-unit-m.md`.
//!
//! The batch covers the 79 rawcodes of [`CODES`]. Every code is listed with its RTTI class and parent (atlas
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
use crate::cast::{effect_duration, CastError, CastRec, CastTarget, SpellMsg};
use crate::natives::game::Payload;
use crate::timer::TimerId;
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
/// S52: the `CAbilityMorph` transforms of this batch. Their order ids (primary / alternate getters) are in
/// `unit_abilities_a::MORPH_ORDERS`, the morph machinery is batch A's.
pub const RAVEN_FORM: u32 = code(b"Arav");
pub const STONE_FORM: u32 = code(b"Astn");
pub const MOUNT: u32 = code(b"Amou");
pub const PHOENIX_MORPH: u32 = code(b"Aphx");
pub const REPAIR: u32 = code(b"Arep");
pub const RENEW: u32 = code(b"Aren");
pub const RESTORATION: u32 = code(b"Arst");
pub const RETURN: u32 = code(b"Artn");
pub const REPLENISH: u32 = code(b"Arpb");
pub const REPLENISH_LIFE: u32 = code(b"Arpl");
pub const REPLENISH_MANA: u32 = code(b"Arpm");
pub const UNSUMMON: u32 = code(b"Auns");
pub const TAUNT: u32 = code(b"Atau");
pub const PANDA_IMMOLATION: u32 = code(b"Apig");
pub const AURA_REGEN_LIFE: u32 = code(b"Aoar");
/// The aura buff code of `Aoar`: the code getter of `CBuffAuraRegenLife` (`0x6F022D40`) returns this, CONFIRMED (S52).
pub const BUFF_REGEN_LIFE: u32 = code(b"Boar");
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
    (b"Amou", "CAbilityMount", "CAbilityMorph", Modelled),
    (b"Amtc", "CAbilityMeatCargo", "CAbilityCargoHold", Listed),
    (b"Aphx", "CAbilityPhoenix", "CAbilityMorph", Modelled),
    (b"Apig", "CAbilityPandaImmolation", "CAbilityPermImmolation", Modelled),
    (b"Apit", "CAbilityPurchaseItem", "CAbility", Listed),
    (b"Apiv", "CAbilityPermInvis", "CBonusBase", Query),
    (b"Apo2", "CAbilityPoisonDamage", "CAbilityPoisonAttack", Modelled),
    (b"Apoi", "CAbilityPoisonAttack", "CAbilityPassive", Modelled),
    (b"Apos", "CAbilityPossession", "CAbilitySimpleSpell", Modelled),
    (b"Aps2", "CAbilityPossessionTwo", "CAbilityPossession", Modelled),
    (b"Apsh", "CAbilityPhaseShift", "CAbilityModalSpell", Listed),
    (b"Apts", "CAbilityPlagueToss", "CAbilityPassive", Listed),
    (b"Apxf", "CAbilityPhoenixFire", "CAbilityPassive", Listed),
    (b"Arav", "CAbilityRavenForm", "CAbilityMorph", Modelled),
    (b"Arbr", "CAbilityReinforce", "CAbilitySpiked", Listed),
    (b"Arem", "CAbilityRegenMana", "CAbilityRegenBase", Listed),
    (b"Aren", "CAbilityRenew", "CAbilityRepair", Modelled),
    (b"Arep", "CAbilityRepair", "CAbilityAutoTargetSpell", Modelled),
    (b"Aret", "CAbilityRetrain", "CAbilitySpell", Listed),
    (b"Arev", "CAbilityRevive", "CAbilityBuildQueue", Listed),
    (b"Arng", "CAbilityRevenge", "CAbilitySpell", Listed),
    (b"Aroc", "CAbilityBarrage", "CAbilityPassive", Listed),
    (b"Aroo", "CAbilityRoot", "CAbilityButton", Listed),
    (b"Arpb", "CAbilityReplenish", "CAbilityAutoTargetSpell", Modelled),
    (b"Arpl", "CAbilityReplenishLife", "CAbilityReplenish", Modelled),
    (b"Arpm", "CAbilityReplenishMana", "CAbilityReplenish", Modelled),
    (b"Arst", "CAbilityRestoration", "CAbilityRepair", Modelled),
    (b"Artn", "CAbilityReturn", "CAbility", Modelled),
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
    (b"Astn", "CAbilityStoneForm", "CAbilityMorph", Modelled),
    (b"Atau", "CAbilityTaunt", "CAbilitySimpleSpell", Modelled),
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
    (b"Auns", "CAbilityUnsummon", "CAbilitySimpleSpell", Modelled),
    (b"Avng", "CAbilityVengeance", "CAbilityCarrionScarabs", Listed),
    (b"Awan", "CAbilityWander", "CAbility", Listed),
    (b"Awha", "CAbilityWispHarvest", "CAbilityHarvestBase", Listed),
    (b"Awrp", "CAbilityWarp", "CAbility", Listed),
    (b"Andt", "CAbilityNeutralDetectAoe", "CAbilityNeutralSpell", Listed),
    (b"Aneu", "CAbilityNeutral", "CAbility", Listed),
    (b"Aoar", "CAbilityAuraRegenLife", "CAbilityAura", Modelled),
];

/// `(order id, ability)`: the cast getters (slot `0x348`) of `docs/research/ability-orders/getters.tsv`. `Aps2`
/// shares the id of `Apos`, so only the first class is registered (a unit with `Aps2` alone is handled through the
/// alias row, see [`Game::set_spell_alias`]). `Amil` / `Amic` have no getter value in the table (their order
/// is issued by the host or the AI directly through [`crate::cast::begin_cast`]).
pub const SPELL_ORDERS: [(u32, u32); 13] = [
    (852_196, POSSESSION),
    (852_483, SPELL_STEAL),
    (852_499, SPIRIT_LINK),
    (852_480, MAGIC_LEASH),
    // S52. Cast getters (slot 0x348) of the repair and replenish classes; 852020 is `returnresources` from the
    // original's order-name initializer (`orders/names.rs`), `Artn` has no cast getter.
    (852_024, REPAIR),
    (852_161, RENEW),
    (852_202, RESTORATION),
    (852_020, RETURN),
    (852_542, REPLENISH),
    (852_545, REPLENISH_LIFE),
    (852_548, REPLENISH_MANA),
    (852_210, UNSUMMON),
    (852_520, TAUNT),
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
    /// Share fraction (`DataA`) of each group: the part of a hit the struck unit keeps.
    frac: BTreeMap<u32, F32>,
    next_group: u32,
    /// `(unit, hit target)` of units whose life dropped to zero through a link share and still need their death
    /// dispatch (the host drains it).
    pub pending_deaths: Vec<(ObjKey, ObjKey)>,
    /// Running repairs by builder (S52).
    repairs: BTreeMap<ObjKey, RepairJob>,
    /// Running replenish channels by caster (S52).
    replenish: BTreeMap<ObjKey, ReplenishJob>,
    /// Running permanent-immolation pulses by (unit, ability) (S52).
    pulses: BTreeMap<(ObjKey, u32), TimerId>,
}

pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (id, base) in SPELL_ORDERS {
        ids.insert(id, base);
    }
}

/// Abilities cast without a target (Call to Arms).
pub(crate) fn takes_no_target(base: u32) -> bool {
    matches!(base, MILITIA | MILITIA_CONVERT | RETURN | TAUNT)
}

/// Classes whose `Cast` field is the length of the effect (a channel of `Cast / Dur` ticks), not a delay before it:
/// the replenish family (UNSURE, see the spec). `cast.rs` skips its stage-2 wait for them.
pub(crate) fn cast_field_is_duration(base: u32) -> bool {
    matches!(base, REPLENISH | REPLENISH_LIFE | REPLENISH_MANA)
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
        REPAIR | RENEW | RESTORATION => {
            // targs of the level rows (`AbilityData.slk`): the repair rows add `mechanical,nonancient`, the renew row
            // has neither. The target must be hurt.
            let w = if base == RENEW {
                "friend,ground,air,structure,bridge,alive,dead,invu,vuln"
            } else {
                "mechanical,friend,nonancient,ground,air,structure,bridge,alive,dead,invu,vuln"
            };
            let u = gm.unit(target);
            (bad(w) || !arith::lt(u.life, u.max_life)).then_some(CastError::BadTarget)
        }
        REPLENISH | REPLENISH_LIFE | REPLENISH_MANA => {
            let u = gm.unit(target);
            let hurt = arith::lt(u.life, u.max_life);
            let dry = arith::lt(u.mana, u.max_mana);
            let needs = match base {
                REPLENISH_LIFE => hurt,
                REPLENISH_MANA => dry,
                _ => hurt || dry,
            };
            (bad("ground,air,friend,self,organic,vuln,invu") || !needs).then_some(CastError::BadTarget)
        }
        UNSUMMON => bad("structure,player").then_some(CastError::BadTarget),
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
        REPAIR | RENEW | RESTORATION => {
            if let CastTarget::Unit(t) = rec.target {
                start_repair(game, rec, t);
            }
            true
        }
        RETURN => {
            game.borrow_mut().force_return(rec.caster);
            true
        }
        REPLENISH | REPLENISH_LIFE | REPLENISH_MANA => {
            if let CastTarget::Unit(t) = rec.target {
                start_replenish(game, rec, t);
            }
            true
        }
        UNSUMMON => {
            if let CastTarget::Unit(t) = rec.target {
                unsummon(game, rec, t);
            }
            true
        }
        TAUNT => {
            taunt(game, rec);
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

/// Spirit Link (`CAbilitySpiritLink`, buff `Bspl`): the chain of `F2I(DataB)` units (the target first, then each time the
/// nearest eligible friendly unit within `Area` of the LAST unit chosen; CONFIRMED roles: `DataB` is the count and `DataA` the
/// fraction a struck unit keeps, `0x6F1FFA00`; the nearest-pick details of `0x6F1DEDC0` are UNSURE); the buff lasts `Dur` /
/// `HeroDur` (120 s here when the level has none, UNSURE). The damage share is [`Game::m_link_share`].
fn spirit_link(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let max = softfloat::conv::f2i(rec.level.data[1]).max(1) as usize;
    let word = targs_word("air,ground,friend,organic");
    let mut members = vec![target];
    while members.len() < max {
        let last = *members.last().expect("non-empty");
        let (cx, cy) = (gm.unit(last).x, gm.unit(last).y);
        let mut best: Option<(F32, ObjKey)> = None;
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
            if members.contains(&k) || !gm.alive(k) || !targs_ok(&gm, rec.caster, k, word) {
                continue;
            }
            if gm.is_enemy(gm.unit(rec.caster).owner, gm.unit(k).owner) {
                continue;
            }
            let d = arith::dist2(gm.unit(k).x, gm.unit(k).y, cx, cy);
            if best.is_none_or(|(bd, _)| arith::lt(d, bd)) {
                best = Some((d, k));
            }
        }
        match best {
            Some((_, k)) => members.push(k),
            None => break,
        }
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
    gm.spells.m.frac.insert(id, rec.level.data[0]);
}

/// The Spirit Link split of one hit (CONFIRMED, Unicorn 400 cases, `tools/emu/v2_spiritlink_emu.py`: handler `0x6F1E9760` and
/// the share routine `0x6F1EFED0`): the struck unit's record is scaled to `own = amount * fraction`; the rest
/// `shared = amount - own` (soft sub) is divided by the number `n` of linked units (`div(shared, I2R(n))`) and EACH linked unit,
/// the struck one included, takes that as a separate record of class `0x1000000`. Nothing is shared when `shared` is zero.
/// Returns `(own, per-unit share)`.
pub fn spirit_link_split(amount: F32, fraction: F32, n: usize, div: impl Fn(F32, F32) -> F32) -> (F32, Option<F32>) {
    let own = arith::mul(amount, fraction);
    let shared = arith::sub(amount, own);
    if n == 0 || arith::eq(shared, F32::ZERO) {
        return (own, None);
    }
    (own, Some(div(shared, softfloat::conv::i2r(n as i32))))
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

    /// The damage share of Spirit Link ([`spirit_link_split`], CONFIRMED arithmetic): the struck unit keeps `amount * DataA`
    /// of the hit and every live linked unit (itself included) takes `(amount - own) / n` as well. Returns the amount to
    /// apply to `target`: `own` plus its own part of the share (the original delivers them as two records; the sum is the
    /// model's, UNSURE by at most an ulp). The others are set through `set_life`; a unit that dies of its share is
    /// recorded in [`State::pending_deaths`] (the death dispatch of those is left to the host).
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
        if members.is_empty() || !members.contains(&target) || arith::eq(amount, F32::ZERO) {
            return amount;
        }
        let frac = self.spells.m.frac.get(&g).copied().unwrap_or(F32::ZERO);
        let (own, per) = spirit_link_split(amount, frac, members.len(), |a, b| self.soft_div(a, b));
        let Some(per) = per else {
            return own;
        };
        for k in members {
            if k == target {
                continue;
            }
            let next = arith::sub(self.unit(k).life, per);
            if crate::natives::set_life(self, k, next) {
                self.spells.m.pending_deaths.push((k, target));
            }
        }
        arith::add(own, per)
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
        // S50: the data row carries the radius in `Rng` (900); `Area` is 0.
        let radius = if arith::lt(F32::ZERO, level.area) { level.area } else { level.rng };
        !arith::lt(arith::mul(radius, radius), d2)
    }
}

// ---- S52: timers, repair, replenish, unsummon, taunt ----

/// Timer kinds of this module (`SpellMsg::UnitSpell`, routed here by `unit_abilities_a::on_timer`).
pub const KIND_REPAIR: u8 = 0x70;
pub const KIND_REPLENISH: u8 = 0x71;
pub const KIND_PULSE: u8 = 0x72;
/// Seconds between two repair ticks (a model choice, UNSURE: the original's repair progress is a per-tick task of the
/// builder that was not read).
pub const REPAIR_PERIOD: f32 = 0.5;

pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, base: u32, _aux: u32) {
    match kind {
        KIND_REPAIR => game.borrow_mut().repair_tick(unit),
        KIND_REPLENISH => game.borrow_mut().replenish_tick(unit),
        KIND_PULSE => {
            let dead = game.borrow_mut().pulse_tick(unit, base);
            for victim in dead {
                crate::natives::dispatch_unit_death_by(game, rt, victim, Some(unit));
            }
        }
        _ => {}
    }
}

impl Game {
    /// Starts and stops the passive pulses of this module as `unit`'s abilities change (called from
    /// `sync_unit_abilities_a`, so at creation and whenever abilities are added or removed).
    pub(crate) fn sync_unit_abilities_m(&mut self, unit: ObjKey) {
        let has = self.unit(unit).abilities.get(PANDA_IMMOLATION).is_some_and(|a| !a.is_removed());
        let running = self.spells.m.pulses.contains_key(&(unit, PANDA_IMMOLATION));
        if has && !running {
            let level = self.spell_level(unit, PANDA_IMMOLATION);
            let period = if arith::lt(F32::ZERO, level.dur) { level.dur.to_f32() } else { 1.0 };
            let id = self.timers.start(
                period,
                true,
                Payload::Spell(SpellMsg::UnitSpell { kind: KIND_PULSE, unit, base: PANDA_IMMOLATION, aux: 0 }),
            );
            self.spells.m.pulses.insert((unit, PANDA_IMMOLATION), id);
        } else if !has && running {
            if let Some(id) = self.spells.m.pulses.remove(&(unit, PANDA_IMMOLATION)) {
                self.timers.cancel(id);
            }
        }
    }

    /// Panda Immolation (`CAbilityPandaImmolation < CAbilityPermImmolation`, always on): every `Dur` seconds the enemies
    /// within `Area` of the unit (targs `ground,enemy,neutral,organic`) take `DataA` damage. LIKELY (the editor meaning
    /// of the fields, as the toggled Immolation of `undead_ne.rs`); UNSURE: the damage type (the spell row 9 as the
    /// toggle) and that the buff `Bpig` marks the victims (not modelled). Returns the victims that died.
    fn pulse_tick(&mut self, unit: ObjKey, base: u32) -> Vec<ObjKey> {
        let has = self.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed());
        if self.unit(unit).removed || self.unit(unit).dead || !has {
            if let Some(id) = self.spells.m.pulses.remove(&(unit, base)) {
                self.timers.cancel(id);
            }
            return Vec::new();
        }
        let level = self.spell_level(unit, base);
        let (x, y) = (self.unit(unit).x, self.unit(unit).y);
        let word = targs_word("ground,enemy,neutral,organic");
        let mut dead = Vec::new();
        for k in crate::natives::group::circle_candidates(self, x, y, level.area) {
            if k == unit || !self.alive(k) || !targs_ok(self, unit, k, word) || self.unit_is_invulnerable(k) {
                continue;
            }
            if let Some(d) = self.unit_damage_target(unit, k, level.data[0], false, false, 0, 9) {
                dead.push(d);
            }
        }
        dead
    }
}

/// A running repair: the builder is the map key.
#[derive(Debug, Clone)]
pub struct RepairJob {
    pub target: ObjKey,
    /// `DataA`: the share of the target's cost a full repair costs (editor default 0.35).
    ratio_cost: F32,
    /// `DataB`: the share of the build time a full repair takes (editor default 1.5).
    ratio_time: F32,
    /// Life restored so far.
    repaired: F32,
    /// Gold and lumber (x10) already paid.
    paid: [i32; 2],
    /// Where the builder stood when it started (it leaves the job by moving).
    at: (F32, F32),
    timer: TimerId,
}

/// A running replenish channel (the Moon Well).
#[derive(Debug, Clone)]
pub struct ReplenishJob {
    pub target: ObjKey,
    life: F32,
    mana: F32,
    ticks_left: u32,
    period: f32,
    timer: TimerId,
}

fn nonzero(x: F32) -> bool {
    x.to_bits() & 0x7FFF_FFFF != 0
}

/// Repair, Renew, Restoration (`CAbilityRepair < CAbilityAutoTargetSpell`, `CAbilityRenew` and `CAbilityRestoration <
/// CAbilityRepair`): the builder restores the target's life at `max life / (bldtm * DataB)` per second and pays
/// `DataA * cost * repaired / max life` of gold and lumber as it goes; it stops when the target is whole, when the
/// owner cannot pay, or when the builder moves. LIKELY structure (the editor meaning of `DataA` / `DataB`, the
/// 0.35 / 1.5 defaults of `AbilityData.slk`), UNSURE: the 0.5 s tick, paying as it goes, `bldtm` as the base time
/// (there is no repair-time column in `UnitBalance.slk`), and `DataC` / `DataD` (power build) are not modelled.
fn start_repair(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    if let Some(old) = gm.spells.m.repairs.remove(&rec.caster) {
        gm.timers.cancel(old.timer);
    }
    let timer = gm.timers.start(
        REPAIR_PERIOD,
        false,
        Payload::Spell(SpellMsg::UnitSpell { kind: KIND_REPAIR, unit: rec.caster, base: rec.base, aux: 0 }),
    );
    let at = (gm.unit(rec.caster).x, gm.unit(rec.caster).y);
    gm.spells.m.repairs.insert(
        rec.caster,
        RepairJob { target, ratio_cost: rec.level.data[0], ratio_time: rec.level.data[1], repaired: F32::ZERO, paid: [0, 0], at, timer },
    );
}

impl Game {
    /// The target `builder` is repairing, if any.
    pub fn m_repairing(&self, builder: ObjKey) -> Option<ObjKey> {
        self.spells.m.repairs.get(&builder).map(|j| j.target)
    }

    fn end_repair(&mut self, builder: ObjKey) {
        if let Some(j) = self.spells.m.repairs.remove(&builder) {
            self.timers.cancel(j.timer);
        }
    }

    fn repair_tick(&mut self, builder: ObjKey) {
        let Some(job) = self.spells.m.repairs.get(&builder).cloned() else {
            return;
        };
        let (target, mut paid) = (job.target, job.paid);
        let moved = {
            let b = self.unit(builder);
            b.x.to_bits() != job.at.0.to_bits() || b.y.to_bits() != job.at.1.to_bits()
        };
        if !self.alive(builder) || !self.alive(target) || moved || self.is_stunned(builder) {
            self.end_repair(builder);
            return;
        }
        let (type_id, owner) = (self.unit(target).type_id, self.unit(target).owner);
        let bldtm = self.production().build_seconds.get(&type_id).copied().unwrap_or(0);
        let (life, max) = (self.unit(target).life, self.unit(target).max_life);
        let deficit = arith::sub(max, life);
        if bldtm <= 0 || !nonzero(max) || !arith::lt(F32::ZERO, deficit) {
            self.end_repair(builder);
            return;
        }
        // Life per tick: max life * period / (bldtm * time ratio).
        let period = F32::from_bits(REPAIR_PERIOD.to_bits());
        let secs = arith::mul(softfloat::conv::i2r(bldtm), job.ratio_time);
        if !arith::lt(F32::ZERO, secs) {
            self.end_repair(builder);
            return;
        }
        let mut gain = self.soft_div(arith::mul(max, period), secs);
        if arith::lt(deficit, gain) {
            gain = deficit;
        }
        let repaired = arith::add(job.repaired, gain);
        // What has been earned in resources so far, x10, and what is still to pay.
        let ty = self.unit_types().get(type_id).map(|t| (t.gold_cost, t.wood_cost)).unwrap_or((0, 0));
        let frac = self.soft_div(repaired, max);
        let mut due = [0i32; 2];
        for (i, cost) in [ty.0, ty.1].into_iter().enumerate() {
            let total = softfloat::conv::f2i(arith::mul(softfloat::conv::i2r(cost.saturating_mul(10)), job.ratio_cost));
            due[i] = softfloat::conv::f2i(arith::mul(softfloat::conv::i2r(total), frac)).max(paid[i]);
        }
        let need = [due[0] - paid[0], due[1] - paid[1]];
        let affordable = self.economy.get(owner as usize).is_some_and(|p| {
            p.stored(crate::economy::Res::Gold) >= need[0] && p.stored(crate::economy::Res::Lumber) >= need[1]
        });
        if !affordable {
            // The owner cannot pay: the repair stops (the original raises an error and the builder idles, UNSURE).
            self.end_repair(builder);
            return;
        }
        for (i, res) in [crate::economy::Res::Gold, crate::economy::Res::Lumber].into_iter().enumerate() {
            if need[i] > 0 {
                crate::economy::change(&mut self.economy, owner as usize, res, -need[i], crate::economy::Flags::NONE);
                paid[i] = due[i];
            }
        }
        let next = arith::add(life, gain);
        crate::natives::set_life(self, target, if arith::lt(max, next) { max } else { next });
        let whole = !arith::lt(self.unit(target).life, max);
        if whole {
            self.end_repair(builder);
            return;
        }
        let timer = self.timers.start(
            REPAIR_PERIOD,
            false,
            Payload::Spell(SpellMsg::UnitSpell { kind: KIND_REPAIR, unit: builder, base: REPAIR, aux: 0 }),
        );
        if let Some(j) = self.spells.m.repairs.get_mut(&builder) {
            j.repaired = repaired;
            j.paid = paid;
            j.timer = timer;
        }
    }
}

/// Replenish Mana and Life / Life / Mana (`CAbilityReplenish`, `...Life`, `...Mana`; the Moon Well). UNSURE model: the
/// `Cast` field is the channel length (`cast_field_is_duration`), `Dur` the tick period, and every tick gives the
/// target `DataA` life (Arpb, Arpl) and `DataB` mana (Arpb, Arpm); the generic cast paid `Cost` mana once for the
/// whole channel. The first tick is immediate.
fn start_replenish(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    if let Some(old) = gm.spells.m.replenish.remove(&rec.caster) {
        gm.timers.cancel(old.timer);
    }
    let l = &rec.level;
    let life = if rec.base == REPLENISH_MANA { F32::ZERO } else { l.data[0] };
    let mana = if rec.base == REPLENISH_LIFE { F32::ZERO } else { l.data[1] };
    let period = if arith::lt(F32::ZERO, l.dur) { l.dur.to_f32() } else { 1.0 };
    let ticks = if arith::lt(F32::ZERO, l.cast) { (l.cast.to_f32() / period).floor().max(1.0) as u32 } else { 1 };
    let timer = gm.timers.start(
        period,
        false,
        Payload::Spell(SpellMsg::UnitSpell { kind: KIND_REPLENISH, unit: rec.caster, base: rec.base, aux: 0 }),
    );
    gm.spells.m.replenish.insert(rec.caster, ReplenishJob { target, life, mana, ticks_left: ticks, period, timer });
    // The first tick now (the job's remaining count excludes it).
    gm.replenish_tick_now(rec.caster);
}

impl Game {
    fn replenish_tick_now(&mut self, caster: ObjKey) {
        let Some(job) = self.spells.m.replenish.get(&caster).cloned() else {
            return;
        };
        if !self.alive(caster) || !self.alive(job.target) {
            self.end_replenish(caster);
            return;
        }
        let t = job.target;
        let (life, max_life, mana, max_mana) = {
            let u = self.unit(t);
            (u.life, u.max_life, u.mana, u.max_mana)
        };
        if nonzero(job.life) {
            let sum = arith::add(life, job.life);
            crate::natives::set_life(self, t, if arith::lt(max_life, sum) { max_life } else { sum });
        }
        if nonzero(job.mana) {
            let sum = arith::add(mana, job.mana);
            self.write_mana(t, if arith::lt(max_mana, sum) { max_mana } else { sum });
        }
        if let Some(j) = self.spells.m.replenish.get_mut(&caster) {
            j.ticks_left = j.ticks_left.saturating_sub(1);
        }
    }

    fn end_replenish(&mut self, caster: ObjKey) {
        if let Some(j) = self.spells.m.replenish.remove(&caster) {
            self.timers.cancel(j.timer);
        }
    }

    fn replenish_tick(&mut self, caster: ObjKey) {
        let Some(job) = self.spells.m.replenish.get(&caster).cloned() else {
            return;
        };
        if job.ticks_left == 0 {
            self.end_replenish(caster);
            return;
        }
        self.replenish_tick_now(caster);
        let left = self.spells.m.replenish.get(&caster).map_or(0, |j| j.ticks_left);
        if left == 0 {
            self.end_replenish(caster);
            return;
        }
        let timer = self.timers.start(
            job.period,
            false,
            Payload::Spell(SpellMsg::UnitSpell { kind: KIND_REPLENISH, unit: caster, base: REPLENISH, aux: 0 }),
        );
        if let Some(j) = self.spells.m.replenish.get_mut(&caster) {
            j.timer = timer;
        }
    }

    /// The target of a running replenish channel of `caster`.
    pub fn m_replenishing(&self, caster: ObjKey) -> Option<ObjKey> {
        self.spells.m.replenish.get(&caster).map(|j| j.target)
    }
}

/// Unsummon Building (`CAbilityUnsummon < CAbilitySimpleSpell`, order 852210, targs `structure,player`): the target is
/// removed and its owner is refunded `DataA` of its gold and lumber cost (plain changer, no tax). UNSURE: what `DataA`
/// (0.5) and `DataB` (50) mean, the original was not read.
fn unsummon(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let (type_id, owner) = (gm.unit(target).type_id, gm.unit(target).owner);
    let (gold, wood) = gm.unit_types().get(type_id).map(|t| (t.gold_cost, t.wood_cost)).unwrap_or((0, 0));
    let share = |c: i32| softfloat::conv::f2i(arith::mul(softfloat::conv::i2r(c), rec.level.data[0]));
    crate::natives::unit::remove_unit(&mut gm, target);
    gm.credit_resource(owner, crate::economy::Res::Gold, share(gold), crate::economy::Flags::NONE);
    gm.credit_resource(owner, crate::economy::Res::Lumber, share(wood), crate::economy::Flags::NONE);
}

/// Taunt (`CAbilityTaunt < CAbilitySimpleSpell`, immediate order 852520, targs `air,ground,enemy,vuln,invu`): every
/// enemy within `Area` of the caster is ordered to attack it. UNSURE: the original may hold the order for `DataA` (10)
/// seconds; here the order is an ordinary attack order that later orders replace.
fn taunt(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (cx, cy) = (gm.unit(rec.caster).x, gm.unit(rec.caster).y);
    let handle = gm.handle_of(rec.caster);
    if handle == 0 {
        return;
    }
    let word = targs_word("air,ground,enemy,vuln,invu");
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        if k == rec.caster || !gm.alive(k) || !targs_ok(&gm, rec.caster, k, word) {
            continue;
        }
        let order = crate::orders::Order {
            id: crate::orders::ORDER_ATTACK,
            class: crate::orders::OrderClass::Target,
            point: None,
            target: handle,
        };
        let _ = gm.issue_order(k, order, true);
    }
}
