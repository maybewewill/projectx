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
    /// `AttackHalfAngle`, or `None` when no layer sets it.
    pub attack_half_angle: Option<F32>,
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
            upkeep_usage,
            upkeep_gold_tax,
            upkeep_lumber_tax,
            train_refund_rate,
            research_refund_rate,
            revive_refund_rate,
            low_gold_amount,
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
            attack_half_angle: None,
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
