//! Misc natives through `ScriptHost` (`natives/misc.rs`): blight, terrain, the preload log, start locations and the
//! player settings that do not depend on units.
//!
//! The terrain provider test uses a Rust implementation of the trait, to check that the provider answers when the
//! script has written nothing, and that a script's write takes precedence over it.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::misc::{MiscEvent, TerrainProvider};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type location extends handle
type rect extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native Location takes real x, real y returns location

native SetBlightPoint takes player p, real x, real y, boolean add returns nothing
native SetBlight takes player p, real x, real y, real radius, boolean add returns nothing
native SetBlightRect takes player p, rect r, boolean add returns nothing
native IsPointBlighted takes real x, real y returns boolean
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native GetTerrainType takes real x, real y returns integer
native GetTerrainVariance takes real x, real y returns integer
native GetTerrainCliffLevel takes real x, real y returns integer
native SetTerrainType takes real x, real y, integer t, integer v, integer area, integer shape returns nothing
native IsTerrainPathable takes real x, real y, integer pathing returns boolean
native SetTerrainPathable takes real x, real y, integer pathing, boolean flag returns nothing
native Preload takes string s returns nothing
native PreloadStart takes nothing returns nothing
native PreloadEnd takes real t returns nothing
native DefineStartLocation takes integer i, real x, real y returns nothing
native GetStartLocationX takes integer i returns real
native GetStartLocationY takes integer i returns real
native SetStartLocPrioCount takes integer i, integer n returns nothing
native SetStartLocPrio takes integer i, integer slot, integer other, integer t returns nothing
native GetStartLocPrioSlot takes integer i, integer slot returns integer
native GetStartLocPrio takes integer i, integer slot returns integer
native ForcePlayerStartLocation takes player p, integer i returns nothing
native GetPlayerStartLocation takes player p returns integer
native GetPlayerStartLocationX takes player p returns real
native SetPlayerRacePreference takes player p, integer pref returns nothing
native IsPlayerRacePrefSet takes player p, integer pref returns boolean
native SetPlayerRaceSelectable takes player p, boolean flag returns nothing
native GetPlayerSelectable takes player p returns boolean
native SetPlayerOnScoreScreen takes player p, boolean flag returns nothing
native GetPlayerHandicap takes player p returns real
native SetPlayerHandicap takes player p, real h returns nothing
native GetPlayerHandicapXP takes player p returns real
native SetPlayerHandicapXP takes player p, real h returns nothing
native SetPlayerTaxRate takes player a, player b, integer state, integer rate returns nothing
native GetPlayerTaxRate takes player a, player b, integer state returns integer
native SetMapFlag takes integer f, boolean flag returns nothing
native SetPlayerTechMaxAllowed takes player p, integer tech, integer max returns nothing
native GetPlayerTechMaxAllowed takes player p, integer tech returns integer
native AddPlayerTechResearched takes player p, integer tech, integer count returns nothing
native SetPlayerTechResearched takes player p, integer tech, integer level returns nothing
native GetPlayerTechResearched takes player p, integer tech, boolean specific returns boolean
native GetPlayerTechCount takes player p, integer tech, boolean specific returns integer
native SetPlayerAbilityAvailable takes player p, integer ability, boolean flag returns nothing
native GetHandleId takes handle h returns integer
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

fn real(host: &ScriptHost, name: &str) -> f32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_real())
        .unwrap_or_else(|| panic!("global {name} is not a real"))
        .to_f32()
}

const GLOBALS: &str = r#"
globals
integer a = -1
integer b = -1
integer c = -1
integer d = -1
boolean ok = false
boolean ok2 = false
boolean ok3 = false
real r1 = 0.0
real r2 = 0.0
real r3 = 0.0
endglobals
"#;

#[test]
fn blight_points_radius_and_rects() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set ok = IsPointBlighted(I2R(10), I2R(10))
    call SetBlightPoint(Player(0), I2R(10), I2R(10), true)
    set ok2 = IsPointBlighted(I2R(10), I2R(10))
    set ok3 = IsPointBlighted(I2R(100), I2R(100))
    call SetBlight(Player(0), I2R(100), I2R(100), I2R(40), true)
    call SetBlightRect(Player(0), Rect(I2R(0), I2R(200), I2R(64), I2R(264)), true)
    call SetBlightPoint(Player(0), I2R(10), I2R(10), false)
endfunction
"#
    ));
    // Read the state after the run: the first checks happen before the blight is set, so they are re-read below.
    run(&mut host);
    let g = host.game();
    let p = |v: f32| F32::from_bits(v.to_bits());
    // The point (10, 10) was blighted and then cleared.
    assert!(!g.is_blighted(p(10.0), p(10.0)));
    assert!(g.is_blighted(p(100.0), p(100.0)));
    // A cell 3 cells away from the centre is outside a 40 radius.
    assert!(!g.is_blighted(p(100.0 + 3.0 * 32.0), p(100.0)));
    // The rect covers cells 0..=2 on x and 6..=8 on y (inclusive ends).
    assert!(g.is_blighted(p(10.0), p(200.0)));
    assert!(g.is_blighted(p(40.0), p(260.0)));
    assert!(!g.is_blighted(p(300.0), p(10.0)));
    drop(g);
    assert!(!boolean(&host, "ok"));
    assert!(boolean(&host, "ok2"), "a point blight reads back at once");
    assert!(
        !boolean(&host, "ok3"),
        "the blight radius does not reach here"
    );
}

#[test]
fn terrain_writes_read_back_and_defaults_apply_without_a_provider() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set a = GetTerrainType(I2R(300), I2R(300))
    set b = GetTerrainVariance(I2R(300), I2R(300))
    set c = GetTerrainCliffLevel(I2R(300), I2R(300))
    set ok = IsTerrainPathable(I2R(300), I2R(300), 1)
    call SetTerrainType(I2R(300), I2R(300), 77, 3, -1, 0)
    set d = GetTerrainType(I2R(310), I2R(310))
    set a = GetTerrainType(I2R(300), I2R(300))
    set b = GetTerrainVariance(I2R(300), I2R(300))
    call SetTerrainPathable(I2R(300), I2R(300), 1, false)
    set ok2 = IsTerrainPathable(I2R(300), I2R(300), 1)
    set ok3 = IsTerrainPathable(I2R(300), I2R(300), 2)
endfunction
"#
    ));
    run(&mut host);
    assert_eq!(
        int(&host, "d"),
        77,
        "the tile is 128 units, so (310, 310) shares the write"
    );
    assert_eq!(int(&host, "a"), 77);
    assert_eq!(int(&host, "b"), 3);
    assert!(!boolean(&host, "ok2"));
    assert!(
        boolean(&host, "ok3"),
        "another pathing type is not affected"
    );
}

struct FakeTerrain;

impl TerrainProvider for FakeTerrain {
    fn tile_type(&self, _x: F32, _y: F32) -> Option<u32> {
        Some(9)
    }
    fn tile_variance(&self, _x: F32, _y: F32) -> Option<u32> {
        None
    }
    fn cliff_level(&self, _x: F32, _y: F32) -> Option<u32> {
        Some(2)
    }
    fn pathable(&self, _x: F32, _y: F32, pathing: u32) -> Option<bool> {
        (pathing == 1).then_some(false)
    }
}

#[test]
fn terrain_provider_answers_until_the_script_writes() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set a = GetTerrainType(I2R(50), I2R(50))
    set b = GetTerrainCliffLevel(I2R(50), I2R(50))
    set c = GetTerrainVariance(I2R(50), I2R(50))
    set ok = IsTerrainPathable(I2R(50), I2R(50), 1)
    call SetTerrainType(I2R(50), I2R(50), 4, 0, 0, 0)
    set d = GetTerrainType(I2R(50), I2R(50))
endfunction
"#
    ));
    host.game_mut()
        .set_terrain_provider(Some(Box::new(FakeTerrain)));
    run(&mut host);
    assert_eq!(int(&host, "a"), 9);
    assert_eq!(int(&host, "b"), 2);
    assert_eq!(
        int(&host, "c"),
        0,
        "the provider gave no variance: the default applies"
    );
    assert!(!boolean(&host, "ok"));
    assert_eq!(int(&host, "d"), 4, "the script's write takes precedence");
}

#[test]
fn preload_natives_log_their_calls() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call Preload("x.ini")
    call PreloadStart()
    call PreloadEnd(1.5)
endfunction
"#
    ));
    run(&mut host);
    let events = host.game().misc_events().to_vec();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[0],
        MiscEvent::Preload {
            native: "Preload",
            text: "x.ini".to_string()
        }
    );
    assert_eq!(
        events[1],
        MiscEvent::Preload {
            native: "PreloadStart",
            text: String::new()
        }
    );
    assert_eq!(
        events[2],
        MiscEvent::Preload {
            native: "PreloadEnd",
            text: "1.5".to_string()
        }
    );
}

#[test]
fn start_locations_and_priorities() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call DefineStartLocation(0, I2R(100), I2R(200))
    call ForcePlayerStartLocation(Player(2), 0)
    set r1 = GetPlayerStartLocationX(Player(2))
    set r2 = GetStartLocationY(0)
    set a = GetPlayerStartLocation(Player(2))
    set r3 = GetStartLocationX(12)
    call SetStartLocPrioCount(0, 3)
    call SetStartLocPrio(0, 1, 5, 2)
    set c = GetStartLocPrioSlot(0, 1)
    set d = GetStartLocPrio(0, 1)
endfunction
"#
    ));
    run(&mut host);
    assert_eq!(real(&host, "r1"), 100.0);
    assert_eq!(real(&host, "r2"), 200.0);
    assert_eq!(int(&host, "a"), 0);
    assert_eq!(real(&host, "r3"), 0.0, "an index of 12 or more reads 0");
    assert_eq!(int(&host, "c"), 5);
    assert_eq!(int(&host, "d"), 2);
}

#[test]
fn player_race_handicap_and_tax_settings() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call SetPlayerRaceSelectable(Player(0), false)
    set ok = GetPlayerSelectable(Player(0))
    call SetPlayerRacePreference(Player(0), 0x40 + 2)
    set ok2 = IsPlayerRacePrefSet(Player(0), 2)
    set ok3 = GetPlayerSelectable(Player(0))
    call SetPlayerHandicap(Player(0), 0.5)
    call SetPlayerHandicap(Player(0), 0.0005)
    set r1 = GetPlayerHandicap(Player(0))
    call SetPlayerTaxRate(Player(0), Player(1), 1, 30)
    set a = GetPlayerTaxRate(Player(0), Player(1), 1)
    set b = GetPlayerTaxRate(Player(0), Player(1), 2)
    call SetPlayerOnScoreScreen(Player(0), true)
    call SetMapFlag(256, true)
    call SetPlayerTaxRate(Player(0), Player(1), 1, 99)
    set c = GetPlayerTaxRate(Player(0), Player(1), 1)
endfunction
"#
    ));
    run(&mut host);
    assert!(!boolean(&host, "ok"));
    assert!(boolean(&host, "ok2"));
    assert!(
        !boolean(&host, "ok3"),
        "the preference write keeps the selectable bit"
    );
    assert_eq!(real(&host, "r1"), 0.5, "a handicap below 0.001 is refused");
    assert_eq!(int(&host, "a"), 30);
    assert_eq!(int(&host, "b"), 0);
    assert_eq!(int(&host, "c"), 30, "the tax lock flag blocks the write");
}

#[test]
fn player_tech_levels_and_abilities() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call AddPlayerTechResearched(Player(0), 1, 2)
    set a = GetPlayerTechCount(Player(0), 1, true)
    call SetPlayerTechMaxAllowed(Player(0), 1, 3)
    call AddPlayerTechResearched(Player(0), 1, 5)
    set b = GetPlayerTechCount(Player(0), 1, true)
    call SetPlayerTechResearched(Player(0), 1, 1)
    set c = GetPlayerTechCount(Player(0), 1, true)
    set ok = GetPlayerTechResearched(Player(0), 1, true)
    set d = GetPlayerTechMaxAllowed(Player(0), 1)
    call SetPlayerAbilityAvailable(Player(0), 77, false)
endfunction
"#
    ));
    run(&mut host);
    assert_eq!(int(&host, "a"), 2);
    // The max of 3 stops the adds at 3 levels.
    assert_eq!(int(&host, "b"), 3);
    // Setting a lower level does not remove levels.
    assert_eq!(int(&host, "c"), 3);
    assert!(boolean(&host, "ok"));
    assert_eq!(int(&host, "d"), 3);
    assert!(host
        .game()
        .player_misc(0)
        .unavailable_abilities
        .contains(&77));
}
