//! Unit and player-unit events through `ScriptHost` (`natives/unit_event.rs`, spec
//! `docs/spec/groups-events.md` 5 to 9).
//!
//! The action functions record the order in which they run as digits of `acc` (`acc = acc * 10 + k`), so
//! an expected value reads as the firing order. The death dispatch runs inside the native that killed the
//! unit, and the actions run as chains, which the host steps through.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{UnitType, UnitTypes};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type boolexpr extends handle
type filterfunc extends boolexpr
type trigger extends handle
type triggeraction extends handle
type event extends handle
type unitevent extends handle
type playerunitevent extends handle
type triggercondition extends handle

native I2R takes integer i returns real
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native KillUnit takes unit whichUnit returns nothing
native SetWidgetLife takes widget wid, real newLife returns nothing
native GetUnitTypeId takes unit whichUnit returns integer
native ConvertUnitEvent takes integer i returns unitevent
native ConvertPlayerUnitEvent takes integer i returns playerunitevent
native CreateTrigger takes nothing returns trigger
native DestroyTrigger takes trigger t returns nothing
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerAddCondition takes trigger t, boolexpr b returns triggercondition
native Condition takes code c returns boolexpr
native TriggerRegisterUnitEvent takes trigger t, unit whichUnit, unitevent whichEvent returns event
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, playerunitevent whichEvent, boolexpr filter returns event
native GetTriggerUnit takes nothing returns unit
native GetDyingUnit takes nothing returns unit
native GetKillingUnit takes nothing returns unit
native GetTriggerPlayer takes nothing returns player
native Filter takes code c returns filterfunc
native GetFilterUnit takes nothing returns unit
"#;

/// Type ids: raw four-character codes as integers.
const FOOTMAN: i32 = 0x6866_6F6F; // 'hfoo'
const MAGE: i32 = 0x686D_6167; // 'hmag'

/// Event ids (`EVENT_UNIT_DEATH` = 53, `EVENT_PLAYER_UNIT_DEATH` = 20).
const UNIT_DEATH: i32 = 53;
const PLAYER_UNIT_DEATH: i32 = 20;

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{COMMON}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        let types: &mut UnitTypes = g.unit_types_mut();
        let ty = |name: &str| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x43D2_0000),
            max_mana: F32::ZERO,
            start_mana: F32::ZERO,
        };
        types.insert(FOOTMAN as u32, ty("Footman"));
        types.insert(MAGE as u32, ty("Mage"));
    }
    host
}

fn run(host: &mut ScriptHost) {
    for _ in 0..3 {
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

fn handle(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"))
}

/// Globals and action functions. Action `k` appends `k` to `acc`. `dying`, `tunit` and `tplayer` hold the
/// handle ids the actions read.
const COMMON: &str = r#"
globals
integer acc = 0
integer dying = 0
integer tunit = 0
integer killer = 0
integer tplayer = 0
integer calls = 0
unit u1
unit u2
unit m1
unit p1
trigger t1
trigger t2
trigger t3
endglobals

function a1 takes nothing returns nothing
    set acc = acc * 10 + 1
    set dying = GetHandleId(GetDyingUnit())
    set tunit = GetHandleId(GetTriggerUnit())
    set killer = GetHandleId(GetKillingUnit())
endfunction

function a2 takes nothing returns nothing
    set acc = acc * 10 + 2
endfunction

function a3 takes nothing returns nothing
    set acc = acc * 10 + 3
    set tplayer = GetHandleId(GetTriggerPlayer())
endfunction

function is_foot takes nothing returns boolean
    set calls = calls + 1
    return GetUnitTypeId(GetFilterUnit()) == 'hfoo'
endfunction
"#;

#[test]
fn death_fires_the_player_trigger_first_then_unit_triggers_oldest_first() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(2), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function a2)
    call TriggerRegisterUnitEvent(t2, u1, ConvertUnitEvent(53))
    set t3 = CreateTrigger()
    call TriggerAddAction(t3, function a3)
    call TriggerRegisterPlayerUnitEvent(t3, Player(0), ConvertPlayerUnitEvent(20), null)
    call KillUnit(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Player-unit key first (t3), then the unit's table oldest first (t1, then t2).
    assert_eq!(int(&host, "acc"), 312);
    // The dying unit and the trigger unit are u1; the killer is not tracked (null).
    assert_eq!(int(&host, "dying"), handle(&host, "u1") as i32);
    assert_eq!(int(&host, "tunit"), handle(&host, "u1") as i32);
    assert_eq!(int(&host, "killer"), 0);
    // GetTriggerPlayer in the player-unit trigger is player 0.
    let p0 = host.game_mut().player_handle(0);
    assert_eq!(int(&host, "tplayer"), p0 as i32);
}

#[test]
fn a_unit_that_is_already_dead_does_not_die_again() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    call KillUnit(u1)
    call KillUnit(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Only the first kill is a death: the second one sets the same life and fires no death.
    assert_eq!(int(&host, "acc"), 1);
}

#[test]
fn set_widget_life_to_zero_is_a_death() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(2), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    call SetWidgetLife(u2, I2R(50))
    call SetWidgetLife(u1, I2R(0))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(int(&host, "dying"), handle(&host, "u1") as i32);
}

#[test]
fn re_registration_keeps_the_original_position() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function a2)
    call TriggerRegisterUnitEvent(t2, u1, ConvertUnitEvent(53))
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    call KillUnit(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // t1 registered again keeps its place and does not fire twice: 1 then 2.
    assert_eq!(int(&host, "acc"), 12);
}

#[test]
fn destroyed_trigger_is_no_longer_notified() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    call DestroyTrigger(t1)
    call KillUnit(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 0);
}

#[test]
fn unit_event_is_only_for_its_own_unit() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(2), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    call KillUnit(u2)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 0);
}

#[test]
fn player_unit_death_uses_the_owner_and_the_filter() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set m1 = CreateUnit(Player(0), 'hmag', I2R(2), I2R(0), I2R(0))
    set p1 = CreateUnit(Player(1), 'hfoo', I2R(3), I2R(0), I2R(0))
    set t3 = CreateTrigger()
    call TriggerAddAction(t3, function a3)
    call TriggerRegisterPlayerUnitEvent(t3, Player(0), ConvertPlayerUnitEvent(20), Filter(function is_foot))
    call KillUnit(m1)
    call KillUnit(p1)
    call KillUnit(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // The mage fails the filter, the player 1 unit has another owner, and the footman fires once.
    assert_eq!(int(&host, "acc"), 3);
    assert_eq!(
        int(&host, "calls"),
        2,
        "the filter runs for the owner's units only"
    );
}

#[test]
fn unknown_event_id_is_rejected() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(999))
    call KillUnit(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Event id 999 is outside the accepted ranges, so nothing is registered.
    assert_eq!(int(&host, "acc"), 0);
}

#[test]
fn false_condition_stops_the_death_actions() {
    let mut host = host_for(
        r#"
function never takes nothing returns boolean
    return false
endfunction

function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerAddCondition(t1, Condition(function never))
    call TriggerRegisterUnitEvent(t1, u1, ConvertUnitEvent(53))
    call KillUnit(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 0);
}

#[test]
fn death_event_ids_match_the_spec_keys() {
    // Keys are the event id plus 0x80200 (spec 5.2).
    assert_eq!(UNIT_DEATH + 0x8_0200, 0x8_0235);
    assert_eq!(PLAYER_UNIT_DEATH + 0x8_0200, 0x8_0214);
}
