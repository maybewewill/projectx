//! The order in which a hashtable flush releases its saved handles (`natives/hashtable.rs` with
//! `sim::hashmap`). The released ids are reused last-freed-first, so the timers created after the flush
//! show the order. The map orders (bucket ascending, newest first inside a bucket) are the ones checked
//! in Unicorn for the child maps; the parent order is the same algorithm (LIKELY).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::ScriptHost;

const PRELUDE: &str = r#"
type hashtable extends handle
type timer extends handle

native InitHashtable takes nothing returns hashtable
native SaveTimerHandle takes hashtable t, integer p, integer c, timer v returns boolean
native FlushChildHashtable takes hashtable t, integer p returns nothing
native FlushParentHashtable takes hashtable t returns nothing
native CreateTimer takes nothing returns timer
native DestroyTimer takes timer t returns nothing
native GetHandleId takes handle h returns integer
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

const GLOBALS: &str = r#"
globals
    hashtable gH = null
    timer gA = null
    timer gB = null
    timer gC = null
    integer gIdA = 0
    integer gIdB = 0
    integer gIdC = 0
    timer gN1 = null
    timer gN2 = null
    timer gN3 = null
    integer gId1 = 0
    integer gId2 = 0
    integer gId3 = 0
endglobals
"#;

/// Children 0 and 4 share bucket 0 (inserted in that order), child 1 is in bucket 1. Bucket order with
/// the newest first inside a bucket releases 4, 0, 1 (gB, gA, gC), so the next three timers take the
/// ids of gC, gA, gB. A release in key order (0, 1, 4) would give gB, gC, gA.
#[test]
fn flush_child_releases_handles_in_bucket_order() {
    let body = r#"
function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gA = CreateTimer()
    set gB = CreateTimer()
    set gC = CreateTimer()
    set gIdA = GetHandleId(gA)
    set gIdB = GetHandleId(gB)
    set gIdC = GetHandleId(gC)
    call SaveTimerHandle(gH, 7, 0, gA)
    call SaveTimerHandle(gH, 7, 4, gB)
    call SaveTimerHandle(gH, 7, 1, gC)
    call DestroyTimer(gA)
    call DestroyTimer(gB)
    call DestroyTimer(gC)
    call FlushChildHashtable(gH, 7)
    set gN1 = CreateTimer()
    set gN2 = CreateTimer()
    set gN3 = CreateTimer()
    set gId1 = GetHandleId(gN1)
    set gId2 = GetHandleId(gN2)
    set gId3 = GetHandleId(gN3)
endfunction
"#;
    let mut host = host_for(&format!("{GLOBALS}{body}"));
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gId1"), int(&host, "gIdC"));
    assert_eq!(int(&host, "gId2"), int(&host, "gIdA"));
    assert_eq!(int(&host, "gId3"), int(&host, "gIdB"));
}

/// Parents 1 and 5 share bucket 1 (5 is newer, so its node goes first), parent 2 is in bucket 2.
/// Releases: gB (parent 5), gA (parent 1), gC (parent 2), so the new timers take gC, gA, gB.
#[test]
fn flush_parent_releases_parents_in_bucket_order() {
    let body = r#"
function Main takes nothing returns nothing
    set gH = InitHashtable()
    set gA = CreateTimer()
    set gB = CreateTimer()
    set gC = CreateTimer()
    set gIdA = GetHandleId(gA)
    set gIdB = GetHandleId(gB)
    set gIdC = GetHandleId(gC)
    call SaveTimerHandle(gH, 1, 0, gA)
    call SaveTimerHandle(gH, 5, 0, gB)
    call SaveTimerHandle(gH, 2, 0, gC)
    call DestroyTimer(gA)
    call DestroyTimer(gB)
    call DestroyTimer(gC)
    call FlushParentHashtable(gH)
    set gN1 = CreateTimer()
    set gN2 = CreateTimer()
    set gN3 = CreateTimer()
    set gId1 = GetHandleId(gN1)
    set gId2 = GetHandleId(gN2)
    set gId3 = GetHandleId(gN3)
endfunction
"#;
    let mut host = host_for(&format!("{GLOBALS}{body}"));
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(int(&host, "gId1"), int(&host, "gIdC"));
    assert_eq!(int(&host, "gId2"), int(&host, "gIdA"));
    assert_eq!(int(&host, "gId3"), int(&host, "gIdB"));
}
