//! Hero progression (`docs/spec/heroes.md`): the experience tables, experience gain and levels, skill points, and the
//! strength and agility getters. The natives are in `natives/hero.rs`; this module holds the arithmetic over
//! [`HeroObj`].
//!
//! Conventions. The original keeps the level as a counter `c = level - 1` (read by `0x6F473170`, written by
//! `0x6F473140`); [`HeroObj::level`] is `c + 1`. Experience is an int (`+0x8C`), skill points an int (`+0x90`), the
//! suspend flag `+0xEC`. Every real operation is a [`softfloat`] operation, which is bit-exact with `Game.dll`.
//!
//! Verification labels follow `docs/spec/heroes.md`: the experience builder is checked in Unicorn against the
//! original (`tools/emu/hero_xp_emu.py`, the vectors are compared by `tests/hero_xp_vectors.rs`). The level-up and
//! stat paths are read from the disassembly and covered by unit tests; they are not run in the emulator yet.

use softfloat::{arith, conv, math, Tables, F32};

use crate::natives::game::HeroObj;

/// `100.0` (start-up constant `0x6FAAE574`): the divisor of the factor column.
const FACTOR_DIVISOR: F32 = F32(0x42C8_0000);
/// `0.5` (start-up constant `0x6FAAE4D0`): the rounding term added before the experience gain is truncated.
const HALF: F32 = F32(0x3F00_0000);

/// The attributes of a hero type: base strength and agility and their per-level gains (the unit record columns
/// `STR`, `AGI`, `STRplus`, `AGIplus`). Intelligence is not modelled (see `natives/hero.rs`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeroAttributes {
    pub str_base: i32,
    pub str_plus: F32,
    pub agi_base: i32,
    pub agi_plus: F32,
}

/// The inputs of the experience builder, as the Misc reader returns them (`gamedata::misc::HeroMisc`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct XpInputs {
    /// `MaxHeroLevel`.
    pub max_hero_level: i32,
    /// `NeedHeroXP`.
    pub need: Vec<i32>,
    /// `NeedHeroXPFormulaA`, `B`, `C`.
    pub need_formula: [F32; 3],
    /// `GrantHeroXP`.
    pub grant: Vec<i32>,
    /// `GrantHeroXPFormulaA`, `B`, `C`.
    pub grant_formula: [F32; 3],
    /// `HeroFactorXP`.
    pub factor: Vec<i32>,
    /// `MaxUnitLevel`.
    pub max_unit_level: i32,
    /// `GrantNormalXP`.
    pub normal: Vec<i32>,
    /// `GrantNormalXPFormulaA`, `B`, `C`.
    pub normal_formula: [F32; 3],
}

/// The built experience tables (see [`build`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct XpTables {
    /// Need XP per index (`0x6FAAE420`, field +0). Index `i` is the threshold to pass from level `i + 1` to
    /// `i + 2`.
    need: Vec<i32>,
    /// Grant XP per index (field +4): the experience a hero of that index grants.
    grant: Vec<i32>,
    /// Factor per index (field +8, f32): `factor_list[i] * recip(100)`.
    factor: Vec<F32>,
    /// Grant XP of a normal unit per unit level index (`0x6FAAE430`).
    unit_grant: Vec<u32>,
}

impl XpTables {
    /// A table with only the need thresholds, for hosts that install them directly (the grant and factor columns are
    /// empty). `MaxHeroLevel` is the length of `need`.
    pub fn from_need_list(need: &[i32]) -> XpTables {
        XpTables {
            need: need.to_vec(),
            ..XpTables::default()
        }
    }

    /// `MaxHeroLevel` as the table was built.
    pub fn max_level(&self) -> i32 {
        self.need.len() as i32
    }

    /// The need-XP lookup of the original (`0x6F207760`): the entry of `clamp(level, 1, N) - 1`, `0` when the table
    /// is empty.
    pub fn need_xp(&self, level: i32) -> i32 {
        let n = self.need.len() as i32;
        if n == 0 {
            return 0;
        }
        self.need[(level.clamp(1, n) - 1) as usize]
    }

    /// Need XP of a hero table index (field +0), or `None` past the table.
    pub fn hero_need(&self, index: usize) -> Option<i32> {
        self.need.get(index).copied()
    }

    /// Grant XP of a hero table index (field +4), or `None` past the table.
    pub fn grant_hero_xp(&self, index: usize) -> Option<i32> {
        self.grant.get(index).copied()
    }

    /// The factor of a hero table index (field +8), or `None` past the table.
    pub fn hero_factor(&self, index: usize) -> Option<F32> {
        self.factor.get(index).copied()
    }

    /// Grant XP of a normal unit at unit level index `index`, or `None` past the table.
    pub fn grant_normal_xp(&self, index: usize) -> Option<u32> {
        self.unit_grant.get(index).copied()
    }
}

/// One recurrence series of the builder (the loops of `0x6F00ABD0`). Element `i` is the list value for
/// `i < min(n, list.len())`; beyond the list the value `v` follows `v = A * prev + B * x + C` with `x = i + x_offset`
/// (CONFIRMED by the Unicorn trace: the order of the additions is `(A*prev + B*x) + C`). The stored int is
/// `f2i(v)`; `prev` is the float `v`.
///
/// An empty list is not modelled (the original reads the word in front of its table; see `docs/spec/heroes.md`);
/// here the first `prev` is `0`.
fn recurrence(n: usize, list: &[i32], f: [F32; 3], x_offset: i32) -> Vec<i32> {
    let count = n.min(list.len());
    let mut out = vec![0i32; n];
    out[..count].copy_from_slice(&list[..count]);
    let mut prev = if count > 0 {
        conv::i2r(list[count - 1])
    } else {
        F32::ZERO
    };
    for (i, slot) in out.iter_mut().enumerate().skip(count) {
        let x = conv::i2r((i as i32).wrapping_add(x_offset));
        let sum = arith::add(arith::mul(f[0], prev), arith::mul(f[1], x));
        let v = arith::add(sum, f[2]);
        *slot = conv::f2i(v);
        prev = v;
    }
    out
}

/// Builds the experience tables from the Misc values (the builder `0x6F00ABD0`, CONFIRMED by Unicorn over
/// `tools/emu/hero_xp_emu.py` cases; `t` supplies the reciprocal table used by the factor column).
pub fn build(inputs: &XpInputs, t: &Tables) -> XpTables {
    let n = inputs.max_hero_level.max(0) as usize;
    let mut need = recurrence(n, &inputs.need, inputs.need_formula, 2);
    // The last index copies the one before it (CONFIRMED: `0x6F00ABD0` stores field +0 of entry N-1 from entry N-2).
    if n > 1 {
        need[n - 1] = need[n - 2];
    }
    let grant = recurrence(n, &inputs.grant, inputs.grant_formula, 1);

    // Factor column: `I2R(value) * recip(100.0)` (CONFIRMED: the builder multiplies by the reciprocal, it does not
    // divide). Past the list the last factor repeats.
    let fcount = n.min(inputs.factor.len());
    let mut factor = vec![F32::ZERO; n];
    let recip = arith::recip(FACTOR_DIVISOR, t);
    for (slot, &value) in factor.iter_mut().zip(inputs.factor.iter()).take(fcount) {
        *slot = arith::mul(conv::i2r(value), recip);
    }
    if fcount > 0 {
        let last = factor[fcount - 1];
        for slot in factor.iter_mut().skip(fcount) {
            *slot = last;
        }
    }

    let m = inputs.max_unit_level.max(0) as usize;
    let unit_grant = recurrence(m, &inputs.normal, inputs.normal_formula, 1)
        .into_iter()
        .map(|v| v as u32)
        .collect();

    XpTables {
        need,
        grant,
        factor,
        unit_grant,
    }
}

/// Whether the hero is at its maximum level (`0x6F207F10`: `MaxHeroLevel <= level`, with `level = c + 1`).
fn at_max(t: &XpTables, counter: i32) -> bool {
    t.max_level() <= counter.wrapping_add(1)
}

/// Adds experience (`0x6F20B080`, the plain branch: no handicap scaling, no kill distribution). `value` is the
/// amount as f32. The gain is `f2i(floor(value + 0.5))` (CONFIRMED: `0x6F6EF330` then `0x6F6EEC20`).
///
/// A suspended hero (`+0xEC`) and a zero amount change nothing. After the gain the level loop runs: while the next
/// level's need is met and the hero is below the maximum, the level rises by one and grants a skill point while the
/// new counter is below `MaxHeroLevel`. At the maximum the experience is set to the need of the current level
/// (CONFIRMED). When the hero leveled and `slack` is known (the learnable ability levels of its five hero abilities),
/// the skill points are capped at it (`0x6F20B080` tail; `None` when the hero abilities are not modelled).
pub fn add_xp(t: &XpTables, h: &mut HeroObj, value: F32, slack: Option<i32>) {
    if h.xp_suspended || value.0 & 0x7FFF_FFFF == 0 {
        return;
    }
    let gain = conv::f2i(math::floor(arith::add(value, HALF)));
    h.xp = h.xp.wrapping_add(gain);

    let mut counter = h.level - 1;
    let mut leveled = false;
    if t.need_xp(counter + 1) <= h.xp {
        while !at_max(t, counter) {
            leveled = true;
            counter += 1;
            if counter < t.max_level() {
                h.skill_points = h.skill_points.wrapping_add(1);
            }
            if t.need_xp(counter + 1) > h.xp {
                break;
            }
        }
    }
    h.level = counter + 1;
    if at_max(t, counter) {
        h.xp = t.need_xp(counter + 1);
    }
    if leveled {
        if let Some(s) = slack {
            if s < h.skill_points {
                h.skill_points = s;
            }
        }
    }
}

/// `SetHeroXP`: raises the experience to `target` through [`add_xp`]; a lower target does nothing (CONFIRMED by the
/// decompile of the native: the delta must be positive).
pub fn set_xp(t: &XpTables, h: &mut HeroObj, target: i32, slack: Option<i32>) {
    let delta = target.wrapping_sub(h.xp);
    if delta > 0 {
        add_xp(t, h, conv::i2r(delta), slack);
    }
}

/// `SetHeroLevel`: when the level is positive and not above the requested one, the experience is raised to the
/// need-XP entry `level - 1` (`0x6F26EB50` with the argument `level - 1`, which the lookup clamps). A level below
/// the current one does nothing. The literal reading gives `level 1` the entry 0 (LIKELY, see `docs/spec/heroes.md`).
pub fn set_level(t: &XpTables, h: &mut HeroObj, level: i32, slack: Option<i32>) {
    if level <= 0 || h.level > level {
        return;
    }
    let target = t.need_xp(level - 1);
    let delta = target.wrapping_sub(h.xp);
    if delta > 0 {
        add_xp(t, h, conv::i2r(delta), slack);
    }
}

/// Removes one level (`0x6F20B4D0`). At level 1 the experience is reset to 0 and nothing else changes (`false`).
/// Otherwise the counter drops, one skill point is spent when there is one (the ability-level part of the original is
/// not modelled, see `docs/spec/heroes.md`), and the experience becomes the need of the new level (or 0 at level 1).
fn strip_one(t: &XpTables, h: &mut HeroObj) -> bool {
    if h.level <= 1 {
        h.xp = 0;
        return false;
    }
    h.level -= 1;
    if h.skill_points >= 1 {
        h.skill_points -= 1;
    }
    let counter = h.level - 1;
    h.xp = if counter == 0 { 0 } else { t.need_xp(counter) };
    true
}

/// `UnitStripHeroLevel`: removes up to `count` levels (`0x6F20B6E0`). Nothing happens at level 1 with no skill
/// points. The count is limited to the levels above 1, and the loop stops at the first level that cannot be removed.
/// Returns whether a level was removed.
pub fn strip_levels(t: &XpTables, h: &mut HeroObj, count: u32) -> bool {
    if h.level <= 1 && h.skill_points == 0 {
        return false;
    }
    // The original limits the count to `level` (its counter plus one) when that is not larger.
    let mut n = count;
    if (h.level as u32) <= n {
        n = h.level as u32;
    }
    let mut removed = false;
    while n != 0 {
        let one = strip_one(t, h);
        removed |= one;
        n -= 1;
        if !one {
            break;
        }
    }
    removed
}

/// `UnitModifySkillPoints` (`0x6F208B80`): changes the skill points by `delta`. The change is limited so the points
/// never go below zero, and, when `slack` is known, capped at the learnable ability levels. Returns `false` when the
/// change is zero after the limits.
pub fn modify_skill_points(h: &mut HeroObj, delta: i32, slack: Option<i32>) -> bool {
    let cur = h.skill_points;
    let mut d = delta;
    if cur.wrapping_neg() != d && cur <= d.wrapping_neg() {
        d = cur.wrapping_neg();
    }
    if d == 0 {
        return false;
    }
    if let Some(s) = slack {
        if s < d {
            d = s;
        }
    }
    h.skill_points = cur.wrapping_add(d);
    true
}

/// A strength or agility value (`0x6F2777B0`, `0x6F2778B0`; CONFIRMED by disassembly): `base + trunc(gain * counter)`,
/// where the counter is the level minus one. Item bonuses are not modelled (they count as 0).
pub fn attribute(base: i32, gain: F32, counter: i32) -> i32 {
    base.wrapping_add(conv::f2i(arith::mul(gain, conv::i2r(counter))))
}

/// The base that makes [`attribute`] read `new` (`SetHeroStr` / `SetHeroAgi`: the base moves by the difference to
/// the current value without items, CONFIRMED by the setters `0x6F277950`, `0x6F2779B0`).
pub fn rebase(base: i32, gain: F32, counter: i32, new: i32) -> i32 {
    let current = attribute(base, gain, counter);
    base.wrapping_add(new.wrapping_sub(current))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> Option<Tables> {
        let path = std::env::var("GAME_DLL").ok()?;
        let bytes = std::fs::read(path).ok()?;
        Tables::from_game_dll(&bytes).ok()
    }

    fn f(v: f32) -> F32 {
        F32(v.to_bits())
    }

    #[test]
    fn recurrence_matches_the_hand_trace() {
        // List [600] (count 1), A=0.5, B=0.25, C=1.0, x offset 2: index 1 has x = 3, index 2 has x = 4.
        // index 1: 0.5*600 + 0.25*3 + 1 = 301.75 -> 301; index 2: 0.5*301.75 + 0.25*4 + 1 = 152.875 -> 152.
        let out = recurrence(3, &[600], [f(0.5), f(0.25), f(1.0)], 2);
        assert_eq!(out, vec![600, 301, 152]);
    }

    #[test]
    fn build_copies_the_last_need_and_uses_the_reciprocal() {
        let Some(t) = tables() else {
            eprintln!("GAME_DLL not set: the factor column needs the reciprocal table");
            return;
        };
        let inputs = XpInputs {
            max_hero_level: 3,
            need: vec![200],
            need_formula: [f(0.5), f(0.25), f(1.0)],
            factor: vec![10, 20],
            ..XpInputs::default()
        };
        let x = build(&inputs, &t);
        // need: 200, 0.5*200 + 0.25*3 + 1 = 101.75 -> 101, then the last index copies index 1.
        assert_eq!(x.need, vec![200, 101, 101]);
        assert_eq!(x.hero_factor(0), Some(arith::mul(conv::i2r(10), arith::recip(FACTOR_DIVISOR, &t))));
        assert_eq!(x.hero_factor(2), x.hero_factor(1));
    }

    #[test]
    fn need_lookup_clamps_levels() {
        let t = XpTables {
            need: vec![200, 400, 600],
            ..XpTables::default()
        };
        assert_eq!(t.need_xp(0), 200);
        assert_eq!(t.need_xp(1), 200);
        assert_eq!(t.need_xp(3), 600);
        assert_eq!(t.need_xp(9), 600);
        assert_eq!(XpTables::default().need_xp(2), 0);
    }

    #[test]
    fn level_up_grants_skill_points_and_clamps_at_max() {
        let t = XpTables {
            need: vec![200, 400, 600],
            ..XpTables::default()
        };
        let mut h = HeroObj::default();
        add_xp(&t, &mut h, conv::i2r(450), None);
        // 450 passes the need of level 2 (200) and level 3 (400): two level-ups, two skill points. Level 3 is the
        // maximum (N = 3), so the experience is clamped to the need of the current level (600).
        assert_eq!(h.level, 3);
        assert_eq!(h.skill_points, 2);
        assert_eq!(h.xp, 600);
        add_xp(&t, &mut h, conv::i2r(10_000), None);
        assert_eq!((h.level, h.xp, h.skill_points), (3, 600, 2));
    }

    #[test]
    fn suspended_or_zero_gain_nothing() {
        let t = XpTables {
            need: vec![200],
            ..XpTables::default()
        };
        let mut h = HeroObj {
            xp_suspended: true,
            ..HeroObj::default()
        };
        add_xp(&t, &mut h, conv::i2r(500), None);
        assert_eq!((h.xp, h.level), (0, 1));
        h.xp_suspended = false;
        add_xp(&t, &mut h, F32::ZERO, None);
        assert_eq!(h.xp, 0);
    }

    #[test]
    fn strip_stops_at_level_one_and_resets_xp() {
        let t = XpTables {
            need: vec![200, 400, 600],
            ..XpTables::default()
        };
        let mut h = HeroObj {
            level: 3,
            xp: 500,
            skill_points: 1,
            ..HeroObj::default()
        };
        assert!(strip_levels(&t, &mut h, 5));
        // Two levels removed: level 1, experience 0; the second strip finds level 1 and stops.
        assert_eq!((h.level, h.xp, h.skill_points), (1, 0, 0));
        assert!(!strip_levels(&t, &mut h, 1));
    }

    #[test]
    fn skill_points_cannot_go_negative() {
        let mut h = HeroObj {
            skill_points: 2,
            ..HeroObj::default()
        };
        assert!(modify_skill_points(&mut h, -5, None));
        assert_eq!(h.skill_points, 0);
        assert!(!modify_skill_points(&mut h, -1, None));
        assert!(modify_skill_points(&mut h, 3, Some(1)));
        assert_eq!(h.skill_points, 1);
    }

    #[test]
    fn attribute_truncates_the_gain() {
        // 2.5 * 3 = 7.5 -> 7
        assert_eq!(attribute(20, f(2.5), 3), 27);
        let base = rebase(20, f(2.5), 3, 30);
        assert_eq!(attribute(base, f(2.5), 3), 30);
    }
}
