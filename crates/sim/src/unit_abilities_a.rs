//! Unit abilities, batch A (S46). Spec: `docs/spec/abilities-unit-a.md`.
//!
//! 81 ability codes of units and buildings (auras, passives, cargo, detection, transforms and a few spells). Each code
//! has an entry in [`ABILITIES`] (RTTI class and parent from the ability atlas `docs/research/ability-atlas/factory.tsv`,
//! what the model does with it, and the confidence); the entries marked [`Sim::Data`] are recorded but not simulated.
//!
//! Hooks (called from `cast.rs`, `auras.rs` and `unit_spells.rs`): [`register_orders`], [`takes_no_target`],
//! [`target_error`], [`run_effect`], [`immediate_order`], [`on_timer`] and [`Game::sync_unit_abilities_a`].
//! Labels as elsewhere: CONFIRMED = read in the machine code, LIKELY = strong inference (class parent, editor field
//! meaning, data), UNSURE = a stand-in.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::cast::{CastError, CastRec, CastTarget, SpellMsg};
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::ScriptRuntime;
use crate::timer::TimerId;
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// What the model does with a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sim {
    /// An aura emitter (`CAbilityAura`).
    Aura,
    /// A passive with a counter or hook on the unit.
    Passive,
    /// A cast spell.
    Spell,
    /// A transform (`CAbilityMorph`).
    Morph,
    /// The transport system.
    Cargo,
    /// A detector.
    Detector,
    /// A hook for the invisibility module (S44).
    Hook,
    /// Handled by another module (economy, production).
    Elsewhere,
    /// A marker without behaviour.
    Marker,
    /// Recorded only.
    Data,
}

/// Confidence of the model of one code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conf {
    Confirmed,
    Likely,
    Unsure,
    NotDone,
}

/// One registry entry.
#[derive(Debug, Clone, Copy)]
pub struct Info {
    pub code: u32,
    /// The RTTI class and its nearest modelled parent (atlas).
    pub class: &'static str,
    pub parent: &'static str,
    pub sim: Sim,
    pub conf: Conf,
    pub note: &'static str,
}

/// The registry of this batch.
pub const ABILITIES: &[Info] = &[
    Info { code: code(b"Aabr"), class: "CAbilityAuraBlightRegen", parent: "CAbilityAura", sim: Sim::Aura, conf: Conf::Likely, note: "CAbilityAura; buff Babr (getter 0x6F022E40); life regen DataA, a fraction of max life when DataB != 0" },
    Info { code: code(b"Aabs"), class: "CAbilityAbsorb", parent: "CAbilitySimpleSpell", sim: Sim::Spell, conf: Conf::Likely, note: "CAbilityAbsorb order 852529: moves up to DataB mana from the target to the caster" },
    Info { code: code(b"Aaha"), class: "CAbilityAcolyteHarvest", parent: "CPower", sim: Sim::Elsewhere, conf: Conf::Likely, note: "harvest: crates/sim/src/production.rs (gold/lumber carry per trip)" },
    Info { code: code(b"Aakb"), class: "CAbilityAuraKotoBeast", parent: "CAbilityAura", sim: Sim::Aura, conf: Conf::Likely, note: "CBuffAuraKotoBeast < CBuffAuraCommand; buff Bakb; damage bonus DataA (fraction of the base damage)" },
    Info { code: code(b"Aall"), class: "CAbilityAllied", parent: "CAbilityNeutral", sim: Sim::Marker, conf: Conf::Likely, note: "CAbilityNeutral marker of shop buildings (DataA 600 range, flags B-D); no simulation effect" },
    Info { code: code(b"Aami"), class: "CAbilityAntiMagicShellInstant", parent: "CAbilityAntiMagicShell", sim: Sim::Spell, conf: Conf::Unsure, note: "CAbilityAntiMagicShellInstant < CAbilityAntiMagicShell: self cast form" },
    Info { code: code(b"Aamk"), class: "CAbilityAttributeModSkill", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "hero attribute bonus DataA/B/C: needs the hero attribute setter path" },
    Info { code: code(b"Aams"), class: "CAbilityAntiMagicShell", parent: "CAbilitySimpleSpell", sim: Sim::Spell, conf: Conf::Unsure, note: "CAbilityAntiMagicShell: spell immunity counter for Dur, buff Bams" },
    Info { code: code(b"Aapl"), class: "CAbilityAuraPlague", parent: "CAbilityAura", sim: Sim::Data, conf: Conf::NotDone, note: "Disease Cloud: CBuffAuraPlague < CBuffLifeDoT; DataA 120 / DataB 1 / DataC 10 and UnitID uplg not decoded (tick amount, ward spawn on death)" },
    Info { code: code(b"Aarm"), class: "CAbilityAuraRegenMana", parent: "CAbilityAura", sim: Sim::Aura, conf: Conf::Likely, note: "CAbilityAura; buff Barm (getter 0x6F022DC0); mana regen DataA, a fraction of max mana when DataB != 0" },
    Info { code: code(b"Aasl"), class: "CAbilityAuraSlow", parent: "CAbilityAura", sim: Sim::Aura, conf: Conf::Likely, note: "CAbilityAura, CBuffAuraSlow < CBuffAuraEndurance; buff Basl; movement DataA, attack speed DataB" },
    Info { code: code(b"Aast"), class: "CAbilityAncestralSpirit", parent: "CAbilityClosestTargetSpell", sim: Sim::Data, conf: Conf::NotDone, note: "revives a dead unit: needs the corpse revive path of revive.rs" },
    Info { code: code(b"Aave"), class: "CAbilityAvengerForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityAvengerForm < CAbilityMorph: UnitID target form, DataA alternate form" },
    Info { code: code(b"Aawa"), class: "CAbilityAwaken", parent: "CAbilityInterfaced", sim: Sim::Data, conf: Conf::NotDone, note: "Awaken: neutral buyable hero awaken, no data" },
    Info { code: code(b"Abar"), class: "CAbilityBarkskin", parent: "CAbilityModalSpell", sim: Sim::Data, conf: Conf::NotDone, note: "modal spell (Barkskin) with on/off orders 852135..7: effect not decoded" },
    Info { code: code(b"Abdt"), class: "CAbilityBurrowDetector", parent: "CAbilityDetector", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityBurrowDetector < CAbilityDetector (burrowed units)" },
    Info { code: code(b"Abgm"), class: "CAbilityBlightedGoldMine", parent: "CAbilityPossessedGoldMine", sim: Sim::Elsewhere, conf: Conf::Unsure, note: "possessed gold mine derived classes; harvest rows loaded by engine objdata, haunting/entangling not modelled" },
    Info { code: code(b"Abli"), class: "CAbilityBlightGrowth", parent: "CAbility", sim: Sim::Data, conf: Conf::NotDone, note: "blight dispel: needs a blight grid writer" },
    Info { code: code(b"Ablp"), class: "CAbilityBlightPlacement", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "blight placement: needs a blight grid writer" },
    Info { code: code(b"Abof"), class: "CAbilityBallsOfFire", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Balls of Fire passive attack effect, not decoded" },
    Info { code: code(b"Abrf"), class: "CAbilityBearForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityBearForm < CAbilityMorph" },
    Info { code: code(b"Absk"), class: "CAbilityBerserkerRage", parent: "CAbilitySpell", sim: Sim::Spell, conf: Conf::Unsure, note: "CAbilityBerserkerRage: attack speed +DataA, movement +DataB for Dur (damage-taken part DataC not modelled)" },
    Info { code: code(b"Abtl"), class: "CAbilityBattlestations", parent: "CAbilitySpell", sim: Sim::Data, conf: Conf::NotDone, note: "Battlestations: converts peons to militia (UnitID opeo), needs unit morph by list" },
    Info { code: code(b"Abun"), class: "CAbilityBunker", parent: "CAbilityCargoHold", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityBunker < CAbilityCargoHold: burrow, capacity DataA 4" },
    Info { code: code(b"Abur"), class: "CAbilityBurrow", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityBurrow < CAbilityMorph" },
    Info { code: code(b"Acan"), class: "CAbilityCannibalize", parent: "CAbilityClosestTargetSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Cannibalize: corpse channel with DataA life per second (DataB range 800)" },
    Info { code: code(b"Acar"), class: "CAbilityCargoHold", parent: "CPower", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoHold: capacity DataA; holds loaded units" },
    Info { code: code(b"Acha"), class: "CAbilityChaos", parent: "CAbilityChaosBase", sim: Sim::Data, conf: Conf::NotDone, note: "Chaos: unit type change by upgrade (UnitID in the row)" },
    Info { code: code(b"Achd"), class: "CAbilityCargoHoldDeath", parent: "CAbilityPurge", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoHoldDeath: holder whose cargo is released when it dies; capacity DataA 10" },
    Info { code: code(b"Achl"), class: "CAbilityChaosCargoLoad", parent: "CAbilityChaosBase", sim: Sim::Data, conf: Conf::NotDone, note: "Chaos cargo load: unit type change when loaded" },
    Info { code: code(b"Aclf"), class: "CAbilityCloudOfFog", parent: "CAbilitySilence", sim: Sim::Data, conf: Conf::NotDone, note: "Cloud of Fog (Silence parent): area effect with DataA ranged damage factor" },
    Info { code: code(b"Acmg"), class: "CAbilityControlMagic", parent: "CAbilitySimpleSpell", sim: Sim::Spell, conf: Conf::Unsure, note: "CAbilityControlMagic order 852474: owner change of a summoned or low level target" },
    Info { code: code(b"Acoa"), class: "CAbilityCoupleArcher", parent: "CAbilityCouple", sim: Sim::Data, conf: Conf::NotDone, note: "Couple (Archer): archer mounts a hippogryph, needs unit merge" },
    Info { code: code(b"Acoh"), class: "CAbilityCoupleHippogryph", parent: "CAbilityCouple", sim: Sim::Data, conf: Conf::NotDone, note: "Couple (Hippogryph): needs unit merge" },
    Info { code: code(b"Acoi"), class: "CAbilityCoupleInstant", parent: "CAbilityClosestTargetSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Couple instant: needs unit merge" },
    Info { code: code(b"Acor"), class: "CAbilityCorrosiveBreath", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Corrosive Breath: structure armor reduction DataA 50, passive attack effect" },
    Info { code: code(b"Acpf"), class: "CAbilityCorporealForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityCorporealForm < CAbilityMorph" },
    Info { code: code(b"Adch"), class: "CAbilityDisenchant", parent: "CAbilityDispelMagic", sim: Sim::Spell, conf: Conf::Likely, note: "CAbilityDisenchant < CAbilityDispelMagic: runs the Dispel Magic effect (DataB 300 summon damage)" },
    Info { code: code(b"Adda"), class: "CAbilityDeathDamageAoe", parent: "CAbility", sim: Sim::Data, conf: Conf::NotDone, note: "Death Damage (sapper/mine): area damage on death DataA/B/C/D rings, needs a death hook" },
    Info { code: code(b"Adec"), class: "CAbilityDecouple", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Decouple: splits hippogryph rider, needs unit split" },
    Info { code: code(b"Adef"), class: "CAbilityDefend", parent: "CAbilityButton", sim: Sim::Data, conf: Conf::NotDone, note: "Defend: toggle, DataA damage reduction 0.5 vs pierce, DataC move penalty" },
    Info { code: code(b"Adet"), class: "CAbilityDetector", parent: "CAbilityPassive", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityDetector: Rng detection radius; marks invisible enemies as detected for the owner" },
    Info { code: code(b"Adev"), class: "CAbilityDevour", parent: "CAbilityCargoLoad", sim: Sim::Cargo, conf: Conf::Unsure, note: "CAbilityDevour < CAbilityCargoLoad: order 852104; swallows the target into the devour hold (DataA 5 = digestion damage per second UNSURE)" },
    Info { code: code(b"Adri"), class: "CAbilityCargoDropInstant", parent: "CAbilitySpell", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoDropInstant: unload all, order 852049" },
    Info { code: code(b"Adro"), class: "CAbilityCargoDrop", parent: "CAbilitySimpleSpell", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoDrop: unload at point, order 852048" },
    Info { code: code(b"Adta"), class: "CAbilityDetectAoe", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Detect area spell (SimpleSpell, order 852015)" },
    Info { code: code(b"Adtn"), class: "CAbilityDetonate", parent: "CAbilityDispelMagic", sim: Sim::Spell, conf: Conf::Likely, note: "CAbilityDetonate < CAbilityDispelMagic: dispel effect, then the caster dies" },
    Info { code: code(b"Adts"), class: "CAbilityMagicSentry", parent: "CAbilityDetector", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityMagicSentry < CAbilityDetector" },
    Info { code: code(b"Advc"), class: "CAbilityDevourCargo", parent: "CAbilityCargoHold", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityDevourCargo < CAbilityCargoHold: devour stomach, capacity DataA 2" },
    Info { code: code(b"Advm"), class: "CAbilityDevourMagic", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Devour Magic: area dispel plus DataA life / DataB mana per buff" },
    Info { code: code(b"Aeat"), class: "CAbilityEatTree", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Eat Tree: tree target (destructable cast target not supported)" },
    Info { code: code(b"Aegm"), class: "CAbilityEntangledGoldMine", parent: "CAbilityPossessedGoldMine", sim: Sim::Elsewhere, conf: Conf::Unsure, note: "possessed gold mine derived classes; harvest rows loaded by engine objdata, haunting/entangling not modelled" },
    Info { code: code(b"Aenc"), class: "CAbilityEntangleCargo", parent: "CAbilityCargoHold", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityEntangleCargo < CAbilityCargoHold: entangled gold mine, capacity DataA 5" },
    Info { code: code(b"Aent"), class: "CAbilityEntangle", parent: "CPower", sim: Sim::Data, conf: Conf::NotDone, note: "Entangle: gold mine target, needs mine ownership change" },
    Info { code: code(b"Aesn"), class: "CAbilitySentinel", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Sentinel: tree target, creates an owl" },
    Info { code: code(b"Aetf"), class: "CAbilityEtherealForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityEtherealForm < CAbilityMorph" },
    Info { code: code(b"Aeth"), class: "CAbilityGhostVisible", parent: "CAbility", sim: Sim::Hook, conf: Conf::Unsure, note: "CAbilityGhostVisible: lets the unit see ghosts (DataA/DataB flags); hook only" },
    Info { code: code(b"Aetl"), class: "CAbilityEthereal", parent: "CAbilityPassive", sim: Sim::Passive, conf: Conf::Confirmed, note: "init slot 0x80 -> 0x6F07A340 adds one to the ethereal counter (+0x158), removal 0x6F0796A0 subtracts" },
    Info { code: code(b"Aexh"), class: "CAbilityExhume", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Exhume: meat wagon corpse storage" },
    Info { code: code(b"Afak"), class: "CAbilityFlamingAttack", parent: "CAbilityFlamingArrows", sim: Sim::Data, conf: Conf::NotDone, note: "Orb of Annihilation / flaming attack: on-hit splash" },
    Info { code: code(b"Afla"), class: "CAbilityFlare", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Flare: reveal area at point for Dur 15 (needs a fog reveal modifier)" },
    Info { code: code(b"Aflk"), class: "CAbilityFlakCannon", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Flak Cannon: on-hit air splash" },
    Info { code: code(b"Afrb"), class: "CAbilityFrostBreath", parent: "CAbility", sim: Sim::Data, conf: Conf::NotDone, note: "Frost Breath: attack applying Bfro" },
    Info { code: code(b"Afrz"), class: "CAbilityFreezingBreath", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Freezing Breath: structure attack slow" },
    Info { code: code(b"Afsh"), class: "CAbilityFragShards", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Frag Shards: on-hit splash" },
    Info { code: code(b"Afzy"), class: "CAbilityFrenzy", parent: "CAbilityBloodlust", sim: Sim::Spell, conf: Conf::Likely, note: "CAbilityFrenzy < CAbilityBloodlust: self Bloodlust numbers, buff Bfzy" },
    Info { code: code(b"Agho"), class: "CAbilityGhost", parent: "CAbilityPermInvis", sim: Sim::Hook, conf: Conf::Unsure, note: "CAbilityGhost < CAbilityPermInvis: permanent invisibility; hook only (S44 invisibility.rs owns the rules)" },
    Info { code: code(b"Agld"), class: "CAbilityGoldMine", parent: "CAbilityGoldMineBase", sim: Sim::Elsewhere, conf: Conf::Confirmed, note: "gold mine amount: crates/sim/src/economy.rs Mine (+0x6C), natives/economy.rs" },
    Info { code: code(b"Agra"), class: "CAbilityGrabTree", parent: "CAbilityEatTree", sim: Sim::Data, conf: Conf::NotDone, note: "Grab Tree: tree target" },
    Info { code: code(b"Agyb"), class: "CAbilityGyroBombs", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Gyrocopter Bombs: attack mode, no data" },
    Info { code: code(b"Agyd"), class: "CAbilityGraveyard", parent: "CAbility", sim: Sim::Data, conf: Conf::NotDone, note: "Graveyard: corpse generation marker" },
    Info { code: code(b"Agyv"), class: "CAbilityGyroVision", parent: "CAbilityDetector", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityGyroVision < CAbilityDetector" },
    Info { code: code(b"Ahar"), class: "CAbilityHarvest", parent: "CAbilityHarvestLumber", sim: Sim::Elsewhere, conf: Conf::Likely, note: "harvest: crates/sim/src/production.rs (gold/lumber carry per trip)" },
    Info { code: code(b"Ahid"), class: "CAbilityHide", parent: "CAbilityButton", sim: Sim::Data, conf: Conf::NotDone, note: "Shadow Meld (creep): invisibility at night, S44 hook" },
    Info { code: code(b"Ahrl"), class: "CAbilityHarvestLumber", parent: "CAbilityHarvestReturn", sim: Sim::Elsewhere, conf: Conf::Likely, note: "harvest: crates/sim/src/production.rs (gold/lumber carry per trip)" },
    Info { code: code(b"Aimp"), class: "CAbilityImpalingBolt", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Impaling Bolt: attack mode, no data" },
    Info { code: code(b"Alam"), class: "CAbilitySacrificialLamb", parent: "CAbilitySimpleSpell", sim: Sim::Data, conf: Conf::NotDone, note: "Sacrifice: acolyte into a haunted gold mine" },
    Info { code: code(b"Alea"), class: "CAbilityLeash", parent: "CAbility", sim: Sim::Data, conf: Conf::NotDone, note: "Leash: neutral leash, no data" },
    Info { code: code(b"Aliq"), class: "CAbilityLiquidFire", parent: "CAbilityPassive", sim: Sim::Data, conf: Conf::NotDone, note: "Liquid Fire: structure attack debuff (DataA 8, DataC 0.8)" },
    Info { code: code(b"Alit"), class: "CAbilityLightningAttack", parent: "CAbility", sim: Sim::Data, conf: Conf::NotDone, note: "Lightning Attack: attack mode, DataA/B" },
    Info { code: code(b"Aloa"), class: "CAbilityCargoLoad", parent: "CAbilitySimpleSpell", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoLoad (SimpleSpell): load order 852046, target unit with a cargo hold" },
];

/// The registry entry of `base`.
pub fn info(base: u32) -> Option<&'static Info> {
    ABILITIES.iter().find(|i| i.code == base)
}

pub const ETHEREAL: u32 = code(b"Aetl");
pub const AURA_BLIGHT_REGEN: u32 = code(b"Aabr");
pub const AURA_REGEN_MANA: u32 = code(b"Aarm");
pub const AURA_SLOW: u32 = code(b"Aasl");
pub const AURA_WAR_DRUMS: u32 = code(b"Aakb");
/// Buff codes of the aura classes (the code getters of `CBuffAura*`: `0x6F022E40`, `0x6F022DC0`, `0x6F196250`,
/// `0x6F195CD0`, CONFIRMED).
pub const BUFF_BLIGHT_REGEN: u32 = code(b"Babr");
pub const BUFF_REGEN_MANA: u32 = code(b"Barm");
pub const BUFF_AURA_SLOW: u32 = code(b"Basl");
pub const BUFF_WAR_DRUMS: u32 = code(b"Bakb");

/// Timer kinds of this module (`SpellMsg::UnitSpell` with `kind >= 0x40`).
pub const KIND_AURA_SCAN: u8 = 0x40;
pub const KIND_DETECT: u8 = 0x41;
pub const KIND_MORPH: u8 = 0x42;
pub const KIND_DIGEST: u8 = 0x43;

/// The aura scan period of the `CAbilityAura` base (`0x6F053B30`: 0.5 s proximity scan, CONFIRMED in `auras.rs`).
const SCAN_PERIOD: f32 = 0.5;

/// State of the abilities of this batch (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Units that carry one ethereal count from `Aetl`.
    eth: BTreeSet<ObjKey>,
    /// Running aura emitters: `(unit, ability) -> scan timer`.
    emitters: BTreeMap<(ObjKey, u32), TimerId>,
    /// Cargo: holder -> passengers in load order.
    cargo: BTreeMap<ObjKey, Vec<ObjKey>>,
    /// Passenger -> holder.
    carried: BTreeMap<ObjKey, ObjKey>,
    /// Detection: unit -> bit mask of the players that currently detect it.
    detected: BTreeMap<ObjKey, u16>,
    /// Running detector timers.
    detectors: BTreeMap<ObjKey, TimerId>,
    /// Units that are permanently invisible by `Agho` (hook for the invisibility module).
    ghosts: BTreeSet<ObjKey>,
    /// Units that carry `Aeth` (see ghosts).
    ghost_seers: BTreeSet<ObjKey>,
    /// Pending morphs: unit -> (target type, ability that started it).
    morphing: BTreeMap<ObjKey, (u32, u32)>,
    /// Digestion records of devoured units: passenger -> (devourer, damage per second).
    digest: BTreeMap<ObjKey, (ObjKey, F32)>,
}

fn aura_def(base: u32) -> Option<(u32, &'static str)> {
    Some(match base {
        AURA_BLIGHT_REGEN => (BUFF_BLIGHT_REGEN, "ground,air,organic,vuln,invu,friend,neutral"),
        AURA_REGEN_MANA => (BUFF_REGEN_MANA, "ground,air,organic,vuln,invu"),
        AURA_SLOW => (BUFF_AURA_SLOW, "air,ground,enemy,vuln,invu"),
        AURA_WAR_DRUMS => (BUFF_WAR_DRUMS, "air,ground,friend,self,vuln,invu"),
        _ => return None,
    })
}

/// Order ids of this batch (cast getters verified by `tools/analysis/ability_orders.py`).
pub const ORDER_ABSORB: u32 = 852_529;
pub const ORDER_LOAD: u32 = 852_046;
pub const ORDER_DROP: u32 = 852_048;
pub const ORDER_DROP_INSTANT: u32 = 852_049;
pub const ORDER_DEVOUR: u32 = 852_104;
pub const ORDER_DISENCHANT: u32 = 852_495;
pub const ORDER_DETONATE: u32 = 852_145;
pub const ORDER_BERSERK: u32 = 852_100;
pub const ORDER_ANTI_MAGIC: u32 = 852_186;
pub const ORDER_CONTROL_MAGIC: u32 = 852_474;

fn nonzero(x: F32) -> bool {
    x.to_bits() & 0x7FFF_FFFF != 0
}

impl Game {
    /// Keeps the counters and emitters of this batch in step with the abilities of `unit` (called from `sync_auras`, so
    /// at creation and whenever abilities are added or removed).
    pub fn sync_unit_abilities_a(&mut self, unit: ObjKey) {
        // Ethereal (CONFIRMED): init slot 0x80 adds one to unit+0x158, the removal slot subtracts one.
        let has = self.unit(unit).abilities.get(ETHEREAL).is_some_and(|a| !a.is_removed());
        let counted = self.spells.unit_a.eth.contains(&unit);
        if has && !counted {
            self.spells.unit_a.eth.insert(unit);
            self.unit_mut(unit).ethereal += 1;
        } else if !has && counted {
            self.spells.unit_a.eth.remove(&unit);
            self.unit_mut(unit).ethereal -= 1;
        }
        for base in [AURA_BLIGHT_REGEN, AURA_REGEN_MANA, AURA_SLOW, AURA_WAR_DRUMS] {
            let has = self.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed());
            let running = self.spells.unit_a.emitters.contains_key(&(unit, base));
            if has && !running {
                let id = self.timers.start(
                    SCAN_PERIOD,
                    true,
                    Payload::Spell(SpellMsg::UnitSpell { kind: KIND_AURA_SCAN, unit, base, aux: 0 }),
                );
                self.spells.unit_a.emitters.insert((unit, base), id);
                self.a_aura_apply(unit, base, unit);
            } else if !has && running {
                if let Some(id) = self.spells.unit_a.emitters.remove(&(unit, base)) {
                    self.timers.cancel(id);
                }
            }
        }
        self.sync_cargo_and_detect(unit);
        self.sync_ghosts(unit);
    }

    /// Applies aura `base` of `source` to `target` (the shared apply of `CAbilityAura`, `auras.rs`): source gates,
    /// the `targs` filter, then a buff of the class's code with the 4.0 s duration and the level's strength.
    fn a_aura_apply(&mut self, source: ObjKey, base: u32, target: ObjKey) {
        let Some((buff, targs)) = aura_def(base) else {
            return;
        };
        {
            let s = self.unit(source);
            if s.dead || s.removed || s.loaded {
                return;
            }
            let t = self.unit(target);
            if t.dead || t.removed {
                return;
            }
            // An illusion source emits to itself only (`0x6F043770`).
            if source != target && s.illusion {
                return;
            }
        }
        let Some(level) = self.ability_level_record(source, base) else {
            return;
        };
        if !targs_ok(self, source, target, targs_word(targs)) {
            return;
        }
        if self.unit(target).illusion {
            // `0x6F1ACF90`: the effect of an aura buff is suspended on an illusion.
            return;
        }
        let d = level.data;
        let mut preds = BuffPredicates { aura: true, ..buff_predicates(buff) };
        let mods = match base {
            AURA_BLIGHT_REGEN => {
                preds.positive = true;
                let amount = if nonzero(d[1]) { arith::mul(d[0], self.unit(target).max_life) } else { d[0] };
                UnitMods { life_regen: amount, ..UnitMods::default() }
            }
            AURA_REGEN_MANA => {
                preds.positive = true;
                let amount = if nonzero(d[1]) { arith::mul(d[0], self.unit(target).max_mana) } else { d[0] };
                UnitMods { mana_regen: amount, ..UnitMods::default() }
            }
            AURA_SLOW => {
                preds.negative = true;
                UnitMods { move_delta: d[0], attack_delta: d[1], ..UnitMods::default() }
            }
            _ => {
                // War Drums: the Command aura damage bonus. The original adds `round(DataA * base damage)` to the
                // dice bonus (`auras.rs`); this model scales the raw damage by `DataA` instead (UNSURE).
                preds.positive = true;
                UnitMods { damage_pct: d[0], ..UnitMods::default() }
            }
        };
        self.apply_mods_buff(target, buff, preds, mods, crate::auras::AURA_BUFF_DURATION);
    }

    fn a_aura_scan(&mut self, unit: ObjKey, base: u32) {
        let has = self.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed());
        if self.unit(unit).removed || !has {
            if let Some(id) = self.spells.unit_a.emitters.remove(&(unit, base)) {
                self.timers.cancel(id);
            }
            return;
        }
        let Some(level) = self.ability_level_record(unit, base) else {
            return;
        };
        let (x, y) = (self.unit(unit).x, self.unit(unit).y);
        for t in crate::natives::group::circle_candidates(self, x, y, level.area) {
            self.a_aura_apply(unit, base, t);
        }
    }
}

/// Order ids this batch adds: `(order id, ability)`.
pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    let _ = ids;
}

/// Abilities of this batch cast without a target.
pub(crate) fn takes_no_target(base: u32) -> bool {
    let _ = base;
    false
}

/// Extra target rules (called after the earlier modules' rules).
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    let _ = (gm, caster, base, target);
    None
}

/// The class effects of this batch. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    let _ = (game, rt, rec);
    false
}

/// Immediate orders of this batch (toggles, unloading). `None` when `id` is not one of them.
pub(crate) fn immediate_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    let _ = (game, unit, id);
    None
}

/// Timer dispatch for `kind >= 0x40`.
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, base: u32, aux: u32) {
    let _ = (rt, aux);
    let mut gm = game.borrow_mut();
    if kind == KIND_AURA_SCAN {
        gm.a_aura_scan(unit, base);
    }
}

impl Game {
    fn sync_cargo_and_detect(&mut self, _unit: ObjKey) {}
    fn sync_ghosts(&mut self, _unit: ObjKey) {}
}

#[allow(dead_code)]
fn unused(_: CastTarget, _: i32) -> i32 {
    conv::f2i(F32::ZERO)
}
