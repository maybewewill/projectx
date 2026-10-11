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
use super::group::in_range;
use super::hero::is_hero_id;
use super::player::raw_at;
use crate::abilities::{is_factory_code, Ability};

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
        support: Support::NotImplemented(
            "N:detection is not modelled",
        ),
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
        support: Support::NotImplemented(
            "N:type-class switch needs the type flag words (FUN_6F29CB80 and helpers), not read",
        ),
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
        support: Support::NotImplemented(
            "N:transport relation (FUN_6F079F80) is not modelled",
        ),
    },
    NativeEntry {
        name: "IsUnitInvisible",
        support: Support::NotImplemented(
            "N:invisibility is not modelled",
        ),
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
        support: Support::NotImplemented(
            "N:type-class switch needs the type flag words (FUN_6F3B2EB0 mapping), not recovered",
        ),
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
        name: "SetUnitAnimation",
        support: Support::NotImplemented(
            "N:animation system is not modelled",
        ),
    },
    NativeEntry {
        name: "SetUnitAnimationByIndex",
        support: Support::NotImplemented(
            "N:animation system is not modelled",
        ),
    },
    NativeEntry {
        name: "SetUnitAnimationWithRarity",
        support: Support::NotImplemented(
            "N:animation system is not modelled",
        ),
    },
    NativeEntry {
        name: "SetUnitBlendTime",
        support: Support::NotImplemented(
            "N:animation blend is not modelled",
        ),
    },
    NativeEntry {
        name: "SetUnitColor",
        support: Support::NotImplemented(
            "N:player colour and visuals are not modelled",
        ),
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
        name: "SetUnitFacingTimed",
        support: Support::NotImplemented(
            "N:timed turn is the turn system (movement agent)",
        ),
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
        support: Support::NotImplemented(
            "N:per-player rescuable state is not modelled",
        ),
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
        support: Support::NotImplemented(
            "N:supply slot accounting is not modelled",
        ),
    },
    NativeEntry {
        name: "SetUnitUseFood",
        support: Support::NotImplemented(
            "N:player food accounting is not modelled",
        ),
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
        name: "UnitAddIndicator",
        support: Support::NotImplemented(
            "N:indicator visual is not modelled",
        ),
    },
    NativeEntry {
        name: "UnitAddType",
        support: Support::NotImplemented(
            "N:type flag word mapping (FUN_6F3B2EB0) not recovered",
        ),
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
        support: Support::NotImplemented(
            "N:combat (attack-damage spec) is not modelled",
        ),
    },
    NativeEntry {
        name: "UnitDamageTarget",
        support: Support::NotImplemented(
            "N:combat (attack-damage spec) is not modelled",
        ),
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
        support: Support::NotImplemented(
            "N:type flag word mapping (FUN_6F3B2EB0) not recovered",
        ),
    },
    NativeEntry {
        name: "UnitResetCooldown",
        // CONFIRMED by disassembly (abilities-2.md section 1.2): the native walks the unit's ability list and
        // changes no ability field; its only other call is the local-player UI refresh. The simulation has no
        // cooldowns to reset, so the registered native is a no-op.
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
        support: Support::NotImplemented(
            "N:the icon state is not modelled",
        ),
    },
    NativeEntry {
        name: "UnitShareVision",
        support: Support::NotImplemented(
            "N:vision and fog are not modelled",
        ),
    },
    NativeEntry {
        name: "UnitSuspendDecay",
        support: Support::NotImplemented(
            "N:corpse decay is not modelled",
        ),
    },
    NativeEntry {
        name: "UnitUseItem",
        support: Support::NotImplemented(
            "N:item orders (orders agent)",
        ),
    },
    NativeEntry {
        name: "UnitUseItemPoint",
        support: Support::NotImplemented(
            "N:item orders (orders agent)",
        ),
    },
    NativeEntry {
        name: "UnitUseItemTarget",
        support: Support::NotImplemented(
            "N:item orders (orders agent)",
        ),
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
            let stored = if arith::gt(h, F32::ZERO) {
                h
            } else {
                FLY_HEIGHT_FLOOR
            };
            gm.unit_mut(k).fly_height = stored;
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
    for (name, state) in [("IsUnitVisible", 4), ("IsUnitFogged", 2), ("IsUnitMasked", 1)] {
        let g = Rc::clone(game);
        reg.register(name, move |_, a| {
            let gm = g.borrow();
            let v = match (unit_at(&gm, a, 0), gm.player_index_of_handle(handle_arg(a, 1))) {
                (Some(k), Some(p)) => gm.unit_fog_test(k, p, state),
                _ => false,
            };
            NativeResult::Return(Value::Boolean(v))
        });
    }

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

    reg.register("IsHeroUnitId", |_, a| {
        let v = is_hero_id(raw_at(a, 0) as u32);
        NativeResult::Return(Value::Boolean(v))
    });

    let g = Rc::clone(game);
    reg.register("GetUnitAbilityLevel", move |_, a| {
        let gm = g.borrow();
        let id = raw_at(a, 1) as u32;
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
        let id = raw_at(a, 1) as u32;
        // The factory needs a definition record, and `Amov` / `Aatk` have no factory leaf (see `crate::abilities`).
        let definition = gm.ability_types().get(id).copied();
        let added = match unit_at(&gm, a, 0) {
            Some(k)
                if definition.is_some()
                    && is_factory_code(id)
                    && !gm.unit(k).abilities.contains(id) =>
            {
                gm.unit_mut(k)
                    .abilities
                    .attach(Ability::new(id, definition));
                true
            }
            _ => false,
        };
        NativeResult::Return(Value::Boolean(added))
    });

    let g = Rc::clone(game);
    reg.register("UnitRemoveAbility", move |_, a| {
        let mut gm = g.borrow_mut();
        let id = raw_at(a, 1) as u32;
        let removed = match unit_at(&gm, a, 0) {
            Some(k) => gm.unit_mut(k).abilities.remove(id).is_some(),
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
