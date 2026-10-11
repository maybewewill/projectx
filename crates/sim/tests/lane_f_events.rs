//! `GetRescuer` and `GetSellingUnit` through `ScriptHost` (`natives/event.rs`, `docs/spec/natives-lane-f.md`): the
//! getters answer for their own event ids only. The firings come from Rust, like the other payload events.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::event::{self, Datum, EventSlots, Field};
use sim::natives::game::{ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type boolexpr extends handle
type trigger extends handle
type triggeraction extends handle
type event extends handle

native I2R takes integer i returns real
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, integer whichEvent, boolexpr filter returns event
native TriggerRegisterUnitEvent takes trigger t, unit whichUnit, integer whichEvent returns event
native GetRescuer takes nothing returns unit
native GetSellingUnit takes nothing returns unit
native GetTriggerUnit takes nothing returns unit

globals
unit rescued
unit rescuer
unit shop
unit other
integer seen_rescuer = 0
integer seen_rescued = 0
integer seen_selling = 0
integer fired = 0
trigger t1
trigger t2
endglobals

function on_rescue takes nothing returns nothing
    set fired = fired + 1
    set seen_rescuer = GetHandleId(GetRescuer())
    set seen_rescued = GetHandleId(GetTriggerUnit())
endfunction

function on_sell takes nothing returns nothing
    set fired = fired + 1
    set seen_selling = GetHandleId(GetSellingUnit())
endfunction
"#;

const FOOTMAN: u32 = 0x6866_6F6F;

fn host_for(main: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{main}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    host.game_mut().unit_types_mut().insert(
        FOOTMAN,
        UnitType {
            name: "Footman".into(),
            max_life: F32::from_bits(0x43D2_0000),
            ..Default::default()
        },
    );
    host
}

fn run(host: &mut ScriptHost) {
    for _ in 0..3 {
        host.step();
    }
    assert!(host.misfires().is_empty());
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("{name}"))
}

fn unit_key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host.vm().global(name).and_then(|v| v.as_handle()).expect("handle");
    host.game().unit_of_handle(id).expect("live unit")
}

fn handle_id(host: &ScriptHost, name: &str) -> i32 {
    host.vm().global(name).and_then(|v| v.as_handle()).expect("handle") as i32
}

const KEY_BASE: u32 = 0x8_0200;

#[test]
fn rescue_fires_the_player_event_then_the_unit_event_with_the_rescuer() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set rescued = CreateUnit(Player(1), 'hfoo', I2R(0), I2R(0), I2R(0))
    set rescuer = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function on_rescue)
    call TriggerRegisterPlayerUnitEvent(t1, Player(1), 19, null)
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function on_rescue)
    call TriggerRegisterUnitEvent(t2, rescued, 63)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let (rescued, rescuer) = (unit_key(&host, "rescued"), unit_key(&host, "rescuer"));
    let game = host.game_handle();
    event::fire_rescued(&game, host.vm_mut(), rescued, rescuer);
    run(&mut host);
    assert_eq!(int(&host, "fired"), 2, "the owner's player-unit listener and the unit's own listener");
    assert_eq!(int(&host, "seen_rescuer"), handle_id(&host, "rescuer"));
    assert_eq!(
        int(&host, "seen_rescued"),
        handle_id(&host, "rescued"),
        "the trigger unit is the rescued unit"
    );
}

#[test]
fn rescuer_is_null_for_other_events() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set rescued = CreateUnit(Player(1), 'hfoo', I2R(0), I2R(0), I2R(0))
    set rescuer = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function on_rescue)
    call TriggerRegisterUnitEvent(t1, rescued, 62)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let (rescued, rescuer) = (unit_key(&host, "rescued"), unit_key(&host, "rescuer"));
    // An attacked event (62) that carries a rescuer datum anyway: the getter does not answer for it.
    let slots = EventSlots {
        id: 0x3E,
        unit: Some(rescued),
        data: vec![(Field::Rescuer, Datum::Object(rescuer))],
        ..Default::default()
    };
    let game = host.game_handle();
    event::fire_event(&game, host.vm_mut(), rescued, KEY_BASE + 0x3E, slots);
    run(&mut host);
    assert_eq!(int(&host, "fired"), 1);
    assert_eq!(int(&host, "seen_rescuer"), 0);
}

#[test]
fn selling_unit_is_the_payload_for_player_events_and_the_trigger_unit_for_unit_events() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set shop = CreateUnit(Player(1), 'hfoo', I2R(0), I2R(0), I2R(0))
    set other = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function on_sell)
    call TriggerRegisterPlayerUnitEvent(t1, Player(0), 269, null)
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function on_sell)
    call TriggerRegisterUnitEvent(t2, shop, 286)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    let (shop, other) = (unit_key(&host, "shop"), unit_key(&host, "other"));
    let game = host.game_handle();
    // Player-unit sell unit (0x10D): the shop is a payload field.
    event::fire_event(
        &game,
        host.vm_mut(),
        ObjKey(0),
        KEY_BASE + 0x10D,
        EventSlots {
            id: 0x10D,
            unit: Some(other),
            data: vec![(Field::Seller, Datum::Object(shop))],
            ..Default::default()
        },
    );
    run(&mut host);
    assert_eq!(int(&host, "seen_selling"), handle_id(&host, "shop"));
    // Unit twin (0x11E): the trigger unit is the shop, whatever the payload holds.
    event::fire_event(
        &game,
        host.vm_mut(),
        shop,
        KEY_BASE + 0x11E,
        EventSlots {
            id: 0x11E,
            unit: Some(shop),
            data: vec![(Field::Seller, Datum::Object(other))],
            ..Default::default()
        },
    );
    run(&mut host);
    assert_eq!(int(&host, "fired"), 2);
    assert_eq!(int(&host, "seen_selling"), handle_id(&host, "shop"));
}
