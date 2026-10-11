//! Unit attribute natives through `ScriptHost` (`natives/unit_attr.rs`, the model in `natives/game.rs`).
//!
//! The scripts use integer-valued reals (`I2R`), so they need no lookup tables. Exact bit patterns are read back
//! from the globals after the run. Stored values that the scripts cannot read (vertex colour, scale, flags) are
//! checked on the model through `Game::unit`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{AbilityType, UnitType};
use sim::natives::hero::NATIVES as HERO_NATIVES;
use sim::natives::unit_attr::{Support, NATIVES as UNIT_NATIVES};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type location extends handle
type force extends handle

native I2R takes integer i returns real
native R2I takes real r returns integer
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing
native CreateForce takes nothing returns force
native ForceAddPlayer takes force whichForce, player whichPlayer returns nothing
native IsUnitInForce takes unit whichUnit, force whichForce returns boolean
native GetUnitTypeId takes unit whichUnit returns integer
native SetUnitMoveSpeed takes unit whichUnit, real newSpeed returns nothing
native GetUnitMoveSpeed takes unit whichUnit returns real
native GetUnitDefaultMoveSpeed takes unit whichUnit returns real
native SetUnitTurnSpeed takes unit whichUnit, real turnSpeed returns nothing
native GetUnitTurnSpeed takes unit whichUnit returns real
native GetUnitDefaultTurnSpeed takes unit whichUnit returns real
native SetUnitPropWindow takes unit whichUnit, real propWindow returns nothing
native GetUnitPropWindow takes unit whichUnit returns real
native GetUnitDefaultPropWindow takes unit whichUnit returns real
native SetUnitFlyHeight takes unit whichUnit, real newHeight, real rate returns nothing
native GetUnitFlyHeight takes unit whichUnit returns real
native GetUnitDefaultFlyHeight takes unit whichUnit returns real
native SetUnitAcquireRange takes unit whichUnit, real newRange returns nothing
native GetUnitAcquireRange takes unit whichUnit returns real
native GetUnitDefaultAcquireRange takes unit whichUnit returns real
native SetUnitTimeScale takes unit whichUnit, real timeScale returns nothing
native SetUnitScale takes unit whichUnit, real x, real y, real z returns nothing
native SetUnitVertexColor takes unit whichUnit, integer red, integer green, integer blue, integer alpha returns nothing
native SetUnitUserData takes unit whichUnit, integer data returns nothing
native GetUnitUserData takes unit whichUnit returns integer
native SetUnitCreepGuard takes unit whichUnit, boolean creepGuard returns nothing
native SetUnitExploded takes unit whichUnit, boolean exploded returns nothing
native SetUnitFog takes real a, real b, real c, real d, real e returns nothing
native GetUnitPointValue takes unit whichUnit returns integer
native GetUnitPointValueByType takes integer unitType returns integer
native GetUnitFoodUsed takes unit whichUnit returns integer
native GetUnitFoodMade takes unit whichUnit returns integer
native IsUnit takes unit whichUnit, unit otherUnit returns boolean
native IsUnitIllusion takes unit whichUnit returns boolean
native IsUnitLoaded takes unit whichUnit returns boolean
native IsUnitOwnedByPlayer takes unit whichUnit, player whichPlayer returns boolean
native IsUnitInRange takes unit whichUnit, unit otherUnit, real distance returns boolean
native IsUnitInRangeXY takes unit whichUnit, real x, real y, real distance returns boolean
native IsHeroUnitId takes integer unitId returns boolean
native GetUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer
native SetUnitAbilityLevel takes unit whichUnit, integer abilcode, integer level returns integer
native IncUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer
native DecUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
native UnitMakeAbilityPermanent takes unit whichUnit, boolean permanent, integer abilityId returns boolean
"#;

/// Footman: the type record values the tests read back (speeds in bits, see `ut`).
const FOOTMAN: u32 = 0x6866_6F6F; // 'hfoo'
/// Hero type 'Hpal'.
const HERO: u32 = 0x4870_616C;
/// Ability with a max level of 3.
const ABIL_MAX3: u32 = 0x4148_627A; // 'AHbz'
/// Ability with no max-level data (max 1).
const ABIL_NOMAX: u32 = 0x4148_7678; // 'AHvx'
/// Ability granted by the type 'hfoo' at creation.
const ABIL_GRANT: u32 = 0x4148_6772; // 'AHgr'

fn footman_type() -> UnitType {
    UnitType {
        name: "Footman".to_string(),
        max_life: F32::from_bits(0x43D2_0000),
        move_speed: F32::from_bits(0x4387_0000),  // 270.0
        turn_speed: F32::from_bits(0x3F80_0000),  // 1.0
        prop_window: F32::from_bits(0x4180_0000), // 16.0
        fly_height: F32::from_bits(0x0000_0000),
        acquire_range: F32::from_bits(0x4416_0000), // 600.0
        point_value: 5,
        food_used: 2,
        food_made: 0,
        abilities: vec![ABIL_GRANT],
        ..Default::default()
    }
}

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(FOOTMAN, footman_type());
        g.unit_types_mut().insert(
            HERO,
            UnitType {
                name: "Paladin".to_string(),
                max_life: F32::from_bits(0x4416_0000),
                ..Default::default()
            },
        );
        g.ability_types_mut()
            .insert(ABIL_MAX3, AbilityType { max_level: Some(3) });
        g.ability_types_mut()
            .insert(ABIL_NOMAX, AbilityType { max_level: None });
        g.ability_types_mut()
            .insert(ABIL_GRANT, AbilityType { max_level: None });
    }
    host
}

fn run(host: &mut ScriptHost) {
    host.start("main", &[]).expect("main starts");
    for _ in 0..4 {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| v.as_boolean())
        .unwrap_or_else(|| panic!("global {name} is not a boolean"))
}

fn real_bits(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_real())
        .unwrap_or_else(|| panic!("global {name} is not a real"))
        .to_bits()
}

/// The model unit behind the global handle `name` (the unit must be live).
fn model_unit(host: &ScriptHost, name: &str) -> sim::natives::game::UnitObj {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .expect("unit handle");
    let g = host.game();
    let k = g.unit_of_handle(id).expect("live unit");
    g.unit(k).clone()
}

const GLOBALS: &str = r#"
globals
unit u1
unit u2
unit u3
unit nul
force f1
integer ix = -1
integer ix2 = -1
integer ix6 = -1
integer ix7 = -1
integer ix8 = -1
integer ix9 = -1
integer ix10 = -1
integer ix3 = -1
integer ix4 = -1
integer ix5 = -1
real r1 = 0.0
real r2 = 0.0
real r3 = 0.0
real r4 = 0.0
real r5 = 0.0
real r6 = 0.0
real r7 = 0.0
real r8 = 0.0
real r9 = 0.0
boolean b1 = false
boolean b2 = false
boolean b3 = false
boolean b4 = false
boolean b5 = false
boolean b6 = false
boolean b7 = false
boolean b8 = false
endglobals
"#;

#[test]
fn speed_fly_and_acquire_round_trip_with_exact_bits() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set r1 = GetUnitMoveSpeed(u1)
    set r2 = GetUnitDefaultMoveSpeed(u1)
    call SetUnitMoveSpeed(u1, I2R(300))
    set r3 = GetUnitMoveSpeed(u1)
    call SetUnitTurnSpeed(u1, I2R(2))
    set r4 = GetUnitTurnSpeed(u1)
    set r5 = GetUnitDefaultTurnSpeed(u1)
    call SetUnitPropWindow(u1, I2R(7))
    set r6 = GetUnitPropWindow(u1)
    call SetUnitFlyHeight(u1, I2R(-5), I2R(0))
    set r7 = GetUnitFlyHeight(u1)
    call SetUnitFlyHeight(u1, I2R(7), I2R(0))
    set r8 = GetUnitFlyHeight(u1)
    set r9 = GetUnitDefaultFlyHeight(u1)
endfunction
"
    ));
    run(&mut host);
    // Default move speed comes from the type; a set value is stored as given.
    assert_eq!(real_bits(&host, "r1"), 0x4387_0000);
    assert_eq!(real_bits(&host, "r2"), 0x4387_0000);
    assert_eq!(real_bits(&host, "r3"), 0x4396_0000); // 300.0
    assert_eq!(real_bits(&host, "r4"), 0x4000_0000); // 2.0
    assert_eq!(real_bits(&host, "r5"), 0x3F80_0000); // type 1.0
    assert_eq!(real_bits(&host, "r6"), 0x40E0_0000); // 7.0
    assert_eq!(
        real_bits(&host, "r7"),
        0x3C23_D70A,
        "non-positive fly height keeps 0.01"
    );
    assert_eq!(real_bits(&host, "r8"), 0x40E0_0000); // 7.0
    assert_eq!(real_bits(&host, "r9"), 0);
}

#[test]
fn acquire_range_is_stored_only_above_fifty() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitAcquireRange(u1, I2R(40))
    set r1 = GetUnitAcquireRange(u1)
    call SetUnitAcquireRange(u1, I2R(50))
    set r2 = GetUnitAcquireRange(u1)
    call SetUnitAcquireRange(u1, I2R(51))
    set r3 = GetUnitAcquireRange(u1)
    set r4 = GetUnitAcquireRange(nul)
    set u2 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    call SetUnitAcquireRange(u2, I2R(500))
    set r5 = GetUnitAcquireRange(u2)
endfunction
"
    ));
    // 0x6F26D620 acts only on units with the attack ability (unit+0x1E8): the footman has it, the
    // hero type here does not.
    host.game_mut().set_type_orders(
        FOOTMAN,
        TypeOrderData {
            abilities: vec![AbilityKind::Attack],
        },
    );
    run(&mut host);
    assert_eq!(
        real_bits(&host, "r1"),
        0x4416_0000,
        "40 is ignored: the type default stays"
    );
    assert_eq!(real_bits(&host, "r2"), 0x4416_0000, "50 is not above 50");
    assert_eq!(real_bits(&host, "r3"), 0x424C_0000, "51 is stored");
    assert_eq!(real_bits(&host, "r4"), 0, "a null unit reads 0.0");
    assert_eq!(
        real_bits(&host, "r5"),
        0,
        "no attack ability: the setter does nothing"
    );
}

#[test]
fn stored_values_are_read_back_through_the_model() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitVertexColor(u1, 300, -5, 128, 256)
    call SetUnitTimeScale(u1, I2R(3))
    call SetUnitScale(u1, I2R(2), I2R(9), I2R(9))
    call SetUnitUserData(u1, 42)
    call SetUnitCreepGuard(u1, true)
    call SetUnitExploded(u1, true)
    set ix = GetUnitUserData(u1)
    set ix2 = GetUnitUserData(nul)
    call SetUnitFog(I2R(0), I2R(0), I2R(0), I2R(0), I2R(0))
endfunction
"
    ));
    run(&mut host);
    let u = model_unit(&host, "u1");
    assert_eq!(
        u.vertex_color, 0xFFFF_0080,
        "alpha 255, red clamped to 255, green 0, blue 128"
    );
    assert_eq!(u.time_scale.to_bits(), 0x4040_0000);
    assert_eq!(
        u.scale.to_bits(),
        0x4000_0000,
        "only the first scale value is kept"
    );
    assert_eq!(u.user_data, 42);
    assert!(u.creep_guard);
    assert!(u.exploded);
    assert_eq!(int(&host, "ix"), 42);
    assert_eq!(int(&host, "ix2"), 0);
}

#[test]
fn type_values_and_food_and_point_value() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set ix = GetUnitPointValue(u1)
    set ix2 = GetUnitPointValueByType('hfoo')
    set ix3 = GetUnitFoodUsed(u1)
    set ix4 = GetUnitFoodMade(u1)
    set ix5 = GetUnitPointValue(nul)
endfunction
"
    ));
    run(&mut host);
    assert_eq!(int(&host, "ix"), 5);
    assert_eq!(int(&host, "ix2"), 5);
    assert_eq!(int(&host, "ix3"), 2);
    assert_eq!(int(&host, "ix4"), 0);
    assert_eq!(int(&host, "ix5"), 0);
}

#[test]
fn identity_ownership_force_and_range_tests() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(1), 'hfoo', I2R(30), I2R(40), I2R(0))
    set f1 = CreateForce()
    call ForceAddPlayer(f1, Player(0))
    set b1 = IsUnit(u1, u1)
    set b2 = IsUnit(u1, u2)
    set b3 = IsUnit(nul, nul)
    set b4 = IsUnitOwnedByPlayer(u1, Player(0))
    set b5 = IsUnitOwnedByPlayer(u1, Player(1))
    set b6 = IsUnitInForce(u1, f1)
    set ix = 0
    set ix2 = 0
    set ix3 = 0
    set ix4 = 0
    if IsUnitInForce(u2, f1) then
        set ix = 1
    endif
    if IsUnitInRange(u1, u2, I2R(100)) then
        set ix2 = 1
    endif
    if IsUnitInRange(u1, u2, I2R(10)) then
        set ix3 = 1
    endif
    if IsUnitInRangeXY(u1, I2R(30), I2R(40), I2R(100)) then
        set ix4 = 1
    endif
endfunction
"
    ));
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(!boolean(&host, "b2"));
    assert!(
        boolean(&host, "b3"),
        "two null handles resolve to the same (no) object"
    );
    assert!(boolean(&host, "b4"));
    assert!(!boolean(&host, "b5"));
    assert!(boolean(&host, "b6"));
    assert_eq!(
        int(&host, "ix"),
        0,
        "u2 is owned by player 1, not in the force"
    );
    assert_eq!(int(&host, "ix2"), 1);
    assert_eq!(int(&host, "ix3"), 0);
    assert_eq!(int(&host, "ix4"), 1);
}

#[test]
fn hero_id_and_illusion_flags() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    set b1 = IsHeroUnitId('Hpal')
    set b2 = IsHeroUnitId('hfoo')
    set b3 = IsUnitIllusion(u1)
    set b4 = IsUnitLoaded(u1)
endfunction
"
    ));
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(!boolean(&host, "b2"));
    assert!(!boolean(&host, "b3"));
    assert!(!boolean(&host, "b4"));
    // The hero-type unit is a hero (its state is created with it).
    assert!(model_unit(&host, "u2").hero.is_some());
    assert!(model_unit(&host, "u1").hero.is_none());
}

#[test]
fn abilities_add_level_and_permanent() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set ix = GetUnitAbilityLevel(u1, 'AHgr')
    set b1 = UnitAddAbility(u1, 'AHgr')
    set b2 = UnitAddAbility(u1, 'AHbz')
    set b3 = UnitAddAbility(u1, 'AHbz')
    set b4 = UnitAddAbility(u1, 'zzzz')
    set ix2 = GetUnitAbilityLevel(u1, 'AHbz')
    set ix3 = SetUnitAbilityLevel(u1, 'AHbz', 5)
    set ix4 = GetUnitAbilityLevel(u1, 'AHbz')
    set ix5 = DecUnitAbilityLevel(u1, 'AHbz')
    set b5 = UnitMakeAbilityPermanent(u1, true, 'AHbz')
    set b6 = UnitRemoveAbility(u1, 'AHbz')
    set ix6 = GetUnitAbilityLevel(u1, 'AHbz')
    set b7 = UnitRemoveAbility(u1, 'AHbz')
    set ix7 = SetUnitAbilityLevel(u1, 'AHbz', 2)
    set b8 = UnitAddAbility(u1, 'AHvx')
    set ix8 = SetUnitAbilityLevel(u1, 'AHvx', 4)
    set ix9 = IncUnitAbilityLevel(u1, 'AHvx')
    set ix10 = SetUnitAbilityLevel(u1, 'AHnn', 2)
endfunction
"
    ));
    run(&mut host);
    // The type grants AHgr at level 1 at creation, so AddAbility on it fails and its level reads 1.
    assert_eq!(int(&host, "ix"), 1);
    assert!(!boolean(&host, "b1"));
    assert!(boolean(&host, "b2"));
    assert!(
        !boolean(&host, "b3"),
        "a second add of the same ability fails"
    );
    assert!(
        !boolean(&host, "b4"),
        "an unregistered ability cannot be added"
    );
    assert_eq!(int(&host, "ix2"), 1);
    assert_eq!(
        int(&host, "ix3"),
        3,
        "the target is clamped to the max level 3"
    );
    assert_eq!(int(&host, "ix4"), 3);
    assert_eq!(int(&host, "ix5"), 2);
    assert!(boolean(&host, "b5"));
    assert!(boolean(&host, "b6"));
    assert_eq!(int(&host, "ix6"), 0, "a removed ability reads 0");
    assert!(!boolean(&host, "b7"), "removing it again fails");
    assert_eq!(
        int(&host, "ix7"),
        0,
        "setting a level on a missing ability returns 0"
    );
    assert!(boolean(&host, "b8"));
    assert_eq!(int(&host, "ix8"), 1, "no max data means max 1");
    // UNSURE: the decompiled guard compares the zero-based level with the max, so Inc reaches level 2 here.
    assert_eq!(int(&host, "ix9"), 2);
    assert_eq!(int(&host, "ix10"), 0);
}

#[test]
fn permanent_counter_follows_the_flag() {
    let mut host = host_for(&format!(
        "{GLOBALS}
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set b1 = UnitAddAbility(u1, 'AHbz')
    set b2 = UnitMakeAbilityPermanent(u1, true, 'AHbz')
    set b3 = UnitMakeAbilityPermanent(u1, true, 'AHbz')
    set b4 = UnitMakeAbilityPermanent(u1, false, 'AHbz')
    set b5 = UnitMakeAbilityPermanent(u1, false, 'AHgr')
endfunction
"
    ));
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(boolean(&host, "b2"));
    assert!(boolean(&host, "b3"));
    assert!(boolean(&host, "b4"));
    assert!(
        boolean(&host, "b5"),
        "AHgr is granted at creation, so it can be made permanent"
    );
    let u = model_unit(&host, "u1");
    assert_eq!(
        u.abilities[&ABIL_MAX3].permanent, 1,
        "two raises and one lowering"
    );
}

/// The tables cover every native of the work list once (the eight presentation and timed-turn natives moved to lane_e.rs), and an entry is registered exactly when it is not
/// `NotImplemented`. The `NotImplemented` entries are checked through the VM: a script that calls them leaves
/// them unbound.
#[test]
fn support_tables_cover_the_work_list() {
    assert_eq!(UNIT_NATIVES.len() + HERO_NATIVES.len(), 107);
    let mut names: Vec<&str> = UNIT_NATIVES
        .iter()
        .chain(HERO_NATIVES.iter())
        .map(|e| e.name)
        .collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 107, "no name appears twice");
    let implemented = UNIT_NATIVES
        .iter()
        .chain(HERO_NATIVES.iter())
        .filter(|e| !matches!(e.support, Support::NotImplemented(_)))
        .count();
    // 56 with UnitResetCooldown (a no-op in the simulation, abilities-2.md section 1.2), plus nine hero natives
    // (AddHeroXP, SetHeroXP, SetHeroLevel, UnitStripHeroLevel, UnitModifySkillPoints, GetHeroStr/Agi/Int,
    // SetHeroStr/Agi/Int, docs/spec/heroes.md).
    // Plus the three fog tests IsUnitVisible / IsUnitFogged / IsUnitMasked (vision.md section 7, LIKELY).
    // Plus SetUnitTypeSlots (shop stock, shop-stock.md).
    // Plus UnitDamageTarget, UnitDamagePoint, UnitUseItem, UnitUseItemPoint, UnitUseItemTarget, SetUnitRescuable.
    // Plus IsUnitType, IsUnitIdType, UnitAddType, UnitRemoveType, IsUnitInTransport, UnitSetUsesAltIcon (unit-classification.md).
    assert_eq!(implemented, 83);

    let host = host_for(
        r#"
native GetUnitName takes unit whichUnit returns string
native SelectHeroSkill takes unit whichHero, integer abilcode returns nothing
native GetUnitLevel takes unit whichUnit returns integer
native GetHeroXP takes unit whichUnit returns integer
function unused takes nothing returns nothing
    local string s
    local unit u
    set s = GetUnitName(u)
    call SelectHeroSkill(u, 5)
    set s = null
endfunction
function main takes nothing returns nothing
endfunction
"#,
    );
    let unbound = host.vm().unbound_natives();
    assert!(unbound.contains(&"GetUnitName"));
    assert!(unbound.contains(&"SelectHeroSkill"));
    assert!(!unbound.contains(&"GetHeroXP"), "GetHeroXP is registered");
}
