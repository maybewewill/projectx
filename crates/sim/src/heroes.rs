//! Hero progression (`docs/spec/heroes.md`): the experience tables, experience gain and levels, skill points, and the
//! strength and agility getters. The natives are in `natives/hero.rs`; this module holds the arithmetic over
//! [`HeroObj`].
//!
//! Conventions. The original keeps the level as a counter `c = level - 1` (read by `0x6F473170`, written by
//! `0x6F473140`); [`HeroObj::level`] is `c + 1`. Experience is an int (`+0x8C`), skill points an int (`+0x90`), the
//! suspend flag `+0xEC`. Every real operation is a [`softfloat`] operation, which is bit-exact with `Game.dll`.
//!
//! Verification labels follow `docs/spec/heroes.md`: the experience builder is checked in Unicorn against the
//! original (`tools/emu/hero_xp_emu.py`, the vectors are compared by `tests/hero_xp_vectors.rs`). The experience engine
//! (plain and kill branches, level walk, level-up tail) and the kill split are checked the same way
//! (`tools/emu/hero_kill_xp_emu.py`, `tests/kill_xp_vectors.rs`, `docs/spec/heroes.md` section 8). The stat paths are
//! read from the disassembly and covered by unit tests; they are not run in the emulator yet.

use softfloat::{arith, conv, math, Tables, F32};

use crate::natives::game::HeroObj;

/// `100.0` (start-up constant `0x6FAAE574`): the divisor of the factor column.
const FACTOR_DIVISOR: F32 = F32(0x42C8_0000);
/// `0.5` (start-up constant `0x6FAAE4D0`): the rounding term added before the experience gain is truncated.
const HALF: F32 = F32(0x3F00_0000);

/// The attributes of a hero type: base strength, agility and intelligence and their per-level gains (the unit record
/// columns `STR`, `AGI`, `INT`, `STRplus`, `AGIplus`, `INTplus`). The intelligence base is the counter behind the
/// reference at hero+0x7C, written from unit record `+0x28` at creation (`docs/spec/heroes.md`, section 6).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeroAttributes {
    pub str_base: i32,
    pub str_plus: F32,
    pub agi_base: i32,
    pub agi_plus: F32,
    pub int_base: i32,
    pub int_plus: F32,
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

    /// The tables from their columns as the builder leaves them (need, factor, grant XP of a hero, grant XP of a
    /// normal unit per level). Used by hosts and by the Unicorn vectors, which give the columns directly.
    pub fn from_columns(need: Vec<i32>, factor: Vec<F32>, grant: Vec<i32>, unit_grant: Vec<u32>) -> XpTables {
        XpTables {
            need,
            grant,
            factor,
            unit_grant,
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
    grant_tail(t, h, slack);
}

/// The level loop and the tail of `0x6F20B080`, after the experience of the call has been set (shared by the plain and
/// the kill branches). See [`add_xp`] for the rules; `slack` is the skill-point cap.
fn grant_tail(t: &XpTables, h: &mut HeroObj, slack: Option<i32>) {
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

/// `1.0` (start-up constant `0x6FAAE4F4`).
const ONE: F32 = F32(0x3F80_0000);

/// Zero of either sign: the `fucompp` equality of the original against `0.0`.
fn is_zero(x: F32) -> bool {
    x.0 & 0x7FFF_FFFF == 0
}

/// The upgrade-based bonus of the kill branch of `0x6F20B080` (`0x6F40B340`, `0x6F40B380`, `0x6F32E020`,
/// `0x6F32E040`). `b1` is the count `0x6F40B340(player, 'HERO', 11)` returns for the recipient's player, `slots` the
/// number of slots the recipient's type answers `0x6F40B380` for, `codes` the nine rawcodes at `0x6FA6F044`, and
/// `levels` / `value` the per-code level count (`0x6F32E020`) and value of a level (`0x6F32E040`, index clamped).
pub struct BonusEnv<'a> {
    pub b1: i32,
    pub slots: i32,
    pub codes: [u32; 9],
    pub levels: &'a dyn Fn(u32) -> i32,
    pub value: &'a dyn Fn(u32, i32) -> F32,
}

/// What the kill branch reads from the recipient: the owner's handicap (player `+0x2A4`), the bonus source, and the
/// five hero ability slots whose learnable levels cap the skill points after a level-up (`0x6F207C80`, `0x6F207CC0`:
/// `(min, max)` per slot; the cap is `sum of max(0, max - min)`).
pub struct EngineEnv<'a> {
    pub handicap: F32,
    pub bonus: BonusEnv<'a>,
    pub caps: [(i32, i32); 5],
}

/// The bonus `local` of `0x6F20B080`: walks the codes of the recipient while its slot exists (at most nine), and takes
/// the value of the last code that has levels, at level `min(levels, b1) - 1` (CONFIRMED by the Unicorn run; the walk
/// stops at the first missing slot or when `b1` is zero).
fn bonus_of(env: &BonusEnv) -> F32 {
    let mut local = F32::ZERO;
    let mut i = 0usize;
    while env.b1 != 0 && i < 9 && (i as i32) < env.slots {
        let code = env.codes[i];
        i += 1;
        if code == 0 {
            continue;
        }
        let k = (env.levels)(code);
        if k != 0 {
            let level = (k as u32).min(env.b1 as u32) as i32;
            local = (env.value)(code, level - 1);
        }
    }
    local
}

/// The level-walk branch of `0x6F20B080` (the `value` is spread over the levels with the hero factor of each level).
/// From the experience `xp` and the level counter (`level - 1`), with the walk value `v`:
///
/// - `x` starts as `i2r(xp)`; while the counter is below `MaxHeroLevel - 1` and the factor `T` of the counter is not
///   zero: `x += v * T`, the counter moves up, `need` = `i2r(need of the new counter)`; if `need < x` then `x := need`,
///   `v := (x_before - need) / T` and the walk goes on while the counter is below the last level; otherwise it stops.
/// - the result is `f2i(round(x))` and it **sets** the experience (no addition).
///
/// Verified against the original by `tools/emu/hero_kill_xp_emu.py` (mode A, the walk flag).
fn walk_xp(t: &XpTables, tb: &Tables, level: i32, xp: i32, mut v: F32) -> i32 {
    let m = t.max_level();
    let mut x = conv::i2r(xp);
    let mut c = level - 1;
    if c < m - 1 {
        loop {
            let tf = usize::try_from(c)
                .ok()
                .and_then(|i| t.hero_factor(i))
                .unwrap_or(F32::ZERO);
            if is_zero(tf) {
                break;
            }
            x = arith::add(x, arith::mul(v, tf));
            c += 1;
            let nf = conv::i2r(t.need_xp(c));
            if !arith::lt(nf, x) {
                break;
            }
            let d = arith::sub(x, nf);
            let q = arith::div(d, tf, tb);
            x = nf;
            v = q;
            if c >= m - 1 {
                break;
            }
        }
    }
    conv::f2i(math::round(x))
}

/// The flags of the experience engine (`0x6F20B080`): `handicap` (`param_5`) scales the value by the owner's handicap,
/// `walk` (`param_6`) takes the level walk instead of the plain gain, `range` (`param_7`) applies the upgrade bonus.
/// A kill split sets all three; the plain `AddHeroXP` sets none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gain {
    pub handicap: bool,
    pub walk: bool,
    pub range: bool,
}

/// The experience engine `0x6F20B080` for one hero: `value` is the amount as f32, `gain` the flags. The show flag and
/// the effect calls carry no numbers and are not modelled.
///
/// Order (CONFIRMED by the disassembly and the Unicorn run): suspended or zero is a no-op; `v = value * handicap`
/// (when `handicap`); `v = v * (1 + bonus)` (when `range` and the bonus is not zero); the gain (plain: `xp += f2i(round(v))`,
/// walk: [`walk_xp`]); then the shared tail with the skill-point cap of `env.caps`.
pub fn experience_engine(t: &XpTables, tb: &Tables, h: &mut HeroObj, value: F32, gain: Gain, env: &EngineEnv) {
    if h.xp_suspended || is_zero(value) {
        return;
    }
    let mut v = if gain.handicap {
        arith::mul(value, env.handicap)
    } else {
        value
    };
    let bonus = if gain.range {
        bonus_of(&env.bonus)
    } else {
        F32::ZERO
    };
    if !is_zero(bonus) {
        v = arith::mul(v, arith::add(ONE, bonus));
    }
    h.xp = if gain.walk {
        walk_xp(t, tb, h.level, h.xp, v)
    } else {
        h.xp.wrapping_add(conv::f2i(math::round(v)))
    };
    let slack = env
        .caps
        .iter()
        .fold(0i32, |s, &(lo, hi)| s.wrapping_add(if lo < hi { hi - lo } else { 0 }));
    grant_tail(t, h, Some(slack));
}

/// The victim of a kill as the amount function `0x6F2A0230` and the split `0x6F2A3190` read it.
#[derive(Debug, Clone, PartialEq)]
pub struct KillVictim {
    /// The type id starts with an uppercase letter (`0x6F2A0230` then uses the hero table; not an illusion).
    pub type_upper: bool,
    /// Unit flags bit 30 (illusion).
    pub illusion: bool,
    /// Unit `+0x248` bit 2 (summoned): the `SummonedKillFactor` applies to the amount.
    pub summoned: bool,
    /// The owner index (`vf59`). Owners 12 and above take the level walk (`0x6F2A3190`, CONFIRMED by the run).
    pub owner: i32,
    /// The victim's level as `0x6F2A0210` answers it: the hero counter + 1, or the unit level (`0x6F29DCA0`).
    pub level: i32,
    /// The ability `0x6F0787D0` finds for the victim: `(answers the vf 0x36C flag, level override +0xE4)`.
    pub ability: Option<(bool, i32)>,
}

/// The Misc values of the kill split that are not in [`XpTables`].
#[derive(Debug, Clone, PartialEq)]
pub struct KillEnv {
    /// `MaxUnitLevel`.
    pub max_unit: i32,
    /// `SummonedKillFactor` (a real).
    pub summon_factor: F32,
}

/// The amount `K` of `0x6F2A0230`: the level (overridden by the victim's ability when it answers the flag), clamped to
/// `[1, max]` (a level below 1 gives 0), the grant of the table of the victim (heroes: `GrantHeroXP`, others:
/// `GrantNormalXP`) at index `level - 1`, times the summoned factor when the victim is summoned, truncated to an int.
pub fn kill_amount(t: &XpTables, v: &KillVictim, env: &KillEnv) -> i32 {
    let mut level = v.level;
    if let Some((true, over)) = v.ability {
        level = over;
    }
    if level < 1 {
        return 0;
    }
    let hero_key = v.type_upper && !v.illusion;
    let max = if hero_key { t.max_level() } else { env.max_unit };
    if level > max {
        level = max;
    }
    let index = (level - 1) as usize;
    let grant = if hero_key {
        t.grant_hero_xp(index)
    } else {
        t.grant_normal_xp(index).map(|g| g as i32)
    }
    .unwrap_or(0);
    let amount = conv::i2r(grant);
    let amount = if v.summoned {
        arith::mul(amount, env.summon_factor)
    } else {
        amount
    };
    conv::f2i(amount)
}

/// The kill split `0x6F2A3190` on the chosen recipient list (the in-range list when it is not empty, else the global
/// one; the caller chooses, in list order): the amount `K` (see [`kill_amount`]) is divided by the list count `N` in
/// softfloat (`F = i2r(K) / i2r(N)`, once, before any recipient), and when `F > 0` every recipient gets
/// [`experience_engine`] with `F` as the value, the handicap flag and the range flag set, and the walk flag set when the
/// victim's owner is 12 or above. Returns `K`.
pub fn distribute_kill(
    t: &XpTables,
    tb: &Tables,
    victim: &KillVictim,
    env: &KillEnv,
    recipients: &mut [(HeroObj, EngineEnv)],
) -> i32 {
    let k = kill_amount(t, victim, env);
    let n = recipients.len() as i32;
    if n == 0 {
        return k;
    }
    let share = arith::div(conv::i2r(k), conv::i2r(n), tb);
    if !arith::lt(F32::ZERO, share) {
        return k;
    }
    let walk = victim.owner >= 12;
    let gain = Gain {
        handicap: true,
        walk,
        range: true,
    };
    for (hero, e) in recipients.iter_mut() {
        experience_engine(t, tb, hero, share, gain, e);
    }
    k
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

/// The intelligence value (`0x6F0DB3A0`, Unicorn-checked by `tools/emu/hero_int_emu.py`): the same form as
/// [`attribute`], `base + trunc(gain * level counter)`, with the base read from the counter behind hero+0x7C.
pub fn intelligence(base: i32, gain: F32, counter: i32) -> i32 {
    attribute(base, gain, counter)
}

/// The base that makes [`attribute`] read `new` (`SetHeroStr` / `SetHeroAgi`: the base moves by the difference to
/// the current value without items, CONFIRMED by the setters `0x6F277950`, `0x6F2779B0`). `SetHeroInt` uses the same
/// rule (`0x6F277980`, which adds the difference to the base counter through `0x6F20A260`).
pub fn rebase(base: i32, gain: F32, counter: i32, new: i32) -> i32 {
    let current = attribute(base, gain, counter);
    base.wrapping_add(new.wrapping_sub(current))
}

// ---------------------------------------------------------------------------------------------------------------------
// Derived attribute bonuses (docs/spec/heroes.md, section 9). Each reader is `value = f(total) * key`, with the
// attribute total of the matching core (CONFIRMED by Unicorn, `tools/emu/hero_bonus_emu.py`).
// ---------------------------------------------------------------------------------------------------------------------

/// The primary attribute total (`+0xCC`), chosen as the reader `0x6F207F40` does: `1` strength (core `0x6F0DB300`),
/// `2` intelligence (`0x6F0DB3A0`), anything else agility (`0x6F0DB350`). CONFIRMED by the three cores in Unicorn.
pub fn primary_total(primary: i32, str_total: i32, agi_total: i32, int_total: i32) -> i32 {
    match primary {
        1 => str_total,
        2 => int_total,
        _ => agi_total,
    }
}

/// The eight Misc reals of the derived bonuses (`gamedata::misc::AttrBonusMisc`, copied by the engine; `sim` does not
/// depend on `gamedata`). Absent keys are `0.0` (the reader's default).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BonusRates {
    pub str_hit_point: F32,
    pub str_regen: F32,
    pub str_attack: F32,
    pub agi_defense_base: F32,
    pub agi_defense: F32,
    pub agi_move: F32,
    pub agi_attack_speed: F32,
    pub int_mana: F32,
}

/// The derived bonuses of a hero for the current attributes (section 9). The values are the readers' results; the
/// hit-point and mana values are the ones the setters send to the unit (see [`max_life_delta`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DerivedBonuses {
    /// Maximum life bonus: `I2R(str) * StrHitPointBonus * factor` (reader `0x6F208060`; the factor is the real at
    /// the reader's second pointer, `*(global + 0x29C)` per the setter `0x6F20A160`, value UNSURE).
    pub hit_points: F32,
    /// Life regeneration bonus: `I2R(str) * StrRegenBonus` (reader `0x6F2080F0`).
    pub regen: F32,
    /// Attack damage bonus: `I2R(primary) * StrAttackBonus` (reader `0x6F207F40`).
    pub attack: F32,
    /// Armor bonus: `AgiDefenseBase + I2R(agi) * AgiDefenseBonus` (reader `0x6F207FB0`).
    pub armor: F32,
    /// Movement speed bonus: `I2R(agi) * AgiMoveBonus` (reader `0x6F208170`).
    pub move_speed: F32,
    /// Attack speed bonus: `I2R(agi) * AgiAttackSpeedBonus` (reader `0x6F2081F0`).
    pub attack_speed: F32,
    /// Maximum mana bonus: `I2R(int) * IntManaBonus` (reader `0x6F208270`).
    pub mana: F32,
}

/// `I2R(total) * per`, the shape of every reader except the armor base and the hit points.
fn scaled(total: i32, per: F32) -> F32 {
    arith::mul(conv::i2r(total), per)
}

/// The derived bonuses of `h` for `misc` and the hit-point factor (see [`DerivedBonuses::hit_points`]).
pub fn derived_bonuses(h: &HeroObj, rates: &BonusRates, factor: F32) -> DerivedBonuses {
    let counter = h.level - 1;
    let str_total = attribute(h.str_base, h.str_plus, counter);
    let agi_total = attribute(h.agi_base, h.agi_plus, counter);
    let int_total = intelligence(h.int_base, h.int_plus, counter);
    let primary = primary_total(h.primary, str_total, agi_total, int_total);
    DerivedBonuses {
        hit_points: arith::mul(scaled(str_total, rates.str_hit_point), factor),
        regen: scaled(str_total, rates.str_regen),
        attack: scaled(primary, rates.str_attack),
        armor: arith::add(rates.agi_defense_base, scaled(agi_total, rates.agi_defense)),
        move_speed: scaled(agi_total, rates.agi_move),
        attack_speed: scaled(agi_total, rates.agi_attack_speed),
        mana: scaled(int_total, rates.int_mana),
    }
}

/// The change of the maximum life that a strength change causes: the difference of the hit-point bonus before and
/// after the attribute change (the setter sends `new - old` to the life sink, `0x6F209000`, CONFIRMED arithmetic shape;
/// the stored old value is LIKELY the previous bonus, UNSURE).
pub fn max_life_delta(before: &DerivedBonuses, after: &DerivedBonuses) -> F32 {
    arith::sub(after.hit_points, before.hit_points)
}

/// The change of the maximum mana an intelligence change causes (`0x6F209210` path, same shape as [`max_life_delta`]).
pub fn max_mana_delta(before: &DerivedBonuses, after: &DerivedBonuses) -> F32 {
    arith::sub(after.mana, before.mana)
}

/// The life cells of a unit as the life sink `0x6F2AC450` sees them (`docs/spec/heroes.md`, section 10).
///
/// The original keeps the current life at `+0x78`, the lower clamp at `+0x80` and the maximum at `+0x84` (the stat
/// block helpers and the pair setter `0x6F4A9390` are read in section 10). The model has no lower clamp field: it is
/// passed in as `floor` (`0.0` for every unit the model creates, LIKELY; the writer of `+0x80` is not found).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LifeCells {
    /// Current life (`+0x78`).
    pub life: F32,
    /// Maximum life (`+0x84`).
    pub max: F32,
    /// Lower clamp of the pair (`+0x80`).
    pub floor: F32,
}

/// The unit life sink `0x6F2AC450(unit, value, mode)` (CONFIRMED by Unicorn: `tools/emu/life_sink_emu.py`, the
/// vectors are in `tests/life_sink_vectors.rs`). `flagged` is the unit flag `+0x5C` bit 8 read by `vt+0x13C`.
///
/// Steps, in the original's order:
///
/// 1. `value == 0.0` (either zero) returns at once.
/// 2. `d = floor(value + 0.5)` (`0x6F6EF330`): every change is rounded half up to a whole number before it is used.
/// 3. `min = 0.0` when `flagged`, else `1.0` (the living minimum of the mode-1 path).
/// 4. Mode 1 (temporary, `permanent` false): proportional (without `tables` only the maximum moves). `ratio = life / max` (`0` when `max` is zero), the maximum
///    becomes `max + d`, the life becomes `floor(newmax * ratio + 0.5)` raised to `min`.
/// 5. Mode 2 (permanent), and mode 3 when `d >= 0`: absolute. Non-negative `d` (zero included): the maximum rises by `d` and the life
///    rises by `d` (clamped by the new maximum). Non-positive `d`: the life is `max(life + d, min)` (read before the
///    maximum changes), then the maximum becomes `max + d`.
/// 6. Any other mode, and mode 3 when `d < 0`: only the maximum changes by `d` (the life is clamped to it).
///
/// Setters: `set_max` writes the maximum and clamps the life to `[floor, max]` (`0x6F4A9390`). `set_life` clamps a
/// non-positive value to `0.0` (`vt+0x124`), then sets through the delta path `life + (v - life)` and clamps to
/// `[floor, max]` (`0x6F477350` / `0x6F4A92B0`). `add_life` is `set_core(d + life)` (`vt+0x128`, no zero clamp).
pub fn life_sink(c: &mut LifeCells, flagged: bool, value: F32, mode: u32, tables: Option<&Tables>) {
    if value.to_bits() & 0x7fff_ffff == 0 {
        return;
    }
    let d = math::floor(arith::add(value, HALF));
    let min = if flagged { F32::ZERO } else { F32::ONE };
    match mode {
        1 => {
            let new_max = arith::add(c.max, d);
            let Some(t) = tables else {
                // The division needs the reciprocal table of the original (`MissingTables`): only the maximum moves.
                set_max(c, new_max);
                return;
            };
            let ratio = if c.max.to_bits() & 0x7fff_ffff == 0 {
                F32::ZERO
            } else {
                arith::div(c.life, c.max, t)
            };
            set_max(c, new_max);
            let target = math::floor(arith::add(arith::mul(c.max, ratio), HALF));
            let target = if arith::lt(target, min) { min } else { target };
            set_life(c, target);
        }
        2 => absolute(c, d, min),
        3 if arith::le(F32::ZERO, d) => absolute(c, d, min),
        _ => {
            let new_max = arith::add(c.max, d);
            set_max(c, new_max);
        }
    }
}

/// The absolute branches of [`life_sink`] (modes 2 and 3 with the rules of step 5).
fn absolute(c: &mut LifeCells, d: F32, min: F32) {
    // Zero takes the rising branch (the flag test of the original falls through on equal), NaN the falling one.
    if arith::le(F32::ZERO, d) {
        let new_max = arith::add(c.max, d);
        set_max(c, new_max);
        add_life(c, d);
    } else {
        let lowered = arith::add(c.life, d);
        let target = if arith::lt(lowered, min) { min } else { lowered };
        set_life(c, target);
        let new_max = arith::add(c.max, d);
        set_max(c, new_max);
    }
}

/// `0x6F4A9390`: the maximum is written and the life is clamped to `[floor, max]`.
fn set_max(c: &mut LifeCells, new_max: F32) {
    c.max = new_max;
    c.life = clamp_pair(c.life, c.floor, c.max);
}

/// The clamp of the maximum setter `0x6F4A9390` (through `0x6F477400`). An inverted pair (`hi < lo`) gives `hi`;
/// otherwise below `lo` gives `lo` and above `hi` gives `hi`. CONFIRMED on a 90-point grid in Unicorn (current life,
/// floor and maximum in {-5..300}, 0 mismatches).
pub fn clamp_pair(v: F32, lo: F32, hi: F32) -> F32 {
    if arith::lt(hi, lo) {
        hi
    } else if arith::lt(v, lo) {
        lo
    } else if arith::lt(hi, v) {
        hi
    } else {
        v
    }
}

/// The clamp of the life setter `0x6F4A92B0` (through `0x6F477350`): below `lo` gives `lo`, above `hi` gives `hi`,
/// with no inverted-pair test. CONFIRMED by the life sink vectors (an inverted pair gives the floor, case 83).
pub fn clamp_life(v: F32, lo: F32, hi: F32) -> F32 {
    if arith::lt(v, lo) {
        lo
    } else if arith::lt(hi, v) {
        hi
    } else {
        v
    }
}

/// `vt+0x124` (`0x6F28B1F0`): a non-positive value becomes `0.0`, then the value is set.
fn set_life(c: &mut LifeCells, value: F32) {
    let v = if arith::le(value, F32::ZERO) {
        F32::ZERO
    } else {
        value
    };
    set_core(c, v);
}

/// `0x6F477350` -> `0x6F4A92B0`: the new life is `life + (v - life)` (the delta is formed first), clamped.
fn set_core(c: &mut LifeCells, v: F32) {
    let cur = c.life;
    let delta = arith::sub(v, cur);
    // The original's helpers take the first operand from EDX and the second from the stack, so `add` is formed as
    // `cur + delta` (zero signs follow this order, see the probes in the notes of section 10).
    let next = arith::add(cur, delta);
    c.life = clamp_life(next, c.floor, c.max);
}

/// `vt+0x128` (`0x6F28B230`): the life changes by `d` (the sum is formed first, then set through [`set_core`]).
fn add_life(c: &mut LifeCells, d: F32) {
    let sum = arith::add(d, c.life);
    set_core(c, sum);
}

// ---------------------------------------------------------------------------------------------------------------------
// Kill candidates (docs/spec/heroes.md, section 8.3). The per-candidate routine `0x6F28B990` gates and tests each hero
// of the killer's candidate list. Checked in Unicorn by `tools/emu/hero_candidate_emu.py` (2,000 cases with the
// normaliser, the distance and the comparison run as the binary runs them). The position normaliser is NOT modelled:
// its cell term is not decoded, so the candidate positions are not confirmed (UNSURE), and nothing calls these
// helpers from the death path yet.
// ---------------------------------------------------------------------------------------------------------------------

/// The unit flags that exclude a kill candidate: `0x40000100` (bit 30, the illusion flag, and bit 8, `0x100`, which
/// the removal sequence sets; death alone does not set it, LIKELY from `docs/spec/unit-core.md`).
pub const CANDIDATE_EXCLUDED_FLAGS: u32 = 0x4000_0100;

/// Whether a hero passes the eligibility of `0x6F28B990` (CONFIRMED): `MaxLevelHeroesDrainExp` (`drain_on`) or the hero
/// below its maximum level (`at_max` is the answer of `0x6F26EB10`), the flags free of [`CANDIDATE_EXCLUDED_FLAGS`],
/// and the candidate's owner bit set in the killer's shared-experience mask (`shares_xp`, the test `0x6F3A34E0`). No
/// life test appears in the routine, so a dead hero that is not removed passes (LIKELY, see `docs/spec/heroes.md`).
pub fn is_candidate(drain_on: bool, at_max: bool, flags: u32, shares_xp: bool) -> bool {
    (drain_on || !at_max) && flags & CANDIDATE_EXCLUDED_FLAGS == 0 && shares_xp
}

/// The range test of `0x6F28B990` (CONFIRMED by Unicorn): a candidate is listed in range unless `v < d`, with `d` the
/// squared distance of the normalised positions and `v = R * R` (the record's `+0x0C`, the `HeroExpRange` squared,
/// with no division by 32). A tie is listed. NaN on either side is listed too: the x87 compare is unordered and the
/// original rejects only the strictly-less case (`fcompp`, `test ah,5`, `jnp`).
pub fn in_range(d: F32, v: F32) -> bool {
    !arith::lt(v, d)
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
    fn kill_amount_uses_the_victim_table_clamp_and_summon_factor() {
        // Hero table: need, factor, grant (index 1 = 20, index 2 = 30). Unit table: 5, 6.
        let t = XpTables::from_columns(vec![100, 200, 300], vec![F32::ZERO; 3], vec![10, 20, 30], vec![5, 6]);
        let env = KillEnv {
            max_unit: 2,
            summon_factor: f(0.5),
        };
        let hero = |level: i32| KillVictim {
            type_upper: true,
            illusion: false,
            summoned: false,
            owner: 0,
            level,
            ability: None,
        };
        assert_eq!(kill_amount(&t, &hero(2), &env), 20);
        assert_eq!(kill_amount(&t, &hero(9), &env), 30, "level clamps to MaxHeroLevel");
        assert_eq!(kill_amount(&t, &hero(0), &env), 0, "a level below 1 gives nothing");
        let overridden = KillVictim {
            ability: Some((true, 1)),
            ..hero(2)
        };
        assert_eq!(kill_amount(&t, &overridden, &env), 10, "the ability level override");
        let not_answering = KillVictim {
            ability: Some((false, 1)),
            ..hero(2)
        };
        assert_eq!(kill_amount(&t, &not_answering, &env), 20, "an ability that does not answer is ignored");
        let summoned = KillVictim {
            summoned: true,
            ..hero(2)
        };
        assert_eq!(kill_amount(&t, &summoned, &env), 10, "the summoned factor 0.5 on 20");
        let unit = KillVictim {
            type_upper: false,
            level: 9,
            ..hero(2)
        };
        assert_eq!(kill_amount(&t, &unit, &env), 6, "units use GrantNormalXP clamped to MaxUnitLevel");
        let illusion = KillVictim {
            illusion: true,
            level: 1,
            ..hero(2)
        };
        assert_eq!(kill_amount(&t, &illusion, &env), 5, "an illusion uses the unit table");
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

    #[test]
    fn intelligence_is_base_plus_gain_times_counter_and_rebases() {
        let gain = F32::from_bits(1.5f32.to_bits());
        assert_eq!(intelligence(12, gain, 2), 15);
        assert_eq!(intelligence(-3, F32::from_bits(1000.0f32.to_bits()), 17), 16997);
        // SetHeroInt(40) at counter 2: the base moves by 40 - 15 = 25.
        let base = rebase(12, gain, 2, 40);
        assert_eq!(base, 37);
        assert_eq!(intelligence(base, gain, 2), 40);
    }

    #[test]
    fn candidate_gate_follows_the_routine() {
        // Drain on lets a maximum-level hero through; drain off rejects it.
        assert!(is_candidate(true, true, 0, true));
        assert!(!is_candidate(false, true, 0, true));
        assert!(is_candidate(false, false, 0, true));
        // The flags 0x40000100 exclude, any other bit does not (0x10 is the parent substitution, not an exclusion).
        assert!(!is_candidate(true, false, 0x0000_0100, true));
        assert!(!is_candidate(true, false, 0x4000_0000, true));
        assert!(is_candidate(true, false, 0x0000_0010 | 0x0000_0020, true));
        // The shared-experience bit is required.
        assert!(!is_candidate(true, false, 0, false));
    }

    #[test]
    fn in_range_rejects_only_strictly_less_v() {
        let (one, two) = (f(1.0), f(2.0));
        // d <= v is listed, including the tie.
        assert!(in_range(one, two));
        assert!(in_range(two, two));
        // v < d is rejected.
        assert!(!in_range(two, one));
        // NaN on either side is listed (unordered compare).
        let nan = F32::from_bits(0x7FC0_0000);
        assert!(in_range(nan, one));
        assert!(in_range(one, nan));
    }
}
