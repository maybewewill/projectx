//! Regions through `ScriptHost` (`natives/region.rs`, the notes there list the CONFIRMED and LIKELY points).
//!
//! The actions record digits in `acc` (`acc = acc * 10 + k`), so an expected value reads as the firing
//! order, as in `unit_event_natives.rs`. Boolean answers are packed into `bits` (one bit per `bit(...)`
//! call, first answer most significant), so an expected value reads as the sequence of answers.
//!
//! The playable area is 0 to 2048 on both axes, so no rect corner is clamped by it except the ones a test
//! sets on purpose (the `Rect` clamp keeps the corners at most 32 below the playable maximum).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{Playable, UnitType, UnitTypes};
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
type triggercondition extends handle
type region extends handle
type rect extends handle

native I2R takes integer i returns real
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitTypeId takes unit whichUnit returns integer
native SetUnitX takes unit whichUnit, real newX returns nothing
native SetUnitY takes unit whichUnit, real newY returns nothing
native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing
native CreateTrigger takes nothing returns trigger
native DestroyTrigger takes trigger t returns nothing
native TriggerAddAction takes trigger t, code c returns triggeraction
native Filter takes code c returns filterfunc
native GetFilterUnit takes nothing returns unit
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native CreateRegion takes nothing returns region
native RemoveRegion takes region whichRegion returns nothing
native RegionAddRect takes region whichRegion, rect r returns nothing
native RegionClearRect takes region whichRegion, rect r returns nothing
native RegionAddCell takes region whichRegion, real x, real y returns nothing
native RegionClearCell takes region whichRegion, real x, real y returns nothing
native IsUnitInRegion takes region whichRegion, unit whichUnit returns boolean
native IsPointInRegion takes region whichRegion, real x, real y returns boolean
native TriggerRegisterEnterRegion takes trigger t, region whichRegion, boolexpr filter returns event
native TriggerRegisterLeaveRegion takes trigger t, region whichRegion, boolexpr filter returns event
native GetEnteringUnit takes nothing returns unit
native GetLeavingUnit takes nothing returns unit
native GetTriggeringRegion takes nothing returns region
"#;

/// Unit type ids: raw four-character codes as integers.
const FOOTMAN: i32 = 0x6866_6F6F; // 'hfoo'
const MAGE: i32 = 0x686D_6167; // 'hmag'

const COMMON: &str = r#"
globals
integer acc = 0
integer bits = 0
integer entering = 0
integer leaving = 0
integer tregion = 0
integer lv_in_enter = 0
integer calls = 0
integer rid = 0
unit u1
unit u2
region r1
region r2
region r3
trigger t1
trigger t2
trigger t3
endglobals

function bit takes boolean b returns nothing
    if b then
        set bits = bits * 2 + 1
    else
        set bits = bits * 2
    endif
endfunction

function a1 takes nothing returns nothing
    set acc = acc * 10 + 1
    set entering = GetHandleId(GetEnteringUnit())
    set lv_in_enter = GetHandleId(GetLeavingUnit())
    set tregion = GetHandleId(GetTriggeringRegion())
endfunction

function a2 takes nothing returns nothing
    set acc = acc * 10 + 2
    set leaving = GetHandleId(GetLeavingUnit())
    set tregion = GetHandleId(GetTriggeringRegion())
endfunction

function a3 takes nothing returns nothing
    set acc = acc * 10 + 3
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
        g.set_playable(Some(Playable {
            min_x: F32::ZERO,
            min_y: F32::ZERO,
            max_x: F32::from_bits(0x4500_0000), // 2048.0
            max_y: F32::from_bits(0x4500_0000),
        }));
        let types: &mut UnitTypes = g.unit_types_mut();
        let ty = |name: &str| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x43D2_0000),
            max_mana: F32::ZERO,
            start_mana: F32::ZERO,
            inventory_size: 0,
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

#[test]
fn rect_edges_map_to_cells_with_the_max_cell_included() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set r1 = CreateRegion()
    call RegionAddRect(r1, Rect(I2R(32), I2R(32), I2R(64), I2R(64)))
    call bit(IsPointInRegion(r1, 32.0, 40.0))
    call bit(IsPointInRegion(r1, 31.999998, 40.0))
    call bit(IsPointInRegion(r1, 95.0, 95.0))
    call bit(IsPointInRegion(r1, 96.0, 40.0))
    call bit(IsPointInRegion(r1, 64.0, 40.0))
    call bit(IsPointInRegion(r1, 40.0, 31.0))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Cells 1 and 2 on both axes: 32.0 starts cell 1, 31.999998 is cell 0, 96.0 is cell 3, and the
    // max edge at 64.0 includes cell 2 (the cell that starts at 64).
    assert_eq!(int(&host, "bits"), 0b10_1010);
}

#[test]
fn clearing_a_rect_removes_only_its_cells() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set r1 = CreateRegion()
    call RegionAddRect(r1, Rect(I2R(0), I2R(0), I2R(64), I2R(64)))
    call RegionClearRect(r1, Rect(I2R(0), I2R(0), I2R(32), I2R(32)))
    call bit(IsPointInRegion(r1, 10.0, 10.0))
    call bit(IsPointInRegion(r1, 70.0, 10.0))
    call RegionAddCell(r1, 10.0, 10.0)
    call bit(IsPointInRegion(r1, 10.0, 10.0))
    call RegionClearCell(r1, 10.0, 10.0)
    call bit(IsPointInRegion(r1, 10.0, 10.0))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // The clear box of Rect(0, 0, 32, 32) is cells 0 and 1 (its max edge cell is included), so cell 2 stays.
    assert_eq!(int(&host, "bits"), 0b0110);
}

#[test]
fn points_below_the_playable_origin_are_outside_and_the_rect_clamps() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set r1 = CreateRegion()
    // The corner -100 is clamped to the playable minimum 0, so the rect covers cells 0 and 1.
    call RegionAddRect(r1, Rect(I2R(-100), I2R(-100), I2R(40), I2R(40)))
    call bit(IsPointInRegion(r1, 0.0, 0.0))
    call bit(IsPointInRegion(r1, 63.0, 63.0))
    call bit(IsPointInRegion(r1, 64.0, 0.0))
    call bit(IsPointInRegion(r1, -1.0, 0.0))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Cells 0 and 1 are inside; cell 2 (x = 64) and cell -1 (x = -1) are outside.
    assert_eq!(int(&host, "bits"), 0b1100);
}

#[test]
fn unit_membership_follows_its_position() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(50), I2R(50), I2R(0))
    set r1 = CreateRegion()
    call RegionAddRect(r1, Rect(I2R(32), I2R(32), I2R(64), I2R(64)))
    call bit(IsUnitInRegion(r1, u1))
    call SetUnitPosition(u1, I2R(200), I2R(200))
    call bit(IsUnitInRegion(r1, u1))
    call SetUnitX(u1, I2R(40))
    call bit(IsUnitInRegion(r1, u1))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Inside, then outside after SetUnitPosition, then (40, 200) is still outside.
    assert_eq!(int(&host, "bits"), 0b100);
}

#[test]
fn enter_and_leave_fire_on_set_unit_position_with_the_event_getters() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(10), I2R(10), I2R(0))
    set r1 = CreateRegion()
    call RegionAddCell(r1, 40.0, 40.0)
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterEnterRegion(t1, r1, null)
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function a2)
    call TriggerRegisterLeaveRegion(t2, r1, null)
    call SetUnitPosition(u1, I2R(40), I2R(40))
    call SetUnitPosition(u1, 41.0, 41.0)
    call SetUnitPosition(u1, I2R(100), I2R(100))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Enter (1), then the move inside cell (1, 1) fires nothing, then leave (2).
    assert_eq!(int(&host, "acc"), 12);
    let u1 = handle(&host, "u1");
    let r1 = handle(&host, "r1");
    assert_eq!(int(&host, "entering"), u1 as i32);
    // In the enter action GetLeavingUnit is null; in the leave action it is the unit.
    assert_eq!(int(&host, "lv_in_enter"), 0);
    assert_eq!(int(&host, "leaving"), u1 as i32);
    assert_eq!(int(&host, "tregion"), r1 as i32);
}

#[test]
fn moving_inside_the_region_fires_nothing() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(10), I2R(10), I2R(0))
    set r1 = CreateRegion()
    call RegionAddRect(r1, Rect(I2R(32), I2R(32), I2R(64), I2R(64)))
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterEnterRegion(t1, r1, null)
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function a2)
    call TriggerRegisterLeaveRegion(t2, r1, null)
    call SetUnitPosition(u1, I2R(40), I2R(40))
    call SetUnitPosition(u1, I2R(90), I2R(90))
    call SetUnitPosition(u1, I2R(100), I2R(100))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Enter at (40, 40), no event for the move to (90, 90) (still cells 1 and 2 inside), leave at (100, 100).
    assert_eq!(int(&host, "acc"), 12);
}

#[test]
fn set_unit_x_and_y_do_not_fire_region_events() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(10), I2R(10), I2R(0))
    set r1 = CreateRegion()
    call RegionAddCell(r1, 40.0, 40.0)
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterEnterRegion(t1, r1, null)
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function a2)
    call TriggerRegisterLeaveRegion(t2, r1, null)
    // SetUnitX and SetUnitY move the unit into the cell silently.
    call SetUnitX(u1, I2R(40))
    call SetUnitY(u1, I2R(40))
    call bit(IsUnitInRegion(r1, u1))
    // The next SetUnitPosition sees the unit leave the region (old cell inside, new cell outside).
    call SetUnitPosition(u1, I2R(10), I2R(10))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "bits"), 1);
    assert_eq!(int(&host, "acc"), 2);
}

#[test]
fn several_triggers_fire_oldest_first_and_re_registration_keeps_the_place() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(10), I2R(10), I2R(0))
    set r1 = CreateRegion()
    call RegionAddCell(r1, 40.0, 40.0)
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterEnterRegion(t1, r1, null)
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function a3)
    call TriggerRegisterEnterRegion(t2, r1, null)
    // Registering t1 again on the same region keeps its place and does not fire it twice.
    call TriggerRegisterEnterRegion(t1, r1, null)
    call SetUnitPosition(u1, I2R(40), I2R(40))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 13);
}

#[test]
fn filter_runs_at_detection_with_the_unit_and_gates_the_trigger() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(10), I2R(10), I2R(0))
    set u2 = CreateUnit(Player(0), 'hmag', I2R(10), I2R(10), I2R(0))
    set r1 = CreateRegion()
    call RegionAddCell(r1, 40.0, 40.0)
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterEnterRegion(t1, r1, Filter(function is_foot))
    call SetUnitPosition(u2, I2R(40), I2R(40))
    call SetUnitPosition(u1, I2R(40), I2R(40))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    // The mage fails the filter (no action), the footman passes it. The filter ran once per crossing.
    assert_eq!(int(&host, "acc"), 1);
    assert_eq!(int(&host, "calls"), 2);
    assert_eq!(int(&host, "entering"), handle(&host, "u1") as i32);
}

#[test]
fn a_destroyed_trigger_or_a_removed_region_never_fires() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(10), I2R(10), I2R(0))
    set r1 = CreateRegion()
    call RegionAddCell(r1, 40.0, 40.0)
    set t1 = CreateTrigger()
    call TriggerAddAction(t1, function a1)
    call TriggerRegisterEnterRegion(t1, r1, null)
    call DestroyTrigger(t1)
    set r2 = CreateRegion()
    call RegionAddCell(r2, 40.0, 40.0)
    set t2 = CreateTrigger()
    call TriggerAddAction(t2, function a3)
    call TriggerRegisterEnterRegion(t2, r2, null)
    call RemoveRegion(r2)
    call SetUnitPosition(u1, I2R(40), I2R(40))
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 0);
}

#[test]
fn region_handle_ids_are_reused_last_freed_first_and_stale_handles_are_outside() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set r1 = CreateRegion()
    call RegionAddCell(r1, 40.0, 40.0)
    set rid = GetHandleId(r1)
    call RemoveRegion(r1)
    // The stale handle no longer resolves: the cell is not inside.
    call bit(IsPointInRegion(r1, 40.0, 40.0))
    set r2 = CreateRegion()
    set tregion = GetHandleId(r2)
endfunction
"#,
    );
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_ne!(int(&host, "rid"), 0);
    assert_eq!(int(&host, "bits"), 0);
    // The creation reference of the removed region was the last one, so its id is reused.
    assert_eq!(int(&host, "tregion"), int(&host, "rid"));
}
