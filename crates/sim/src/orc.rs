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

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

use crate::abilities::code;
use crate::cast::{effect_duration, BuffEffect, CastRec, CastTarget, BUFF_STUN};
use crate::natives::game::{Game, ObjKey};
use crate::natives::unit_attr::is_unit_type;
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::unitclass::ty;

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

/// The other seven abilities' base codes.
pub const WIND_WALK: u32 = code(b"AOwk");
pub const MIRROR_IMAGE: u32 = code(b"AOmi");
pub const SHOCKWAVE: u32 = code(b"AOsh");
pub const WAR_STOMP: u32 = code(b"AOws");
pub const CHAIN_LIGHTNING: u32 = code(b"AOcl");
pub const FAR_SIGHT: u32 = code(b"AOfs");
pub const FERAL_SPIRIT: u32 = code(b"AOsf");
/// Buff codes.
pub const BUFF_WIND_WALK: u32 = code(b"BOwk");
pub const BUFF_FAR_SIGHT: u32 = code(b"BOfs");

/// Damage type of the spell hits (the class word `0x8000` of the stomp record, slot `0x42C` = `0x6F18F880`; the same
/// bit index as Storm Bolt in `cast.rs`).
const SPELL_DAMAGE_TYPE: u32 = 15;

/// Order ids (UNSURE: the public order-id list, like the three spells of `cast.rs`).
pub fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    ids.insert(852_127, WAR_STOMP);
    ids.insert(852_125, SHOCKWAVE);
    ids.insert(852_119, CHAIN_LIGHTNING);
    ids.insert(852_129, WIND_WALK);
    ids.insert(852_123, MIRROR_IMAGE); // DLL slot 0x308 of CAbilityMirrorImage (852093 is Mass Teleport)
    ids.insert(852_122, FAR_SIGHT);
    ids.insert(852_120, FERAL_SPIRIT);
}

/// Abilities cast without a target (`targs` empty or area around the caster).
pub fn takes_no_target(base: u32) -> bool {
    matches!(base, WAR_STOMP | WIND_WALK | MIRROR_IMAGE | FERAL_SPIRIT)
}

/// Abilities cast on a point.
pub fn takes_point(base: u32) -> bool {
    matches!(base, SHOCKWAVE | FAR_SIGHT)
}

/// Per-game state of the orc abilities.
#[derive(Debug, Default, Clone)]
pub struct OrcState {
    /// Critical Strike hit counters (`+0x84`) per unit; absent = 1.
    crit_count: BTreeMap<ObjKey, i32>,
    /// Units whose Critical Strike is armed for the coming execute (flag `0x200`).
    crit_armed: BTreeSet<ObjKey>,
    /// The summoned unit type of each level of Feral Spirit (`UnitID`), index = level - 1.
    summon_units: BTreeMap<u32, Vec<u32>>,
    /// Mirror Image copies: (damage dealt factor, damage taken factor).
    illusions: BTreeMap<ObjKey, (F32, F32)>,
    /// Far Sight reveals: marker buff serial -> fog modifier.
    reveals: BTreeMap<u32, ObjKey>,
}

impl OrcState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Game {
    /// Installs the summoned unit types of an ability (`UnitID` per level).
    pub fn set_summon_units(&mut self, base: u32, units: Vec<u32>) {
        self.orc.summon_units.insert(base, units);
    }

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

    /// The execute: applies the orc modifiers of an attacker to the raw damage (before armor): an armed Critical
    /// Strike (CONFIRMED order and arithmetic), then the Wind Walk first-hit bonus and the Mirror Image damage factor
    /// (their order relative to the crit is UNSURE).
    pub fn orc_crit_raw(&mut self, attacker: ObjKey, raw: F32) -> F32 {
        let mut raw = raw;
        if self.orc.crit_armed.contains(&attacker) {
            if let Some(record) = self.ability_level_record(attacker, CRITICAL_STRIKE) {
                raw = crit_damage(raw, record.data[1]);
            }
        }
        // Wind Walk: the first strike ends the buff and adds DataC (flat; UNSURE whether DataC is a flat bonus).
        let ww = self.unit(attacker).buffs.iter().find(|b| b.base == BUFF_WIND_WALK).map(|b| b.serial);
        if let Some(serial) = ww {
            if let Some(record) = self.ability_level_record(attacker, WIND_WALK) {
                raw = arith::add(raw, record.data[2]);
            }
            self.drop_spell_buff(serial);
        }
        if let Some(&(dealt, _)) = self.orc.illusions.get(&attacker) {
            raw = arith::mul(raw, dealt);
        }
        raw
    }

    /// The Mirror Image damage taken factor of an illusion (DataC); 1.0 for any other unit. Not applied to damage yet
    /// (UNSURE where the original applies it).
    pub fn illusion_taken_factor(&self, unit: ObjKey) -> F32 {
        self.orc.illusions.get(&unit).map_or(F32::ONE, |&(_, taken)| taken)
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


// ---- the class effects ----

fn spell_target_ok(gm: &Game, owner: u32, k: ObjKey) -> bool {
    let u = gm.unit(k);
    !u.dead && !u.removed && !u.hidden && gm.is_enemy(owner, u.owner) && !is_unit_type(gm, u, ty::MAGIC_IMMUNE)
}

/// Dispatches the effect of an orc hero ability. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        WAR_STOMP => war_stomp(game, rt, rec),
        CHAIN_LIGHTNING => chain_lightning(game, rt, rec),
        SHOCKWAVE => shockwave(game, rt, rec),
        WIND_WALK => wind_walk(game, rec),
        MIRROR_IMAGE => mirror_image(game, rec),
        FERAL_SPIRIT => feral_spirit(game, rec),
        FAR_SIGHT => far_sight(game, rec),
        _ => return false,
    }
    true
}

/// War Stomp (`0x6F1608B0` instant slot, per-unit effect `0x6F1607B0`, CONFIRMED structure): every unit of the area
/// around the caster that the group builder lets through takes `DataA` damage (class `0x8000`), and a unit that is
/// still alive is stunned for `HeroDur` (hero) or `Dur` (slot `0x430` = `0x6F18ABF0`, same duration rule as Storm
/// Bolt). A unit the immunity test `0x6F28D750` rejects gets a zero-damage record and no stun (modelled as untouched).
/// The units come in the area query's order (x-major cells). UNSURE: the builder's relation filter (enemies of the
/// caster's owner here), the ground/organic target flags (`ground,organic`: no air, structures, mechanical).
fn war_stomp(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let (cx, cy, owner) = {
            let u = gm.unit(rec.caster);
            (u.x, u.y, u.owner)
        };
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
            if k == rec.caster || !spell_target_ok(&gm, owner, k) {
                continue;
            }
            let u = gm.unit(k);
            if !is_unit_type(&gm, u, ty::GROUND)
                || is_unit_type(&gm, u, ty::STRUCTURE)
                || is_unit_type(&gm, u, ty::MECHANICAL)
            {
                continue;
            }
            if let Some(t) = gm.unit_damage_target(rec.caster, k, rec.level.data[0], false, false, 0, SPELL_DAMAGE_TYPE) {
                killed.push(t);
                continue;
            }
            if gm.alive(k) {
                let hero = gm.target_hero(k);
                let dur = effect_duration(&rec.level, hero, F32::ZERO);
                gm.add_spell_buff(k, BUFF_STUN, BuffEffect::Stun, dur);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// Chain Lightning (class `0x6F8B0E74`; hop body `0x6F1C2F50`, next-target search `0x6F1B6F30`, CONFIRMED structure):
/// the hop damage is `DataA * m` with `m` starting at 1.0, and after each hop `m := m * (1 - DataC)` (soft mul/sub, the
/// factor cell at `+0x12C`). `DataB` is the number of units hit. The next unit is the NEAREST valid unit to the unit
/// just hit, inside `Area` (the best-of collector `0x6F1B0780` keeps a candidate only when its distance is strictly
/// smaller, so the first of equals wins), never a unit already hit (list at `+0x134`). UNSURE: the delay between hops
/// (none here), the relation filter (enemies, not magic immune, not structure/mechanical per `air,ground,enemy,
/// neutral,organic`), the damage class.
fn chain_lightning(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let CastTarget::Unit(first) = rec.target else {
        return;
    };
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let owner = gm.unit(rec.caster).owner;
        let hops = conv::f2i(rec.level.data[1]).max(0);
        let keep = arith::sub(F32::ONE, rec.level.data[2]);
        let mut mult = F32::ONE;
        let mut hit: Vec<ObjKey> = Vec::new();
        let mut current = first;
        for _ in 0..hops {
            if !gm.alive(current) {
                break;
            }
            let amount = arith::mul(rec.level.data[0], mult);
            if let Some(t) = gm.unit_damage_target(rec.caster, current, amount, false, false, 0, SPELL_DAMAGE_TYPE) {
                killed.push(t);
            }
            hit.push(current);
            mult = arith::mul(mult, keep);
            let (cx, cy) = {
                let u = gm.unit(current);
                (u.x, u.y)
            };
            let mut best: Option<(ObjKey, F32)> = None;
            for k in crate::natives::group::circle_candidates(&gm, cx, cy, rec.level.area) {
                if hit.contains(&k) || k == rec.caster || !spell_target_ok(&gm, owner, k) {
                    continue;
                }
                let u = gm.unit(k);
                if is_unit_type(&gm, u, ty::STRUCTURE) || is_unit_type(&gm, u, ty::MECHANICAL) {
                    continue;
                }
                let d = arith::dist2(cx, cy, u.x, u.y);
                if best.is_none_or(|(_, bd)| arith::lt(d, bd)) {
                    best = Some((k, d));
                }
            }
            match best {
                Some((k, _)) => current = k,
                None => break,
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// Shockwave (class `0x6F92075C`): a line missile from the caster towards the point. Data: `DataA` damage, `DataB`
/// damage cap, `DataC` travel distance, `Area`/`DataD` start/end width. The missile flight is the engine projectile
/// system (not decoded), so this is a MODEL (UNSURE): at the effect time every ground unit or structure of an enemy
/// whose distance to the segment is within the width is hit once, in the order of travel (projection along the line,
/// ties in the area query's order), for `min(DataA, DataB)`.
fn shockwave(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let CastTarget::Point(px, py) = rec.target else {
        return;
    };
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let (cx, cy, owner) = {
            let u = gm.unit(rec.caster);
            (u.x, u.y, u.owner)
        };
        let dx = arith::sub(px, cx);
        let dy = arith::sub(py, cy);
        let dd = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
        if !arith::lt(F32::ZERO, dd) {
            return;
        }
        let len = rec.level.data[2];
        let width = if arith::lt(rec.level.area, rec.level.data[3]) { rec.level.data[3] } else { rec.level.area };
        let reach = arith::add(len, width);
        let l2dd = arith::mul(arith::mul(len, len), dd);
        let w2dd = arith::mul(arith::mul(width, width), dd);
        let amount = if arith::lt(rec.level.data[1], rec.level.data[0]) { rec.level.data[1] } else { rec.level.data[0] };
        let mut ordered: Vec<(F32, ObjKey)> = Vec::new();
        for k in crate::natives::group::circle_candidates(&gm, cx, cy, reach) {
            if k == rec.caster || !spell_target_ok(&gm, owner, k) {
                continue;
            }
            let u = gm.unit(k);
            if is_unit_type(&gm, u, ty::FLYING) {
                continue;
            }
            let vx = arith::sub(u.x, cx);
            let vy = arith::sub(u.y, cy);
            let dot = arith::add(arith::mul(vx, dx), arith::mul(vy, dy));
            let cross = arith::sub(arith::mul(vx, dy), arith::mul(vy, dx));
            if arith::lt(dot, F32::ZERO) || arith::lt(l2dd, arith::mul(dot, dot)) {
                continue;
            }
            if arith::lt(w2dd, arith::mul(cross, cross)) {
                continue;
            }
            // Stable insertion by the projection (equal keys keep the query order).
            let at = ordered.iter().position(|o| arith::lt(dot, o.0)).unwrap_or(ordered.len());
            ordered.insert(at, (dot, k));
        }
        for (_, k) in ordered {
            if let Some(t) = gm.unit_damage_target(rec.caster, k, amount, false, false, 0, SPELL_DAMAGE_TYPE) {
                killed.push(t);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(rec.caster));
    }
}

/// Wind Walk (class `0x6F8F2534`, effect `0x6F18CAA0`): the duration is `HeroDur` for a hero caster, else `Dur`; the
/// buff carries `DataB` (movement speed bonus, modelled as a speed factor `1 + DataB`) and `DataC` (the bonus of the
/// first strike, see [`Game::orc_crit_raw`]); `DataA` is the fade time (presentation). Invisibility is not modelled.
fn wind_walk(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let effect = BuffEffect::Slow { move_delta: rec.level.data[1], attack_delta: F32::ZERO };
    gm.add_spell_buff(rec.caster, BUFF_WIND_WALK, effect, dur);
}

fn rad_to_deg(r: F32) -> F32 {
    arith::mul(r, F32::from_bits(0x4265_2EE1))
}

/// Mirror Image (class `0x6F8D9E5C`, effect `0x6F135740`): `DataA` copies of the caster, with `DataB` as the damage
/// they deal and `DataC` as the damage they take (factors), living `Dur`/`HeroDur`. No RNG draw is reachable from the
/// effect through direct calls (checked to depth 5). UNSURE: the placement (all copies at the caster's position here;
/// the engine's push-apart spreads them), the copies' life and abilities, where the taken factor applies.
fn mirror_image(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let count = conv::f2i(rec.level.data[0]).max(0);
    let (owner, type_id, x, y, facing) = {
        let u = gm.unit(rec.caster);
        (u.owner, u.type_id, u.x, u.y, u.facing)
    };
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    for _ in 0..count {
        let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, type_id, x, y, rad_to_deg(facing), true) else {
            continue;
        };
        gm.unit_mut(k).illusion = true;
        gm.orc.illusions.insert(k, (rec.level.data[1], rec.level.data[2]));
        gm.apply_timed_life(k, crate::abilities::TIMED_LIFE, dur);
    }
}

/// Feral Spirit (class `0x6F927B2C`, effect `0x6F1DF330`): `DataB` units of the level's `UnitID`, summoned, living
/// `Dur`/`HeroDur`. No RNG draw is reachable through direct calls. UNSURE: the placement (at the caster here).
fn feral_spirit(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let level_index =
        usize::try_from(gm.unit(rec.caster).abilities.get(FERAL_SPIRIT).map_or(0, |a| a.level_index)).unwrap_or(0);
    let Some(&unit_type) = gm.orc.summon_units.get(&FERAL_SPIRIT).and_then(|v| v.get(level_index)) else {
        return;
    };
    let count = conv::f2i(rec.level.data[1]).max(0);
    let (owner, x, y, facing) = {
        let u = gm.unit(rec.caster);
        (u.owner, u.x, u.y, u.facing)
    };
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    for _ in 0..count {
        let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, unit_type, x, y, rad_to_deg(facing), true) else {
            continue;
        };
        {
            let u = gm.unit_mut(k);
            u.summoned = true;
            u.class_flags |= 0x4;
        }
        gm.apply_timed_life(k, crate::abilities::TIMED_LIFE, dur);
    }
}

/// Far Sight (class `0x6F88B25C`): reveals the circle of `Area` around the point to the caster's owner for `Dur`.
/// MODEL (UNSURE): a visible-state fog modifier started now and stopped when the marker buff on the caster ends.
fn far_sight(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Point(px, py) = rec.target else {
        return;
    };
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let owner = gm.unit(rec.caster).owner;
    let cells = gm.fog_grid().circle_shape(px, py, rec.level.area);
    let handle = gm.fog_create(owner, crate::natives::fog::FOG_VISIBLE, cells, false, false);
    let Some(key) = gm.fog_modifier_of_handle(handle) else {
        return;
    };
    gm.fog_start(key);
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    let serial = gm.add_spell_buff(rec.caster, BUFF_FAR_SIGHT, BuffEffect::Marker, dur);
    if serial == 0 {
        gm.fog_stop(key);
    } else {
        gm.orc.reveals.insert(serial, key);
    }
}

/// Called by `Game::drop_spell_buff` when a spell buff ends: stops the Far Sight reveal tied to it.
pub(crate) fn on_buff_dropped(game: &mut Game, serial: u32) {
    if let Some(key) = game.orc.reveals.remove(&serial) {
        game.fog_stop(key);
    }
}
