//! S48: the hero and hero-like abilities that had no model yet. Spec: `docs/spec/abilities-hero-extra.md`.
//!
//! Night elf: Fan of Knives `AEfk`, Mana Burn `AEmb`, Metamorphosis `AEme`, Poison Arrows `AEpa`, Starfall `AEsf`,
//! Scout `AEst`, Spirit of Vengeance `AEsv`, Tranquility `AEtq`. Undead: Animate Dead `AUan`, Carrion Scarabs `AUcb`,
//! Dark Summoning `AUds`, Locust Swarm `AUls`. Human: Cold Arrows `AHca`, Searing (Flaming) Arrows `AHfa`. Neutral:
//! creep Animate Dead `ACad`, creep Devour `ACdv`,. Orc: `AOwd` is the factory base of
//! the Serpent Ward (`orc.rs` runs it under `AOsw` and now also under its base code).
//!
//! The class identities, vtables, parents and cast order ids come from the ability atlas
//! (`docs/research/ability-atlas/factory.tsv`) and the order getters (`docs/research/ability-orders`); the level data
//! meanings are read from the 1.26a `AbilityData.slk` rows. The *numbers* of the effects are models: nothing here ran in
//! Unicorn, so every rule below carries a label (LIKELY = from the data layout and the public tooltips, UNSURE = a
//! stand-in). Starfall and Tranquility use the shared channel lifecycle of `cast.rs` (channel time, `0xD01BD`, end hook).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32};

use crate::abilities::code;
use crate::cast::{BuffEffect, CastRec, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey};
use crate::natives::group::circle_candidates;
use crate::natives::{dispatch_unit_death_by, set_life, ScriptRuntime};

pub const FAN_OF_KNIVES: u32 = code(b"AEfk");
pub const MANA_BURN: u32 = code(b"AEmb");
pub const METAMORPHOSIS: u32 = code(b"AEme");
pub const POISON_ARROWS: u32 = code(b"AEpa");
pub const STARFALL: u32 = code(b"AEsf");
pub const SCOUT: u32 = code(b"AEst");
pub const SPIRIT_OF_VENGEANCE: u32 = code(b"AEsv");
pub const TRANQUILITY: u32 = code(b"AEtq");
pub const ANIMATE_DEAD: u32 = code(b"AUan");
pub const CARRION_SCARABS: u32 = code(b"AUcb");
pub const DARK_SUMMONING: u32 = code(b"AUds");
pub const LOCUST_SWARM: u32 = code(b"AUls");
pub const COLD_ARROWS: u32 = code(b"AHca");
pub const SEARING_ARROWS: u32 = code(b"AHfa");
pub const CREEP_ANIMATE_DEAD: u32 = code(b"ACad");
pub const CREEP_DEVOUR: u32 = code(b"ACdv");

pub const BUFF_METAMORPHOSIS: u32 = code(b"BEme");
pub const BUFF_SCOUT: u32 = code(b"BEst");
pub const BUFF_VENGEANCE: u32 = code(b"BEsv");
pub const BUFF_ANIMATE_DEAD: u32 = code(b"BUan");
pub const BUFF_SCARABS: u32 = code(b"BUcb");
pub const BUFF_LOCUST: u32 = code(b"BUls");
pub const BUFF_COLD_ARROWS: u32 = code(b"BHca");
pub const BUFF_POISON_ARROWS: u32 = code(b"Bpoi");
pub const BUFF_DEVOURED: u32 = code(b"Bdvv");

/// Cast order ids (slot `0x348` getters, `tools/analysis/ability_orders.py`, `docs/research/ability-orders`).
pub const ORDERS: &[(u32, u32)] = &[
    (852_526, FAN_OF_KNIVES),
    (852_179, MANA_BURN),
    (852_180, METAMORPHOSIS),
    (852_181, SCOUT),
    (852_528, SPIRIT_OF_VENGEANCE),
    (852_183, STARFALL),
    (852_184, TRANQUILITY),
    (852_217, ANIMATE_DEAD),
    (852_551, CARRION_SCARABS),
    (852_220, DARK_SUMMONING),
    (852_556, LOCUST_SWARM),
    (852_246, CREEP_ANIMATE_DEAD),
    (852_247, CREEP_DEVOUR),
];

// Damage type numbers (the JASS constants).
const DT_FIRE: u32 = 8;
const DT_COLD: u32 = 9;
const DT_POISON: u32 = 11;
const DT_MAGIC: u32 = 14;

/// Timer kinds (`SpellMsg::HeroSpell`, below `0x80`; the numbers continue `undead_ne::TICK_*`).
pub const TICK_META_START: u8 = 20;
pub const TICK_CHANNEL: u8 = 21;
pub const TICK_DEVOUR_END: u8 = 22;

#[derive(Debug, Clone, Copy)]
struct Channel {
    caster: ObjKey,
    base: u32,
    level: SpellLevel,
}

#[derive(Debug, Clone, Copy)]
struct Devour {
    caster: ObjKey,
    target: ObjKey,
    serial: u32,
}

/// State of this module (a field of `undead_ne::State`).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Running Metamorphoses: buff serial -> (unit, bonus life).
    meta: BTreeMap<u32, (ObjKey, F32)>,
    /// Metamorphosis casts waiting for the transform delay: id -> (caster, level).
    meta_pending: BTreeMap<u32, (ObjKey, SpellLevel)>,
    /// Running channels (Starfall, Tranquility) by cast id.
    channels: BTreeMap<u32, Channel>,
    devours: BTreeMap<u32, Devour>,
    /// Locust Swarms still releasing locusts by cast id (V7).
    locusts: BTreeMap<u32, Locust>,
    next: u32,
    /// The unit type a spell creates when the level record carries none (Carrion Scarabs' `DataC` rawcode).
    extra_unit: BTreeMap<u32, u32>,
    /// Starfall waves and Tranquility pulses delivered, `(caster, base, units hit)` (tests).
    pub pulses: Vec<(ObjKey, u32, usize)>,
}

impl Game {
    /// Host API: the unit type `base` creates when its level record has no `UnitID` (Carrion Scarabs: `ucs1`..`ucs3`).
    pub fn set_extra_unit(&mut self, base: u32, unit_type: u32) {
        self.spells.hero.extra.extra_unit.insert(base, unit_type);
    }

    /// The Starfall waves and Tranquility pulses so far, `(caster, ability, units affected)`.
    pub fn channel_pulses(&self) -> &[(ObjKey, u32, usize)] {
        &self.spells.hero.extra.pulses
    }
}

/// Abilities cast without a target (the immediate orders).
pub fn takes_no_target(base: u32) -> bool {
    matches!(
        base,
        FAN_OF_KNIVES
            | METAMORPHOSIS
            | SCOUT
            | SPIRIT_OF_VENGEANCE
            | STARFALL
            | TRANQUILITY
            | ANIMATE_DEAD
            | CREEP_ANIMATE_DEAD
            | LOCUST_SWARM
    )
}

/// The channel time of a channelled cast (LIKELY: the hero rule duration, `HeroDur`, 30 s for both spells; Starfall's
/// `Dur` of 45 s is the non-hero form of the field).
pub fn channel_time_override(base: u32, level: &SpellLevel) -> Option<F32> {
    match base {
        STARFALL | TRANQUILITY => Some(if arith::lt(F32::ZERO, level.hero_dur) { level.hero_dur } else { level.dur }),
        _ => None,
    }
}

pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match (rec.base, rec.target) {
        (FAN_OF_KNIVES, _) => fan_of_knives(game, rt, rec),
        (MANA_BURN, CastTarget::Unit(t)) => mana_burn(game, rt, rec, t),
        (METAMORPHOSIS, _) => metamorphosis(game, rec),
        (SCOUT, _) => summon_at_caster(game, rec, BUFF_SCOUT),
        (SPIRIT_OF_VENGEANCE, _) => summon_at_caster(game, rec, BUFF_VENGEANCE),
        (LOCUST_SWARM, _) => locust_swarm(game, rec),
        (STARFALL | TRANQUILITY, _) => channel_start(game, rec),
        (ANIMATE_DEAD | CREEP_ANIMATE_DEAD, _) => animate_dead(game, rec),
        (CARRION_SCARABS, CastTarget::Unit(t)) => carrion_scarabs(game, rec, t),
        (DARK_SUMMONING, CastTarget::Unit(t)) => dark_summoning(game, rec, t),
        (CREEP_DEVOUR, CastTarget::Unit(t)) => devour(game, rec, t),
        (
            MANA_BURN | CARRION_SCARABS | DARK_SUMMONING | CREEP_DEVOUR,
            _,
        ) => {}
        _ => return false,
    }
    true
}

/// Whether `t` is a legal unit target for the cast check: a live unit, except for Carrion Scarabs whose target is a
/// corpse (targs `dead`).
pub(crate) fn target_ok(gm: &Game, base: u32, t: ObjKey) -> bool {
    if base == CARRION_SCARABS || crate::unit_abilities_a2::wants_corpse(base) {
        let u = gm.unit(t);
        return u.dead && !u.removed;
    }
    gm.alive(t)
}

/// `true` for abilities of this module whose effect is a passive on-hit orb (no cast).
pub fn is_orb(base: u32) -> bool {
    matches!(base, COLD_ARROWS | SEARING_ARROWS | POISON_ARROWS)
}

fn is_enemy_unit(gm: &Game, owner: u32, k: ObjKey) -> bool {
    gm.alive(k) && gm.is_enemy(owner, gm.unit(k).owner)
}

fn organic_ground_or_air(gm: &Game, k: ObjKey) -> bool {
    let u = gm.unit(k);
    !crate::natives::unit_attr::is_unit_type(gm, u, crate::unitclass::ty::STRUCTURE)
        && !crate::natives::unit_attr::is_unit_type(gm, u, crate::unitclass::ty::MECHANICAL)
}

fn dispatch(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, killer: ObjKey, dead: Vec<ObjKey>) {
    for t in dead {
        dispatch_unit_death_by(game, rt, t, Some(killer));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Fan of Knives (`CAbilityFanOfKnives`, `0x6F8AAC14`, a `CAbilitySimpleSpell`, effect slot `0x428` = `0x6F08C100`).

/// The per-victim amounts of the area launcher `0x6F2C5C40` (CONFIRMED, Unicorn 400 cases, `tools/emu/v2_fan_emu.py`): `DataB` is a
/// pool ("max damage", 1.0 when zero, then the pool is not used up), victims are taken nearest first; before each victim the loop
/// stops unless the pool is above 0.0 or when `limit != 0 && i > limit` (`limit` = F2I(`DataC`), so `limit + 1` victims at
/// most); a pool victim takes `prev - (prev - DataA)` through the truncating soft float (so it can differ from `DataA` by one
/// ulp) and the pool falls by `DataA`: the victim that crosses zero is still hit in full.
pub fn fan_plan(n: usize, data_a: F32, data_b: F32, limit: i32) -> Vec<F32> {
    let pooled = data_b.to_f32() != 0.0;
    let mut pool = if pooled { data_b } else { F32::from_bits(0x3F80_0000) };
    let mut out = Vec::new();
    for i in 0..n {
        if !arith::lt(F32::ZERO, pool) || (limit != 0 && i as i64 > limit as i64) {
            break;
        }
        let mut each = data_a;
        if pooled {
            let prev = pool;
            pool = arith::sub(pool, data_a);
            each = arith::sub(prev, pool);
        }
        out.push(each);
    }
    out
}

fn fan_of_knives(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) {
    let mut dead = Vec::new();
    {
        let mut gm = game.borrow_mut();
        if !gm.alive(rec.caster) {
            return;
        }
        let (cx, cy, owner) = {
            let c = gm.unit(rec.caster);
            (c.x, c.y, c.owner)
        };
        // targs `air,ground,enemy,organic`: living enemies that are neither structures nor mechanical.
        let victims: Vec<ObjKey> = circle_candidates(&gm, cx, cy, rec.level.area)
            .into_iter()
            .filter(|&k| is_enemy_unit(&gm, owner, k) && organic_ground_or_air(&gm, k) && !gm.unit_is_invulnerable(k))
            .collect();
        // Nearest first (the launcher sorts its collector by distance, ties by unit id).
        let mut victims: Vec<(F32, ObjKey)> = victims
            .into_iter()
            .map(|k| {
                let u = gm.unit(k);
                (arith::dist2(u.x, u.y, cx, cy), k)
            })
            .collect();
        victims.sort_by(|a, b| {
            if arith::lt(a.0, b.0) {
                std::cmp::Ordering::Less
            } else if arith::lt(b.0, a.0) {
                std::cmp::Ordering::Greater
            } else {
                a.1 .0.cmp(&b.1 .0)
            }
        });
        let plan = fan_plan(victims.len(), rec.level.data[0], rec.level.data[1], conv::f2i(rec.level.data[2]));
        for ((_, k), each) in victims.into_iter().zip(plan) {
            if let Some(d) = gm.unit_damage_target(rec.caster, k, each, false, false, 0, DT_MAGIC) {
                dead.push(d);
            }
        }
    }
    dispatch(game, rt, rec.caster, dead);
}

// ---------------------------------------------------------------------------------------------------------------
// Mana Burn (`CAbilityManaBurn`, `0x6F8DEFBC`): the cast launches a bolt (`0x6F135220` -> launcher `0x6F2C5C40` with
// DataA, DataB and the missile art); the burn itself is applied on arrival.

/// The burn of one impact (CONFIRMED, Unicorn 400 cases, `tools/emu/v2_manaburn_emu.py`: impact `0x6F119650` with the mana taker
/// `0x6F283DB0`): the amount is `DataA` (`0.0` for an invulnerable or magic-immune target, or when the missile has no ability),
/// `burned = min(mana, amount)` (strict `<`), the target's mana becomes `mana + (-burned)` (soft add) and the damage dealt is
/// `burned` itself. `DataB` and `DataC` are never read by the launch or the impact. Returns `(mana after, damage)`.
pub fn mana_burn_result(mana: F32, data_a: F32, blocked: bool) -> (F32, F32) {
    let amount = if blocked { F32::ZERO } else { data_a };
    let burned = if arith::lt(mana, amount) { mana } else { amount };
    (arith::add(mana, F32::from_bits(burned.to_bits() ^ 0x8000_0000)), burned)
}

fn mana_burn(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let mut dead = None;
    {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        let blocked = gm.unit_is_invulnerable(target) || gm.unit(target).magic_immune > 0;
        let (rest, damage) = mana_burn_result(gm.unit(target).mana, rec.level.data[0], blocked);
        gm.write_mana(target, rest);
        // The original delivers a record (flag 0x10 when the amount is zero) even for a zero burn; the model skips the
        // zero-damage delivery.
        if arith::lt(F32::ZERO, damage) {
            dead = gm.unit_damage_target(rec.caster, target, damage, false, false, 0, DT_MAGIC);
        }
    }
    dispatch(game, rt, rec.caster, dead.into_iter().collect());
}

// ---------------------------------------------------------------------------------------------------------------
// Metamorphosis (`CAbilityMetamorphosis`, `0x6F8D9A1C`, a `CAbilityMorph`): after the transform delay `Dur` (1.5 s) the
// hero becomes `UnitID` (`Edmm`) for `HeroDur`; `DataA` is the normal form's id, `DataE` the bonus hit points.

fn metamorphosis(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    gm.spells.hero.extra.next += 1;
    let id = gm.spells.hero.extra.next;
    gm.spells.hero.extra.meta_pending.insert(id, (rec.caster, rec.level));
    gm.timers_start_hero(rec.level.dur.to_f32().max(0.0), TICK_META_START, rec.caster, id);
}

/// The transform: the `BEme` buff for `HeroDur` and the bonus hit points `DataE` on maximum and current life (UNSURE: the
/// morph into `UnitID` with the form's own attack and abilities is not applied; `CAbilityMorph` swaps the unit type, which
/// needs the unit builder).
fn meta_start(game: &RefCell<Game>, id: u32) {
    let mut gm = game.borrow_mut();
    let Some((caster, level)) = gm.spells.hero.extra.meta_pending.remove(&id) else {
        return;
    };
    if !gm.alive(caster) {
        return;
    }
    // V7 (CONFIRMED, `0x6F13D5C0`): an earlier `BEme` buff is removed; the buff lasts `HeroDur + 0.01` (none for an illusion or a
    // `HeroDur` that is not above zero); the `DataE` bonus goes through the life sink in mode 3 whether or not a buff was made.
    let illusion = gm.unit(caster).illusion;
    let plan = meta_in_plan(level.hero_dur, illusion, level.data[4]);
    let serial = gm.add_spell_buff(caster, BUFF_METAMORPHOSIS, BuffEffect::Marker, plan.buff_duration.unwrap_or(F32::ZERO));
    life_sink_mode3(&mut gm, caster, plan.sink_value);
    if serial != 0 {
        gm.spells.hero.extra.meta.insert(serial, (caster, plan.sink_value));
    }
}

/// What the Metamorphosis morph-in hook (slot `0x40C`, `0x6F13D5C0`) does with the level: the buff duration `add(HeroDur, 0.01)`
/// (`0x6FAAE480`) when the effective `HeroDur` (0.0 for an illusion caster) is above zero, and the life sink value `DataE`
/// (mode 3). CONFIRMED (V7, Unicorn `tools/emu/v7_meta_emu.py`, 300 cases).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetaIn {
    pub buff_duration: Option<F32>,
    pub sink_value: F32,
}

pub fn meta_in_plan(hero_dur: F32, illusion: bool, data_e: F32) -> MetaIn {
    let effective = if illusion { F32::ZERO } else { hero_dur };
    MetaIn {
        buff_duration: arith::lt(F32::ZERO, effective).then(|| arith::add(effective, F32::from_bits(0x3C23_D70A))),
        sink_value: data_e,
    }
}

/// The revert hook (slot `0x414`, `0x6F111D20`): the stored bonus goes through the life sink negated (sign bit flipped), mode 3.
/// CONFIRMED (V7).
pub fn meta_out_value(stored: F32) -> F32 {
    F32::from_bits(stored.to_bits() ^ 0x8000_0000)
}

/// The unit life sink `0x6F2AC450` in mode 3 (maximum and life rise together for a non-negative value; a negative value only
/// lowers the maximum and clamps the life), CONFIRMED earlier (`tests/life_sink_vectors.rs`).
fn life_sink_mode3(gm: &mut Game, unit: ObjKey, value: F32) {
    let flagged = gm.unit(unit).dead;
    let mut cells = gm.life_cells(unit);
    let tables = gm.order_tables().cloned();
    crate::heroes::life_sink(&mut cells, flagged, value, 3, tables.as_ref());
    gm.put_life_cells(unit, cells);
}

/// The buff `serial` ended (called by `Game::drop_spell_buff`): the form wears off, the bonus hit points go.
pub(crate) fn on_buff_dropped(gm: &mut Game, serial: u32) {
    let Some((unit, bonus)) = gm.spells.hero.extra.meta.remove(&serial) else {
        return;
    };
    if !gm.alive(unit) {
        return;
    }
    life_sink_mode3(gm, unit, meta_out_value(bonus));
}

// ---------------------------------------------------------------------------------------------------------------
// Summons at the caster: Scout (`CAbilityScout`, owl `nowl`/`now2`/`now3`), Spirit of Vengeance (`espv`), Locust Swarm.
// CAbilityScout and CAbilitySpiritOfVengeance are `CAbilityWaterElemental` (the shared summon loop `0x6F163390`), so
// they follow Water Elemental: `f2i(DataA)` units of `UnitID` with the timed life `Dur`.

fn summon_at_caster(game: &RefCell<Game>, rec: &CastRec, timed_life: u32) {
    let count = conv::f2i(rec.level.data[0]).max(1);
    summon_n(game, rec, timed_life, count, rec.level.dur);
}

fn summon_n(game: &RefCell<Game>, rec: &CastRec, timed_life: u32, count: i32, dur: F32) {
    let mut gm = game.borrow_mut();
    let unit_type = if rec.level.unit_id != 0 {
        rec.level.unit_id
    } else {
        gm.spells.hero.extra.extra_unit.get(&rec.base).copied().unwrap_or(0)
    };
    if unit_type == 0 || !gm.alive(rec.caster) {
        return;
    }
    let (x, y, owner, facing) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner, c.facing)
    };
    for _ in 0..count.max(0) {
        // UNSURE: the original finds a free spot around the caster; the model uses the caster's position.
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, unit_type, x, y, facing, true) {
            gm.unit_mut(k).summoned = true;
            if arith::lt(F32::ZERO, dur) {
                gm.apply_timed_life(k, timed_life, dur);
            }
        }
    }
}

/// The number of locusts a Locust Swarm releases: `f2i(DataA)` clamped to `1..=40` (stored at ability `+0x308`).
/// CONFIRMED (V7, Unicorn `tools/emu/v7_locust_emu.py`, 300 cases, effect `0x6F134EA0`). Stock `DataA` is 20.
pub fn locust_count(data_a: F32) -> i32 {
    conv::f2i(data_a).clamp(1, 40)
}

/// The Locust Swarm plan (`0x6F134EA0`): the count, the step interval `DataB` (the value the 0xD01C1 timer gets), the total time
/// `add(ability +0x9C, Dur)` of the 0xD01C4 timer and the numbers the locust ability `Aloc` gets (`0x6F124EF0`: `Area`,
/// `f2i(DataC)`, `DataD`, `DataE`). CONFIRMED (V7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocustPlan {
    pub count: i32,
    pub interval: F32,
    pub total: F32,
    pub aloc: (F32, i32, F32, F32),
}

pub fn locust_plan(level: &SpellLevel, extra_9c: F32) -> LocustPlan {
    LocustPlan {
        count: locust_count(level.data[0]),
        interval: level.data[1],
        total: arith::add(level.dur, extra_9c),
        aloc: (level.area, conv::f2i(level.data[2]), level.data[3], level.data[4]),
    }
}

/// Whether the step that has made the `made`-th locust (1-based, the counter after `++`) arms the next one: `made < count`.
pub fn locust_continues(made: i32, count: i32) -> bool {
    made < count
}

/// One Locust Swarm swarm in progress: the original makes ONE locust per step (`0x6F12CE70`, a unit of `UnitID` at the caster
/// for the caster's owner), the first at once and then one every `DataB` seconds (message `0xD01C1`; stock 0.2 s) until the
/// count is reached; the swarm ends `Dur + (ability +0x9C)` seconds after the cast (message `0xD01C4`, `0x6F12F1B0(1)`
/// releases every locust). Each locust's own ability `Aloc` gets `(Area, f2i(DataC), DataD, DataE)` (`0x6F124EF0`).
#[derive(Debug, Clone, Copy)]
struct Locust {
    caster: ObjKey,
    level: SpellLevel,
    count: i32,
    spawned: i32,
    unit_type: u32,
    /// The time left of the swarm when the next locust is made.
    remaining: F32,
}

/// Locust Swarm (`CAbilityLocustSwarm`, `0x6F8DDA84`): V7 fixed the model (it made `f2i(DataC)` locusts at once and ignored
/// `DataA`/`DataB`). The life steal (`DataD`, `DataE`) and the release of the locusts at the end need per-attack hooks and are
/// not modelled (UNSURE: the model gives each locust a timed life of the swarm time left).
fn locust_swarm(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let unit_type = if rec.level.unit_id != 0 {
        rec.level.unit_id
    } else {
        gm.spells.hero.extra.extra_unit.get(&rec.base).copied().unwrap_or(0)
    };
    if unit_type == 0 || !gm.alive(rec.caster) {
        return;
    }
    gm.spells.hero.extra.next += 1;
    let id = gm.spells.hero.extra.next;
    let swarm = Locust {
        caster: rec.caster,
        level: rec.level,
        count: locust_plan(&rec.level, F32::ZERO).count,
        spawned: 0,
        unit_type,
        remaining: locust_plan(&rec.level, F32::ZERO).total,
    };
    gm.spells.hero.extra.locusts.insert(id, swarm);
    locust_step(&mut gm, id);
}

/// `0x6F12CE70`: makes the next locust and, while `spawned < count`, arms the next step `DataB` seconds away.
fn locust_step(gm: &mut Game, id: u32) {
    let Some(mut sw) = gm.spells.hero.extra.locusts.get(&id).copied() else {
        return;
    };
    if !gm.alive(sw.caster) || sw.spawned >= sw.count {
        gm.spells.hero.extra.locusts.remove(&id);
        return;
    }
    let (x, y, owner, facing) = {
        let c = gm.unit(sw.caster);
        (c.x, c.y, c.owner, c.facing)
    };
    if let Some((k, _)) = crate::natives::unit::build(gm, owner, sw.unit_type, x, y, facing, true) {
        gm.unit_mut(k).summoned = true;
        if arith::lt(F32::ZERO, sw.remaining) {
            gm.apply_timed_life(k, BUFF_LOCUST, sw.remaining);
        }
    }
    sw.spawned += 1;
    let interval = sw.level.data[1];
    if sw.spawned < sw.count {
        sw.remaining = arith::sub(sw.remaining, interval);
        gm.spells.hero.extra.locusts.insert(id, sw);
        gm.timers_start_hero(interval.to_f32().max(0.0), TICK_CHANNEL, sw.caster, id);
    } else {
        gm.spells.hero.extra.locusts.remove(&id);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Animate Dead (`CAbilityAnimateDead`, `0x6F90CC84`; the creep form `CAbilityCreepAnimateDead` derives from it).

/// Corpses within `Area` of the caster, nearest first (UNSURE: the original orders its corpse group through the group
/// builder; ties break by object index). Heroes, structures, mechanical units and illusions are excluded.
fn corpses_near(gm: &Game, x: F32, y: F32, area: F32) -> Vec<ObjKey> {
    let mut v: Vec<(F32, ObjKey)> = Vec::new();
    for k in circle_candidates(gm, x, y, area) {
        let u = gm.unit(k);
        if !u.dead || u.removed || u.illusion || gm.target_hero(k) {
            continue;
        }
        if !organic_ground_or_air(gm, k) {
            continue;
        }
        let (dx, dy) = (arith::sub(u.x, x), arith::sub(u.y, y));
        v.push((arith::add(arith::mul(dx, dx), arith::mul(dy, dy)), k));
    }
    v.sort_by(|a, b| {
        a.0.to_f32().partial_cmp(&b.0.to_f32()).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1))
    });
    v.into_iter().map(|(_, k)| k).collect()
}

/// Raises up to `f2i(DataA)` corpses of the area as new units of the same type under the caster's player, for the level's
/// `Dur` (`BUan` timed life). LIKELY from the data (`DataA` 6, `Dur` 40 / 120, `Area` 900). `DataB` = 1 probably marks
/// the raised units invulnerable (UNSURE, not modelled).
fn animate_dead(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let (cx, cy, owner) = {
        let c = gm.unit(rec.caster);
        (c.x, c.y, c.owner)
    };
    let max = conv::f2i(rec.level.data[0]).max(0) as usize;
    let dur = arith::add(rec.level.dur, F32::ZERO);
    for corpse in corpses_near(&gm, cx, cy, rec.level.area).into_iter().take(max) {
        let (ty, x, y, facing) = {
            let u = gm.unit(corpse);
            (u.type_id, u.x, u.y, u.facing)
        };
        crate::natives::unit::remove_unit(&mut gm, corpse);
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, ty, x, y, facing, true) {
            gm.unit_mut(k).summoned = true;
            if arith::lt(F32::ZERO, dur) {
                gm.apply_timed_life(k, BUFF_ANIMATE_DEAD, dur);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Carrion Scarabs (`CAbilityCarrionScarabs`, `0x6F8CBD34`, a `CAbilityRaiseDead`): like Raise Dead the effect takes ONE
// corpse (the target), creates `f2i(DataA)` units of the level's scarab type (`DataC` rawcode `ucs1`..`ucs3`) at it and
// removes the corpse. The scarabs are permanent (no `Dur` in the data). UNSURE: `DataE` (5) is not modelled.

fn carrion_scarabs(game: &RefCell<Game>, rec: &CastRec, corpse: ObjKey) {
    let mut gm = game.borrow_mut();
    let unit_type = if rec.level.unit_id != 0 {
        rec.level.unit_id
    } else {
        gm.spells.hero.extra.extra_unit.get(&CARRION_SCARABS).copied().unwrap_or(0)
    };
    {
        let u = gm.unit(corpse);
        if !u.dead || u.removed || unit_type == 0 {
            return;
        }
    }
    let n = conv::f2i(rec.level.data[0]).max(1);
    let owner = gm.unit(rec.caster).owner;
    let (x, y, facing) = {
        let u = gm.unit(corpse);
        (u.x, u.y, u.facing)
    };
    crate::natives::unit::remove_unit(&mut gm, corpse);
    for _ in 0..n {
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, unit_type, x, y, facing, true) {
            gm.unit_mut(k).summoned = true;
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Dark Summoning (`CAbilityDarkSummoning`, `0x6F88C32C`, an item-class `CAbilitySimpleSpell`, targs `player`): up to
// `f2i(DataA)` of the caster's own units within `Area` of the target unit are moved next to the caster (UNSURE: whether
// the target is a unit or a point; the data says a player-owned unit).

fn dark_summoning(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) || !gm.alive(rec.caster) {
        return;
    }
    let (tx, ty, owner) = {
        let t = gm.unit(target);
        (t.x, t.y, gm.unit(rec.caster).owner)
    };
    let (cx, cy) = (gm.unit(rec.caster).x, gm.unit(rec.caster).y);
    let max = conv::f2i(rec.level.data[0]).max(0) as usize;
    let mut moved = 0usize;
    for k in circle_candidates(&gm, tx, ty, rec.level.area) {
        if moved >= max {
            break;
        }
        if k == rec.caster || !gm.alive(k) || gm.unit(k).owner != owner {
            continue;
        }
        if gm.unit_types().is_building(gm.unit(k).type_id) {
            continue;
        }
        let (px, py) = gm.clamp_unit_move(cx, cy);
        gm.store_unit_position(k, px, py);
        moved += 1;
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Creep Devour (`CAbilityCreepDevour`, `0x6F8A571C`, a `CAbilityDevour`): the target (ground, non-hero, organic, enemy) is
// swallowed (`Bdvv`, disabled) and digested (`Bdig`); `DataA` = 5 is the digest time after which it dies (LIKELY from the
// Kodo Beast form of the class; the digest damage per second of the Kodo's `DataB` does not exist in this row).

fn devour(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) || !gm.alive(rec.caster) || gm.target_hero(target) {
        return;
    }
    let serial = gm.add_spell_buff(target, BUFF_DEVOURED, BuffEffect::Stun, rec.level.data[0]);
    gm.spells.hero.extra.next += 1;
    let id = gm.spells.hero.extra.next;
    gm.spells.hero.extra.devours.insert(id, Devour { caster: rec.caster, target, serial });
    gm.timers_start_hero(rec.level.data[0].to_f32().max(0.0), TICK_DEVOUR_END, target, id);
}

fn devour_end(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let (caster, dead) = {
        let mut gm = game.borrow_mut();
        let Some(d) = gm.spells.hero.extra.devours.remove(&id) else {
            return;
        };
        gm.drop_spell_buff(d.serial);
        // The devourer must still be alive to finish the digestion; a dead one releases the target.
        if !gm.alive(d.caster) || !gm.alive(d.target) {
            return;
        }
        let died = set_life(&mut gm, d.target, F32::ZERO);
        (d.caster, died.then_some(d.target))
    };
    dispatch(game, rt, caster, dead.into_iter().collect());
}

// ---------------------------------------------------------------------------------------------------------------
// Channelled: Starfall (`CAbilityStarfall`, `0x6F8EFFB4`, derived from `CAbilityTranquility`) and Tranquility
// (`CAbilityTranquility`, `0x6F8EFBC4`, a `CAbilitySpell`). Both are immediate, have a `0x428`-style effect object
// (`XEsf`/`XEtq`) and a channel time; the shared lifecycle of `cast.rs` provides the channel timer and the end hook.
// Model: a pulse every `DataB` seconds (Starfall 1.5 s, Tranquility 1.0 s) over `Area` around the caster until the
// channel ends. Starfall hurts every enemy (structures included) for `DataA`; Tranquility heals every friendly unit in the
// area (the caster included) for `DataA`. UNSURE: the stars of the original fall one per target with `DataC` as the
// damage area; the pulse timing (first pulse after one interval) is a stand-in.

fn channel_start(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    if !gm.alive(rec.caster) {
        return;
    }
    let interval = rec.level.data[1].to_f32();
    gm.spells.hero.extra.channels.insert(
        rec.id,
        Channel {
            caster: rec.caster,
            base: rec.base,
            level: rec.level,
        },
    );
    gm.timers_start_hero(if interval > 0.0 { interval } else { 1.0 }, TICK_CHANNEL, rec.caster, rec.id);
}

/// The channel-end hook (`0x388`): the effect object is destroyed, so the pulses stop.
pub(crate) fn on_cast_end(game: &RefCell<Game>, rec: &CastRec) {
    game.borrow_mut().spells.hero.extra.channels.remove(&rec.id);
}

fn channel_pulse(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let mut dead = Vec::new();
    let caster;
    {
        let mut gm = game.borrow_mut();
        let Some(ch) = gm.spells.hero.extra.channels.get(&id).copied() else {
            return;
        };
        caster = ch.caster;
        if !gm.alive(ch.caster) {
            gm.spells.hero.extra.channels.remove(&id);
            return;
        }
        let (cx, cy, owner) = {
            let c = gm.unit(ch.caster);
            (c.x, c.y, c.owner)
        };
        let units = circle_candidates(&gm, cx, cy, ch.level.area);
        let mut hit = 0usize;
        for k in units {
            if !gm.alive(k) {
                continue;
            }
            let o = gm.unit(k).owner;
            if ch.base == STARFALL {
                if gm.is_enemy(owner, o) && !gm.unit_is_invulnerable(k) {
                    hit += 1;
                    if let Some(d) = gm.unit_damage_target(ch.caster, k, ch.level.data[0], false, false, 0, DT_MAGIC) {
                        dead.push(d);
                    }
                }
            } else if !gm.is_enemy(owner, o) {
                hit += 1;
                heal(&mut gm, k, ch.level.data[0]);
            }
        }
        gm.spells.hero.extra.pulses.push((ch.caster, ch.base, hit));
        let interval = ch.level.data[1].to_f32();
        gm.timers_start_hero(if interval > 0.0 { interval } else { 1.0 }, TICK_CHANNEL, ch.caster, id);
    }
    dispatch(game, rt, caster, dead);
}

fn heal(gm: &mut Game, unit: ObjKey, amount: F32) {
    let u = gm.unit(unit);
    let sum = arith::add(u.life, amount);
    let next = if arith::lt(u.max_life, sum) { u.max_life } else { sum };
    set_life(gm, unit, next);
}

// ---------------------------------------------------------------------------------------------------------------
// The on-hit orbs: Cold Arrows (`CAbilityColdArrows`, `0x6F913ADC`), Searing / Flaming Arrows (`CAbilityFlamingArrows`,
// `0x6F8AE384`) and Poison Arrows (`CAbilityPoisonArrows`, `0x6F8E118C`), all `CAbilityRangerArrow` (an autocast orb:
// order ids on/off/manual). Model (UNSURE where marked): the orb is always on once learned (the autocast toggle orders
// are not wired); each hit with enough mana pays the level's `Cost`, then adds the bonus `DataA` as spell damage of the
// orb's damage type (the original adds it to the attack's damage record) and applies the orb's buff:
// * Cold Arrows: the `BHca` slow, movement `-DataB`, attack speed `-DataC`, for `HeroDur` on heroes else `Dur`;
//   targs `air,ground,enemy,neutral`.
// * Searing Arrows: only the bonus damage; targs include structures.
// * Poison Arrows (the data row is the creep one): bonus `DataA`, and a poison of `DataB` damage per second for the hero
//   rule duration (LIKELY from the family pattern: 10/20/30 and 4/6/8); targs `air,ground,organic`.

impl Game {
    /// Called by the attack path after a hit that left the victim alive; returns the units that died of the orb damage.
    pub fn hero_orb_on_hit(&mut self, attacker: ObjKey, target: ObjKey) -> Vec<ObjKey> {
        let mut died = Vec::new();
        for base in [SEARING_ARROWS, COLD_ARROWS, POISON_ARROWS] {
            if !self.alive(target) {
                break;
            }
            if self.unit(attacker).abilities.get(base).is_none_or(|a| a.is_removed()) {
                continue;
            }
            let Some(level) = self.ability_level_record(attacker, base) else {
                continue;
            };
            let words = match base {
                SEARING_ARROWS => "air,ground,structure,enemy,neutral",
                COLD_ARROWS => "air,ground,enemy,neutral",
                _ => "air,ground,organic",
            };
            if !crate::unit_spells::targs_ok(self, attacker, target, crate::unit_spells::targs_word(words)) {
                continue;
            }
            let cost = level.cost;
            if cost > 0 {
                let mana = self.unit(attacker).mana;
                let c = conv::i2r(cost);
                if arith::lt(mana, c) {
                    continue;
                }
                self.write_mana(attacker, arith::sub(mana, c));
            }
            let dtype = match base {
                SEARING_ARROWS => DT_FIRE,
                COLD_ARROWS => DT_COLD,
                _ => DT_POISON,
            };
            if arith::lt(F32::ZERO, level.data[0]) {
                if let Some(d) = self.unit_damage_target(attacker, target, level.data[0], false, false, 0, dtype) {
                    died.push(d);
                    continue;
                }
            }
            let hero = self.target_hero(target);
            let dur = if hero { level.hero_dur } else { level.dur };
            if !arith::lt(F32::ZERO, dur) {
                continue;
            }
            match base {
                COLD_ARROWS => {
                    let mods = crate::unit_spells::UnitMods {
                        move_delta: arith::neg(level.data[1]),
                        attack_delta: arith::neg(level.data[2]),
                        ..crate::unit_spells::UnitMods::default()
                    };
                    let preds = crate::abilities::BuffPredicates {
                        negative: true,
                        ..crate::abilities::buff_predicates(BUFF_COLD_ARROWS)
                    };
                    self.apply_mods_buff(target, BUFF_COLD_ARROWS, preds, mods, dur);
                }
                POISON_ARROWS => {
                    let preds = crate::abilities::BuffPredicates {
                        negative: true,
                        physical: true,
                        ..crate::abilities::buff_predicates(BUFF_POISON_ARROWS)
                    };
                    let serial = self.apply_mods_buff(
                        target,
                        BUFF_POISON_ARROWS,
                        preds,
                        crate::unit_spells::UnitMods::default(),
                        dur,
                    );
                    self.register_tick(target, serial, attacker, level.data[1], false, DT_POISON, true);
                }
                _ => {}
            }
        }
        died
    }
}

/// A due module timer of the kinds [`TICK_META_START`], [`TICK_CHANNEL`], [`TICK_DEVOUR_END`].
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, _unit: ObjKey, aux: u32) {
    match kind {
        TICK_META_START => meta_start(game, aux),
        TICK_CHANNEL => {
            let swarm = game.borrow().spells.hero.extra.locusts.contains_key(&aux);
            if swarm {
                locust_step(&mut game.borrow_mut(), aux);
            } else {
                channel_pulse(game, rt, aux)
            }
        }
        TICK_DEVOUR_END => devour_end(game, rt, aux),
        _ => {}
    }
}
