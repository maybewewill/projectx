//! Stampede and Volcano (S50), the two long-running area effects of the neutral heroes. Spec:
//! `docs/spec/abilities-neutral.md` section 8. They use the wave helpers of `neutral_waves.rs` (damage type, marker rule).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32};

use crate::abilities::code;
use crate::cast::{CastRec, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::neutral_heroes2::start_timer;
use crate::neutral_waves::{wave_marker, WAVE_DAMAGE_TYPE};
use crate::unit_spells::targs_word;

/// Beastmaster: Stampede (`CAbilityStampede < CAbilitySimpleSpell`, buff `CBuffStampede`) and Firelord: Volcano
/// (`CAbilityVolcano < CAbilitySimpleSpell`, effect `CEffectVolcano`, marker buff `CBuffVolcanoAoe`).
pub const STAMPEDE: u32 = code(b"ANst");
pub const VOLCANO: u32 = code(b"ANvc");
pub const BUFF_STAMPEDE: u32 = code(b"BNst");
pub const BUFF_VOLCANO: u32 = code(b"BNva");
pub const ORDER_STAMPEDE: u32 = 852_593;
pub const ORDER_VOLCANO: u32 = 852_669;

/// Timer kinds (`aux` is the effect id): the Stampede spawn tick, a lizard explosion, a Volcano wave.
pub const KIND_STAMPEDE: u8 = 0x61;
pub const KIND_BLAST: u8 = 0x62;
pub const KIND_VOLCANO: u8 = 0x63;

const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);
const HALF: F32 = F32::from_bits(0x3F00_0000);

/// A running Stampede: the spawn tick of the channel (`CAbilityStampede` timers `+304` / `+324`).
#[derive(Debug, Clone, Copy)]
struct StampedeFx {
    cast: u32,
    caster: ObjKey,
    x: F32,
    y: F32,
    level: SpellLevel,
    interval: F32,
}

/// A falling lizard: explodes at its point after `DataE` seconds.
#[derive(Debug, Clone, Copy)]
struct Blast {
    caster: ObjKey,
    x: F32,
    y: F32,
    level: SpellLevel,
}

/// A Volcano: `left` waves still to come, one every `DataC` seconds.
#[derive(Debug, Clone, Copy)]
struct VolcanoFx {
    caster: ObjKey,
    x: F32,
    y: F32,
    level: SpellLevel,
    left: i32,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    stampedes: BTreeMap<u32, StampedeFx>,
    blasts: BTreeMap<u32, Blast>,
    volcanoes: BTreeMap<u32, VolcanoFx>,
}

pub(crate) fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    ids.insert(ORDER_STAMPEDE, STAMPEDE);
    ids.insert(ORDER_VOLCANO, VOLCANO);
}

pub(crate) fn is_rain_spell(base: u32) -> bool {
    base == STAMPEDE || base == VOLCANO
}

/// The Stampede channel runs for the level's `HeroDur` (the caster is a hero; `Dur` is equal in the data).
pub(crate) fn channel_time(base: u32, level: &SpellLevel) -> Option<F32> {
    (base == STAMPEDE).then_some(level.hero_dur)
}

/// The channel end slot of Stampede (`0x6F1DF720`, CONFIRMED) destroys its spawn timers however the cast ends (lizards
/// already falling still explode).
pub(crate) fn on_cast_end(game: &RefCell<Game>, rec: &CastRec) {
    game.borrow_mut().spells.neutral.h2.rain.stampedes.retain(|_, s| s.cast != rec.id);
}

pub(crate) fn start(game: &RefCell<Game>, rec: &CastRec) {
    match rec.base {
        STAMPEDE => start_stampede(game, rec),
        VOLCANO => start_volcano(game, rec),
        _ => {}
    }
}

pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, aux: u32) {
    match kind {
        KIND_STAMPEDE => stampede_tick(game, aux),
        KIND_BLAST => blast(game, rt, aux),
        KIND_VOLCANO => volcano_wave(game, rt, aux),
        _ => {}
    }
}

fn point_of(gm: &Game, rec: &CastRec) -> Option<(F32, F32)> {
    match rec.target {
        CastTarget::Point(x, y) => Some((x, y)),
        CastTarget::Unit(t) => Some((gm.unit(t).x, gm.unit(t).y)),
        CastTarget::None => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Stampede

/// Stampede (`ANst`, order 852593; labels `NST1..5`): the Beastmaster channels for `Dur`; the target point is the centre
/// of the spawn disc (`Area`). The class slot `0x418` (`0x6F1FCE20`, CONFIRMED structure) turns the caster to the point,
/// strips three buffs and fires the cast events; the channel end slot `0x388` (`0x6F1DF720`, CONFIRMED) destroys the
/// spawn timers. Model: a spawn tick every `1 / DataA` seconds ("Beasts Per Second"), each tick drops one lizard at a
/// random point of the disc (two draws of stream 4: the angle `2 pi U` and the radius `Area * U`, UNSURE), which explodes
/// `DataE` seconds later (`DataC` damage to the enemy and neutral ground units and structures within `DataD`, "Damage
/// Amount / Radius / Delay"; the beast collision radius `DataB` has no simulation effect).
fn start_stampede(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let Some((x, y)) = point_of(&gm, rec) else {
        return;
    };
    if !arith::lt(F32::ZERO, rec.level.data[0]) {
        return;
    }
    let interval = gm.soft_div(F32::ONE, rec.level.data[0]);
    let id = gm.neutral2_next_id();
    gm.spells.neutral.h2.rain.stampedes.insert(id, StampedeFx { cast: rec.id, caster: rec.caster, x, y, level: rec.level, interval });
    start_timer(&mut gm, interval.to_f32(), rec.caster, STAMPEDE, id, KIND_STAMPEDE);
}

fn stampede_tick(game: &RefCell<Game>, id: u32) {
    let mut gm = game.borrow_mut();
    let Some(s) = gm.spells.neutral.h2.rain.stampedes.get(&id).copied() else {
        return;
    };
    if !gm.alive(s.caster) {
        gm.spells.neutral.h2.rain.stampedes.remove(&id);
        return;
    }
    // The landing point.
    let tables = gm.order_tables().cloned();
    let mut point = (s.x, s.y);
    if let Some(tables) = tables {
        if let Some((table, streams)) = gm.rng_parts() {
            let table = table.clone();
            let rng = streams.stream_mut(4);
            let u1 = rng.random_fraction(&table);
            let u2 = rng.random_fraction(&table);
            let angle = arith::mul(TWO_PI, u1);
            let r = arith::mul(s.level.area, u2);
            point = (
                arith::add(s.x, arith::mul(r, softfloat::math::cos(angle, &tables))),
                arith::add(s.y, arith::mul(r, softfloat::math::sin(angle, &tables))),
            );
        }
    }
    let bid = gm.neutral2_next_id();
    gm.spells.neutral.h2.rain.blasts.insert(bid, Blast { caster: s.caster, x: point.0, y: point.1, level: s.level });
    let delay = s.level.data[4];
    start_timer(&mut gm, if arith::lt(F32::ZERO, delay) { delay.to_f32() } else { 0.0 }, s.caster, STAMPEDE, bid, KIND_BLAST);
    start_timer(&mut gm, s.interval.to_f32(), s.caster, STAMPEDE, id, KIND_STAMPEDE);
}

/// A lizard explodes: `DataC` damage (damage type 15) to every enemy or neutral ground unit or structure within `DataD`.
fn blast(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(b) = game.borrow_mut().spells.neutral.h2.rain.blasts.remove(&id) else {
        return;
    };
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let word = targs_word("ground,structure,enemy,neutral");
        for k in crate::natives::group::circle_candidates(&gm, b.x, b.y, b.level.data[3]) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden || u.dead {
                    continue;
                }
            }
            if !crate::orc::area_predicate(&gm, b.caster, k, word) {
                continue;
            }
            if let Some(d) = gm.unit_damage_target(b.caster, k, b.level.data[2], false, false, 0, WAVE_DAMAGE_TYPE) {
                killed.push(d);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(b.caster));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Volcano

/// Volcano (`ANvc`, order 852669; labels `NVC1..6`): a volcano rises at the point and throws `DataB` waves, one every
/// `DataC` seconds (the first at once; tooltip: 8 waves of 5 seconds = "Lasts 35 seconds"). A wave hits every
/// ground unit and structure within `Area`: `DataE` damage inside half the area, `DataE * DataF` between (the "Full
/// Damage Amount" and "Half Damage Factor" of the editor; the ring split at half the area is UNSURE, `DataA` = 3 rock
/// rings is presentation), structures take `DataD` times as much, and a surviving non-structure is stunned for `Dur`
/// (`HeroDur` for heroes). The `CBuffVolcanoAoe` marker (`BNva`) keeps a unit from being hit twice by overlapping
/// volcanoes in one interval, as for Blizzard. The destructible `Volc` is not placed (no pathing change).
fn start_volcano(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let Some((x, y)) = point_of(&gm, rec) else {
        return;
    };
    let left = conv::f2i(rec.level.data[1]);
    if left <= 0 {
        return;
    }
    let id = gm.neutral2_next_id();
    gm.spells.neutral.h2.rain.volcanoes.insert(id, VolcanoFx { caster: rec.caster, x, y, level: rec.level, left });
    // The first wave is on the next dispatch.
    start_timer(&mut gm, 0.0, rec.caster, VOLCANO, id, KIND_VOLCANO);
}

fn volcano_wave(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let Some(v) = game.borrow().spells.neutral.h2.rain.volcanoes.get(&id).copied() else {
        return;
    };
    let word = targs_word("ground,structure,enemy,neutral");
    let half_area = arith::mul(v.level.area, HALF);
    let (victims, inner): (Vec<ObjKey>, Vec<ObjKey>) = {
        let gm = game.borrow();
        let ok = |k: &ObjKey| {
            let u = gm.unit(*k);
            !u.removed && !u.hidden && !u.dead && crate::orc::area_predicate(&gm, v.caster, *k, word)
        };
        let all: Vec<ObjKey> = crate::natives::group::circle_candidates(&gm, v.x, v.y, v.level.area).into_iter().filter(ok).collect();
        let inner: Vec<ObjKey> =
            crate::natives::group::circle_candidates(&gm, v.x, v.y, half_area).into_iter().filter(|k| all.contains(k)).collect();
        (all, inner)
    };
    let full = v.level.data[4];
    let half = arith::mul(full, v.level.data[5]);
    let interval = v.level.data[2];
    let mut killed = Vec::new();
    for k in victims {
        let (structure, hero) = {
            let gm = game.borrow();
            if !gm.alive(k) {
                continue;
            }
            (gm.unit_types().is_building(gm.unit(k).type_id), gm.target_hero(k))
        };
        let base = if inner.contains(&k) { full } else { half };
        let amount = if structure { arith::mul(v.level.data[3], base) } else { base };
        if !wave_marker(game, k, BUFF_VOLCANO, amount, interval) {
            continue;
        }
        let mut gm = game.borrow_mut();
        if let Some(d) = gm.unit_damage_target(v.caster, k, amount, false, false, 0, WAVE_DAMAGE_TYPE) {
            killed.push(d);
            continue;
        }
        let dur = crate::cast::effect_duration(&v.level, hero, F32::ZERO);
        if !structure && arith::lt(F32::ZERO, dur) {
            gm.apply_stun(k, dur);
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(v.caster));
    }
    let mut gm = game.borrow_mut();
    let again = match gm.spells.neutral.h2.rain.volcanoes.get_mut(&id) {
        Some(e) => {
            e.left -= 1;
            e.left > 0
        }
        None => false,
    };
    if again {
        start_timer(&mut gm, interval.to_f32(), v.caster, VOLCANO, id, KIND_VOLCANO);
    } else {
        gm.spells.neutral.h2.rain.volcanoes.remove(&id);
    }
}
