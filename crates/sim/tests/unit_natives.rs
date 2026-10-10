//! Unit natives through `ScriptHost` (`natives/unit.rs`, the model in `natives/game.rs`).
//!
//! The scripts use integer-valued reals only (`I2R`), so they need no lookup tables for literals. The
//! facing conversion is checked against the same softfloat expression the native uses (the constants are
//! the start-up values of the 1.26a image, AGENTS.md 4.3m). With `GAME_DLL` set the tables are loaded
//! too, and the same assertions run.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ObjKey, UnitEvent, UnitType, UnitTypes};
use sim::{ScriptHost, HANDLE_BASE};
use softfloat::{arith, Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type location extends handle
type timer extends handle
type hashtable extends handle
type unitstate extends handle

native I2R takes integer i returns real
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native GetPlayerId takes player p returns integer
native Location takes real x, real y returns location
native GetLocationX takes location l returns real
native GetLocationY takes location l returns real
native CreateTimer takes nothing returns timer
native InitHashtable takes nothing returns hashtable
native SaveUnitHandle takes hashtable t, integer p, integer c, unit whichUnit returns boolean
native ConvertUnitState takes integer i returns unitstate
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateUnitByName takes player id, string unitname, real x, real y, real face returns unit
native CreateUnitAtLoc takes player id, integer unitid, location whichLocation, real face returns unit
native CreateUnitAtLocByName takes player id, string unitname, location whichLocation, real face returns unit
native RemoveUnit takes unit whichUnit returns nothing
native KillUnit takes unit whichUnit returns nothing
native GetUnitX takes unit whichUnit returns real
native GetUnitY takes unit whichUnit returns real
native GetUnitLoc takes unit whichUnit returns location
native SetUnitX takes unit whichUnit, real newX returns nothing
native SetUnitY takes unit whichUnit, real newY returns nothing
native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing
native SetUnitPositionLoc takes unit whichUnit, location whichLocation returns nothing
native GetUnitFacing takes unit whichUnit returns real
native SetUnitFacing takes unit whichUnit, real facingAngle returns nothing
native GetOwningPlayer takes unit whichUnit returns player
native SetUnitOwner takes unit whichUnit, player whichPlayer, boolean changeColor returns nothing
native GetUnitTypeId takes unit whichUnit returns integer
native GetUnitState takes unit whichUnit, unitstate whichUnitState returns real
native SetUnitState takes unit whichUnit, unitstate whichUnitState, real newVal returns nothing
native GetWidgetLife takes widget wid returns real
native SetWidgetLife takes widget wid, real newLife returns nothing
native GetWidgetX takes widget wid returns real
native GetWidgetY takes widget wid returns real
native ShowUnit takes unit whichUnit, boolean show returns nothing
native IsUnitHidden takes unit whichUnit returns boolean
native PauseUnit takes unit whichUnit, boolean flag returns nothing
native IsUnitPaused takes unit whichUnit returns boolean
"#;

/// Type ids: raw four-character codes as integers.
const FOOTMAN: i32 = 0x6866_6F6F; // 'hfoo'
const MAGE: i32 = 0x686D_6167; // 'hmag'

/// Life 420.0, mana 300.0 and starting mana 150.0 (the registered templates below).
const FOOTMAN_LIFE: u32 = 0x43D2_0000;
const MAGE_MANA: u32 = 0x4396_0000;
const MAGE_START_MANA: u32 = 0x4316_0000;

/// The soft-float lookup tables of the user's `Game.dll` (`GAME_DLL`), or `None` when it is unset.
fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn ut(name: &str, max_life: u32, max_mana: u32, start_mana: u32) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: F32::from_bits(max_life),
        max_mana: F32::from_bits(max_mana),
        start_mana: F32::from_bits(start_mana),
    }
}

/// A host with the synthetic unit types registered (footman: life 420, no mana; mage: life 250, mana 300,
/// start mana 150).
fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse_with(&full, tables().as_ref()).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    {
        let mut g = host.game_mut();
        let types = g.unit_types_mut();
        types.insert(FOOTMAN as u32, ut("Footman", FOOTMAN_LIFE, 0, 0));
        types.insert(
            MAGE as u32,
            ut("Mage", 0x437A_0000, MAGE_MANA, MAGE_START_MANA),
        );
    }
    host
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

/// The handle id of a `unit`, `player`, `location` or other handle global.
fn handle(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"))
}

/// The exact bits of a real global.
fn bits(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_real())
        .unwrap_or_else(|| panic!("global {name} is not a real"))
        .to_bits()
}

fn f(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// The arena key of the unit behind handle `id`, if it is still live.
fn key_of(host: &ScriptHost, id: u32) -> Option<ObjKey> {
    host.game().handle_object(id)
}

/// Runs `Main` to completion.
fn run(host: &mut ScriptHost) {
    host.start("Main", &[]).unwrap();
    host.step();
}

#[test]
fn create_and_read_back_the_template_values() {
    let src = r#"
globals
    player gP = null
    unit gU = null
    unit gM = null
    real gX = 0.0
    real gY = 0.0
    real gLife = 0.0
    real gMaxLife = 0.0
    real gMana = 0.0
    real gMaxMana = 0.0
    integer gType = 0
    integer gOwner = -1
    integer gHandle = 0
    integer gMageType = 0
endglobals

function Main takes nothing returns nothing
    set gP = Player(2)
    set gU = CreateUnit(gP, 'hfoo', I2R(100), I2R(-200), I2R(0))
    set gM = CreateUnitByName(gP, "Mage", I2R(3), I2R(4), I2R(0))
    set gX = GetUnitX(gU)
    set gY = GetUnitY(gU)
    set gLife = GetUnitState(gU, ConvertUnitState(0))
    set gMaxLife = GetUnitState(gU, ConvertUnitState(1))
    set gMana = GetUnitState(gM, ConvertUnitState(2))
    set gMaxMana = GetUnitState(gM, ConvertUnitState(3))
    set gType = GetUnitTypeId(gU)
    set gOwner = GetPlayerId(GetOwningPlayer(gU))
    set gHandle = GetHandleId(gU)
    set gMageType = GetUnitTypeId(gM)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(bits(&host, "gX"), 100.0f32.to_bits());
    assert_eq!(bits(&host, "gY"), (-200.0f32).to_bits());
    // A new unit starts at its type's life and starting mana.
    assert_eq!(bits(&host, "gLife"), FOOTMAN_LIFE);
    assert_eq!(bits(&host, "gMaxLife"), FOOTMAN_LIFE);
    assert_eq!(bits(&host, "gMana"), MAGE_START_MANA);
    assert_eq!(bits(&host, "gMaxMana"), MAGE_MANA);
    assert_eq!(int(&host, "gType"), FOOTMAN);
    assert_eq!(int(&host, "gMageType"), MAGE);
    assert_eq!(int(&host, "gOwner"), 2);
    // The unit's handle is the first id after the player's (player 2 gets the first id, the unit the next).
    assert_eq!(int(&host, "gHandle") as u32, HANDLE_BASE + 2);
    assert_eq!(handle(&host, "gU"), HANDLE_BASE + 2);
    assert_eq!(handle(&host, "gM"), HANDLE_BASE + 3);
}

#[test]
fn creation_by_location_uses_the_location_coordinates() {
    let src = r#"
globals
    location gL = null
    unit gU = null
    unit gV = null
    real gX = 0.0
    real gY = 0.0
    real gLX = 0.0
    real gLY = 0.0
endglobals

function Main takes nothing returns nothing
    set gL = Location(I2R(-64), I2R(96))
    set gU = CreateUnitAtLoc(Player(1), 'hmag', gL, I2R(0))
    set gV = CreateUnitAtLocByName(Player(1), "Footman", gL, I2R(0))
    set gX = GetUnitX(gU)
    set gY = GetUnitY(gV)
    set gLX = GetLocationX(GetUnitLoc(gU))
    set gLY = GetLocationY(GetUnitLoc(gV))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(bits(&host, "gX"), (-64.0f32).to_bits());
    assert_eq!(bits(&host, "gY"), 96.0f32.to_bits());
    // GetUnitLoc copies the unit's position into a new location.
    assert_eq!(bits(&host, "gLX"), (-64.0f32).to_bits());
    assert_eq!(bits(&host, "gLY"), 96.0f32.to_bits());
}

#[test]
fn failed_creation_does_not_consume_a_handle_id() {
    let src = r#"
globals
    player gP = null
    unit gBadType = null
    unit gBadName = null
    unit gBadPlayer = null
    unit gBadLoc = null
    location gBefore = null
    location gAfter = null
endglobals

function Main takes nothing returns nothing
    set gP = Player(1)
    set gBefore = Location(I2R(0), I2R(0))
    set gBadType = CreateUnit(gP, 'zzzz', I2R(0), I2R(0), I2R(0))
    set gBadName = CreateUnitByName(gP, "NoSuchUnit", I2R(0), I2R(0), I2R(0))
    set gBadPlayer = CreateUnit(null, 'hfoo', I2R(0), I2R(0), I2R(0))
    set gBadLoc = CreateUnitAtLoc(gP, 'hfoo', null, I2R(0))
    set gAfter = Location(I2R(1), I2R(1))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(handle(&host, "gBadType"), 0);
    assert_eq!(handle(&host, "gBadName"), 0);
    assert_eq!(handle(&host, "gBadPlayer"), 0);
    assert_eq!(handle(&host, "gBadLoc"), 0);
    // Player 1 takes the first id, the location the second, and the four failed creations took none.
    assert_eq!(handle(&host, "gBefore"), HANDLE_BASE + 2);
    assert_eq!(handle(&host, "gAfter"), HANDLE_BASE + 3);
}

#[test]
fn unit_ids_are_reused_last_freed_first_and_held_ids_stay_reserved() {
    let src = r#"
globals
    player gP = null
    location gL = null
    unit gA = null
    unit gB = null
    timer gT1 = null
    timer gT2 = null
    timer gT3 = null
    hashtable gH = null
    integer gHeldId = 0
    integer gT2Id = 0
endglobals

function Main takes nothing returns nothing
    set gP = Player(0)
    set gL = Location(I2R(0), I2R(0))
    set gA = CreateUnit(gP, 'hfoo', I2R(0), I2R(0), I2R(0))
    set gB = CreateUnit(gP, 'hfoo', I2R(0), I2R(0), I2R(0))
    call RemoveUnit(gA)
    // The freed id of A is handed out first, before any fresh id.
    set gT1 = CreateTimer()
    // Holding B in a hashtable keeps its id reserved when B is removed.
    set gH = InitHashtable()
    call SaveUnitHandle(gH, 0, 0, gB)
    set gHeldId = GetHandleId(gB)
    call RemoveUnit(gB)
    set gT2 = CreateTimer()
    set gT2Id = GetHandleId(gT2)
    set gT3 = CreateTimer()
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    let id_a = HANDLE_BASE + 3;
    let id_b = HANDLE_BASE + 4;
    // Timers take A's id (LIFO reuse) and then fresh ids: the table has grown past B's id.
    assert_eq!(handle(&host, "gT1"), id_a);
    assert_eq!(int(&host, "gHeldId") as u32, id_b);
    assert_ne!(int(&host, "gT2Id") as u32, id_b, "a held id is not reused");
    assert_eq!(handle(&host, "gT3"), HANDLE_BASE + 7);
    // A removed unit still answers its id; the object is gone.
    assert_eq!(int(&host, "gHeldId") as u32, id_b);
}

#[test]
fn kill_sets_life_to_zero_and_keeps_the_unit() {
    let src = r#"
globals
    unit gU = null
    real gLife = -1.0
    real gX = -1.0
    real gWidgetLife = -1.0
    real gAfterSecond = -1.0
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', I2R(10), I2R(20), I2R(0))
    call KillUnit(gU)
    set gLife = GetUnitState(gU, ConvertUnitState(0))
    set gX = GetUnitX(gU)
    set gWidgetLife = GetWidgetLife(gU)
    call KillUnit(gU)
    set gAfterSecond = GetWidgetLife(gU)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(bits(&host, "gLife"), 0);
    assert_eq!(bits(&host, "gWidgetLife"), 0);
    assert_eq!(bits(&host, "gAfterSecond"), 0);
    // The unit is still there: its position is readable.
    assert_eq!(bits(&host, "gX"), 10.0f32.to_bits());
    let u = handle(&host, "gU");
    let key = key_of(&host, u).expect("killed units keep their handle");
    assert!(host.game().unit(key).dead);
    assert!(!host.game().unit(key).removed);
    // One death, from the first kill. Each kill fires the life-changed event.
    let events = host.game().unit_events().to_vec();
    assert_eq!(
        events,
        vec![
            UnitEvent::Notify {
                unit: key,
                id: 0xD01E6
            },
            UnitEvent::Death(key),
            UnitEvent::Notify {
                unit: key,
                id: 0xD01E6
            },
        ]
    );
}

#[test]
fn set_life_clamps_non_positive_values_to_plus_zero() {
    let src = r#"
globals
    unit gU = null
    real gNegZero = -1.0
    real gNeg = -1.0
    real gPos = -1.0
    real gBigMana = -1.0
    integer gEvents = 0
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hmag', I2R(0), I2R(0), I2R(0))
    call SetUnitState(gU, ConvertUnitState(0), -I2R(0))
    set gNegZero = GetUnitState(gU, ConvertUnitState(0))
    call SetUnitState(gU, ConvertUnitState(0), I2R(100))
    call SetUnitState(gU, ConvertUnitState(0), I2R(-5))
    set gNeg = GetUnitState(gU, ConvertUnitState(0))
    call SetUnitState(gU, ConvertUnitState(0), I2R(42))
    set gPos = GetUnitState(gU, ConvertUnitState(0))
    // Mana is stored as given: no clamp and no event.
    call SetUnitState(gU, ConvertUnitState(2), I2R(-5))
    set gBigMana = GetUnitState(gU, ConvertUnitState(2))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // A zero life from a negative zero is +0.0, not -0.0.
    assert_eq!(bits(&host, "gNegZero"), 0x0000_0000);
    assert_eq!(bits(&host, "gNeg"), 0x0000_0000);
    assert_eq!(bits(&host, "gPos"), 42.0f32.to_bits());
    assert_eq!(bits(&host, "gBigMana"), (-5.0f32).to_bits());
    // Life changes fire the life event; the change to zero of a living unit is its death.
    let u = handle(&host, "gU");
    let key = key_of(&host, u).unwrap();
    let events = host.game().unit_events().to_vec();
    let notify = UnitEvent::Notify {
        unit: key,
        id: 0xD01E6,
    };
    // Four life events: to zero, to 100, to zero (negative) and to 42; mana adds none.
    assert_eq!(events.iter().filter(|e| **e == notify).count(), 4);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, UnitEvent::Death(_)))
            .count(),
        1
    );
}

#[test]
fn state_rules_only_apply_life_and_mana() {
    let src = r#"
globals
    unit gU = null
    real gMaxLife = 0.0
    real gMaxMana = 0.0
    real gUnknown = -1.0
    real gUnknownSet = -1.0
    real gMana = 0.0
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hmag', I2R(0), I2R(0), I2R(0))
    // The maxima are ignored when set.
    call SetUnitState(gU, ConvertUnitState(1), I2R(9999))
    call SetUnitState(gU, ConvertUnitState(3), I2R(9999))
    set gMaxLife = GetUnitState(gU, ConvertUnitState(1))
    set gMaxMana = GetUnitState(gU, ConvertUnitState(3))
    // A state outside 0 to 3 reads 0.0 and writes nothing.
    set gUnknown = GetUnitState(gU, ConvertUnitState(7))
    call SetUnitState(gU, ConvertUnitState(7), I2R(5))
    set gUnknownSet = GetUnitState(gU, ConvertUnitState(7))
    call SetUnitState(gU, ConvertUnitState(2), I2R(12))
    set gMana = GetUnitState(gU, ConvertUnitState(2))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(
        bits(&host, "gMaxLife"),
        0x437A_0000,
        "max life 250.0 is unchanged"
    );
    assert_eq!(bits(&host, "gMaxMana"), MAGE_MANA);
    assert_eq!(bits(&host, "gUnknown"), 0);
    assert_eq!(bits(&host, "gUnknownSet"), 0);
    assert_eq!(bits(&host, "gMana"), 12.0f32.to_bits());
}

#[test]
fn positions_store_the_raw_values() {
    let src = r#"
globals
    unit gU = null
    location gL = null
    real gX1 = 0.0
    real gY1 = 0.0
    real gX2 = 0.0
    real gY2 = 0.0
    real gX3 = 0.0
    real gY3 = 0.0
    real gWX = 0.0
    real gWY = 0.0
    real gLocX = 0.0
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitX(gU, I2R(7))
    call SetUnitY(gU, I2R(-3))
    set gX1 = GetUnitX(gU)
    set gY1 = GetUnitY(gU)
    call SetUnitPosition(gU, I2R(-1000), I2R(2048))
    set gX2 = GetUnitX(gU)
    set gY2 = GetUnitY(gU)
    set gL = Location(I2R(11), I2R(-12))
    call SetUnitPositionLoc(gU, gL)
    set gX3 = GetWidgetX(gU)
    set gY3 = GetWidgetY(gU)
    set gWX = GetUnitX(gU)
    set gWY = GetWidgetY(gU)
    set gLocX = GetLocationX(GetUnitLoc(gU))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(bits(&host, "gX1"), 7.0f32.to_bits());
    assert_eq!(bits(&host, "gY1"), (-3.0f32).to_bits());
    assert_eq!(bits(&host, "gX2"), (-1000.0f32).to_bits());
    assert_eq!(bits(&host, "gY2"), 2048.0f32.to_bits());
    assert_eq!(bits(&host, "gX3"), 11.0f32.to_bits());
    assert_eq!(bits(&host, "gY3"), (-12.0f32).to_bits());
    assert_eq!(bits(&host, "gWX"), 11.0f32.to_bits());
    assert_eq!(bits(&host, "gWY"), (-12.0f32).to_bits());
    assert_eq!(bits(&host, "gLocX"), 11.0f32.to_bits());
}

#[test]
fn facing_converts_degrees_with_the_startup_constants() {
    let src = r#"
globals
    unit gU = null
    unit gF = null
    real gFace = -1.0
    real gBack = -1.0
    real gZero = -1.0
    real gInit = -1.0
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(90))
    set gInit = GetUnitFacing(gU)
    call SetUnitFacing(gU, I2R(45))
    set gFace = GetUnitFacing(gU)
    set gF = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set gZero = GetUnitFacing(gF)
    call SetUnitFacing(gF, I2R(180))
    set gBack = GetUnitFacing(gF)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    let deg = |x: f32| f(x);
    let to_rad = |a: F32| arith::mul(a, F32::from_bits(0x3C8E_FA35));
    let to_deg = |r: F32| arith::mul(r, F32::from_bits(0x4265_2EE1));
    // The stored facing is the softfloat product of the degrees and pi/180; the getter multiplies back.
    let expect = |x: f32| to_deg(to_rad(deg(x))).to_bits();
    assert_eq!(bits(&host, "gInit"), expect(90.0));
    assert_eq!(bits(&host, "gFace"), expect(45.0));
    assert_eq!(bits(&host, "gBack"), expect(180.0));
    assert_eq!(bits(&host, "gZero"), 0);
    // Pinned bits from the truncating softfloat (not IEEE round-to-nearest): 45 degrees come back as
    // 44.999996, and pi/4 is stored as 0x3F490FDA (IEEE rounding would give ...FDB).
    assert_eq!(bits(&host, "gFace"), 0x4233_FFFF);
    assert_eq!(bits(&host, "gInit"), 0x42B3_FFFF);
    assert_eq!(bits(&host, "gBack"), 0x4333_FFFF);
    let key_u = key_of(&host, handle(&host, "gU")).unwrap();
    assert_eq!(host.game().unit(key_u).facing.to_bits(), 0x3F49_0FDA);

    let key = key_of(&host, handle(&host, "gF")).unwrap();
    assert_eq!(
        host.game().unit(key).facing.to_bits(),
        to_rad(deg(180.0)).to_bits(),
        "the mover stores radians"
    );
}

#[test]
fn owner_change_records_one_event_per_real_change() {
    let src = r#"
globals
    unit gU = null
    integer gBefore = -1
    integer gAfter = -1
    integer gSame = -1
    integer gNull = -1
    integer gAfterNull = -1
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set gBefore = GetPlayerId(GetOwningPlayer(gU))
    call SetUnitOwner(gU, Player(3), false)
    set gAfter = GetPlayerId(GetOwningPlayer(gU))
    call SetUnitOwner(gU, Player(3), true)
    set gSame = GetPlayerId(GetOwningPlayer(gU))
    call SetUnitOwner(gU, null, false)
    set gAfterNull = GetPlayerId(GetOwningPlayer(gU))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gBefore"), 0);
    assert_eq!(int(&host, "gAfter"), 3);
    assert_eq!(int(&host, "gSame"), 3);
    assert_eq!(int(&host, "gAfterNull"), 3);
    let key = key_of(&host, handle(&host, "gU")).unwrap();
    let events = host.game().unit_events().to_vec();
    assert_eq!(
        events,
        vec![UnitEvent::OwnerChanged {
            unit: key,
            from: 0,
            to: 3
        }]
    );
}

#[test]
fn removal_runs_the_sequence_once_and_frees_the_unit() {
    let src = r#"
globals
    unit gU = null
    real gX = -1.0
    integer gType = -1
    real gLife = -1.0
    player gOwner = null
    boolean gHidden = true
    boolean gPaused = true
    location gLoc = null
    real gWidget = -1.0
    integer gId = 0
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hmag', I2R(5), I2R(6), I2R(0))
    set gId = GetHandleId(gU)
    call RemoveUnit(gU)
    // A removed unit answers with the defaults of its natives.
    set gX = GetUnitX(gU)
    set gType = GetUnitTypeId(gU)
    set gLife = GetUnitState(gU, ConvertUnitState(0))
    set gOwner = GetOwningPlayer(gU)
    set gHidden = IsUnitHidden(gU)
    set gPaused = IsUnitPaused(gU)
    set gLoc = GetUnitLoc(gU)
    set gWidget = GetWidgetLife(gU)
    // Every native that changes a unit ignores the removed one.
    call SetUnitX(gU, I2R(99))
    call KillUnit(gU)
    call RemoveUnit(gU)
    call ShowUnit(gU, false)
    call SetUnitOwner(gU, Player(4), false)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(bits(&host, "gX"), 0);
    assert_eq!(int(&host, "gType"), 0);
    assert_eq!(bits(&host, "gLife"), 0);
    assert_eq!(handle(&host, "gOwner"), 0);
    assert!(!boolean(&host, "gHidden"));
    assert!(!boolean(&host, "gPaused"));
    assert_eq!(handle(&host, "gLoc"), 0);
    assert_eq!(bits(&host, "gWidget"), 0);
    // Player 0 takes the first id, the unit the second. The id is still readable from the variable,
    // even though the object is gone.
    assert_eq!(int(&host, "gId") as u32, HANDLE_BASE + 2);
    // The freed id is reused by the next handle in this script, Player(4) (LIFO), so it no longer names
    // the unit.
    assert!(host
        .game()
        .handle_of_kind(HANDLE_BASE + 2, sim::natives::HandleKind::Unit)
        .is_none());

    // The removal sequence, once, in order. The second RemoveUnit, KillUnit and the owner change add nothing.
    // The unit is the last object of the arena (the sixteen player slots come first).
    let events = host.game().unit_events().to_vec();
    let key = ObjKey(host.game().object_count() as u32 - 1);
    assert_eq!(
        events,
        vec![
            UnitEvent::Removed(key),
            UnitEvent::Notify {
                unit: key,
                id: 0xD015A
            },
            UnitEvent::Notify {
                unit: key,
                id: 0xD0164
            },
            UnitEvent::Notify {
                unit: key,
                id: 0xD0156
            },
            UnitEvent::Notify {
                unit: key,
                id: 0xD0178
            },
            UnitEvent::Notify {
                unit: key,
                id: 0xD0144
            },
        ]
    );
    assert!(host.game().unit(key).removed);
    assert!(!host.game().unit(key).dead);
}

#[test]
fn a_removed_unit_frees_the_id_for_reuse_by_the_next_handle() {
    // The unit is the only holder, so its id is free at once and the next location takes it.
    let src = r#"
globals
    unit gU = null
    location gL = null
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call RemoveUnit(gU)
    set gL = Location(I2R(0), I2R(0))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // Player 0 holds HANDLE_BASE + 1, the unit HANDLE_BASE + 2, which is freed and reused.
    assert_eq!(handle(&host, "gL"), HANDLE_BASE + 2);
}

#[test]
fn show_and_pause_store_their_flags() {
    let src = r#"
globals
    unit gU = null
    boolean gA = false
    boolean gB = true
    boolean gC = false
    boolean gD = false
endglobals

function Main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call ShowUnit(gU, false)
    set gA = IsUnitHidden(gU)
    call ShowUnit(gU, true)
    set gB = IsUnitHidden(gU)
    call PauseUnit(gU, true)
    set gC = IsUnitPaused(gU)
    call PauseUnit(gU, false)
    set gD = IsUnitPaused(gU)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert!(boolean(&host, "gA"));
    assert!(!boolean(&host, "gB"));
    assert!(boolean(&host, "gC"));
    assert!(!boolean(&host, "gD"));
}

#[test]
fn unit_types_registry_replaces_and_renames() {
    let mut types = UnitTypes::new();
    assert!(types.is_empty());
    types.insert(FOOTMAN as u32, ut("Footman", FOOTMAN_LIFE, 0, 0));
    assert_eq!(types.id_by_name("Footman"), Some(FOOTMAN as u32));
    assert_eq!(types.len(), 1);
    // Replacing a type with a new name drops the old name; the old name no longer resolves.
    let old = types.insert(FOOTMAN as u32, ut("Knight", FOOTMAN_LIFE, 0, 0));
    assert_eq!(old.map(|t| t.name), Some("Footman".to_string()));
    assert_eq!(types.id_by_name("Footman"), None);
    assert_eq!(types.id_by_name("Knight"), Some(FOOTMAN as u32));
    assert_eq!(types.len(), 1);
    assert!(types.get(MAGE as u32).is_none());
}

#[test]
fn invalid_handles_give_the_defaults() {
    let src = r#"
globals
    real gX = -1.0
    real gLife = -1.0
    integer gType = -1
    real gFace = -1.0
    boolean gHidden = true
endglobals

function Main takes nothing returns nothing
    set gX = GetWidgetX(null)
    set gLife = GetWidgetLife(null)
    set gType = GetUnitTypeId(null)
    set gFace = GetUnitFacing(null)
    set gHidden = IsUnitHidden(null)
    call SetUnitState(null, ConvertUnitState(0), I2R(5))
    call KillUnit(null)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(bits(&host, "gX"), 0);
    assert_eq!(bits(&host, "gLife"), 0);
    assert_eq!(int(&host, "gType"), 0);
    assert_eq!(bits(&host, "gFace"), 0);
    assert!(!boolean(&host, "gHidden"));
    assert!(host.game().unit_events().is_empty());
}
