//! The `[Misc]` section: the gameplay constants of the attack cycle and the damage model
//! (`docs/spec/attack-damage.md`, sections 4, 5 and 7).
//!
//! Sources, in load order (the caller passes the layers in this order and a later layer overrides an earlier one,
//! key by key): `Units\MiscData.txt`, then `Units\MiscGame.txt`, then the map's `war3mapMisc.txt`.
//!
//! Keys and shapes:
//! - `DefenseArmor`: one real, the armor constant `K`.
//! - `DamageBonusSpells`, `DamageBonusNormal`, `DamageBonusPierce`, `DamageBonusSiege`, `DamageBonusMagic`,
//!   `DamageBonusChaos`, `DamageBonusHero`: a comma list of 8 reals each. Row = attack type in the binary order of
//!   the spec (section 7: Spells 0, Normal 1, Pierce 2, Siege 3, Magic 4, Chaos 5, Hero 6); column = armor type
//!   0 to 7 (the list order).
//! - `EtherealDamageBonus`: a comma list of 7 reals (UNSURE: the count, from spec section 8 item 2).
//! - `ReactionDelay`: one real, seconds. `AttackHalfAngle`: one real, radians (UNSURE meaning, spec attack-cycle).
//!
//! Defaults, used when no layer sets the key (all UNSURE, none is in the decompile export):
//! - `DefenseArmor` = 0.06 (the value OpenRealm uses; the negative-armor table also hard-codes 0.06).
//! - every `DamageBonus*` cell and every ethereal entry = 1.0 (a neutral factor).
//! - `ReactionDelay` = 0.0 (the placeholder of `sim::combat::CombatMisc`).
//! - `AttackHalfAngle` absent = `None`, which skips the facing gate (the placeholder of `sim::combat::CombatMisc`).
//!
//! A value that is present but malformed (not a real, wrong list length) is rejected: the key takes its default and
//! a note says why. Reals go through [`crate::real::parse_real`]: the S2R rules when the Game.dll tables are given
//! (UNSURE that the original's Misc reader uses S2R), `f64` rounding otherwise.

use softfloat::{Tables, F32};

use crate::real::{parse_real, scalar};
use crate::Profile;

/// The section that holds the constants.
pub const MISC_SECTION: &str = "Misc";
/// Number of attack-type rows of the damage-type table.
pub const ATTACK_ROWS: usize = 7;
/// Number of armor-type columns of the damage-type table.
pub const ARMOR_COLUMNS: usize = 8;
/// Number of entries of `EtherealDamageBonus` (UNSURE, see the module documentation).
pub const ETHEREAL_ENTRIES: usize = 7;
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
    /// `DefenseArmor`, the armor constant `K`.
    pub defense_armor: F32,
    /// `damage_bonus[attack_row][armor_column]`.
    pub damage_bonus: [[F32; ARMOR_COLUMNS]; ATTACK_ROWS],
    /// `EtherealDamageBonus`.
    pub ethereal_damage_bonus: [F32; ETHEREAL_ENTRIES],
    /// `ReactionDelay` in seconds.
    pub reaction_delay: F32,
    /// `AttackHalfAngle`, or `None` when no layer sets it.
    pub attack_half_angle: Option<F32>,
    /// The keys that took their default (absent, or present but rejected), in reading order.
    pub defaulted: Vec<&'static str>,
    /// Why a present value was rejected, one line each.
    pub notes: Vec<String>,
    /// How many of the layers had a `[Misc]` section. Zero means no Misc data at all (every key is a default).
    pub layers_with_misc: usize,
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
        let attack_half_angle = reader.real("AttackHalfAngle");
        let Reader {
            defaulted, notes, ..
        } = reader;
        MiscData {
            defense_armor,
            damage_bonus,
            ethereal_damage_bonus,
            reaction_delay,
            attack_half_angle,
            defaulted,
            notes,
            layers_with_misc,
        }
    }

    /// The defaults of every constant (see the module documentation; UNSURE).
    fn default_values() -> MiscData {
        let neutral = F32::ONE;
        MiscData {
            defense_armor: F32::from_bits(0.06f32.to_bits()),
            damage_bonus: [[neutral; ARMOR_COLUMNS]; ATTACK_ROWS],
            ethereal_damage_bonus: [neutral; ETHEREAL_ENTRIES],
            reaction_delay: F32::ZERO,
            attack_half_angle: None,
            defaulted: Vec::new(),
            notes: Vec::new(),
            layers_with_misc: 0,
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

    /// A comma list of exactly `N` reals.
    fn list<const N: usize>(&mut self, key: &'static str) -> Option<[F32; N]> {
        if self.profile.get(MISC_SECTION, key).is_none() {
            self.defaulted.push(key);
            return None;
        }
        let items = self.profile.get_list(MISC_SECTION, key);
        if items.len() != N {
            self.defaulted.push(key);
            self.notes.push(format!(
                "{key}: {} values, {N} expected; default used",
                items.len()
            ));
            return None;
        }
        let mut out = [F32::ZERO; N];
        for (slot, item) in out.iter_mut().zip(&items) {
            match parse_real(item, self.tables) {
                Some(v) => *slot = v,
                None => {
                    self.defaulted.push(key);
                    self.notes
                        .push(format!("{key}: {item:?} is not a real; default used"));
                    return None;
                }
            }
        }
        Some(out)
    }
}
