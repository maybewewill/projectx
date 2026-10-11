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
//!   count as 0. `GetHeroInt` / `SetHeroInt` are not implemented (see [`NATIVES`]).

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
        support: Support::NotImplemented(
            "N:the intelligence getter adds the level counter of the unit object (not the hero's) to a gain term; which field is its base is not settled (heroes.md section 4)",
        ),
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
        support: Support::NotImplemented(
            "N:skill learning (FUN_6F2095D0) not read",
        ),
    },
    NativeEntry {
        name: "SetHeroAgi",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetHeroInt",
        support: Support::NotImplemented(
            "N:the setter moves the intelligence base, which depends on the open question of GetHeroInt",
        ),
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
        with_hero(&g, a, |t, h| heroes::add_xp(t, h, conv::i2r(int_arg(a, 1)), None));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetHeroXP", move |_, a| {
        with_hero(&g, a, |t, h| heroes::set_xp(t, h, int_arg(a, 1), None));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetHeroLevel", move |_, a| {
        with_hero(&g, a, |t, h| heroes::set_level(t, h, int_arg(a, 1), None));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitStripHeroLevel", move |_, a| {
        let mut removed = false;
        with_hero(&g, a, |t, h| removed = heroes::strip_levels(t, h, int_arg(a, 1) as u32));
        NativeResult::Return(Value::Boolean(removed))
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
    reg.register("SetHeroStr", move |_, a| {
        let value = int_arg(a, 1);
        with_hero_only(&g, a, |h| h.str_base = heroes::rebase(h.str_base, h.str_plus, h.level - 1, value));
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetHeroAgi", move |_, a| {
        let value = int_arg(a, 1);
        with_hero_only(&g, a, |h| h.agi_base = heroes::rebase(h.agi_base, h.agi_plus, h.level - 1, value));
        NativeResult::Return(Value::Null)
    });
}

/// Runs `f` on the experience tables and the hero state of argument 0 when the hero gate passes.
fn with_hero(g: &Rc<RefCell<Game>>, a: &[Value], f: impl FnOnce(&heroes::XpTables, &mut HeroObj)) {
    let mut gm = g.borrow_mut();
    let Some(k) = hero_key(&gm, a) else {
        return;
    };
    let tables = gm.hero_xp().clone();
    if let Some(h) = gm.unit_mut(k).hero.as_mut() {
        f(&tables, h);
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
