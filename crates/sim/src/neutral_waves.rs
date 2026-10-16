//! Periodic area effects of the neutral heroes (S50): Cluster Rockets, Healing Spray (the `CAbilityClusterRockets`
//! family), later Stampede and Volcano. Spec: `docs/spec/abilities-neutral.md` section 8.
//!
//! The family shares one effect lifecycle: the cast makes an effect object that fires a wave every `DataB` seconds and
//! applies one amount to every unit of the area through the marker rule of the `CBuffAoe` buffs (the structure of
//! Blizzard, whose level layout `Area` / `DataA` damage / `DataD` cap / `DataE` building factor it shares).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32};

use crate::abilities::code;
use crate::cast::{CastRec, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::neutral_heroes2::start_timer;
use crate::unit_spells::targs_word;

/// Tinker: Cluster Rockets (`CAbilityClusterRockets < CAbilitySimpleSpell`, effect `XNcs`, marker buff `CBuffClusterRockets
/// < CBuffAoe`) and Alchemist: Healing Spray (`CAbilityHealingSpray < CAbilityClusterRockets`, effect `XNhs`, marker buff
/// `CBuffHealingSpray`).
pub const CLUSTER_ROCKETS: u32 = code(b"ANcs");
pub const HEALING_SPRAY: u32 = code(b"ANhs");
pub const BUFF_CLUSTER: u32 = code(b"BNcs");
pub const BUFF_SPRAY: u32 = code(b"BNhs");
/// Naga Sea Witch: Monsoon (`CAbilityMonsoon < CAbilitySimpleSpell`, effect `XNmo`): the Starfall field set (`ESF1..3`) on a
/// target point.
pub const MONSOON: u32 = code(b"ANmo");
pub const ORDER_MONSOON: u32 = 852_591;

/// Cluster Rockets and Healing Spray orders (slot `0x348`).
pub const ORDER_CLUSTER_ROCKETS: u32 = 852_652;
pub const ORDER_HEALING_SPRAY: u32 = 852_664;

/// Timer kind of the wave effects (`aux` is the effect id).
pub const KIND_WAVE: u8 = 0x60;

/// The damage type of the wave damage of Cluster Rockets (the neutral spell type; the effect's class word was not read,
/// UNSURE).
pub(crate) const WAVE_DAMAGE_TYPE: u32 = 15;

/// A wave effect of Cluster Rockets or Healing Spray (the effect objects `XNcs` / `XNhs`).
#[derive(Debug, Clone, Copy)]
struct WaveFx {
    /// The cast that made it (an interrupt of the channel destroys it).
    cast: u32,
    caster: ObjKey,
    base: u32,
    x: F32,
    y: F32,
    level: SpellLevel,
    left: i32,
    interval: F32,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    fx: BTreeMap<u32, WaveFx>,
    /// The marker buffs of the wave effects: `(unit, buff code) -> (serial, value)`.
    markers: BTreeMap<(ObjKey, u32), (u32, F32)>,
}

pub(crate) fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    ids.insert(ORDER_CLUSTER_ROCKETS, CLUSTER_ROCKETS);
    ids.insert(ORDER_HEALING_SPRAY, HEALING_SPRAY);
    ids.insert(ORDER_MONSOON, MONSOON);
}

pub(crate) fn is_wave_spell(base: u32) -> bool {
    base == CLUSTER_ROCKETS || base == HEALING_SPRAY || base == MONSOON
}

/// The wave count of the class (slot `0x430`): Cluster Rockets `0x6F161280` = `I2R(f2i(DataF / DataB))` (the effect
/// duration over the wave interval, the soft divide `0x6F6EFE10`), Healing Spray `0x6F19E680` = the `DataF` getter
/// (`+0x34`, the editor's "Wave Count"). The operand order of the divide is read from the shape of the code (LIKELY).
pub fn wave_count(gm: &Game, base: u32, level: &SpellLevel) -> i32 {
    if base == HEALING_SPRAY {
        return conv::f2i(level.data[5]);
    }
    if base == MONSOON {
        // The storm lasts `HeroDur` (the caster is a hero) and strikes every `DataB` seconds.
        if !arith::lt(F32::ZERO, level.data[1]) {
            return 0;
        }
        return conv::f2i(gm.soft_div(level.hero_dur, level.data[1]));
    }
    if !arith::lt(F32::ZERO, level.data[1]) {
        return 0;
    }
    conv::f2i(gm.soft_div(level.data[5], level.data[1]))
}

/// The effect lifetime of Healing Spray (slot `0x434` = `0x6F194AD0`): `DataB * waves + 0.01` (the constant
/// `0x6FAAE480`). The channel time of the cast.
fn spray_duration(level: &SpellLevel) -> F32 {
    let waves = conv::i2r(conv::f2i(level.data[5]));
    arith::add(arith::mul(level.data[1], waves), F32::from_bits(0x3C23_D70A))
}

/// The channel time of the wave spells: the lifetime of the effect (slot `0x434`: Cluster Rockets `DataF`, Healing Spray
/// as above).
pub(crate) fn channel_time(base: u32, level: &SpellLevel) -> Option<F32> {
    match base {
        CLUSTER_ROCKETS => Some(level.data[5]),
        HEALING_SPRAY => Some(spray_duration(level)),
        MONSOON => Some(level.hero_dur),
        _ => None,
    }
}

/// An interrupt of the channel destroys the effects of the cast.
pub(crate) fn on_cast_end(game: &RefCell<Game>, rec: &CastRec, completed: bool) {
    if completed {
        return;
    }
    game.borrow_mut().spells.neutral.h2.waves.fx.retain(|_, w| w.cast != rec.id);
}

/// The setup of `XNcs` / `XNhs` (`0x6F18B150`, shared by both classes): the effect gets the area, `DataA` (damage or
/// heal), `DataD` (the cap), `DataE` (the building factor), the wave count and the wave interval `DataB`. The first wave
/// runs one interval after the cast (UNSURE: the timer's first dispatch), the rest every interval.
pub(crate) fn start(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (x, y) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => return,
    };
    let left = wave_count(&gm, rec.base, &rec.level);
    if left <= 0 || !arith::lt(F32::ZERO, rec.level.data[1]) {
        return;
    }
    let id = gm.neutral2_next_id();
    let fx = WaveFx { cast: rec.id, caster: rec.caster, base: rec.base, x, y, level: rec.level, left, interval: rec.level.data[1] };
    gm.spells.neutral.h2.waves.fx.insert(id, fx);
    start_timer(&mut gm, fx.interval.to_f32(), rec.caster, rec.base, id, KIND_WAVE);
}

pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, aux: u32) {
    if kind != KIND_WAVE {
        return;
    }
    let Some(w) = game.borrow().spells.neutral.h2.waves.fx.get(&aux).copied() else {
        return;
    };
    wave(game, rt, aux, w);
}

/// One wave. Cluster Rockets: every enemy or neutral ground unit or structure within `Area` of the point takes `DataA`
/// (capped so that the whole wave deals at most `DataD`, structures take `DataE` times as much; the shared formula
/// `human_heroes::blizzard_unit_damage`, Unicorn-checked for Blizzard, whose level layout is the same) and is stunned for
/// `Dur` / `HeroDur`. Healing Spray: the same cap and building factor with the sign flipped (the effect's heal flag,
/// slot `0x8C`), applied to the friendly organic units. A unit that carries the marker of an earlier wave of this
/// interval takes nothing (the `CBuffAoe` marker, "strongest wins").
fn wave(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32, w: WaveFx) {
    if !game.borrow().alive(w.caster) {
        game.borrow_mut().spells.neutral.h2.waves.fx.remove(&id);
        return;
    }
    let heal = w.base == HEALING_SPRAY;
    let monsoon = w.base == MONSOON;
    let (targets, tables) = {
        let gm = game.borrow();
        let word = if heal {
            targs_word("ground,air,friend,organic")
        } else if monsoon {
            targs_word("air,ground,structure,enemy,neutral")
        } else {
            targs_word("ground,enemy,neutral,structure")
        };
        let t: Vec<ObjKey> = crate::natives::group::circle_candidates(&gm, w.x, w.y, w.level.area)
            .into_iter()
            .filter(|&k| {
                let u = gm.unit(k);
                !u.removed && !u.hidden && !u.dead && crate::orc::area_predicate(&gm, w.caster, k, word)
            })
            .collect();
        (t, gm.order_tables().cloned())
    };
    let params = crate::human_heroes::BlizzardParams {
        waves: 0,
        interval: w.interval,
        damage: w.level.data[0],
        cap: w.level.data[3],
        shards: 1,
        building: w.level.data[4],
        data_e: F32::ZERO,
        dur: w.level.dur,
        hero_dur: w.level.hero_dur,
        area: w.level.area,
    };
    let count = targets.len() as i32;
    let marker_code = if heal { BUFF_SPRAY } else { BUFF_CLUSTER };
    let mut killed = Vec::new();
    for k in targets {
        let (structure, hero) = {
            let gm = game.borrow();
            if !gm.alive(k) {
                continue;
            }
            (gm.unit_types().is_building(gm.unit(k).type_id), gm.target_hero(k))
        };
        if monsoon {
            // Monsoon: every enemy in the area takes `DataA` each strike (structures `DataC` times as much, the editor's
            // "Building Reduction"); no marker and no stun.
            let amount = if structure { arith::mul(w.level.data[0], w.level.data[2]) } else { w.level.data[0] };
            if let Some(d) = game.borrow_mut().unit_damage_target(w.caster, k, amount, false, false, 0, WAVE_DAMAGE_TYPE) {
                killed.push(d);
            }
            continue;
        }
        let amount = crate::human_heroes::blizzard_unit_damage(&params, count, structure, heal, tables.as_ref());
        let magnitude = if heal { arith::neg(amount) } else { amount };
        if !wave_marker(game, k, marker_code, magnitude, w.interval) {
            continue;
        }
        let mut gm = game.borrow_mut();
        if heal {
            heal_unit(&mut gm, k, magnitude);
        } else {
            if let Some(d) = gm.unit_damage_target(w.caster, k, magnitude, false, false, 0, WAVE_DAMAGE_TYPE) {
                killed.push(d);
                continue;
            }
            let dur = crate::cast::effect_duration(&w.level, hero, F32::ZERO);
            if !structure && arith::lt(F32::ZERO, dur) {
                gm.apply_stun(k, dur);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(w.caster));
    }
    let mut gm = game.borrow_mut();
    let again = match gm.spells.neutral.h2.waves.fx.get_mut(&id) {
        Some(e) => {
            e.left -= 1;
            e.left > 0
        }
        None => false,
    };
    if again {
        start_timer(&mut gm, w.interval.to_f32(), w.caster, w.base, id, KIND_WAVE);
    } else {
        gm.spells.neutral.h2.waves.fx.remove(&id);
    }
}

/// `AddLife` clamped to the maximum (as `undead_ne::heal_unit`).
pub(crate) fn heal_unit(gm: &mut Game, unit: ObjKey, amount: F32) {
    let u = gm.unit(unit);
    let sum = arith::add(u.life, amount);
    let next = if arith::lt(u.max_life, sum) { u.max_life } else { sum };
    crate::natives::unit::set_life(gm, unit, next);
}

/// The marker rule of `0x6F1AF950` / `0x6F1CADD0` (shared by the `CBuffAoe` family, as for Blizzard): a unit without the
/// marker gets one that lasts `interval - 0.01` and takes the wave; a unit that has it takes none, and a larger value
/// only replaces the stored one and refreshes the duration. Returns whether the unit takes this wave.
pub(crate) fn wave_marker(game: &RefCell<Game>, unit: ObjKey, buff: u32, value: F32, interval: F32) -> bool {
    let mut gm = game.borrow_mut();
    let dur = if arith::lt(F32::ZERO, interval) {
        arith::sub(interval, F32::from_bits(0x3C23_D70A))
    } else {
        F32::ZERO
    };
    let existing = gm
        .spells
        .neutral
        .h2
        .waves
        .markers
        .get(&(unit, buff))
        .copied()
        .filter(|(serial, _)| gm.spell_buff_alive(unit, *serial));
    match existing {
        None => {
            let serial = gm.add_spell_buff(unit, buff, crate::cast::BuffEffect::Marker, dur);
            gm.spells.neutral.h2.waves.markers.insert((unit, buff), (serial, value));
            true
        }
        Some((serial, old)) => {
            if crate::human_heroes::blizzard_buff_replaces(old, value) {
                gm.drop_spell_buff(serial);
                let serial = gm.add_spell_buff(unit, buff, crate::cast::BuffEffect::Marker, dur);
                gm.spells.neutral.h2.waves.markers.insert((unit, buff), (serial, value));
            }
            false
        }
    }
}
