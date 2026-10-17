//! Neutral and creep abilities (S42). Spec: `docs/spec/abilities-neutral.md`.
//!
//! ## How a creep version relates to the modelled ability
//!
//! The creep camps use ability rows of `AbilityData.slk` that are *aliases*: a row whose `code` column names a base
//! ability (`ACcl` has `code` `AOcl`, `ACbl` `Ablo`, `ACsw` `Aslo`). The game builds the base ability's class and reads
//! the level records of the alias row, so every creep Chain Lightning, Bloodlust or Slow is the hero / unit ability
//! with other numbers. The model does the same: a unit that gets an alias row in its `abilList` receives the modelled
//! ability (`Ability::base` = the modelled code) whose [`crate::abilities::Ability::source`] is the alias row, and the
//! level lookups (`Game::ability_level_record`) read the alias row's records. Everything that is keyed by the base code
//! (orders, the effect dispatch, the passive hooks) works unchanged. The engine fills the table from the `code` column
//! ([`standard_for_group`], `engine::objdata`).
//!
//! A few creep abilities have a class of their own that derives from a modelled class (RTTI parent, and the
//! overrides read from the machine code): [`STATIC_PARENTS`]. Their class overrides are presentation (the effect
//! spawn slot `0x42c` / `0x430`) or order ids, so they run the parent's effect (LIKELY, see the spec).
//!
//! Labels: CONFIRMED = read in the machine code, LIKELY = strong inference, UNSURE = a stand-in.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, F32};

use crate::abilities::{buff_predicates, code};
use crate::natives::game::{Game, ObjKey};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};

/// Envenomed Spears (`Aven`, `CAbilityVenomSpear`), Magic Immunity (`Amim`), Pulverize (`Awar`).
pub const VENOM_SPEAR: u32 = code(b"Aven");
pub const MAGIC_IMMUNITY: u32 = code(b"Amim");
pub const PULVERIZE: u32 = code(b"Awar");
/// S43: Cleaving Attack, Resistant Skin, Mana Shield, Cyclone, Silence, Charm, Howl of Terror.
pub const CLEAVE: u32 = code(b"ANca");
pub const RESISTANT_SKIN: u32 = code(b"Arsk");
pub const MANA_SHIELD: u32 = code(b"ANms");
pub const CYCLONE: u32 = code(b"Acyc");
pub const SILENCE: u32 = code(b"ANsi");
pub const CHARM: u32 = code(b"ANch");
pub const HOWL: u32 = code(b"ANht");
/// S44 neutral heroes: Beastmaster summons (`CAbilitySummon* < CAbilityWaterElemental`), Brewmaster Drunken Haze
/// (`CAbilityDrunkenHaze < CAbilitySilence`) and Drunken Brawler (`CAbilityDrunkenBrawler < CAbilityCriticalStrike`).
pub const SUMMON_GRIZZLY: u32 = code(b"ANsg");
pub const SUMMON_QUILBEAST: u32 = code(b"ANsq");
pub const SUMMON_EAGLE: u32 = code(b"ANsw");
pub const BUFF_GRIZZLY: u32 = code(b"BNsg");
pub const BUFF_QUILBEAST: u32 = code(b"BNsq");
pub const BUFF_EAGLE: u32 = code(b"BNsw");
pub const HAZE: u32 = code(b"ANdh");
pub const BUFF_HAZE: u32 = code(b"BNdh");
pub const BRAWLER: u32 = code(b"ANdb");
/// Firelord Summon Lava Spawn (`CAbilityLavaMonster < CAbilityWaterElemental`), Soul Burn (`CAbilitySoulBurn <
/// CAbilitySimpleSpell`), Alchemist Acid Bomb (`CAbilityAcidBomb < CAbilitySimpleSpell`), Sea Witch Forked Lightning.
pub const LAVA_SPAWN: u32 = code(b"ANlm");
pub const BUFF_LAVA_SPAWN: u32 = code(b"BNlm");
pub const SOUL_BURN: u32 = code(b"ANso");
pub const BUFF_SOUL_BURN: u32 = code(b"BNso");
pub const ACID_BOMB: u32 = code(b"ANab");
pub const BUFF_ACID_BOMB: u32 = code(b"BNab");
pub const FORKED_LIGHTNING: u32 = code(b"ANfl");
/// The damage type of the over-time and lightning damage of the neutral hero spells (the orc spell type).
const NEUTRAL_SPELL_DAMAGE_TYPE: u32 = 15;
pub const BUFF_MANA_SHIELD: u32 = code(b"BNms");
pub const BUFF_CYCLONE: u32 = code(b"Bcyc");
/// The second Cyclone buff (`CBuffCycloneTwo < CBuffCyclone`, the invulnerability half in this model).
pub const BUFF_CYCLONE_TWO: u32 = code(b"Bcy2");
pub const BUFF_SILENCE: u32 = code(b"BNsi");
pub const BUFF_HOWL: u32 = code(b"BNht");
/// Mana Shield on (`manashieldon`, slot `0x348`) and off (slot `0x318`) order ids
/// (`tools/analysis/ability_orders.py`).
pub const ORDER_MANA_SHIELD_ON: u32 = 852_589;
pub const ORDER_MANA_SHIELD_OFF: u32 = 852_590;
/// The damage type of the Cleaving Attack area hit (class word `0x20`, as Pulverize).
const CLEAVE_DAMAGE_TYPE: u32 = 5;
/// The poison buff of Envenomed Spears (`BuffID` `Bpoi`).
pub const BUFF_POISON: u32 = code(b"Bpoi");
/// The combat stream of the Pulverize roll (`sub_6F2853D0(…, 40)` at `0x6F170FF0`).
pub const PULVERIZE_STREAM: usize = 40;
/// Damage type of the poison tick (class word `0x800000`, `unit_spells::DAMAGE_TYPE_SLOW_POISON`).
const POISON_DAMAGE_TYPE: u32 = 23;
/// Damage type of the Pulverize area damage (class `0x20` = 1 << 5, ENHANCED).
const PULVERIZE_DAMAGE_TYPE: u32 = 5;

/// `(derived class code, modelled code it derives from, order id of the derived class)`. Parents are the RTTI
/// parents of the atlas (`docs/research/ability-atlas/factory.tsv`); the order ids are the Unicorn-checked cast getters
/// (`tools/analysis/ability_orders.py`, slot `0x348`).
pub const STATIC_PARENTS: [(u32, u32, u32); 9] = [
    // CAbilityInferno (neutral Pit Lord row) shares the layout of the Dreadlord's `AUin` (S50).
    (code(b"ANin"), code(b"AUin"), 852_232),
    // CAbilityNeutralHeal < CAbilityHeal: only the order ids (slots 0x348 / 0x308 / 0x318) differ.
    (code(b"Anhe"), code(b"Ahea"), ORDER_NEUTRAL_HEAL),
    // CAbilityCreepThunderBolt < CAbilityThunderBolt, no override but the codes.
    (code(b"ACtb"), code(b"AHtb"), 852_252),
    // CAbilityCreepThunderClap < CAbilityThunderClap; the only extra override (slot 0x430) spawns the effect.
    (code(b"ACtc"), code(b"AHtc"), 852_253),
    // CAbilityFireBolt < CAbilityThunderBolt; the only extra override (slot 0x42c) spawns the effect.
    (code(b"ANfb"), code(b"AHtb"), 852_231),
    // CAbilityBreathOfFire < CAbilityCarrionSwarm (overrides: AI hint slot 0x100 and the effect slot 0x42c).
    (code(b"ANbf"), code(b"AUcs"), 852_580),
    // CAbilityBreathOfFrost < CAbilityBreathOfFire (effect slot 0x42c only).
    (code(b"ACbf"), code(b"AUcs"), 852_560),
    // CAbilityCreepReincarnation < CAbilityReincarnation: slot 0x80 only adds the order ids 852314 / 852315.
    (code(b"ACrn"), code(b"AOre"), 0),
    // CAbilityRainOfFire < CAbilityBlizzard (Pit Lord): one override; the data layout is the Blizzard one (the editor
    // fields `HBZ1..6`), so the channelled wave engine of `human_heroes.rs` runs (S43, LIKELY).
    (code(b"ANrf"), code(b"AHbz"), 852_238),
];

/// Neutral Heal (`Anhe`, `CAbilityNeutralHeal < CAbilityHeal`): the cast, autocast-on and autocast-off order ids are the
/// only overrides (slots `0x348`, `0x308`, `0x318`).
pub const NEUTRAL_HEAL: u32 = code(b"Anhe");
const ORDER_NEUTRAL_HEAL: u32 = 852_248;
const ORDER_NEUTRAL_HEAL_ON: u32 = 852_249;
const ORDER_NEUTRAL_HEAL_OFF: u32 = 852_250;

/// The codes the model implements with level data (the hero, unit and creep spells and passives). The engine uses it to
/// decide which alias rows of a `code` group stand for a modelled ability.
pub const MODELLED: &[[u8; 4]] = &[
    *b"Aadm", *b"AEah", *b"AEar", *b"AEbl", *b"AEer", *b"AEev", *b"AEfn", *b"AEim", *b"Aens", *b"AEsh", *b"Aeye",
    *b"Afae", *b"Afbk", *b"Afra", *b"AHab", *b"AHad", *b"AHav", *b"AHbh", *b"AHbn", *b"AHbz", *b"AHdr", *b"AHds",
    *b"Ahea", *b"AHer", *b"AHfs", *b"AHhb", *b"AHmt", *b"AHpx", *b"AHre", *b"AHtb", *b"AHtc", *b"Ahwd", *b"AHwe",
    *b"Ainf", *b"Alsh", *b"AOac", *b"AOae", *b"AOcl", *b"AOcr", *b"AOeq", *b"AOfs", *b"AOhw", *b"AOhx", *b"AOmi",
    *b"AOre", *b"AOsf", *b"AOsh", *b"AOsw", *b"AOvd", *b"AOwk", *b"AOws", *b"AOww", *b"Aply", *b"Aprg", *b"Arai",
    *b"Arej", *b"Aroa", *b"Aslo", *b"Aspo", *b"Asta", *b"AUau", *b"AUav", *b"AUcs", *b"AUdc", *b"AUdd", *b"AUdp",
    *b"AUdr", *b"AUfa", *b"AUfn", *b"AUfu", *b"Auhf", *b"AUim", *b"AUin", *b"AUsl", *b"AUts", *b"Aweb", *b"Ablo",
    *b"Acri", *b"Acrs", *b"Adis", *b"Aven", *b"Amim", *b"Awar", *b"ANca", *b"Arsk", *b"ANms", *b"Acyc", *b"ANsi",
    *b"ANch", *b"ANht", *b"AHbz", *b"Aivs", *b"Apiv", *b"Agho", *b"Ashm", *b"ANsg", *b"ANsq", *b"ANsw", *b"ANdh", *b"ANdb", *b"ANlm", *b"ANso", *b"ANab", *b"ANfl",
    *b"ANtm", *b"ANdo", *b"ANcs", *b"ANhs", *b"ANst", *b"ANvc", *b"ANef", *b"ANcr", *b"ANrg", *b"ANeg", *b"ANsy", *b"ANto", *b"Atdg", *b"Atsp", *b"ANba", *b"ANbs", *b"ANic",
    *b"ANbr", *b"ANmr", *b"ANde", *b"ANpi", *b"ANrc", *b"ANfd", *b"ANwm", *b"ANpa", *b"ANcl", *b"ANdp", *b"ANin", *b"ANmo", *b"ANia",
];

fn is_modelled(c: u32) -> bool {
    MODELLED.iter().any(|m| u32::from_be_bytes(*m) == c)
        || crate::unit_abilities_a::ABILITIES.iter().any(|i| i.code == c)
        || crate::unit_abilities_m::CODES.iter().any(|(b, _, _, st)| {
            u32::from_be_bytes(**b) == c && matches!(st, crate::unit_abilities_m::Status::Modelled | crate::unit_abilities_m::Status::Query)
        })
}

/// The modelled ability the alias rows of one `code` group stand for, if any. `code_col` is the group's `code` column,
/// `ids` the rows that carry it. A static parent class wins, then a modelled row of the group (`AOsw` for the
/// `AOwd` group), then the code itself.
pub fn standard_for_group(code_col: u32, ids: &[u32]) -> Option<u32> {
    if let Some(&(_, parent, _)) = STATIC_PARENTS.iter().find(|p| p.0 == code_col) {
        return Some(parent);
    }
    if is_modelled(code_col) {
        return Some(code_col);
    }
    ids.iter().copied().find(|&i| is_modelled(i))
}

/// Whether an alias row is left to the item ability system (`AI..` rows are item abilities).
pub fn is_item_row(id: u32) -> bool {
    (id >> 24) == u32::from(b'A') && ((id >> 16) & 0xFF) == u32::from(b'I')
}

/// State of the neutral spells (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// `alias row -> modelled code`.
    alias: BTreeMap<u32, u32>,
    /// Units that carry one Magic Immunity count from `Amim`.
    immune: BTreeSet<ObjKey>,
    /// Pulverize hit counters (1 before any roll) and armed flags (`ability+0x20 & 0x200`).
    pulv_count: BTreeMap<ObjKey, i32>,
    pulv_armed: BTreeSet<ObjKey>,
    /// Mana Shield parameters `(DataA, DataB)` of the level the shield was cast at, by unit (valid while `BNms` exists).
    shield: BTreeMap<ObjKey, (F32, F32)>,
    /// State of the S50 batch (`neutral_heroes2.rs`).
    pub(crate) h2: crate::neutral_heroes2::State,
    /// The invisibility model (`invisibility.rs`).
    pub(crate) inv: crate::invisibility::State,
}

/// The order ids this module adds: `(order id, modelled code)`.
pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (_, parent, order) in STATIC_PARENTS {
        if order != 0 {
            ids.insert(order, parent);
        }
    }
    for (id, base) in [
        (ORDER_MANA_SHIELD_ON, MANA_SHIELD),
        (852_581, CHARM),
        (852_144, CYCLONE),
        (852_592, SILENCE),
        (852_588, HOWL),
        (852_585, HAZE),
        (852_594, SUMMON_GRIZZLY),
        (852_595, SUMMON_QUILBEAST),
        (852_596, SUMMON_EAGLE),
        (852_667, LAVA_SPAWN),
        (852_668, SOUL_BURN),
        (852_662, ACID_BOMB),
        (852_587, FORKED_LIGHTNING),
    ] {
        ids.insert(id, base);
    }
    crate::neutral_heroes2::register_orders(ids);
}

/// Abilities of this module cast without a target (Mana Shield, Howl of Terror).
pub(crate) fn takes_no_target(base: u32) -> bool {
    base == MANA_SHIELD || base == HOWL || is_summon(base) || crate::neutral_heroes2::takes_no_target(base)
}

fn is_summon(base: u32) -> bool {
    base == SUMMON_GRIZZLY || base == SUMMON_QUILBEAST || base == SUMMON_EAGLE || base == LAVA_SPAWN
}

/// The autocast toggle orders of Neutral Heal (`healon` / `healoff` of the neutral class). `None` for any other id.
pub(crate) fn toggle_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    if id == ORDER_MANA_SHIELD_OFF {
        // `manashieldoff` (slot `0x318`): the shield comes off (LIKELY: the buff is destroyed, `0x6F079CC0`).
        let mut gm = game.borrow_mut();
        if gm.unit(unit).abilities.get(MANA_SHIELD).is_none_or(|a| a.is_removed()) {
            return Some(false);
        }
        gm.remove_buff_code(unit, BUFF_MANA_SHIELD);
        return Some(true);
    }
    if let Some(r) = crate::neutral_forms::toggle_order(game, unit, id) {
        return Some(r);
    }
    if let Some(r) = crate::neutral_orbs::toggle_order(game, unit, id) {
        return Some(r);
    }
    if id != ORDER_NEUTRAL_HEAL_ON && id != ORDER_NEUTRAL_HEAL_OFF {
        return None;
    }
    let mut gm = game.borrow_mut();
    if !gm.unit(unit).abilities.contains(crate::unit_spells::HEAL) {
        return Some(false);
    }
    gm.set_autocast(unit, crate::unit_spells::HEAL, id == ORDER_NEUTRAL_HEAL_ON);
    Some(true)
}

/// One immunity count on (`0x6F07A3A0`, ECX = unit, argument = `flag`; CONFIRMED, **Unicorn V9**, `tools/emu/v9_emu.py` check
/// `immunity`, 400 sequences): `+0x15C` always goes up, `+0x160` only with the flag. Returns the new `(+0x15C, +0x160)` and whether
/// the "enter" work runs (side effect `0x6F2AB310`, removal of the purge debuff, the dispel walker): when `+0x15C` became 1, or the flag
/// is set and `+0x160` became 1. Magic Immunity, Avatar and Divine Shield pass the flag, Anti-Magic Shell and Sanctuary do not.
pub fn immunity_add(counts: (i32, i32), flag: bool) -> ((i32, i32), bool) {
    let c = counts.0.wrapping_add(1);
    let d = if flag { counts.1.wrapping_add(1) } else { counts.1 };
    ((c, d), c == 1 || (flag && d == 1))
}

/// One immunity count off (`0x6F077320`): the mirror of [`immunity_add`]; returns whether the "leave" work (`0x6F2AC3A0`) runs: when
/// `+0x15C` became 0, or the flag is set and `+0x160` became 0.
pub fn immunity_remove(counts: (i32, i32), flag: bool) -> ((i32, i32), bool) {
    let c = counts.0.wrapping_sub(1);
    let d = if flag { counts.1.wrapping_sub(1) } else { counts.1 };
    ((c, d), c == 0 || (flag && d == 0))
}

impl Game {
    /// Adds one magic immunity count to `unit` through [`immunity_add`]; the enter work dispels (`magic_immunity_dispel`).
    pub(crate) fn magic_immunity_add(&mut self, unit: ObjKey, flag: bool) {
        let u = self.unit_mut(unit);
        let ((c, d), entered) = immunity_add((u.magic_immune, u.magic_immune_flagged), flag);
        u.magic_immune = c;
        u.magic_immune_flagged = d;
        if entered {
            self.magic_immunity_dispel(unit);
        }
    }

    /// Removes one magic immunity count from `unit` through [`immunity_remove`] (the leave work is not modelled).
    pub(crate) fn magic_immunity_remove(&mut self, unit: ObjKey, flag: bool) {
        let u = self.unit_mut(unit);
        let ((c, d), _) = immunity_remove((u.magic_immune, u.magic_immune_flagged), flag);
        u.magic_immune = c;
        u.magic_immune_flagged = d;
    }

    /// Registers `alias` (an ability row) as an alias of the modelled ability `base`.
    pub fn set_spell_alias(&mut self, alias: u32, base: u32) {
        if alias != base {
            self.spells.neutral.alias.insert(alias, base);
        }
    }

    /// The modelled code a unit gets for the ability row `id` (`id` itself when it is no alias).
    pub fn spell_alias_base(&self, id: u32) -> u32 {
        self.spells.neutral.alias.get(&id).copied().unwrap_or(id)
    }

    /// Keeps the passive counters in step with the abilities a unit has (called with the aura sync, so on creation and
    /// whenever abilities are added or removed). Magic Immunity (`Amim`, `CAbilityMagicImmunity`): the init slot
    /// `0x80` (`0x6F110730`) calls the immunity increment `0x6F07A3A0` and the removal slot `0x6F110700` the
    /// decrement `0x6F077320` (CONFIRMED); the counter is the one `magic_immune` of Avatar and Divine Shield.
    pub fn sync_neutral_spells(&mut self, unit: ObjKey) {
        crate::neutral_forms::sync_neutral2(self, unit);
        crate::invisibility::sync(self, unit);
        crate::wander::sync(self, unit);
        let has = self.unit(unit).abilities.get(MAGIC_IMMUNITY).is_some_and(|a| !a.is_removed());
        let counted = self.spells.neutral.immune.contains(&unit);
        if has && !counted {
            self.spells.neutral.immune.insert(unit);
            self.magic_immunity_add(unit, true);
        } else if !has && counted {
            self.spells.neutral.immune.remove(&unit);
            self.magic_immunity_remove(unit, true);
        }
    }

    /// Envenomed Spears (`Aven`): `CAbilityVenomSpear < CAbilityPoisonAttack` has no override but its codes, so the
    /// on-hit buff is the poison attack of Slow Poison without the slow part (the `0x34C..0x354` overrides of `Aspo`
    /// add the slows). The buff is `Bpoi` (the level's first `BuffID`); `DataA` is the damage per second (class 23),
    /// `Dur` / `HeroDur` the duration, targets `air,ground,organic`. The stacking variant `Bpsd` (`DataD`) is not
    /// modelled (UNSURE, as for Slow Poison).
    pub(crate) fn neutral_on_hit(&mut self, attacker: ObjKey, target: ObjKey) {
        if !self.alive(target) {
            return;
        }
        if self.unit(attacker).abilities.get(VENOM_SPEAR).is_none_or(|a| a.is_removed()) {
            return;
        }
        let Some(level) = self.ability_level_record(attacker, VENOM_SPEAR) else {
            return;
        };
        if !crate::unit_spells::targs_ok(self, attacker, target, crate::unit_spells::targs_word("air,ground,organic")) {
            return;
        }
        let hero = self.target_hero(target);
        let existing = self.unit(target).buffs.iter().any(|b| b.base == BUFF_POISON);
        // `0x6F13F2F0` (CONFIRMED, V6 `PA` rows): the numbers come from the plan; an illusion attacker poisons for 0.
        let plan = poison_plan(self.unit(attacker).illusion, hero, existing, false, &level);
        let dur = plan.dur;
        if !arith::lt(F32::ZERO, dur) {
            return;
        }
        let preds = crate::abilities::BuffPredicates {
            negative: true,
            physical: true,
            ..buff_predicates(BUFF_POISON)
        };
        let serial = self.apply_mods_buff(target, BUFF_POISON, preds, crate::unit_spells::UnitMods::default(), dur);
        self.register_tick(target, serial, attacker, plan.a, false, POISON_DAMAGE_TYPE, true);
    }

    /// Pulverize swing start (`CAbilityWarStomp`, `Awar`, slot `0x124` = `0x6F170F90`, CONFIRMED structure): the
    /// effective chance is `count * PRD(DataA)` (`0x6F163160`: `I2R(count)` times the table value of `0x6F1D21F0`, the
    /// same pair as Critical Strike, `orc::effective_chance`); a stream-40 draw `d` arms the ability when
    /// `!(chance < d)` and resets the counter to 1, otherwise the counter grows by one. A target that the spell
    /// immunity test `0x6F28D750` refuses ends the handler before the roll (modelled as: magic immune target).
    pub fn neutral_swing_start(&mut self, unit: ObjKey) {
        self.spells.neutral.pulv_armed.remove(&unit);
        let alive = {
            let u = self.unit(unit);
            !u.dead && !u.removed
        };
        if !alive || self.unit(unit).abilities.get(PULVERIZE).is_none_or(|a| a.is_removed()) {
            return;
        }
        let Some(level) = self.ability_level_record(unit, PULVERIZE) else {
            return;
        };
        let count = *self.spells.neutral.pulv_count.get(&unit).unwrap_or(&1);
        let eff = crate::orc::effective_chance(level.data[0], count);
        let Some((table, streams)) = self.rng_parts() else {
            return;
        };
        let draw = streams.stream_mut(PULVERIZE_STREAM).random_fraction(table);
        // `*chance < draw` -> not armed (an ordered x87 compare, soft `lt`).
        if arith::lt(eff, draw) {
            self.spells.neutral.pulv_count.insert(unit, count.wrapping_add(1));
        } else {
            self.spells.neutral.pulv_armed.insert(unit);
            self.spells.neutral.pulv_count.insert(unit, 1);
        }
    }

    /// Whether Pulverize is armed for the coming hit of `unit` (tests and the host).
    pub fn pulverize_armed(&self, unit: ObjKey) -> bool {
        self.spells.neutral.pulv_armed.contains(&unit)
    }

    /// The Pulverize hit counter of a unit (1 before any roll).
    pub fn pulverize_count(&self, unit: ObjKey) -> i32 {
        *self.spells.neutral.pulv_count.get(&unit).unwrap_or(&1)
    }

    /// The execute point (slot `0x170` = `0x6F1775F0`, CONFIRMED): an armed ability clears its flag and damages the
    /// area around the CASTER with two rings `(DataC, DataB)` and `(DataD, 0.5 * DataB)` through the area body
    /// `0x6F2C31D0` (damage class `0x20`, ability word = the level's `targs`); a unit takes the amount of the smallest
    /// ring that contains it (collector `0x6F2C2E80`). The callback `0x6F171180` refuses the whole damage when the
    /// pulverizing unit is an illusion. Returns the units that died. The ring membership is the plain circle (the
    /// collision-adjusted test `0x6F476C40` is UNSURE, as for the other area spells).
    pub fn neutral_execute(&mut self, unit: ObjKey) -> Vec<ObjKey> {
        if !self.spells.neutral.pulv_armed.remove(&unit) {
            return Vec::new();
        }
        let Some(level) = self.ability_level_record(unit, PULVERIZE) else {
            return Vec::new();
        };
        if self.unit(unit).illusion || !self.alive(unit) {
            return Vec::new();
        }
        let (cx, cy) = {
            let u = self.unit(unit);
            (u.x, u.y)
        };
        let word = crate::unit_spells::targs_word("ground,enemy,neutral");
        let rings = pulverize_rings(level.data[1], level.data[2], level.data[3]);
        // The collector gives a unit the amount of the smallest ring that contains it; the query reaches the largest ring.
        let sets: Vec<BTreeSet<ObjKey>> = rings
            .iter()
            .map(|&(radius, _)| crate::natives::group::circle_candidates(self, cx, cy, radius).into_iter().collect())
            .collect();
        let reach = rings.iter().fold(F32::ZERO, |m, &(r, _)| if arith::lt(m, r) { r } else { m });
        let mut killed = Vec::new();
        for k in crate::natives::group::circle_candidates(self, cx, cy, reach) {
            {
                let u = self.unit(k);
                if u.removed || u.hidden {
                    continue;
                }
            }
            if !crate::orc::area_predicate(self, unit, k, word) {
                continue;
            }
            let mut best: Option<(F32, F32)> = None;
            for (i, &(radius, amount)) in rings.iter().enumerate() {
                if sets[i].contains(&k) && best.is_none_or(|(r, _)| arith::lt(radius, r)) {
                    best = Some((radius, amount));
                }
            }
            let Some((_, amount)) = best else { continue };
            if let Some(t) = self.unit_damage_target(unit, k, amount, false, false, 0, PULVERIZE_DAMAGE_TYPE) {
                killed.push(t);
            }
        }
        killed
    }
}

/// Dispatches the deaths of a Pulverize hit (the host side of [`Game::neutral_execute`] when the caller owns the game
/// cell).
pub fn dispatch_deaths(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, killer: ObjKey, killed: Vec<ObjKey>) {
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(killer));
    }
}

// ---- S43: more creep abilities (spec `docs/spec/abilities-neutral.md` section 6) ----

use crate::cast::{effect_duration, CastRec, CastTarget, SpellLevel};
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// Extra target rules of the S43 spells (called from `unit_spells::target_error`).
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<crate::cast::CastError> {
    use crate::cast::CastError::BadTarget;
    match base {
        CYCLONE => {
            // targs of the creep and hero rows: ground,enemy,neutral,organic.
            let w = targs_word("ground,enemy,neutral,organic");
            (gm.unit(target).removed || !targs_ok(gm, caster, target, w)).then_some(BadTarget)
        }
        HAZE | SOUL_BURN | ACID_BOMB | FORKED_LIGHTNING => {
            let w = targs_word("air,ground,enemy,organic,neutral");
            (gm.unit(target).removed || !targs_ok(gm, caster, target, w)).then_some(BadTarget)
        }
        CHARM => {
            let w = targs_word("air,ground,nonhero,enemy,neutral,organic");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(BadTarget);
            }
            // `0x6F1A2600` (CONFIRMED, V6): `f2i(DataA)` is the highest creep level that can be charmed; only a unit of the
            // `creeps` race is refused above it (`neutral_heroes2::creep_level_refuses`, Unicorn `CL` rows).
            let max = softfloat::conv::f2i(gm.spell_level(caster, CHARM).data[0]);
            crate::neutral_heroes2::creep_too_high(gm, target, max).then_some(BadTarget)
        }
        _ => crate::neutral_heroes2::target_error(gm, caster, base, target)
            .or_else(|| crate::unit_abilities_m::target_error(gm, caster, base, target)),
    }
}

/// The class effects of the S43 spells. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        MANA_SHIELD => {
            mana_shield(game, rec);
            true
        }
        CYCLONE => {
            if let CastTarget::Unit(t) = rec.target {
                cyclone(game, rec, t);
            }
            true
        }
        SILENCE => {
            silence(game, rec);
            true
        }
        HOWL => {
            howl(game, rec);
            true
        }
        CHARM => {
            if let CastTarget::Unit(t) = rec.target {
                charm(game, rt, rec, t);
            }
            true
        }
        HAZE => {
            haze(game, rec);
            true
        }
        SOUL_BURN => {
            if let CastTarget::Unit(t) = rec.target {
                soul_burn(game, rec, t);
            }
            true
        }
        ACID_BOMB => {
            if let CastTarget::Unit(t) = rec.target {
                acid_bomb(game, rec, t);
            }
            true
        }
        FORKED_LIGHTNING => {
            if let CastTarget::Unit(t) = rec.target {
                forked_lightning(game, rt, rec, t);
            }
            true
        }
        SUMMON_GRIZZLY | SUMMON_QUILBEAST | SUMMON_EAGLE | LAVA_SPAWN => {
            // `CAbilitySummon* < CAbilityWaterElemental` (one override each, the class code): the summon loop
            // `0x6F163390` creates `f2i(DataA)` units of the level's `UnitID` with a timed life of `Dur`.
            let buff = match rec.base {
                SUMMON_GRIZZLY => BUFF_GRIZZLY,
                SUMMON_QUILBEAST => BUFF_QUILBEAST,
                LAVA_SPAWN => BUFF_LAVA_SPAWN,
                _ => BUFF_EAGLE,
            };
            crate::human_heroes::summon(game, rec, buff);
            true
        }
        _ => crate::neutral_heroes2::run_effect(game, rt, rec) || crate::unit_abilities_m::run_effect(game, rt, rec),
    }
}

/// The hero-rule duration of a target (`HeroDur` for heroes, `Dur` otherwise).
fn duration_for(gm: &Game, level: &SpellLevel, target: ObjKey) -> F32 {
    effect_duration(level, gm.target_hero(target), F32::ZERO)
}

/// Mana Shield (`ANms`, `CAbilityManaShield < CAbilitySimpleSpell`, buff `CBuffManaShield < CBuff`): the cast puts the
/// buff `BNms` on the caster and gives it the two level numbers (`DataA`, `DataB`: the apply slot `0x354`
/// `0x6F1116E0` stores them in two property cells, CONFIRMED). The buff has no duration (it ends when the mana runs
/// out or the off order comes). The damage side is [`Game::neutral_absorb`].
fn mana_shield(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        positive: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_MANA_SHIELD)
    };
    gm.apply_mods_buff(rec.caster, BUFF_MANA_SHIELD, preds, UnitMods::default(), F32::ZERO);
    gm.spells.neutral.shield.insert(rec.caster, (rec.level.data[0], rec.level.data[1]));
}

/// The rings the Pulverize execute slot `0x6F1775F0` hands to the area body `0x6F2C31D0` (two rings, damage class
/// `0x20`, the ability's `targs` word, the callback `0x6F171180`): `(DataC, DataB)` and `(DataD, 0.5 * DataB)`
/// (the constant at `0x6FAAE4D0`). Rows `PR` of `v3_vectors.txt`.
pub fn pulverize_rings(data_b: F32, data_c: F32, data_d: F32) -> [(F32, F32); 2] {
    [(data_c, data_b), (data_d, arith::mul(data_b, F32::from_bits(0x3F00_0000)))]
}

/// What the Mana Shield damage hook `0x6F126DD0` does, in the order the original does it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShieldEvent {
    /// The buff is destroyed (`0x6F079CC0`).
    Remove,
    /// The damage record's amount is reduced by this many points (`0x6F255790`, bits of the float).
    Reduce(u32),
    /// This much mana is spent (`0x6F283DB0`, bits of the float).
    Spend(u32),
}

fn nf(v: F32) -> f32 {
    f32::from_bits(v.to_bits())
}

/// `0x6F022530`: `x / p` where `p` is a property cell's value; a zero `p` (x87 equal) answers 1.0 for `x == 0`, else
/// plus or minus the largest float (`0x6FAAE624` / `0x6FAAE628`).
pub fn shield_safe_div(x: F32, p: F32, div: &dyn Fn(F32, F32) -> F32) -> F32 {
    if nf(p) == 0.0 {
        if nf(x) == 0.0 {
            F32::from_bits(0x3F80_0000)
        } else if nf(x) > 0.0 {
            F32::from_bits(0x7F7F_FFFF)
        } else {
            F32::from_bits(0xFF7F_FFFF)
        }
    } else {
        div(x, p)
    }
}

/// The Mana Shield damage hook (`CBuffManaShield` slot `0x6F126DD0`): `share` is the buff's `+0xD8` value (`DataB`), `per`
/// the value of the cell at `+0xCC` (`DataA`, damage absorbed per mana point), `amount` the record's amount, `mana` the
/// unit's mana, `mana_after(spent)` the mana once the spend is done. `flags` is the record's flag word (`0x10` skips the
/// shield), `source` whether the record's source resolves. Rows `MS` of `v3_vectors.txt`.
#[allow(clippy::too_many_arguments)]
pub fn mana_shield_hit(
    flags: u32,
    source: bool,
    share: F32,
    per: F32,
    amount: F32,
    mana: F32,
    mana_after: &dyn Fn(F32) -> F32,
    div: &dyn Fn(F32, F32) -> F32,
) -> Vec<ShieldEvent> {
    if !source || flags & 0x10 != 0 {
        return Vec::new();
    }
    let mut covered = arith::mul(amount, share);
    let check = arith::sub(covered, F32::ZERO);
    let eps = f32::from_bits(0x3A83_126F);
    if eps > f32::from_bits(check.to_bits() & 0x7FFF_FFFF) {
        return Vec::new();
    }
    // The mana the whole share would cost; when the mana cannot pay it the covered part is what the mana buys.
    let need = shield_safe_div(covered, per, div);
    if nf(mana) < nf(need) {
        covered = arith::mul(mana, per);
    }
    let spent = shield_safe_div(covered, per, div);
    let mut events = Vec::new();
    if nf(covered) == 0.0 {
        events.push(ShieldEvent::Remove);
    }
    events.push(ShieldEvent::Reduce(covered.to_bits()));
    events.push(ShieldEvent::Spend(spent.to_bits()));
    let after = nf(mana_after(spent));
    if after <= 0.0 {
        events.push(ShieldEvent::Remove);
    }
    events
}

/// `0x6F255790(record, &x, &0.0)` on the record's first amount: when the amount is not above `x` (x87 compare; NaN takes
/// the other path) the amount becomes exactly 0.0, otherwise it is `amount - x` (soft subtraction). Rows `RR`.
pub fn record_reduce(amount: F32, x: F32) -> F32 {
    if nf(amount) <= nf(x) {
        F32::ZERO
    } else {
        arith::sub(amount, x)
    }
}

/// Cyclone (`Acyc`, `CAbilityCyclone`; buffs `Bcyc` / `Bcy2`, `CBuffCycloneTwo < CBuffCyclone`): the target is lifted for
/// the hero-rule duration, cannot act (the stun count) and cannot be hurt (the invulnerability count). The Misc key
/// `CycloneStasis` (read by the effect slot `0x6F1CCC70`) only decides whether the effect object freezes the target
/// in place; it has no simulation effect here. The split of the two halves between the two buffs is UNSURE.
fn cyclone(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    // `0x6F1CCB50` (CONFIRMED, V6 `CY` rows): before the buff goes on, the walker `0x6F079FE0` removes the target's physical and
    // then its magic buffs of both polarities (timed life and aura buffs stay, no dispellable test).
    for (physical, magic) in [(true, false), (false, true)] {
        let select = crate::abilities::BuffSelect {
            remove_positive: true,
            remove_negative: true,
            physical,
            magic,
            ..Default::default()
        };
        gm.remove_buffs(target, select);
    }
    let dur = duration_for(&gm, &rec.level, target);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    gm.apply_stun_as(BUFF_CYCLONE, target, dur);
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_CYCLONE_TWO)
    };
    gm.apply_mods_buff(target, BUFF_CYCLONE_TWO, preds, UnitMods { invuln: true, ..UnitMods::default() }, dur);
}

/// What the Cyclone effect `0x6F1CCC70` does before and while it puts the buff on (Unicorn `CY` rows of `tools/emu/v6_emu.py`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CyclonePlan {
    /// The two removal passes (physical buffs, then magic buffs; both polarities) run: false only for a target whose type kind
    /// (`0x6F29C240`) is 2.
    pub removes: bool,
    /// The buff creator: 0 none (a target with unit flag `0x100`, or kind 2), 1 = `0x6F1C9160` (taken when `DataA` is not zero,
    /// or NaN, and Misc `CycloneStasis` is 0), 2 = `0x6F1C9230`.
    pub creator: u8,
    /// `HeroDur` for a hero target, else `Dur` plus the ability's `+0x9C` term (soft add).
    pub dur: F32,
}

/// See [`CyclonePlan`].
pub fn cyclone_plan(hero: bool, level: &SpellLevel, extra: F32, stasis: i32, kind_is_2: bool, flag100: bool) -> CyclonePlan {
    let dur = effect_duration(level, hero, extra);
    if kind_is_2 {
        return CyclonePlan { removes: false, creator: 0, dur };
    }
    // x87 equality with 0.0: only an ordered equal compare is "zero" (NaN is unordered and counts as nonzero)
    let a = level.data[0].to_f32();
    let nonzero = !(a == 0.0);
    let creator = if flag100 {
        0
    } else if nonzero && stasis == 0 {
        1
    } else {
        2
    };
    CyclonePlan { removes: true, creator, dur }
}

/// Silence (`ANsi`, `CAbilitySilence`; buff `CBuffSilence < CBuffSpeed`, `BNsi`): every enemy air or ground organic
/// (or neutral) unit within `Area` of the target point gets the buff for the hero-rule duration; a silenced unit
/// cannot start a spell ([`Game::is_silenced`]). Units with spell immunity are skipped (the immunity test
/// `0x6F28D750`, LIKELY). `DataA` (the `silenceFlags` word, 8 in the data) is not decoded: every silence blocks all
/// spells here (UNSURE); the speed fields of the `CBuffSpeed` base (`NSI2..4`) are zero in 1.26a.
fn silence(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (cx, cy) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => return,
    };
    let word = targs_word("air,ground,enemy,organic,neutral");
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_SILENCE)
    };
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        {
            let u = gm.unit(k);
            if u.removed || u.hidden || u.dead || u.magic_immune > 0 {
                continue;
            }
        }
        if !targs_ok(&gm, rec.caster, k, word) {
            continue;
        }
        let dur = duration_for(&gm, &rec.level, k);
        if arith::lt(F32::ZERO, dur) {
            gm.apply_mods_buff(k, BUFF_SILENCE, preds, UnitMods::default(), dur);
        }
    }
}

/// Drunken Haze (`ANdh`, `CAbilityDrunkenHaze < CAbilitySilence`, buff `BNdh`; order 852585): the Silence area shape
/// around the target (a unit in the data: `Rng` 550, `Area` 200): every enemy air / ground organic or neutral unit in
/// the area gets `BNdh` for the hero-rule duration. The buff makes the unit miss with chance `DataB` (the Curse miss
/// walk, [`UnitMods::miss_chance`]) and slows its movement by `DataC` (`move_delta = -DataC`). The role of the two
/// numbers (miss `DataB` 0.45..0.8, slow `DataC` 0.15..0.5) is read from the tooltip order, not from a run: LIKELY /
/// UNSURE. Spell-immune units are skipped.
fn haze(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (cx, cy) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => return,
    };
    let word = targs_word("air,ground,enemy,organic,neutral");
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_HAZE)
    };
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        {
            let u = gm.unit(k);
            if u.removed || u.hidden || u.dead || u.magic_immune > 0 {
                continue;
            }
        }
        if !targs_ok(&gm, rec.caster, k, word) {
            continue;
        }
        let dur = duration_for(&gm, &rec.level, k);
        if arith::lt(F32::ZERO, dur) {
            let mods = UnitMods {
                miss_chance: rec.level.data[1],
                move_delta: arith::neg(rec.level.data[2]),
                ..UnitMods::default()
            };
            gm.apply_mods_buff(k, BUFF_HAZE, preds, mods, dur);
        }
    }
}

/// Soul Burn (`ANso`, `CAbilitySoulBurn < CAbilitySimpleSpell`, buff `BNso`; order 852668): the target takes `DataA`
/// damage per second for the hero-rule duration (first tick at 0.01 s, then every second, like the other over-time
/// buffs) and cannot cast (the silence of [`Game::is_silenced`]). The numbers: `DataA` 7.14 x 14 s = 100 total at
/// level 1 (CONFIRMED by the data, the role of `DataB` = 1 and `DataC` = 0.5 is not decoded). Structure LIKELY, the
/// silence UNSURE.
fn soul_burn(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let dur = duration_for(&gm, &rec.level, target);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_SOUL_BURN)
    };
    let serial = gm.apply_mods_buff(target, BUFF_SOUL_BURN, preds, UnitMods::default(), dur);
    gm.register_tick(target, serial, rec.caster, rec.level.data[0], false, NEUTRAL_SPELL_DAMAGE_TYPE, true);
}

/// Acid Bomb (`ANab`, `CAbilityAcidBomb < CAbilitySimpleSpell`, buff `BNab`; order 852662): the target loses `DataC`
/// armor and takes `DataD` damage per second for the hero-rule duration (editor values 3 / 4 / 5 and 5 / 10 / 15 over
/// 15 s). The assignment of the data fields is from the value shapes (UNSURE); `DataE` (3 / 6.5 / 10) and the
/// 4 overrides (effect, buff apply, AI hints) are not modelled.
fn acid_bomb(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) {
        return;
    }
    let dur = duration_for(&gm, &rec.level, target);
    if !arith::lt(F32::ZERO, dur) {
        return;
    }
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_ACID_BOMB)
    };
    let mods = UnitMods { armor: arith::neg(rec.level.data[2]), ..UnitMods::default() };
    let serial = gm.apply_mods_buff(target, BUFF_ACID_BOMB, preds, mods, dur);
    gm.register_tick(target, serial, rec.caster, rec.level.data[3], false, NEUTRAL_SPELL_DAMAGE_TYPE, true);
}

/// Forked Lightning (`ANfl`, `CAbilityForkedLightning < CAbilitySimpleSpell`; order 852587): the target and the nearest
/// other enemy units within `Area` of it, `DataB` units in all, take `DataA` damage each (editor: 85 / 160 / 250, 3
/// targets, `Area` 125, `Dur` 0.7 is the lightning's life). The real spell is a fork along a line (`DataC` 900 long, `DataD`
/// 300 wide); the area form is a stand-in (UNSURE).
fn forked_lightning(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let killed: Vec<ObjKey> = {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        let (cx, cy) = (gm.unit(target).x, gm.unit(target).y);
        let word = targs_word("air,ground,enemy,organic,neutral");
        let mut picks: Vec<(F32, ObjKey)> = Vec::new();
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
            if k == target {
                continue;
            }
            {
                let u = gm.unit(k);
                if u.removed || u.hidden || u.dead || u.magic_immune > 0 {
                    continue;
                }
            }
            if !targs_ok(&gm, rec.caster, k, word) {
                continue;
            }
            let u = gm.unit(k);
            let (dx, dy) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
            picks.push((arith::add(arith::mul(dx, dx), arith::mul(dy, dy)), k));
        }
        // Nearest first (strict `<`, equal distances keep the walk order).
        picks.sort_by(|a, b| {
            if arith::lt(a.0, b.0) {
                std::cmp::Ordering::Less
            } else if arith::lt(b.0, a.0) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let total = softfloat::conv::f2i(rec.level.data[1]).max(1) as usize;
        let mut victims = vec![target];
        victims.extend(picks.into_iter().map(|p| p.1).take(total - 1));
        let mut killed = Vec::new();
        for k in victims {
            let immune = gm.unit(k).magic_immune > 0;
            if immune {
                continue;
            }
            if let Some(t) = gm.unit_damage_target(rec.caster, k, rec.level.data[0], false, false, 0, NEUTRAL_SPELL_DAMAGE_TYPE) {
                killed.push(t);
            }
        }
        killed
    };
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// Howl of Terror (`ANht`, `CAbilityHowlOfTerror < CAbilityRoar`, buff `BNht`): the mirror of Roar: every enemy ground
/// or air unit within `Area` of the caster loses `DataA` (a fraction) of its damage for the hero-rule duration. The
/// sign and the relation come from the level's `targs` (`air,ground,enemy,neutral`); the buff base is the Roar one
/// (damage fraction sink). LIKELY.
fn howl(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (cx, cy) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y)
    };
    let word = targs_word("air,ground,enemy,neutral");
    let preds = crate::abilities::BuffPredicates {
        negative: true,
        magic: true,
        dispellable: true,
        ..buff_predicates(BUFF_HOWL)
    };
    for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
        {
            let u = gm.unit(k);
            if u.removed || u.hidden || u.dead || u.magic_immune > 0 {
                continue;
            }
        }
        if !targs_ok(&gm, rec.caster, k, word) {
            continue;
        }
        let dur = duration_for(&gm, &rec.level, k);
        if arith::lt(F32::ZERO, dur) {
            let mods = UnitMods { damage_pct: arith::neg(rec.level.data[0]), ..UnitMods::default() };
            gm.apply_mods_buff(k, BUFF_HOWL, preds, mods, dur);
        }
    }
}

/// What the Charm effect `0x6F1B13E0` does to a target (Unicorn `CE` rows of `tools/emu/v6_emu.py`): an illusion is killed
/// (its life setter, vtable `0x124`, gets 0.0), any other unit changes owner.
pub fn charm_kills(illusion: bool) -> bool {
    illusion
}

/// Charm (`ANch`, `CAbilityCharm`; the only extra overrides are the target rule and the effect spawn): the target
/// changes owner to the caster's owner for good (the owner change of `SetUnitOwner`), its orders are cleared. The
/// target rule (`DataA` = highest creep level) is in [`target_error`]. LIKELY.
fn charm(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let changed = {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        // `0x6F1B13E0` (CONFIRMED, V6 `CE` rows): an illusion target is killed (its life setter gets 0.0), every other
        // target changes owner.
        if charm_kills(gm.unit(target).illusion) {
            let died = crate::natives::unit::set_life(&mut gm, target, F32::ZERO);
            drop(gm);
            if died {
                dispatch_unit_death_by(game, rt, target, None);
            }
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
        from.map(|f| (f, to))
    };
    if let Some((from, to)) = changed {
        crate::natives::event::fire_owner_change(game, rt, target, from, to);
    }
}

/// Which buff path the poison attack apply takes (`0x6F13F2F0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoisonPath {
    /// The stacking variant is wanted but the ability flag `0x200` is set: nothing is applied.
    Skip,
    /// A new poison buff (`Bpoi`, creator `0x6F13ADC0`, apply slot `0x378`).
    New,
    /// The victim already carries one: its slot `0x380` refreshes it.
    Refresh,
    /// The stacking buff (creator `0x6F13AE90`, apply slot `0x35C`).
    Stacking,
}

/// The arguments the poison attack apply hands to the buff (Unicorn `PA` rows of `tools/emu/v6_emu.py`, 350 rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoisonPlan {
    pub path: PoisonPath,
    /// `HeroDur` for a hero victim, else `Dur` (no extra term).
    pub dur: F32,
    /// `DataA`, `DataB`, `DataC` of the level, all three 0.0 when the **attacker** is an illusion.
    pub a: F32,
    pub b: F32,
    pub c: F32,
    /// `f2i(DataD) & 8`, handed to the plain paths.
    pub flag8: i32,
    /// `f2i(DataD)`, handed to the stacking path; its low three bits select the stacking mode (0 = plain).
    pub d: i32,
}

/// See [`PoisonPlan`]. `existing` is whether the victim already has the plain poison buff (found through `0x6F0787D0`),
/// `flag200` the ability's flag `0x200` (set while a stacking buff is being applied; cleared at the end of the apply).
pub fn poison_plan(attacker_illusion: bool, victim_hero: bool, existing: bool, flag200: bool, level: &SpellLevel) -> PoisonPlan {
    let (a, b, c) = if attacker_illusion {
        (F32::ZERO, F32::ZERO, F32::ZERO)
    } else {
        (level.data[0], level.data[1], level.data[2])
    };
    let dur = if victim_hero { level.hero_dur } else { level.dur };
    let d = softfloat::conv::f2i(level.data[3]);
    let path = if d & 7 == 0 {
        if existing {
            PoisonPath::Refresh
        } else {
            PoisonPath::New
        }
    } else if flag200 {
        PoisonPath::Skip
    } else {
        PoisonPath::Stacking
    };
    PoisonPlan { path, dur, a, b, c, flag8: d & 8, d }
}

impl Game {
    /// Whether `unit` carries the Silence buff (it cannot start a spell).
    pub fn is_silenced(&self, unit: ObjKey) -> bool {
        self.unit(unit)
            .buffs
            .iter()
            .any(|b| b.base == BUFF_SILENCE || b.base == BUFF_SOUL_BURN || b.base == crate::neutral_heroes2::BUFF_DOOM)
    }

    /// Mana Shield (`CBuffManaShield` damage slot `0x6F126DD0`, structure CONFIRMED, numbers LIKELY): of an incoming
    /// damage `d` the share `DataB * d` goes to the mana at `DataA` damage per mana point. When the mana cannot pay it
    /// all, the mana pays what it can (to zero) and the buff is destroyed. Returns the damage that reaches the life.
    /// The record-flag test `& 0x10` of the original (damage that skips the shield) is not modelled.
    pub(crate) fn neutral_absorb(&mut self, target: ObjKey, amount: F32) -> F32 {
        let Some(&(per_mana, share)) = self.spells.neutral.shield.get(&target) else {
            return amount;
        };
        if !self.unit(target).buffs.iter().any(|b| b.base == BUFF_MANA_SHIELD) {
            self.spells.neutral.shield.remove(&target);
            return amount;
        }
        let mana = self.unit(target).mana;
        // The mana after the spend (the sink `0x6F283DB0` is a plain subtraction here: no mana modifiers in the model).
        let after = |spent: F32| arith::sub(mana, spent);
        let events = {
            let this = &*self;
            mana_shield_hit(0, true, share, per_mana, amount, mana, &after, &|a, b| this.soft_div(a, b))
        };
        let mut amount = amount;
        for e in events {
            match e {
                ShieldEvent::Reduce(x) => amount = record_reduce(amount, F32::from_bits(x)),
                ShieldEvent::Spend(x) => {
                    let m = self.unit(target).mana;
                    self.write_mana(target, arith::sub(m, F32::from_bits(x)));
                }
                ShieldEvent::Remove => {
                    self.remove_buff_code(target, BUFF_MANA_SHIELD);
                    self.spells.neutral.shield.remove(&target);
                }
            }
        }
        amount
    }

    /// Cleaving Attack (`ANca`, `CAbilityCleavingAttack < CAbilityPassive`; swing start `0x6F1B1440` arms it when the
    /// attack target passes `0x6F26D590`, execute slot `0x154` = `0x6F1B7310` CONFIRMED structure): the area body
    /// `0x6F2C31D0` around the attack's target damages the enemy ground units within `Area` with `DataA` times the
    /// attack's damage (class `0x20`, the level's `targs`); the callback `0x6F1B14F0` is taken to skip the main target
    /// (LIKELY). Returns the units that died.
    pub(crate) fn neutral_cleave(&mut self, attacker: ObjKey, target: ObjKey, damage: F32) -> Vec<ObjKey> {
        if self.unit(attacker).abilities.get(CLEAVE).is_none_or(|a| a.is_removed()) {
            return Vec::new();
        }
        let Some(level) = self.ability_level_record(attacker, CLEAVE) else {
            return Vec::new();
        };
        if !arith::lt(F32::ZERO, level.data[0]) || !self.alive(attacker) {
            return Vec::new();
        }
        let (cx, cy) = {
            let u = self.unit(target);
            (u.x, u.y)
        };
        let amount = arith::mul(level.data[0], damage);
        let word = targs_word("ground,enemy,neutral");
        let mut killed = Vec::new();
        for k in crate::natives::group::circle_candidates(self, cx, cy, level.area) {
            if k == target {
                continue;
            }
            {
                let u = self.unit(k);
                if u.removed || u.hidden || u.dead {
                    continue;
                }
            }
            if !crate::orc::area_predicate(self, attacker, k, word) {
                continue;
            }
            if let Some(t) = self.unit_damage_target(attacker, k, amount, false, false, 0, CLEAVE_DAMAGE_TYPE) {
                killed.push(t);
            }
        }
        killed
    }
}
