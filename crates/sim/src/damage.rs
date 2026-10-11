//! The damage of one attack execute (`docs/spec/attack-damage.md`, sections 3 to 5).
//!
//! Pipeline implemented here, in the spec's order:
//! 1. Raw amount: the roll of the weapon on RNG stream 2 (each die `1..=sides`, see [`roll_total`]) plus the flat
//!    bonus, clamped at zero, converted to a real with the integer-to-real helper.
//! 2. Armor factor, physical damage only: `M = 1 - R`, where for armor `A >= 0`, `R = (K A) / (1 + K A)` (so
//!    `M = 1 / (1 + K A)`), and for `A < 0` `R` comes from a 21-entry table indexed by `min(trunc(-A), 20)`.
//! 3. Attack type times armor type factor from the 7 x 8 damage-type table.
//!
//! The target side of the original (`0x6F2A40D0`, `docs/spec/attack-damage.md` section 11) is [`target_side_amount`]:
//! the class word gate, the status flag, the armor factor (only for class `0x10`), the damage-type table, the
//! ethereal factor, the minimum-one rule, the relation factor and the zero checks, in the original's order. Verified
//! against the original in Unicorn (`tools/emu/target_damage_emu.py`, `crates/sim/tests/target_damage.rs`).
//! The 7 x 8 table and the ethereal factors are built from Misc exactly as the original's builder does
//! (`docs/spec/attack-damage.md` section 4). Not implemented (open in the spec): the attack-handler passes (evasion,
//! critical, bash and the other abilities; their list walk is not in [`target_side_amount`]), the target's counters
//! and flags (the model uses zero), and the Gaussian dice path above 16 dice (its constants are UNSURE;
//! [`SpecDamage`] falls back to the weapon's placeholder value there). The evasion decision ([`evades`]) and the
//! chance table ([`evasion_chance`]) are implemented, but not consulted by [`SpecDamage`]: which unit's abilities
//! the original's miss gate walks is not decoded (spec section 12.3).
//!
//! The 21-entry negative-armor table at `0x6FAB4300` is in BSS: its values are written by a start-up initialiser
//! (`0x6F85A4C0`, see `docs/spec/attack-damage.md` section 5.1), so a file read cannot see them.
//! [`NegativeArmorTable::computed`] re-runs that initialiser's arithmetic with the softfloat helpers.
//! [`NegativeArmorTable::from_game_dll`] refuses the all-zero file image. The 7 x 8 damage-type table is NOT in
//! `Game.dll`: the original builds it from the Misc data (`0x6F26D300`), so the host passes it in.
//! Nothing here embeds table contents.

use softfloat::{arith, conv, Tables, F32};

use crate::combat::{AttackType, DamageModel, NativeHit};
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

/// The Misc values the model needs: the armor constant `K` (`DefenseArmor`, `0.0` when absent in the original), the
/// damage-type table, and the ethereal factor of each attack type (row `0..=6`, see
/// `docs/spec/attack-damage.md` section 4.2).
#[derive(Clone, Debug, PartialEq)]
pub struct DamageMisc {
    pub defense_armor: F32,
    pub type_table: DamageTypeTable,
    pub ethereal_by_attack_type: [F32; ATTACK_TYPE_ROWS],
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

/// The damage class word of a basic attack (attack record `+0x14`). CONFIRMED: the execute routine
/// `CAbilityAttack` (`0x6F0CF660`) pushes the literal `0x10` for every normal attack (at `0x6F0CF81E`), whatever the
/// weapon's attack type is. The armor factor applies to class `0x10` only.
pub const ATTACK_DAMAGE_CLASS: u32 = 0x10;

/// The `flags` word of a basic attack's record (`+0x0C`): `0x100`, or `0x180` when the execute's flag argument is
/// set (CONFIRMED value set, `0x6F0CF803`). The model uses `0x100`.
pub const ATTACK_RECORD_FLAGS: u32 = 0x100;

/// Class mask A of the magic-immunity gate (`0x6FA6F2CC`, CONFIRMED value from start-up; its meaning is UNSURE).
pub const CLASS_MASK_A: u32 = 0x033E_E700;
/// Class mask B of the ethereal rule (`0x6FA6F2D0`, CONFIRMED value from start-up; its meaning is UNSURE).
pub const CLASS_MASK_B: u32 = 0x00C1_1830;
/// The relation constant `0x6FAAE574` (CONFIRMED value from start-up: 100.0; its meaning is UNSURE).
pub const RELATION_CONSTANT_BITS: u32 = 0x42C8_0000;

/// The game rules the target-side routine reads from Misc (`0x6F009F90`, boolean keys, element 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageRules {
    /// Misc `MagicImmunesResistDamage` (key at `0x6F88E2CC`): switches the immunity gate to the class-mask rules.
    pub magic_immunes_resist_damage: bool,
    /// Misc `EtherealDamageBonusAlly` (key at `0x6F88E420`): the ethereal factor also applies to allied sources.
    pub ethereal_damage_bonus_ally: bool,
    /// The relation constant (`0x6FAAE574`), the factor of one relation case.
    pub relation_constant: F32,
}

impl Default for DamageRules {
    fn default() -> Self {
        DamageRules {
            magic_immunes_resist_damage: false,
            ethereal_damage_bonus_ally: false,
            relation_constant: F32::from_bits(RELATION_CONSTANT_BITS),
        }
    }
}

/// The inputs of the target side of one damage event (the attack record and the target's fields the routine reads).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSide {
    /// Record `+0x10`: the raw amount, already clamped at zero by `GetDamageAmount`.
    pub raw: F32,
    /// Record `+0x14`: the damage class word (`0x10` for a basic attack, `1 << damage type` for a native).
    pub class: u32,
    /// Record `+0x20`: the attack row of the damage-type table and of the ethereal slots (`0..=6`). For a basic attack
    /// it is the weapon's atkType index (`0` Spells, `1` Normal, `2` Pierce, `3` Siege, `4` Magic, `5` Chaos, `6` Hero),
    /// CONFIRMED by Unicorn (spec section 12.1): the execute writes it from the unit's weapon field `+0xF4` before the
    /// missile-type switch, and the switch does not change it.
    pub category: u32,
    /// Record `+0x00` is not null (a source unit exists).
    pub source_present: bool,
    /// Record `+0x0C`: flags (`0x4` and `0x10` stop the damage; `0x20` lets the invulnerable target take it).
    pub record_flags: u32,
    /// Target `+0x5C` flags (`0x100` dead or untargetable; `0x10000`, `0x2000000`, `0x80`, `0x8000000` form the status
    /// flag, see [`status_flag`]).
    pub target_flags: u32,
    /// Target `+0x20` flags (`0x8` with a record without `0x20` stops the damage).
    pub target_flags20: u32,
    /// Target armor value `+0xE0` (float).
    pub armor: F32,
    /// Target armor type `+0xE4`, the damage-type table column (`0..=7`).
    pub armor_type: u8,
    /// Target `+0x158` counter: positive enables the ethereal factor and the second gate.
    pub counter_158: i32,
    /// Target `+0x15C` counter: positive stops the damage on the first gate.
    pub counter_15c: i32,
    /// The target's virtual `+0x11C` answer (invulnerable), used by the first gate.
    pub invulnerable: bool,
    /// `FUN_6f285b00(source, target)`: the source is not allied with the target (LIKELY meaning).
    pub not_allied: bool,
    /// The relation case: `None` when game setting 13 is off, otherwise (source player flag `+0x268` is set, target
    /// player flag is set).
    pub relation: Option<(bool, bool)>,
}

/// The status flag of the target: `0x6F26DE50` with argument 1 (CONFIRMED by disassembly: `test al,al; jns` tests the
/// sign bit of the LOW BYTE, i.e. bit 7) gives 1 when flag `0x10000` is set and either bit 7 is clear or flag
/// `0x8000000` is set; the routine's result is then AND-ed with flag `0x2000000` (`0x6F2A40F5`..`0x6F2A4104`). The
/// result is stored as the int 1, which reads as the float `1.4e-45`. When set, the armor factor is skipped and the
/// damage-type column is 4.
pub fn status_flag(target_flags: u32) -> bool {
    target_flags & 0x0001_0000 != 0
        && target_flags & 0x0200_0000 != 0
        && (target_flags & 0x80 == 0 || target_flags & 0x0800_0000 != 0)
}

/// The target side of one damage event: `0x6F2A40D0` from the amount on, in the original's order (CONFIRMED by
/// disassembly, `docs/spec/attack-damage.md` section 11). Returns the amount that reaches the HP change, `0.0` when
/// the damage is stopped.
///
/// Order: the dead flag; the immunity gate (class and counters); the flags of the record; the minimum-one value
/// (`min(raw, 1.0)`, taken before the armor step); the armor factor (only for class `0x10` without the status flag);
/// the damage-type factor (column = armor type, or 4 with the status flag); the ethereal factor; the maximum of the
/// amount and the minimum-one value; the relation factor; the zero test on the amount after the multipliers; the
/// target `+0x20` test. The attack-handler passes are not in this function.
pub fn target_side_amount(
    side: &TargetSide,
    misc: &DamageMisc,
    rules: &DamageRules,
    negative: &NegativeArmorTable,
    tables: &Tables,
) -> F32 {
    let stop = F32::ZERO;
    if side.target_flags & 0x100 != 0 {
        return stop;
    }
    // The immunity gate. Rule off: the gate applies to every class except 0x10, 0x20, 0x4000000 and 0x400000.
    // Rule on: the first gate is the class-mask test or attack row 4; the second is row 1, 2, 3, 5 or 6, or the
    // second mask.
    let class = side.class;
    let category = side.category;
    let (gate, second) = if rules.magic_immunes_resist_damage {
        (
            category == 4 || class & CLASS_MASK_A != 0,
            matches!(category, 1 | 2 | 3 | 5 | 6) || class & CLASS_MASK_B != 0,
        )
    } else {
        (
            !matches!(class, 0x10 | 0x20 | 0x0400_0000 | 0x0040_0000),
            false,
        )
    };
    if gate {
        if side.invulnerable || side.counter_15c > 0 {
            return stop;
        }
    } else if second && side.counter_158 > 0 {
        return stop;
    }
    if side.record_flags & 0x4 != 0 || side.record_flags & 0x10 != 0 {
        return stop;
    }
    let status = status_flag(side.target_flags);
    let category_u8 = u8::try_from(category).unwrap_or(u8::MAX);

    // The minimum-one value: the raw amount clamped at zero when the row is not 0, then min(., 1.0).
    let clamped = if arith::lt(side.raw, F32::ZERO) && category != 0 {
        F32::ZERO
    } else {
        side.raw
    };
    let minimum_one = if arith::lt(clamped, F32::ONE) {
        clamped
    } else {
        F32::ONE
    };

    // Record `+0x10` through the multipliers.
    let mut amount = side.raw;
    if !status && side.class == ATTACK_DAMAGE_CLASS {
        let m = armor_factor(side.armor, misc.defense_armor, negative, tables);
        amount = arith::mul(amount, m);
    }
    let column = if status {
        4
    } else {
        side.armor_type.min(7)
    };
    let type_factor = misc.type_table.factor(category_u8, column);
    if !arith::eq(type_factor, F32::ONE) {
        amount = arith::mul(amount, type_factor);
    }
    let ethereal_applies = !side.source_present
        || rules.ethereal_damage_bonus_ally
        || side.not_allied;
    if ethereal_applies && side.counter_158 > 0 {
        let slot = misc
            .ethereal_by_attack_type
            .get(usize::from(category_u8))
            .copied()
            .unwrap_or(F32::ZERO);
        if !arith::eq(slot, F32::ONE) {
            amount = arith::mul(amount, slot);
        }
    }

    // The amount sent on: the larger of the multiplied amount and the minimum-one value (ties keep the minimum).
    let mut sent = if arith::lt(minimum_one, amount) {
        amount
    } else {
        minimum_one
    };
    if arith::eq(amount, F32::ZERO) {
        return stop;
    }
    // The relation case REPLACES the sent value (CONFIRMED by Unicorn, 2026-10-11): with the source flag clear and the
    // target flag set, the sent value is the raw amount times the relation constant, so the armor, table and
    // ethereal factors are lost in that case. With the source flag set and the target flag clear it is zero.
    if let Some((source_flag, target_flag)) = side.relation {
        if side.source_present {
            if !source_flag {
                if target_flag {
                    sent = arith::mul(side.raw, rules.relation_constant);
                }
            } else if !target_flag {
                sent = F32::ZERO;
            }
        }
    }
    if side.target_flags20 & 0x8 != 0 && side.record_flags & 0x20 == 0 {
        return stop;
    }
    sent
}

/// The attack record that `UnitDamageTarget` (`0x6F3C8850`) and the per-target step of `UnitDamagePoint`
/// (`0x6F2BFED0`) build from their script arguments. Field offsets are the record's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeRecord {
    /// `+0x04`: the weapon type argument.
    pub weapon: u32,
    /// `+0x0C`: `0x100` for the attack flag, `|1` for the ranged flag.
    pub flags: u32,
    /// `+0x14`: `1 << (damage type & 31)` (the shift count is the low byte of the damage type, taken mod 32).
    pub class: u32,
    /// `+0x20`: the attack type argument, the damage-type table row.
    pub row: u32,
}

/// The record built from the script arguments, or `None` when the native rejects them (CONFIRMED by disassembly
/// of `0x6F3C8850` and checked in Unicorn, `tools/emu/unit_damage_target_emu.py`): the damage type must not be
/// above `0x80000000` (unsigned), the attack type at most 7 and the weapon type at most 24. Any nonzero flag
/// argument counts as true.
pub fn native_record(
    attack: bool,
    ranged: bool,
    attack_type: u32,
    damage_type: u32,
    weapon_type: u32,
) -> Option<NativeRecord> {
    if damage_type > 0x8000_0000 || attack_type > 7 || weapon_type > 0x18 {
        return None;
    }
    Some(NativeRecord {
        weapon: weapon_type,
        flags: (if attack { 0x100 } else { 0 }) | u32::from(ranged),
        class: 1u32 << (damage_type & 31),
        row: attack_type,
    })
}

/// The RNG stream of the evasion draw (`0x6F08BD20` sets `edx = 2` before the draw, CONFIRMED; spec section 12.2).
pub const EVASION_STREAM: usize = 2;

/// The evasion decision of `CAbilityEvasion::EvadeAttack` (`0x6F08BD20`, CONFIRMED by disassembly and by Unicorn,
/// `tools/emu/evasion_emu.py`: 300 decision cases and 0 mismatches). The attack is evaded iff `chance > draw`, a strict
/// compare. An unordered compare (either operand NaN) also evades: the x87 `fcom` sets the parity flag and the routine
/// tests it. Equal values do not evade.
pub fn evades(chance: F32, draw: F32) -> bool {
    // Evades unless the chance is less than or equal to the draw. `partial_cmp` is `None` for NaN, which evades.
    let order = f32::from_bits(chance.to_bits()).partial_cmp(&f32::from_bits(draw.to_bits()));
    !matches!(order, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal))
}

/// The evasion chance of one level (`0x6F02E9E0`, CONFIRMED by Unicorn: 100 cases, 0 mismatches): the float stored at
/// the level's record `+0x20` in the level table of the ability's definition, for `0 <= level < count`; `0.0` for a
/// negative level, a level at or above the count, or a definition without its data flag. `table` is the level table
/// (one entry per level) and must be empty when the definition has no data.
pub fn evasion_chance(table: &[F32], level: i32) -> F32 {
    match usize::try_from(level) {
        Ok(index) if index < table.len() => table[index],
        _ => F32::ZERO,
    }
}

/// The spec damage model: dice on stream 2, then the target side ([`target_side_amount`]) with the target's armor
/// and the damage-type table. The target's counters, flags and alliance are zero (not in the model yet).
#[derive(Debug, Clone)]
pub struct SpecDamage {
    tables: Tables,
    misc: DamageMisc,
    rules: DamageRules,
    negative: NegativeArmorTable,
}

impl SpecDamage {
    /// A model with the given tables and Misc values; the rules are the defaults (both Misc flags off).
    pub fn new(tables: Tables, misc: DamageMisc, negative: NegativeArmorTable) -> Self {
        SpecDamage {
            tables,
            misc,
            rules: DamageRules::default(),
            negative,
        }
    }

    /// The same model with the given Misc rules.
    pub fn with_rules(mut self, rules: DamageRules) -> Self {
        self.rules = rules;
        self
    }

    /// The ethereal factor stored for an attack type (CONFIRMED: the table slot is the attack type's row, see
    /// `docs/spec/attack-damage.md` section 4.2). It applies only under the ethereal conditions of
    /// [`target_side_amount`].
    pub fn ethereal_factor(&self, attack_type: u8) -> F32 {
        self.misc
            .ethereal_by_attack_type
            .get(usize::from(attack_type))
            .copied()
            .unwrap_or(F32::ZERO)
    }

    /// The raw amount of one execute (`CAbilityAttack::GetDamageAmount`, CONFIRMED by disassembly): the roll on
    /// combat stream 2 ([`roll_total`]), then clamped at zero before the armor step. The bonus is an integer added
    /// to the dice total before the I2R conversion. `None` without an RNG table or above [`MAX_DICE`] dice.
    fn raw(&self, game: &mut Game, attacker: ObjKey, weapon: &AttackType) -> Option<F32> {
        // The hero attack bonus (`docs/spec/heroes.md`, section 10) is added to the dice bonus as an integer. The
        // conversion of the stored real (truncation) is UNSURE.
        let hero = conv::r2i(game.unit(attacker).hero_attack);
        let bonus = weapon.dice_bonus.saturating_add(hero);
        let (table, streams) = game.rng_parts()?;
        let total = roll_total(
            streams.stream_mut(COMBAT_STREAM),
            table,
            i32::try_from(weapon.dice).ok()?,
            weapon.sides,
            bonus,
        )?;
        Some(conv::i2r(total.max(0)))
    }
}

impl SpecDamage {
    /// The target side of a script damage record ([`NativeHit`]): the same routine as an attack, with the class word
    /// `1 << damage type` and the attack-type argument as the table row. Counters and flags are zero as in `roll`.
    pub fn hit_amount(&self, game: &Game, hit: &NativeHit) -> F32 {
        let defense = game.unit_defense(hit.target);
        let side = TargetSide {
            raw: hit.raw,
            class: hit.class,
            category: hit.row,
            source_present: true,
            record_flags: hit.record_flags,
            target_flags: 0,
            target_flags20: 0,
            armor: defense.armor,
            armor_type: defense.armor_type,
            counter_158: 0,
            counter_15c: 0,
            invulnerable: hit.invulnerable,
            not_allied: hit.not_allied,
            relation: None,
        };
        target_side_amount(&side, &self.misc, &self.rules, &self.negative, &self.tables)
    }
}

impl DamageModel for SpecDamage {
    fn native_hit(&mut self, game: &Game, hit: &NativeHit) -> F32 {
        self.hit_amount(game, hit)
    }

    fn roll(
        &mut self,
        game: &mut Game,
        attacker: ObjKey,
        target: ObjKey,
        weapon: &AttackType,
    ) -> F32 {
        let Some(raw) = self.raw(game, attacker, weapon) else {
            // No RNG table, or more dice than the known sampler handles: the placeholder value (documented).
            return weapon.damage;
        };
        let defense = game.unit_defense(target);
        let side = TargetSide {
            raw,
            class: weapon.damage_class,
            category: u32::from(weapon.damage_type),
            source_present: true,
            record_flags: ATTACK_RECORD_FLAGS,
            target_flags: 0,
            target_flags20: 0,
            armor: defense.armor,
            armor_type: defense.armor_type,
            counter_158: 0,
            counter_15c: 0,
            invulnerable: false,
            not_allied: false,
            relation: None,
        };
        target_side_amount(&side, &self.misc, &self.rules, &self.negative, &self.tables)
    }
}
