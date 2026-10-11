//! Hero natives: the hero experience, level and skill point getters, `SuspendHeroXP`, and the table of every
//! hero native (implemented or not). The model is in `natives/game.rs` ([`HeroObj`]).
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions (`natives.c` bodies in the decompile export,
//! the helpers in `chunk_010.c`, `chunk_012.c` and `chunk_013.c`). Labels as in `unit.rs`: CONFIRMED = read in the
//! decompile, LIKELY = strong inference, UNSURE = guessed or not read.
//!
//! * Hero gate (CONFIRMED, every hero native): the unit must be live, its type id must start with an uppercase
//!   letter (`IsHeroUnitId`, the top byte of the four-character code in `'A'..='Z'`), and the unit must not be an
//!   illusion (object `+0x5C` bit 30). Otherwise the native returns its default and does nothing.
//! * Experience (CONFIRMED offset, LIKELY meaning): `GetHeroXP` reads the hero sub-object at `+0x8C`.
//! * Skill points (CONFIRMED offset): `GetHeroSkillPoints` reads the hero sub-object at `+0x90`.
//! * `SuspendHeroXP` (CONFIRMED store into the hero sub-object at `+0xEC`). What the flag stops is not read.
//! * Level (UNSURE): the hero level is read through the unit-level counter helpers (`FUN_6F473170`, plus one). The
//!   model stores the level directly and starts a hero at 1.
//!
//! Not implemented (see [`NATIVES`]): the experience curve, the level-up and level-down engine (`FUN_6F20B080`,
//! `FUN_6F20B4D0`), the stat formulas (they use the softfloat helpers with an x87-style calling convention and the
//! item bonus helpers), the revive path, skill learning, and the proper-name table. Each of these needs more of
//! the decompile read before its behaviour can be matched. The XP curve is game data (TODO(gamedata)).

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};

use super::game::{Game, HeroObj, ObjKey};
use super::unit_attr::{NativeEntry, Support};

/// Every hero native with its support level. The ones marked `NotImplemented` are not registered: a script that
/// calls one fails to bind (see `unbound_natives` in the VM), which is the work queue for the hero system.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "AddHeroXP",
        support: Support::NotImplemented(
            "N:experience gain runs the level-up engine (FUN_6F20B080, FUN_6F20B4D0), not read",
        ),
    },
    NativeEntry {
        name: "GetHeroAgi",
        support: Support::NotImplemented(
            "N:stat formula: softfloat helpers with an x87-style calling convention and item bonus helpers not read",
        ),
    },
    NativeEntry {
        name: "GetHeroInt",
        support: Support::NotImplemented(
            "N:stat formula not read (see GetHeroAgi)",
        ),
    },
    NativeEntry {
        name: "GetHeroLevel",
        support: Support::Unsure,
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
        support: Support::NotImplemented(
            "N:stat formula not read (see GetHeroAgi)",
        ),
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
        support: Support::NotImplemented(
            "N:stat set path (FUN_6F20A1E0, FUN_6F20A260, FUN_6F20A160) not read",
        ),
    },
    NativeEntry {
        name: "SetHeroInt",
        support: Support::NotImplemented(
            "N:stat set path not read (see SetHeroAgi)",
        ),
    },
    NativeEntry {
        name: "SetHeroLevel",
        support: Support::NotImplemented(
            "N:needs the experience curve (game data) and the level-up engine",
        ),
    },
    NativeEntry {
        name: "SetHeroStr",
        support: Support::NotImplemented(
            "N:stat set path not read (see SetHeroAgi)",
        ),
    },
    NativeEntry {
        name: "SetHeroXP",
        support: Support::NotImplemented(
            "N:needs the level-up engine (FUN_6F20B080) and the experience curve",
        ),
    },
    NativeEntry {
        name: "SuspendHeroXP",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitModifySkillPoints",
        support: Support::NotImplemented(
            "N:skill bookkeeping (FUN_6F208B80) only partly read; the skill slots are not modelled",
        ),
    },
    NativeEntry {
        name: "UnitStripHeroLevel",
        support: Support::NotImplemented(
            "N:level-down loop (FUN_6F20B6E0, FUN_6F20B4D0) not read",
        ),
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

/// The boolean in argument `i`, or false.
fn bool_arg(a: &[Value], i: usize) -> bool {
    match a.get(i) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(n)) => *n != 0,
        _ => false,
    }
}
