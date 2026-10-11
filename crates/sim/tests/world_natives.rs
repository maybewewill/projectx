//! Waygates, resource nodes, defeat conditions, trackables, object and item-type data, the slot counts and the
//! animation log (`natives/economy.rs`, `defeat.rs`, `object.rs`, `item.rs`, `gamestate.rs`, `animation.rs`).
//!
//! Each native is driven from a JASS script; the state is read back through the handles the script stores in its
//! globals, and through the game objects behind them.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ItemType, Object, UnitType};
use sim::natives::presentation::{PresentationEvent, PropValue};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type item extends widget
type rect extends handle
type defeatcondition extends handle
type trackable extends handle
type itemtype extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateItem takes integer itemid, real x, real y returns item
native GetItemTypeId takes item i returns integer
native UnitId takes string unitName returns integer
native UnitId2String takes integer unitId returns string
native GetFoodMade takes integer unitId returns integer
native GetFoodUsed takes integer unitId returns integer
native SetPlayers takes integer count returns nothing
native GetPlayers takes nothing returns integer
native SetTeams takes integer count returns nothing
native GetTeams takes nothing returns integer
native WaygateActivate takes unit whichWaygate, boolean activate returns nothing
native WaygateIsActive takes unit whichWaygate returns boolean
native WaygateSetDestination takes unit whichWaygate, real x, real y returns nothing
native WaygateGetDestinationX takes unit whichWaygate returns real
native WaygateGetDestinationY takes unit whichWaygate returns real
native AddResourceAmount takes unit whichUnit, integer amount returns nothing
native GetResourceAmount takes unit whichUnit returns integer
native SetResourceAmount takes unit whichUnit, integer amount returns nothing
native CreateDefeatCondition takes nothing returns defeatcondition
native DefeatConditionSetDescription takes defeatcondition condition, string description returns nothing
native DestroyDefeatCondition takes defeatcondition condition returns nothing
native CreateTrackable takes string trackableModelPath, real x, real y, real z returns trackable
native IsItemIdPowerup takes integer itemId returns boolean
native IsItemIdSellable takes integer itemId returns boolean
native IsItemIdPawnable takes integer itemId returns boolean
native IsItemSellable takes item theItem returns boolean
native GetItemType takes item i returns itemtype
native GetItemLevel takes item i returns integer
native GetItemUserData takes item i returns integer
native SetItemUserData takes item i, integer data returns nothing
native SetItemDropID takes item i, integer id returns nothing
native QueueUnitAnimation takes unit whichUnit, string whichAnimation returns nothing
native AddUnitAnimationProperties takes unit whichUnit, string animProperties, boolean add returns nothing
native ResetUnitLookAt takes unit whichUnit returns nothing
native GetHandleId takes handle h returns integer
"#;

const FOOTMAN: u32 = 0x6866_6F6F;
const WAYGATE: u32 = 0x6967_7477; // 'igtw'
const FARM: u32 = 0x6868_6F75; // 'hhou'
const ITEM_PLAIN: u32 = 0x4974_6D41; // 'ItmA'
const ITEM_POWER: u32 = 0x4974_6D42; // 'ItmB'
const WAYGATE_ABILITY: u32 = 0x4177_7270; // 'Awrp'

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        let ut = |name: &str| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x43D2_0000),
            ..Default::default()
        };
        g.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                food_used: 2,
                ..ut("Footman")
            },
        );
        g.unit_types_mut().insert(
            FARM,
            UnitType {
                food_made: 8,
                ..ut("Farm")
            },
        );
        g.unit_types_mut().insert(
            WAYGATE,
            UnitType {
                abilities: vec![WAYGATE_ABILITY],
                ..ut("Waygate")
            },
        );
        let item = |class: u32, level: i32, powerup: bool| ItemType {
            class,
            level_ex: level,
            level_plain: level,
            flag_48: 1,
            charges: 0,
            flags: 0,
            powerup,
            sellable: !powerup,
            pawnable: powerup,
        };
        g.item_types_mut().insert(ITEM_PLAIN, item(3, 2, false));
        g.item_types_mut().insert(ITEM_POWER, item(6, 1, true));
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

fn real(host: &ScriptHost, name: &str) -> F32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_real())
        .unwrap_or_else(|| panic!("global {name} is not a real"))
}

fn handle(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"))
}

#[test]
fn waygate_state_and_destination_need_the_waygate_ability() {
    let src = r#"
globals
  unit gWg = null
  unit gFoot = null
  boolean gActive = false
  boolean gFootActive = false
  real gX = 0.0
  real gY = 0.0
  real gFootX = 9.0
endglobals
function Main takes nothing returns nothing
  set gWg = CreateUnit(Player(0), 'igtw', I2R(0), I2R(0), I2R(0))
  set gFoot = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  call WaygateSetDestination(gWg, I2R(100), I2R(-20))
  call WaygateActivate(gWg, true)
  set gActive = WaygateIsActive(gWg)
  set gX = WaygateGetDestinationX(gWg)
  set gY = WaygateGetDestinationY(gWg)
  call WaygateActivate(gFoot, true)
  set gFootActive = WaygateIsActive(gFoot)
  call WaygateSetDestination(gFoot, I2R(5), I2R(5))
  set gFootX = WaygateGetDestinationX(gFoot)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert!(boolean(&host, "gActive"));
    assert_eq!(real(&host, "gX").to_f32(), 100.0);
    assert_eq!(real(&host, "gY").to_f32(), -20.0);
    assert!(
        !boolean(&host, "gFootActive"),
        "a unit without the ability is not a waygate"
    );
    assert_eq!(
        real(&host, "gFootX").to_f32(),
        0.0,
        "the default destination is 0"
    );
}

#[test]
fn resource_nodes_round_trip_and_ignore_other_units() {
    let src = r#"
globals
  unit gMine = null
  unit gFoot = null
  integer gAmt = 0
  integer gAmt2 = 0
  integer gFootAmt = 0
endglobals
function Main takes nothing returns nothing
  set gMine = CreateUnit(Player(0), 'hhou', I2R(0), I2R(0), I2R(0))
  set gFoot = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  call AddResourceAmount(gFoot, 50)
  set gFootAmt = GetResourceAmount(gFoot)
endfunction
function Later takes nothing returns nothing
  call AddResourceAmount(gMine, 25)
  set gAmt = GetResourceAmount(gMine)
  call SetResourceAmount(gMine, gAmt + 5)
  set gAmt2 = GetResourceAmount(gMine)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert_eq!(int(&host, "gFootAmt"), 0, "a non-node keeps no amount");
    let mine = host
        .game()
        .unit_of_handle(handle(&host, "gMine"))
        .expect("mine unit");
    host.game_mut().add_resource_node(mine, 500);
    host.start("Later", &[]).expect("start");
    run(&mut host, 2);
    assert_eq!(int(&host, "gAmt"), 525);
    assert_eq!(int(&host, "gAmt2"), 530);
    assert_eq!(host.game().resource_amount(mine), Some(530));
}

#[test]
fn defeat_condition_keeps_its_text_until_destroyed() {
    let src = r#"
globals
  defeatcondition gDc = null
endglobals
function Main takes nothing returns nothing
  set gDc = CreateDefeatCondition()
  call DefeatConditionSetDescription(gDc, "You lose")
endfunction
function Later takes nothing returns nothing
  call DestroyDefeatCondition(gDc)
  call DefeatConditionSetDescription(gDc, "Ignored")
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    let id = handle(&host, "gDc");
    let key = host.game().handle_object(id).expect("live condition");
    match host.game().object(key) {
        Object::DefeatCondition(d) => assert_eq!(d.description, "You lose"),
        _ => panic!("not a defeat condition"),
    }
    host.start("Later", &[]).expect("start");
    run(&mut host, 2);
    assert!(
        host.game().handle_object(id).is_none(),
        "the handle is released"
    );
    match host.game().object(key) {
        Object::DefeatCondition(d) => {
            assert_eq!(d.description, "You lose", "a destroyed one is not changed")
        }
        _ => panic!("not a defeat condition"),
    };
}

#[test]
fn trackable_is_a_handle_with_its_position() {
    let src = r#"
globals
  trackable gTr = null
endglobals
function Main takes nothing returns nothing
  set gTr = CreateTrackable("Rock.mdx", I2R(1), I2R(2), I2R(3))
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    let key = host
        .game()
        .handle_object(handle(&host, "gTr"))
        .expect("live trackable");
    match host.game().object(key) {
        Object::Trackable(t) => {
            assert_eq!(t.model, "Rock.mdx");
            assert_eq!((t.x.to_f32(), t.y.to_f32(), t.z.to_f32()), (1.0, 2.0, 3.0));
        }
        _ => panic!("not a trackable"),
    };
}

#[test]
fn unit_names_and_food_come_from_the_type_registry() {
    let src = r#"
globals
  integer gId = 0
  string gName = ""
  boolean gSame = false
  integer gFoodUsed = 0
  integer gFoodMade = 0
  integer gUnknown = -1
endglobals
function Main takes nothing returns nothing
  set gId = UnitId("Footman")
  set gName = UnitId2String('hfoo')
  set gSame = gName == "Footman"
  set gFoodUsed = GetFoodUsed('hfoo')
  set gFoodMade = GetFoodMade('hhou')
  set gUnknown = UnitId("NoSuchUnit")
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert_eq!(int(&host, "gId"), FOOTMAN as i32);
    assert!(boolean(&host, "gSame"));
    assert_eq!(int(&host, "gFoodUsed"), 2);
    assert_eq!(int(&host, "gFoodMade"), 8);
    assert_eq!(int(&host, "gUnknown"), 0, "an unknown name gives 0");
}

#[test]
fn item_type_flags_and_item_data_round_trip() {
    let src = r#"
globals
  boolean gPlainPower = true
  boolean gPowerPower = false
  boolean gPlainSell = false
  boolean gPowerSell = true
  boolean gPowerPawn = false
  integer gClass = 0
  integer gLevel = 0
  integer gUser = 0
  integer gUnknownClass = 0
  item gIt = null
endglobals
function Main takes nothing returns nothing
  set gPlainPower = IsItemIdPowerup('ItmA')
  set gPowerPower = IsItemIdPowerup('ItmB')
  set gPlainSell = IsItemIdSellable('ItmA')
  set gPowerSell = IsItemIdSellable('ItmB')
  set gPowerPawn = IsItemIdPawnable('ItmA')
  set gIt = CreateItem('ItmA', I2R(0), I2R(0))
  set gClass = GetHandleId(GetItemType(gIt))
  set gLevel = GetItemLevel(gIt)
  call SetItemUserData(gIt, 42)
  set gUser = GetItemUserData(gIt)
  call SetItemDropID(gIt, 7)
  set gUnknownClass = GetHandleId(GetItemType(null))
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert!(!boolean(&host, "gPlainPower"));
    assert!(boolean(&host, "gPowerPower"));
    assert!(
        boolean(&host, "gPlainSell"),
        "the plain item is sellable (the test table)"
    );
    assert!(
        !boolean(&host, "gPowerSell"),
        "the powerup is not sellable (the test table)"
    );
    assert!(!boolean(&host, "gPowerPawn"));
    assert_eq!(int(&host, "gClass"), 3);
    assert_eq!(int(&host, "gLevel"), 2);
    assert_eq!(int(&host, "gUser"), 42);
    assert_eq!(
        int(&host, "gUnknownClass"),
        7,
        "a null item reports the class 7 (any)"
    );
}

#[test]
fn slot_counts_are_stored_as_given() {
    let src = r#"
globals
  integer gPlayers = 0
  integer gTeams = 0
endglobals
function Main takes nothing returns nothing
  call SetPlayers(12)
  call SetTeams(4)
  set gPlayers = GetPlayers()
  set gTeams = GetTeams()
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert_eq!(int(&host, "gPlayers"), 12);
    assert_eq!(int(&host, "gTeams"), 4);
}

#[test]
fn animation_calls_are_logged_for_live_units_only() {
    let src = r#"
globals
  unit gU = null
endglobals
function Main takes nothing returns nothing
  set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  call QueueUnitAnimation(gU, "stand")
  call AddUnitAnimationProperties(gU, "alternate", true)
  call ResetUnitLookAt(gU)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    let id = handle(&host, "gU");
    let events: Vec<PresentationEvent> = host.game().presentation().log.events().to_vec();
    let commands: Vec<(&str, Vec<PropValue>)> = events
        .iter()
        .filter_map(|e| match e {
            PresentationEvent::Command { name, args } => Some((*name, args.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        commands,
        vec![
            (
                "QueueUnitAnimation",
                vec![PropValue::Handle(id), PropValue::Text("stand".into())]
            ),
            (
                "AddUnitAnimationProperties",
                vec![
                    PropValue::Handle(id),
                    PropValue::Text("alternate".into()),
                    PropValue::Bool(true),
                ]
            ),
            ("ResetUnitLookAt", vec![PropValue::Handle(id)]),
        ]
    );
}
