//! Hero revive cost, time and restore fractions of Game.dll 1.26a (`docs/spec/revive.md`).
//!
//! * Cost `0x6F0340A0`: factor `t = ReviveBaseFactor + I2R(level - 1) * ReviveLevelFactor`, capped by `ReviveMaxFactor`
//!   (x87 compare: the cap wins when `max <= t`, NaN too); gold = `F2I(I2R(F2I((I2R(typeGold) * t + 0.5) / 5.0)) * 5.0)`
//!   limited to `HeroMaxReviveCostGold` (unsigned minimum); lumber the same with the lumber keys.
//! * Time `0x6F030CF0`: `t = I2R(level) * ReviveTimeFactor` capped by `ReviveMaxTimeFactor`, then
//!   `I2R(F2I(I2R(buildTime) * t / 5.0)) * 5.0`, raised to at least 1.0, then limited by `HeroMaxReviveTime`.
//! * Restore (`0x6F059E20`, `0x6F061E70`): life = `HeroReviveLifeFactor` of the maximum, mana = the type's starting mana when
//!   `HeroReviveManaStart` is non-zero, else `HeroReviveManaFactor` of the maximum.
//!
//! All arithmetic is softfloat. The cost and time are checked against the original in Unicorn (400 cases,
//! `tools/emu/revive_cost_emu.py`, `tests/revive_vectors.rs`).

use softfloat::{arith, conv, Tables, F32};

/// The `[Misc]` revive keys (absent keys are 0 / 0.0).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReviveConfig {
    pub base_factor: F32,
    pub level_factor: F32,
    pub max_factor: F32,
    pub base_lumber_factor: F32,
    pub lumber_level_factor: F32,
    pub time_factor: F32,
    pub max_time_factor: F32,
    pub max_time: F32,
    pub max_cost_gold: u32,
    pub max_cost_lumber: u32,
    pub mana_start: bool,
    pub mana_factor: F32,
    pub life_factor: F32,
}

const HALF: F32 = F32::from_bits(0x3F00_0000);
const FIVE: F32 = F32::from_bits(0x40A0_0000);
const ONE: F32 = F32::from_bits(0x3F80_0000);

fn fl(x: F32) -> f32 {
    f32::from_bits(x.to_bits())
}

/// `max <= t ? max : t` (the x87 compare of the cap; unordered picks the cap).
fn cap(t: F32, max: F32) -> F32 {
    if fl(max) <= fl(t) || fl(max).is_nan() || fl(t).is_nan() {
        max
    } else {
        t
    }
}

fn resource(cfg_base: F32, cfg_level: F32, cfg_max: F32, level: i32, type_cost: i32, limit: u32, tb: &Tables) -> u32 {
    let t = arith::add(cfg_base, arith::mul(conv::i2r(level.wrapping_sub(1)), cfg_level));
    let f = cap(t, cfg_max);
    let x = arith::add(arith::mul(conv::i2r(type_cost), f), HALF);
    let n = conv::f2i(arith::div(x, FIVE, tb));
    let v = conv::f2i(arith::mul(conv::i2r(n), FIVE)) as u32;
    v.min(limit)
}

/// The revive cost `(gold, lumber)` (raw units, not x10) of a hero of `level` whose type costs `type_gold` / `type_lumber`.
pub fn cost(cfg: &ReviveConfig, level: i32, type_gold: i32, type_lumber: i32, tb: &Tables) -> (u32, u32) {
    let g = resource(cfg.base_factor, cfg.level_factor, cfg.max_factor, level, type_gold, cfg.max_cost_gold, tb);
    let l = resource(
        cfg.base_lumber_factor,
        cfg.lumber_level_factor,
        cfg.max_factor,
        level,
        type_lumber,
        cfg.max_cost_lumber,
        tb,
    );
    (g, l)
}

/// The revive time in seconds for a hero of `level` whose type has build time `build_time`.
pub fn time(cfg: &ReviveConfig, level: i32, build_time: i32, tb: &Tables) -> F32 {
    let t = arith::mul(conv::i2r(level), cfg.time_factor);
    let f = cap(t, cfg.max_time_factor);
    let x = arith::mul(conv::i2r(build_time), f);
    let n = conv::f2i(arith::div(x, FIVE, tb));
    let secs = arith::mul(conv::i2r(n), FIVE);
    let secs = if fl(ONE) > fl(secs) { ONE } else { secs };
    if fl(secs) <= fl(cfg.max_time) || fl(secs).is_nan() || fl(cfg.max_time).is_nan() {
        secs
    } else {
        cfg.max_time
    }
}
