//! The damage of one attack execute (`docs/spec/attack-damage.md`, sections 3 to 5).
//!
//! Pipeline implemented here, in the spec's order:
//! 1. Raw amount: the roll of the weapon on RNG stream 2 (each die `1..=sides`, see [`roll_total`]) plus the flat
//!    bonus, clamped at zero, converted to a real with the integer-to-real helper.
//! 2. Armor factor, physical damage only: `M = 1 - R`, where for armor `A >= 0`, `R = (K A) / (1 + K A)` (so
//!    `M = 1 / (1 + K A)`), and for `A < 0` `R` comes from a 21-entry table indexed by `min(trunc(-A), 20)`.
//! 3. Attack type times armor type factor from the 7 x 8 damage-type table.
//!
//! Not implemented (open in the spec): evasion and the other handler categories (other specs), the ethereal table
//! (its lookup key is UNSURE), the relation factor, the miss/critical rolls, and the Gaussian dice path above 16
//! dice (its constants are UNSURE; [`SpecDamage`] falls back to the weapon's placeholder value there).
//!
//! The 21-entry negative-armor table at `0x6FAB4300` is in BSS: its values are written by a start-up initialiser
//! (`0x6F85A4C0`, see `docs/spec/attack-damage.md` section 5.1), so a file read cannot see them.
//! [`NegativeArmorTable::computed`] re-runs that initialiser's arithmetic with the softfloat helpers.
//! [`NegativeArmorTable::from_game_dll`] refuses the all-zero file image. The 7 x 8 damage-type table is NOT in
//! `Game.dll`: the original builds it from the Misc data (`0x6F26D300`), so the host passes it in.
//! Nothing here embeds table contents.

use softfloat::{arith, conv, Tables, F32};

use crate::combat::{AttackType, DamageModel};
use crate::natives::game::{Game, ObjKey};
use crate::pe;
use crate::rng::{Rng, RngTable};

/// Virtual address of the 21-entry negative-armor table in `Game.dll` 1.26a.
pub const NEG_ARMOR_VA: u32 = 0x6FAB_4300;
/// Number of entries in the negative-armor table (index 0..=20).
pub const NEG_ARMOR_LEN: usize = 21;
/// Number of attack-type rows of the damage-type table (Misc `DamageBonus*`).
pub const ATTACK_TYPE_ROWS: usize = 7;
/// Number of armor-type columns of the damage-type table.
pub const ARMOR_TYPE_COLS: usize = 8;
/// The dice sampler is used directly up to this many dice (above it the spec's Gaussian path, UNSURE).
pub const MAX_DICE: u32 = 16;
/// RNG stream of combat (rng.md section 6; the call that selects it is LIKELY, spec section 3.4).
pub const COMBAT_STREAM: usize = 2;

/// Errors when reading the negative-armor table from a `Game.dll` image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageTableError {
    /// The file is not a PE32 image, or the table address is not backed by file data.
    Image(&'static str),
    /// All 21 entries are zero: the table is filled at run time, not stored in the file.
    NotStored,
    /// An entry is NaN or infinite.
    NotFinite,
}

impl std::fmt::Display for DamageTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DamageTableError::Image(why) => {
                write!(f, "cannot read the armor table from Game.dll: {why}")
            }
            DamageTableError::NotStored => {
                write!(
                    f,
                    "the negative-armor table is all zeros in Game.dll (filled at run time)"
                )
            }
            DamageTableError::NotFinite => {
                write!(f, "the negative-armor table has a non-finite entry")
            }
        }
    }
}

impl std::error::Error for DamageTableError {}

/// The negative-armor table: `R` for `n = min(trunc(-A), 20)`.
#[derive(Clone, PartialEq)]
pub struct NegativeArmorTable([F32; NEG_ARMOR_LEN]);

impl std::fmt::Debug for NegativeArmorTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The values are data from the user's Game.dll; print the size only.
        f.write_str("NegativeArmorTable(21 entries)")
    }
}

/// The denominator of the negative-armor table: the initialiser divides by `I2R(100000)`.
pub const NEG_ARMOR_SCALE: i32 = 100_000;
/// The per-level decay of the negative-armor table (`0.94` per armor point, the value the initialiser's
/// integers encode).
pub const NEG_ARMOR_DECAY: f64 = 0.94;

impl NegativeArmorTable {
    /// The table as the start-up initialiser of `Game.dll` builds it (`docs/spec/attack-damage.md` section 5.1).
    ///
    /// Entry `n` is `DIV(I2R(k_n), I2R(100000))` with `k_n = -trunc(100000 * (1 - 0.94^n))` computed in `f64`
    /// (`k_0 = 0`). The 21 integers are the ones in the original's code (checked in Unicorn; their fractional parts
    /// stay at least 0.01 away from an integer, so the `f64` rounding cannot change them). The division is the
    /// softfloat DIV, so the stored values are the original's (not correctly rounded) quotients.
    pub fn computed(tables: &Tables) -> Self {
        let scale = conv::i2r(NEG_ARMOR_SCALE);
        let mut decay = 1.0f64; // 0.94^n, by repeated multiplication (IEEE, deterministic)
        let mut out = [F32::ZERO; NEG_ARMOR_LEN];
        for slot in out.iter_mut() {
            let exact = f64::from(NEG_ARMOR_SCALE) * (1.0 - decay);
            let k = -(exact.trunc() as i32);
            *slot = arith::div(conv::i2r(k), scale, tables);
            decay *= NEG_ARMOR_DECAY;
        }
        NegativeArmorTable(out)
    }

    /// The 21 entry bit patterns, in table order.
    pub fn to_bits(&self) -> [u32; NEG_ARMOR_LEN] {
        self.0.map(|v| v.to_bits())
    }

    /// Wraps the 21 entry bit patterns. Used by tests and by callers that cache the values.
    pub fn from_bits(bits: [u32; NEG_ARMOR_LEN]) -> Result<Self, DamageTableError> {
        if bits.iter().all(|&b| b == 0) {
            return Err(DamageTableError::NotStored);
        }
        if bits.iter().any(|&b| (b >> 23) & 0xFF == 0xFF) {
            return Err(DamageTableError::NotFinite);
        }
        Ok(NegativeArmorTable(bits.map(F32::from_bits)))
    }

    /// Reads the table from the bytes of the user's `Game.dll` (1.26a).
    pub fn from_game_dll(dll: &[u8]) -> Result<Self, DamageTableError> {
        let raw =
            pe::read_va(dll, NEG_ARMOR_VA, NEG_ARMOR_LEN * 4).map_err(DamageTableError::Image)?;
        let mut bits = [0u32; NEG_ARMOR_LEN];
        for (slot, chunk) in bits.iter_mut().zip(raw.chunks_exact(4)) {
            *slot = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        Self::from_bits(bits)
    }

    /// The entry for armor `a < 0`: index `min(trunc(-a), 20)`.
    pub fn lookup(&self, a: F32) -> F32 {
        let n = conv::f2i(arith::neg(a)).clamp(0, (NEG_ARMOR_LEN - 1) as i32);
        self.0[n as usize]
    }
}

/// The 7 x 8 damage-type table (Misc `DamageBonus*`): row = attack type, column = armor type.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageTypeTable(pub [[F32; ARMOR_TYPE_COLS]; ATTACK_TYPE_ROWS]);

impl DamageTypeTable {
    /// The factor for an attack type and an armor type. Indices above the table are clamped to the last row or column
    /// (UNVERIFIED: the original's behaviour for out-of-range types is not checked).
    pub fn factor(&self, attack_type: u8, armor_type: u8) -> F32 {
        let row = usize::from(attack_type).min(ATTACK_TYPE_ROWS - 1);
        let col = usize::from(armor_type).min(ARMOR_TYPE_COLS - 1);
        self.0[row][col]
    }
}

/// The Misc values the model needs: the armor constant `K` (`DefenseArmor`) and the damage-type table.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageMisc {
    pub defense_armor: F32,
    pub type_table: DamageTypeTable,
}

/// The armor factor `M` of armor `a` (spec section 5). `M` is `1 - R` with the softfloat helpers.
pub fn armor_factor(a: F32, k: F32, negative: &NegativeArmorTable, tables: &Tables) -> F32 {
    let r = if arith::lt(a, F32::ZERO) {
        negative.lookup(a)
    } else {
        let x = arith::mul(k, a);
        let y = arith::add(F32::ONE, x);
        arith::div(x, y, tables)
    };
    if arith::gt(r, F32::ONE) {
        return F32::ZERO;
    }
    arith::sub(F32::ONE, r)
}

/// The sampler `0x6F26CAD0`: the sum of `count` dice with `sides` sides (spec section 3.1). One step of `stream` per
/// die, each die `floor(r * sides / 2^32)`, so values run `0..sides-1` (CONFIRMED by emulation). The caller adds one
/// per die, see [`roll_total`]. `None` above [`MAX_DICE`] dice, where the spec's Gaussian path is not known.
pub fn dice_sum(stream: &mut Rng, table: &RngTable, count: u32, sides: u32) -> Option<u32> {
    if count > MAX_DICE {
        return None;
    }
    let mut sum: u64 = 0;
    for _ in 0..count {
        let r = u64::from(stream.next_u32(table));
        sum += (r * u64::from(sides)) >> 32;
    }
    Some(sum as u32)
}

/// The roll wrapper `0x6F285350` (CONFIRMED by disassembly, 400/400 in Unicorn with the damage path): `count` dice of
/// `sides` sides, each die `1..=sides` (the sampler's `0..sides-1` plus one per die), plus `bonus`, as a wrapping
/// int32. `count <= 0` returns `0` and draws nothing: the flat bonus is dropped on that path too. `None` above
/// [`MAX_DICE`] dice (Gaussian path, not implemented).
pub fn roll_total(
    stream: &mut Rng,
    table: &RngTable,
    count: i32,
    sides: u32,
    bonus: i32,
) -> Option<i32> {
    if count <= 0 {
        return Some(0);
    }
    let sum = dice_sum(stream, table, count as u32, sides)?;
    Some((sum as i32).wrapping_add(count).wrapping_add(bonus))
}

/// The spec damage model: dice on stream 2, then armor and the damage-type table.
#[derive(Debug, Clone)]
pub struct SpecDamage {
    tables: Tables,
    armor_k: F32,
    negative: NegativeArmorTable,
    types: DamageTypeTable,
}

impl SpecDamage {
    /// A model with the given tables and Misc values.
    pub fn new(tables: Tables, misc: DamageMisc, negative: NegativeArmorTable) -> Self {
        SpecDamage {
            tables,
            armor_k: misc.defense_armor,
            negative,
            types: misc.type_table,
        }
    }

    /// The raw amount of one execute (`CAbilityAttack::GetDamageAmount`, CONFIRMED by disassembly): the roll on
    /// combat stream 2 ([`roll_total`]), then clamped at zero before the armor step. The bonus is an integer added
    /// to the dice total before the I2R conversion. `None` without an RNG table or above [`MAX_DICE`] dice.
    fn raw(&self, game: &mut Game, weapon: &AttackType) -> Option<F32> {
        let (table, streams) = game.rng_parts()?;
        let total = roll_total(
            streams.stream_mut(COMBAT_STREAM),
            table,
            i32::try_from(weapon.dice).ok()?,
            weapon.sides,
            weapon.dice_bonus,
        )?;
        Some(conv::i2r(total.max(0)))
    }
}

impl DamageModel for SpecDamage {
    fn roll(
        &mut self,
        game: &mut Game,
        _attacker: ObjKey,
        target: ObjKey,
        weapon: &AttackType,
    ) -> F32 {
        let Some(raw) = self.raw(game, weapon) else {
            // No RNG table, or more dice than the known sampler handles: the placeholder value (documented).
            return weapon.damage;
        };
        let defense = game.unit_defense(target);
        let mut amount = raw;
        if weapon.physical {
            let m = armor_factor(defense.armor, self.armor_k, &self.negative, &self.tables);
            amount = arith::mul(amount, m);
        }
        let factor = self.types.factor(weapon.damage_type, defense.armor_type);
        amount = arith::mul(amount, factor);
        if arith::lt(amount, F32::ZERO) {
            return F32::ZERO;
        }
        amount
    }
}
