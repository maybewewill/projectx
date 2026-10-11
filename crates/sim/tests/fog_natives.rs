//! Fog of war natives through `ScriptHost` (`natives/fog.rs`).
//!
//! The checks are on the handle ids of the fog modifiers interleaved with timers (flag 0 allocation, the handle
//! allocated last, a failed creation takes none, a destroyed modifier's id is reused LIFO), on the visibility queries
//! per stored fog state (the start and stop of modifiers, the explicit `SetFogState*` states, the shared vision), and on
//! the null player and the null rect and location.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::fog::{FOG_FOGGED, FOG_MASKED, FOG_VISIBLE};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type location extends handle
type rect extends handle
type timer extends handle
type fogmodifier extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native Location takes real x, real y returns location
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native CreateTimer takes nothing returns timer
native GetHandleId takes handle h returns integer
native CreateFogModifierRadius takes player p, integer state, real x, real y, real radius, boolean useShared, boolean afterUnits returns fogmodifier
native CreateFogModifierRadiusLoc takes player p, integer state, location l, real radius, boolean useShared, boolean afterUnits returns fogmodifier
native CreateFogModifierRect takes player p, integer state, rect r, boolean useShared, boolean afterUnits returns fogmodifier
native DestroyFogModifier takes fogmodifier fm returns nothing
native FogModifierStart takes fogmodifier fm returns nothing
native FogModifierStop takes fogmodifier fm returns nothing
native SetFogStateRect takes player p, integer state, rect r, boolean useShared returns nothing
native SetFogStateRadius takes player p, integer state, real x, real y, real radius, boolean useShared returns nothing
native SetFogStateRadiusLoc takes player p, integer state, location l, real radius, boolean useShared returns nothing
native IsVisibleToPlayer takes real x, real y, player p returns boolean
native IsFoggedToPlayer takes real x, real y, player p returns boolean
native IsMaskedToPlayer takes real x, real y, player p returns boolean
native IsLocationVisibleToPlayer takes location l, player p returns boolean
native IsLocationFoggedToPlayer takes location l, player p returns boolean
native IsLocationMaskedToPlayer takes location l, player p returns boolean
native SetPlayerAlliance takes player a, player b, integer t, boolean v returns nothing
"#;

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm")
}

fn run(host: &mut ScriptHost) {
    host.start("main", &[]).expect("start");
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

/// Globals of the scripts: handle ids (`i*`) and results (`b*`).
const GLOBALS: &str = r#"
globals
integer ta = -1
integer tb = -1
integer tc = -1
integer td = -1
integer te = -1
integer tf = -1
integer tg = -1
boolean b1 = false
boolean b2 = false
boolean b3 = false
boolean b4 = false
boolean b5 = false
boolean b6 = false
boolean b7 = false
boolean b8 = false
boolean b9 = false
boolean b10 = false
boolean b11 = false
boolean b12 = false
timer t1
timer t2
timer t3
fogmodifier f1
fogmodifier f2
fogmodifier f3
fogmodifier f4
endglobals
"#;

/// Handle ids interleave with timers: a modifier takes the next id, a null player creates nothing, a destroyed
/// modifier's id is reused by the next allocation (LIFO), and a timer takes the freed id.
#[test]
fn fog_modifier_ids_interleave_with_timers() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set t1 = CreateTimer()
    set f1 = CreateFogModifierRadius(Player(0), 4, I2R(0), I2R(0), I2R(300), false, false)
    set t2 = CreateTimer()
    set f2 = CreateFogModifierRect(Player(20), 4, Rect(I2R(0), I2R(0), I2R(10), I2R(10)), false, false)
    set f3 = CreateFogModifierRect(Player(1), 4, Rect(I2R(0), I2R(0), I2R(10), I2R(10)), false, false)
    set ta = GetHandleId(t1)
    set tb = GetHandleId(f1)
    set tc = GetHandleId(t2)
    set td = GetHandleId(f3)
    call DestroyFogModifier(f1)
    set t3 = CreateTimer()
    set te = GetHandleId(t3)
    set f4 = CreateFogModifierRadius(Player(0), 2, I2R(0), I2R(0), I2R(5), false, false)
    set tf = GetHandleId(f4)
endfunction
"#
    ));
    run(&mut host);
    // Player(i) takes its handle the first time a script asks for it (flag 0), and Rect(...) takes one too, so the
    // ids below count those as well: t1 1, Player(0) 2 and f1 3, t2 4, the rect of the null-player call 5, Player(1) 6,
    // its rect 7 and f3 8.
    assert_eq!(int(&host, "ta"), 0x100001);
    assert_eq!(int(&host, "tb"), 0x100003);
    // The null-player creation allocated nothing after its rect: t2 takes the next id after f1.
    assert_eq!(int(&host, "tc"), 0x100004);
    assert_eq!(int(&host, "td"), 0x100008);
    // Destroying f1 frees its id (0x100003) and the next allocation reuses it (LIFO).
    assert_eq!(int(&host, "te"), 0x100003);
    // Player(0) already has its id (0x100002): f4 takes the next new id.
    assert_eq!(int(&host, "tf"), 0x100009);
}

#[test]
fn visibility_follows_started_and_stopped_modifiers() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set f1 = CreateFogModifierRadius(Player(0), 4, I2R(0), I2R(0), I2R(300), false, false)
    set b1 = IsMaskedToPlayer(I2R(100), I2R(100), Player(0))
    set b2 = IsVisibleToPlayer(I2R(100), I2R(100), Player(0))
    call FogModifierStart(f1)
    set b3 = IsVisibleToPlayer(I2R(100), I2R(100), Player(0))
    set b4 = IsVisibleToPlayer(I2R(1000), I2R(1000), Player(0))
    set b5 = IsVisibleToPlayer(I2R(100), I2R(100), Player(1))
    call FogModifierStop(f1)
    set b6 = IsVisibleToPlayer(I2R(100), I2R(100), Player(0))
    set b7 = IsFoggedToPlayer(I2R(100), I2R(100), Player(0))
    set b8 = IsMaskedToPlayer(I2R(1000), I2R(1000), Player(0))
    set b9 = IsVisibleToPlayer(I2R(100), I2R(100), Player(20))
endfunction
"#
    ));
    run(&mut host);
    assert!(boolean(&host, "b1"), "a new map is masked");
    assert!(!boolean(&host, "b2"));
    assert!(boolean(&host, "b3"), "a started modifier reveals its cells");
    assert!(
        !boolean(&host, "b4"),
        "a point outside the radius stays hidden"
    );
    assert!(!boolean(&host, "b5"), "another player does not see it");
    assert!(
        !boolean(&host, "b6"),
        "a stopped modifier no longer reveals"
    );
    assert!(
        boolean(&host, "b7"),
        "an explored cell is fogged after the vision leaves"
    );
    assert!(boolean(&host, "b8"));
    assert!(!boolean(&host, "b9"), "a null player answers false");
}

#[test]
fn rect_and_location_modifiers_and_location_queries() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set f1 = CreateFogModifierRect(Player(1), 4, Rect(I2R(0), I2R(0), I2R(200), I2R(200)), false, false)
    call FogModifierStart(f1)
    set b1 = IsLocationVisibleToPlayer(Location(I2R(50), I2R(50)), Player(1))
    set b2 = IsLocationVisibleToPlayer(Location(I2R(900), I2R(50)), Player(1))
    set f2 = CreateFogModifierRadiusLoc(Player(2), 2, Location(I2R(0), I2R(0)), I2R(300), false, false)
    call FogModifierStart(f2)
    set b3 = IsLocationFoggedToPlayer(Location(I2R(50), I2R(50)), Player(2))
    set b4 = IsLocationMaskedToPlayer(Location(I2R(900), I2R(900)), Player(2))
    set f3 = CreateFogModifierRect(Player(1), 4, Rect(I2R(0), I2R(0), I2R(1), I2R(1)), false, false)
    set b5 = IsLocationVisibleToPlayer(Location(I2R(50), I2R(50)), Player(1))
endfunction
"#
    ));
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(!boolean(&host, "b2"));
    // A fogged modifier (state 2) makes the cell fogged, not visible.
    assert!(boolean(&host, "b3"));
    assert!(boolean(&host, "b4"));
    // A modifier that was never started does not change the state.
    assert!(boolean(&host, "b5"));
}

#[test]
fn explicit_fog_states_and_shared_vision() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call SetFogStateRect(Player(0), 4, Rect(I2R(0), I2R(0), I2R(256), I2R(256)), false)
    set b1 = IsVisibleToPlayer(I2R(100), I2R(100), Player(0))
    call SetFogStateRadius(Player(0), 2, I2R(500), I2R(500), I2R(100), false)
    set b2 = IsFoggedToPlayer(I2R(500), I2R(500), Player(0))
    call SetFogStateRadiusLoc(Player(0), 1, Location(I2R(900), I2R(900)), I2R(100), false)
    set b3 = IsMaskedToPlayer(I2R(900), I2R(900), Player(0))
    call SetPlayerAlliance(Player(3), Player(4), 5, true)
    set f1 = CreateFogModifierRadius(Player(3), 4, I2R(2000), I2R(2000), I2R(200), true, false)
    call FogModifierStart(f1)
    set b4 = IsVisibleToPlayer(I2R(2000), I2R(2000), Player(4))
    set b5 = IsVisibleToPlayer(I2R(2000), I2R(2000), Player(3))
    set f2 = CreateFogModifierRadius(Player(3), 4, I2R(3000), I2R(3000), I2R(200), false, false)
    call FogModifierStart(f2)
    set b6 = IsVisibleToPlayer(I2R(3000), I2R(3000), Player(4))
endfunction
"#
    ));
    run(&mut host);
    assert!(
        boolean(&host, "b1"),
        "an explicit visible state shows the cell"
    );
    assert!(boolean(&host, "b2"), "an explicit fogged state");
    assert!(boolean(&host, "b3"), "an explicit masked state");
    assert!(
        boolean(&host, "b4"),
        "shared vision reaches the allied player"
    );
    assert!(boolean(&host, "b5"));
    assert!(
        !boolean(&host, "b6"),
        "without useShared the ally does not see it"
    );
    let g = host.game();
    // The same states read back by point (100, 100) visible, (500, 500) fogged, (900, 900) masked.
    let p = |v: f32| F32::from_bits(v.to_bits());
    assert_eq!(g.fog_state_at(0, p(100.0), p(100.0)), FOG_VISIBLE);
    assert_eq!(g.fog_state_at(0, p(500.0), p(500.0)), FOG_FOGGED);
    assert_eq!(g.fog_state_at(0, p(900.0), p(900.0)), FOG_MASKED);
}

#[test]
fn null_handles_create_nothing() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set f1 = CreateFogModifierRadiusLoc(Player(0), 4, null, I2R(10), false, false)
    set b1 = f1 == null
    set f2 = CreateFogModifierRect(Player(0), 4, null, false, false)
    set b2 = f2 == null
    set t1 = CreateTimer()
    set ta = GetHandleId(t1)
endfunction
"#
    ));
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(boolean(&host, "b2"));
    // Player(0) took 0x100001 (its first use); the timer takes the next id.
    assert_eq!(int(&host, "ta"), 0x100002);
}
