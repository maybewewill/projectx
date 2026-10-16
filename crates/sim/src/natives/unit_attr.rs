//! Unit attribute natives: the stored unit values (user data, vertex colour, time scale, scale, the creep guard and
//! exploded flags), move, turn and prop speeds, fly height, acquire range (each with its `GetUnitDefault*` type
//! value), point value and food, the unit tests (`IsUnit`, `IsUnitIllusion`, `IsUnitLoaded`,
//! `IsUnitOwnedByPlayer`, `IsUnitInForce`, the range tests, `IsHeroUnitId`), and the ability level natives
//! (`GetUnitAbilityLevel`, `SetUnitAbilityLevel`, `IncUnitAbilityLevel`, `DecUnitAbilityLevel`, `UnitAddAbility`,
//! `UnitRemoveAbility`, `UnitMakeAbilityPermanent`). It also holds [`NATIVES`], the table of every unit native
//! that is not in the other modules, with its support level.
//!
//! Behaviour notes. Addresses are the 1.26a `Game.dll` functions in `natives.c` (decompile export). Labels as in
//! `unit.rs`: CONFIRMED = read in the decompile, LIKELY = strong inference, UNSURE = guessed or not read.
//!
//! * Handle gate (CONFIRMED, every native): the unit handle resolves through the shared resolver, so a null,
//!   freed or removed unit gives the native's default. Invalid units return `0.0` for reals, `0` for integers and
//!   `false` for booleans (the `DAT_6FAAE470` default is `0.0`), and the unit natives do nothing.
//! * Type records (CONFIRMED offsets, the record lookup is the unit's type id): move speed `+0x1D4`, turn speed
//!   `+0x1E0`, prop window `+0x1E4`, fly height `+0x1F8`, acquire range `+0x194`, point value `+0x1D0`, food used
//!   `+0x5C`, food made `+0x60`. A missing record gives `0.0`, except turn speed and prop window, which give `0.5`
//!   (the constant `DAT_6FAAE4D0`). The type registry always has the record here, so that fallback is not reached.
//! * Turn speed and prop window getters read the unit object (`+0xB8`, `+0xBC`, CONFIRMED offsets). Their setters are
//!   virtual calls (vtable slots 48 and 49) whose bodies were not read, so the stored value is taken as given (UNSURE).
//! * Move speed setter (UNSURE): the original stores through the mover sub-object and may clamp. The value is
//!   stored as given. The model gives every unit a mover, and the native does nothing without one in the original.
//! * Fly height (LIKELY): a value that is not greater than zero is stored as `0.01` (constant `0x6FAAE480`, CONFIRMED
//!   value); a positive value is stored as given.
//! * Acquire range (LIKELY): the setter stores only a value greater than `50.0` (constant `0x6FAAE568`, CONFIRMED
//!   value). A smaller value is ignored.
//! * Vertex colour (CONFIRMED layout, LIKELY channel order): each channel is clamped to `0..=255` (values above 255
//!   and negative values, which are large unsigned, are clamped the same way), and the colour is stored as
//!   `0xAARRGGBB` in object `+0x2D4`.
//! * Scale (UNSURE): `SetUnitScale` reads only its first real (the body dereferences one pointer). Only that value
//!   is stored, and the second and third values are dropped.
//! * Invulnerable: `SetUnitInvulnerable` lives in `natives/ability.rs` (it adds or removes the ability `Avul`).
//! * Abilities (CONFIRMED structure, the model is `crate::abilities`): a unit's abilities are objects keyed by the
//!   base raw id. A level is stored as a zero-based index (`+0x50`); `GetUnitAbilityLevel` returns that plus one, or 0
//!   when the unit lacks the ability. The max level
//!   of an ability is its record `+0x3C` when the flag `+0x2C` is set, and 1 otherwise. Setting a level clamps the
//!   target to `[1, max]`. The level-up and level-down virtuals (slots 185 and 186) are not read, so their side
//!   effects (events, stat changes) are not modelled.
//! * `IncUnitAbilityLevel` (UNSURE): the decompile guards the increase with `level0 < max` (the zero-based level
//!   against the max), which can allow one level above the max. The guard is reproduced as read.
//! * `UnitAddAbility` (LIKELY): fails when the unit has the ability already, or when the id is not in the ability
//!   registry (the original's factory finds no record). A new ability is at level 1 with permanent count 0.
//! * `UnitMakeAbilityPermanent` (CONFIRMED): adds one to the permanent count (`+0x38`) for a true flag, and subtracts
//!   one otherwise, with no clamp. It returns true when the unit has the ability.
//! * Unit tests: `IsUnit` compares the two resolved objects, so two invalid handles compare equal (CONFIRMED from the
//!   decompile, which has no check). `IsUnitIllusion` and `IsUnitLoaded` read the flag bits of `+0x5C`. `IsUnitInForce`
//!   tests the force's player bit for the owner (LIKELY). The range tests use the circle test of `group.rs`
//!   ([`in_range`]), with the playable minimum as the origin (LIKELY: the original's range helper is the same one the
//!   group queries use).
//! * `SetUnitFog` (CONFIRMED): the body is empty, so the native does nothing.
//!
//! The tests are in `tests/unit_attr_natives.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, F32};

use super::game::{Game, HandleKind, ObjKey, Object, UnitType};
use crate::unitclass;
use super::group::in_range;
use super::hashtable::enum_at;
use super::hero::is_hero_id;
use super::player::raw_at;
use crate::abilities::{is_factory_code, Ability, RESCUE};

/// How a native is implemented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Read in the decompile, with the offsets and the control flow visible.
    Confirmed,
    /// Strong structural evidence; a detail is not fully read.
    Likely,
    /// Guessed from names or position, or part of the body was not read.
    Unsure,
    /// Not registered. The reason names the system that is missing or the part of the decompile not read.
    NotImplemented(&'static str),
}

/// One row of a native table: the JASS name, its support level and, for `NotImplemented`, the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeEntry {
    pub name: &'static str,
    pub support: Support,
}

/// Every unit native outside `unit.rs`, `unit_event.rs` and `hero.rs`, with its support level. The implemented ones
/// (every `Support` except `NotImplemented`) are registered by [`register`]. The not-implemented ones are the work
/// queue for the systems named in their reasons.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry {
        name: "DecUnitAbilityLevel",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitAbilityLevel",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetUnitAcquireRange",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitCurrentOrder",
        support: Support::NotImplemented(
            "N:order system (orders agent): the order queue is not modelled",
        ),
    },
    NativeEntry {
        name: "GetUnitDefaultAcquireRange",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitDefaultFlyHeight",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitDefaultMoveSpeed",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitDefaultPropWindow",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitDefaultTurnSpeed",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitFlyHeight",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "GetUnitFoodMade",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitFoodUsed",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitLevel",
        support: Support::NotImplemented(
            "N:non-hero path needs the type level record and the level cap FUN_6F009F90; the hero path is GetHeroLevel",
        ),
    },
    NativeEntry {
        name: "GetUnitMoveSpeed",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "GetUnitName",
        support: Support::NotImplemented(
            "N:display name is not in the unit type registry",
        ),
    },
    NativeEntry {
        name: "GetUnitPointValue",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitPointValueByType",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitPropWindow",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitRace",
        support: Support::NotImplemented(
            "N:race handles are not modelled (PlayerObj.race is a raw value)",
        ),
    },
    NativeEntry {
        name: "GetUnitRallyDestructable",
        support: Support::NotImplemented(
            "N:rally state is not modelled",
        ),
    },
    NativeEntry {
        name: "GetUnitRallyPoint",
        support: Support::NotImplemented(
            "N:rally state is not modelled",
        ),
    },
    NativeEntry {
        name: "GetUnitRallyUnit",
        support: Support::NotImplemented(
            "N:rally state is not modelled",
        ),
    },
    NativeEntry {
        name: "GetUnitTurnSpeed",
        support: Support::Likely,
    },
    NativeEntry {
        name: "GetUnitUserData",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IncUnitAbilityLevel",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "IsHeroUnitId",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsUnit",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsUnitAlly",
        support: Support::NotImplemented(
            "N:alliance type index read by the original (FUN_6F41B420) not recovered",
        ),
    },
    NativeEntry {
        name: "IsUnitDetected",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitEnemy",
        support: Support::NotImplemented(
            "N:built on IsUnitAlly",
        ),
    },
    NativeEntry {
        name: "IsUnitFogged",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitIdType",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitIllusion",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsUnitInForce",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitInRange",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitInRangeLoc",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitInRangeXY",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitInTransport",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitInvisible",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitLoaded",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsUnitMasked",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitOwnedByPlayer",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "IsUnitRace",
        support: Support::NotImplemented(
            "N:race handles are not modelled",
        ),
    },
    NativeEntry {
        name: "IsUnitSelected",
        support: Support::NotImplemented(
            "N:selection is not modelled",
        ),
    },
    NativeEntry {
        name: "IsUnitType",
        support: Support::Likely,
    },
    NativeEntry {
        name: "IsUnitVisible",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetUnitAbilityLevel",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetUnitAcquireRange",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetUnitCreepGuard",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetUnitExploded",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetUnitFlyHeight",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetUnitFog",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetUnitInvulnerable",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetUnitLookAt",
        support: Support::NotImplemented(
            "N:look-at is a turn and visual feature",
        ),
    },
    NativeEntry {
        name: "SetUnitMoveSpeed",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetUnitPathing",
        support: Support::NotImplemented(
            "N:the pathing blocker hook is not implemented",
        ),
    },
    NativeEntry {
        name: "SetUnitPropWindow",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetUnitRescuable",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetUnitRescueRange",
        support: Support::NotImplemented(
            "N:rescue range is not modelled",
        ),
    },
    NativeEntry {
        name: "SetUnitScale",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetUnitTimeScale",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetUnitTurnSpeed",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "SetUnitTypeSlots",
        support: Support::Likely,
    },
    NativeEntry {
        name: "SetUnitUseFood",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetUnitUserData",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "SetUnitVertexColor",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "UnitAddAbility",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitAddType",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "UnitApplyTimedLife",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitCountBuffsEx",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitHasBuffsEx",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitDamagePoint",
        support: Support::Unsure,
    },
    NativeEntry {
        name: "UnitDamageTarget",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitMakeAbilityPermanent",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "UnitPauseTimedLife",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitRemoveAbility",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitRemoveBuffs",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitRemoveBuffsEx",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitRemoveType",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "UnitResetCooldown",
        // CONFIRMED by disassembly (abilities-2.md section 1.2): the native walks the unit's ability list and
        // changes no ability field itself, but it calls 0x6F037E10 on spell abilities, which re-arms a running
        // cooldown to 0.01 s (abilities-5.md section 0). `Game::reset_cooldowns` does that.
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitSetConstructionProgress",
        support: Support::NotImplemented(
            "N:construction is not modelled",
        ),
    },
    NativeEntry {
        name: "UnitSetUpgradeProgress",
        support: Support::NotImplemented(
            "N:upgrades are not modelled",
        ),
    },
    NativeEntry {
        name: "UnitSetUsesAltIcon",
        support: Support::Confirmed,
    },
    NativeEntry {
        name: "UnitSuspendDecay",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitUseItem",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitUseItemPoint",
        support: Support::Likely,
    },
    NativeEntry {
        name: "UnitUseItemTarget",
        support: Support::Likely,
    },
];

/// `SetUnitAcquireRange` ignores a value at or below this (`0x6FAAE568`, CONFIRMED value 50.0).
const ACQUIRE_RANGE_MIN: F32 = F32::from_bits(0x4248_0000);
/// The fly height kept for a non-positive value (`0x6FAAE480`, CONFIRMED value 0.01).
const FLY_HEIGHT_FLOOR: F32 = F32::from_bits(0x3C23_D70A);
/// The default turn speed and prop window when the type record is missing (`0x6FAAE4D0`, CONFIRMED value 0.5).
const TURN_PROP_FALLBACK: F32 = F32::from_bits(0x3F00_0000);

/// Registers the implemented unit attribute natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("GetUnitUserData", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(0, |k| gm.unit(k).user_data);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitUserData", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).user_data = raw_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitVertexColor", move |_, a| {
        // (unit, red, green, blue, alpha)
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            let channel = |i: usize| raw_at(a, i).clamp(0, 255) as u32;
            let packed = (channel(4) << 24) | (channel(1) << 16) | (channel(2) << 8) | channel(3);
            gm.unit_mut(k).vertex_color = packed;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitTimeScale", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).time_scale = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitScale", move |_, a| {
        // Only the first scale value is read by the original (UNSURE, see the module notes).
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).scale = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitCreepGuard", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).creep_guard = bool_arg(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitExploded", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).exploded = bool_arg(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    // The body of `SetUnitFog` is empty in the original (CONFIRMED).
    reg.register("SetUnitFog", |_, _| NativeResult::Return(Value::Null));

    // `UnitDamageTarget(source, target, amount, attack, ranged, attackType, damageType, weaponType)` (`0x6F3C8850`,
    // CONFIRMED by disassembly). Order: the source unit resolves, the widget resolves, damage type is not above
    // 0x80000000 (unsigned), attack type is at most 7, weapon type at most 24; any failure returns false with no
    // effect. Then the attack record is built (amount, class `1 << damageType`, source, flags `0x100` if attack and
    // `|1` if ranged, weapon type) with the ATTACK TYPE ARGUMENT STORED IN THE RECORD'S ROW FIELD (`+0x20`; the
    // earlier spec reading "stored only in a local" was wrong: the local is the record's last dword), and the
    // target's slot `0x120` (the target side, `0x6F2A40D0` for units) runs. The native returns true. A destructable
    // or item widget is accepted but takes no damage here (UNSURE: their slot `0x120` bodies are not read).
    let g = Rc::clone(game);
    reg.register("UnitDamageTarget", move |ctx, a| {
        let (source, died) = {
            let mut gm = g.borrow_mut();
            let Some(source) = unit_at(&gm, a, 0) else {
                return NativeResult::Return(Value::Boolean(false));
            };
            let widget = handle_arg(a, 1);
            let target = gm.unit_of_handle(widget);
            let other = gm.destructable_of_handle(widget).is_some() || gm.item_of_handle(widget).is_some();
            if target.is_none() && !other {
                return NativeResult::Return(Value::Boolean(false));
            }
            let damage_type = super::hashtable::enum_at(a, 6) as u32;
            let attack_type = super::hashtable::enum_at(a, 5) as u32;
            let weapon_type = super::hashtable::enum_at(a, 7) as u32;
            if crate::damage::native_record(false, false, attack_type, damage_type, weapon_type).is_none() {
                return NativeResult::Return(Value::Boolean(false));
            }
            let died = target.and_then(|t| {
                gm.unit_damage_target(
                    source,
                    t,
                    super::hashtable::real_at(a, 2),
                    super::hashtable::bool_at(a, 3),
                    super::hashtable::bool_at(a, 4),
                    attack_type,
                    damage_type,
                )
            });
            (source, died)
        };
        if let Some(t) = died {
            super::unit_event::dispatch_unit_death_by(&g, ctx, t, Some(source));
        }
        NativeResult::Return(Value::Boolean(true))
    });

    // `UnitDamagePoint(source, delay, radius, x, y, amount, attack, ranged, attackType, damageType, weaponType)`
    // (`0x6F3C8770`, checks CONFIRMED): same argument checks as `UnitDamageTarget` (the source must resolve). A delay
    // that is not above 0.0 (compare `0x6F01FD60`: `0.0 >= delay`, NaN counts as above) damages at once through
    // `0x6F088050` -> `0x6F2C31D0`; otherwise a delayed object is built (`0x6F1D4440` -> `0x6F2C4D70`). The native
    // returns true. The membership test is UNSURE (see `combat.rs::apply_point_damage`).
    let g = Rc::clone(game);
    reg.register("UnitDamagePoint", move |ctx, a| {
        let damage_type = super::hashtable::enum_at(a, 9) as u32;
        let attack_type = super::hashtable::enum_at(a, 8) as u32;
        let weapon_type = super::hashtable::enum_at(a, 10) as u32;
        let (source, died) = {
            let mut gm = g.borrow_mut();
            let Some(source) = unit_at(&gm, a, 0) else {
                return NativeResult::Return(Value::Boolean(false));
            };
            if crate::damage::native_record(false, false, attack_type, damage_type, weapon_type).is_none() {
                return NativeResult::Return(Value::Boolean(false));
            }
            let delay = super::hashtable::real_at(a, 1);
            let pd = crate::combat::PointDamage {
                source,
                x: super::hashtable::real_at(a, 3),
                y: super::hashtable::real_at(a, 4),
                radius: super::hashtable::real_at(a, 2),
                amount: super::hashtable::real_at(a, 5),
                attack: super::hashtable::bool_at(a, 6),
                ranged: super::hashtable::bool_at(a, 7),
                attack_type,
                damage_type,
            };
            // Immediate when `0.0 >= delay` (ordered compare).
            if arith::le(delay, F32::ZERO) {
                (source, gm.apply_point_damage(&pd))
            } else {
                gm.schedule_point_damage(delay, pd);
                (source, Vec::new())
            }
        };
        for t in died {
            super::unit_event::dispatch_unit_death_by(&g, ctx, t, Some(source));
        }
        NativeResult::Return(Value::Boolean(true))
    });

    // `SetUnitRescuable(unit, player, flag)` (`0x6F3C5F10` -> `0x6F07B3E0`, CONFIRMED by disassembly): the state is a
    // per-player bit mask in the unit's `Arsc` ability (`+0x6C`, bit = the player's slot index). With the ability
    // present, a false flag clears the player's bit and a true flag does nothing (the true branch returns before
    // writing). Without it, a false flag does nothing and a true flag creates the ability and sets the bit. The
    // ability is built from a built-in definition, not from the registry. A null unit or player does nothing.
    let g = Rc::clone(game);
    reg.register("SetUnitRescuable", move |_, a| {
        let mut gm = g.borrow_mut();
        let (Some(k), Some(p)) = (
            unit_at(&gm, a, 0),
            gm.player_index_of_handle(handle_arg(a, 1)),
        ) else {
            return NativeResult::Return(Value::Null);
        };
        let flag = super::hashtable::bool_at(a, 2);
        let bit = 1u32 << (p & 31);
        let abilities = &mut gm.unit_mut(k).abilities;
        match abilities.get_mut(RESCUE) {
            Some(ab) => {
                if !flag {
                    ab.mask &= !bit;
                }
            }
            None => {
                if flag {
                    let mut ab = Ability::new(RESCUE, None);
                    ab.mask = bit;
                    abilities.attach(ab);
                }
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitFoodUsed", move |_, a| {
        let gm = g.borrow();
        let v = unit_type(&gm, a).map_or(0, |t| t.food_used);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitFoodMade", move |_, a| {
        let gm = g.borrow();
        let v = unit_type(&gm, a).map_or(0, |t| t.food_made);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitPointValue", move |_, a| {
        let gm = g.borrow();
        let v = unit_type(&gm, a).map_or(0, |t| t.point_value);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitPointValueByType", move |_, a| {
        let gm = g.borrow();
        let v = gm
            .unit_types()
            .get(raw_at(a, 0) as u32)
            .map_or(0, |t| t.point_value);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitMoveSpeed", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).move_speed);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitMoveSpeed", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).move_speed = real_at(a, 1);
            // The speed sink updates the mover's cap (0x6F4747B0, movement-6.md section 5).
            gm.mover_speed_changed(k);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitDefaultMoveSpeed", move |_, a| {
        let gm = g.borrow();
        let v = unit_type(&gm, a).map_or(F32::ZERO, |t| t.move_speed);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitTurnSpeed", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).turn_speed);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitTurnSpeed", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).turn_speed = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitDefaultTurnSpeed", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |_| {
            unit_type(&gm, a).map_or(TURN_PROP_FALLBACK, |t| t.turn_speed)
        });
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitPropWindow", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).prop_window);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitPropWindow", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).prop_window = real_at(a, 1);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitDefaultPropWindow", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |_| {
            unit_type(&gm, a).map_or(TURN_PROP_FALLBACK, |t| t.prop_window)
        });
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitFlyHeight", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).fly_height);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitFlyHeight", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            let h = real_at(a, 1);
            let rate = real_at(a, 2);
            let stored = if arith::gt(h, F32::ZERO) {
                h
            } else {
                FLY_HEIGHT_FLOOR
            };
            // The target field answers GetUnitFlyHeight at once; the property that the unit's height reads climbs at
            // `rate` (docs/spec/terrain.md section 6.3).
            gm.set_fly_height(k, stored, rate);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitDefaultFlyHeight", move |_, a| {
        let gm = g.borrow();
        let v = unit_type(&gm, a).map_or(F32::ZERO, |t| t.fly_height);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitAcquireRange", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).map_or(F32::ZERO, |k| gm.unit(k).acquire_range);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitAcquireRange", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            let r = real_at(a, 1);
            // 0x6F26D620: nothing happens without the attack ability (unit+0x1E8). Otherwise the
            // value is stored when the x87 compare `50.0 < r` sets C0, which an unordered (NaN)
            // compare also does, so NaN is stored and values <= 50.0 are ignored (disassembly).
            let has_attack = gm.unit(k).abilities.contains(crate::abilities::ATTACK);
            if has_attack && (r.to_f32().is_nan() || r.to_f32() > ACQUIRE_RANGE_MIN.to_f32()) {
                gm.unit_mut(k).acquire_range = r;
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetUnitDefaultAcquireRange", move |_, a| {
        let gm = g.borrow();
        let v = unit_type(&gm, a).map_or(F32::ZERO, |t| t.acquire_range);
        NativeResult::Return(Value::Real(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnit", move |_, a| {
        let gm = g.borrow();
        // Two invalid handles resolve to no object, and no object equals no object (CONFIRMED).
        let v = unit_at(&gm, a, 0) == unit_at(&gm, a, 1);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnitIllusion", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).is_some_and(|k| gm.unit(k).illusion);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnitLoaded", move |_, a| {
        let gm = g.borrow();
        let v = unit_at(&gm, a, 0).is_some_and(|k| gm.unit(k).loaded);
        NativeResult::Return(Value::Boolean(v))
    });

    // The three fog tests of a unit for a player (vision.md section 7): the state 4 (visible), 2 (fogged) or 1
    // (masked) of the unit's cell, or the reveal bit (`Game::unit_fog_test`, LIKELY).
    // `IsUnitVisible` also passes the invisibility gate `0x6F3A15F0` first (S44, `invisibility.rs`).
    for (name, state) in [("IsUnitVisible", 4), ("IsUnitFogged", 2), ("IsUnitMasked", 1)] {
        let g = Rc::clone(game);
        reg.register(name, move |_, a| {
            let gm = g.borrow();
            let v = match (unit_at(&gm, a, 0), gm.player_index_of_handle(handle_arg(a, 1))) {
                (Some(k), Some(p)) => {
                    (state != 4 || gm.invisibility_gate(k, p)) && gm.unit_fog_test(k, p, state)
                }
                _ => false,
            };
            NativeResult::Return(Value::Boolean(v))
        });
    }

    // `IsUnitInvisible(unit, player)`: slot `0xFC` mode 4 is false (invisible flag set and the gate refuses the player).
    let g = Rc::clone(game);
    reg.register("IsUnitInvisible", move |_, a| {
        let gm = g.borrow();
        let v = match (unit_at(&gm, a, 0), gm.player_index_of_handle(handle_arg(a, 1))) {
            (Some(k), Some(p)) => gm.is_unit_invisible_for(k, p),
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    // `IsUnitDetected(unit, player)` (`0x6F3C7C80`): the player is in the unit's detection mask (LIKELY).
    let g = Rc::clone(game);
    reg.register("IsUnitDetected", move |_, a| {
        let gm = g.borrow();
        let v = match (unit_at(&gm, a, 0), gm.player_index_of_handle(handle_arg(a, 1))) {
            (Some(k), Some(p)) => gm.detecting_players(k) & (1 << p) != 0,
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnitOwnedByPlayer", move |_, a| {
        let gm = g.borrow();
        let owner = gm.player_index_of_handle(handle_arg(a, 1));
        let v = match (unit_at(&gm, a, 0), owner) {
            (Some(k), Some(p)) => gm.unit(k).owner == p,
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnitInForce", move |_, a| {
        let gm = g.borrow();
        // The force holds one bit per player slot (LIKELY: the original's membership test for the owner).
        let v = match (unit_at(&gm, a, 0), gm.force_of_handle(handle_arg(a, 1))) {
            (Some(k), Some(f)) => {
                let owner = gm.unit(k).owner;
                owner < 32 && (gm.force(f).members >> owner) & 1 == 1
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnitInRange", move |_, a| {
        let gm = g.borrow();
        let v = match (unit_at(&gm, a, 0), unit_at(&gm, a, 1)) {
            (Some(u), Some(t)) => {
                let (ux, uy) = (gm.unit(u).x, gm.unit(u).y);
                let (tx, ty) = (gm.unit(t).x, gm.unit(t).y);
                in_range(tx, ty, ux, uy, real_at(a, 2), origin(&gm))
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnitInRangeXY", move |_, a| {
        let gm = g.borrow();
        let v = match unit_at(&gm, a, 0) {
            Some(u) => {
                let (ux, uy) = (gm.unit(u).x, gm.unit(u).y);
                in_range(
                    ux,
                    uy,
                    real_at(a, 1),
                    real_at(a, 2),
                    real_at(a, 3),
                    origin(&gm),
                )
            }
            None => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("IsUnitInRangeLoc", move |_, a| {
        let gm = g.borrow();
        let v = match (unit_at(&gm, a, 0), loc_at(&gm, a, 1)) {
            (Some(u), Some((lx, ly))) => {
                let (ux, uy) = (gm.unit(u).x, gm.unit(u).y);
                in_range(ux, uy, lx, ly, real_at(a, 2), origin(&gm))
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(v))
    });

    // Unit classification (docs/spec/unit-classification.md). `IsUnitType`: the value is the ConvertUnitType index,
    // 0 to 26; anything else, and an invalid unit, gives false (CONFIRMED: the switch is range-checked, `0x6F3C7E90`).
    let g = Rc::clone(game);
    reg.register("IsUnitType", move |_, a| {
        let gm = g.borrow();
        let t = enum_at(a, 1) as u32;
        let v = unit_at(&gm, a, 0).is_some_and(|k| is_unit_type(&gm, gm.unit(k), t));
        NativeResult::Return(Value::Boolean(v))
    });

    // `UnitAddType` / `UnitRemoveType`: change the unit's classification word; true only for the nine values that
    // have a bit (CONFIRMED, `0x6F3C7BC0` / `0x6F3C7C20`).
    for (name, add) in [("UnitAddType", true), ("UnitRemoveType", false)] {
        let g = Rc::clone(game);
        reg.register(name, move |_, a| {
            let mut gm = g.borrow_mut();
            let t = enum_at(a, 1) as u32;
            let v = match unit_at(&gm, a, 0) {
                Some(k) => {
                    let w = &mut gm.unit_mut(k).class_flags;
                    if add {
                        unitclass::add_type(w, t)
                    } else {
                        unitclass::remove_type(w, t)
                    }
                }
                None => false,
            };
            NativeResult::Return(Value::Boolean(v))
        });
    }

    // `IsUnitIdType(id, unittype)`: the same classes read from the type record (CONFIRMED, `0x6F3B7F60`).
    let g = Rc::clone(game);
    reg.register("IsUnitIdType", move |_, a| {
        let gm = g.borrow();
        let id = raw_at(a, 0) as u32;
        let t = enum_at(a, 1) as u32;
        let view = gm.unit_types().get(id).map_or_else(unitclass::IdView::default, |ut| unitclass::IdView {
            structure: gm.unit_types().is_building(id),
            movement: ut.movement,
            classification: ut.classification,
            weapons_on: ut.weapons_on,
            kind: ut.weapon_kind,
            targs: ut.weapon_targs,
        });
        let v = t < unitclass::ty::COUNT && unitclass::id_type_test(id, &view, t);
        NativeResult::Return(Value::Boolean(v))
    });

    // `IsUnitInTransport(unit, transport)`: the cargo relation (`0x6F079F80`) needs a transport system, which the model
    // does not have, so no unit is ever inside another (LIKELY; invalid handles give false, CONFIRMED).
    reg.register("IsUnitInTransport", |_, _| NativeResult::Return(Value::Boolean(false)));

    // `UnitSetUsesAltIcon` (CONFIRMED, `0x6F3CB860`): sets or clears bit 0x200 of object `+0x280`.
    let g = Rc::clone(game);
    reg.register("UnitSetUsesAltIcon", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_at(&gm, a, 0) {
            gm.unit_mut(k).alt_icon = matches!(a.get(1), Some(Value::Boolean(true)));
        }
        NativeResult::Return(Value::Null)
    });

    reg.register("IsHeroUnitId", |_, a| {
        let v = is_hero_id(raw_at(a, 0) as u32);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitAbilityLevel", move |_, a| {
        let gm = g.borrow();
        let id = gm.spell_alias_base(raw_at(a, 1) as u32);
        let v = unit_at(&gm, a, 0)
            .and_then(|k| gm.unit(k).abilities.get(id))
            .map_or(0, Ability::level);
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SetUnitAbilityLevel", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 1) as u32;
        let k = unit_at(&gm, a, 0);
        let requested = raw_at(a, 2);
        let v = with_ability(&mut gm, k, id, |ab| ab.set_level(requested));
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("IncUnitAbilityLevel", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 1) as u32;
        let k = unit_at(&gm, a, 0);
        let v = with_ability(&mut gm, k, id, |ab| {
            ab.inc_level();
            ab.level()
        });
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("DecUnitAbilityLevel", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 1) as u32;
        let k = unit_at(&gm, a, 0);
        let v = with_ability(&mut gm, k, id, |ab| {
            ab.dec_level();
            ab.level()
        });
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("UnitAddAbility", move |_, a| {
        let mut gm = g.borrow_mut();
        let row = raw_at(a, 1) as u32;
        // A creep / neutral alias row is attached as the modelled ability it stands for (`neutral_spells.rs`).
        let id = gm.spell_alias_base(row);
        let definition = gm.ability_types().get(row).copied();
        let added = match unit_at(&gm, a, 0) {
            Some(k)
                if definition.is_some()
                    && is_factory_code(id)
                    && !gm.unit(k).abilities.contains(id) =>
            {
                gm.unit_mut(k)
                    .abilities
                    .attach(Ability::aliased(id, row, definition));
                gm.sync_auras(k);
                true
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(added))
    });

    let g = Rc::clone(game);
    reg.register("UnitRemoveAbility", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = gm.spell_alias_base(raw_at(a, 1) as u32);
        let removed = match unit_at(&gm, a, 0) {
            Some(k) => {
                let removed = gm.unit_mut(k).abilities.remove(id).is_some();
                if removed {
                    gm.sync_auras(k);
                }
                removed
            }
            None => false,
        };
        NativeResult::Return(Value::Boolean(removed))
    });

    let g = Rc::clone(game);
    reg.register("UnitMakeAbilityPermanent", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 2) as u32;
        let permanent = bool_arg(a, 1);
        let done = match unit_at(&gm, a, 0) {
            Some(k) => match gm.unit_mut(k).abilities.get_mut(id) {
                Some(ab) => {
                    ab.adjust_permanent(permanent);
                    true
                }
                None => false,
            },
            None => false,
        };
        NativeResult::Return(Value::Boolean(done))
    });
}

/// Runs `f` on ability `id` of unit `k`, when the unit is live and has the ability. Returns the result of `f`, or 0
/// when there is no such ability.
fn with_ability(
    gm: &mut Game,
    k: Option<ObjKey>,
    id: u32,
    f: impl FnOnce(&mut Ability) -> i32,
) -> i32 {
    match k.and_then(|k| gm.unit_mut(k).abilities.get_mut(id)) {
        Some(ab) => f(ab),
        None => 0,
    }
}

/// The range origin: the playable minimum, or zero without a playable area (LIKELY, see the notes).
fn origin(gm: &Game) -> (F32, F32) {
    gm.playable()
        .map_or((F32::ZERO, F32::ZERO), |p| (p.min_x, p.min_y))
}

/// The type record of the unit in argument 0, if the unit is live (the record lookup is by the unit's type id).
fn unit_type<'a>(gm: &'a Game, a: &[Value]) -> Option<&'a UnitType> {
    let k = unit_at(gm, a, 0)?;
    gm.unit_types().get(gm.unit(k).type_id)
}

/// The unit in argument `i`, if it is a live unit.
pub(crate) fn unit_at(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.unit_of_handle(handle_arg(a, i))
}

/// The location in argument `i` as its coordinates, if it is a live location.
fn loc_at(gm: &Game, a: &[Value], i: usize) -> Option<(F32, F32)> {
    let k = gm.handle_of_kind(handle_arg(a, i), HandleKind::Location)?;
    match gm.object(k) {
        Object::Location(l) => Some((l.x, l.y)),
        _ => None,
    }
}

/// The handle id in argument `i`, or 0 (the null handle) when there is none.
fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

/// `IsUnitType` on a resolved unit (`0x6F3C7E90`). See `crate::unitclass` for the per-value rules.
pub fn is_unit_type(gm: &Game, u: &super::game::UnitObj, t: u32) -> bool {
    use crate::unitclass as uc;
    if t >= uc::ty::COUNT {
        return false;
    }
    let ut = gm.unit_types().get(u.type_id);
    let view = uc::UnitView {
        type_id: u.type_id,
        illusion: u.illusion,
        dead: u.dead,
        structure: gm.unit_types().is_building(u.type_id),
        movement: ut.map_or(0, |t| t.movement),
        class_flags: u.class_flags,
        ethereal: u.ethereal,
        magic_immune: u.magic_immune,
    };
    if let Some(v) = uc::simple_test(&view, t) {
        return v;
    }
    match t {
        uc::ty::ATTACKS_FLYING | uc::ty::ATTACKS_GROUND | uc::ty::MELEE_ATTACKER | uc::ty::RANGED_ATTACKER => {
            // The attack ability holds the weapon data copied from the type record (LIKELY: same values). A weapon
            // is enabled by its `weapsOn` bit; the disabling counters have no writer in the model.
            let w = u.abilities.get(crate::abilities::ATTACK).and(ut).map(|t| uc::WeaponView {
                kind: t.weapon_kind,
                targs: t.weapon_targs,
                enabled: [t.weapons_on & 1 != 0, t.weapons_on & 2 != 0],
            });
            uc::attack_test(t, w.as_ref())
        }
        _ => {
            u.buffs.iter().any(|b| uc::code_gives_state(t, b.base))
                || u.abilities.iter().any(|a| uc::code_gives_state(t, a.base))
        }
    }
}

/// The real in argument `i`, or 0.0.
pub(crate) fn real_at(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

/// The boolean in argument `i`, or false.
pub(crate) fn bool_arg(a: &[Value], i: usize) -> bool {
    match a.get(i) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(n)) => *n != 0,
        _ => false,
    }
}
