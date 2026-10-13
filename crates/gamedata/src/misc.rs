//! The `[Misc]` section: the gameplay constants of the attack cycle and the damage model
//! (`docs/spec/attack-damage.md`, sections 4, 5 and 7).
//!
//! Sources, in load order (the caller passes the layers in this order and a later layer overrides an earlier one,
//! key by key): `Units\MiscData.txt`, then `Units\MiscGame.txt`, then the map's `war3mapMisc.txt`.
//!
//! Keys and shapes (CONFIRMED by the disassembly of the builder `0x6F26D300` and the reader `0x6F009EE0`):
//! - `DefenseArmor`: one real, the armor constant `K`, read as element 0 of the key.
//! - `DamageBonusSpells`, `DamageBonusNormal`, `DamageBonusPierce`, `DamageBonusSiege`, `DamageBonusMagic`,
//!   `DamageBonusChaos`, `DamageBonusHero`: a comma list. Row = attack type (0 Spells, 1 Normal, 2 Pierce, 3 Siege,
//!   4 Magic, 5 Chaos, 6 Hero); column = armor type 0 to 7, element `column` of the list.
//! - `EtherealDamageBonus`: a comma list read as elements 0 to 6. Their place in the damage table is not in
//!   list order: see [`MiscData::ethereal_for_attack_type`].
//! - `ReactionDelay`: one real, seconds. `AttackHalfAngle`: one real, radians (meaning UNSURE, attack-cycle spec).
//!
//! Reading rules of the original (CONFIRMED by disassembly, see `docs/spec/attack-damage.md` section 4.1):
//! - the value of element `i` of a key is parsed with the S2R real parser (`0x6F6F0520`);
//! - a key that is absent, an element past the end of the list, and every error path return `0.0`
//!   (the reader's default `DAT_6FAAE470`, which is `0.0`). A missing `DamageBonus*` cell is therefore `0.0`,
//!   not a neutral `1.0`, and a missing `DefenseArmor` is `K = 0.0`;
//! - elements past the eighth of a `DamageBonus*` list are not read; a short list leaves its tail at `0.0`.
//!
//! Only the malformed-text case is not seen in the disassembly: S2R on a non-number. Here it is recorded as a note
//! and takes `0.0` (UNSURE what the original returns for such text).
//!
//! Economy keys (`docs/spec/economy-3.md`, sections 2 and 4; read by the same reader family):
//! - `UpkeepUsage`: a list of integers, the food thresholds of the upkeep tiers (row 0; UNSURE how the original lays
//!   out more than one race row in the key, so only the first row is read).
//! - `UpkeepGoldTax`, `UpkeepLumberTax`: a list of reals, the tax fraction of each tier (e.g. `0.3`).
//! - `TrainRefundRate`, `ResearchRefundRate`, `ReviveRefundRate`, `LowGoldAmount`: one real each.
//!
//! An absent key, or a malformed element, gives the reader's default `0.0`. So a missing `UpkeepUsage` is an
//! all-zero threshold row (tier 0 at any food). The engine uses the built-in economy defaults instead, but only
//! when no `[Misc]` layer exists at all.
//!
//! Reals go through [`crate::real::parse_real`]: the S2R rules when the Game.dll tables are given, `f64` rounding
//! otherwise. Integers (`UpkeepUsage`) follow `atoi`: leading blanks and a sign, then digits; text with no digits
//! gives 0 (UNSURE for malformed text).

use softfloat::{Tables, F32};

use crate::real::{parse_real, scalar};
use crate::Profile;

/// The section that holds the constants.
pub const MISC_SECTION: &str = "Misc";
/// Number of attack-type rows of the damage-type table.
pub const ATTACK_ROWS: usize = 7;
/// Number of armor-type columns of the damage-type table.
pub const ARMOR_COLUMNS: usize = 8;
/// Number of elements read from `EtherealDamageBonus` (the builder reads indices 0 to 6).
pub const ETHEREAL_ENTRIES: usize = 7;
/// For each attack type (row), the element of `EtherealDamageBonus` stored in its slot. CONFIRMED by the store
/// addresses of the builder: slots `0x6FAB42E4 + 4 * row` hold elements `5, 0, 1, 2, 3, 4, 6`.
pub const ETHEREAL_ELEMENT_BY_ROW: [usize; ATTACK_ROWS] = [5, 0, 1, 2, 3, 4, 6];
/// Number of upkeep tiers read from `UpkeepUsage`, `UpkeepGoldTax` and `UpkeepLumberTax` (CONFIRMED: 10 entries).
pub const UPKEEP_TIERS: usize = 10;
/// The `DamageBonus*` keys, in row order (binary order, spec section 7).
pub const DAMAGE_BONUS_KEYS: [&str; ATTACK_ROWS] = [
    "DamageBonusSpells",
    "DamageBonusNormal",
    "DamageBonusPierce",
    "DamageBonusSiege",
    "DamageBonusMagic",
    "DamageBonusChaos",
    "DamageBonusHero",
];

/// The typed Misc constants (see the module documentation for the sources and the defaults).
#[derive(Debug, Clone, PartialEq)]
pub struct MiscData {
    /// `DefenseArmor`, the armor constant `K` (`0.0` when absent, as in the original).
    pub defense_armor: F32,
    /// `damage_bonus[attack_row][armor_column]`.
    pub damage_bonus: [[F32; ARMOR_COLUMNS]; ATTACK_ROWS],
    /// `EtherealDamageBonus` elements 0 to 6, in list order (not in table order, see
    /// [`MiscData::ethereal_for_attack_type`]).
    pub ethereal_damage_bonus: [F32; ETHEREAL_ENTRIES],
    /// `ReactionDelay` in seconds.
    pub reaction_delay: F32,
    /// `ChanceToMiss`: the probability of the uphill miss of a missile (`0.0` when absent, as in the original;
    /// `docs/spec/missiles.md` section 9).
    pub chance_to_miss: F32,
    /// `AttackHalfAngle`, or `None` when no layer sets it.
    pub attack_half_angle: Option<F32>,
    /// The hero experience keys (`docs/spec/heroes.md`, section 2).
    pub hero: HeroMisc,
    /// The keys that took their default (absent, or present but rejected), in reading order.
    pub defaulted: Vec<&'static str>,
    /// Why a present value was rejected, one line each.
    pub notes: Vec<String>,
    /// How many of the layers had a `[Misc]` section. Zero means no Misc data at all (every key is a default).
    pub layers_with_misc: usize,
    /// `UpkeepUsage`: the food thresholds of tiers 0 to 9 (row 0). A zero entry ends the tier walk.
    pub upkeep_usage: [i32; UPKEEP_TIERS],
    /// `UpkeepGoldTax`: the gold tax fraction of each tier.
    pub upkeep_gold_tax: [F32; UPKEEP_TIERS],
    /// `UpkeepLumberTax`: the lumber tax fraction of each tier.
    pub upkeep_lumber_tax: [F32; UPKEEP_TIERS],
    /// `TrainRefundRate`: the fraction of a cancelled training's cost that is refunded. `0.0` when absent.
    pub train_refund_rate: F32,
    /// `ResearchRefundRate`. `0.0` when absent.
    pub research_refund_rate: F32,
    /// `ReviveRefundRate`. `0.0` when absent.
    pub revive_refund_rate: F32,
    /// `LowGoldAmount`, the mine threshold of the low-gold notice. `0.0` when absent.
    pub low_gold_amount: F32,
    /// `DayLength`: the seconds of one day cycle (the time-of-day rate divides by it, `docs/spec/vision.md` 13).
    /// `0.0` when absent, as the original's reader does.
    pub day_length: F32,
    /// `DayHours`: the hours of game time (state 2) that one day cycle covers; also the value at which the cycle wraps.
    /// `0.0` when absent.
    pub day_hours: F32,
    /// `Dawn`: the hour at which day starts (inclusive). `0.0` when absent.
    pub dawn: F32,
    /// `Dusk`: the hour at which night starts (exclusive). `0.0` when absent.
    pub dusk: F32,
    /// `BuildingAngle`: the orientation of the AI's building footprints in degrees (`0x6F020D30`; the engine passes
    /// it through `i2r(f2i(..))`). `0.0` when absent.
    pub building_angle: F32,
    /// `FrostMoveSpeedDecrease`: the movement slow of the Frost buff (Frost Nova), stock 0.5. `0.0` when absent.
    pub frost_move_speed_decrease: F32,
    /// `FrostAttackSpeedDecrease`: the attack-rate slow of the Frost buff, stock 0.25. `0.0` when absent.
    pub frost_attack_speed_decrease: F32,
}

impl MiscData {
    /// Reads the constants from the layers, in load order (a later layer overrides an earlier one).
    pub fn from_layers(layers: &[&Profile], tables: Option<&Tables>) -> MiscData {
        let mut merged = Profile::default();
        let mut layers_with_misc = 0;
        for layer in layers {
            if layer.sections().any(|s| s == MISC_SECTION) {
                layers_with_misc += 1;
            }
            merged.merge(layer);
        }
        let mut reader = Reader {
            profile: &merged,
            tables,
            defaulted: Vec::new(),
            notes: Vec::new(),
        };
        let defaults = MiscData::default_values();
        let defense_armor = reader
            .real("DefenseArmor")
            .unwrap_or(defaults.defense_armor);
        let mut damage_bonus = defaults.damage_bonus;
        for (row, key) in DAMAGE_BONUS_KEYS.iter().enumerate() {
            if let Some(values) = reader.list::<ARMOR_COLUMNS>(key) {
                damage_bonus[row] = values;
            }
        }
        let ethereal_damage_bonus = reader
            .list::<ETHEREAL_ENTRIES>("EtherealDamageBonus")
            .unwrap_or(defaults.ethereal_damage_bonus);
        let reaction_delay = reader
            .real("ReactionDelay")
            .unwrap_or(defaults.reaction_delay);
        let chance_to_miss = reader.real("ChanceToMiss").unwrap_or(defaults.chance_to_miss);
        let attack_half_angle = reader.real("AttackHalfAngle");
        let upkeep_usage = reader
            .ints::<UPKEEP_TIERS>("UpkeepUsage")
            .unwrap_or([0; UPKEEP_TIERS]);
        let upkeep_gold_tax = reader
            .list::<UPKEEP_TIERS>("UpkeepGoldTax")
            .unwrap_or([F32::ZERO; UPKEEP_TIERS]);
        let upkeep_lumber_tax = reader
            .list::<UPKEEP_TIERS>("UpkeepLumberTax")
            .unwrap_or([F32::ZERO; UPKEEP_TIERS]);
        let train_refund_rate = reader.real("TrainRefundRate").unwrap_or(F32::ZERO);
        let research_refund_rate = reader.real("ResearchRefundRate").unwrap_or(F32::ZERO);
        let revive_refund_rate = reader.real("ReviveRefundRate").unwrap_or(F32::ZERO);
        let low_gold_amount = reader.real("LowGoldAmount").unwrap_or(F32::ZERO);
        let day_length = reader.real("DayLength").unwrap_or(F32::ZERO);
        let day_hours = reader.real("DayHours").unwrap_or(F32::ZERO);
        let dawn = reader.real("Dawn").unwrap_or(F32::ZERO);
        let dusk = reader.real("Dusk").unwrap_or(F32::ZERO);
        let building_angle = reader.real("BuildingAngle").unwrap_or(F32::ZERO);
        let frost_move_speed_decrease = reader.real("FrostMoveSpeedDecrease").unwrap_or(F32::ZERO);
        let frost_attack_speed_decrease = reader.real("FrostAttackSpeedDecrease").unwrap_or(F32::ZERO);
        let hero = HeroMisc::read(&mut reader);
        let Reader {
            defaulted, notes, ..
        } = reader;
        MiscData {
            defense_armor,
            damage_bonus,
            ethereal_damage_bonus,
            reaction_delay,
            chance_to_miss,
            attack_half_angle,
            hero,
            defaulted,
            notes,
            layers_with_misc,
            upkeep_usage,
            upkeep_gold_tax,
            upkeep_lumber_tax,
            train_refund_rate,
            research_refund_rate,
            revive_refund_rate,
            low_gold_amount,
            day_length,
            day_hours,
            dawn,
            dusk,
            building_angle,
            frost_move_speed_decrease,
            frost_attack_speed_decrease,
        }
    }

    /// The ethereal factor stored for an attack type (row `0..=6`), as the builder stores it in the table slot
    /// `0x6FAB42E4 + 4 * row`. Rows above 6 are not used by the original and return `0.0` here.
    pub fn ethereal_for_attack_type(&self, row: usize) -> F32 {
        match ETHEREAL_ELEMENT_BY_ROW.get(row) {
            Some(&element) => self.ethereal_damage_bonus[element],
            None => F32::ZERO,
        }
    }

    /// The defaults of every constant: what the original's reader returns for a missing key (`0.0`), except
    /// `AttackHalfAngle`, which has no default in the binary that was checked (`None`).
    fn default_values() -> MiscData {
        MiscData {
            defense_armor: F32::ZERO,
            damage_bonus: [[F32::ZERO; ARMOR_COLUMNS]; ATTACK_ROWS],
            ethereal_damage_bonus: [F32::ZERO; ETHEREAL_ENTRIES],
            reaction_delay: F32::ZERO,
            chance_to_miss: F32::ZERO,
            attack_half_angle: None,
            hero: HeroMisc::default(),
            defaulted: Vec::new(),
            notes: Vec::new(),
            layers_with_misc: 0,
            upkeep_usage: [0; UPKEEP_TIERS],
            upkeep_gold_tax: [F32::ZERO; UPKEEP_TIERS],
            upkeep_lumber_tax: [F32::ZERO; UPKEEP_TIERS],
            train_refund_rate: F32::ZERO,
            research_refund_rate: F32::ZERO,
            revive_refund_rate: F32::ZERO,
            low_gold_amount: F32::ZERO,
            day_length: F32::ZERO,
            day_hours: F32::ZERO,
            dawn: F32::ZERO,
            dusk: F32::ZERO,
            building_angle: F32::ZERO,
            frost_move_speed_decrease: F32::ZERO,
            frost_attack_speed_decrease: F32::ZERO,
        }
    }
}

/// The hero experience keys of `[Misc]` (`docs/spec/heroes.md`, section 2). CONFIRMED by the builder
/// `0x6F00ABD0` (Unicorn-checked, `tools/emu/hero_xp_emu.py`): the integer keys are read through the list reader
/// (element 0 for the scalars), the formula constants through the real reader with default `0.0`.
///
/// Integer elements are parsed as decimal integers (LIKELY: the original's integer reader; a malformed element is
/// `0`). A missing list is empty. The original reads the word before its table when a list is empty and the table
/// is not, which is not modelled, so the builder requires at least one element (see `sim::heroes`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeroMisc {
    /// `MaxHeroLevel` (element 0).
    pub max_hero_level: i32,
    /// `NeedHeroXP`: the experience thresholds of the first levels.
    pub need_hero_xp: Vec<i32>,
    /// `NeedHeroXPFormulaA`, `B`, `C`: the recurrence constants of the thresholds beyond the list.
    pub need_hero_xp_formula: [F32; 3],
    /// `GrantHeroXP`: experience granted for a hero of each level.
    pub grant_hero_xp: Vec<i32>,
    /// `GrantHeroXPFormulaA`, `B`, `C`.
    pub grant_hero_xp_formula: [F32; 3],
    /// `HeroFactorXP`: per-level factor, stored as the list value divided by 100 (the builder's column).
    pub hero_factor_xp: Vec<i32>,
    /// `MaxUnitLevel` (element 0).
    pub max_unit_level: i32,
    /// `GrantNormalXP`: experience granted for a normal unit of each level.
    pub grant_normal_xp: Vec<i32>,
    /// `GrantNormalXPFormulaA`, `B`, `C`.
    pub grant_normal_xp_formula: [F32; 3],
    /// The per-attribute bonus keys (`docs/spec/heroes.md`, section 9). Read by the derived-bonus readers.
    pub attr_bonus: AttrBonusMisc,
    /// `BuildingKillsGiveExp` (`0x6FAB4268`): a kill of a building gives experience. Read by the integer reader of
    /// the Misc loader `0x6F26E080` (key at `0x6F92F244`); absent gives `false` (LIKELY: the reader's default 0).
    pub building_kills_give_exp: bool,
    /// `GlobalExperience` (`0x6FA70E94`): the kill experience also goes to the global list (key at `0x6F92F25C`).
    /// Absent gives `false` (LIKELY, same reader). Only a non-zero value counts as on.
    pub global_experience: bool,
    /// `MaxLevelHeroesDrainExp` (`0x6FA70E98`): heroes at the maximum level still take kill experience (key at
    /// `0x6F92F270`). Absent gives `false` (LIKELY, same reader).
    pub max_level_heroes_drain_exp: bool,
    /// `HeroExpRange` (real, world units): the distance within which a hero shares a kill's experience. Read with the
    /// real reader `0x6F009EE0` (default 0.0).
    pub hero_exp_range: F32,
    /// `SummonedKillFactor` (real): the kill experience of a summoned unit is scaled by it (default 0.0).
    pub summoned_kill_factor: F32,
    /// `HeroAbilityLevelSkip` (integer, element 0): the level skip of a hero skill whose definition has none
    /// (`0x6F207B60`, default 0).
    pub hero_ability_level_skip: i32,
    /// The revive keys (`docs/spec/revive.md`).
    pub revive: ReviveMisc,
}

/// The `[Misc]` keys of the hero revive formulas (`docs/spec/revive.md`, readers `0x6F0340A0`, `0x6F030CF0`,
/// `0x6F059E20`). Reals use the real reader `0x6F009EE0` (absent gives 0.0), integers `0x6F009F90` (absent gives 0).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReviveMisc {
    /// `ReviveBaseFactor`.
    pub base_factor: F32,
    /// `ReviveLevelFactor`.
    pub level_factor: F32,
    /// `ReviveMaxFactor`.
    pub max_factor: F32,
    /// `ReviveBaseLumberFactor`.
    pub base_lumber_factor: F32,
    /// `ReviveLumberLevelFactor`.
    pub lumber_level_factor: F32,
    /// `ReviveTimeFactor`.
    pub time_factor: F32,
    /// `ReviveMaxTimeFactor`.
    pub max_time_factor: F32,
    /// `HeroMaxReviveTime` (seconds).
    pub max_time: F32,
    /// `HeroMaxReviveCostGold` (unsigned limit).
    pub max_cost_gold: i32,
    /// `HeroMaxReviveCostLumber`.
    pub max_cost_lumber: i32,
    /// `HeroReviveManaStart`: non-zero gives the type's starting mana on revive.
    pub mana_start: i32,
    /// `HeroReviveManaFactor`: revived mana as a fraction of the maximum when `mana_start` is 0.
    pub mana_factor: F32,
    /// `HeroReviveLifeFactor`: revived life as a fraction of the maximum.
    pub life_factor: F32,
}

/// The eight reals that turn a hero attribute into a derived bonus (`docs/spec/heroes.md`, section 9). Each is read
/// through the real reader `0x6F009EE0` with default `0.0`, so an absent key contributes nothing (CONFIRMED reader
/// rule). The keys are the ones the Game.dll readers `0x6F208060`, `0x6F2080F0`, `0x6F207F40`, `0x6F207FB0`,
/// `0x6F208170`, `0x6F2081F0` and `0x6F208270` read. `StrAttackBonus` is the only attack key: it multiplies the
/// hero's primary attribute (`sim::heroes::primary_total`), and the game reads no `AgiAttackBonus` or `IntRegenBonus`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttrBonusMisc {
    /// `StrHitPointBonus`: maximum life per point of strength (multiplies the strength total and a factor).
    pub str_hit_point_bonus: F32,
    /// `StrRegenBonus`: life regeneration per point of strength.
    pub str_regen_bonus: F32,
    /// `StrAttackBonus`: attack damage per point of the primary attribute.
    pub str_attack_bonus: F32,
    /// `AgiDefenseBase`: the armor added to the agility armor.
    pub agi_defense_base: F32,
    /// `AgiDefenseBonus`: armor per point of agility.
    pub agi_defense_bonus: F32,
    /// `AgiMoveBonus`: movement speed per point of agility.
    pub agi_move_bonus: F32,
    /// `AgiAttackSpeedBonus`: attack speed per point of agility.
    pub agi_attack_speed_bonus: F32,
    /// `IntManaBonus`: maximum mana per point of intelligence.
    pub int_mana_bonus: F32,
}

impl AttrBonusMisc {
    fn read(reader: &mut Reader<'_>) -> AttrBonusMisc {
        AttrBonusMisc {
            str_hit_point_bonus: reader.real_or_zero("StrHitPointBonus"),
            str_regen_bonus: reader.real_or_zero("StrRegenBonus"),
            str_attack_bonus: reader.real_or_zero("StrAttackBonus"),
            agi_defense_base: reader.real_or_zero("AgiDefenseBase"),
            agi_defense_bonus: reader.real_or_zero("AgiDefenseBonus"),
            agi_move_bonus: reader.real_or_zero("AgiMoveBonus"),
            agi_attack_speed_bonus: reader.real_or_zero("AgiAttackSpeedBonus"),
            int_mana_bonus: reader.real_or_zero("IntManaBonus"),
        }
    }
}

impl HeroMisc {
    fn read(reader: &mut Reader<'_>) -> HeroMisc {
        let max_hero_level = reader.int_element("MaxHeroLevel");
        let need_hero_xp = reader.int_list("NeedHeroXP");
        let need_hero_xp_formula = [
            reader.real_or_zero("NeedHeroXPFormulaA"),
            reader.real_or_zero("NeedHeroXPFormulaB"),
            reader.real_or_zero("NeedHeroXPFormulaC"),
        ];
        let grant_hero_xp = reader.int_list("GrantHeroXP");
        let grant_hero_xp_formula = [
            reader.real_or_zero("GrantHeroXPFormulaA"),
            reader.real_or_zero("GrantHeroXPFormulaB"),
            reader.real_or_zero("GrantHeroXPFormulaC"),
        ];
        let hero_factor_xp = reader.int_list("HeroFactorXP");
        let max_unit_level = reader.int_element("MaxUnitLevel");
        let grant_normal_xp = reader.int_list("GrantNormalXP");
        let grant_normal_xp_formula = [
            reader.real_or_zero("GrantNormalXPFormulaA"),
            reader.real_or_zero("GrantNormalXPFormulaB"),
            reader.real_or_zero("GrantNormalXPFormulaC"),
        ];
        let attr_bonus = AttrBonusMisc::read(reader);
        let building_kills_give_exp = reader.int_element("BuildingKillsGiveExp") != 0;
        let global_experience = reader.int_element("GlobalExperience") != 0;
        let max_level_heroes_drain_exp = reader.int_element("MaxLevelHeroesDrainExp") != 0;
        let hero_exp_range = reader.real_or_zero("HeroExpRange");
        let summoned_kill_factor = reader.real_or_zero("SummonedKillFactor");
        let hero_ability_level_skip = reader.int_element("HeroAbilityLevelSkip");
        let revive = ReviveMisc {
            base_factor: reader.real_or_zero("ReviveBaseFactor"),
            level_factor: reader.real_or_zero("ReviveLevelFactor"),
            max_factor: reader.real_or_zero("ReviveMaxFactor"),
            base_lumber_factor: reader.real_or_zero("ReviveBaseLumberFactor"),
            lumber_level_factor: reader.real_or_zero("ReviveLumberLevelFactor"),
            time_factor: reader.real_or_zero("ReviveTimeFactor"),
            max_time_factor: reader.real_or_zero("ReviveMaxTimeFactor"),
            max_time: reader.real_or_zero("HeroMaxReviveTime"),
            max_cost_gold: reader.int_element("HeroMaxReviveCostGold"),
            max_cost_lumber: reader.int_element("HeroMaxReviveCostLumber"),
            mana_start: reader.int_element("HeroReviveManaStart"),
            mana_factor: reader.real_or_zero("HeroReviveManaFactor"),
            life_factor: reader.real_or_zero("HeroReviveLifeFactor"),
        };
        HeroMisc {
            max_hero_level,
            need_hero_xp,
            need_hero_xp_formula,
            grant_hero_xp,
            grant_hero_xp_formula,
            hero_factor_xp,
            max_unit_level,
            grant_normal_xp,
            grant_normal_xp_formula,
            attr_bonus,
            building_kills_give_exp,
            global_experience,
            max_level_heroes_drain_exp,
            hero_exp_range,
            summoned_kill_factor,
            hero_ability_level_skip,
            revive,
        }
    }
}

/// Reads keys of the merged `[Misc]` section, recording the defaults and the rejected values.
struct Reader<'a> {
    profile: &'a Profile,
    tables: Option<&'a Tables>,
    defaulted: Vec<&'static str>,
    notes: Vec<String>,
}

impl Reader<'_> {
    /// Element 0 of an integer key (the hero keys). `0` when absent or malformed. Not recorded as defaulted: the
    /// hero keys have their own reading rules (see [`HeroMisc`]).
    fn int_element(&mut self, key: &'static str) -> i32 {
        self.profile
            .get_list(MISC_SECTION, key)
            .first()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(0)
    }

    /// Every element of an integer list key (malformed elements are `0`).
    fn int_list(&mut self, key: &'static str) -> Vec<i32> {
        self.profile
            .get_list(MISC_SECTION, key)
            .iter()
            .map(|s| s.trim().parse::<i32>().unwrap_or(0))
            .collect()
    }

    /// One real of a hero key, `0.0` when absent or malformed (the reader's default).
    fn real_or_zero(&mut self, key: &'static str) -> F32 {
        self.profile
            .get(MISC_SECTION, key)
            .and_then(|raw| parse_real(raw, self.tables))
            .unwrap_or(F32::ZERO)
    }

    /// One real. `None` when the key is absent (recorded as defaulted) or malformed (recorded with a note).
    fn real(&mut self, key: &'static str) -> Option<F32> {
        let Some(raw) = self.profile.get(MISC_SECTION, key) else {
            self.defaulted.push(key);
            return None;
        };
        match parse_real(raw, self.tables) {
            Some(v) => Some(v),
            None => {
                self.defaulted.push(key);
                self.notes.push(format!(
                    "{key}: {:?} is not a real; default used",
                    scalar(raw)
                ));
                None
            }
        }
    }

    /// The first `N` elements of a comma list, by index. An absent key is `None` (recorded as defaulted). A list
    /// shorter than `N` leaves the missing elements at `0.0`, as the reader does for a missing index; a malformed
    /// element is `0.0` with a note.
    fn list<const N: usize>(&mut self, key: &'static str) -> Option<[F32; N]> {
        if self.profile.get(MISC_SECTION, key).is_none() {
            self.defaulted.push(key);
            return None;
        }
        let items = self.profile.get_list(MISC_SECTION, key);
        let mut out = [F32::ZERO; N];
        for (i, slot) in out.iter_mut().enumerate() {
            let Some(item) = items.get(i) else {
                break;
            };
            match parse_real(item, self.tables) {
                Some(v) => *slot = v,
                None => {
                    self.notes
                        .push(format!("{key}[{i}]: {item:?} is not a real; 0.0 used"));
                }
            }
        }
        if items.len() < N {
            self.notes.push(format!(
                "{key}: {} values, {N} read; the rest are 0.0",
                items.len()
            ));
        }
        Some(out)
    }

    /// The first `N` elements of a comma list as integers (`atoi`, see the module notes). Absent key: `None`.
    fn ints<const N: usize>(&mut self, key: &'static str) -> Option<[i32; N]> {
        if self.profile.get(MISC_SECTION, key).is_none() {
            self.defaulted.push(key);
            return None;
        }
        let items = self.profile.get_list(MISC_SECTION, key);
        let mut out = [0i32; N];
        for (slot, item) in out.iter_mut().zip(items.iter()) {
            *slot = atoi(item);
        }
        if items.len() > N {
            self.notes.push(format!(
                "{key}: {} values, only the first {N} are read",
                items.len()
            ));
        }
        Some(out)
    }
}

/// `atoi`: leading blanks, an optional sign, then decimal digits, with the 32-bit wrap of the C result. Text with
/// no digits gives 0.
pub fn atoi(text: &str) -> i32 {
    let t = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut v: i32 = 0;
    for b in digits.bytes().take_while(u8::is_ascii_digit) {
        v = v.wrapping_mul(10).wrapping_add(i32::from(b - b'0'));
    }
    if neg {
        v.wrapping_neg()
    } else {
        v
    }
}
