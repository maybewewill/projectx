//! Neutral hero and creep abilities, third batch (S50). Spec: `docs/spec/abilities-neutral.md` section 8.
//!
//! Method as in the earlier neutral batches: the class identity and the order ids come from the ability atlas and
//! `tools/analysis/ability_orders.py`, the field roles of the level data from the editor's own labels
//! (`AbilityMetaData.slk` + `WorldEditStrings.txt`, the `WESTRING_AEVAL_*` keys) and the tooltips of
//! `NeutralAbilityStrings.txt`, the structure from IDA decompiles of the class overrides and of the buff objects
//! (`CBuff*` removal / apply slots). Labels: CONFIRMED = read in the machine code, LIKELY = strong inference,
//! UNSURE = a stand-in. No new numeric kernel needed a Unicorn run (the arithmetic is soft-float multiply / add / round
//! on values read from the level record, in the order the decompile shows).
//!
//! Timers: the module owns the `SpellMsg::UnitSpell` kinds `0x60..=0x6F` (`cast.rs` routes them to [`on_timer`]).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32};

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::cast::{CastError, CastRec, CastTarget, SpellLevel, SpellMsg};
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// Alchemist: Transmute (`CAbilityTransmute < CAbilitySimpleSpell`, buff `CBuffTransmute < CBuffPause`).
pub const TRANSMUTE: u32 = code(b"ANtm");
/// Pit Lord: Doom (`CAbilityDoom < CAbilitySimpleSpell`, buff `CBuffDoom < CBuffLifeDoT`, minion buff `CBuffDoomMinion`).
pub const DOOM: u32 = code(b"ANdo");
pub const BUFF_DOOM: u32 = code(b"BNdo");
pub const BUFF_DOOM_MINION: u32 = code(b"BNdi");

/// Transmute order (slot `0x348`), Doom order.
pub const ORDER_TRANSMUTE: u32 = 852_665;
pub const ORDER_DOOM: u32 = 852_583;

/// Timer kinds of this module.
pub const KIND_FX: u8 = 0x60;

/// The over-time damage type of Doom (`CBuffLifeDoT` ticks of the neutral spells; the class word of the other neutral
/// damage over time, `neutral_spells::NEUTRAL_SPELL_DAMAGE_TYPE`).
const DOOM_DAMAGE_TYPE: u32 = 15;

/// A Doom mark: who cast it and the level numbers the death needs (`CBuffDoom` holds the summon count `+0x110`, the
/// unit id `+0x108`, the minion duration `+0x11C` and the source handle `+0xFC`, CONFIRMED by `0x6F0B6750`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct DoomMark {
    source: ObjKey,
    level: SpellLevel,
}

/// State of the batch (owned by `neutral_spells::State`).
#[derive(Debug, Clone, Default)]
pub struct State {
    doom: BTreeMap<ObjKey, DoomMark>,
    next: u32,
    pub(crate) waves: crate::neutral_waves::State,
    pub(crate) rain: crate::neutral_rain::State,
    pub(crate) forms: crate::neutral_forms::State,
    pub(crate) orbs: crate::neutral_orbs::State,
    pub(crate) misc: crate::neutral_misc::State,
}

pub(crate) fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    ids.insert(ORDER_TRANSMUTE, TRANSMUTE);
    ids.insert(ORDER_DOOM, DOOM);
    crate::neutral_waves::register_orders(ids);
    crate::neutral_rain::register_orders(ids);
    crate::neutral_forms::register_orders(ids);
    crate::neutral_orbs::register_orders(ids);
    crate::neutral_misc::register_orders(ids);
}

/// Abilities of this batch that are cast without a target.
pub(crate) fn takes_no_target(base: u32) -> bool {
    crate::neutral_forms::takes_no_target(base) || crate::neutral_misc::takes_no_target(base)
}

/// The race enum value of creeps (record `+0x1C0`).
pub const RACE_CREEPS: i32 = 8;

/// The unit level `0x6F29DCA0` answers (Unicorn `CL` rows of `tools/emu/v6_emu.py`): the type's `level`, replaced by the
/// level field of a level-changing buff when one answers yes to its slot `0x36C`; not positive gives 0; otherwise at
/// most Misc `MaxHeroLevel` for an unit whose type code starts with an uppercase letter and that is not an illusion, at
/// most `MaxUnitLevel` for every other unit.
pub fn unit_level_value(type_level: i32, buff_level: Option<i32>, upper: bool, illusion: bool, max_hero: i32, max_unit: i32) -> i32 {
    let level = buff_level.unwrap_or(type_level);
    if level <= 0 {
        return 0;
    }
    let cap = if upper && !illusion { max_hero } else { max_unit };
    level.min(cap)
}

/// `0x6F1D2630(unit, max)`, the creep level rule of the Transmute, Doom and Charm target slots (CONFIRMED, `CL` rows): the
/// target is refused when its type's race is `creeps` (8) and its unit level is above `max`. Units of any other race are
/// never refused.
pub fn creep_level_refuses(race: i32, level: i32, max: i32) -> bool {
    race == RACE_CREEPS && level > max
}

/// [`creep_level_refuses`] on a live unit. The race comes from the type's pick record when the host registered one; a sim
/// without it falls back to the old stand-in (a unit of a neutral player slot, 12 and up, is a creep).
pub(crate) fn creep_too_high(gm: &Game, target: ObjKey, max: i32) -> bool {
    let u = gm.unit(target);
    let level = gm.unit_types().get(u.type_id).map_or(0, |t| t.level);
    let cfg = gm.kill_xp_config();
    let upper = (u.type_id >> 24) as u8 >= b'A' && (u.type_id >> 24) as u8 <= b'Z';
    let level = unit_level_value(level, None, upper, u.illusion, cfg.max_unit_level, cfg.max_unit_level);
    match gm.unit_types().pick_records().iter().find(|(i, _)| *i == u.type_id) {
        Some((_, rec)) => creep_level_refuses(rec.race, level, max),
        None => u.owner >= 12 && level > max,
    }
}

/// Extra target rules (`target_error` slot `0x3F0` of the classes).
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<CastError> {
    use CastError::BadTarget;
    match base {
        TRANSMUTE => {
            // targs `air,ground,enemy,neutral,nonhero` (level 1); slot `0x3F0` = `0x6F19E7A0`: the creep level rule with
            // `f2i(DataC)` (error 51), then a summoned unit (unit `+0x248 & 4`, error 0x9F).
            let w = targs_word("air,ground,enemy,neutral,nonhero");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(BadTarget);
            }
            let max = conv::f2i(gm.spell_level(caster, TRANSMUTE).data[2]);
            (creep_too_high(gm, target, max) || gm.unit(target).summoned).then_some(BadTarget)
        }
        DOOM => {
            // targs `air,ground,nonhero,organic`; slot `0x3F0` = `0x6F089BC0`: the creep level rule with `f2i(DataD)`
            // (error 51), then a target that carries the Doom buff already (error 0xD0).
            let w = targs_word("air,ground,nonhero,organic");
            if gm.unit(target).removed || !targs_ok(gm, caster, target, w) {
                return Some(BadTarget);
            }
            let max = conv::f2i(gm.spell_level(caster, DOOM).data[3]);
            (creep_too_high(gm, target, max) || gm.neutral2_doomed(target)).then_some(BadTarget)
        }
        _ => crate::neutral_orbs::target_error(gm, caster, base, target)
            .or_else(|| crate::neutral_misc::target_error(gm, caster, base, target)),
    }
}

/// The class effects. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    match rec.base {
        TRANSMUTE => {
            if let CastTarget::Unit(t) = rec.target {
                transmute(game, rt, rec, t);
            }
            true
        }
        DOOM => {
            if let CastTarget::Unit(t) = rec.target {
                doom(game, rec, t);
            }
            true
        }
        b if crate::neutral_waves::is_wave_spell(b) => {
            crate::neutral_waves::start(game, rec);
            true
        }
        b if crate::neutral_rain::is_rain_spell(b) => {
            crate::neutral_rain::start(game, rec);
            true
        }
        b if crate::neutral_forms::is_forms_spell(b) => crate::neutral_forms::run_effect(game, rec),
        crate::neutral_orbs::BLACK_ARROW_SPELL => crate::neutral_orbs::run_effect(game, rec),
        b if crate::neutral_misc::is_misc_spell(b) => crate::neutral_misc::run_effect(game, rt, rec),
        _ => false,
    }
}

/// Timer dispatch for the kinds of this module (`aux` is the effect id).
pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, _unit: ObjKey, _base: u32, aux: u32) {
    match kind {
        crate::neutral_waves::KIND_WAVE => crate::neutral_waves::on_timer(game, rt, kind, aux),
        crate::neutral_rain::KIND_STAMPEDE..=crate::neutral_rain::KIND_VOLCANO => crate::neutral_rain::on_timer(game, rt, kind, aux),
        crate::invisibility::KIND_SCAN => crate::invisibility::on_scan(game),
        crate::wander::KIND_WANDER => crate::wander::on_timer(game, rt, _unit),
        crate::neutral_orbs::KIND_INCINERATE => crate::neutral_orbs::on_timer(game, rt, kind, aux),
        crate::neutral_misc::KIND_DRAIN..=crate::neutral_misc::KIND_PERM_IMMOLATION => {
            crate::neutral_misc::on_timer(game, rt, kind, _unit, aux)
        }
        crate::neutral_forms::KIND_SPLIT_END..=crate::neutral_forms::KIND_PASSIVE => {
            crate::neutral_forms::on_timer(game, rt, kind, _unit, _base, aux)
        }
        _ => {}
    }
}

/// Whether the `Cast` field of the class is not a cast time: Parasite stores its summoned unit duration there (the
/// editor's `NPA6`), so the cast core must not wait on it.
pub(crate) fn cast_field_is_data(base: u32) -> bool {
    base == crate::neutral_misc::PARASITE
}

/// The channel time of the classes of this batch that set one.
pub(crate) fn channel_time_override(base: u32, level: &SpellLevel) -> Option<F32> {
    crate::neutral_waves::channel_time(base, level)
        .or_else(|| crate::neutral_rain::channel_time(base, level))
        .or_else(|| crate::neutral_misc::channel_time(base, level))
}

/// The cast `rec` ended (`completed` false when an order interrupted it): the effects tied to the channel stop.
pub(crate) fn on_cast_end(game: &RefCell<Game>, rec: &CastRec, completed: bool) {
    crate::neutral_waves::on_cast_end(game, rec, completed);
    crate::neutral_rain::on_cast_end(game, rec);
}

/// A unit died (called by the death dispatch, after the unit is marked dead): the removal notifications of the buffs the
/// batch owns.
pub(crate) fn on_unit_died(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, _killer: Option<ObjKey>) {
    doom_death(game, unit);
    crate::neutral_forms::on_unit_died(game, rt, unit);
    crate::neutral_orbs::on_unit_died(game, unit);
    crate::neutral_misc::on_unit_died(game, unit);
}

/// Transmute (`ANtm`; the effect `0x6F1C18B0` puts `BNtm` on the target and the buff's action slot `0x33C` =
/// `0x6F1B59B0` runs `0x6F1B58B0`, CONFIRMED): the owner of the caster gets
/// `f2i(round(DataA * I2R(goldcost)))` gold and `f2i(round(DataB * I2R(woodcost)))` lumber (the soft multiply, then the
/// round helper `0x6F6EF330` = `floor(x + 0.5)`), the unit's life is set to zero (the life setter, no killer) and the
/// unit is flagged `0x1000` (no corpse, LIKELY). A target whose flag word `+0x5C` has bit `0x40000000` gives nothing
/// (not modelled). With `DataD` set the unit's own bounty is added too (`0x6F29DD30` / `0x6F29DDD0`); the sim has no
/// bounty values, so that part is NOT DONE.
fn transmute(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, rec: &CastRec, target: ObjKey) {
    let died = {
        let mut gm = game.borrow_mut();
        if !gm.alive(target) {
            return;
        }
        let ty = gm.unit_types().get(gm.unit(target).type_id).cloned();
        let (gold_cost, wood_cost) = ty.map_or((0, 0), |t| (t.gold_cost, t.wood_cost));
        let (gold, wood) = transmute_amounts(rec.level.data[0], rec.level.data[1], gold_cost, wood_cost, None);
        let owner = gm.unit(rec.caster).owner;
        if gold != 0 {
            gm.credit_resource(owner, crate::economy::Res::Gold, gold, crate::economy::Flags::NONE);
        }
        if wood != 0 {
            gm.credit_resource(owner, crate::economy::Res::Lumber, wood, crate::economy::Flags::NONE);
        }
        crate::natives::unit::set_life(&mut gm, target, F32::ZERO)
    };
    if died {
        dispatch_unit_death_by(game, rt, target, None);
    }
}

/// The amounts of the Transmute buff body `0x6F1B58B0` (Unicorn `TM` rows of `tools/emu/v6_emu.py`): gold =
/// `f2i(round(DataA * I2R(goldcost)))`, lumber = `f2i(round(DataB * I2R(woodcost)))` (soft multiply, the round helper
/// `0x6F6EF330`, then the truncating `f2i`), plus the target's own bounty (`0x6F29DD30` / `0x6F29DDD0`) when the buff
/// carries the `DataD` flag and the bounty test `0x6F473170` answers yes (`bounty` is `Some((gold, lumber))` then; the
/// sim has no bounty values, so the unit-spell path passes `None`).
pub fn transmute_amounts(gold_mult: F32, wood_mult: F32, gold_cost: i32, wood_cost: i32, bounty: Option<(i32, i32)>) -> (i32, i32) {
    let mut gold = conv::f2i(softfloat::math::round(arith::mul(gold_mult, conv::i2r(gold_cost))));
    let mut wood = conv::f2i(softfloat::math::round(arith::mul(wood_mult, conv::i2r(wood_cost))));
    if let Some((bg, bw)) = bounty {
        gold = gold.wrapping_add(bg);
        wood = wood.wrapping_add(bw);
    }
    (gold, wood)
}

/// Doom (`ANdo`; effect `0x6F0B65F0` creates `BNdo`, `CBuffDoom < CBuffLifeDoT`): the target takes `DataA` damage per
/// second until it dies (the other `CBuffLifeDoT` ticks: first at 0.01 s, then every second), cannot cast
/// ([`Game::is_silenced`]) and the buff cannot be dispelled (apply slot `0x354` sets the buff flag `0x200`, CONFIRMED;
/// the dispel predicates `0x1D8` / `0x1E0` answer no / no). No duration. The death is handled by [`doom_death`].
fn doom(game: &RefCell<Game>, rec: &CastRec, target: ObjKey) {
    let mut gm = game.borrow_mut();
    if !gm.alive(target) || gm.neutral2_doomed(target) {
        return;
    }
    let preds = BuffPredicates { negative: true, magic: false, dispellable: false, ..buff_predicates(BUFF_DOOM) };
    let serial = gm.apply_mods_buff(target, BUFF_DOOM, preds, UnitMods::default(), F32::ZERO);
    gm.register_tick(target, serial, rec.caster, rec.level.data[0], false, DOOM_DAMAGE_TYPE, true);
    gm.spells.neutral.h2.doom.insert(target, DoomMark { source: rec.caster, level: rec.level });
}

/// The removal notification of `CBuffDoom` (slot `0x284` = `0x6F0B6750`, CONFIRMED): for the doomed unit's death it
/// creates `f2i(DataB)` units of the level's `UnitID` at the dead unit's position for the owner of the source, each
/// with a timed life of `DataC` (buff `BNdi`, `CBuffDoomMinion < CBuffTimedLife`) and with the summoned class bit
/// cleared (`+0x248 &= ~4`); the dead unit is flagged `0x3000` (no corpse).
fn doom_death(game: &RefCell<Game>, unit: ObjKey) {
    let Some(mark) = game.borrow_mut().spells.neutral.h2.doom.remove(&unit) else {
        return;
    };
    let mut gm = game.borrow_mut();
    if mark.level.unit_id == 0 {
        return;
    }
    let (x, y, facing) = {
        let u = gm.unit(unit);
        (u.x, u.y, u.facing)
    };
    let owner = gm.unit(mark.source).owner;
    let n = conv::f2i(mark.level.data[1]).max(0);
    for _ in 0..n {
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, mark.level.unit_id, x, y, facing, true) {
            gm.apply_timed_life(k, BUFF_DOOM_MINION, mark.level.data[2]);
        }
    }
}

impl Game {
    /// Whether `unit` carries the Doom buff.
    pub fn neutral2_doomed(&self, unit: ObjKey) -> bool {
        self.unit(unit).buffs.iter().any(|b| b.base == BUFF_DOOM)
    }

    pub(crate) fn neutral2_next_id(&mut self) -> u32 {
        self.spells.neutral.h2.next += 1;
        self.spells.neutral.h2.next
    }
}

/// Starts a one-shot timer of this module.
pub(crate) fn start_timer(gm: &mut Game, seconds: f32, unit: ObjKey, base: u32, aux: u32, kind: u8) {
    gm.timers
        .start(seconds, false, Payload::Spell(SpellMsg::UnitSpell { kind, unit, base, aux }));
}
