//! Group order natives (`natives/order.rs`, `GROUP_NATIVES`): the delivery order, the reach of point and target orders
//! (the first 12 entries), the return value ("any member accepted") and the target that is not a unit.
//!
//! Order ids: holdposition 0xD0019 (immediate; stop 0xD0004 is not recorded as an issued order by the model), move 0xD0012 (point), attack 0xD000F (target). The events record the order in
//! which the members receive it: the owner's player-unit event (38 immediate, 39 point, 40 target) fires once per
//! accepted unit, during the native call, so a trigger action can read the unit (`GetTriggerUnit`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ItemType, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type item extends widget
type group extends handle
type boolexpr extends handle
type trigger extends handle
type triggeraction extends handle
type event extends handle
type playerunitevent extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateItem takes integer itemid, real x, real y returns item
native CreateGroup takes nothing returns group
native GroupAddUnit takes group g, unit whichUnit returns nothing
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native ConvertPlayerUnitEvent takes integer i returns playerunitevent
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, playerunitevent whichEvent, boolexpr filter returns event
native GetTriggerUnit takes nothing returns unit
native GetUnitUserData takes unit u returns integer
native SetUnitUserData takes unit u, integer data returns nothing
native GetUnitCurrentOrder takes unit whichUnit returns integer
native GroupImmediateOrderById takes group g, integer order returns boolean
native GroupPointOrder takes group g, string order, real x, real y returns boolean
native GroupPointOrderById takes group g, integer order, real x, real y returns boolean
native GroupTargetOrderById takes group g, integer order, widget target returns boolean
native GroupTargetOrder takes group g, string order, widget target returns boolean
native OrderId takes string orderStr returns integer
"#;

const FOOTMAN: u32 = 0x6866_6F6F;
const RANGER: u32 = 0x6872_6E67;
const ITEM: u32 = 0x4974_6D41;
const MOVE: u32 = 0xD_0012;

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).ok()?;
    Some(Tables::from_game_dll(&bytes).expect("lookup tables"))
}

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    {
        let mut g = host.game_mut();
        let ut = |name: &str| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x43D2_0000),
            move_speed: F32::from_bits(270.0f32.to_bits()),
            ..Default::default()
        };
        g.unit_types_mut().insert(FOOTMAN, ut("Footman"));
        g.unit_types_mut().insert(RANGER, ut("Ranger"));
        g.set_type_orders(
            FOOTMAN,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
        g.set_type_orders(
            RANGER,
            TypeOrderData {
                abilities: Vec::new(),
            },
        );
        g.item_types_mut().insert(
            ITEM,
            ItemType {
                class: 1,
                level_ex: 1,
                level_plain: 1,
                flag_48: 1,
                charges: 0,
                flags: 0,
                powerup: false,
                sellable: false,
                pawnable: false,
            },
        );
    }
    host
}

fn run(host: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
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

/// The recorder function shared by the scripts (its globals are declared in each script's `globals` block):
/// `OnIssued` counts the events and appends the user data of each unit, so `gSeq` reads the units in delivery order
/// (`gSeq = gSeq * 10 + data`, one digit per unit).
const ONISSUED: &str = r#"
function OnIssued takes nothing returns nothing
  set gSeq = gSeq * 10 + GetUnitUserData(GetTriggerUnit())
  set gCount = gCount + 1
endfunction
"#;

#[test]
fn immediate_group_order_reaches_members_in_insertion_order() {
    let src = format!(
        r#"globals
  integer gSeq = 0
  integer gCount = 0
  group gG = null
  trigger gT = null
  boolean gOk = false
  unit gA = null
  unit gB = null
  unit gC = null
endglobals
{ONISSUED}
function Main takes nothing returns nothing
  set gA = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  set gB = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  set gC = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  call SetUnitUserData(gA, 1)
  call SetUnitUserData(gB, 2)
  call SetUnitUserData(gC, 3)
  set gG = CreateGroup()
  call GroupAddUnit(gG, gC)
  call GroupAddUnit(gG, gA)
  call GroupAddUnit(gG, gB)
  set gT = CreateTrigger()
  call TriggerRegisterPlayerUnitEvent(gT, Player(0), ConvertPlayerUnitEvent(38), null)
  call TriggerAddAction(gT, function OnIssued)
  set gOk = GroupImmediateOrderById(gG, 0xD0019)
endfunction
"#
    );
    let mut host = host_for(&src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 3);
    assert!(
        boolean(&host, "gOk"),
        "an accepting member makes the result true"
    );
    assert_eq!(int(&host, "gCount"), 3, "every member gets the order");
    assert_eq!(
        int(&host, "gSeq"),
        312,
        "delivery follows the insertion order (C, A, B)"
    );
    let issued: Vec<_> = host
        .game()
        .orders()
        .issued()
        .iter()
        .map(|r| r.unit)
        .collect();
    assert_eq!(issued.len(), 3);
}

#[test]
fn point_group_order_reaches_only_the_first_twelve_entries() {
    let src = format!(
        r#"globals
  integer gSeq = 0
  integer gCount = 0
  group gG = null
  trigger gT = null
  boolean gOk = false
  integer gI = 0
  unit gFirst = null
  unit gLast = null
  unit gU = null
endglobals
{ONISSUED}
function Main takes nothing returns nothing
  set gG = CreateGroup()
  loop
    exitwhen gI == 13
    set gI = gI + 1
    set gU = CreateUnit(Player(0), 'hfoo', I2R(gI), I2R(0), I2R(0))
    call SetUnitUserData(gU, gI)
    call GroupAddUnit(gG, gU)
    if gI == 1 then
      set gFirst = gU
    endif
    set gLast = gU
  endloop
  set gT = CreateTrigger()
  call TriggerRegisterPlayerUnitEvent(gT, Player(0), ConvertPlayerUnitEvent(39), null)
  call TriggerAddAction(gT, function OnIssued)
  set gOk = GroupPointOrderById(gG, {MOVE}, I2R(100), I2R(50))
endfunction
"#
    );
    let mut host = host_for(&src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 3);
    assert!(boolean(&host, "gOk"));
    assert_eq!(
        int(&host, "gCount"),
        12,
        "the point order reaches 12 entries"
    );
    let last = host
        .vm()
        .global("gLast")
        .and_then(|v| v.as_handle())
        .expect("handle");
    let first = host
        .vm()
        .global("gFirst")
        .and_then(|v| v.as_handle())
        .expect("handle");
    let last_key = host.game().unit_of_handle(last).expect("unit");
    let first_key = host.game().unit_of_handle(first).expect("unit");
    assert!(
        !host.game().unit_order_ids(first_key).is_empty(),
        "the first entry got the order"
    );
    assert!(
        host.game().unit_order_ids(last_key).is_empty(),
        "the thirteenth entry got no order"
    );
}

#[test]
fn group_point_order_returns_true_when_any_member_accepts() {
    // A ranger refuses point orders (see order_natives.rs); with a footman in the group the result is true.
    let src = format!(
        r#"globals
  integer gSeq = 0
  integer gCount = 0
  group gMixed = null
  group gRangers = null
  trigger gT = null
  boolean gMixedOk = false
  boolean gRangerOk = false
  unit gR = null
  unit gF = null
endglobals
{ONISSUED}
function Main takes nothing returns nothing
  set gR = CreateUnit(Player(0), 'hrng', I2R(0), I2R(0), I2R(0))
  set gF = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  call SetUnitUserData(gF, 7)
  set gMixed = CreateGroup()
  call GroupAddUnit(gMixed, gR)
  call GroupAddUnit(gMixed, gF)
  set gRangers = CreateGroup()
  call GroupAddUnit(gRangers, gR)
  set gT = CreateTrigger()
  call TriggerRegisterPlayerUnitEvent(gT, Player(0), ConvertPlayerUnitEvent(39), null)
  call TriggerAddAction(gT, function OnIssued)
  set gRangerOk = GroupPointOrder(gRangers, "move", I2R(10), I2R(10))
  set gMixedOk = GroupPointOrder(gMixed, "move", I2R(10), I2R(10))
endfunction
"#
    );
    let mut host = host_for(&src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 3);
    assert!(
        !boolean(&host, "gRangerOk"),
        "a group of rangers is refused"
    );
    assert!(
        boolean(&host, "gMixedOk"),
        "one accepting footman makes the result true"
    );
    assert_eq!(int(&host, "gCount"), 1, "only the footman gets the event");
    assert_eq!(int(&host, "gSeq"), 7);
}

#[test]
fn target_group_order_with_a_non_unit_target_returns_true_and_issues_nothing() {
    let src = format!(
        r#"globals
  integer gSeq = 0
  integer gCount = 0
  group gG = null
  trigger gT = null
  boolean gOk = false
  item gItem = null
  unit gU = null
endglobals
{ONISSUED}
function Main takes nothing returns nothing
  set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  set gG = CreateGroup()
  call GroupAddUnit(gG, gU)
  set gItem = CreateItem('ItmA', I2R(0), I2R(0))
  set gT = CreateTrigger()
  call TriggerRegisterPlayerUnitEvent(gT, Player(0), ConvertPlayerUnitEvent(40), null)
  call TriggerAddAction(gT, function OnIssued)
  set gOk = GroupTargetOrderById(gG, 0xD000F, gItem)
endfunction
"#
    );
    let mut host = host_for(&src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 3);
    assert!(
        boolean(&host, "gOk"),
        "an item target gives true (CONFIRMED early exit)"
    );
    assert_eq!(int(&host, "gCount"), 0, "nothing is issued");
    let u = host
        .vm()
        .global("gU")
        .and_then(|v| v.as_handle())
        .expect("handle");
    let key = host.game().unit_of_handle(u).expect("unit");
    assert!(
        host.game().unit_order_ids(key).is_empty(),
        "the footman has no order"
    );
}
