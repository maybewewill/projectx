//! Unit group natives through `ScriptHost` (`natives/group.rs`, spec `docs/spec/groups-events.md` 1 to 4).
//!
//! Positions are integer-valued (`I2R`). The callbacks encode the visited units as digits of an integer
//! (`acc = acc * 10 + x`), so an expected value reads as the visit order, and a separate counter gives the
//! number of visits. Where the original's behaviour is only LIKELY or UNVERIFIED, the test says so.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{UnitType, UnitTypes};
use sim::{ScriptHost, HANDLE_BASE};
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type group extends handle
type boolexpr extends handle
type filterfunc extends boolexpr
type location extends handle
type rect extends handle
type unitevent extends handle

native I2R takes integer i returns real
native R2I takes real r returns integer
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native KillUnit takes unit whichUnit returns nothing
native RemoveUnit takes unit whichUnit returns nothing
native GetUnitX takes unit whichUnit returns real
native GetUnitTypeId takes unit whichUnit returns integer
native CreateGroup takes nothing returns group
native DestroyGroup takes group g returns nothing
native GroupAddUnit takes group g, unit whichUnit returns nothing
native GroupRemoveUnit takes group g, unit whichUnit returns nothing
native GroupClear takes group g returns nothing
native IsUnitInGroup takes unit whichUnit, group g returns boolean
native FirstOfGroup takes group g returns unit
native ForGroup takes group g, code callback returns nothing
native GroupEnumUnitsOfType takes group g, string unitname, boolexpr filter returns nothing
native GroupEnumUnitsOfTypeCounted takes group g, string unitname, boolexpr filter, integer countLimit returns nothing
native GroupEnumUnitsOfPlayer takes group g, player whichPlayer, boolexpr filter returns nothing
native GroupEnumUnitsInRange takes group g, real x, real y, real radius, boolexpr filter returns nothing
native GroupEnumUnitsInRangeCounted takes group g, real x, real y, real radius, boolexpr filter, integer countLimit returns nothing
native GroupEnumUnitsInRect takes group g, rect r, boolexpr filter returns nothing
native GetEnumUnit takes nothing returns unit
native GetFilterUnit takes nothing returns unit
native Filter takes code c returns filterfunc
native Rect takes real minx, real miny, real maxx, real maxy returns rect
"#;

/// Type ids: raw four-character codes as integers.
const FOOTMAN: i32 = 0x6866_6F6F; // 'hfoo'
const MAGE: i32 = 0x686D_6167; // 'hmag'

/// A host with the footman and mage types registered (names and life are all these tests need).
fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
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

/// Runs the host for a few steps: the main thread and any chain it started.
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

/// Globals and callbacks shared by the scripts. `u1`, `u2`, `u3` are footmen at x = 1, 2, 3 (player 0).
const COMMON: &str = r#"
globals
integer acc = 0
integer cnt = 0
integer calls = 0
integer first = 0
integer flag = 0
unit u1
unit u2
unit u3
unit u4
group g
group h
endglobals

function rec takes nothing returns nothing
    set acc = acc * 10 + R2I(GetUnitX(GetEnumUnit()))
    set cnt = cnt + 1
endfunction

function filt_rec takes nothing returns boolean
    set calls = calls * 10 + R2I(GetUnitX(GetFilterUnit()))
    return true
endfunction

function make_units takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(1), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(2), I2R(0), I2R(0))
    set u3 = CreateUnit(Player(0), 'hfoo', I2R(3), I2R(0), I2R(0))
endfunction
"#;

fn script(body: &str) -> String {
    format!("{COMMON}\n{body}")
}

#[test]
fn group_keeps_insertion_order_and_ignores_duplicates() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u3)
    call GroupAddUnit(g, u1)
    call GroupAddUnit(g, u2)
    call GroupAddUnit(g, u1)
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(
        int(&host, "acc"),
        312,
        "insertion order, the duplicate is not added"
    );
    assert_eq!(int(&host, "cnt"), 3);
}

#[test]
fn for_group_visits_dead_units_and_skips_removed_ones() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call GroupAddUnit(g, u2)
    call GroupAddUnit(g, u3)
    call KillUnit(u2)
    call RemoveUnit(u3)
    call ForGroup(g, function rec)
    set flag = 0
    if IsUnitInGroup(u3, g) then
        set flag = 1
    endif
    set first = GetHandleId(FirstOfGroup(g))
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // u2 is dead but still visited; u3 is removed and skipped.
    assert_eq!(int(&host, "acc"), 12);
    assert_eq!(int(&host, "cnt"), 2);
    // A removed unit is rejected by IsUnitInGroup (the resolver).
    assert_eq!(int(&host, "flag"), 0);
    // The head is u1, which is alive, so its id comes back.
    assert_eq!(int(&host, "first"), handle(&host, "u1") as i32);
}

#[test]
fn first_of_group_returns_a_removed_head_ghost() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call GroupAddUnit(g, u2)
    call RemoveUnit(u1)
    set first = GetHandleId(FirstOfGroup(g))
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // FirstOfGroup does not check the removed flag (spec 2.7), so the removed head still answers. The
    // conversion makes a fresh id for the removed object (LIKELY), so the id is not u1's old one.
    let first = int(&host, "first") as u32;
    assert!(first >= HANDLE_BASE, "the ghost head gives a handle");
    assert_ne!(
        first,
        handle(&host, "u1"),
        "a fresh id for the removed object (LIKELY)"
    );
}

#[test]
fn enumeration_clears_the_group_first_then_filters_in_candidate_order() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set u4 = CreateUnit(Player(1), 'hfoo', I2R(4), I2R(0), I2R(0))
    set g = CreateGroup()
    call GroupAddUnit(g, u4)
    call GroupEnumUnitsOfPlayer(g, Player(0), Filter(function filt_rec))
    call ForGroup(g, function rec)
    set flag = 0
    if IsUnitInGroup(u4, g) then
        set flag = 1
    endif
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // u4 (owner 1) was in the group; the enumeration cleared it. The owner test runs before the filter, so
    // the filter sees only u1, u2, u3 in creation order.
    assert_eq!(int(&host, "calls"), 123);
    assert_eq!(int(&host, "acc"), 123);
    assert_eq!(int(&host, "cnt"), 3);
    assert_eq!(int(&host, "flag"), 0);
}

#[test]
fn type_enumeration_uses_the_registered_name_and_ignores_unknown_names() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u3)
    call GroupEnumUnitsOfType(g, "Mage", null)
    call ForGroup(g, function rec)
    set h = CreateGroup()
    call GroupAddUnit(h, u2)
    call GroupEnumUnitsOfType(h, "NoSuchUnit", null)
    set acc = 0
    set cnt = 0
    call ForGroup(h, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // No mage exists, so the group is cleared and stays empty (the last loop over g is not checked here,
    // acc was reset before the second group).
    // An unknown name does not clear the group, so u2 is still there.
    assert_eq!(int(&host, "acc"), 2);
    assert_eq!(int(&host, "cnt"), 1);
}

#[test]
fn type_enumeration_clears_and_collects_matching_units() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call GroupEnumUnitsOfType(g, "Footman", null)
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "acc"), 123);
    assert_eq!(int(&host, "cnt"), 3);
}

#[test]
fn counted_enumeration_keeps_inserting_past_the_limit() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupEnumUnitsInRangeCounted(g, I2R(0), I2R(0), I2R(10), null, 1)
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // Spec 3.3: the counter goes up only when an insert fails, and a fresh unit never fails. So the
    // limit of 1 never stops the query and all three units are inserted (LIKELY, to be checked).
    assert_eq!(int(&host, "cnt"), 3);
    assert_eq!(int(&host, "acc"), 123);
}

#[test]
fn counted_enumeration_with_a_negative_limit_is_unlimited() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupEnumUnitsInRangeCounted(g, I2R(0), I2R(0), I2R(10), null, -1)
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // A negative limit is a huge unsigned value, so it never stops the query (spec 3.4).
    assert_eq!(int(&host, "cnt"), 3);
}

#[test]
fn range_enumeration_applies_the_area_test_before_the_filter() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupEnumUnitsInRange(g, I2R(0), I2R(0), I2R(2), Filter(function filt_rec))
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // UNVERIFIED formula (squared distance, inclusive): u1 (d = 1) and u2 (d = 2) are inside, u3 is not.
    // The filter only sees the candidates that pass the area test.
    assert_eq!(int(&host, "calls"), 12);
    assert_eq!(int(&host, "acc"), 12);
}

#[test]
fn rect_enumeration_uses_inclusive_bounds() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupEnumUnitsInRect(g, Rect(I2R(2), I2R(-1), I2R(5), I2R(1)), null)
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // UNVERIFIED (spec 3.7): the edges are inclusive here. u2 (x = 2) and u3 (x = 3) are inside.
    assert_eq!(int(&host, "acc"), 23);
    assert_eq!(int(&host, "cnt"), 2);
}

#[test]
fn ghost_entries_are_visible_until_the_next_enumeration() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call GroupAddUnit(g, u2)
    call RemoveUnit(u1)
    call ForGroup(g, function rec)
    set flag = 0
    if IsUnitInGroup(u1, g) then
        set flag = 1
    endif
    set first = GetHandleId(FirstOfGroup(g))
    call GroupEnumUnitsOfType(g, "Footman", null)
    set acc = 0
    set cnt = 0
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // The ghost u1 is still the head (FirstOfGroup answers with a handle), and the loop skips it.
    assert!(int(&host, "first") >= HANDLE_BASE as i32);
    assert_eq!(int(&host, "flag"), 0);
    // The enumeration cleared the ghost. The footmen u2 and u3 are collected, u1 (removed) is not.
    assert_eq!(int(&host, "acc"), 23);
    assert_eq!(int(&host, "cnt"), 2);
}

#[test]
fn removed_units_are_not_candidates() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    call RemoveUnit(u2)
    set g = CreateGroup()
    call GroupEnumUnitsInRange(g, I2R(0), I2R(0), I2R(10), Filter(function filt_rec))
    call ForGroup(g, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    // A removed unit is skipped at the candidate level, before the filter.
    assert_eq!(int(&host, "calls"), 13);
    assert_eq!(int(&host, "acc"), 13);
}

#[test]
fn removed_unit_keeps_its_id_while_a_group_holds_it() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call RemoveUnit(u1)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    let id = handle(&host, "u1");
    let gm = host.game();
    // The creation reference went with RemoveUnit; the group's reference is left, so the id stays reserved
    // and the object is gone (LIKELY, see natives/group.rs).
    assert_eq!(gm.handles().refcount(id), Some(1));
    assert!(gm.handles().get(id).is_none());
    assert!(!gm.handles().free_ids().contains(&id));
}

#[test]
fn clearing_a_group_releases_its_references_in_list_order() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call GroupAddUnit(g, u2)
    call RemoveUnit(u1)
    call RemoveUnit(u2)
    call GroupClear(g)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    let id1 = handle(&host, "u1");
    let id2 = handle(&host, "u2");
    let gm = host.game();
    // The head (u1) is released first, then u2. The free stack is last-in first-out, so u2's id is the
    // next one handed out.
    assert_eq!(gm.handles().free_ids(), &[id1, id2]);
    assert_eq!(gm.handles().refcount(id1), Some(0));
}

#[test]
fn group_removal_releases_the_reference() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call GroupRemoveUnit(g, u1)
    set flag = 0
    if IsUnitInGroup(u1, g) then
        set flag = 1
    endif
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "flag"), 0);
    let id = handle(&host, "u1");
    // Alive unit: the creation reference is still there, the group's was released.
    assert_eq!(host.game().handles().refcount(id), Some(1));
}

#[test]
fn destroy_group_releases_members() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    call make_units()
    set g = CreateGroup()
    call GroupAddUnit(g, u1)
    call RemoveUnit(u1)
    call DestroyGroup(g)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    let id = handle(&host, "u1");
    // LIKELY: the group's destruction releases its members, so the removed unit's id is now free.
    assert!(host.game().handles().free_ids().contains(&id));
}
