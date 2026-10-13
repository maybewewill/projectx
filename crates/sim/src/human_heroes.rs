//! Human hero abilities: Thunder Clap (`AHtc`), Bash (`AHbh`), Avatar (`AHav`), Divine Shield (`AHds`),
//! Resurrection (`AHre`) and Summon Water Elemental (`AHwe`), on the spell cast core of [`crate::cast`].
//! Spec: `docs/spec/abilities-human-heroes.md` (one section per ability with CONFIRMED / LIKELY / UNSURE labels).
//!
//! Classes (RTTI, vtables of the user's `Game.dll`): `CAbilityThunderClap` `0x6F8F67C4`, `CAbilityBash`
//! `0x6F90A414`, `CAbilityAvatar` `0x6F90998C`, `CAbilityDivineShield` `0x6F894AEC`, `CAbilityResurrection`
//! `0x6F91C1CC`, `CAbilityWaterElemental` `0x6F89BF84`; buffs `CBuffThunderClap` `0x6F8E79B4`, `CBuffPause` (Bash's
//! stun) `0x6F8B3BEC`, `CBuffAvatar` `0x6F9039A4`, `CBuffDivineShield` `0x6F894794`, `CBuffWaterElemental`
//! `0x6F8EBDBC` (the timed life buff `BHwe`).
//!
//! Summary of what the model does (details and labels in the spec):
//!
//! * **Thunder Clap**: an immediate cast. Every enemy ground non-structure unit in `Area` around the caster takes
//!   `DataA` damage (damage type 15, class `0x8000`) in cell-walk order, and a survivor gets the `BHtc` slow of
//!   `-DataC` movement and `-DataD` attack speed for `HeroDur` (hero) or `Dur` (CONFIRMED structure: `0x6F1608B0`,
//!   `0x6F1607B0`, `0x6F18A930`).
//! * **Bash**: a passive with a pseudo-random distribution (CONFIRMED: `0x6F1A3470`, table `0x6F1D21F0`, stream 3).
//!   Armed per swing; a damage multiplier `DataB`, the stun `BPSE` (`Dur`/`HeroDur`) and the flat `DataC` damage
//!   (type 17, class `0x20000`) on hit.
//! * **Avatar**: an immediate cast; a `BHav` buff of `Dur` with armor `DataA`, maximum life `DataB` (life sink mode 3),
//!   damage `int(DataC)` and one magic immunity count.
//! * **Divine Shield**: an immediate cast; a `BHds` buff of `Dur`/`HeroDur` (by the caster) with one invulnerability
//!   count and one magic immunity count; `undivineshield` removes it when `DataA` allows.
//! * **Resurrection**: an immediate cast; up to `int(DataA)` allied corpses in `Area` come back with full life.
//! * **Water Elemental**: an immediate cast; `int(DataA)` units of the level's `UnitID` appear with a `BHwe` timed life
//!   of `Dur`.

use std::cell::RefCell;

use softfloat::{arith, conv, Tables, F32};

use crate::abilities::{buff_predicates, code, Buff, BuffPredicates};
use crate::auras::BuffEffect;
use crate::cast::{begin_cast, effect_duration, CastRec, CastTarget, SpellLevel};
use crate::heroes::life_sink;
use crate::missiles::MOVE_FLY;
use crate::natives::game::{Game, ObjKey, Object, ScriptRuntime};
use crate::natives::{circle_candidates, dispatch_unit_death_by};
use crate::rng::{Rng, RngTable};

/// Base codes.
pub const THUNDER_CLAP: u32 = code(b"AHtc");
pub const BASH: u32 = code(b"AHbh");
pub const AVATAR: u32 = code(b"AHav");
pub const DIVINE_SHIELD: u32 = code(b"AHds");
pub const RESURRECTION: u32 = code(b"AHre");
pub const WATER_ELEMENTAL: u32 = code(b"AHwe");
pub const BLIZZARD: u32 = code(b"AHbz");
pub const MASS_TELEPORT: u32 = code(b"AHmt");

/// Buff codes (CONFIRMED by the code getters of the buff classes, slot 7).
pub const BUFF_THUNDER_CLAP: u32 = code(b"BHtc");
/// Bash's stun is a `CBuffPause` (`BPSE`, the code `0x6F0D2280` returns), not a `BSTN` stun.
pub const BUFF_PAUSE: u32 = code(b"BPSE");
pub const BUFF_AVATAR: u32 = code(b"BHav");
pub const BUFF_DIVINE_SHIELD: u32 = code(b"BHds");
/// The timed life buff of the water elemental (one of the seven special timed life ids).
pub const BUFF_WATER_ELEMENTAL: u32 = code(b"BHwe");
/// Wind Walk's buff: Bash does not roll while its owner has it (`0x6F15A110` returns `BOwk`).
pub const BUFF_WIND_WALK: u32 = code(b"BOwk");

/// Cast order ids from the original slot 0x348 (Unicorn-checked by tools/analysis/ability_orders.py).
pub const ORDER_THUNDER_CLAP: u32 = 852_096;
pub const ORDER_AVATAR: u32 = 852_086;
pub const ORDER_DIVINE_SHIELD: u32 = 852_090;
/// UNSURE: the state-dependent cancel getter has not been verified.
pub const ORDER_UNDIVINE_SHIELD: u32 = 852_056;
pub const ORDER_RESURRECTION: u32 = 852_094;
pub const ORDER_WATER_ELEMENTAL: u32 = 852_097;

pub const ORDER_BLIZZARD: u32 = 852_089;
pub const ORDER_MASS_TELEPORT: u32 = 852_093;

/// The `(order id, ability)` pairs the cast core registers.
pub const SPELL_ORDERS: [(u32, u32); 7] = [
    (ORDER_BLIZZARD, BLIZZARD),
    (ORDER_MASS_TELEPORT, MASS_TELEPORT),
    (ORDER_THUNDER_CLAP, THUNDER_CLAP),
    (ORDER_AVATAR, AVATAR),
    (ORDER_DIVINE_SHIELD, DIVINE_SHIELD),
    (ORDER_RESURRECTION, RESURRECTION),
    (ORDER_WATER_ELEMENTAL, WATER_ELEMENTAL),
];

/// Damage type numbers (the JASS constants): Thunder Clap class `0x8000` is `DAMAGE_TYPE_SONIC` (15), Bash's flat
/// damage class `0x20000` is `DAMAGE_TYPE_FORCE` (17).
const THUNDER_CLAP_DAMAGE_TYPE: u32 = 15;
const BASH_DAMAGE_TYPE: u32 = 17;

/// Combat RNG stream of Bash's roll (CONFIRMED: `0x6F1B2EC0` calls `0x6F2853D0` with EDX = 3).
pub const BASH_STREAM: usize = 3;

/// Whether `base` is cast without a target.
pub fn is_immediate_spell(base: u32) -> bool {
    matches!(
        base,
        THUNDER_CLAP | AVATAR | DIVINE_SHIELD | RESURRECTION | WATER_ELEMENTAL
    )
}

// ---------------------------------------------------------------------------------------------------------------
// The pseudo-random distribution of Bash (CONFIRMED, Unicorn: `tools/emu/human_heroes_emu.py`).

/// The numerators of the table the start-up initialiser of `0x6F1D21F0` builds (over 10000), one per 5 percent step.
pub const PRD_NUMERATORS: [i32; 21] = [
    0, 38, 147, 320, 547, 819, 1126, 1461, 1815, 2184, 2562, 2953, 3333, 3781, 4203, 4607, 5000, 5799, 6666, 7706,
    10000,
];

/// The table index of a chance in percent: `f2i(chance * 0.2 + 0.5)` clamped to `0..=20` (`0x6F1D21F0`).
pub fn prd_index(chance: F32) -> usize {
    let scaled = arith::mul(chance, F32::from_bits(0x3E4C_CCCD));
    let rounded = arith::add(scaled, F32::from_bits(0x3F00_0000));
    conv::f2i(rounded).clamp(0, 20) as usize
}

/// The distribution constant of a chance: `DIV(I2R(numerator), I2R(10000))` of the indexed entry.
pub fn prd_constant(chance: F32, tables: &Tables) -> F32 {
    let n = PRD_NUMERATORS[prd_index(chance)];
    arith::div(conv::i2r(n), conv::i2r(10000), tables)
}

/// The effective chance of the current swing (`0x6F1A3470`): the constant times the number of swings since the last
/// proc (the counter starts at 1).
pub fn prd_effective_chance(chance: F32, count: i32, tables: &Tables) -> F32 {
    arith::mul(conv::i2r(count), prd_constant(chance, tables))
}

/// The proc test of `0x6F1B2EC0`: the draw `r` (a real in `[0, 1)`) procs when `r <= effective` (`0x6F01FD60`; NaN
/// never procs).
pub fn bash_procs(draw: F32, effective: F32) -> bool {
    arith::le(draw, effective)
}

/// Per-owner Bash state: the armed flag (ability flag `0x200`), the swing counter (`+0x84`, starts at 1) and the
/// stored multiplied damage (`+0x80`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BashState {
    pub armed: bool,
    pub count: i32,
    pub stored: F32,
}

impl Default for BashState {
    fn default() -> Self {
        BashState {
            armed: false,
            count: 1,
            stored: F32::ZERO,
        }
    }
}

fn nonzero(x: F32) -> bool {
    // The original tests `x == 0.0` with an unordered-aware compare: NaN counts as non-zero.
    !arith::eq(x, F32::ZERO) || x.to_f32().is_nan()
}

impl Game {
    /// The Bash state of `unit` (defaults when it never rolled).
    pub fn bash_state(&self, unit: ObjKey) -> BashState {
        self.spells.bash.get(&unit).copied().unwrap_or_default()
    }

    fn has_ability(&self, unit: ObjKey, base: u32) -> bool {
        self.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed())
    }

    /// Bash, slot 73 (`0x6F1B2EC0`): the swing starts against `target`. Clears the armed flag, then rolls.
    pub(crate) fn bash_on_attack_start(&mut self, attacker: ObjKey, target: ObjKey) {
        if !self.has_ability(attacker, BASH) {
            return;
        }
        self.spells.bash.entry(attacker).or_default().armed = false;
        if !self.alive(target) {
            return;
        }
        // Wind Walk: the owner's `BOwk` buff stops the roll.
        if self.buff_position(attacker, BUFF_WIND_WALK).is_some() {
            return;
        }
        let (a_owner, t_owner) = (self.unit(attacker).owner, self.unit(target).owner);
        // The target filter of the ability (`0x6F02E000` -> `0x6F26D590`): an enemy (UNSURE: the stock `targs`).
        if !self.is_enemy(a_owner, t_owner) {
            return;
        }
        let Some(level) = self.ability_level_record(attacker, BASH) else {
            return;
        };
        let count = self.spells.bash.entry(attacker).or_default().count;
        let Some(effective) = self
            .order_tables()
            .map(|t| prd_effective_chance(level.data[0], count, t))
        else {
            return;
        };
        let Some((table, streams)) = self.rng_parts() else {
            return;
        };
        let u = streams.stream_mut(BASH_STREAM).random_fraction(table);
        // `0x6F2853D0`: the real is `add(0.0, mul(1.0, U))`, i.e. U itself.
        let draw = arith::add(F32::ZERO, arith::mul(F32::ONE, u));
        let st = self.spells.bash.entry(attacker).or_default();
        if bash_procs(draw, effective) {
            st.armed = true;
            st.count = 1;
        } else {
            st.count += 1;
        }
    }

    /// Bash, slot 77 (`0x6F1A3590`): the damage roll of an armed swing. Returns the damage with the multiplier's
    /// extra added (`DataB`: stored `damage * DataB`, the record gains `stored - damage`).
    pub(crate) fn bash_modify_damage(&mut self, attacker: ObjKey, damage: F32) -> F32 {
        if !self.has_ability(attacker, BASH) {
            return damage;
        }
        let st = self.spells.bash.entry(attacker).or_default();
        st.stored = F32::ZERO;
        if !st.armed {
            return damage;
        }
        let Some(level) = self.ability_level_record(attacker, BASH) else {
            return damage;
        };
        let b = level.data[1];
        if !nonzero(b) {
            return damage;
        }
        let stored = arith::mul(damage, b);
        self.spells.bash.entry(attacker).or_default().stored = stored;
        let extra = arith::sub(stored, damage);
        arith::add(damage, extra)
    }

    /// Bash, slot 85 (`0x6F1CC530`): the hit lands on `target`. Applies the pause (`Dur`/`HeroDur`), then the flat
    /// `DataC` damage unless the owner is an illusion. Returns the unit this killed.
    pub(crate) fn bash_on_hit(&mut self, attacker: ObjKey, target: ObjKey) -> Option<ObjKey> {
        if !self.has_ability(attacker, BASH) {
            return None;
        }
        let st = self.spells.bash.entry(attacker).or_default();
        if !st.armed {
            return None;
        }
        st.stored = F32::ZERO;
        let level = self.ability_level_record(attacker, BASH)?;
        if !self.alive(target) {
            return None;
        }
        let hero = self.target_hero(target);
        let dur = effect_duration(&level, hero, F32::ZERO);
        if nonzero(dur) {
            self.apply_stun_as(BUFF_PAUSE, target, dur);
        }
        let flat = level.data[2];
        if nonzero(flat) && !self.unit(attacker).illusion {
            return self.unit_damage_target(attacker, target, flat, false, false, 0, BASH_DAMAGE_TYPE);
        }
        None
    }

    /// Bash, slot 74 (`0x6F1B3060`): the swing ended without a hit; the armed flag goes.
    pub(crate) fn bash_clear(&mut self, attacker: ObjKey) {
        if let Some(st) = self.spells.bash.get_mut(&attacker) {
            st.armed = false;
        }
    }

    // ---- Avatar -------------------------------------------------------------------------------------------------

    /// `CBuffAvatar::apply` (`0x6F1A03B0`): the three stat sinks and one magic immunity count.
    pub(crate) fn apply_avatar(&mut self, unit: ObjKey, duration: F32, armor: F32, life: F32, damage: i32) -> u32 {
        let serial = self.next_buff_serial();
        self.unit_mut(unit).buffs.push(Buff {
            serial,
            base: BUFF_AVATAR,
            predicates: BuffPredicates {
                positive: true,
                ..buff_predicates(BUFF_AVATAR)
            },
            timed_life: None,
            effect: Some(BuffEffect::Avatar { armor, life, damage }),
            expiry: None,
        });
        self.set_buff_duration(unit, serial, duration);
        self.add_armor(unit, armor);
        self.avatar_life(unit, life);
        self.add_damage(unit, damage);
        self.unit_mut(unit).magic_immune += 1;
        serial
    }

    /// The life sink `0x6F2AC450` in mode 3 with the buff's `DataB`.
    fn avatar_life(&mut self, unit: ObjKey, value: F32) {
        let flagged = self.unit(unit).dead;
        let mut cells = self.life_cells(unit);
        life_sink(&mut cells, flagged, value, 3, self.order_tables());
        self.put_life_cells(unit, cells);
    }

    /// `CBuffAvatar::remove` (`0x6F1A0480`): the negated sinks and one magic immunity count off.
    pub(crate) fn undo_avatar(&mut self, unit: ObjKey, armor: F32, life: F32, damage: i32) {
        self.add_armor(unit, arith::neg(armor));
        self.avatar_life(unit, arith::neg(life));
        self.add_damage(unit, damage.saturating_neg());
        self.unit_mut(unit).magic_immune -= 1;
    }

    // ---- Divine Shield ------------------------------------------------------------------------------------------

    /// Applies or refreshes the `BHds` buff of `unit` (`0x6F0B64C0`: a buff found by its code gets the set-duration
    /// rule, otherwise a new one is created and applied).
    pub(crate) fn apply_divine_shield(&mut self, unit: ObjKey, duration: F32) -> u32 {
        if let Some(i) = self.buff_position(unit, BUFF_DIVINE_SHIELD) {
            let serial = self.unit(unit).buffs[i].serial;
            self.set_buff_duration(unit, serial, duration);
            return serial;
        }
        let serial = self.next_buff_serial();
        self.unit_mut(unit).buffs.push(Buff {
            serial,
            base: BUFF_DIVINE_SHIELD,
            predicates: BuffPredicates {
                positive: true,
                ..buff_predicates(BUFF_DIVINE_SHIELD)
            },
            timed_life: None,
            effect: Some(BuffEffect::DivineShield),
            expiry: None,
        });
        self.set_buff_duration(unit, serial, duration);
        let u = self.unit_mut(unit);
        u.invuln_count += 1;
        u.magic_immune += 1;
        serial
    }

    /// Whether the Divine Shield of `unit` may be cancelled (`0x6F089A50`: `DataA` is not zero; the Misc test of the
    /// original is not modelled).
    pub fn can_cancel_divine_shield(&self, unit: ObjKey) -> bool {
        self.ability_level_record(unit, DIVINE_SHIELD)
            .is_some_and(|l| nonzero(l.data[0]))
    }

    /// `undivineshield`: removes the `BHds` buff when the ability allows it. Returns whether a buff went.
    pub fn cancel_divine_shield(&mut self, unit: ObjKey) -> bool {
        if !self.has_ability(unit, DIVINE_SHIELD) || !self.can_cancel_divine_shield(unit) {
            return false;
        }
        let Some(i) = self.buff_position(unit, BUFF_DIVINE_SHIELD) else {
            return false;
        };
        let mut b = self.unit_mut(unit).buffs.remove(i);
        self.finish_buff(unit, &mut b);
        true
    }

    // ---- Resurrection -------------------------------------------------------------------------------------------

    /// Brings a corpse back: not dead, full life (the revive passes the fraction 1.0, `0x6F1B5A60`).
    pub(crate) fn resurrect_unit(&mut self, k: ObjKey) {
        let max = self.unit(k).max_life;
        self.unit_mut(k).dead = false;
        self.write_life(k, max);
        self.stats_set_dead(k, false);
    }
}

fn thunder_clap_targets(gm: &Game, caster: ObjKey, level: &SpellLevel) -> Vec<ObjKey> {
    let c = gm.unit(caster);
    let owner = c.owner;
    circle_candidates(gm, c.x, c.y, level.area)
        .into_iter()
        .filter(|&k| k != caster && gm.alive(k))
        .filter(|&k| {
            let t = gm.unit(k);
            let flying = gm
                .unit_types()
                .get(t.type_id)
                .is_some_and(|ty| ty.movement & MOVE_FLY != 0);
            gm.is_enemy(owner, t.owner) && !gm.unit_types().is_building(t.type_id) && !flying
        })
        .collect()
}

fn thunder_clap(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let level = rec.level;
    let targets = thunder_clap_targets(&game.borrow(), rec.caster, &level);
    for t in targets {
        let died = {
            let mut gm = game.borrow_mut();
            if !gm.alive(t) {
                continue;
            }
            gm.unit_damage_target(rec.caster, t, level.data[0], false, false, 0, THUNDER_CLAP_DAMAGE_TYPE)
        };
        if let Some(k) = died {
            dispatch_unit_death_by(game, rt, k, Some(rec.caster));
            continue;
        }
        let mut gm = game.borrow_mut();
        if !gm.alive(t) {
            continue;
        }
        let hero = gm.target_hero(t);
        let dur = effect_duration(&level, hero, F32::ZERO);
        if nonzero(dur) {
            // The buff gets `-DataC` (movement) and `-DataD` (attack speed), `0x6F18A930`.
            gm.apply_slow_as(
                BUFF_THUNDER_CLAP,
                t,
                dur,
                arith::neg(level.data[2]),
                arith::neg(level.data[3]),
            );
        }
    }
}

fn avatar(game: &RefCell<Game>, rec: &CastRec) {
    let l = rec.level;
    // Avatar always takes `Dur` (+ the unknown extra `ability+0x9C`, 0.0 here): `0x6F1CA180` has no hero test.
    let dur = arith::add(l.dur, F32::ZERO);
    game.borrow_mut()
        .apply_avatar(rec.caster, dur, l.data[0], l.data[1], conv::f2i(l.data[2]));
}

fn divine_shield(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    // The hero test is on the caster (`0x6F0B64C0`: `0x6F1D2660(owner)`).
    let hero = gm.target_hero(rec.caster);
    let dur = effect_duration(&rec.level, hero, F32::ZERO);
    gm.apply_divine_shield(rec.caster, dur);
}

/// A candidate record of the Resurrection sort (`0x6F1D1720` fills 20 bytes): the corpse, an integer key (`+4`, meaning
/// NOT FOUND: the filler `0x6F1CFF70` was not located), a real (`+8`), a flag (`+0xC`) and the caster's player
/// (`+0x10`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResurrectionRec {
    pub owner: i32,
    pub key: i32,
    pub real: F32,
    pub flag: i32,
    pub player: i32,
    /// The corpse's agent id (`unit + 0xC`), the final tie break.
    pub id: i32,
}

/// The sort comparator `0x6F19EA20` (CONFIRMED, Unicorn: 400 cases): negative when `a` goes first. The corpse of the
/// caster's own player first; then the larger key first (`0x6F19E970`, with the wrapped difference test of the original),
/// then a zero flag first, then the smaller real first (a NaN difference puts `a` last), then the larger agent id first.
pub fn resurrection_compare(a: &ResurrectionRec, b: &ResurrectionRec) -> i32 {
    if a.owner != b.owner {
        if a.owner == a.player {
            return -1;
        }
        if b.owner == b.player {
            return 1;
        }
    }
    let d = a.key.wrapping_sub(b.key);
    if d != 0 {
        return if d <= 0 { 1 } else { -1 };
    }
    if a.flag != b.flag {
        return if b.flag == 0 { 1 } else { -1 };
    }
    let v = arith::sub(a.real, b.real).to_f32();
    if v.is_nan() {
        1
    } else if v == 0.0 {
        b.id.wrapping_sub(a.id)
    } else if v > 0.0 {
        1
    } else {
        -1
    }
}

fn resurrection(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let max = conv::f2i(rec.level.data[0]);
    let (cx, cy, owner) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner)
    };
    let area2 = arith::mul(rec.level.area, rec.level.area);
    let mut recs = Vec::new();
    for i in 0..gm.object_count() {
        let k = ObjKey(i as u32);
        if !matches!(gm.object(k), Object::Unit(_)) || k == rec.caster {
            continue;
        }
        let u = gm.unit(k);
        if !u.dead || u.removed {
            continue;
        }
        let (dx, dy) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
        let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
        if !arith::le(d2, area2) || gm.is_enemy(owner, u.owner) {
            continue;
        }
        // The filler of the key fields is NOT FOUND (UNSURE): key, real and flag are all equal here, so the sort
        // reduces to "the caster's player first, then the larger agent id".
        recs.push((
            k,
            ResurrectionRec {
                owner: u.owner as i32,
                key: 0,
                real: F32::ZERO,
                flag: 0,
                player: owner as i32,
                id: k.0 as i32,
            },
        ));
    }
    recs.sort_by(|a, b| resurrection_compare(&a.1, &b.1).cmp(&0));
    let count = usize::try_from(max).unwrap_or(0);
    for (k, _) in recs.into_iter().take(count) {
        gm.resurrect_unit(k);
    }
}

fn water_elemental(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let n = conv::f2i(rec.level.data[0]);
    let dur = arith::add(rec.level.dur, F32::ZERO);
    let (x, y, owner, facing) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner, c.facing)
    };
    if rec.level.unit_id == 0 {
        return;
    }
    for _ in 0..n.max(0) {
        // UNSURE: the original places the unit at a spot found around the caster (`0x6F1D26B0`); the model uses the
        // caster's position.
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, rec.level.unit_id, x, y, facing, true) {
            gm.unit_mut(k).summoned = true;
            gm.apply_timed_life(k, BUFF_WATER_ELEMENTAL, dur);
        }
    }
}


// ---------------------------------------------------------------------------------------------------------------
// Blizzard (`AHbz`): the effect object `XHbz` (`CEffectBlizzard`, `0x6F8FB97C`). Spec section 11.

/// Level fields as `0x6F1B9E90` hands them to the effect setup `0x6F1B9BB0` (CONFIRMED, Unicorn: 400 level records).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlizzardParams {
    /// Number of waves: `f2i(DataA)` (stored at `+0x78`/`+0x84`; the setup clamps it to at least 1).
    pub waves: i32,
    /// Wave interval: the level's `Cast` field (`+0x80`).
    pub interval: F32,
    /// Damage per wave and unit: `DataB` (`+0x8C`).
    pub damage: F32,
    /// Cap on the total damage of one wave: `DataF` (`+0x94`), 0 = none.
    pub cap: F32,
    /// Shards per wave: `f2i(DataC)` (the setup clamps it to `1..=32`, `+0x98`).
    pub shards: i32,
    /// Building factor: `DataD` (`+0xAC`).
    pub building: F32,
    /// `DataE` (`+0x354` property), `Dur + 0` and `HeroDur` (`+0x35C`/`+0x364`): the optional debuff duration.
    pub data_e: F32,
    pub dur: F32,
    pub hero_dur: F32,
    /// Damage radius: `Area` (`+0x74`).
    pub area: F32,
}

/// The mapping of a level record to the effect parameters.
pub fn blizzard_params(l: &SpellLevel) -> BlizzardParams {
    BlizzardParams {
        waves: conv::f2i(l.data[0]).max(1),
        interval: l.cast,
        damage: l.data[1],
        cap: l.data[5],
        shards: conv::f2i(l.data[2]).clamp(1, 32),
        building: l.data[3],
        data_e: l.data[4],
        dur: arith::add(l.dur, F32::ZERO),
        hero_dur: l.hero_dur,
        area: l.area,
    }
}

/// The unclamped `f2i(DataA)` and `f2i(DataC)` the ability method passes (the vector check compares these).
pub fn blizzard_raw_counts(l: &SpellLevel) -> (i32, i32) {
    (conv::f2i(l.data[0]), conv::f2i(l.data[2]))
}

const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);
const POINT_FOUR: F32 = F32::from_bits(0x3ECC_CCCD);
const HALF: F32 = F32::from_bits(0x3F00_0000);
/// The delay of the first damage wave: the constant `0x6FAAE4F0` (0.9), passed as the start delay of the `0xD01B1`
/// timer by the setup `0x6F1B9BB0` (CONFIRMED by the argument capture; the value is the start-up value of that cell).
const FIRST_DAMAGE_DELAY: f32 = 0.9;

/// The result of one wave's shard placement (`0x6F1AF6E0`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShardWave {
    /// The shuffled launch order (`+0xCC`): the first shard launched is `perm[count - 1]`.
    pub perm: Vec<u32>,
    /// The offset of every shard from the wave centre (properties `+0x150 + 8i` and `+0x254 + 8i`).
    pub offsets: Vec<(F32, F32)>,
}

fn stream_real(rng: &mut Rng, table: &RngTable) -> F32 {
    // `0x6F2853D0`: `add(0.0, mul(1.0, U))` (Unicorn-checked for stream 3 in `human_heroes_vectors.rs`).
    arith::add(F32::ZERO, arith::mul(F32::ONE, rng.random_fraction(table)))
}

/// `0x6F1AF6E0` (CONFIRMED, Unicorn: 400 cases). `n` shards (1..=32) in a circle of radius `area / 2` with a random
/// start angle and a jitter of `area * 0.4 * (U - 0.5)` per axis. With more than five shards the last one is not on
/// the circle but at a random point of the jitter square. Draws: the global generator once per `i = 1..n` for the
/// shuffle, stream 4 for everything else (extra shard x, y; start angle; x, y jitter per circle shard).
pub fn blizzard_shards(
    n: usize,
    area: F32,
    global: &mut Rng,
    stream4: &mut Rng,
    table: &RngTable,
    tables: &Tables,
) -> ShardWave {
    let mut perm: Vec<u32> = (0..n as u32).collect();
    for i in 1..n {
        let r = global.next_u32(table);
        let j = ((u64::from(r) * (i as u64 + 1)) >> 32) as usize;
        perm.swap(i, j);
    }
    let half = arith::mul(area, HALF);
    let jitter = arith::mul(area, POINT_FOUR);
    let mut offsets = vec![(F32::ZERO, F32::ZERO); n];
    let mut circle = n;
    if n > 5 {
        circle = n - 1;
        let jx = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        let jy = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        offsets[circle] = (jx, jy);
    }
    let step = arith::div(TWO_PI, conv::i2r(circle as i32), tables);
    let base = arith::mul(TWO_PI, stream_real(stream4, table));
    for (i, slot) in offsets.iter_mut().enumerate().take(circle) {
        let angle = arith::add(base, arith::mul(step, conv::i2r(i as i32)));
        let c = softfloat::math::cos(angle, tables);
        let s = softfloat::math::sin(angle, tables);
        let rx = arith::mul(half, c);
        let ry = arith::mul(half, s);
        let jx = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        let jy = arith::mul(jitter, arith::sub(stream_real(stream4, table), HALF));
        *slot = (arith::add(rx, jx), arith::add(ry, jy));
    }
    ShardWave { perm, offsets }
}

/// The damage one unit takes in one wave (`0x6F1AF950`, from the disassembly; not Unicorn-run, LIKELY): the wave cap
/// divides the damage when `damage * count` exceeds it, then the building factor applies to structures.
pub fn blizzard_unit_damage(p: &BlizzardParams, count: i32, structure: bool, tables: Option<&Tables>) -> F32 {
    let mut d = p.damage;
    if nonzero(p.cap) {
        let total = arith::mul(d, conv::i2r(count));
        if arith::lt(p.cap, total) {
            // The division needs the soft-float tables; without them the cap cannot be applied.
            if let Some(t) = tables {
                d = arith::div(p.cap, conv::i2r(count), t);
            }
        }
    }
    if structure {
        d = arith::mul(d, p.building);
    }
    d
}

#[derive(Debug, Clone)]
struct BlizzardFx {
    caster: ObjKey,
    x: F32,
    y: F32,
    p: BlizzardParams,
    visual_left: i32,
    damage_left: i32,
}

/// Mass Teleport channel in progress.
#[derive(Debug, Clone, Copy)]
struct TeleportFx {
    caster: ObjKey,
    dest: ObjKey,
    level: SpellLevel,
}

/// State of the effects this module creates (owned by the spell system).
#[derive(Debug, Clone, Default)]
pub struct State {
    next: u32,
    blizzards: std::collections::BTreeMap<u32, BlizzardFx>,
    teleports: std::collections::BTreeMap<u32, TeleportFx>,
    /// Every shard wave placed, `(effect id, wave)`, for tests and the renderer.
    pub log: Vec<(u32, ShardWave)>,
}

/// Timer kinds of [`crate::cast::SpellMsg::Human`].
pub const TICK_BLIZZARD_WAVE: u8 = 1;
pub const TICK_BLIZZARD_DAMAGE: u8 = 2;
pub const TICK_TELEPORT: u8 = 3;

fn human_timer(gm: &mut Game, seconds: f32, kind: u8, id: u32) {
    gm.timers.start(
        seconds,
        false,
        crate::natives::game::Payload::Spell(crate::cast::SpellMsg::Human { kind, id }),
    );
}

fn blizzard_start(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Point(x, y) = rec.target else {
        return;
    };
    let p = blizzard_params(&rec.level);
    let id = {
        let mut gm = game.borrow_mut();
        gm.spells.human.next += 1;
        let id = gm.spells.human.next;
        gm.spells.human.blizzards.insert(
            id,
            BlizzardFx {
                caster: rec.caster,
                x,
                y,
                p,
                visual_left: p.waves,
                damage_left: p.waves,
            },
        );
        id
    };
    // `0x6F1B9D2D`: the first visual wave starts at once, the damage timer after 0.9 s.
    blizzard_wave(game, id);
    human_timer(&mut game.borrow_mut(), FIRST_DAMAGE_DELAY, TICK_BLIZZARD_DAMAGE, id);
}

/// Message `0xD01B0` (`0x6F1B6C00`): a visual wave: one placement, the counter, the next wave.
fn blizzard_wave(game: &RefCell<Game>, id: u32) {
    let mut gm = game.borrow_mut();
    let Some(mut fx) = gm.spells.human.blizzards.get(&id).cloned() else {
        return;
    };
    let tables = gm.order_tables().cloned();
    let mut placed = None;
    if let Some(tables) = tables {
        if let Some((table, streams)) = gm.rng_parts() {
            let table = table.clone();
            let mut global = streams.global;
            let mut s4 = streams.stream(4);
            let wave = blizzard_shards(fx.p.shards as usize, fx.p.area, &mut global, &mut s4, &table, &tables);
            streams.global = global;
            *streams.stream_mut(4) = s4;
            placed = Some(wave);
        }
    }
    if let Some(w) = placed {
        gm.spells.human.log.push((id, w));
    }
    fx.visual_left -= 1;
    let again = fx.visual_left > 0;
    let interval = fx.p.interval.to_f32();
    gm.spells.human.blizzards.insert(id, fx);
    if again {
        human_timer(&mut gm, interval, TICK_BLIZZARD_WAVE, id);
    }
}

/// Message `0xD01B1` (`0x6F1B9DB0`): the damage wave over the area.
fn blizzard_damage(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(fx) = game.borrow().spells.human.blizzards.get(&id).cloned() else {
        return;
    };
    // UNSURE: the effect ends with its caster (the channel end `0x6F1AFBE0` destroys it).
    if !game.borrow().alive(fx.caster) {
        game.borrow_mut().spells.human.blizzards.remove(&id);
        return;
    }
    let (targets, tables) = {
        let gm = game.borrow();
        let owner = gm.unit(fx.caster).owner;
        // UNSURE: the stock `targs` is empty (`_`); the model hits enemy units and structures.
        let t: Vec<ObjKey> = circle_candidates(&gm, fx.x, fx.y, fx.p.area)
            .into_iter()
            .filter(|&k| gm.alive(k) && gm.is_enemy(owner, gm.unit(k).owner))
            .collect();
        (t, gm.order_tables().cloned())
    };
    let count = targets.len() as i32;
    for t in targets {
        let amount = {
            let gm = game.borrow();
            if !gm.alive(t) {
                continue;
            }
            let structure = gm.unit_types().is_building(gm.unit(t).type_id);
            blizzard_unit_damage(&fx.p, count, structure, tables.as_ref())
        };
        let died = game
            .borrow_mut()
            .unit_damage_target(fx.caster, t, amount, false, false, 0, BLIZZARD_DAMAGE_TYPE);
        if let Some(k) = died {
            dispatch_unit_death_by(game, rt, k, Some(fx.caster));
        }
    }
    let mut gm = game.borrow_mut();
    if let Some(e) = gm.spells.human.blizzards.get_mut(&id) {
        e.damage_left -= 1;
        let (again, iv, done) = (e.damage_left > 0, e.p.interval.to_f32(), e.visual_left <= 0);
        if again {
            human_timer(&mut gm, iv, TICK_BLIZZARD_DAMAGE, id);
        } else if done {
            gm.spells.human.blizzards.remove(&id);
        }
    }
}

/// Damage type of Blizzard: the effect's class word is `0x200` (`0x6F1B45F0`), `1 << 9` = `DAMAGE_TYPE_COLD`.
const BLIZZARD_DAMAGE_TYPE: u32 = 9;

// ---------------------------------------------------------------------------------------------------------------
// Mass Teleport (`AHmt`). Spec section 12.

/// A candidate record of the selection (`0x6F056890`, 20 bytes): the unit, `f2i` of the squared distance to the
/// caster, the level key, the hero flag, the "same owner as the caster" flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeleportRec {
    pub dist: i32,
    pub key: i32,
    pub hero: i32,
    pub own: i32,
    /// The unit's agent id (`unit + 0xC`), the final tie break.
    pub id: i32,
}

/// The sort comparator `0x6F026120` (CONFIRMED, Unicorn: 400 cases): negative when `a` goes first. Own units first,
/// heroes first, higher key first, nearer first, then the larger agent id first.
pub fn teleport_compare(a: &TeleportRec, b: &TeleportRec) -> i32 {
    let c = b.own.wrapping_sub(a.own);
    if c != 0 {
        return c;
    }
    let c = b.hero.wrapping_sub(a.hero);
    if c != 0 {
        return c;
    }
    let c = b.key.wrapping_sub(a.key);
    if c != 0 {
        return c;
    }
    let c = a.dist.wrapping_sub(b.dist);
    if c != 0 {
        return c;
    }
    b.id.wrapping_sub(a.id)
}

/// The count rule of `0x6F069D60` (CONFIRMED structure): with `limit = f2i(DataA) - 1` (compared as unsigned, so
/// `DataA < 1` never trims), more candidates than the limit are sorted and cut to the limit.
pub fn teleport_select(mut recs: Vec<(ObjKey, TeleportRec)>, data_a: i32) -> Vec<(ObjKey, TeleportRec)> {
    let limit = data_a.wrapping_sub(1) as u32;
    if recs.len() as u32 > limit {
        recs.sort_by(|a, b| teleport_compare(&a.1, &b.1).cmp(&0));
        recs.truncate(limit as usize);
    }
    recs
}

fn teleport_start(game: &RefCell<Game>, rec: &CastRec) {
    let CastTarget::Unit(dest) = rec.target else {
        return;
    };
    let mut gm = game.borrow_mut();
    gm.spells.human.next += 1;
    let id = gm.spells.human.next;
    gm.spells.human.teleports.insert(
        id,
        TeleportFx {
            caster: rec.caster,
            dest,
            level: rec.level,
        },
    );
    // The delay is DataB (the property at `+0x90` is set from it by the level init `0x6F060AE0`; LIKELY).
    let delay = rec.level.data[1].to_f32();
    human_timer(&mut gm, delay, TICK_TELEPORT, id);
}

fn teleport_finish(game: &RefCell<Game>, id: u32) {
    let Some(fx) = game.borrow_mut().spells.human.teleports.remove(&id) else {
        return;
    };
    let mut gm = game.borrow_mut();
    if !gm.alive(fx.caster) || !gm.alive(fx.dest) {
        return;
    }
    let (cx, cy, owner) = {
        let c = gm.unit(fx.caster);
        (c.x, c.y, c.owner)
    };
    let (dx, dy) = {
        let d = gm.unit(fx.dest);
        (d.x, d.y)
    };
    let mut recs = Vec::new();
    for k in circle_candidates(&gm, cx, cy, fx.level.area) {
        if k == fx.caster || !gm.alive(k) {
            continue;
        }
        let u = gm.unit(k);
        // UNSURE: the filter word `0x40303006` of the group builder; the model takes allied and own non-structures.
        if gm.is_enemy(owner, u.owner) || gm.unit_types().is_building(u.type_id) {
            continue;
        }
        let (ex, ey) = (arith::sub(u.x, cx), arith::sub(u.y, cy));
        let d2 = arith::add(arith::add(arith::mul(ex, ex), arith::mul(ey, ey)), F32::ZERO);
        let hero = u.hero.as_ref().map(|h| h.level);
        recs.push((
            k,
            TeleportRec {
                dist: conv::f2i(d2),
                key: hero.unwrap_or(0),
                hero: i32::from(hero.is_some() && !u.illusion),
                own: i32::from(u.owner == owner),
                id: k.0 as i32,
            },
        ));
    }
    let chosen = teleport_select(recs, conv::f2i(fx.level.data[0]));
    let cluster = nonzero(fx.level.data[2]);
    // The caster goes to the destination unit's position, the others keep their offset when clustering (UNSURE).
    let (px, py) = gm.clamp_unit_move(dx, dy);
    gm.store_unit_position(fx.caster, px, py);
    for (k, _) in chosen {
        let (ux, uy) = {
            let u = gm.unit(k);
            (u.x, u.y)
        };
        let (tx, ty) = if cluster {
            (arith::add(dx, arith::sub(ux, cx)), arith::add(dy, arith::sub(uy, cy)))
        } else {
            (dx, dy)
        };
        let (px, py) = gm.clamp_unit_move(tx, ty);
        gm.store_unit_position(k, px, py);
    }
}

/// The class effects of the point and unit target spells of this module; `false` when `rec.base` is not one.
pub(crate) fn run_target_effect(game: &RefCell<Game>, rec: &CastRec) -> bool {
    match rec.base {
        BLIZZARD => blizzard_start(game, rec),
        MASS_TELEPORT => teleport_start(game, rec),
        _ => return false,
    }
    true
}

/// A timer of [`crate::cast::SpellMsg::Human`].
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, id: u32) {
    match kind {
        TICK_BLIZZARD_WAVE => blizzard_wave(game, id),
        TICK_BLIZZARD_DAMAGE => blizzard_damage(game, rt, id),
        TICK_TELEPORT => teleport_finish(game, id),
        _ => {}
    }
}

/// The effect of an immediate spell (the class effect slot `0x3C0` / `0x428`), called by the cast core.
pub(crate) fn run_immediate_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    match rec.base {
        THUNDER_CLAP => thunder_clap(game, rt, rec),
        AVATAR => avatar(game, rec),
        DIVINE_SHIELD => divine_shield(game, rec),
        RESURRECTION => resurrection(game, rec),
        WATER_ELEMENTAL => water_elemental(game, rec),
        _ => {}
    }
}

/// An immediate order of `unit`: a spell order starts the cast, `undivineshield` removes the shield. `None` when the
/// id is not one of these (the order system takes it).
pub(crate) fn immediate_order(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    id: u32,
) -> Option<bool> {
    if id == ORDER_UNDIVINE_SHIELD {
        return Some(game.borrow_mut().cancel_divine_shield(unit));
    }
    let base = game.borrow().spell_for_order(unit, id)?;
    if !is_immediate_spell(base) {
        return None;
    }
    Some(begin_cast(game, rt, unit, base, CastTarget::None).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prd_index_rounds_to_five_percent_steps() {
        let f = |v: f32| F32::from_bits(v.to_bits());
        assert_eq!(prd_index(f(0.0)), 0);
        assert_eq!(prd_index(f(25.0)), 5);
        assert_eq!(prd_index(f(15.0)), 3);
        assert_eq!(prd_index(f(100.0)), 20);
        assert_eq!(prd_index(f(500.0)), 20);
        assert_eq!(prd_index(f(-40.0)), 0);
    }

    #[test]
    fn immediate_spells_are_the_five_casts() {
        assert!(is_immediate_spell(THUNDER_CLAP));
        assert!(!is_immediate_spell(BASH));
        assert!(!is_immediate_spell(code(b"AHtb")));
    }

    #[test]
    fn codes_are_the_four_characters() {
        assert_eq!(BUFF_PAUSE, 0x4250_5345);
        assert_eq!(THUNDER_CLAP, 0x4148_7463);
    }
}
