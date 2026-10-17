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
    matches!(base, MILITIA | MILITIA_CONVERT | RETURN | TAUNT | REPLENISH | REPLENISH_LIFE | REPLENISH_MANA)
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
            (bad(w) || gm.repair_gate(gm.unit(caster).owner, target) != 0).then_some(CastError::BadTarget)
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
            replenish_pulse(game, rec);
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
pub const KIND_PULSE: u8 = 0x72;

pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, base: u32, _aux: u32) {
    match kind {
        KIND_REPAIR => game.borrow_mut().repair_event(unit, false, false),
        KIND_REPAIR_GOLD => game.borrow_mut().repair_event(unit, true, false),
        KIND_REPAIR_LUMBER => game.borrow_mut().repair_event(unit, false, true),
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

/// Timer kinds of the repair: the one-second validation tick (message `0xD01C1`, kind [`KIND_REPAIR`]), the gold payment (`0xD01D0`) and
/// the lumber payment (`0xD01D1`).
pub const KIND_REPAIR_GOLD: u8 = 0x73;
pub const KIND_REPAIR_LUMBER: u8 = 0x74;

/// A running repair: the builder is the map key. CONFIRMED structure (V10): a life regeneration modifier on the target, a periodic
/// payment timer per resource (one unit each time) and, when both periods are at least a second, a one-second validation tick.
#[derive(Debug, Clone)]
pub struct RepairJob {
    pub target: ObjKey,
    modifier: Option<crate::agile::ModId>,
    timers: Vec<TimerId>,
    /// Where the builder stood when it started (it leaves the job by moving; UNSURE, the original re-checks the range instead).
    at: (F32, F32),
}

fn nonzero(x: F32) -> bool {
    x.to_bits() & 0x7FFF_FFFF != 0
}

fn lt87(a: F32, b: F32) -> bool {
    f64::from(a.to_f32()) < f64::from(b.to_f32())
}

/// The inputs of the repair start for a finished building (no `ABnP` ability on the target): the level fields `DataA` (cost share) and
/// `DataB` (time share), the target type's `goldRep`, `lumberRep` and `reptm` (UnitBalance), its maximum life and whether the builder's
/// owner can pay one unit of each resource (`0x6F02D730` / `0x6F02D760`).
#[derive(Debug, Clone, Copy)]
pub struct RepairStartIn {
    pub data_a: F32,
    pub data_b: F32,
    pub gold_rep: i32,
    pub lumber_rep: i32,
    pub rep_time: i32,
    pub max_life: F32,
    pub gold_ok: bool,
    pub lumber_ok: bool,
}

/// What the repair start does, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepairEvent {
    /// The can-pay test for one unit of gold / lumber.
    CanPay { gold: bool },
    /// One unit is paid (`0x6F416A00(-1, 0, 0, 0)`, the wrapper scales by 10).
    Pay { gold: bool },
    /// A periodic timer: message `0xD01D0` / `0xD01D1` (payments) or `0xD01C1` (validation tick).
    Timer { msg: u32, period: F32 },
    /// The life regeneration modifier of the target (`0x6F02DC60`, second argument `0.0`).
    Modifier { rate: F32 },
}

/// The repair start `0x6F070AB0` for a finished building (CONFIRMED by Unicorn, V10 `rep_start`): `T = max(5.0, DataB * reptm)`; the totals
/// are `DataA * goldRep` and `DataA * lumberRep`; a nonzero total needs one unit to be payable (else nothing starts); then each nonzero
/// total pays one unit at once and starts a periodic timer of `T / total` seconds; the one-second tick starts when both periods are at
/// least 1.0 (a missing period counts as 1.0); the target gets a life regeneration of `max life / T` per second.
pub fn repair_start_plan(inp: &RepairStartIn, div: &dyn Fn(F32, F32) -> F32) -> Vec<RepairEvent> {
    use softfloat::conv::i2r;
    let mut ev = Vec::new();
    let gold_total = arith::mul(inp.data_a, i2r(inp.gold_rep));
    let lumber_total = arith::mul(inp.data_a, i2r(inp.lumber_rep));
    let mut time = arith::mul(inp.data_b, i2r(inp.rep_time));
    let floor = F32::from_bits(0x40A0_0000);
    if lt87(time, floor) {
        time = floor;
    }
    let gold = lt87(F32::ZERO, gold_total);
    let lumber = lt87(F32::ZERO, lumber_total);
    if gold {
        ev.push(RepairEvent::CanPay { gold: true });
        if !inp.gold_ok {
            return ev;
        }
    }
    if lumber {
        ev.push(RepairEvent::CanPay { gold: false });
        if !inp.lumber_ok {
            return ev;
        }
    }
    let (mut gold_period, mut lumber_period) = (F32::ONE, F32::ONE);
    if gold {
        gold_period = div(time, gold_total);
        ev.push(RepairEvent::Pay { gold: true });
        ev.push(RepairEvent::Timer { msg: 0xD01D0, period: gold_period });
    }
    if lumber {
        lumber_period = div(time, lumber_total);
        ev.push(RepairEvent::Pay { gold: false });
        ev.push(RepairEvent::Timer { msg: 0xD01D1, period: lumber_period });
    }
    let one_le = |p: F32| f64::from(F32::ONE.to_f32()) <= f64::from(p.to_f32());
    if one_le(gold_period) && one_le(lumber_period) {
        ev.push(RepairEvent::Timer { msg: 0xD01C1, period: F32::ONE });
    }
    ev.push(RepairEvent::Modifier { rate: div(inp.max_life, time) });
    ev
}

/// The target gate of Repair (slot `0x3F0` = `0x6F04BF70`, CONFIRMED by Unicorn, V10 `rep_gate`) for a target without an `ABnP` ability:
/// `152` for a locked unit (`+0x5C & 0x100`), `30` when `max - life` is ordered below 1.0, `161` with an `Buns` buff, `162` for an
/// illusion, `8` / `9` when the repair needs gold / lumber (`goldRep` / `lumberRep` nonzero) and one unit cannot be paid, else `0`.
pub fn repair_gate_code(flags: u32, life: F32, max_life: F32, unsummoning: bool, gold_rep: i32, lumber_rep: i32, gold_ok: bool, lumber_ok: bool) -> u32 {
    if flags & 0x100 != 0 {
        return 152;
    }
    if lt87(arith::sub(max_life, life), F32::ONE) {
        return 30;
    }
    if unsummoning {
        return 161;
    }
    if flags & 0x4000_0000 != 0 {
        return 162;
    }
    if gold_rep != 0 && !gold_ok {
        return 8;
    }
    if lumber_rep != 0 && !lumber_ok {
        return 9;
    }
    0
}

const BUFF_UNSUMMON: u32 = code(b"Buns");

impl Game {
    /// The gate code of a repair of `target` by a builder of `owner` (see [`repair_gate_code`]).
    pub(crate) fn repair_gate(&self, owner: u32, target: ObjKey) -> u32 {
        let u = self.unit(target);
        let (gold_rep, lumber_rep) = self.unit_types().get(u.type_id).map_or((0, 0), |t| (t.gold_rep, t.lumber_rep));
        let can = |res| self.economy.get(owner as usize).is_some_and(|p| p.stored(res) / 10 >= 1);
        let flags = if u.illusion { 0x4000_0000 } else { 0 };
        repair_gate_code(
            flags,
            u.life,
            u.max_life,
            self.buff_position(target, BUFF_UNSUMMON).is_some(),
            gold_rep,
            lumber_rep,
            can(crate::economy::Res::Gold),
            can(crate::economy::Res::Lumber),
        )
    }
}

/// Repair, Renew, Restoration (`CAbilityRepair < CAbilityAutoTargetSpell`, `CAbilityRenew` and `CAbilityRestoration <
/// CAbilityRepair`), effect slot `0x424` = `0x6F070AB0` for a finished building: see [`repair_start_plan`]. Not modelled: a target that
/// is still under construction (the `ABnP` branches: taking over or joining a construction, the "power build" `DataC` / `DataD`).
fn start_repair(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    gm.end_repair(rec.caster);
    let owner = gm.unit(rec.caster).owner;
    let (type_id, max_life) = (gm.unit(target).type_id, gm.unit(target).max_life);
    let (gold_rep, lumber_rep, rep_time) =
        gm.unit_types().get(type_id).map_or((0, 0, 0), |t| (t.gold_rep, t.lumber_rep, t.repair_time));
    let can = |gm: &Game, res| gm.economy.get(owner as usize).is_some_and(|p| p.stored(res) / 10 >= 1);
    let inp = RepairStartIn {
        data_a: rec.level.data[0],
        data_b: rec.level.data[1],
        gold_rep,
        lumber_rep,
        rep_time,
        max_life,
        gold_ok: can(&gm, crate::economy::Res::Gold),
        lumber_ok: can(&gm, crate::economy::Res::Lumber),
    };
    let at = (gm.unit(rec.caster).x, gm.unit(rec.caster).y);
    let mut job = RepairJob { target, modifier: None, timers: Vec::new(), at };
    let plan = repair_start_plan(&inp, &|a, b| gm.soft_div(a, b));
    for e in plan {
        match e {
            RepairEvent::CanPay { .. } => {}
            RepairEvent::Pay { gold } => {
                let res = if gold { crate::economy::Res::Gold } else { crate::economy::Res::Lumber };
                crate::economy::change(&mut gm.economy, owner as usize, res, -10, crate::economy::Flags::NONE);
            }
            RepairEvent::Timer { msg, period } => {
                let kind = match msg {
                    0xD01D0 => KIND_REPAIR_GOLD,
                    0xD01D1 => KIND_REPAIR_LUMBER,
                    _ => KIND_REPAIR,
                };
                let t = gm.timers.start(
                    period.to_f32(),
                    true,
                    Payload::Spell(SpellMsg::UnitSpell { kind, unit: rec.caster, base: rec.base, aux: 0 }),
                );
                job.timers.push(t);
            }
            RepairEvent::Modifier { rate } => job.modifier = gm.add_life_regen_modifier(target, rate),
        }
    }
    gm.spells.m.repairs.insert(rec.caster, job);
}

impl Game {
    /// The target `builder` is repairing, if any.
    pub fn m_repairing(&self, builder: ObjKey) -> Option<ObjKey> {
        self.spells.m.repairs.get(&builder).map(|j| j.target)
    }

    /// The stop of the ability (`0x6F06BBB0`): the payment and tick timers end and the regeneration modifier is released.
    fn end_repair(&mut self, builder: ObjKey) {
        if let Some(j) = self.spells.m.repairs.remove(&builder) {
            for t in j.timers {
                self.timers.cancel(t);
            }
            if let Some(m) = j.modifier {
                self.release_modifier(m);
            }
        }
    }

    /// A repair timer fired (`0x6F070FE0`): the target gate is checked again (a target that is whole, a builder that cannot pay, a
    /// vanished target all stop the repair), then a payment event takes one unit. LIKELY (the validation inside `0x6F035070` is
    /// summarised by the gate, the alive tests and the builder staying put; the range re-check and re-order are not modelled).
    fn repair_event(&mut self, builder: ObjKey, gold: bool, lumber: bool) {
        let Some(job) = self.spells.m.repairs.get(&builder).cloned() else {
            return;
        };
        let moved = {
            let b = self.unit(builder);
            b.x.to_bits() != job.at.0.to_bits() || b.y.to_bits() != job.at.1.to_bits()
        };
        if !self.alive(builder) || !self.alive(job.target) || moved || self.is_stunned(builder) {
            self.end_repair(builder);
            return;
        }
        let owner = self.unit(builder).owner;
        if self.repair_gate(owner, job.target) != 0 {
            self.end_repair(builder);
            return;
        }
        for (on, res) in [(gold, crate::economy::Res::Gold), (lumber, crate::economy::Res::Lumber)] {
            if on {
                crate::economy::change(&mut self.economy, owner as usize, res, -10, crate::economy::Flags::NONE);
            }
        }
    }
}

// ---- Replenish (the Moon Well), CONFIRMED by Unicorn (V10, `tools/emu/v10_emu.py` checks `rep_effect`, `rep_precheck`) ----

/// The marker buffs of the Moon Well (`CBuffReplenish < CBuffHeal`, id getters `0x6F1D34E0`, `0x6F1D3560`, `0x6F1D35E0`).
pub const BUFF_REPLENISH: u32 = code(b"Brpb");
pub const BUFF_REPLENISH_LIFE: u32 = code(b"Brpl");
pub const BUFF_REPLENISH_MANA: u32 = code(b"Brpm");

/// The inputs of the Moon Well pulse (`CAbilityReplenish`, `Arpb`; `CAbilityReplenishLife` `Arpl` and `...Mana` `Arpm` differ only in
/// slots `0x478` / `0x47C`, which return `0.0` for the missing half). All values are level-record fields (`Cast` +4, `Dur` +8,
/// `HeroDur` +0xC, `Cost` +0x10, `Cool` +0x14, `DataA` +0x20, `DataB` +0x24, `DataE` +0x30) or ability fields (`extra` +0x9C,
/// `extra_cool` +0x13C).
#[derive(Debug, Clone, Copy)]
pub struct ReplenishIn {
    pub base: u32,
    pub cast: F32,
    pub dur: F32,
    pub hero_dur: F32,
    pub extra: F32,
    pub cost: i32,
    pub cool: F32,
    pub data_a: F32,
    pub data_b: F32,
    pub data_e: F32,
    pub extra_cool: F32,
    /// Misc `IllusionsCanRestoreLife` / `IllusionsCanRestoreMana`.
    pub misc_life: bool,
    pub misc_mana: bool,
    /// Misc `EtherealHealBonus`.
    pub ethereal_bonus: F32,
}

/// One candidate of the group, in the collector's order, with every value the callback `0x6F1FAAA0` reads.
#[derive(Debug, Clone, Copy)]
pub struct ReplenishUnit {
    pub hero: bool,
    pub life: F32,
    pub max_life: F32,
    pub mana: F32,
    pub max_mana: F32,
    pub illusion: bool,
    /// The ethereal count (`+0x158`, a signed compare).
    pub ethereal: i32,
    /// Whether the unit already carries each marker buff the callback could look up (`Brpb`, `Brpl`, `Brpm`).
    pub existing: [bool; 3],
    /// The per-unit gate, slot `0x3EC` (`0x6F04F7A0`), nonzero = the unit is refused.
    pub refused: bool,
}

/// What the pulse does, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReplenishEvent {
    /// The target effect `0x6F052EC0` (presentation).
    Fx(usize),
    /// The marker lookup `0x6F0787D0(unit, id, 0, 0, 1, 1)`.
    Lookup { unit: usize, id: u32 },
    /// The marker exists: slot `0x350` with the duration.
    Refresh { unit: usize, id: u32, dur: F32 },
    /// The marker is created: `0x324`, `0x354(unit, &dur, caster)`, registered on the unit.
    Create { unit: usize, id: u32, dur: F32 },
    /// The life adder `0x6F277C20` gets `amount`.
    Life { unit: usize, amount: F32 },
    /// The mana adder `0x6F279140` gets `amount`.
    Mana { unit: usize, amount: F32 },
    /// The caster effect `0x6F053040` after the group.
    CasterFx,
    /// The mana payment `0x6F283DB0(caster, &out, &cost, 1)`.
    Pay(F32),
    /// The cooldown start (`Cool + ability +0x13C`, flag 1).
    Cooldown(F32),
}

/// The precheck value of the Moon Well, slot `0x374` = `0x6F1DBCC0` (CONFIRMED by Unicorn, V10 `rep_precheck`): `0.0 + I2R(F2I(DataE) *
/// Cost)` by soft add (the base slot `0x36C` returns `0.0`). The readiness test compares the caster's mana with it (slot `0x370`
/// `0x6F1D36A0`: a value equal to `0.0` passes, otherwise the mana check `0x6F283C70`). The generic payment of the cast body is skipped
/// (slot `0x3D4` returns 0): the effect pays itself.
pub fn replenish_precheck(data_e: F32, cost: i32) -> F32 {
    let product = softfloat::conv::f2i(data_e).wrapping_mul(cost);
    arith::add(F32::ZERO, softfloat::conv::i2r(product))
}

/// The readiness test of the Moon Well for a caster with `mana` (slot `0x370` = `0x6F1D36A0`, CONFIRMED by reading: a precheck value equal to
/// `0.0` passes, otherwise the unit-side mana check). `None` for abilities with the generic rule.
pub(crate) fn precheck_ok(base: u32, level: &crate::cast::SpellLevel, mana: F32) -> Option<bool> {
    if !pays_in_effect(base) {
        return None;
    }
    let need = replenish_precheck(level.data[4], level.cost);
    Some(!nonzero(need) || crate::cast::mana_check(mana, need))
}

/// The classes whose cost slot `0x3D4` returns 0, so the generic cast body pays nothing (`0x6F05C840`): the effect pays itself.
pub(crate) fn pays_in_effect(base: u32) -> bool {
    matches!(base, REPLENISH | REPLENISH_LIFE | REPLENISH_MANA)
}

/// The pulse of the Moon Well (effect slot `0x428` = `0x6F1FADC0` and its per-unit callback `0x6F1FAAA0`; CONFIRMED by Unicorn, V10
/// `rep_effect`): an immediate pulse with no target. The callback runs for the candidates of the group (targs flags, `Area` around the
/// caster) in collector order; the record it shares holds the life amount (`DataA`) and the mana amount (`DataB`) and **is changed in
/// place** by the ethereal bonus, so a later unit sees the scaled value. Per unit: stop when `F2I(Cast)` is nonzero and that many
/// units were taken; skip a unit that needs nothing (an amount is "set" when nonzero, "needed" when `0.0 < max - current`) or that slot
/// `0x3EC` refuses; otherwise count it, look up / refresh / create the marker (`Brpb` both amounts, `Brpl` life only, `Brpm` mana
/// only) with `HeroDur` for a hero else `Dur + extra`, then (unless an illusion is blocked by the two Misc switches) scale the life
/// amount for an ethereal unit and hand the amounts to the adders. After the group the caster pays `max(1, min(count, F2I(DataE))) *
/// Cost` and starts its cooldown.
pub fn replenish_plan(inp: &ReplenishIn, units: &[ReplenishUnit]) -> Vec<ReplenishEvent> {
    let mut ev = Vec::new();
    let mut life_amt = if inp.base == REPLENISH_MANA { F32::ZERO } else { inp.data_a };
    let mana_amt = if inp.base == REPLENISH_LIFE { F32::ZERO } else { inp.data_b };
    let limit = softfloat::conv::f2i(inp.cast);
    let normal_dur = arith::add(inp.dur, inp.extra);
    let mut count: i32 = 0;
    for (i, u) in units.iter().enumerate() {
        if limit != 0 && count == limit {
            break;
        }
        let life_set = nonzero(life_amt);
        let mana_set = nonzero(mana_amt);
        let life_need = life_set && lt87(F32::ZERO, arith::sub(u.max_life, u.life));
        let mana_need = mana_set && lt87(F32::ZERO, arith::sub(u.max_mana, u.mana));
        if !((life_set || mana_set) && (life_need || mana_need)) || u.refused {
            continue;
        }
        ev.push(ReplenishEvent::Fx(i));
        count = count.wrapping_add(1);
        let dur = if u.hero { inp.hero_dur } else { normal_dur };
        let (id, slot) = match (life_set, mana_set) {
            (true, true) => (BUFF_REPLENISH, 0),
            (true, false) => (BUFF_REPLENISH_LIFE, 1),
            _ => (BUFF_REPLENISH_MANA, 2),
        };
        ev.push(ReplenishEvent::Lookup { unit: i, id });
        if u.existing[slot] {
            ev.push(ReplenishEvent::Refresh { unit: i, id, dur });
        } else {
            ev.push(ReplenishEvent::Create { unit: i, id, dur });
        }
        if (u.illusion && !inp.misc_life && life_need) || (u.illusion && !inp.misc_mana && mana_need) {
            continue;
        }
        if u.ethereal > 0 {
            life_amt = arith::mul(life_amt, inp.ethereal_bonus);
        }
        if life_need {
            ev.push(ReplenishEvent::Life { unit: i, amount: life_amt });
        }
        if mana_need {
            ev.push(ReplenishEvent::Mana { unit: i, amount: mana_amt });
        }
    }
    ev.push(ReplenishEvent::CasterFx);
    let cap = softfloat::conv::f2i(inp.data_e) as u32;
    let mut n = if (count as u32) < cap { count as u32 } else { cap };
    if n == 0 {
        n = 1;
    }
    ev.push(ReplenishEvent::Pay(softfloat::conv::i2r((n as i32).wrapping_mul(inp.cost))));
    ev.push(ReplenishEvent::Cooldown(arith::add(inp.cool, inp.extra_cool)));
    ev
}

/// The Moon Well pulse in the game: collects the candidates like the other area effects (circle of `Area` around the caster, the
/// level's targs flags `ground,air,friend,self,organic,vuln,invu`), runs [`replenish_plan`] and applies the events.
fn replenish_pulse(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let (cx, cy) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y)
    };
    let word = targs_word("ground,air,friend,self,organic,vuln,invu");
    let mut keys: Vec<ObjKey> = Vec::new();
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        let u = gm.unit(k);
        if !gm.alive(k) || u.hidden || u.removed || !targs_ok(&gm, rec.caster, k, word) {
            continue;
        }
        keys.push(k);
    }
    let units: Vec<ReplenishUnit> = keys
        .iter()
        .map(|&k| {
            let u = gm.unit(k);
            ReplenishUnit {
                hero: gm.target_hero(k),
                life: u.life,
                max_life: u.max_life,
                mana: u.mana,
                max_mana: u.max_mana,
                illusion: u.illusion,
                ethereal: u.ethereal as i32,
                existing: [BUFF_REPLENISH, BUFF_REPLENISH_LIFE, BUFF_REPLENISH_MANA].map(|b| gm.buff_position(k, b).is_some()),
                refused: false,
            }
        })
        .collect();
    let d = &rec.level.data;
    let inp = ReplenishIn {
        base: rec.base,
        cast: rec.level.cast,
        dur: rec.level.dur,
        hero_dur: rec.level.hero_dur,
        extra: F32::ZERO,
        cost: rec.level.cost,
        cool: rec.level.cool,
        data_a: d[0],
        data_b: d[1],
        data_e: d[4],
        extra_cool: F32::ZERO,
        misc_life: gm.spells.item.illusions_heal,
        misc_mana: gm.spells.item.illusions_mana,
        ethereal_bonus: gm.spells.unit.ethereal_heal_bonus,
    };
    let plan = replenish_plan(&inp, &units);
    for e in plan {
        match e {
            ReplenishEvent::Create { unit, id, dur } => gm.replenish_mark(keys[unit], id, dur),
            ReplenishEvent::Refresh { unit, id, dur } => gm.replenish_mark(keys[unit], id, dur),
            ReplenishEvent::Life { unit, amount } => crate::item_abilities::add_life(&mut gm, keys[unit], amount),
            ReplenishEvent::Mana { unit, amount } => crate::item_abilities::add_mana(&mut gm, keys[unit], amount),
            ReplenishEvent::Pay(cost) => {
                let mana = gm.unit(rec.caster).mana;
                let (_, new_mana) = crate::cast::pay_mana(mana, cost);
                gm.write_mana(rec.caster, new_mana);
            }
            // The cooldown is started by the cast body after the effect (`Cool`, no ability bonus in the model).
            ReplenishEvent::Fx(_) | ReplenishEvent::Lookup { .. } | ReplenishEvent::CasterFx | ReplenishEvent::Cooldown(_) => {}
        }
    }
}

impl Game {
    /// Puts (or refreshes, by the set-duration rule) a Moon Well marker on `target`.
    fn replenish_mark(&mut self, target: ObjKey, id: u32, duration: F32) {
        let serial = match self.buff_position(target, id) {
            Some(i) => self.unit(target).buffs[i].serial,
            None => {
                let serial = self.next_buff_serial();
                self.unit_mut(target).buffs.push(crate::abilities::Buff {
                    serial,
                    base: id,
                    predicates: buff_predicates(id),
                    ..crate::abilities::Buff::default()
                });
                serial
            }
        };
        self.set_buff_duration(target, serial, duration);
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

/// One candidate of Taunt's sort (a 20-byte record in the original): `f1` the distance key, `f2` the "already taunted"
/// key (0.0 without the buff), `f3` / `f4` the relation flags `0x6F285B80` / `0x6F285B00`, `id` the unit's agent id
/// (`unit +0x0C`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TauntRec {
    pub f1: F32,
    pub f2: F32,
    pub f3: i32,
    pub f4: i32,
    pub id: i32,
}

fn taunt_sign(d: F32) -> i32 {
    if 0.0f32 < d.to_f32() {
        1
    } else {
        -1
    }
}

fn taunt_nonzero(d: F32) -> bool {
    // x87 `fucomp` against 0.0: equal only when ordered and equal
    let x = d.to_f32();
    x != 0.0 || x.is_nan()
}

/// The base comparator `0x6F160310` (CONFIRMED by Unicorn, V4): `A.f1 - B.f1` (soft subtract), a nonzero difference gives
/// `+1` when positive and `-1` otherwise (NaN included); equal first keys fall to `A.f2 - B.f2` the same way, then to the
/// agent ids `B.id - A.id`.
pub fn taunt_compare_base(a: &TauntRec, b: &TauntRec) -> i32 {
    let d1 = arith::sub(a.f1, b.f1);
    if taunt_nonzero(d1) {
        return taunt_sign(d1);
    }
    let d2 = arith::sub(a.f2, b.f2);
    if taunt_nonzero(d2) {
        return taunt_sign(d2);
    }
    b.id.wrapping_sub(a.id)
}

/// The three comparators: mode 0 = `0x6F160310`; mode 1 = `0x6F16E390` (`B.f3 - A.f3`, then the base); mode 2 =
/// `0x6F16E3B0` (`B.f4 - A.f4`, then the base).
pub fn taunt_compare(mode: u32, a: &TauntRec, b: &TauntRec) -> i32 {
    let first = match mode {
        1 => b.f3.wrapping_sub(a.f3),
        2 => b.f4.wrapping_sub(a.f4),
        _ => 0,
    };
    if first != 0 {
        first
    } else {
        taunt_compare_base(a, b)
    }
}

/// The CRT `qsort` of MSVC 2005 as the game links it (`0x6F4C6210`, CONFIRMED structure): pieces of at most 8 elements are
/// sorted by repeatedly moving the maximum to the end, larger ones are partitioned around the middle element (swapped to the
/// front) with the smaller side iterated and the larger pushed. Comparator results are tested as signed ints. `width` is the
/// record size in bytes (20) for the "which side is larger" test.
pub fn crt_qsort<T>(v: &mut [T], width: usize, cmp: &dyn Fn(&T, &T) -> i32) {
    if v.len() < 2 {
        return;
    }
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let (mut lo, mut hi) = (0usize, v.len() - 1);
    loop {
        let size = hi - lo + 1;
        let mut again = false;
        if size <= 8 {
            let mut top = hi;
            while top > lo {
                let mut max = lo;
                for p in lo + 1..=top {
                    if cmp(&v[p], &v[max]) > 0 {
                        max = p;
                    }
                }
                v.swap(max, top);
                top -= 1;
            }
        } else {
            let mid = lo + size / 2;
            v.swap(mid, lo);
            let mut loguy = lo;
            let mut higuy = hi + 1;
            loop {
                loop {
                    loguy += 1;
                    if !(loguy <= hi && cmp(&v[loguy], &v[lo]) <= 0) {
                        break;
                    }
                }
                loop {
                    higuy -= 1;
                    if !(higuy > lo && cmp(&v[higuy], &v[lo]) >= 0) {
                        break;
                    }
                }
                if higuy < loguy {
                    break;
                }
                v.swap(loguy, higuy);
            }
            v.swap(lo, higuy);
            // byte distances as in the original: `higuy - 1 - lo >= hi - loguy`
            if (higuy as i64 - lo as i64) * width as i64 - 1 >= (hi as i64 - loguy as i64) * width as i64 {
                if lo + 1 < higuy {
                    stack.push((lo, higuy - 1));
                }
                if loguy < hi {
                    lo = loguy;
                    again = true;
                }
            } else {
                if loguy < hi {
                    stack.push((loguy, hi));
                }
                if lo + 1 < higuy {
                    hi = higuy - 1;
                    again = true;
                }
            }
        }
        if again {
            continue;
        }
        match stack.pop() {
            Some((l, h)) => {
                lo = l;
                hi = h;
            }
            None => break,
        }
    }
}

/// What Taunt's effect `0x6F17B5A0` does with the collected candidates (CONFIRMED by Unicorn, V4, with the real sort): with
/// `n = F2I(DataA)` nonzero the records are sorted by the comparator chosen from `DataB` (nonzero: mode 2) and `DataC`
/// (nonzero: mode 1, else mode 0); `n` is then limited to the number of candidates (0 or too large: all of them); the first
/// `n` records each get the effect slot `0x42C` with `HeroDur` for a hero, else `Dur + 0x9C`. Without a limit the records
/// keep the collection order. Returns `(index into the input, duration)` in application order.
pub fn taunt_plan(l: &crate::cast::SpellLevel, extra: F32, recs: &[(TauntRec, bool)]) -> Vec<(usize, F32)> {
    group_plan(l, l.data[0], l.data[1], l.data[2], extra, recs)
}

/// The shared selection of Taunt and Roar (`0x6F17B5A0`, `0x6F1EED60`): `limit_field` is the level real whose integer
/// part limits the units (Taunt `DataA`, Roar `DataG`), `key2` picks the second-flag comparator when nonzero (Taunt `DataB`, Roar
/// `DataE`), `key1` the first-flag comparator (Taunt `DataC`, Roar `DataF`). Roar's comparators `0x6F1D3BF0`, `0x6F1DC030`,
/// `0x6F1DC050` are the same code as Taunt's (CONFIRMED by Unicorn, V4).
pub fn group_plan(
    l: &crate::cast::SpellLevel,
    limit_field: F32,
    key2: F32,
    key1: F32,
    extra: F32,
    recs: &[(TauntRec, bool)],
) -> Vec<(usize, F32)> {
    let limit = softfloat::conv::f2i(limit_field) as u32;
    let mut order: Vec<(usize, TauntRec, bool)> = recs.iter().enumerate().map(|(i, (r, h))| (i, *r, *h)).collect();
    if limit != 0 {
        let mode = if taunt_nonzero(key2) {
            2
        } else if taunt_nonzero(key1) {
            1
        } else {
            0
        };
        crt_qsort(&mut order, 20, &|a, b| taunt_compare(mode, &a.1, &b.1));
    }
    let count = order.len() as u32;
    let n = if limit == 0 || limit > count { count } else { limit };
    order[..n as usize]
        .iter()
        .map(|(i, _, hero)| (*i, effect_duration(l, *hero, extra)))
        .collect()
}

/// Taunt (`CAbilityTaunt < CAbilitySimpleSpell`, immediate order 852520, targs `air,ground,enemy,vuln,invu`): the group
/// around the caster within `Area` is collected into 20-byte records, sorted and limited by [`taunt_plan`] (`DataA` units
/// at most: the nearest first for the default `DataB = DataC = 0`), and each chosen unit gets the effect slot `0x42C` (the
/// Taunt buff, which makes it attack the caster: modelled as an attack order, UNSURE). The distance key is the squared world
/// distance (the original compares cell units: same order), the "already taunted" key is 0.0 (no taunt buff in the model),
/// the relation flags are 0 / 1 for enemies.
fn taunt(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (cx, cy) = (gm.unit(rec.caster).x, gm.unit(rec.caster).y);
    let handle = gm.handle_of(rec.caster);
    if handle == 0 {
        return;
    }
    let word = targs_word("air,ground,enemy,vuln,invu");
    let mut cands: Vec<(ObjKey, TauntRec, bool)> = Vec::new();
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        if k == rec.caster || !gm.alive(k) || !targs_ok(&gm, rec.caster, k, word) {
            continue;
        }
        let u = gm.unit(k);
        let (dx, dy) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
        let f1 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
        let ally = !gm.is_enemy(gm.unit(rec.caster).owner, u.owner);
        let hero = gm.target_hero(k);
        let id = gm.handle_of(k) as i32;
        cands.push((k, TauntRec { f1, f2: F32::ZERO, f3: i32::from(ally), f4: i32::from(!ally), id }, hero));
    }
    let plan = taunt_plan(&rec.level, F32::ZERO, &cands.iter().map(|(_, r, h)| (*r, *h)).collect::<Vec<_>>());
    for (i, _dur) in plan {
        let order = crate::orders::Order {
            id: crate::orders::ORDER_ATTACK,
            class: crate::orders::OrderClass::Target,
            point: None,
            target: handle,
        };
        let _ = gm.issue_order(cands[i].0, order, true);
    }
}
