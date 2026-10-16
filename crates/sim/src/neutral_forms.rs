//! Summons, forms and passives of the neutral heroes (S50): Storm Earth and Fire, Chemical Rage, Robo-Goblin, Engineering
//! Upgrade, Pocket Factory, Tornado (with the abilities of the tornado unit). Spec: `docs/spec/abilities-neutral.md`
//! section 8. Field roles come from the editor labels (`WESTRING_AEVAL_*`), the structure from the class overrides.

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, conv, F32};

use crate::abilities::{buff_predicates, code, Ability, BuffPredicates};
use crate::cast::{CastRec, CastTarget, SpellLevel};
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::{dispatch_unit_death_by, ScriptRuntime};
use crate::neutral_heroes2::start_timer;
use crate::timer::TimerId;
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// Brewmaster: Storm, Earth and Fire (`CAbilityElementalFury < CAbilitySimpleSpell`, buff `CBuffElementalFury <
/// CBuffTimedLife`).
pub const ELEMENTAL_FURY: u32 = code(b"ANef");
pub const BUFF_ELEMENTAL: u32 = code(b"BNef");
/// Alchemist: Chemical Rage (`CAbilityChemicalRage < CAbilityMorph`, buff `CBuffChemicalRage < CBuffProgressBar`).
pub const CHEMICAL_RAGE: u32 = code(b"ANcr");
pub const BUFF_RAGE: u32 = code(b"BNcr");
/// Tinker: Robo-Goblin (`CAbilityRoboGoblin < CAbilityMorph`), Engineering Upgrade (`CAbilityEngineeringUpgrade <
/// CAbilityPassive`, buff `CBuffEngineeringUpgrade`), Pocket Factory (`CAbilitySummonFactory < CAbilitySimpleSpell`, buffs
/// `CBuffFactory < CBuffTimedLife`, `BNcg`).
pub const ROBO_GOBLIN: u32 = code(b"ANrg");
pub const BUFF_ROBO: u32 = code(b"BNrg");
pub const ENGINEERING_UPGRADE: u32 = code(b"ANeg");
pub const BUFF_ENGINEERING: u32 = code(b"BNeg");
pub const POCKET_FACTORY: u32 = code(b"ANsy");
pub const BUFF_FACTORY: u32 = code(b"BNfy");
pub const BUFF_GOBLIN: u32 = code(b"BNcg");
/// Firelord / Naga: Tornado (`CAbilityTornado < CAbilitySimpleSpell`, buff `CBuffTornado < CBuffTimedLife`) and the unit
/// abilities of the tornado unit `ntor`: Tornado Damage `Atdg`, Tornado Spin `Atsp` (`CBuffTornadoSpin <
/// CBuffCycloneTwo`).
pub const TORNADO: u32 = code(b"ANto");
pub const BUFF_TORNADO: u32 = code(b"BNto");
pub const TORNADO_DAMAGE: u32 = code(b"Atdg");
pub const TORNADO_SPIN: u32 = code(b"Atsp");
pub const BUFF_SPIN: u32 = code(b"Btsp");
pub const BUFF_SPIN_MARK: u32 = code(b"Btsa");

pub const ORDER_ELEMENTAL_FURY: u32 = 852_586;
pub const ORDER_CHEMICAL_RAGE: u32 = 852_663;
pub const ORDER_ROBO_GOBLIN: u32 = 852_656;
pub const ORDER_ROBO_GOBLIN_BACK: u32 = 852_657;
pub const ORDER_POCKET_FACTORY: u32 = 852_658;
pub const ORDER_TORNADO: u32 = 852_597;

/// Timer kinds of this module.
pub const KIND_SPLIT_END: u8 = 0x64;
pub const KIND_FORM: u8 = 0x65;
pub const KIND_FACTORY: u8 = 0x66;
pub const KIND_PASSIVE: u8 = 0x67;

/// The damage type of the tornado's building damage (the neutral spell type, UNSURE).
const FORMS_DAMAGE_TYPE: u32 = 15;
const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);

/// A split Brewmaster: the hidden caster and the three elementals.
#[derive(Debug, Clone)]
struct Split {
    elementals: Vec<ObjKey>,
    serial: u32,
}

/// A unit in a transformed form: the form it left and the serial of the transformation.
#[derive(Debug, Clone, Copy)]
struct FormRec {
    original: u32,
    ability: u32,
    serial: u32,
}

/// A running Pocket Factory.
#[derive(Debug, Clone, Copy)]
struct FactoryRec {
    caster: ObjKey,
    level: SpellLevel,
    goblin: u32,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    /// The unit-code lists of the level data: `(ability, level index, data index 0..=5) -> codes`. The engine fills them
    /// from the text cells (`DataA`.. of the rows that name units or abilities); tests set them directly.
    lists: BTreeMap<(u32, usize, usize), Vec<u32>>,
    splits: BTreeMap<ObjKey, Split>,
    member_of: BTreeMap<ObjKey, ObjKey>,
    forms: BTreeMap<ObjKey, FormRec>,
    factories: BTreeMap<ObjKey, FactoryRec>,
    /// Running timers of the unit abilities (`Atdg`, `Atsp`).
    passives: BTreeMap<(ObjKey, u32), TimerId>,
    /// The Engineering Upgrade bonuses a unit carries: `(level index, flat damage added)`.
    engineering: BTreeMap<ObjKey, (usize, F32)>,
    serial: u32,
}

pub(crate) fn register_orders(ids: &mut BTreeMap<u32, u32>) {
    for (id, base) in [
        (ORDER_ELEMENTAL_FURY, ELEMENTAL_FURY),
        (ORDER_CHEMICAL_RAGE, CHEMICAL_RAGE),
        (ORDER_ROBO_GOBLIN, ROBO_GOBLIN),
        (ORDER_POCKET_FACTORY, POCKET_FACTORY),
        (ORDER_TORNADO, TORNADO),
    ] {
        ids.insert(id, base);
    }
}

pub(crate) fn takes_no_target(base: u32) -> bool {
    matches!(base, ELEMENTAL_FURY | CHEMICAL_RAGE | ROBO_GOBLIN)
}

pub(crate) fn is_forms_spell(base: u32) -> bool {
    matches!(base, ELEMENTAL_FURY | CHEMICAL_RAGE | ROBO_GOBLIN | POCKET_FACTORY | TORNADO)
}

impl Game {
    /// Installs the unit-code list of data field `data` (0 = `DataA`) of level `level` (0-based) of ability `base`.
    pub fn set_neutral_unit_list(&mut self, base: u32, level: usize, data: usize, codes: Vec<u32>) {
        self.spells.neutral.h2.forms.lists.insert((base, level, data), codes);
    }

    /// The code list of data field `data` of the ability `base` of `unit`: the row the ability reads (an upgraded or
    /// creep alias row has its own lists), at the ability's level, falling back to level 1 and to the base row.
    fn neutral_list(&self, unit: ObjKey, base: u32, data: usize) -> Vec<u32> {
        let (row, level) = self
            .unit(unit)
            .abilities
            .get(base)
            .map_or((base, 0), |a| (a.source, usize::try_from(a.level_index).unwrap_or(0)));
        let lists = &self.spells.neutral.h2.forms.lists;
        [row, base]
            .iter()
            .find_map(|r| lists.get(&(*r, level, data)).or_else(|| lists.get(&(*r, 0, data))))
            .cloned()
            .unwrap_or_default()
    }

    fn forms_serial(&mut self) -> u32 {
        self.spells.neutral.h2.forms.serial += 1;
        self.spells.neutral.h2.forms.serial
    }

    /// Whether the Brewmaster `unit` is split into its elementals.
    pub fn is_split(&self, unit: ObjKey) -> bool {
        self.spells.neutral.h2.forms.splits.contains_key(&unit)
    }

    /// Whether `unit` is in a form made by Chemical Rage or Robo-Goblin.
    pub fn in_neutral_form(&self, unit: ObjKey) -> bool {
        self.spells.neutral.h2.forms.forms.contains_key(&unit)
    }
}

pub(crate) fn run_effect(game: &RefCell<Game>, rec: &CastRec) -> bool {
    match rec.base {
        ELEMENTAL_FURY => split(game, rec),
        CHEMICAL_RAGE | ROBO_GOBLIN => transform(game, rec),
        POCKET_FACTORY => pocket_factory(game, rec),
        TORNADO => tornado(game, rec),
        _ => return false,
    }
    true
}

/// The immediate toggle orders: the Robo-Goblin way back (`0x318` = 852657).
pub(crate) fn toggle_order(game: &RefCell<Game>, unit: ObjKey, id: u32) -> Option<bool> {
    if id != ORDER_ROBO_GOBLIN_BACK {
        return None;
    }
    let mut gm = game.borrow_mut();
    if !gm.alive(unit) || gm.unit(unit).abilities.get(ROBO_GOBLIN).is_none_or(|a| a.is_removed()) {
        return Some(false);
    }
    let Some(rec) = gm.spells.neutral.h2.forms.forms.get(&unit).copied() else {
        return Some(false);
    };
    if rec.ability != ROBO_GOBLIN {
        return Some(false);
    }
    // The way back takes the transformation time again (`Dur`).
    let dur = gm.spell_level(unit, ROBO_GOBLIN).dur.to_f32().max(0.0);
    start_timer(&mut gm, dur, unit, ROBO_GOBLIN, rec.serial | 0x8000_0000, KIND_FORM);
    Some(true)
}

pub(crate) fn on_timer(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, kind: u8, unit: ObjKey, base: u32, aux: u32) {
    match kind {
        KIND_SPLIT_END => split_end(game, rt, unit, aux, false),
        KIND_FORM => form_step(game, unit, base, aux),
        KIND_FACTORY => factory_tick(game, unit, aux),
        KIND_PASSIVE => passive_tick(game, rt, unit, base),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Storm, Earth and Fire

/// Storm, Earth and Fire (`ANef`, order 852586; the editor text: "Splits the Pandaren Brewmaster into elements, forming 3
/// specialized warriors. If any of them survive until the end of their summoned timer, the Brewmaster is reborn. Lasts
/// `HeroDur` seconds."). `DataA` lists the three unit types. The three elementals appear around the caster (on a circle of
/// radius `Area`, 128), the Brewmaster is hidden and cannot act. At the end of `HeroDur` the survivors vanish and the
/// Brewmaster returns at the place of the first survivor; if all three die earlier the Brewmaster dies with them (UNSURE:
/// the death rule is the tooltip's reading; the buff `CBuffElementalFury < CBuffTimedLife` is the elementals' timed life,
/// here a backstop one second after the end).
fn split(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let caster = rec.caster;
    if !gm.alive(caster) || gm.is_split(caster) {
        return;
    }
    let types = gm.neutral_list(caster, ELEMENTAL_FURY, 0);
    if types.is_empty() {
        return;
    }
    let (x, y, owner, facing) = {
        let c = gm.unit(caster);
        (c.x, c.y, c.owner, c.facing)
    };
    let tables = gm.order_tables().cloned();
    let dur = crate::cast::effect_duration(&rec.level, true, F32::ZERO);
    let n = types.len() as i32;
    let mut made = Vec::new();
    for (i, ty) in types.iter().enumerate() {
        let (mut px, mut py) = (x, y);
        if let Some(t) = &tables {
            let step = arith::div(TWO_PI, conv::i2r(n), t);
            let angle = arith::mul(step, conv::i2r(i as i32));
            px = arith::add(x, arith::mul(rec.level.area, softfloat::math::cos(angle, t)));
            py = arith::add(y, arith::mul(rec.level.area, softfloat::math::sin(angle, t)));
        }
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, *ty, px, py, facing, true) {
            gm.unit_mut(k).summoned = true;
            gm.apply_timed_life(k, BUFF_ELEMENTAL, arith::add(dur, F32::ONE));
            made.push(k);
        }
    }
    if made.is_empty() {
        return;
    }
    let serial = gm.forms_serial();
    for &k in &made {
        gm.spells.neutral.h2.forms.member_of.insert(k, caster);
    }
    gm.spells.neutral.h2.forms.splits.insert(caster, Split { elementals: made, serial });
    gm.unit_mut(caster).hidden = true;
    gm.unit_mut(caster).paused = true;
    start_timer(&mut gm, dur.to_f32(), caster, ELEMENTAL_FURY, serial, KIND_SPLIT_END);
}

/// The end of a split: `all_dead` when the last elemental died. Survivors vanish and the Brewmaster returns; with no
/// survivor the Brewmaster dies.
fn split_end(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, caster: ObjKey, serial: u32, all_dead: bool) {
    let died = {
        let mut gm = game.borrow_mut();
        let Some(sp) = gm.spells.neutral.h2.forms.splits.get(&caster).cloned() else {
            return;
        };
        if sp.serial != serial {
            return;
        }
        gm.spells.neutral.h2.forms.splits.remove(&caster);
        let survivors: Vec<ObjKey> = sp.elementals.iter().copied().filter(|&k| gm.alive(k)).collect();
        for &k in &sp.elementals {
            gm.spells.neutral.h2.forms.member_of.remove(&k);
        }
        let place = survivors.first().map(|&k| (gm.unit(k).x, gm.unit(k).y));
        for &k in &survivors {
            crate::natives::unit::remove_unit(&mut gm, k);
        }
        gm.unit_mut(caster).hidden = false;
        gm.unit_mut(caster).paused = false;
        if survivors.is_empty() || all_dead {
            crate::natives::unit::set_life(&mut gm, caster, F32::ZERO)
        } else {
            if let Some((px, py)) = place {
                gm.store_unit_position(caster, px, py);
            }
            false
        }
    };
    if died {
        dispatch_unit_death_by(game, rt, caster, None);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Chemical Rage and Robo-Goblin

/// The type change with the abilities kept (a hero keeps its learned skills across a morph; the type record's ability list
/// replaces only the unit's own): the abilities the old form had and the new form's list lacks are put back.
fn morph_keep(gm: &mut Game, unit: ObjKey, to: u32) {
    let before: Vec<Ability> = gm.unit(unit).abilities.iter().cloned().collect();
    gm.morph_to(unit, to);
    for a in before {
        if !gm.unit(unit).abilities.contains(a.base) {
            gm.unit_mut(unit).abilities.attach(a);
        }
    }
}

/// Chemical Rage (`ANcr`, order 852663, immediate; labels `EME1..4`, `NCR5/6`) and Robo-Goblin (`ANrg`, 852656 to
/// transform and 852657 back, `NRG5/6`): `CAbilityMorph` classes. `Dur` is the transformation time (0.35 s / 1.5 s), the
/// unit becomes the level's `UnitID` then (the editor's "Alternate Form" fields `DataA` name the original form, the
/// effect uses the unit's own type). Chemical Rage ends `HeroDur` (15 s) after the cast and the unit returns to its form;
/// Robo-Goblin stays until the way-back order. The movement and attack speed of the rage ("Info Panel Only" in the
/// editor) come from the form's own unit record. Robo-Goblin adds `DataF` armor (the editor's "Defense Bonus"); the
/// `DataE` strength bonus is not applied (hero attribute modifiers are not exposed to this module).
fn transform(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let unit = rec.caster;
    if !gm.alive(unit) || gm.in_neutral_form(unit) || rec.level.unit_id == 0 || gm.unit_types().get(rec.level.unit_id).is_none() {
        return;
    }
    let original = gm.unit(unit).type_id;
    let serial = gm.forms_serial();
    gm.spells.neutral.h2.forms.forms.insert(unit, FormRec { original, ability: rec.base, serial });
    let dur = rec.level.dur.to_f32().max(0.0);
    // aux packs the step: bit 31 set = the way back.
    start_timer(&mut gm, dur, unit, rec.base, serial, KIND_FORM);
    if rec.base == CHEMICAL_RAGE {
        let total = rec.level.hero_dur.to_f32().max(dur);
        start_timer(&mut gm, total, unit, rec.base, serial | 0x8000_0000, KIND_FORM);
    }
}

/// A form timer. `aux` is the transformation serial; with bit 31 set it is the way back.
fn form_step(game: &RefCell<Game>, unit: ObjKey, base: u32, aux: u32) {
    let mut gm = game.borrow_mut();
    let back = aux & 0x8000_0000 != 0;
    let serial = aux & 0x7FFF_FFFF;
    let Some(rec) = gm.spells.neutral.h2.forms.forms.get(&unit).copied() else {
        return;
    };
    if rec.serial != serial || !gm.alive(unit) {
        if rec.serial == serial {
            gm.spells.neutral.h2.forms.forms.remove(&unit);
        }
        return;
    }
    if back {
        morph_keep(&mut gm, unit, rec.original);
        gm.spells.neutral.h2.forms.forms.remove(&unit);
        gm.remove_buff_code(unit, BUFF_ROBO);
        return;
    }
    let level = gm.spell_level(unit, base);
    if gm.unit(unit).type_id == rec.original {
        morph_keep(&mut gm, unit, level.unit_id);
        if base == ROBO_GOBLIN {
            let preds = BuffPredicates { positive: true, ..buff_predicates(BUFF_ROBO) };
            let mods = UnitMods { armor: level.data[5], ..UnitMods::default() };
            gm.apply_mods_buff(unit, BUFF_ROBO, preds, mods, F32::ZERO);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Pocket Factory

/// Pocket Factory (`ANsy`, order 852658, point; labels `NSY1..5`): a factory unit (the level's `UnitID`) appears at the
/// point for `Dur` (buff `BNfy`, timed life). Every `DataA` seconds it makes one goblin of type `DataB` (a unit code) at
/// an offset of `DataD` from the factory, which lives `DataC` seconds (buff `BNcg`). The leash range `DataE` and the
/// goblins' explosion (`Asdg`) are not modelled. LIKELY.
fn pocket_factory(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (x, y) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => return,
    };
    if rec.level.unit_id == 0 {
        return;
    }
    let (owner, facing) = {
        let c = gm.unit(rec.caster);
        (c.owner, c.facing)
    };
    let goblin = gm.neutral_list(rec.caster, POCKET_FACTORY, 1).first().copied().unwrap_or(0);
    let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, rec.level.unit_id, x, y, facing, true) else {
        return;
    };
    gm.unit_mut(k).summoned = true;
    gm.apply_timed_life(k, BUFF_FACTORY, rec.level.dur);
    gm.spells.neutral.h2.forms.factories.insert(k, FactoryRec { caster: rec.caster, level: rec.level, goblin });
    if arith::lt(F32::ZERO, rec.level.data[0]) {
        let serial = gm.forms_serial();
        start_timer(&mut gm, rec.level.data[0].to_f32(), k, POCKET_FACTORY, serial, KIND_FACTORY);
    }
}

fn factory_tick(game: &RefCell<Game>, factory: ObjKey, _aux: u32) {
    let mut gm = game.borrow_mut();
    let Some(f) = gm.spells.neutral.h2.forms.factories.get(&factory).copied() else {
        return;
    };
    if !gm.alive(factory) {
        gm.spells.neutral.h2.forms.factories.remove(&factory);
        return;
    }
    if f.goblin != 0 {
        let (x, y, owner, facing) = {
            let u = gm.unit(factory);
            (u.x, u.y, u.owner, u.facing)
        };
        let px = arith::add(x, f.level.data[3]);
        if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, f.goblin, px, y, facing, true) {
            gm.unit_mut(k).summoned = true;
            gm.apply_timed_life(k, BUFF_GOBLIN, f.level.data[2]);
        }
    }
    start_timer(&mut gm, f.level.data[0].to_f32(), factory, POCKET_FACTORY, 0, KIND_FACTORY);
}

// ---------------------------------------------------------------------------------------------------------------
// Tornado

/// Tornado (`ANto`, order 852597, point; `UnitID` `ntor`): the tornado unit appears at the point for `Dur` (buff `BNto`,
/// timed life) for the caster's player. Its own abilities do the work: `Aasl` (slow aura, `unit_abilities_a`), `Atdg`
/// (building damage) and `Atsp` (spin), driven by [`sync_neutral2`]; `Atwa` (random wandering) is not modelled.
fn tornado(game: &RefCell<Game>, rec: &CastRec) {
    let mut gm = game.borrow_mut();
    let (x, y) = match rec.target {
        CastTarget::Point(x, y) => (x, y),
        CastTarget::Unit(t) => (gm.unit(t).x, gm.unit(t).y),
        CastTarget::None => return,
    };
    if rec.level.unit_id == 0 {
        return;
    }
    let (owner, facing) = {
        let c = gm.unit(rec.caster);
        (c.owner, c.facing)
    };
    if let Some((k, _)) = crate::natives::unit::build(&mut gm, owner, rec.level.unit_id, x, y, facing, true) {
        gm.unit_mut(k).summoned = true;
        let dur = crate::cast::effect_duration(&rec.level, gm.target_hero(rec.caster), F32::ZERO);
        gm.apply_timed_life(k, BUFF_TORNADO, dur);
        gm.sync_auras(k);
    }
}

/// Keeps the passives of this batch in step with the abilities a unit has (called from `sync_neutral_spells`): the timers
/// of `Atdg` / `Atsp` and the Engineering Upgrade bonuses.
pub(crate) fn sync_neutral2(gm: &mut Game, unit: ObjKey) {
    for (base, period) in [(TORNADO_DAMAGE, 1.0f32), (TORNADO_SPIN, 0.5f32)] {
        let has = gm.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed());
        let running = gm.spells.neutral.h2.forms.passives.contains_key(&(unit, base));
        if has && !running {
            let id = gm.timers.start(
                period,
                true,
                Payload::Spell(crate::cast::SpellMsg::UnitSpell { kind: KIND_PASSIVE, unit, base, aux: 0 }),
            );
            gm.spells.neutral.h2.forms.passives.insert((unit, base), id);
        } else if !has && running {
            if let Some(id) = gm.spells.neutral.h2.forms.passives.remove(&(unit, base)) {
                gm.timers.cancel(id);
            }
        }
    }
    sync_engineering(gm, unit);
}

/// Engineering Upgrade (`ANeg`, passive; labels `NEG1..6`): `DataA` is a movement speed bonus (a fraction: 0.1 = 10
/// percent, the tooltip shows it as a percentage), `DataB` a flat attack damage bonus, and `DataC..F` name four ability
/// upgrades `from,to`: the unit's ability that stands for `from` (a Pocket Factory, Cluster Rockets, Robo-Goblin or
/// Demolish row) is read from the row `to` from then on, at the same level. The bonuses follow the ability's level and go
/// when the ability goes. The movement bonus is the buff `BNeg`; the damage bonus is added to the hero attack bonus.
fn sync_engineering(gm: &mut Game, unit: ObjKey) {
    let has = gm.unit(unit).abilities.get(ENGINEERING_UPGRADE).is_some_and(|a| !a.is_removed());
    let applied = gm.spells.neutral.h2.forms.engineering.get(&unit).copied();
    if !has {
        if let Some((_, dmg)) = applied {
            let u = gm.unit_mut(unit);
            u.hero_attack = arith::sub(u.hero_attack, dmg);
            gm.spells.neutral.h2.forms.engineering.remove(&unit);
            gm.remove_buff_code(unit, BUFF_ENGINEERING);
        }
        return;
    }
    let level_idx = gm.unit(unit).abilities.get(ENGINEERING_UPGRADE).map_or(0, |a| usize::try_from(a.level_index).unwrap_or(0));
    let level = gm.spell_level(unit, ENGINEERING_UPGRADE);
    if applied.map(|a| a.0) != Some(level_idx) {
        if let Some((_, old)) = applied {
            let u = gm.unit_mut(unit);
            u.hero_attack = arith::sub(u.hero_attack, old);
        }
        let u = gm.unit_mut(unit);
        u.hero_attack = arith::add(u.hero_attack, level.data[1]);
        gm.spells.neutral.h2.forms.engineering.insert(unit, (level_idx, level.data[1]));
        let preds = BuffPredicates { positive: true, ..buff_predicates(BUFF_ENGINEERING) };
        gm.apply_mods_buff(unit, BUFF_ENGINEERING, preds, UnitMods { move_delta: level.data[0], ..UnitMods::default() }, F32::ZERO);
    }
    // The ability upgrades of this level.
    for data in 2..6 {
        let pair = gm.neutral_list(unit, ENGINEERING_UPGRADE, data);
        let [from, to] = pair.as_slice() else { continue };
        let base = gm.spell_alias_base(*from);
        if let Some(a) = gm.unit_mut(unit).abilities.get_mut(base) {
            if a.source == *from {
                a.source = *to;
            }
        }
    }
}

/// A tick of `Atdg` / `Atsp` on `unit`.
fn passive_tick(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey, base: u32) {
    {
        let mut gm = game.borrow_mut();
        let ok = gm.alive(unit) && gm.unit(unit).abilities.get(base).is_some_and(|a| !a.is_removed());
        if !ok {
            if let Some(id) = gm.spells.neutral.h2.forms.passives.remove(&(unit, base)) {
                gm.timers.cancel(id);
            }
            return;
        }
    }
    match base {
        TORNADO_DAMAGE => tornado_damage(game, rt, unit),
        TORNADO_SPIN => tornado_spin(game, unit),
        _ => {}
    }
}

/// Tornado Damage (`Atdg`, "Building Damage Aura"; labels `TDG1..5`): every second the enemy structures within `Area`
/// take `DataA`; those within `DataB` take `DataC` (the "Medium" tier) and those within `DataD` take `DataE` (the
/// "Small" tier, unused in the data). LIKELY.
fn tornado_damage(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey) {
    let mut killed = Vec::new();
    {
        let mut gm = game.borrow_mut();
        let l = gm.spell_level(unit, TORNADO_DAMAGE);
        let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
        let word = targs_word("structure,enemy");
        let d2 = |gm: &Game, k: ObjKey| {
            let u = gm.unit(k);
            let (dx, dy) = (arith::sub(u.x, x), arith::sub(u.y, y));
            arith::add(arith::mul(dx, dx), arith::mul(dy, dy))
        };
        for k in crate::natives::group::circle_candidates(&gm, x, y, l.area) {
            {
                let u = gm.unit(k);
                if u.removed || u.hidden || u.dead {
                    continue;
                }
            }
            if !gm.unit_types().is_building(gm.unit(k).type_id) || !crate::orc::area_predicate(&gm, unit, k, word) {
                continue;
            }
            let dist2 = d2(&gm, k);
            let within = |r: F32| arith::lt(F32::ZERO, r) && !arith::lt(arith::mul(r, r), dist2);
            let amount = if within(l.data[3]) {
                l.data[4]
            } else if within(l.data[1]) {
                l.data[2]
            } else {
                l.data[0]
            };
            if let Some(d) = gm.unit_damage_target(unit, k, amount, false, false, 0, FORMS_DAMAGE_TYPE) {
                killed.push(d);
            }
        }
    }
    for t in killed {
        dispatch_unit_death_by(game, rt, t, Some(unit));
    }
}

/// Tornado Spin (`Atsp`; labels `TSP1/2`, buffs `Btsp` / `Btsa`): an enemy ground non-structure unit within `Area` that does
/// not carry the marker `Btsa` is tossed: it is stunned and cannot be hurt for `Dur` (`HeroDur` for heroes) like Cyclone
/// (`CBuffTornadoSpin < CBuffCycloneTwo`) and gets the marker for `DataB` seconds (the "Minimum Hit Interval"). The
/// "Air Time" `DataA` (22 in the data) is not used: its unit is unclear (UNSURE). The tornado picks the units in the area
/// query's order, one per scan.
fn tornado_spin(game: &RefCell<Game>, unit: ObjKey) {
    let mut gm = game.borrow_mut();
    let l = gm.spell_level(unit, TORNADO_SPIN);
    let (x, y) = (gm.unit(unit).x, gm.unit(unit).y);
    let word = targs_word("ground,enemy");
    let pick = crate::natives::group::circle_candidates(&gm, x, y, l.area).into_iter().find(|&k| {
        let u = gm.unit(k);
        !u.removed
            && !u.hidden
            && !u.dead
            && !gm.unit_types().is_building(u.type_id)
            && !u.buffs.iter().any(|b| b.base == BUFF_SPIN_MARK)
            && crate::orc::area_predicate(&gm, unit, k, word)
            && targs_ok(&gm, unit, k, word)
    });
    let Some(k) = pick else { return };
    let dur = crate::cast::effect_duration(&l, gm.target_hero(k), F32::ZERO);
    if arith::lt(F32::ZERO, dur) {
        gm.apply_stun_as(BUFF_SPIN, k, dur);
        let preds = BuffPredicates { negative: true, magic: true, dispellable: true, ..buff_predicates(BUFF_SPIN_MARK) };
        gm.apply_mods_buff(k, BUFF_SPIN_MARK, preds, UnitMods { invuln: true, ..UnitMods::default() }, dur);
    }
}

/// A unit died: a dead elemental may end the split.
pub(crate) fn on_unit_died(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, unit: ObjKey) {
    let last = {
        let gm = game.borrow();
        let forms = &gm.spells.neutral.h2.forms;
        let Some(&caster) = forms.member_of.get(&unit) else {
            return;
        };
        let Some(sp) = forms.splits.get(&caster) else {
            return;
        };
        sp.elementals.iter().all(|&k| k == unit || !gm.alive(k)).then_some((caster, sp.serial))
    };
    if let Some((caster, serial)) = last {
        split_end(game, rt, caster, serial, true);
    }
}
