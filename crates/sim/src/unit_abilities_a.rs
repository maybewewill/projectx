//! Unit abilities, batch A (S46). Spec: `docs/spec/abilities-unit-a.md`.
//!
//! Only codes with modelled behaviour are listed in [`ABILITIES`] (RTTI class and parent from the ability atlas
//! `docs/research/ability-atlas/factory.tsv`, what the model does, and the confidence). The codes of the batch that
//! have no model yet are in the not-done list of the spec.
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
}

/// Confidence of the model of one code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conf {
    Confirmed,
    Likely,
    Unsure,
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
    Info { code: code(b"Aakb"), class: "CAbilityAuraKotoBeast", parent: "CAbilityAura", sim: Sim::Aura, conf: Conf::Likely, note: "CBuffAuraKotoBeast < CBuffAuraCommand; buff Bakb; damage bonus DataA (fraction of the base damage)" },
    Info { code: code(b"Aams"), class: "CAbilityAntiMagicShell", parent: "CAbilitySimpleSpell", sim: Sim::Spell, conf: Conf::Unsure, note: "CAbilityAntiMagicShell: spell immunity counter for Dur, buff Bams" },
    Info { code: code(b"Aarm"), class: "CAbilityAuraRegenMana", parent: "CAbilityAura", sim: Sim::Aura, conf: Conf::Likely, note: "CAbilityAura; buff Barm (getter 0x6F022DC0); mana regen DataA, a fraction of max mana when DataB != 0" },
    Info { code: code(b"Aasl"), class: "CAbilityAuraSlow", parent: "CAbilityAura", sim: Sim::Aura, conf: Conf::Likely, note: "CAbilityAura, CBuffAuraSlow < CBuffAuraEndurance; buff Basl; movement DataA, attack speed DataB" },
    Info { code: code(b"Aave"), class: "CAbilityAvengerForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityAvengerForm < CAbilityMorph: UnitID target form, DataA alternate form" },
    Info { code: code(b"Abdt"), class: "CAbilityBurrowDetector", parent: "CAbilityDetector", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityBurrowDetector < CAbilityDetector (burrowed units)" },
    Info { code: code(b"Abrf"), class: "CAbilityBearForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityBearForm < CAbilityMorph" },
    Info { code: code(b"Absk"), class: "CAbilityBerserkerRage", parent: "CAbilitySpell", sim: Sim::Spell, conf: Conf::Unsure, note: "CAbilityBerserkerRage: attack speed +DataA, movement +DataB for Dur (damage-taken part DataC not modelled)" },
    Info { code: code(b"Abun"), class: "CAbilityBunker", parent: "CAbilityCargoHold", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityBunker < CAbilityCargoHold: burrow, capacity DataA 4" },
    Info { code: code(b"Abur"), class: "CAbilityBurrow", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityBurrow < CAbilityMorph" },
    Info { code: code(b"Acar"), class: "CAbilityCargoHold", parent: "CPower", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoHold: capacity DataA; holds loaded units" },
    Info { code: code(b"Achd"), class: "CAbilityCargoHoldDeath", parent: "CAbilityPurge", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoHoldDeath: holder whose cargo is released when it dies; capacity DataA 10" },
    Info { code: code(b"Acmg"), class: "CAbilityControlMagic", parent: "CAbilitySimpleSpell", sim: Sim::Spell, conf: Conf::Unsure, note: "CAbilityControlMagic order 852474: owner change of a summoned or low level target" },
    Info { code: code(b"Acpf"), class: "CAbilityCorporealForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityCorporealForm < CAbilityMorph" },
    Info { code: code(b"Adch"), class: "CAbilityDisenchant", parent: "CAbilityDispelMagic", sim: Sim::Spell, conf: Conf::Likely, note: "CAbilityDisenchant < CAbilityDispelMagic: runs the Dispel Magic effect (DataB 300 summon damage)" },
    Info { code: code(b"Adet"), class: "CAbilityDetector", parent: "CAbilityPassive", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityDetector: Rng detection radius; marks invisible enemies as detected for the owner" },
    Info { code: code(b"Adev"), class: "CAbilityDevour", parent: "CAbilityCargoLoad", sim: Sim::Cargo, conf: Conf::Unsure, note: "CAbilityDevour < CAbilityCargoLoad: order 852104; swallows the target into the devour hold (DataA 5 = digestion damage per second UNSURE)" },
    Info { code: code(b"Adri"), class: "CAbilityCargoDropInstant", parent: "CAbilitySpell", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoDropInstant: unload all, order 852049" },
    Info { code: code(b"Adro"), class: "CAbilityCargoDrop", parent: "CAbilitySimpleSpell", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityCargoDrop: unload at point, order 852048" },
    Info { code: code(b"Adtn"), class: "CAbilityDetonate", parent: "CAbilityDispelMagic", sim: Sim::Spell, conf: Conf::Likely, note: "CAbilityDetonate < CAbilityDispelMagic: dispel effect, then the caster dies" },
    Info { code: code(b"Adts"), class: "CAbilityMagicSentry", parent: "CAbilityDetector", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityMagicSentry < CAbilityDetector" },
    Info { code: code(b"Advc"), class: "CAbilityDevourCargo", parent: "CAbilityCargoHold", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityDevourCargo < CAbilityCargoHold: devour stomach, capacity DataA 2" },
    Info { code: code(b"Aenc"), class: "CAbilityEntangleCargo", parent: "CAbilityCargoHold", sim: Sim::Cargo, conf: Conf::Likely, note: "CAbilityEntangleCargo < CAbilityCargoHold: entangled gold mine, capacity DataA 5" },
    Info { code: code(b"Aetf"), class: "CAbilityEtherealForm", parent: "CAbilityMorph", sim: Sim::Morph, conf: Conf::Likely, note: "CAbilityEtherealForm < CAbilityMorph" },
    Info { code: code(b"Aeth"), class: "CAbilityGhostVisible", parent: "CAbility", sim: Sim::Hook, conf: Conf::Unsure, note: "CAbilityGhostVisible: lets the unit see ghosts (DataA/DataB flags); hook only" },
    Info { code: code(b"Aetl"), class: "CAbilityEthereal", parent: "CAbilityPassive", sim: Sim::Passive, conf: Conf::Confirmed, note: "init slot 0x80 -> 0x6F07A340 adds one to the ethereal counter (+0x158), removal 0x6F0796A0 subtracts" },
    Info { code: code(b"Afzy"), class: "CAbilityFrenzy", parent: "CAbilityBloodlust", sim: Sim::Spell, conf: Conf::Likely, note: "CAbilityFrenzy < CAbilityBloodlust: self Bloodlust numbers, buff Bfzy" },
    Info { code: code(b"Agho"), class: "CAbilityGhost", parent: "CAbilityPermInvis", sim: Sim::Hook, conf: Conf::Unsure, note: "CAbilityGhost < CAbilityPermInvis: permanent invisibility; hook only (S44 invisibility.rs owns the rules)" },
    Info { code: code(b"Agyv"), class: "CAbilityGyroVision", parent: "CAbilityDetector", sim: Sim::Detector, conf: Conf::Likely, note: "CAbilityGyroVision < CAbilityDetector" },
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
    /// Running cargo upkeep timers (death and cleanup check), by holder.
    cargo_timers: BTreeMap<ObjKey, TimerId>,
    /// Running digestion timers, by devourer.
    digesters: BTreeMap<ObjKey, TimerId>,
    /// Anti-magic shell expiry timers by unit (one magic immunity count each).
    shells: BTreeMap<ObjKey, TimerId>,
    /// The form a transformed unit came from (the alternate order returns to it).
    morph_back: BTreeMap<ObjKey, u32>,
    /// Alternate forms by type, for units placed already transformed.
    morph_alt: BTreeMap<u32, u32>,
}

fn aura_def(base: u32) -> Option<(u32, &'static str)> {
    Some(match base {
        AURA_BLIGHT_REGEN => (BUFF_BLIGHT_REGEN, "ground,air,organic,vuln,invu,friend,neutral"),
        AURA_REGEN_MANA => (BUFF_REGEN_MANA, "ground,air,organic,vuln,invu"),
        AURA_SLOW => (BUFF_AURA_SLOW, "air,ground,enemy,vuln,invu"),
        AURA_WAR_DRUMS => (BUFF_WAR_DRUMS, "air,ground,friend,self,vuln,invu"),
        crate::unit_abilities_m::AURA_REGEN_LIFE => (crate::unit_abilities_m::BUFF_REGEN_LIFE, "ground,air,organic,vuln,invu,friend,neutral"),
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
        for base in [AURA_BLIGHT_REGEN, AURA_REGEN_MANA, AURA_SLOW, AURA_WAR_DRUMS, crate::unit_abilities_m::AURA_REGEN_LIFE] {
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
        self.sync_unit_abilities_m(unit);
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
            AURA_BLIGHT_REGEN | crate::unit_abilities_m::AURA_REGEN_LIFE => {
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
    for (id, base) in [
        (ORDER_LOAD, LOAD),
        (ORDER_DROP, DROP),
        (ORDER_DROP_INSTANT, DROP_INSTANT),
        (ORDER_DEVOUR, DEVOUR),
    ] {
        ids.insert(id, base);
    }
    register_spell_orders(ids);
}

/// Abilities of this batch cast without a target.
pub(crate) fn takes_no_target(base: u32) -> bool {
    matches!(base, DROP | DROP_INSTANT | BERSERK | FRENZY)
}

/// Extra target rules (called after the earlier modules' rules).
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    match base {
        LOAD => {
            // targs: ground,friend,invu,vuln (`Aloa` row).
            let w = targs_word("ground,friend,invu,vuln");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            (!gm.can_load(caster, target)).then_some(CastError::BadTarget)
        }
        DEVOUR => {
            let w = targs_word("ground,nonhero,enemy,organic,neutral");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(CastError::BadTarget);
            }
            (!gm.can_load(caster, target)).then_some(CastError::BadTarget)
        }
        _ => spell_target_error(gm, caster, base, target),
    }
}

/// The class effects of this batch. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        LOAD | DEVOUR => {
            if let CastTarget::Unit(t) = rec.target {
                let mut gm = game.borrow_mut();
                if gm.load_unit(rec.caster, t) && rec.base == DEVOUR {
                    gm.start_digestion(rec.caster, t);
                }
            }
            true
        }
        DROP | DROP_INSTANT => {
            let mut gm = game.borrow_mut();
            match (rec.base, rec.target) {
                (DROP, CastTarget::Unit(t)) => {
                    gm.unload_unit(rec.caster, t);
                }
                _ => gm.unload_all(rec.caster),
            }
            true
        }
        _ => spell_effect(game, rt, rec),
    }
}

/// Immediate orders of this batch (toggles, unloading). `None` when `id` is not one of them.
pub(crate) fn immediate_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    morph_order(game, unit, id)
}

/// Timer dispatch for `kind >= 0x40`.
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, base: u32, aux: u32) {
    match kind {
        KIND_AURA_SCAN => game.borrow_mut().a_aura_scan(unit, base),
        KIND_DETECT => game.borrow_mut().recompute_detection(),
        KIND_SHELL => game.borrow_mut().shell_expired(unit),
        KIND_MORPH => game.borrow_mut().morph_finished(unit, aux),
        KIND_CARGO => {
            let dead = game.borrow_mut().cargo_upkeep(unit);
            for (victim, killer) in dead {
                crate::natives::dispatch_unit_death_by(game, rt, victim, killer);
            }
        }
        KIND_DIGEST => {
            let dead = game.borrow_mut().digest_tick(unit);
            for (victim, killer) in dead {
                crate::natives::dispatch_unit_death_by(game, rt, victim, killer);
            }
        }
        _ => crate::unit_abilities_m::on_timer(game, rt, kind, unit, base, aux),
    }
}

// ---- cargo (Acar, Abun, Aenc, Advc, Achd, Aloa, Adro, Adri, Adev) ----

pub const CARGO_HOLD: u32 = code(b"Acar");
pub const BUNKER: u32 = code(b"Abun");
pub const ENTANGLE_CARGO: u32 = code(b"Aenc");
pub const DEVOUR_CARGO: u32 = code(b"Advc");
pub const CARGO_DEATH: u32 = code(b"Achd");
pub const LOAD: u32 = code(b"Aloa");
pub const DROP: u32 = code(b"Adro");
pub const DROP_INSTANT: u32 = code(b"Adri");
pub const DEVOUR: u32 = code(b"Adev");
pub const BUFF_DIGEST: u32 = code(b"Bdig");
pub const BUFF_DEVOURED: u32 = code(b"Bdvv");
pub const KIND_CARGO: u8 = 0x44;
const HOLDS: [u32; 5] = [CARGO_HOLD, BUNKER, ENTANGLE_CARGO, DEVOUR_CARGO, CARGO_DEATH];
/// The upkeep period of the cargo list (dead holders and passengers are noticed within it; a model choice).
const CARGO_PERIOD: f32 = 0.5;
/// The digestion tick of a devour hold (1 s, the period of `CBuffDoT` in `unit_spells.rs`).
const DIGEST_PERIOD: f32 = 1.0;
/// The ring the unloaded units are put on, in unit directions (UNSURE: the original searches free spots).
const RING: [(f32, f32); 8] = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)];

impl Game {
    /// The cargo capacity of `holder`: the `DataA` (number of units) of its hold abilities (LIKELY: editor labels
    /// "Cargo Hold: capacity"; the rows give 4 for the burrow, 5 for the entangled mine, 2 for the devour stomach).
    pub fn cargo_capacity(&self, holder: ObjKey) -> i32 {
        HOLDS
            .iter()
            .filter(|&&c| self.unit(holder).abilities.get(c).is_some_and(|a| !a.is_removed()))
            .map(|&c| conv::f2i(self.spell_level(holder, c).data[0]).max(0))
            .sum()
    }

    /// The units carried by `holder`, in load order.
    pub fn cargo_of(&self, holder: ObjKey) -> &[ObjKey] {
        self.spells.unit_a.cargo.get(&holder).map_or(&[], |v| v.as_slice())
    }

    /// The holder `unit` is inside.
    pub fn carrier_of(&self, unit: ObjKey) -> Option<ObjKey> {
        self.spells.unit_a.carried.get(&unit).copied()
    }

    /// Whether `holder` can take `unit` now.
    pub fn can_load(&self, holder: ObjKey, unit: ObjKey) -> bool {
        if holder == unit || !self.alive(holder) || !self.alive(unit) {
            return false;
        }
        if self.spells.unit_a.carried.contains_key(&unit) || self.unit(unit).loaded {
            return false;
        }
        let u = self.unit(unit);
        if crate::natives::unit_attr::is_unit_type(self, u, crate::unitclass::ty::STRUCTURE) {
            return false;
        }
        (self.cargo_of(holder).len() as i32) < self.cargo_capacity(holder)
    }

    /// Puts `unit` into `holder`. The passenger takes the holder's position and carries the loaded flag
    /// (`IsUnitLoaded`); it is not simulated while inside (the aura emitters stop on a loaded source, CONFIRMED in
    /// `auras.rs`). Returns whether the load happened.
    pub fn load_unit(&mut self, holder: ObjKey, unit: ObjKey) -> bool {
        if !self.can_load(holder, unit) {
            return false;
        }
        self.spells.unit_a.cargo.entry(holder).or_default().push(unit);
        self.spells.unit_a.carried.insert(unit, holder);
        let (x, y) = (self.unit(holder).x, self.unit(holder).y);
        self.store_unit_position(unit, x, y);
        self.unit_mut(unit).loaded = true;
        if !self.spells.unit_a.cargo_timers.contains_key(&holder) {
            let id = self.timers.start(
                CARGO_PERIOD,
                true,
                Payload::Spell(SpellMsg::UnitSpell { kind: KIND_CARGO, unit: holder, base: 0, aux: 0 }),
            );
            self.spells.unit_a.cargo_timers.insert(holder, id);
        }
        true
    }

    fn drop_record(&mut self, holder: ObjKey, unit: ObjKey) {
        if let Some(v) = self.spells.unit_a.cargo.get_mut(&holder) {
            v.retain(|&k| k != unit);
        }
        self.spells.unit_a.carried.remove(&unit);
        self.spells.unit_a.digest.remove(&unit);
        if self.unit(unit).loaded {
            self.unit_mut(unit).loaded = false;
        }
        self.remove_buff_code(unit, BUFF_DEVOURED);
        if self.spells.unit_a.cargo.get(&holder).is_some_and(|v| v.is_empty()) {
            self.spells.unit_a.cargo.remove(&holder);
            if let Some(id) = self.spells.unit_a.cargo_timers.remove(&holder) {
                self.timers.cancel(id);
            }
            if let Some(id) = self.spells.unit_a.digesters.remove(&holder) {
                self.timers.cancel(id);
            }
            self.remove_buff_code(holder, BUFF_DIGEST);
        }
    }

    /// Takes `unit` out of `holder` and puts it on the ring around the holder, `n` slots in.
    fn unload_at(&mut self, holder: ObjKey, unit: ObjKey, n: usize) {
        let (hx, hy) = (self.unit(holder).x, self.unit(holder).y);
        self.drop_record(holder, unit);
        let (dx, dy) = RING[n % 8];
        let radius = 96.0 + 48.0 * (n / 8) as f32;
        let x = arith::add(hx, F32::from_bits((dx * radius).to_bits()));
        let y = arith::add(hy, F32::from_bits((dy * radius).to_bits()));
        self.store_unit_position(unit, x, y);
    }

    /// Unloads one passenger. Returns whether it was inside.
    pub fn unload_unit(&mut self, holder: ObjKey, unit: ObjKey) -> bool {
        if self.carrier_of(unit) != Some(holder) {
            return false;
        }
        self.unload_at(holder, unit, 0);
        true
    }

    /// Unloads every passenger (`Adri`).
    pub fn unload_all(&mut self, holder: ObjKey) {
        let all: Vec<ObjKey> = self.cargo_of(holder).to_vec();
        for (n, k) in all.into_iter().enumerate() {
            self.unload_at(holder, k, n);
        }
    }

    /// The periodic check of a cargo list: passengers that died or were removed leave it; when the holder died or
    /// vanished the cargo is freed (`Achd`, the death hold: the passengers die with it; UNSURE) or unloaded around the
    /// holder's last position. Returns the units to run the death dispatch for.
    fn cargo_upkeep(&mut self, holder: ObjKey) -> Vec<(ObjKey, Option<ObjKey>)> {
        let mut dead = Vec::new();
        let list: Vec<ObjKey> = self.cargo_of(holder).to_vec();
        for &p in &list {
            if !self.alive(p) {
                self.drop_record(holder, p);
            }
        }
        if !self.alive(holder) {
            let kills = self.unit(holder).abilities.get(CARGO_DEATH).is_some();
            let rest: Vec<ObjKey> = self.cargo_of(holder).to_vec();
            if kills {
                for p in rest {
                    self.drop_record(holder, p);
                    if crate::natives::set_life(self, p, F32::ZERO) {
                        dead.push((p, None));
                    }
                }
            } else {
                for (n, p) in rest.into_iter().enumerate() {
                    self.unload_at(holder, p, n);
                }
            }
        }
        if self.cargo_of(holder).is_empty() {
            if let Some(id) = self.spells.unit_a.cargo_timers.remove(&holder) {
                self.timers.cancel(id);
            }
        }
        dead
    }

    /// Starts the digestion of a devoured unit: the buffs `Bdvv` (devoured) and `Bdig` (digesting) and the 1 s damage
    /// tick of the devourer (`Advc` `DataB`, taken as the damage per second; UNSURE).
    fn start_digestion(&mut self, devourer: ObjKey, victim: ObjKey) {
        let per_second = self.spell_level(devourer, DEVOUR_CARGO).data[1];
        self.spells.unit_a.digest.insert(victim, (devourer, per_second));
        let neg = BuffPredicates { negative: true, ..buff_predicates(BUFF_DEVOURED) };
        self.apply_mods_buff(victim, BUFF_DEVOURED, neg, UnitMods::default(), F32::ZERO);
        let pos = buff_predicates(BUFF_DIGEST);
        self.apply_mods_buff(devourer, BUFF_DIGEST, pos, UnitMods::default(), F32::ZERO);
        if !self.spells.unit_a.digesters.contains_key(&devourer) {
            let id = self.timers.start(
                DIGEST_PERIOD,
                true,
                Payload::Spell(SpellMsg::UnitSpell { kind: KIND_DIGEST, unit: devourer, base: 0, aux: 0 }),
            );
            self.spells.unit_a.digesters.insert(devourer, id);
        }
    }

    fn digest_tick(&mut self, devourer: ObjKey) -> Vec<(ObjKey, Option<ObjKey>)> {
        let mut dead = Vec::new();
        let victims: Vec<(ObjKey, F32)> = self
            .spells
            .unit_a
            .digest
            .iter()
            .filter(|(_, (d, _))| *d == devourer)
            .map(|(&k, &(_, a))| (k, a))
            .collect();
        for (v, amount) in victims {
            if !self.alive(v) {
                continue;
            }
            let life = self.unit(v).life;
            let left = arith::sub(life, amount);
            if !arith::lt(F32::ZERO, left) {
                self.drop_record(devourer, v);
                if crate::natives::set_life(self, v, F32::ZERO) {
                    dead.push((v, Some(devourer)));
                }
            } else {
                self.write_life(v, left);
            }
        }
        dead
    }

    fn sync_cargo_and_detect(&mut self, unit: ObjKey) {
        self.sync_detect(unit);
        // A holder that lost its last hold ability lets its passengers out.
        if !self.cargo_of(unit).is_empty() && self.cargo_capacity(unit) == 0 {
            self.unload_all(unit);
        }
    }


}

// ---- detection and the invisibility hooks (Adet, Adts, Agyv, Abdt, Agho, Aeth) ----

pub const DETECTOR: u32 = code(b"Adet");
pub const MAGIC_SENTRY: u32 = code(b"Adts");
pub const GYRO_VISION: u32 = code(b"Agyv");
pub const BURROW_DETECTOR: u32 = code(b"Abdt");
pub const GHOST: u32 = code(b"Agho");
pub const GHOST_VISIBLE: u32 = code(b"Aeth");
const DETECTORS: [u32; 4] = [DETECTOR, MAGIC_SENTRY, GYRO_VISION, BURROW_DETECTOR];
/// The detector rescan period (a model choice; the original updates on movement and on a timer: UNSURE).
const DETECT_PERIOD: f32 = 0.5;

impl Game {
    fn detector_range(&self, unit: ObjKey) -> Option<F32> {
        let mut best: Option<F32> = None;
        for c in DETECTORS {
            if self.unit(unit).abilities.get(c).is_some_and(|a| !a.is_removed()) {
                let r = self.spell_level(unit, c).rng;
                best = Some(match best {
                    Some(b) if arith::lt(r, b) => b,
                    _ => r,
                });
            }
        }
        best
    }

    /// Recomputes the detection masks from every living detector: each unit within a detector's `Rng` is detected by
    /// the detector's player. (`CAbilityDetector`, `Rng` = the detection radius of the rows, e.g. 900 for the Magic
    /// Sentry, 1100 for the Sentry Ward; the bitmask is the per-player reveal the invisibility module reads.)
    fn recompute_detection(&mut self) {
        let detectors: Vec<ObjKey> = self.spells.unit_a.detectors.keys().copied().collect();
        let mut masks: BTreeMap<ObjKey, u16> = BTreeMap::new();
        for d in detectors {
            if !self.alive(d) || self.unit(d).loaded {
                continue;
            }
            let Some(r) = self.detector_range(d) else {
                continue;
            };
            let (x, y, owner) = (self.unit(d).x, self.unit(d).y, self.unit(d).owner);
            for t in crate::natives::group::circle_candidates(self, x, y, r) {
                let u = self.unit(t);
                if u.removed || u.dead {
                    continue;
                }
                *masks.entry(t).or_insert(0) |= 1u16 << owner.min(15);
            }
        }
        self.spells.unit_a.detected = masks;
    }

    /// Whether `player` currently detects `unit` (some detector of the player has it in range).
    pub fn unit_detected_by(&self, unit: ObjKey, player: u32) -> bool {
        self.spells.unit_a.detected.get(&unit).is_some_and(|m| m & (1u16 << player.min(15)) != 0)
    }

    /// Whether `unit` is permanently invisible through `Agho` (hook for the invisibility module, S44).
    pub fn is_ghost(&self, unit: ObjKey) -> bool {
        self.spells.unit_a.ghosts.contains(&unit)
    }

    /// Whether `unit` carries `Aeth` (sees ghosts, hook).
    pub fn sees_ghosts(&self, unit: ObjKey) -> bool {
        self.spells.unit_a.ghost_seers.contains(&unit)
    }

    /// Whether a ghost is visible to `player`: its own player, a detecting player, or a player with a ghost seer
    /// (hook: the sight and attack rules of the invisibility module combine this with the fog).
    pub fn ghost_visible_to(&self, unit: ObjKey, player: u32) -> bool {
        !self.is_ghost(unit) || self.unit(unit).owner == player || self.unit_detected_by(unit, player)
    }

    fn sync_detect(&mut self, unit: ObjKey) {
        let has = self.detector_range(unit).is_some();
        let running = self.spells.unit_a.detectors.contains_key(&unit);
        if has && !running {
            let id = self.timers.start(
                DETECT_PERIOD,
                true,
                Payload::Spell(SpellMsg::UnitSpell { kind: KIND_DETECT, unit, base: 0, aux: 0 }),
            );
            self.spells.unit_a.detectors.insert(unit, id);
            self.recompute_detection();
        } else if !has && running {
            if let Some(id) = self.spells.unit_a.detectors.remove(&unit) {
                self.timers.cancel(id);
            }
            self.recompute_detection();
        }
    }

    fn sync_ghosts(&mut self, unit: ObjKey) {
        let has = |g: &Game, c: u32| g.unit(unit).abilities.get(c).is_some_and(|a| !a.is_removed());
        let ghost = has(self, GHOST);
        let seer = has(self, GHOST_VISIBLE);
        if ghost {
            self.spells.unit_a.ghosts.insert(unit);
        } else {
            self.spells.unit_a.ghosts.remove(&unit);
        }
        if seer {
            self.spells.unit_a.ghost_seers.insert(unit);
        } else {
            self.spells.unit_a.ghost_seers.remove(&unit);
        }
    }
}

// ---- spells: Absorb, Disenchant, Detonate, Frenzy, Berserk, Anti-magic Shell, Control Magic ----

pub const ABSORB: u32 = code(b"Aabs");
pub const DISENCHANT: u32 = code(b"Adch");
pub const DETONATE: u32 = code(b"Adtn");
pub const FRENZY: u32 = code(b"Afzy");
pub const BERSERK: u32 = code(b"Absk");
pub const ANTI_MAGIC: u32 = code(b"Aams");
pub const ANTI_MAGIC_INSTANT: u32 = code(b"Aami");
pub const CONTROL_MAGIC: u32 = code(b"Acmg");
pub const BUFF_FRENZY: u32 = code(b"Bfzy");
pub const BUFF_BERSERK: u32 = code(b"Bbsk");
pub const BUFF_CONTROL_MAGIC: u32 = code(b"Bcmg");
pub const KIND_SHELL: u8 = 0x45;
pub const ORDER_FRENZY: u32 = 852_561;

/// Registers the spell order ids (called by [`register_orders`]).
fn register_spell_orders(ids: &mut BTreeMap<u32, u32>) {
    for (id, base) in [
        (ORDER_ABSORB, ABSORB),
        (ORDER_DISENCHANT, DISENCHANT),
        (ORDER_DETONATE, DETONATE),
        (ORDER_FRENZY, FRENZY),
        (ORDER_BERSERK, BERSERK),
        (ORDER_ANTI_MAGIC, ANTI_MAGIC),
        (ORDER_CONTROL_MAGIC, CONTROL_MAGIC),
    ] {
        ids.insert(id, base);
    }
}

fn spell_target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    let bad = |w: &str| gm.unit(target).removed || !targs_ok(gm, caster, target, targs_word(w));
    match base {
        ABSORB if bad("player,vuln,invu") || target == caster => Some(CastError::BadTarget),
        FRENZY if target != caster => Some(CastError::BadTarget),
        ANTI_MAGIC if bad("air,ground,vuln,invu") => Some(CastError::BadTarget),
        CONTROL_MAGIC => {
            // targs air,ground,enemy,neutral,ward,organic; the editor label of `DataB` is "Mana per Summoned
            // Hitpoint" (WorldEditStrings CMG2), so only a summoned non-hero unit qualifies and the caster needs
            // `DataB * life` mana on top of the cost (LIKELY reading; the exact formula is not decoded).
            if bad("air,ground,enemy,neutral,ward,organic") || gm.target_hero(target) || !gm.unit(target).summoned {
                return Some(CastError::BadTarget);
            }
            let need = arith::mul(gm.spell_level(caster, CONTROL_MAGIC).data[1], gm.unit(target).life);
            arith::lt(gm.unit(caster).mana, need).then_some(CastError::NoMana)
        }
        _ => None,
    }
}

fn spell_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    let target = match rec.target {
        CastTarget::Unit(t) => t,
        _ => rec.caster,
    };
    match rec.base {
        ABSORB => {
            game.borrow_mut().absorb(rec.caster, target, &rec.level);
        }
        DISENCHANT | DETONATE => {
            // Both classes derive from `CAbilityDispelMagic` and override nothing but the codes (atlas): the Dispel
            // Magic effect runs on the row's numbers. Detonate (the wisp) dies afterwards (LIKELY).
            let parent = CastRec { base: crate::unit_spells::DISPEL_MAGIC, ..*rec };
            crate::unit_spells::run_effect(game, rt, &parent);
            if rec.base == DETONATE {
                let dead = {
                    let mut gm = game.borrow_mut();
                    gm.alive(rec.caster) && crate::natives::set_life(&mut gm, rec.caster, F32::ZERO)
                };
                if dead {
                    crate::natives::dispatch_unit_death_by(game, rt, rec.caster, None);
                }
            }
        }
        FRENZY => {
            // `CAbilityFrenzy < CAbilityBloodlust`: attack speed `DataA`, movement `DataB` (Bloodlust's fields),
            // buff Bfzy, self only (targs air,ground,self).
            let mut gm = game.borrow_mut();
            let hero = gm.target_hero(target);
            let dur = crate::cast::effect_duration(&rec.level, hero, F32::ZERO);
            if gm.alive(target) && arith::lt(F32::ZERO, dur) {
                let preds = BuffPredicates { positive: true, magic: true, dispellable: true, ..buff_predicates(BUFF_FRENZY) };
                let mods = UnitMods { attack_delta: rec.level.data[0], move_delta: rec.level.data[1], ..UnitMods::default() };
                gm.apply_mods_buff(target, BUFF_FRENZY, preds, mods, dur);
            }
        }
        BERSERK => {
            // WorldEditStrings BSK1..3: movement speed increase `DataA`, attack speed increase `DataB`, damage taken
            // increase `DataC` (the last one has no sink in the model and is not applied).
            let mut gm = game.borrow_mut();
            let hero = gm.target_hero(rec.caster);
            let dur = crate::cast::effect_duration(&rec.level, hero, F32::ZERO);
            if gm.alive(rec.caster) && arith::lt(F32::ZERO, dur) {
                let preds = BuffPredicates { positive: true, ..buff_predicates(BUFF_BERSERK) };
                let mods = UnitMods { move_delta: rec.level.data[0], attack_delta: rec.level.data[1], ..UnitMods::default() };
                gm.apply_mods_buff(rec.caster, BUFF_BERSERK, preds, mods, dur);
            }
        }
        ANTI_MAGIC | ANTI_MAGIC_INSTANT => {
            game.borrow_mut().anti_magic_shell(target, &rec.level);
        }
        CONTROL_MAGIC => {
            let changed = {
                let mut gm = game.borrow_mut();
                if !gm.alive(target) {
                    return true;
                }
                let need = arith::mul(rec.level.data[1], gm.unit(target).life);
                let mana = gm.unit(rec.caster).mana;
                gm.write_mana(rec.caster, arith::sub(mana, need));
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
                from.map(|f| (f, to))
            };
            if let Some((from, to)) = changed {
                crate::natives::event::fire_owner_change(game, rt, target, from, to);
            }
        }
        _ => return false,
    }
    true
}

impl Game {
    /// Absorb (`Aabs`, `CAbilityAbsorb`): moves mana from the target to the caster, at most `DataB` ("Maximum Mana
    /// Absorbed", WorldEditStrings ABS2) and at most what the caster can hold. `DataA` ("Maximum Life Absorbed", 0 in
    /// the stock row) is not modelled. LIKELY.
    fn absorb(&mut self, caster: ObjKey, target: ObjKey, level: &crate::cast::SpellLevel) {
        if !self.alive(target) || target == caster {
            return;
        }
        let free = arith::sub(self.unit(caster).max_mana, self.unit(caster).mana);
        let mut take = self.unit(target).mana;
        for cap in [level.data[1], free] {
            if arith::lt(cap, take) {
                take = cap;
            }
        }
        if !arith::lt(F32::ZERO, take) {
            return;
        }
        let (tm, cm) = (self.unit(target).mana, self.unit(caster).mana);
        self.write_mana(target, arith::sub(tm, take));
        self.write_mana(caster, arith::add(cm, take));
    }

    /// Anti-Magic Shell (`Aams`, `CAbilityAntiMagicShell`, buff `Bams`): the target has one magic immunity count for the
    /// duration; casting again restarts the timer without a second count. UNSURE: the data fields (summoned unit
    /// damage, magic damage reduction) are zero in the stock rows and are not used.
    fn anti_magic_shell(&mut self, target: ObjKey, level: &crate::cast::SpellLevel) {
        if !self.alive(target) {
            return;
        }
        let hero = self.target_hero(target);
        let dur = crate::cast::effect_duration(level, hero, F32::ZERO);
        if !arith::lt(F32::ZERO, dur) {
            return;
        }
        let preds = BuffPredicates { positive: true, magic: true, dispellable: true, ..buff_predicates(crate::unit_spells::BUFF_ANTI_MAGIC_SHELL) };
        self.apply_mods_buff(target, crate::unit_spells::BUFF_ANTI_MAGIC_SHELL, preds, UnitMods::default(), dur);
        if let Some(old) = self.spells.unit_a.shells.remove(&target) {
            self.timers.cancel(old);
        } else {
            self.unit_mut(target).magic_immune += 1;
        }
        let id = self.timers.start(
            dur.to_f32(),
            false,
            Payload::Spell(SpellMsg::UnitSpell { kind: KIND_SHELL, unit: target, base: ANTI_MAGIC, aux: 0 }),
        );
        self.spells.unit_a.shells.insert(target, id);
    }

    fn shell_expired(&mut self, unit: ObjKey) {
        if self.spells.unit_a.shells.remove(&unit).is_some() {
            self.unit_mut(unit).magic_immune -= 1;
        }
    }
}

// ---- transforms: Aave, Abrf, Acpf, Aetf, Abur (CAbilityMorph) ----

pub const AVENGER_FORM: u32 = code(b"Aave");
pub const BEAR_FORM: u32 = code(b"Abrf");
pub const CORPOREAL_FORM: u32 = code(b"Acpf");
pub const ETHEREAL_FORM: u32 = code(b"Aetf");
pub const BURROW: u32 = code(b"Abur");
/// `(ability, primary order, alternate order)`: the order getters of the classes (slots `0x308` and `0x318`, Unicorn
/// verified by `tools/analysis/ability_orders.py`). The primary order transforms into the level's `UnitID`, the
/// alternate order back into the earlier form.
pub const MORPH_ORDERS: [(u32, u32, u32); 9] = [
    (AVENGER_FORM, 852_531, 852_532),
    (BEAR_FORM, 852_138, 852_139),
    (CORPOREAL_FORM, 852_493, 852_494),
    (ETHEREAL_FORM, 852_496, 852_497),
    (BURROW, 852_533, 852_534),
    // Batch M (S52): the other `CAbilityMorph` classes. Phoenix has no alternate order (its alternate getter is unhandled).
    (crate::unit_abilities_m::RAVEN_FORM, 852_155, 852_156),
    (crate::unit_abilities_m::STONE_FORM, 852_206, 852_207),
    (crate::unit_abilities_m::MOUNT, 852_469, 852_470),
    (crate::unit_abilities_m::PHOENIX_MORPH, 852_482, 0),
];

fn morph_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    let &(base, primary, _) = MORPH_ORDERS.iter().find(|m| m.1 == id || m.2 == id)?;
    let mut gm = game.borrow_mut();
    if !gm.alive(unit) || gm.unit(unit).abilities.get(base).is_none_or(|a| a.is_removed()) {
        return Some(false);
    }
    if gm.spells.unit_a.morphing.contains_key(&unit) {
        return Some(false);
    }
    let level = gm.spell_level(unit, base);
    let to = if id == primary {
        if gm.spells.unit_a.morph_back.contains_key(&unit) {
            return Some(false);
        }
        level.unit_id
    } else {
        let ty = gm.unit(unit).type_id;
        match gm.spells.unit_a.morph_back.get(&unit).or_else(|| gm.spells.unit_a.morph_alt.get(&ty)) {
            Some(&t) => t,
            None => return Some(false),
        }
    };
    if to == 0 || gm.unit_types().get(to).is_none() {
        return Some(false);
    }
    // `Dur` is the transformation time (1.45 s for the burrow row); the unit changes type when it runs out.
    gm.spells.unit_a.morphing.insert(unit, (to, id));
    gm.timers.start(
        level.dur.to_f32().max(0.0),
        false,
        Payload::Spell(SpellMsg::UnitSpell { kind: KIND_MORPH, unit, base, aux: id }),
    );
    Some(true)
}

impl Game {
    /// Registers the other form of a unit type for a unit that was placed already transformed (the alternate order
    /// of a burrowed unit that never burrowed itself): `form -> original`.
    pub fn set_morph_alt(&mut self, form: u32, original: u32) {
        self.spells.unit_a.morph_alt.insert(form, original);
    }

    fn morph_finished(&mut self, unit: ObjKey, order: u32) {
        let Some((to, started)) = self.spells.unit_a.morphing.remove(&unit) else {
            return;
        };
        let _ = order;
        if !self.alive(unit) {
            return;
        }
        let primary = MORPH_ORDERS.iter().any(|m| m.1 == started);
        let from = self.unit(unit).type_id;
        if primary {
            self.spells.unit_a.morph_back.insert(unit, from);
            self.spells.unit_a.morph_alt.insert(to, from);
        } else {
            self.spells.unit_a.morph_back.remove(&unit);
        }
        self.morph_to(unit, to);
    }

    /// Changes the type of `unit` (a real transform, unlike Polymorph): life and mana keep their fractions, the
    /// type's data and ability list replace the old ones (the setup of `natives::unit::build`: order-data abilities,
    /// then the list; LIKELY, the original rebuilds the unit's type record the same way).
    pub fn morph_to(&mut self, unit: ObjKey, to: u32) {
        let Some(ty) = self.unit_types().get(to).cloned() else {
            return;
        };
        let (lf, mf) = {
            let u = self.unit(unit);
            let lf = if nonzero(u.max_life) { self.soft_div(u.life, u.max_life) } else { F32::ZERO };
            let mf = if nonzero(u.max_mana) { self.soft_div(u.mana, u.max_mana) } else { F32::ZERO };
            (lf, mf)
        };
        let old: Vec<u32> = self.unit(unit).abilities.iter().map(|a| a.base).collect();
        for b in old {
            self.unit_mut(unit).abilities.remove(b);
        }
        let mut list = Vec::new();
        if let Some(data) = self.orders().type_data(to) {
            for kind in &data.abilities {
                list.push((kind.base_code(), kind.base_code()));
            }
        }
        for &id in &ty.abilities {
            let base = self.spell_alias_base(id);
            if !list.iter().any(|&(b, _)| b == base) {
                list.push((base, id));
            }
        }
        for (base, id) in list {
            let ab = crate::abilities::Ability::aliased(base, id, self.ability_types().get(id).copied());
            self.unit_mut(unit).abilities.attach(ab);
        }
        {
            let u = self.unit_mut(unit);
            u.type_id = to;
            u.class_flags = ty.classification;
            u.move_speed = ty.move_speed;
        }
        self.write_max_life(unit, ty.max_life);
        self.write_life(unit, arith::mul(ty.max_life, lf));
        self.unit_mut(unit).max_mana = ty.max_mana;
        self.write_mana(unit, arith::mul(ty.max_mana, mf));
        self.sync_auras(unit);
    }
}
