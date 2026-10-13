//! Orc hero abilities (spec: `docs/spec/abilities-orc-heroes.md`).
//!
//! Labels: CONFIRMED = read in the disassembly and, where a number says so, checked in Unicorn; LIKELY = strong
//! inference; UNSURE = a stand-in.
//!
//! ## Critical Strike (`AOcr`, class `CAbilityCriticalStrike`, vtable `0x6F909D74`)
//! * The roll happens at the swing START (the attack ability broadcasts the order `0xD000F` to every ability of the
//!   attacker through `0x6F281580`, which calls the arming slot `0x124` = `0x6F1B2EC0`; callers `0x6F0CD722`,
//!   `0x6F0D0CE5`), not at the damage point. The arming slot first clears the armed flag, then (not under the
//!   level-gate buff, count gate and target filters) takes ONE draw of combat stream **3** and compares
//!   `draw <= effective chance` (soft `le`, `0x6F01FD60`). On success the flag `0x200` is set and the hit counter
//!   (`+0x84`) is reset to 1, otherwise the counter is incremented.
//! * The effective chance is a PSEUDO-RANDOM DISTRIBUTION: `c` = `DataA` of the level in PERCENT units (the data
//!   holds 15 for 15 %), `table = PRD[clamp(f2i(c * 0.2 + 0.5), 0, 20)]` (`0x6F1D21F0`; 0.2 and 0.5 are the runtime
//!   constants `0x6FAAE4B0` / `0x6FAAE4D0`), effective = `I2R(counter) * table` (`0x6F1A3470`). The 21 table entries
//!   are `I2R(x) / I2R(10000)` for x = 0, 38, 147, 320, 547, 819, 1126, 1461, 1815, 2184, 2562, 2953, 3333, 3781,
//!   4203, 4607, 5000, 5799, 6666, 7706, 10000. Unicorn: 400 table cases and 400 effective-chance cases (random bit
//!   patterns incl. NaN/inf, negative and huge counters), 0 mismatches (`tools/emu/crit_emu.py`).
//! * The counter starts at 1 (the init slot `0x6F1A3440` writes `+0x84 = 1`).
//! * At the execute, right after the dice (`0x6F2763A0`, category `0xB`, slot `0x134` = `0x6F1A3590`): when the flag
//!   is set and `DataB` (the multiplier) is non-zero, `extra = mul(D, DataB) - D` is added to the record damage `D`
//!   through `0x6F255720`, which does nothing when the record already carries a positive bonus (`+0x18 > 0`) and
//!   otherwise stores the bonus in `+0x18`. The result feeds the target side (armor, damage type table), so the
//!   multiplier applies to the RAW damage (CONFIRMED order).
//! * `DataE` non-zero sets the record flag `0x400` (never miss: the miss gate is skipped). Not modelled (the 1.26a
//!   data has `DataE` empty). `DataC` is read by the after-hit slot `0x154` = `0x6F1CC530` (shared with Bash, flat
//!   bonus damage as a second hit; the 1.26a data has 0): not modelled. `DataD` (chance to evade) shares the evasion
//!   walk slots `0x158`/`0x164`; with `DataD` = 0 the score is 0 and the entry is never selected (CONFIRMED by
//!   reading `0x6F1A3540`/`0x6F1A34C0`; data 0).

use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

use crate::abilities::code;
use crate::natives::game::{Game, ObjKey};

/// Critical Strike's base code.
pub const CRITICAL_STRIKE: u32 = code(b"AOcr");
/// The combat RNG stream of the arming roll (`mov edx, 3` before the draw at `0x6F1B2FD3`).
pub const CRIT_STREAM: usize = 3;

/// The 21 entries of the PRD table as float bits (Unicorn: `W` lines of `tests/data/orc/crit.txt`).
pub const PRD_TABLE: [u32; 21] = [
    0x0000_0000, 0x3B79_096C, 0x3C70_D845, 0x3D03_126F, 0x3D60_0D1C, 0x3DA7_BB30, 0x3DE6_9AD4, 0x3E15_9B3D,
    0x3E39_DB23, 0x3E5F_A440, 0x3E83_2CA5, 0x3E97_3190, 0x3EAA_A64C, 0x3EC1_9653, 0x3ED7_3190, 0x3EEB_E0DF,
    0x3F00_0000, 0x3F14_7454, 0x3F2A_A64C, 0x3F45_460B, 0x3F80_0000,
];

/// `0x6F1D21F0`: the PRD table value for the nominal chance `c` in percent units.
pub fn prd_value(chance: F32) -> F32 {
    let scaled = arith::mul(chance, F32::from_bits(0x3E4C_CCCD)); // 0.2
    let idx = conv::f2i(arith::add(scaled, F32::from_bits(0x3F00_0000))); // + 0.5
    let idx = idx.clamp(0, 20) as usize;
    F32::from_bits(PRD_TABLE[idx])
}

/// `0x6F1A3470`: the effective chance for the hit counter `count`.
pub fn effective_chance(chance: F32, count: i32) -> F32 {
    arith::mul(conv::i2r(count), prd_value(chance))
}

/// The arming decision: `draw <= effective` (soft `le`).
pub fn crit_armed(draw: F32, effective: F32) -> bool {
    arith::le(draw, effective)
}

/// The bonus handler `0x6F1A3590` + `0x6F255720` on a record with damage `damage` and no earlier bonus
/// (`+0x18 = 0`): the new damage `damage + (damage * mult - damage)`. A zero multiplier leaves the record alone.
pub fn crit_damage(damage: F32, mult: F32) -> F32 {
    // The x87 equality test skips only an exact zero (either sign); NaN goes on (unordered sets the parity flag).
    if mult.to_f32() == 0.0 {
        return damage;
    }
    let bonus = arith::sub(arith::mul(damage, mult), damage);
    arith::add(arith::sub(damage, F32::ZERO), bonus)
}

/// Per-game state of the orc abilities.
#[derive(Debug, Default, Clone)]
pub struct OrcState {
    /// Critical Strike hit counters (`+0x84`) per unit; absent = 1.
    crit_count: BTreeMap<ObjKey, i32>,
    /// Units whose Critical Strike is armed for the coming execute (flag `0x200`).
    crit_armed: BTreeSet<ObjKey>,
}

impl OrcState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Game {
    /// Swing start: the Critical Strike arming roll (see the module notes). One stream-3 draw per ability.
    pub(crate) fn orc_swing_start(&mut self, unit: ObjKey) {
        self.orc.crit_armed.remove(&unit);
        let alive = {
            let u = self.unit(unit);
            !u.dead && !u.removed
        };
        if !alive || self.unit(unit).abilities.get(CRITICAL_STRIKE).is_none_or(|a| a.is_removed()) {
            return;
        }
        let Some(record) = self.ability_level_record(unit, CRITICAL_STRIKE) else {
            return;
        };
        let count = *self.orc.crit_count.get(&unit).unwrap_or(&1);
        let eff = effective_chance(record.data[0], count);
        let Some((table, streams)) = self.rng_parts() else {
            return;
        };
        let draw = streams.stream_mut(CRIT_STREAM).random_fraction(table);
        if crit_armed(draw, eff) {
            self.orc.crit_armed.insert(unit);
            self.orc.crit_count.insert(unit, 1);
        } else {
            self.orc.crit_count.insert(unit, count.wrapping_add(1));
        }
    }

    /// The execute: applies an armed Critical Strike to the raw damage (before armor).
    pub(crate) fn orc_crit_raw(&mut self, attacker: ObjKey, raw: F32) -> F32 {
        if !self.orc.crit_armed.contains(&attacker) {
            return raw;
        }
        match self.ability_level_record(attacker, CRITICAL_STRIKE) {
            Some(record) => crit_damage(raw, record.data[1]),
            None => raw,
        }
    }

    /// Whether the unit's Critical Strike is armed for its coming hit (for tests and the host).
    pub fn crit_is_armed(&self, unit: ObjKey) -> bool {
        self.orc.crit_armed.contains(&unit)
    }

    /// The Critical Strike hit counter of a unit (1 before any roll).
    pub fn crit_counter(&self, unit: ObjKey) -> i32 {
        *self.orc.crit_count.get(&unit).unwrap_or(&1)
    }
}
