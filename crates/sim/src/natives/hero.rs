//! Hero natives: the experience and level natives, the skill points, the strength and agility getters and setters,
//! `SuspendHeroXP`, and the table of every hero native (implemented or not). The arithmetic is in `crate::heroes`; the
//! model is in `natives/game.rs` ([`HeroObj`]). The derivation and the verification labels are in
//! `docs/spec/heroes.md`.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions (`natives.c` bodies in the decompile export).
//!
//! * Hero gate (CONFIRMED, every hero native): the unit must be live, its type id must start with an uppercase
//!   letter (`IsHeroUnitId`, the top byte of the four-character code in `'A'..='Z'`), and the unit must not be an
//!   illusion (object `+0x5C` bit 30). Otherwise the native returns its default and does nothing.
//! * Experience and levels: see `crate::heroes` (`AddHeroXP` / `SetHeroXP` / `SetHeroLevel` / `UnitStripHeroLevel`).
//!   `showEyeCandy` is ignored (presentation only). The experience tables are installed with `Game::set_hero_xp`;
//!   without them the heroes have no thresholds.
//! * Skill points: `UnitModifySkillPoints` and the level-up gain; the cap by the learnable ability levels is not
//!   applied because the five hero ability slots are not modelled (`crate::heroes`).
//! * Attributes: `GetHeroStr` / `GetHeroAgi` = base + trunc(gain * (level - 1)); item bonuses are not modelled and
//!   count as 0. `GetHeroInt` / `SetHeroInt` = base + trunc(gain * level counter), the base being the counter behind
//!   hero+0x7C (`docs/spec/heroes.md`, section 6); the item part counts as 0.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::conv;

use super::game::{Game, HeroObj, ObjKey};
use super::unit_attr::{NativeEntry, Support};
use crate::heroes;

/// Every hero native with its support level. The ones marked `NotImplemented` are not registered: a script that
/// calls one fails to bind (see `unbound_natives` in the VM), which is the work queue for the hero system.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "IsSuspendedXP",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "AddHeroXP",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetHeroAgi",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetHeroInt",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetHeroLevel",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetHeroProperName",
        support: Support::NotImplemented(
            "N:proper-name table and the draw made at creation not read",
        ),
    },
    NativeEntry {
        name: "GetHeroSkillPoints",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetHeroStr",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetHeroXP",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "ReviveHero",
        support: Support::NotImplemented(
            "N:revive path (FUN_6F2A81F0) only partly read; the corpse and event path is not modelled",
        ),
    },
    NativeEntry {
        name: "ReviveHeroLoc",
        support: Support::NotImplemented(
            "N:same as ReviveHero (location wrapper)",
        ),
    },
    NativeEntry {
        name: "SelectHeroSkill",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetHeroAgi",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetHeroInt",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetHeroLevel",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetHeroStr",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetHeroXP",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SuspendHeroXP",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitModifySkillPoints",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitStripHeroLevel",
        support: Support::Likely,
    },
];

/// Whether the raw type id `id` is a hero type: its top byte is an uppercase ASCII letter (CONFIRMED,
/// `IsHeroUnitId` `0x6F3B2E90`).
pub fn is_hero_id(id: u32) -> bool {
    (id >> 24).wrapping_sub(0x41) < 0x1A
}

/// Registers the implemented hero natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("GetHeroLevel", move |_, a| {
        let gm = g.borrow();
        let v = hero_of(&gm, a).map_or(0, |h| h.level);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetHeroXP", move |_, a| {
        let gm = g.borrow();
        let v = hero_of(&gm, a).map_or(0, |h| h.xp);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetHeroSkillPoints", move |_, a| {
        let gm = g.borrow();
        let v = hero_of(&gm, a).map_or(0, |h| h.skill_points);
        NativeResult::Return(Value::Integer(v))
    });

    // `IsSuspendedXP` (CONFIRMED gate, UNSURE value): the same hero gate as the other hero natives. The flag it reads is
    // the one `SuspendHeroXP` stores; a unit that fails the gate reports true (the original's default is 1).
    let g = Rc::clone(game);
    reg.register("IsSuspendedXP", move |_, a| {
        let gm = g.borrow();
        let v = hero_of(&gm, a).is_none_or(|h| h.xp_suspended);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("SuspendHeroXP", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = hero_key(&gm, a) {
            if let Some(h) = gm.unit_mut(k).hero.as_mut() {
                h.xp_suspended = bool_arg(a, 1);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("AddHeroXP", move |_, a| {
        with_hero(&g, a, |t, h, s| heroes::add_xp(t, h, conv::i2r(int_arg(a, 1)), s));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetHeroXP", move |_, a| {
        with_hero(&g, a, |t, h, s| heroes::set_xp(t, h, int_arg(a, 1), s));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetHeroLevel", move |_, a| {
        with_hero(&g, a, |t, h, s| heroes::set_level(t, h, int_arg(a, 1), s));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitStripHeroLevel", move |_, a| {
        let mut removed = false;
        with_hero(&g, a, |t, h, _| removed = heroes::strip_levels(t, h, int_arg(a, 1) as u32));
        NativeResult::Return(Value::Boolean(removed))
    });

    let g = Rc::clone(game);
    reg.register("SelectHeroSkill", move |ctx, a| {
        select_hero_skill(&g, ctx, a);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitModifySkillPoints", move |_, a| {
        let mut changed = false;
        let delta = int_arg(a, 1);
        with_hero_only(&g, a, |h| changed = heroes::modify_skill_points(h, delta, None));
        NativeResult::Return(Value::Boolean(changed))
    });

    let g = Rc::clone(game);
    reg.register("GetHeroStr", move |_, a| {
        let gm = g.borrow();
        let v = hero_of(&gm, a).map_or(0, |h| heroes::attribute(h.str_base, h.str_plus, h.level - 1));
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetHeroAgi", move |_, a| {
        let gm = g.borrow();
        let v = hero_of(&gm, a).map_or(0, |h| heroes::attribute(h.agi_base, h.agi_plus, h.level - 1));
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetHeroInt", move |_, a| {
        let gm = g.borrow();
        let v = hero_of(&gm, a).map_or(0, |h| heroes::intelligence(h.int_base, h.int_plus, h.level - 1));
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetHeroInt", move |_, a| {
        let value = int_arg(a, 1);
        with_attribute_change(&g, a, bool_arg(a, 2), |h| h.int_base = heroes::rebase(h.int_base, h.int_plus, h.level - 1, value));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetHeroStr", move |_, a| {
        let value = int_arg(a, 1);
        with_attribute_change(&g, a, bool_arg(a, 2), |h| h.str_base = heroes::rebase(h.str_base, h.str_plus, h.level - 1, value));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetHeroAgi", move |_, a| {
        let value = int_arg(a, 1);
        with_attribute_change(&g, a, bool_arg(a, 2), |h| h.agi_base = heroes::rebase(h.agi_base, h.agi_plus, h.level - 1, value));
        NativeResult::Return(Value::Null)
    });
}

/// Runs `f` on the experience tables, the hero state of argument 0 and the skill point cap when the hero gate passes.
/// The cap (`heroes::skill_point_cap`, the learnable levels of the five skill slots) is `None` for a hero without
/// skill slots, which the model leaves uncapped (the original would cap such a hero at 0: UNSURE, kept so that
/// synthetic heroes without a `heroAbilList` still gain points).
fn with_hero(
    g: &Rc<RefCell<Game>>,
    a: &[Value],
    f: impl FnOnce(&heroes::XpTables, &mut HeroObj, Option<i32>),
) {
    let mut gm = g.borrow_mut();
    let Some(k) = hero_key(&gm, a) else {
        return;
    };
    let tables = gm.hero_xp().clone();
    let slack = skill_cap(&gm, k);
    if let Some(h) = gm.unit_mut(k).hero.as_mut() {
        f(&tables, h, slack);
    }
}

/// The skill point cap of the hero `k` (see [`with_hero`]).
fn skill_cap(gm: &Game, k: ObjKey) -> Option<i32> {
    let u = gm.unit(k);
    let h = u.hero.as_ref()?;
    if h.slots.iter().all(|s| s.code == 0) {
        return None;
    }
    Some(heroes::skill_point_cap(&h.slots, &|code| u.abilities.get(code).map(|ab| ab.level_index)))
}

/// The state change of the hero learn (`0x6F277A00` -> `0x6F2095D0`, see [`select_hero_skill`]) without the events:
/// returns the owner and the `(ability, level)` pairs that were set, or `None` when the unit method refuses. The AI
/// host calls this directly for the hero-level callback (`docs/spec/heroes.md` section 14).
pub fn apply_learn(gm: &mut Game, k: ObjKey, code: u32) -> Option<(u32, Vec<(u32, i32)>)> {
    use heroes::LearnEffect;
    let u = gm.unit(k);
    u.hero.as_ref()?;
    let owner = u.owner;
    // `0x6F277A00`: an ability the unit holds is learned only below its max minus one.
    if let Some(ab) = u.abilities.get(code) {
        if ab.level_index >= ab.max_level() - 1 {
            return None;
        }
    }
    let available = !gm.misc.players[(owner as usize).min(15)].unavailable_abilities.contains(&code_of_slot(u, code));
    let misc_skip = gm.kill_xp_config().hero_ability_level_skip;
    let u = gm.unit(k);
    let h = u.hero.as_ref()?;
    let learned = |c: u32| u.abilities.get(c).map(|ab| ab.level_index);
    let skip = |c: u32| gm.hero_ability_def(c).map_or(0, |d| d.level_skip);
    let env = heroes::LearnEnv {
        learned: &learned,
        level_skip: &skip,
        misc_skip,
        available,
        deps: (true, true),
        level_counter: h.level - 1,
        cheat: false,
    };
    let mut points = h.skill_points;
    let slots = h.slots;
    let effects = heroes::learn(&mut points, &slots, code, false, &env);
    let mut set_levels = Vec::new();
    let ability_types = gm.ability_types().clone();
    let unit = gm.unit_mut(k);
    for e in &effects {
        match e {
            LearnEffect::LevelUp(c) => {
                if let Some(ab) = unit.abilities.get_mut(*c) {
                    ab.inc_level();
                }
            }
            LearnEffect::Add(c) => {
                unit.abilities.attach(crate::abilities::Ability::new(*c, ability_types.get(*c).copied()));
            }
            LearnEffect::SetLevel(c, l) => set_levels.push((*c, *l)),
            _ => {}
        }
    }
    if !effects.is_empty() && !matches!(effects[0], LearnEffect::Error(_)) {
        unit.hero.as_mut().expect("hero").skill_points = points;
    }
    Some((owner, set_levels))
}

/// `SelectHeroSkill(whichHero, abilCode)` (`0x6F3C7050` -> unit method `0x6F277A00` -> hero learn `0x6F2095D0`,
/// CONFIRMED structure and Unicorn-checked arithmetic, `docs/spec/heroes.md` section 12). The unit method learns when the
/// unit has no such ability, or its level index is below its max minus one; the learn is [`heroes::learn`]. The
/// effects are applied to the unit's abilities and the hero skill event (player-unit 0x2A, unit 0x4F) fires after the
/// level is set. The dependency check and the debug bit have no model: the dependency answers pass (UNSURE).
fn select_hero_skill(g: &Rc<RefCell<Game>>, ctx: &mut dyn super::ScriptRuntime, a: &[Value]) {
    use super::event::{Datum, Field, Firing};
    use super::unit_event::{fire_observers_in, KEY_BASE};

    let code = int_arg(a, 1) as u32;
    let (k, owner, levels) = {
        let mut gm = g.borrow_mut();
        let Some(k) = hero_key(&gm, a) else {
            return;
        };
        let Some((owner, levels)) = apply_learn(&mut gm, k, code) else {
            return;
        };
        (k, owner, levels)
    };
    for (c, l) in levels {
        let firing = {
            let mut gm = g.borrow_mut();
            gm.firings.push(Firing {
                id: 0x4F,
                data: vec![
                    (Field::Learner, Datum::Object(k)),
                    (Field::Skill, Datum::Int(c as i32)),
                    (Field::SkillLevel, Datum::Int(l)),
                ],
            });
            (gm.firings.len() - 1) as u32
        };
        fire_observers_in(g, ctx, ObjKey(owner), 0x2A + KEY_BASE, k, Some(firing));
        fire_observers_in(g, ctx, k, 0x4F + KEY_BASE, k, Some(firing));
    }
}

/// The ability code the availability check reads: the slot's code (the learn tests the code of the slot it found, which
/// differs from the requested code only for the `'Aroa'` / `'Arou'` alias).
fn code_of_slot(u: &crate::natives::game::UnitObj, code: u32) -> u32 {
    match u.hero.as_ref() {
        Some(h) => h
            .slots
            .iter()
            .find(|s| (code == heroes::ALIAS_ABILITY && s.code == heroes::ALIAS_SLOT) || s.code == code)
            .map_or(code, |s| s.code),
        None => code,
    }
}

/// Runs `f` on the hero state of argument 0 when the hero gate passes.
fn with_hero_only(g: &Rc<RefCell<Game>>, a: &[Value], f: impl FnOnce(&mut HeroObj)) {
    let mut gm = g.borrow_mut();
    let Some(k) = hero_key(&gm, a) else {
        return;
    };
    if let Some(h) = gm.unit_mut(k).hero.as_mut() {
        f(h);
    }
}

/// The unit key of argument 0 when it passes the hero gate (see the module notes).
fn hero_key(gm: &Game, a: &[Value]) -> Option<ObjKey> {
    let k = gm.unit_of_handle(handle_arg(a, 0))?;
    let u = gm.unit(k);
    (is_hero_id(u.type_id) && !u.illusion).then_some(k)
}

/// Runs an attribute setter on argument 0 and applies the derived changes it causes (`docs/spec/heroes.md`, section
/// 10). `permanent` is the setter's boolean: it selects the life sink mode `2` (absolute) instead of `1`
/// (proportional), `mode = (permanent != 0) + 1` as the setter `0x6F209000` computes it (CONFIRMED).
///
/// The derived values are the readers' results before and after the change (`heroes::derived_bonuses`, factor `1.0`,
/// see `heroes::DerivedBonuses::hit_points`). The hit-point change goes through the life sink
/// (`heroes::life_sink`, CONFIRMED by Unicorn). The armor, regeneration, attack speed and attack bonuses are stored
/// on the unit as totals (section 10); the move bonus is added to the move speed. The mana change goes through the
/// mana sink (`heroes::mana_sink`, CONFIRMED by Unicorn), with the mana floor taken as zero (LIKELY).
fn with_attribute_change(g: &Rc<RefCell<Game>>, a: &[Value], permanent: bool, f: impl FnOnce(&mut HeroObj)) {
    let mut gm = g.borrow_mut();
    let Some(k) = hero_key(&gm, a) else {
        return;
    };
    let rates = *gm.hero_rates();
    let (before, after) = {
        let Some(h) = gm.unit_mut(k).hero.as_mut() else {
            return;
        };
        let one = softfloat::F32::ONE;
        let before = heroes::derived_bonuses(h, &rates, one);
        f(h);
        (before, heroes::derived_bonuses(h, &rates, one))
    };
    let mode = if permanent { 2 } else { 1 };
    let hp_delta = heroes::max_life_delta(&before, &after);
    let mana_delta = heroes::max_mana_delta(&before, &after);
    let mut mana_cells = gm.mana_cells(k);
    let move_delta = softfloat::arith::sub(after.move_speed, before.move_speed);
    let flagged = gm.unit(k).dead;
    let mut cells = gm.life_cells(k);
    heroes::life_sink(&mut cells, flagged, hp_delta, mode, gm.order_tables());
    heroes::mana_sink(&mut mana_cells, mana_delta, mode, gm.order_tables());
    gm.put_life_cells(k, cells);
    gm.put_mana_cells(k, mana_cells);
    // `0x6F209090`: the strength regeneration is a modifier of its own; it is rebuilt at every change.
    gm.set_hero_regen(k, after.regen);
    let u = gm.unit_mut(k);
    u.move_speed = softfloat::arith::add(u.move_speed, move_delta);
    u.hero_armor = after.armor;
    u.hero_regen = after.regen;
    u.hero_attack_speed = after.attack_speed;
    u.hero_attack = after.attack;
}

/// The hero state of argument 0, or `None` when the gate fails or the unit has no hero state.
fn hero_of<'a>(gm: &'a Game, a: &[Value]) -> Option<&'a HeroObj> {
    let k = hero_key(gm, a)?;
    gm.unit(k).hero.as_ref()
}

/// The handle id in argument `i`, or 0 (the null handle) when there is none.
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

/// The integer in argument `i`, or 0.
fn int_arg(a: &[Value], i: usize) -> i32 {
    match a.get(i) {
        Some(Value::Integer(n)) => *n,
        Some(Value::Boolean(b)) => i32::from(*b),
        _ => 0,
    }
}

/// The boolean in argument `i`, or false.
fn bool_arg(a: &[Value], i: usize) -> bool {
    match a.get(i) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(n)) => *n != 0,
        _ => false,
    }
}
