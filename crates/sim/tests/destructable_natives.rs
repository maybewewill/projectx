//! Destructable natives through `ScriptHost` (`natives/destructable.rs`, the model in `natives/game.rs`).
//!
//! The scripts use integer-valued reals (`I2R`) and the one negative value, so they need no lookup tables for
//! literals. Real comparisons in the scripts are `>=`, which needs no tables either. The checks are on handle ids,
//! lives, the order of the enumeration and the number of death callbacks.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{DestructableType, UnitType};
use sim::{RngTable, ScriptHost, HANDLE_BASE};
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type destructable extends widget
type item extends handle
type rect extends handle
type boolexpr extends handle
type filterfunc extends boolexpr
type trigger extends handle
type triggeraction extends handle
type event extends handle

native I2R takes integer i returns real
native R2I takes real r returns integer
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native Filter takes code c returns filterfunc
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native KillUnit takes unit whichUnit returns nothing
native CreateItem takes integer itemid, real x, real y returns item
native RemoveItem takes item whichItem returns nothing
native CreateDestructable takes integer objectid, real x, real y, real face, real scale, integer variation returns destructable
native CreateDestructableZ takes integer objectid, real x, real y, real z, real face, real scale, integer variation returns destructable
native CreateDeadDestructable takes integer objectid, real x, real y, real face, real scale, integer variation returns destructable
native RemoveDestructable takes destructable d returns nothing
native KillDestructable takes destructable d returns nothing
native SetDestructableLife takes destructable d, real life returns nothing
native GetDestructableLife takes destructable d returns real
native SetDestructableMaxLife takes destructable d, real max returns nothing
native GetDestructableMaxLife takes destructable d returns real
native DestructableRestoreLife takes destructable d, real life, boolean birthAnim returns nothing
native GetDestructableX takes destructable d returns real
native GetDestructableY takes destructable d returns real
native GetDestructableTypeId takes destructable d returns integer
native SetDestructableInvulnerable takes destructable d, boolean flag returns nothing
native IsDestructableInvulnerable takes destructable d returns boolean
native ShowDestructable takes destructable d, boolean flag returns nothing
native SetDestructableOccluderHeight takes destructable d, real height returns nothing
native GetDestructableOccluderHeight takes destructable d returns real
native EnumDestructablesInRect takes rect r, boolexpr filter, code actionFunc returns nothing
native GetEnumDestructable takes nothing returns destructable
native GetFilterDestructable takes nothing returns destructable
native GetTriggerDestructable takes nothing returns destructable
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerRegisterDeathEvent takes trigger t, widget w returns event
"#;

/// Type ids: `'ATtr'` has three variations and 300 life; `'LTlt'` has one variation and 50 life.
const TREE: u32 = 0x4154_7472;
const ROCK: u32 = 0x4C54_6C74;
const FOOTMAN: u32 = 0x6866_6F6F;
/// `'ratc'`, a plain item type (for the id interleaving).
const PLAIN: u32 = 0x7261_7463;

/// The permutation table of the user's `Game.dll` (`GAME_DLL`), or an identity stand-in.
fn rng_table() -> RngTable {
    if let Ok(path) = std::env::var("GAME_DLL") {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
        return RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL");
    }
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    RngTable::from_bytes(bytes).expect("identity is a permutation")
}

fn ut(name: &str) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: F32::from_bits(0x43D2_0000),
        max_mana: F32::ZERO,
        start_mana: F32::ZERO,
        inventory_size: 0,
        ..Default::default()
    }
}

fn dtype(max_life: f32, variations: u32) -> DestructableType {
    DestructableType {
        max_life: F32::from_bits(max_life.to_bits()),
        variations,
    }
}

/// A host with the unit, item and destructable types. The item type is `'ratc'`.
fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    host.set_rng_table(rng_table());
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(FOOTMAN, ut("Footman"));
        g.destructable_types_mut().insert(TREE, dtype(300.0, 3));
        g.destructable_types_mut().insert(ROCK, dtype(50.0, 1));
        let item = sim::natives::game::ItemType {
            class: 1,
            level_ex: 1,
            level_plain: 1,
            flag_48: 1,
            charges: 0,
            flags: 0,
            powerup: false,
            sellable: false,
            pawnable: false,
        };
        g.item_types_mut().insert(PLAIN, item);
    }
    host
}

/// Runs the host for a few steps: the main thread and the chains it started.
fn run(host: &mut ScriptHost) {
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

/// Globals shared by the scripts. Destructables are `destructable` globals, units are `unit` globals.
const COMMON: &str = r#"
globals
integer l0 = -1
integer l1 = -1
integer l2 = -1
integer l3 = -1
integer l4 = -1
integer l5 = -1
integer l6 = -1
integer m0 = -1
integer m1 = -1
integer oh = -1
integer idu = -1
integer idd1 = -1
integer idit = -1
integer idd2 = -1
integer idbad = -1
integer idd3 = -1
integer idd4 = -1
integer tyok = -1
integer dcnt = 0
integer dseen = -1
integer ucnt = 0
integer uwid = -1
integer fseq = 0
integer eseq = 0
integer ecnt = 0
integer same = 0
integer lv = -1
integer lk = -1
integer ivs = -1
integer ivk = -1
integer dl = -1
integer dm = -1
integer rl = -1
integer rx = -1
integer idr1 = -1
integer idr2 = -1
integer dx2 = -1
boolean iv1 = false
boolean iv2 = false
boolean ivr = true
destructable d1
destructable d2
destructable d3
destructable d4
destructable d5
destructable d6
destructable bad
unit u1
item it1
trigger t
trigger t2
event ev
event ev2
triggeraction ta
triggeraction ta2
rect r
filterfunc flt
endglobals

function on_death takes nothing returns nothing
    set dcnt = dcnt + 1
    set dseen = GetHandleId(GetTriggerDestructable())
endfunction

function on_unit_death takes nothing returns nothing
    set ucnt = ucnt + 1
    set uwid = GetHandleId(GetTriggerDestructable())
endfunction

function ef_filter takes nothing returns boolean
    set fseq = fseq * 10 + R2I(GetDestructableX(GetFilterDestructable()))
    return GetDestructableX(GetFilterDestructable()) >= I2R(2)
endfunction

function ef_act takes nothing returns nothing
    set eseq = eseq * 10 + R2I(GetDestructableX(GetEnumDestructable()))
    set ecnt = ecnt + 1
    if GetFilterDestructable() == GetEnumDestructable() then
        set same = same + 1
    endif
endfunction
"#;

fn script(body: &str) -> String {
    format!("{COMMON}\n{body}")
}

/// Handle ids: a destructable takes the next id like a unit or an item, a failed creation takes none, and a
/// removed destructable's id is reused by the next creation.
#[test]
fn destructable_ids_interleave_with_units_and_items() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set d1 = CreateDestructable('ATtr', I2R(1), I2R(1), I2R(0), I2R(1), 0)
    set it1 = CreateItem('ratc', I2R(1), I2R(1))
    set d2 = CreateDestructable('ATtr', I2R(2), I2R(2), I2R(0), I2R(1), 1)
    set bad = CreateDestructable('zzzz', I2R(0), I2R(0), I2R(0), I2R(1), 0)
    set d3 = CreateDestructable('ATtr', I2R(3), I2R(3), I2R(0), I2R(1), 9)
    set idu = GetHandleId(u1)
    set idd1 = GetHandleId(d1)
    set idit = GetHandleId(it1)
    set idd2 = GetHandleId(d2)
    set idbad = GetHandleId(bad)
    set idd3 = GetHandleId(d3)
    set tyok = GetDestructableTypeId(d3)
    call RemoveDestructable(d1)
    set d4 = CreateDestructable('ATtr', I2R(4), I2R(4), I2R(0), I2R(1), 0)
    set idd4 = GetHandleId(d4)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    let idu = int(&host, "idu") as u32;
    assert!(idu >= HANDLE_BASE);
    assert_eq!(int(&host, "idd1"), int(&host, "idu") + 1);
    assert_eq!(int(&host, "idit"), int(&host, "idd1") + 1);
    assert_eq!(int(&host, "idd2"), int(&host, "idit") + 1);
    assert_eq!(
        int(&host, "idbad"),
        0,
        "an unknown type gives the null handle"
    );
    assert_eq!(
        int(&host, "idd3"),
        int(&host, "idd2") + 1,
        "the failed creation consumed no id"
    );
    assert_eq!(
        int(&host, "tyok") as u32,
        TREE,
        "the type id is the object id"
    );
    assert_eq!(
        int(&host, "idd4"),
        int(&host, "idd1"),
        "the removed destructable's id is reused"
    );
}

/// Life rules: the initial life is the type's max life, a value at or below zero stores zero, a dead
/// destructable is restored only by `DestructableRestoreLife`, and a restore with life zero uses the max life.
#[test]
fn life_rules_and_restore() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set d1 = CreateDestructable('ATtr', I2R(0), I2R(0), I2R(0), I2R(1), 0)
    set l0 = R2I(GetDestructableLife(d1))
    set m0 = R2I(GetDestructableMaxLife(d1))
    call SetDestructableLife(d1, I2R(120))
    set l1 = R2I(GetDestructableLife(d1))
    call SetDestructableLife(d1, I2R(-5))
    set l2 = R2I(GetDestructableLife(d1))
    call DestructableRestoreLife(d1, I2R(40), false)
    set l3 = R2I(GetDestructableLife(d1))
    call DestructableRestoreLife(d1, I2R(10), false)
    set l4 = R2I(GetDestructableLife(d1))
    call KillDestructable(d1)
    set l5 = R2I(GetDestructableLife(d1))
    call DestructableRestoreLife(d1, I2R(0), true)
    set l6 = R2I(GetDestructableLife(d1))
    call SetDestructableMaxLife(d1, I2R(500))
    set m1 = R2I(GetDestructableMaxLife(d1))
    call SetDestructableOccluderHeight(d1, I2R(7))
    set oh = R2I(GetDestructableOccluderHeight(d1))
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(
        int(&host, "l0"),
        300,
        "a new destructable starts at the max life"
    );
    assert_eq!(int(&host, "m0"), 300);
    assert_eq!(int(&host, "l1"), 120);
    assert_eq!(int(&host, "l2"), 0, "a negative life is stored as zero");
    assert_eq!(
        int(&host, "l3"),
        40,
        "a dead destructable is restored to the given life"
    );
    assert_eq!(
        int(&host, "l4"),
        40,
        "a restore on a living destructable does nothing"
    );
    assert_eq!(
        int(&host, "l5"),
        0,
        "KillDestructable sets the life to zero"
    );
    assert_eq!(
        int(&host, "l6"),
        300,
        "a restore with life zero uses the max life"
    );
    assert_eq!(int(&host, "m1"), 500);
    assert_eq!(int(&host, "oh"), 7);
}

/// A death is the change from a positive life to zero. It runs the death trigger once, with
/// `GetTriggerDestructable` set; killing a dead destructable again does not fire it, and a revived one
/// fires it again on its next death.
#[test]
fn kill_fires_the_death_event_once_per_death() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set d1 = CreateDestructable('ATtr', I2R(0), I2R(0), I2R(0), I2R(1), 0)
    set d2 = CreateDestructable('ATtr', I2R(9), I2R(9), I2R(0), I2R(1), 0)
    set idd1 = GetHandleId(d1)
    set t = CreateTrigger()
    set ev = TriggerRegisterDeathEvent(t, d1)
    set ta = TriggerAddAction(t, function on_death)
    call KillDestructable(d1)
    call KillDestructable(d1)
    call SetDestructableLife(d1, I2R(0))
    call SetDestructableLife(d1, I2R(50))
    call KillDestructable(d1)
    call KillDestructable(d2)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(
        int(&host, "dcnt"),
        2,
        "one event per death of d1, none for d2"
    );
    assert_eq!(
        int(&host, "dseen"),
        int(&host, "idd1"),
        "GetTriggerDestructable is the dying destructable"
    );
}

/// A unit can be the widget of `TriggerRegisterDeathEvent`. Its death fires the trigger, and
/// `GetTriggerDestructable` is null there.
#[test]
fn unit_death_event_through_trigger_register_death_event() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set t2 = CreateTrigger()
    set ev2 = TriggerRegisterDeathEvent(t2, u1)
    set ta2 = TriggerAddAction(t2, function on_unit_death)
    call KillUnit(u1)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "ucnt"), 1);
    assert_eq!(int(&host, "uwid"), 0, "a unit death gives no destructable");
}

/// The enumeration visits the live destructables in the rect in creation order. The filter runs for each of them
/// first; the enum scope and the action run only when the filter passed. A removed destructable and one outside
/// the rect are not visited.
#[test]
fn enum_visits_in_creation_order_with_the_filter_first() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set d1 = CreateDestructable('ATtr', I2R(1), I2R(0), I2R(0), I2R(1), 0)
    set d2 = CreateDestructable('ATtr', I2R(2), I2R(0), I2R(0), I2R(1), 0)
    set d3 = CreateDestructable('ATtr', I2R(3), I2R(0), I2R(0), I2R(1), 0)
    set d5 = CreateDestructable('ATtr', I2R(5), I2R(0), I2R(0), I2R(1), 0)
    set d6 = CreateDestructable('ATtr', I2R(20), I2R(0), I2R(0), I2R(1), 0)
    call RemoveDestructable(d5)
    set r = Rect(I2R(0), I2R(-5), I2R(10), I2R(5))
    set flt = Filter(function ef_filter)
    call EnumDestructablesInRect(r, flt, function ef_act)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(
        int(&host, "fseq"),
        123,
        "the filter ran for d1, d2, d3 in creation order"
    );
    assert_eq!(
        int(&host, "eseq"),
        23,
        "the action ran for the two that passed"
    );
    assert_eq!(int(&host, "ecnt"), 2);
    assert_eq!(
        int(&host, "same"),
        2,
        "GetFilterDestructable is the enumerated one"
    );
}

/// Invulnerability is a stored flag. It does not block a life change made from script, so a kill still works.
#[test]
fn invulnerability_is_a_flag_and_does_not_block_script_life_changes() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set d1 = CreateDestructable('ATtr', I2R(0), I2R(0), I2R(0), I2R(1), 0)
    call SetDestructableInvulnerable(d1, true)
    set iv1 = IsDestructableInvulnerable(d1)
    call SetDestructableLife(d1, I2R(10))
    set lv = R2I(GetDestructableLife(d1))
    call KillDestructable(d1)
    set lk = R2I(GetDestructableLife(d1))
    call SetDestructableInvulnerable(d1, false)
    set iv2 = IsDestructableInvulnerable(d1)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(boolean(&host, "iv1"));
    assert_eq!(int(&host, "lv"), 10);
    assert_eq!(int(&host, "lk"), 0);
    assert!(!boolean(&host, "iv2"));
}

/// A dead creation is a corpse with life zero and max life from the type. A removed destructable has no live
/// handle: its getters return the defaults, and its id is reused.
#[test]
fn dead_creation_and_removal_give_the_defaults() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set d1 = CreateDeadDestructable('ATtr', I2R(0), I2R(0), I2R(0), I2R(1), 0)
    set dl = R2I(GetDestructableLife(d1))
    set dm = R2I(GetDestructableMaxLife(d1))
    set idr1 = GetHandleId(d1)
    call KillDestructable(d1)
    call RemoveDestructable(d1)
    set rl = R2I(GetDestructableLife(d1))
    set rx = R2I(GetDestructableX(d1))
    set ivr = IsDestructableInvulnerable(d1)
    set d2 = CreateDestructable('LTlt', I2R(4), I2R(0), I2R(0), I2R(1), 0)
    set idr2 = GetHandleId(d2)
    set dx2 = R2I(GetDestructableX(d2))
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(int(&host, "dl"), 0);
    assert_eq!(int(&host, "dm"), 300);
    assert_eq!(int(&host, "rl"), 0);
    assert_eq!(int(&host, "rx"), 0);
    assert!(!boolean(&host, "ivr"));
    assert_eq!(
        int(&host, "idr2"),
        int(&host, "idr1"),
        "the removed id is reused"
    );
    assert_eq!(int(&host, "dx2"), 4);
}
