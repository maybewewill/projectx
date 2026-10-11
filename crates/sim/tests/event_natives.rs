//! Event registrations and response getters through `ScriptHost` (`natives/event.rs`).
//!
//! The action functions record what they saw as digits of `acc` (`acc = acc * 10 + k`), so an expected value
//! reads as the firing order. Events that the scripts cannot produce by themselves (a chat line, an attack
//! with its payload) are fired from Rust with `event::fire_event` and the host's runtime.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::event::{self, Datum, EventSlots, Field};
use sim::natives::game::{ObjKey, UnitType, UnitTypes};
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
type location extends handle
type item extends widget

native I2R takes integer i returns real
native R2I takes real r returns integer
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native GetPlayerId takes player p returns integer
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native KillUnit takes unit whichUnit returns nothing
native SetWidgetLife takes widget wid, real newLife returns nothing
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native SetUnitOwner takes unit whichUnit, player whichPlayer, boolean changeColor returns nothing
native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing
native GetUnitTypeId takes unit whichUnit returns integer
native SetPlayerState takes player whichPlayer, integer whichState, integer value returns nothing
native CreateTrigger takes nothing returns trigger
native DestroyTrigger takes trigger t returns nothing
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerRegisterUnitEvent takes trigger t, unit whichUnit, integer whichEvent returns event
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, integer whichEvent, boolexpr filter returns event
native TriggerRegisterPlayerStateEvent takes trigger t, player whichPlayer, integer whichState, integer op, real limit returns event
native TriggerRegisterUnitStateEvent takes trigger t, unit whichUnit, integer whichState, integer op, real limit returns event
native TriggerRegisterPlayerChatEvent takes trigger t, player whichPlayer, string chatMessageToDetect, boolean exactMatchOnly returns event
native TriggerRegisterFilterUnitEvent takes trigger t, unit whichUnit, integer whichEvent, boolexpr filter returns event
native TriggerRegisterUnitInRange takes trigger t, unit whichUnit, real range, boolexpr filter returns event
native Condition takes code c returns boolexpr
native Filter takes code c returns filterfunc
native GetFilterUnit takes nothing returns unit
native GetTriggerUnit takes nothing returns unit
native GetTriggerPlayer takes nothing returns player
native GetTriggerEventId takes nothing returns integer
native GetEnteringUnit takes nothing returns unit
native GetChangingUnit takes nothing returns unit
native GetChangingUnitPrevOwner takes nothing returns player
native GetAttacker takes nothing returns unit
native GetEventDamage takes nothing returns real
native GetEventDamageSource takes nothing returns unit
native GetEventTargetUnit takes nothing returns unit
native GetSpellAbilityId takes nothing returns integer
native GetSpellAbilityUnit takes nothing returns unit
native GetSpellTargetX takes nothing returns real
native GetSpellTargetUnit takes nothing returns unit
native GetEventPlayerState takes nothing returns integer
native GetEventUnitState takes nothing returns integer
native GetEventPlayerChatString takes nothing returns string
native GetEventPlayerChatStringMatched takes nothing returns string
native GetOrderedUnit takes nothing returns unit
"#;

const FOOTMAN: i32 = 0x6866_6F6F; // 'hfoo'
const MAGE: i32 = 0x686D_6167; // 'hmag'

const COMMON: &str = r#"
globals
integer acc = 0
integer calls = 0
integer seen = 0
integer seen2 = 0
integer seenr = 0
unit u1
unit u2
unit m1
trigger t1
trigger t2
trigger t3
endglobals

function a1 takes nothing returns nothing
    set acc = acc * 10 + 1
endfunction

function a2 takes nothing returns nothing
    set acc = acc * 10 + 2
endfunction

function is_foot takes nothing returns boolean
    set calls = calls + 1
    return GetUnitTypeId(GetFilterUnit()) == 'hfoo'
endfunction
"#;

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
            inventory_size: 0,
            ..Default::default()
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

fn unit_key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = handle(host, name);
    host.game().unit_of_handle(id).expect("live unit")
}

/// Fires an event from Rust on `source` under `key`, then runs the chains it started.
fn fire(host: &mut ScriptHost, source: ObjKey, key: u32, slots: EventSlots) {
    let game = host.game_handle();
    event::fire_event(&game, host.vm_mut(), source, key, slots);
    run(host);
}

const UNIT_ATTACKED: u32 = 0x3E;
const KEY_BASE: u32 = 0x8_0200;

#[test]
fn player_state_event_compares_the_new_value_with_its_operator() {
    // PLAYER_STATE_RESOURCE_GOLD = 1, GREATER_THAN = 4, limit 100.
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterPlayerStateEvent(t1, Player(0), 1, 4, I2R(100))
    call SetPlayerState(Player(0), 1, 50)
    call SetPlayerState(Player(0), 1, 150)
    call SetPlayerState(Player(0), 1, 80)
    call SetPlayerState(Player(0), 2, 500)
    call SetPlayerState(Player(1), 1, 500)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Only 150 is above 100; lumber and player 1 do not match the registration.
    assert_eq!(int(&host, "acc"), 1);
}

#[test]
fn player_state_event_reports_the_state_index() {
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set seen = GetEventPlayerState()
endfunction

function main takes nothing returns nothing
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterPlayerStateEvent(t1, Player(0), 1, 4, I2R(0))
    call SetPlayerState(Player(0), 1, 10)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "seen"), 1);
}

#[test]
fn unit_life_state_event_fires_on_widget_life_changes() {
    // Life (state 0) LESS_THAN (0) 50.
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitStateEvent(t1, u1, 0, 0, I2R(50))
    call SetWidgetLife(u1, 60.0)
    call SetWidgetLife(u1, 40.0)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 1, "only the change to 40 is below 50");
}

#[test]
fn unit_max_state_registers_nothing() {
    // State 1 (max life) returns an event but no listener is stored, so nothing runs.
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterUnitStateEvent(t1, u1, 1, 2, I2R(50))
    call SetWidgetLife(u1, 40.0)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 0);
}

#[test]
fn chat_event_matches_the_text_and_exposes_both_strings() {
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set acc = acc * 10 + 1
    if GetEventPlayerChatStringMatched() == "-go" and GetEventPlayerChatString() == "-go 5" then
        set seen = 1
    endif
endfunction

function main takes nothing returns nothing
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterPlayerChatEvent(t1, Player(0), "-go", false)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let game = host.game_handle();
    event::fire_player_chat(&game, host.vm_mut(), 0, "hello there");
    event::fire_player_chat(&game, host.vm_mut(), 1, "-go 5");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 0, "no chat line matched yet");
    event::fire_player_chat(&game, host.vm_mut(), 0, "-go 5");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(int(&host, "seen"), 1);
}

#[test]
fn exact_chat_event_needs_the_whole_message() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterPlayerChatEvent(t1, Player(0), "-go", true)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let game = host.game_handle();
    event::fire_player_chat(&game, host.vm_mut(), 0, "-go 5");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 0);
    event::fire_player_chat(&game, host.vm_mut(), 0, "-GO");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 1);
}

#[test]
fn owner_change_exposes_the_unit_and_the_previous_owner() {
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set seen = GetHandleId(GetChangingUnit())
    set seen2 = GetPlayerId(GetChangingUnitPrevOwner())
endfunction

function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterPlayerUnitEvent(t1, Player(1), 270, null)
    call SetUnitOwner(u1, Player(1), false)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "seen"), handle(&host, "u1") as i32);
    assert_eq!(int(&host, "seen2"), 0);
}

#[test]
fn filter_unit_event_runs_its_filter_with_the_trigger_unit() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set m1 = CreateUnit(Player(0), 'hmag', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterFilterUnitEvent(t1, u1, 62, Filter(function is_foot))
    call TriggerRegisterFilterUnitEvent(t1, m1, 62, Filter(function is_foot))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let (u1, m1) = (unit_key(&host, "u1"), unit_key(&host, "m1"));
    let attacked = |unit| EventSlots {
        id: UNIT_ATTACKED,
        unit: Some(unit),
        ..Default::default()
    };
    fire(&mut host, m1, KEY_BASE + UNIT_ATTACKED, attacked(m1));
    assert_eq!(int(&host, "acc"), 0, "the mage fails the filter");
    fire(&mut host, u1, KEY_BASE + UNIT_ATTACKED, attacked(u1));
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(
        int(&host, "calls"),
        2,
        "the filter ran once for each firing"
    );
}

#[test]
fn attack_getters_read_the_payload_only_for_their_event_ids() {
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set acc = acc * 10 + 1
    set seen = GetHandleId(GetAttacker())
    set seenr = R2I(GetEventDamage())
    set seen2 = GetHandleId(GetEventDamageSource())
endfunction

function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(1), 'hmag', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterUnitEvent(t1, u1, 62)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let (u1, u2) = (unit_key(&host, "u1"), unit_key(&host, "u2"));
    let u2_handle = handle(&host, "u2") as i32;
    let payload = vec![
        (Field::Attacker, Datum::Object(u2)),
        (Field::Damage, Datum::Real(F32::from_bits(0x4248_0000))), // 50.0
        (Field::DamageSource, Datum::Object(u2)),
        (Field::EventTarget, Datum::Object(u2)),
    ];
    fire(
        &mut host,
        u1,
        KEY_BASE + UNIT_ATTACKED,
        EventSlots {
            id: UNIT_ATTACKED,
            unit: Some(u1),
            data: payload,
            ..Default::default()
        },
    );
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(
        int(&host, "seen"),
        u2_handle,
        "GetAttacker reads the attacker of 0x3E"
    );
    assert_eq!(int(&host, "seenr"), 50);
    assert_eq!(int(&host, "seen2"), u2_handle);
}

#[test]
fn getters_answer_null_for_an_event_id_outside_their_set() {
    // GetEventTargetUnit accepts 0x3C and 0x3D only, GetAttacker accepts 0x12 and 0x3E.
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set acc = acc * 10 + 1
    set seen = GetHandleId(GetEventTargetUnit())
    set seen2 = GetHandleId(GetAttacker())
endfunction

function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(1), 'hmag', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterUnitEvent(t1, u1, 62)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let u1 = unit_key(&host, "u1");
    let u2 = unit_key(&host, "u2");
    // The payload is present, but the event id 0x3E is not in the EventTarget set (0x3C, 0x3D).
    fire(
        &mut host,
        u1,
        KEY_BASE + UNIT_ATTACKED,
        EventSlots {
            id: UNIT_ATTACKED,
            unit: Some(u1),
            data: vec![
                (Field::EventTarget, Datum::Object(u2)),
                (Field::Attacker, Datum::Object(u2)),
            ],
            ..Default::default()
        },
    );
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(int(&host, "seen"), 0);
    assert_ne!(int(&host, "seen2"), 0);
}

#[test]
fn spell_getters_read_the_caster_ability_and_target() {
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set acc = acc * 10 + 1
    set seen = GetSpellAbilityId()
    set seen2 = GetHandleId(GetSpellAbilityUnit())
    set seenr = R2I(GetSpellTargetX())
endfunction

function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterUnitEvent(t1, u1, 0x121)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let u1 = unit_key(&host, "u1");
    let code = 0x4142_4331;
    fire(
        &mut host,
        u1,
        KEY_BASE + 0x121,
        EventSlots {
            id: 0x121,
            unit: Some(u1),
            data: vec![
                (Field::SpellAbilityCode, Datum::Int(code)),
                (Field::SpellCaster, Datum::Object(u1)),
                (Field::SpellX, Datum::Real(F32::from_bits(0x4248_0000))),
            ],
            ..Default::default()
        },
    );
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(int(&host, "seen"), code);
    assert_eq!(int(&host, "seen2"), handle(&host, "u1") as i32);
    assert_eq!(int(&host, "seenr"), 50);
}

#[test]
fn unit_in_range_fires_when_a_unit_enters_the_range_only() {
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set acc = acc * 10 + 1
    set seen = GetHandleId(GetEnteringUnit())
endfunction

function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set m1 = CreateUnit(Player(1), 'hmag', I2R(500), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterUnitInRange(t1, u1, I2R(100), null)
    call SetUnitPosition(m1, I2R(10), I2R(0))
    call SetUnitPosition(m1, I2R(20), I2R(0))
    call SetUnitPosition(m1, I2R(500), I2R(0))
    call SetUnitPosition(m1, I2R(10), I2R(0))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Entering at 10 and again at 10 after leaving; the move to 20 stays inside and fires nothing.
    assert_eq!(int(&host, "acc"), 11);
    assert_eq!(int(&host, "seen"), handle(&host, "m1") as i32);
}

#[test]
fn unit_in_range_registration_uses_the_range_key_0x8023d() {
    // The event id that GetEnteringUnit accepts for range events is 0x3D; the handle is returned.
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    set seen = GetHandleId(TriggerRegisterUnitInRange(t1, u1, I2R(100), null))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_ne!(int(&host, "seen"), 0);
    let g = host.game();
    let u1 = unit_key(&host, "u1");
    assert_eq!(
        g.observers(u1).iter().filter(|o| o.key == 0x8_023D).count(),
        1
    );
}

#[test]
fn registration_ids_outside_their_ranges_return_the_null_event() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    set seen = GetHandleId(TriggerRegisterFilterUnitEvent(t1, u1, 0x30, null))
    set seen2 = GetHandleId(TriggerRegisterFilterUnitEvent(t1, u1, 0x3C, null))
    set seenr = GetHandleId(TriggerRegisterPlayerStateEvent(t1, Player(0), 26, 0, I2R(0)))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "seen"), 0, "0x30 is not a filter-unit event id");
    assert_ne!(int(&host, "seen2"), 0, "0x3C is accepted");
    assert_eq!(int(&host, "seenr"), 0, "player state 26 is out of range");
}

#[test]
fn getters_of_other_event_ids_stay_null_in_a_state_firing() {
    // A player-state firing (id 0x0B) exposes GetEventPlayerState, and GetOrderedUnit (0x26..0x28) is null.
    let mut host = host_for(
        r#"
function reads takes nothing returns nothing
    set acc = acc * 10 + 1
    set seen = GetEventPlayerState()
    set seen2 = GetHandleId(GetOrderedUnit())
endfunction

function main takes nothing returns nothing
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function reads)
    call TriggerRegisterPlayerStateEvent(t1, Player(0), 1, 0, I2R(1000))
    call SetPlayerState(Player(0), 1, 500)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(int(&host, "seen"), 1, "the state index");
    assert_eq!(int(&host, "seen2"), 0, "GetOrderedUnit is null for id 0x0B");
}
