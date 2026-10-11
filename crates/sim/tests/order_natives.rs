//! Order natives through `ScriptHost` (`natives/order.rs`, the model in `orders.rs`, spec
//! `docs/spec/orders.md` and `orders-2.md`).
//!
//! The queue, limit, lock and id checks need no lookup tables. The movement checks need the softfloat tables of
//! the user's `Game.dll` (`GAME_DLL`): without it the mover does not step, so those tests return early.
//!
//! The expected movement positions are the model's own output (bit patterns, UNVERIFIED against the original):
//! they pin the model so that a change to the mover shows up here.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, PathingMap, TypeOrderData, NAMED_ORDERS, ORDER_LIMIT};
use sim::pathing::{FlagLink, Level, Pyramid, Search};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type location extends handle
type boolexpr extends handle
type trigger extends handle
type triggeraction extends handle
type event extends handle
type unitevent extends handle
type playerunitevent extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native ConvertUnitEvent takes integer i returns unitevent
native ConvertPlayerUnitEvent takes integer i returns playerunitevent
native TriggerRegisterUnitEvent takes trigger t, unit whichUnit, unitevent whichEvent returns event
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, playerunitevent whichEvent, boolexpr filter returns event
native IssueImmediateOrder takes unit whichUnit, string order returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
native GetUnitCurrentOrder takes unit whichUnit returns integer
native GetIssuedOrderId takes nothing returns integer
native GetOrderPointX takes nothing returns real
native GetOrderPointY takes nothing returns real
native GetOrderTargetUnit takes nothing returns unit
native OrderId takes string orderStr returns integer
native OrderId2String takes integer orderId returns string
native GetUnitX takes unit whichUnit returns real
native GetUnitY takes unit whichUnit returns real
native GetUnitFacing takes unit whichUnit returns real
native GetRandomInt takes integer lo, integer hi returns integer
"#;

/// Raw type id of the test footman ('hfoo') and the test ranger ('hrng', no abilities).
const FOOTMAN: u32 = 0x6866_6F6F;
const RANGER: u32 = 0x6872_6E67;

/// The 'hfoo' rawcode in JASS.
const FOOTMAN_CODE: &str = "'hfoo'";

/// Order ids (the spec's values, CONFIRMED).
const MOVE: i32 = 0xD0012;
const ATTACK: i32 = 0xD000F;
const HOLD: i32 = 0xD0019;
const NOTE_STOP: u32 = 0xD0144;
const NOTE_STARTED_OR_CLEARED: u32 = 0xD02A6;
/// Facing after the straight move (radians), the mover's value (see `straight_moves_arrive_with_model_positions`):
/// the unit stops with `facing = normalise(heading)`, and the last heading is slightly below zero (the steering
/// turned the 3.4e-4 rad arccosine drift back past zero), which the normaliser folds to 2*pi - 0.0054.
const FACING_BITS: u32 = 0x40C8_E59E;

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

/// A host with the test unit types. The footman can move and attack and moves 270 world units per second; the
/// ranger has no abilities.
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
            max_mana: F32::ZERO,
            start_mana: F32::ZERO,
            inventory_size: 0,
            ..Default::default()
        };
        let speed = F32::from_bits(270.0f32.to_bits());
        g.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                move_speed: speed,
                // A unit only accelerates while the heading error is inside its propulsion window
                // (movement-6.md), so the fixture needs the footman's real turn rate and window.
                turn_speed: F32::from_bits(0x3F19_999A), // 0.6, the footman's turn rate
                prop_window: F32::from_bits(0x3F86_0A92), // 60 degrees in radians
                ..ut("Footman")
            },
        );
        g.unit_types_mut().insert(
            RANGER,
            UnitType {
                move_speed: speed,
                turn_speed: F32::from_bits(0x3F19_999A), // 0.6, the footman's turn rate
                prop_window: F32::from_bits(0x3F86_0A92), // 60 degrees in radians
                ..ut("Ranger")
            },
        );
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

/// The unit object behind the unit global `name`.
fn unit_key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"));
    host.game()
        .unit_of_handle(id)
        .unwrap_or_else(|| panic!("handle {id} is not a live unit"))
}

/// A script with one footman `gU` at `(x, y)` and the globals used by the checks.
fn footman_script(x: i32, y: i32, body: &str) -> String {
    format!(
        r#"
globals
  unit gU = null
  integer gInt = 0
  integer gInt2 = 0
  boolean gOk = false
  boolean gOk2 = false
  real gReal = 0.0
  real gReal2 = 0.0
endglobals

function Setup takes nothing returns nothing
  set gU = CreateUnit(Player(0), {FOOTMAN_CODE}, I2R({x}), I2R({y}), I2R(0))
endfunction

function Main takes nothing returns nothing
  call Setup()
  {body}
endfunction
"#
    )
}

#[test]
fn order_ids_and_names() {
    // Names and ids of the spec table (orders.md section 1); unknown names give 0, unknown ids give "".
    let src = r#"
globals
  boolean gOk = false
endglobals
function Main takes nothing returns nothing
  set gOk = OrderId("move") == $D0012 and OrderId("attack") == $D000F and OrderId("stop") == $D0004
  set gOk = gOk and OrderId("smart") == $D0003 and OrderId("AImove") == $D0014
  set gOk = gOk and OrderId("patrol") == $D0016 and OrderId("holdposition") == $D0019
  set gOk = gOk and OrderId("nosuchorder") == 0
  set gOk = gOk and OrderId2String($D0003) == "smart" and OrderId2String($D0012) == "move"
  set gOk = gOk and OrderId2String($D0014) == "AImove" and OrderId2String(12345) == ""
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert!(boolean(&host, "gOk"));
    // Every named id round-trips through its name.
    for &(name, id) in NAMED_ORDERS {
        assert_eq!(sim::orders::order_id_by_name(name), Some(id));
        assert_eq!(sim::orders::order_name(id), Some(name));
    }
}

#[test]
fn replace_clears_then_queues_and_append_keeps_the_head() {
    let mut host = host_for(&footman_script(
        0,
        0,
        r#"set gOk = IssuePointOrder(gU, "move", I2R(100), I2R(0))
  set gOk2 = IssuePointOrder(gU, "attack", I2R(400), I2R(400))
  set gInt = GetUnitCurrentOrder(gU)"#,
    ));
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert!(boolean(&host, "gOk") && boolean(&host, "gOk2"));
    // The replacing attack is the current order.
    assert_eq!(int(&host, "gInt"), ATTACK);
    let u = unit_key(&host, "gU");
    assert_eq!(host.game().unit_order_ids(u), vec![ATTACK as u32]);
    // The replace cleared the move: stop notification, then the cleared notification of the move.
    let notes = host.game().orders().notices().to_vec();
    let stop_at = notes.iter().position(|&(_, id)| id == NOTE_STOP);
    assert!(stop_at.is_some(), "replace records the stop notification");
    assert!(notes[stop_at.unwrap()..]
        .iter()
        .any(|&(_, id)| id == NOTE_STARTED_OR_CLEARED));

    // Shift-queued (append) orders go behind the current one and do not change the head.
    assert!(host.game_mut().queue_point_order(
        u,
        MOVE as u32,
        F32::from_bits(5.0f32.to_bits()),
        F32::from_bits(5.0f32.to_bits())
    ));
    assert_eq!(
        host.game().unit_order_ids(u),
        vec![ATTACK as u32, MOVE as u32]
    );
    let head = host.game().unit_order_ids(u)[0];
    assert_eq!(head, ATTACK as u32);
}

#[test]
fn stop_clears_the_list_and_hold_stays_current() {
    let mut host = host_for(&footman_script(
        0,
        0,
        r#"set gOk = IssuePointOrder(gU, "move", I2R(0), I2R(100))
  set gOk2 = IssueImmediateOrder(gU, "stop")
  set gInt = GetUnitCurrentOrder(gU)
  set gOk = gOk and IssueImmediateOrderById(gU, $D0019)
  set gInt2 = GetUnitCurrentOrder(gU)"#,
    ));
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert!(boolean(&host, "gOk2"), "stop is accepted");
    assert_eq!(int(&host, "gInt"), 0, "nothing is current after stop");
    assert_eq!(
        int(&host, "gInt2"),
        HOLD,
        "hold position becomes the current order"
    );
    let u = unit_key(&host, "gU");
    assert_eq!(host.game().unit_order_ids(u), vec![HOLD as u32]);
}

#[test]
fn queue_limit_is_501_orders() {
    let mut host = host_for(&footman_script(
        0,
        0,
        // Hold position never completes, so the list keeps every appended order.
        r#"set gOk = IssueImmediateOrderById(gU, $D0019)"#,
    ));
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    let u = unit_key(&host, "gU");
    assert!(boolean(&host, "gOk"));
    let mut accepted = 1usize;
    for i in 0..(ORDER_LIMIT as usize + 100) {
        let x = F32::from_bits(((i % 50) as f32 + 1.0).to_bits());
        if host.game_mut().queue_point_order(u, MOVE as u32, x, x) {
            accepted += 1;
        }
    }
    // The count is refused once it is above 500: 501 orders in total (the first one plus 500 appended).
    assert_eq!(accepted, ORDER_LIMIT as usize + 1);
    assert_eq!(
        host.game().unit_order_ids(u).len(),
        ORDER_LIMIT as usize + 1
    );
}

#[test]
fn locked_unit_refuses_orders() {
    let src = r#"
globals
  unit gU = null
  boolean gOk = false
  boolean gOk2 = false
  integer gInt = 0
endglobals

function Setup takes nothing returns nothing
  set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
endfunction

function Check takes nothing returns nothing
  set gOk = IssueImmediateOrder(gU, "stop")
  set gOk2 = IssuePointOrder(gU, "move", I2R(3), I2R(3))
  set gInt = GetUnitCurrentOrder(gU)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Setup", &[]).expect("start");
    run(&mut host, 1);
    let u = unit_key(&host, "gU");
    host.game_mut().set_unit_locked(u, true);
    host.start("Check", &[]).expect("start");
    run(&mut host, 1);
    assert!(
        !boolean(&host, "gOk"),
        "a locked unit fails the stop validation"
    );
    assert!(
        boolean(&host, "gOk2"),
        "the move validates; the dispatcher drops it"
    );
    assert_eq!(int(&host, "gInt"), 0);
    assert!(host.game().unit_order_ids(u).is_empty());
}

#[test]
fn issued_order_events_read_their_context() {
    let src = r#"
globals
  trigger gT = null
  trigger gT2 = null
  unit gU = null
  integer gId = 0
  real gX = 0.0
  real gY = 0.0
  integer gPlayerCount = 0
  integer gUnitCount = 0
  unit gTargetUnit = null
endglobals

function OnPlayerPoint takes nothing returns nothing
  set gId = GetIssuedOrderId()
  set gX = GetOrderPointX()
  set gY = GetOrderPointY()
  set gPlayerCount = gPlayerCount + 1
endfunction

function OnUnitPoint takes nothing returns nothing
  set gUnitCount = gUnitCount + 1
endfunction

function Main takes nothing returns nothing
  set gU = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  set gT = CreateTrigger()
  call TriggerRegisterPlayerUnitEvent(gT, Player(0), ConvertPlayerUnitEvent(39), null)
  call TriggerAddAction(gT, function OnPlayerPoint)
  set gT2 = CreateTrigger()
  call TriggerRegisterUnitEvent(gT2, gU, ConvertUnitEvent(76))
  call TriggerAddAction(gT2, function OnUnitPoint)
  call IssuePointOrder(gU, "move", I2R(100), I2R(50))
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 3);
    assert_eq!(
        int(&host, "gPlayerCount"),
        1,
        "the player-unit point event ran once"
    );
    assert_eq!(int(&host, "gUnitCount"), 1, "the unit point event ran once");
    assert_eq!(int(&host, "gId"), MOVE);
    assert_eq!(real(&host, "gX").to_f32(), 100.0);
    assert_eq!(real(&host, "gY").to_f32(), 50.0);
}

#[test]
fn immediate_order_event_is_the_immediate_kind() {
    let src = r#"
globals
  trigger gT = null
  integer gId = 0
  integer gMain = 0
  integer gCount = 0
endglobals

function OnImmediate takes nothing returns nothing
  set gId = GetIssuedOrderId()
  set gCount = gCount + 1
endfunction

function Main takes nothing returns nothing
  local unit u = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  set gT = CreateTrigger()
  call TriggerRegisterPlayerUnitEvent(gT, Player(0), ConvertPlayerUnitEvent(38), null)
  call TriggerAddAction(gT, function OnImmediate)
  call IssueImmediateOrder(u, "holdposition")
  set gMain = GetIssuedOrderId()
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 3);
    assert_eq!(
        int(&host, "gCount"),
        1,
        "the player-unit immediate event ran once"
    );
    assert_eq!(
        int(&host, "gId"),
        HOLD,
        "the event context reads hold position"
    );
    assert_eq!(
        int(&host, "gMain"),
        0,
        "outside an order event the getter reads 0"
    );
}

#[test]
fn point_order_for_a_ranger_is_refused() {
    // A unit type with no abilities refuses every order: the validator finds no ability.
    let src = footman_script(
        0,
        0,
        r#"set gOk = IssuePointOrder(gU, "move", I2R(3), I2R(3))"#,
    )
    .replace("'hfoo'", "'hrng'");
    let mut host = host_for(&src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert!(!boolean(&host, "gOk"), "the ranger has no move ability");
    let u = unit_key(&host, "gU");
    assert!(host.game().unit_order_ids(u).is_empty());
}

/// Moves a footman from `(0, 0)` to `(100, 0)` and then to `(0, 100)`: exact model positions, the facing and the
/// arrival. Needs `GAME_DLL` (the softfloat tables): returns early otherwise.
#[test]
fn straight_moves_arrive_with_model_positions() {
    if tables().is_none() {
        eprintln!("GAME_DLL not set: skipping the movement test");
        return;
    }
    let mut host = host_for(&footman_script(
        0,
        0,
        r#"set gOk = IssuePointOrder(gU, "move", I2R(100), I2R(0))"#,
    ));
    host.start("Main", &[]).expect("start");
    run(&mut host, 1);
    let u = unit_key(&host, "gU");
    let mut positions = Vec::new();
    for _ in 0..10 {
        host.step();
        let g = host.game();
        let obj = g.unit(u);
        positions.push((obj.x, obj.y));
        if g.unit_order_ids(u).is_empty() {
            break;
        }
    }
    // The mover is the original's walk (movement-6.md; kernels CONFIRMED in Unicorn): the unit accelerates by its
    // speed cap (270 / 32 cells per second) on the first walk it faces the goal within the window, and the position is
    // evaluated from the velocity at the game time of each read (batches of 20 ticks = 0.1 s). A move to the east
    // drifts a little in y: the heading of the velocity comes from the softfloat arccosine near 1 (3.4e-4 rad),
    // and the steering turns it back. The unit stops inside its arrival radius (0.49 cells = 15.68 world units) once
    // its heading is aligned, so the order completes on the third recorded step at x = 89.1.
    let golden: [(u32, u32); 3] = [
        (0x4237_9857, 0xBC55_F40A),
        (0x4291_CBD1, 0xBB96_13C0),
        (0x42B2_3215, 0x3B5C_53C0),
    ];
    let bits: Vec<(u32, u32)> = positions
        .iter()
        .map(|(x, y)| (x.to_bits(), y.to_bits()))
        .collect();
    assert_eq!(bits, golden.to_vec(), "positions per step");
    assert!(
        host.game().unit_order_ids(u).is_empty(),
        "the move completed"
    );
    // Facing: the heading of the step vector (acos of the normalised x). The value is the model's own
    // (softfloat division and arccosine near 1), so it is pinned by its bits.
    let facing = host.game().unit(u).facing;
    eprintln!("facing bits {:#010x}", facing.to_bits());
    assert_eq!(facing.to_bits(), FACING_BITS, "facing of the step vector");
}

/// The position of a unit in motion is evaluated from its velocity at the game time (movement-6.md), so it
/// advances on every tick, not only at the 0.03 s walks, and the speed is the cap (270 / 32 cells per second): over
/// one second of ticks the unit covers about 270 world units.
#[test]
fn position_is_evaluated_between_walks() {
    if tables().is_none() {
        eprintln!("GAME_DLL not set: skipping the movement test");
        return;
    }
    let mut host = host_for(&footman_script(
        0,
        0,
        r#"set gOk = IssuePointOrder(gU, "move", I2R(2000), I2R(0))"#,
    ));
    host.start("Main", &[]).expect("start");
    let u = unit_key(&host, "gU");
    // two batches: the first walks have run and the unit is at speed
    host.step_ticks(40);
    let mut xs = Vec::new();
    for _ in 0..200 {
        host.step_ticks(1);
        xs.push(host.game().unit(u).x.to_f32());
    }
    // strictly increasing on every tick (a stepped model would repeat values between walks)
    assert!(xs.windows(2).all(|w| w[1] > w[0]), "x advances on every tick");
    let per_second = xs[199] - xs[0];
    // 199 ticks of 5 ms = 0.995 s at 270 per second, up to the rounding of the softfloat clock
    assert!((per_second - 268.65).abs() < 1.0, "speed {per_second}");
}

/// A route around a wall, through the pathing search. The start and goal are at cell `(1, 1)` and `(6, 1)`; a
/// blocked column at `x = 3` leaves the gap at row 7, so the unit must pass below y = 224. Needs `GAME_DLL`.
#[test]
fn route_goes_around_a_wall() {
    if tables().is_none() {
        eprintln!("GAME_DLL not set: skipping the route test");
        return;
    }
    let mut host = host_for(&footman_script(
        48,
        48,
        r#"set gOk = IssuePointOrder(gU, "move", I2R(208), I2R(48))"#,
    ));
    // 8 by 8 cells: a wall at column 3 except row 7. Cell state 1 is blocked, 0 open (the search's fixture).
    let mut cells = vec![0u32; 64];
    for y in 0..7 {
        cells[y * 8 + 3] = 1 << 30;
    }
    let base: Vec<u32> = cells.clone();
    let coarse = |n: usize| vec![3u32 << 30; n];
    let pyramid = Pyramid::new([
        Level::new(8, 8, &base),
        Level::new(4, 4, &coarse(16)),
        Level::new(2, 2, &coarse(4)),
        Level::new(1, 1, &coarse(1)),
    ]);
    let search = Search::new(pyramid, vec![FlagLink::default(); 256], 8, 0);
    host.game_mut().set_pathing(Some(PathingMap {
        search,
        origin: (F32::ZERO, F32::ZERO),
    }));
    host.start("Main", &[]).expect("start");
    run(&mut host, 1);
    let u = unit_key(&host, "gU");
    let mut max_y = 0.0f32;
    for _ in 0..400 {
        host.step();
        max_y = max_y.max(host.game().unit(u).y.to_f32());
        if host.game().unit_order_ids(u).is_empty() {
            break;
        }
    }
    assert!(
        host.game().unit_order_ids(u).is_empty(),
        "the move completed"
    );
    let end = host.game().unit(u).clone();
    // The mover stops inside its arrival radius: 0.49 cells = 15.68 world units (movement-6.md; the old model used
    // 6.4).
    assert!(
        (end.x.to_f32() - 208.0).abs() <= 15.68,
        "x = {}",
        end.x.to_f32()
    );
    assert!(
        (end.y.to_f32() - 48.0).abs() <= 15.68,
        "y = {}",
        end.y.to_f32()
    );
    assert!(
        max_y >= 224.0,
        "the route passes the gap at row 7 (max y {max_y})"
    );
}

#[test]
fn unit_without_tables_keeps_its_route_and_does_not_move() {
    // Without the softfloat tables nothing moves: the order stays current at the start position.
    if tables().is_some() {
        return;
    }
    let mut host = host_for(&footman_script(
        0,
        0,
        r#"set gOk = IssuePointOrder(gU, "move", I2R(100), I2R(0))"#,
    ));
    host.start("Main", &[]).expect("start");
    run(&mut host, 5);
    let u = unit_key(&host, "gU");
    assert_eq!(host.game().unit(u).x, F32::ZERO);
    assert_eq!(host.game().unit_order_ids(u), vec![MOVE as u32]);
}
