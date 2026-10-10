//! Hashtable and game cache natives through `ScriptHost` (`natives/hashtable.rs`, `natives/gamecache.rs`).
//!
//! Handle ids are compared with each other, never with literals (the first id of a map is LIKELY
//! 0x100001, not CONFIRMED). Strings are compared inside the scripts, where the VM interns them.
//! Real values come from `I2R`, so the scripts need no lookup tables.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::ScriptHost;
const PRELUDE: &str = r#"
type hashtable extends handle
type gamecache extends handle
type timer extends handle
type trigger extends handle
type location extends handle
type unit extends handle

native InitHashtable takes nothing returns hashtable
native SaveInteger takes hashtable t, integer p, integer c, integer v returns nothing
native SaveReal takes hashtable t, integer p, integer c, real v returns nothing
native SaveBoolean takes hashtable t, integer p, integer c, boolean v returns nothing
native SaveStr takes hashtable t, integer p, integer c, string v returns boolean
native SaveTimerHandle takes hashtable t, integer p, integer c, timer v returns boolean
native SaveTriggerHandle takes hashtable t, integer p, integer c, trigger v returns boolean
native SaveLocationHandle takes hashtable t, integer p, integer c, location v returns boolean
native SaveUnitHandle takes hashtable t, integer p, integer c, unit v returns boolean
native LoadInteger takes hashtable t, integer p, integer c returns integer
native LoadReal takes hashtable t, integer p, integer c returns real
native LoadBoolean takes hashtable t, integer p, integer c returns boolean
native LoadStr takes hashtable t, integer p, integer c returns string
native LoadTimerHandle takes hashtable t, integer p, integer c returns timer
native LoadTriggerHandle takes hashtable t, integer p, integer c returns trigger
native HaveSavedInteger takes hashtable t, integer p, integer c returns boolean
native HaveSavedReal takes hashtable t, integer p, integer c returns boolean
native HaveSavedBoolean takes hashtable t, integer p, integer c returns boolean
native HaveSavedString takes hashtable t, integer p, integer c returns boolean
native HaveSavedHandle takes hashtable t, integer p, integer c returns boolean
native RemoveSavedInteger takes hashtable t, integer p, integer c returns nothing
native RemoveSavedHandle takes hashtable t, integer p, integer c returns nothing
native LoadLocationHandle takes hashtable t, integer p, integer c returns location
native FlushChildHashtable takes hashtable t, integer p returns nothing
native FlushParentHashtable takes hashtable t returns nothing

native InitGameCache takes string name returns gamecache
native StoreInteger takes gamecache c, string m, string k, integer v returns nothing
native StoreReal takes gamecache c, string m, string k, real v returns nothing
native StoreBoolean takes gamecache c, string m, string k, boolean v returns nothing
native StoreString takes gamecache c, string m, string k, string v returns boolean
native StoreUnit takes gamecache c, string m, string k, unit u returns boolean
native GetStoredInteger takes gamecache c, string m, string k returns integer
native GetStoredReal takes gamecache c, string m, string k returns real
native GetStoredBoolean takes gamecache c, string m, string k returns boolean
native GetStoredString takes gamecache c, string m, string k returns string
native HaveStoredInteger takes gamecache c, string m, string k returns boolean
native HaveStoredReal takes gamecache c, string m, string k returns boolean
native HaveStoredBoolean takes gamecache c, string m, string k returns boolean
native HaveStoredString takes gamecache c, string m, string k returns boolean
native FlushStoredInteger takes gamecache c, string m, string k returns nothing
native FlushStoredReal takes gamecache c, string m, string k returns nothing
native FlushStoredString takes gamecache c, string m, string k returns nothing
native FlushStoredMission takes gamecache c, string m returns nothing
native FlushGameCache takes gamecache c returns nothing
native SaveGameCache takes gamecache c returns boolean

native CreateTimer takes nothing returns timer
native DestroyTimer takes timer t returns nothing
native CreateTrigger takes nothing returns trigger
native DestroyTrigger takes trigger t returns nothing
native GetHandleId takes handle h returns integer
native I2R takes integer i returns real
native Location takes real x, real y returns location
native RemoveLocation takes location l returns nothing
"#;

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm")
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

fn real_bits(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_real())
        .map(|r| r.to_bits())
        .unwrap_or_else(|| panic!("global {name} is not a real"))
}

#[test]
fn missing_keys_read_as_the_default_of_their_kind() {
    let src = r#"
globals
    hashtable gH = null
    integer gI = 99
    boolean gB = true
    boolean gHaveI = true
    boolean gHaveS = true
    boolean gEmpty = false
    boolean gRealZero = false
    boolean gHandleNull = false
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gI = LoadInteger(gH, 1, 2)
    set gB = LoadBoolean(gH, 1, 2)
    set gHaveI = HaveSavedInteger(gH, 1, 2)
    set gHaveS = HaveSavedString(gH, 1, 2)
    set gEmpty = LoadStr(gH, 1, 2) == ""
    set gRealZero = LoadReal(gH, 1, 2) == I2R(0)
    set gHandleNull = LoadTimerHandle(gH, 1, 2) == null
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gI"), 0);
    assert!(!boolean(&host, "gB"));
    assert!(!boolean(&host, "gHaveI"));
    assert!(!boolean(&host, "gHaveS"));
    assert!(boolean(&host, "gEmpty"));
    assert!(boolean(&host, "gRealZero"));
    assert!(boolean(&host, "gHandleNull"));
}

#[test]
fn each_value_kind_has_its_own_slot_and_keys_are_parent_child_pairs() {
    let src = r#"
globals
    hashtable gH = null
    integer gI = 0
    real gR = 0.0
    boolean gB = false
    boolean gS = false
    boolean gHaveRealAt12 = true
    integer gSwap = 0
    integer gOther = 0
    boolean gOtherHaveI = true
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    call SaveInteger(gH, 1, 2, 42)
    call SaveBoolean(gH, 1, 2, true)
    call SaveStr(gH, 1, 2, "text")
    set gI = LoadInteger(gH, 1, 2)
    set gB = LoadBoolean(gH, 1, 2)
    set gS = LoadStr(gH, 1, 2) == "text"
    set gHaveRealAt12 = HaveSavedReal(gH, 1, 2)
    // (2, 1) and (1, 3) are different keys from (1, 2).
    call SaveInteger(gH, 2, 1, 7)
    call SaveInteger(gH, 1, 3, 8)
    set gSwap = LoadInteger(gH, 2, 1)
    set gOther = LoadInteger(gH, 1, 3)
    set gOtherHaveI = HaveSavedInteger(gH, 3, 1)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gI"), 42);
    assert!(boolean(&host, "gB"));
    assert!(boolean(&host, "gS"));
    assert!(!boolean(&host, "gHaveRealAt12"));
    assert_eq!(int(&host, "gSwap"), 7);
    assert_eq!(int(&host, "gOther"), 8);
    assert!(!boolean(&host, "gOtherHaveI"));
}

#[test]
fn overwrite_replaces_the_value_and_real_keeps_its_bits() {
    let src = r#"
globals
    hashtable gH = null
    integer gI = 0
    real gR = 0.0
    boolean gS = false
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    call SaveInteger(gH, 5, 5, 1)
    call SaveInteger(gH, 5, 5, 2)
    set gI = LoadInteger(gH, 5, 5)
    call SaveReal(gH, 5, 5, I2R(16777219))
    set gR = LoadReal(gH, 5, 5)
    call SaveStr(gH, 5, 5, "first")
    call SaveStr(gH, 5, 5, "second")
    set gS = LoadStr(gH, 5, 5) == "second"
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gI"), 2);
    // I2R truncates the mantissa to 24 bits (16777219 -> 16777218), and the table keeps the bits.
    assert_eq!(real_bits(&host, "gR"), 16_777_218f32.to_bits());
    assert!(boolean(&host, "gS"));
}

#[test]
fn remove_and_have_per_kind() {
    let src = r#"
globals
    hashtable gH = null
    boolean gHaveAfterRemove = true
    boolean gHaveOtherKind = false
    integer gI = 5
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    call SaveInteger(gH, 1, 1, 9)
    call SaveBoolean(gH, 1, 1, true)
    call RemoveSavedInteger(gH, 1, 1)
    set gHaveAfterRemove = HaveSavedInteger(gH, 1, 1)
    set gHaveOtherKind = HaveSavedBoolean(gH, 1, 1)
    set gI = LoadInteger(gH, 1, 1)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(!boolean(&host, "gHaveAfterRemove"));
    assert!(boolean(&host, "gHaveOtherKind"));
    assert_eq!(int(&host, "gI"), 0);
}

#[test]
fn flush_child_removes_one_parent_and_flush_parent_removes_everything() {
    let src = r#"
globals
    hashtable gH = null
    boolean gChild11 = true
    boolean gChild12 = true
    boolean gChild21 = false
    boolean gAfterParent = true
    boolean gAfterReuse = false
    integer gReuse = 0
endglobals

function HaveSavedStringOrBool takes nothing returns boolean
    return HaveSavedString(gH, 3, 3) or HaveSavedBoolean(gH, 3, 4) or HaveSavedInteger(gH, 2, 1)
endfunction

function Main takes nothing returns nothing
    set gH = InitHashtable()
    call SaveInteger(gH, 1, 1, 1)
    call SaveReal(gH, 1, 2, I2R(2))
    call SaveInteger(gH, 2, 1, 3)
    call FlushChildHashtable(gH, 1)
    set gChild11 = HaveSavedInteger(gH, 1, 1)
    set gChild12 = HaveSavedReal(gH, 1, 2)
    set gChild21 = HaveSavedInteger(gH, 2, 1)
    call SaveStr(gH, 3, 3, "x")
    call SaveBoolean(gH, 3, 4, true)
    call FlushParentHashtable(gH)
    set gAfterParent = HaveSavedStringOrBool()
    call SaveInteger(gH, 1, 1, 77)
    set gReuse = LoadInteger(gH, 1, 1)
    set gAfterReuse = HaveSavedInteger(gH, 1, 1)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(!boolean(&host, "gChild11"));
    assert!(!boolean(&host, "gChild12"));
    assert!(boolean(&host, "gChild21"));
    assert!(!boolean(&host, "gAfterParent"));
    assert_eq!(int(&host, "gReuse"), 77);
    assert!(boolean(&host, "gAfterReuse"));
}

#[test]
fn a_saved_handle_keeps_its_id_after_the_object_is_destroyed() {
    let src = r#"
globals
    hashtable gH = null
    timer gT = null
    timer gU = null
    timer gL = null
    integer gIdT = 0
    integer gIdU = 0
    boolean gSaved = false
    boolean gHaveAfterDestroy = false
    boolean gLoadedDead = false
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gT = CreateTimer()
    set gIdT = GetHandleId(gT)
    set gSaved = SaveTimerHandle(gH, 1, 1, gT)
    call DestroyTimer(gT)
    // The table still holds the id, so the next timer gets a fresh id.
    set gU = CreateTimer()
    set gIdU = GetHandleId(gU)
    set gHaveAfterDestroy = HaveSavedHandle(gH, 1, 1)
    // The object is gone, so the typed load gives the null handle.
    set gL = LoadTimerHandle(gH, 1, 1)
    set gLoadedDead = GetHandleId(gL) == 0
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(boolean(&host, "gSaved"));
    assert_ne!(int(&host, "gIdU"), int(&host, "gIdT"));
    assert!(boolean(&host, "gHaveAfterDestroy"));
    assert!(boolean(&host, "gLoadedDead"));
}

#[test]
fn flushing_the_child_releases_the_handle_so_its_id_is_reused() {
    let src = r#"
globals
    hashtable gH = null
    timer gT = null
    timer gU = null
    integer gIdT = 0
    integer gIdU = 0
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gT = CreateTimer()
    set gIdT = GetHandleId(gT)
    call SaveTimerHandle(gH, 1, 1, gT)
    call DestroyTimer(gT)
    call FlushChildHashtable(gH, 1)
    set gU = CreateTimer()
    set gIdU = GetHandleId(gU)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gIdU"), int(&host, "gIdT"));
}

#[test]
fn overwriting_or_removing_a_saved_handle_releases_the_old_one() {
    let src = r#"
globals
    hashtable gH = null
    timer gT = null
    timer gU = null
    timer gV = null
    integer gIdT = 0
    integer gIdU = 0
    integer gIdV = 0
    integer gIdLoaded = 0
    boolean gOverwrote = false
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gT = CreateTimer()
    set gIdT = GetHandleId(gT)
    call SaveTimerHandle(gH, 1, 1, gT)
    call DestroyTimer(gT)
    set gU = CreateTimer()
    set gIdU = GetHandleId(gU)
    // Overwriting releases the table's reference to gT: its id is freed now.
    set gOverwrote = SaveTimerHandle(gH, 1, 1, gU)
    set gV = CreateTimer()
    set gIdV = GetHandleId(gV)
    set gIdLoaded = GetHandleId(LoadTimerHandle(gH, 1, 1))
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(boolean(&host, "gOverwrote"));
    assert_ne!(int(&host, "gIdU"), int(&host, "gIdT"));
    assert_eq!(int(&host, "gIdV"), int(&host, "gIdT"));
    assert_eq!(int(&host, "gIdLoaded"), int(&host, "gIdU"));
}

#[test]
fn remove_saved_handle_releases_the_reference() {
    let src = r#"
globals
    hashtable gH = null
    timer gT = null
    timer gV = null
    integer gIdT = 0
    integer gIdV = 0
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gT = CreateTimer()
    set gIdT = GetHandleId(gT)
    call SaveTimerHandle(gH, 1, 1, gT)
    call DestroyTimer(gT)
    call RemoveSavedHandle(gH, 1, 1)
    set gV = CreateTimer()
    set gIdV = GetHandleId(gV)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gIdV"), int(&host, "gIdT"));
}

#[test]
fn typed_handle_saves_reject_null_and_loads_check_the_kind() {
    let src = r#"
globals
    hashtable gH = null
    timer gT = null
    trigger gTr = null
    boolean gNullSaved = true
    boolean gStillTimerAfterNull = false
    boolean gOverwroteWithTrigger = false
    integer gIdTr = 0
    integer gIdLoadedTr = 0
    integer gLoadedAsTimer = 1
    integer gLoadedTimerAsTrigger = 1
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gT = CreateTimer()
    set gTr = CreateTrigger()
    call SaveTimerHandle(gH, 1, 1, gT)
    // A null handle is rejected by the typed check, so the timer stays.
    set gNullSaved = SaveTimerHandle(gH, 1, 1, null)
    set gStillTimerAfterNull = LoadTimerHandle(gH, 1, 1) == gT
    // A timer saved under a key is read as a null trigger.
    set gLoadedTimerAsTrigger = GetHandleId(LoadTriggerHandle(gH, 1, 1))
    // The handle slot is shared by all handle types: a trigger replaces the timer under the key.
    set gOverwroteWithTrigger = SaveTriggerHandle(gH, 1, 1, gTr)
    set gIdTr = GetHandleId(gTr)
    set gIdLoadedTr = GetHandleId(LoadTriggerHandle(gH, 1, 1))
    set gLoadedAsTimer = GetHandleId(LoadTimerHandle(gH, 1, 1))
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(!boolean(&host, "gNullSaved"));
    assert!(boolean(&host, "gStillTimerAfterNull"));
    assert_eq!(int(&host, "gLoadedTimerAsTrigger"), 0);
    assert!(boolean(&host, "gOverwroteWithTrigger"));
    assert_eq!(int(&host, "gIdLoadedTr"), int(&host, "gIdTr"));
    assert_eq!(int(&host, "gLoadedAsTimer"), 0);
}

#[test]
fn null_and_freed_tables_do_nothing() {
    let src = r#"
globals
    integer gI = 5
    boolean gSave = true
    boolean gHave = true
endglobals

function Main takes nothing returns nothing
    call SaveInteger(null, 1, 1, 3)
    set gI = LoadInteger(null, 1, 1)
    set gSave = SaveStr(null, 1, 1, "x")
    set gHave = HaveSavedInteger(null, 1, 1)
    call FlushParentHashtable(null)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gI"), 0);
    assert!(!boolean(&host, "gSave"));
    assert!(!boolean(&host, "gHave"));
}

#[test]
fn unit_handles_are_null_in_this_simulation_so_their_saves_fail() {
    let src = r#"
globals
    hashtable gH = null
    boolean gSaved = true
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gSaved = SaveUnitHandle(gH, 1, 1, null)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(!boolean(&host, "gSaved"));
}

#[test]
fn game_cache_stores_per_mission_and_key_with_defaults() {
    let src = r#"
globals
    gamecache gC = null
    gamecache gC2 = null
    gamecache gNone = null
    boolean gSameCache = false
    boolean gNullName = false
    integer gI = 9
    integer gMissing = 9
    boolean gHaveI = false
    boolean gHaveOtherMission = true
    boolean gHaveReal = false
    boolean gStr = false
    boolean gUnitStored = true
endglobals

function Main takes nothing returns nothing
    set gNone = InitGameCache("")
    set gNullName = GetHandleId(gNone) == 0
    set gC = InitGameCache("save1")
    set gC2 = InitGameCache("save1")
    set gSameCache = GetHandleId(gC) == GetHandleId(gC2)
    call StoreInteger(gC, "m", "k", 7)
    call StoreReal(gC, "m", "r", I2R(3))
    call StoreString(gC, "m", "s", "hi")
    set gI = GetStoredInteger(gC, "m", "k")
    set gMissing = GetStoredInteger(gC, "m", "other")
    set gHaveI = HaveStoredInteger(gC, "m", "k")
    set gHaveOtherMission = HaveStoredInteger(gC, "n", "k")
    call FlushStoredInteger(gC, "m", "k")
    set gHaveI = HaveStoredInteger(gC, "m", "k")
    set gHaveReal = HaveStoredReal(gC, "m", "r")
    set gStr = GetStoredString(gC, "m", "s") == "hi"
    set gUnitStored = StoreUnit(gC, "m", "u", null)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(boolean(&host, "gNullName"));
    assert!(boolean(&host, "gSameCache"));
    assert_eq!(int(&host, "gI"), 7);
    assert_eq!(int(&host, "gMissing"), 0);
    assert!(!boolean(&host, "gHaveI"));
    assert!(!boolean(&host, "gHaveOtherMission"));
    assert!(boolean(&host, "gHaveReal"));
    assert!(boolean(&host, "gStr"));
    assert!(!boolean(&host, "gUnitStored"));
}

#[test]
fn game_cache_flush_mission_and_flush_all() {
    let src = r#"
globals
    gamecache gC = null
    boolean gMissionGone = false
    boolean gOtherMissionKept = false
    boolean gAllGone = false
endglobals

function Main takes nothing returns nothing
    set gC = InitGameCache("save2")
    call StoreInteger(gC, "a", "x", 1)
    call StoreBoolean(gC, "a", "y", true)
    call StoreInteger(gC, "b", "x", 2)
    call FlushStoredMission(gC, "a")
    set gMissionGone = not HaveStoredInteger(gC, "a", "x") and not HaveStoredBoolean(gC, "a", "y")
    set gOtherMissionKept = HaveStoredInteger(gC, "b", "x")
    call FlushGameCache(gC)
    set gAllGone = not HaveStoredInteger(gC, "b", "x")
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(boolean(&host, "gMissionGone"));
    assert!(boolean(&host, "gOtherMissionKept"));
    assert!(boolean(&host, "gAllGone"));
}

#[test]
fn a_removed_location_loads_as_the_null_handle() {
    let src = r#"
globals
    hashtable gH = null
    location gL = null
    location gBack = null
    boolean gSaved = false
    boolean gBackNull = false
endglobals

function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gL = Location(I2R(3), I2R(4))
    set gSaved = SaveLocationHandle(gH, 1, 1, gL)
    call RemoveLocation(gL)
    set gBack = LoadLocationHandle(gH, 1, 1)
    set gBackNull = GetHandleId(gBack) == 0
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert!(boolean(&host, "gSaved"));
    assert!(boolean(&host, "gBackNull"));
}
