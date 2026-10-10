//! Player natives through `ScriptHost` (`natives/player.rs`). The expected values follow the notes of
//! that module: handle ids are created on first use (LIKELY), alliances are one-directional, and the
//! gold and lumber states are stored times 10 and clamped. The tests use integers and handles only, so
//! they need no lookup tables. With `GAME_DLL` set, the source is parsed with the original's tables too.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::{ScriptHost, HANDLE_BASE};
use softfloat::Tables;

const PRELUDE: &str = r#"
type player extends handle
type race extends handle
type mapcontrol extends handle
type playerslotstate extends handle
type playercolor extends handle
type alliancetype extends handle
type playerstate extends handle

native Player takes integer i returns player
native GetLocalPlayer takes nothing returns player
native GetPlayerId takes player p returns integer
native GetPlayerName takes player p returns string
native SetPlayerName takes player p, string s returns nothing
native GetPlayerRace takes player p returns race
native GetPlayerController takes player p returns mapcontrol
native SetPlayerController takes player p, mapcontrol c returns nothing
native GetPlayerSlotState takes player p returns playerslotstate
native GetPlayerTeam takes player p returns integer
native SetPlayerTeam takes player p, integer t returns nothing
native GetPlayerStartLocation takes player p returns integer
native SetPlayerStartLocation takes player p, integer l returns nothing
native GetPlayerColor takes player p returns playercolor
native SetPlayerColor takes player p, playercolor c returns nothing
native SetPlayerAlliance takes player p, player q, alliancetype t, boolean flag returns nothing
native GetPlayerAlliance takes player p, player q, alliancetype t returns boolean
native IsPlayerAlly takes player p, player q returns boolean
native IsPlayerEnemy takes player p, player q returns boolean
native IsPlayerObserver takes player p returns boolean
native GetPlayerState takes player p, playerstate s returns integer
native SetPlayerState takes player p, playerstate s, integer v returns nothing
native ConvertPlayerState takes integer i returns playerstate
native ConvertAllianceType takes integer i returns alliancetype
native ConvertPlayerColor takes integer i returns playercolor
native ConvertMapControl takes integer i returns mapcontrol
native ConvertRace takes integer i returns race
native GetHandleId takes handle h returns integer
"#;

/// The soft-float lookup tables of the user's `Game.dll` (`GAME_DLL`), or `None` when it is unset.
fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse_with(&full, tables().as_ref()).expect("parse");
    let module = compile(&[prog]).expect("compile");
    ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm")
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

/// Runs `Main` to completion.
fn run(host: &mut ScriptHost) {
    host.start("Main", &[]).unwrap();
    host.step();
}

#[test]
fn player_handles_come_from_the_allocator_on_first_use() {
    let src = r#"
globals
    integer gIdA = 0
    integer gIdB = 0
    integer gIdC = 0
    integer gIdAgain = 0
    integer gNull = 0
    integer gNegative = 0
endglobals

function Main takes nothing returns nothing
    set gIdA = GetHandleId(Player(3))
    set gIdB = GetHandleId(Player(0))
    set gIdAgain = GetHandleId(Player(3))
    set gIdC = GetHandleId(Player(15))
    set gNull = GetHandleId(Player(16))
    set gNegative = GetHandleId(Player(-1))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // The first id a game issues is HANDLE_BASE + 1 (LIKELY, docs/spec/handles-2.md 8.1). Each slot
    // gets its id when a script first asks for it, and asking again returns the same id.
    assert_eq!(int(&host, "gIdA") as u32, HANDLE_BASE + 1);
    assert_eq!(int(&host, "gIdB") as u32, HANDLE_BASE + 2);
    assert_eq!(int(&host, "gIdAgain"), int(&host, "gIdA"));
    assert_eq!(int(&host, "gIdC") as u32, HANDLE_BASE + 3);
    // Out-of-range indices give the null handle (UNSURE: the original does not check the index).
    assert_eq!(int(&host, "gNull"), 0);
    assert_eq!(int(&host, "gNegative"), 0);
}

#[test]
fn player_id_and_local_player_follow_the_host() {
    let src = r#"
globals
    integer gId = -1
    integer gLocal = -1
    integer gLocalNull = -1
endglobals

function Main takes nothing returns nothing
    set gId = GetPlayerId(Player(7))
    set gLocal = GetPlayerId(GetLocalPlayer())
    set gLocalNull = GetPlayerId(null)
endfunction
"#;
    let mut host = host_for(src);
    host.game_mut().set_local_player(4);
    run(&mut host);
    assert_eq!(int(&host, "gId"), 7);
    assert_eq!(int(&host, "gLocal"), 4);
    // A null player has id 0 (CONFIRMED: the getter returns 0 for a null handle).
    assert_eq!(int(&host, "gLocalNull"), 0);
}

#[test]
fn defaults_and_round_trips_of_the_slot_fields() {
    let src = r#"
globals
    string gName = ""
    integer gRace = 0
    integer gCtrl = -1
    integer gSlot = -1
    integer gTeam = -1
    integer gStart = -1
    integer gColor = -1
    integer gCtrlSet = -1
    integer gNullRace = -1
    integer gNullCtrl = -1
endglobals

function Main takes nothing returns nothing
    local player p = Player(2)
    call SetPlayerName(p, "Alpha")
    set gName = GetPlayerName(p)
    set gRace = GetHandleId(GetPlayerRace(p))
    call SetPlayerController(p, ConvertMapControl(1))
    set gCtrl = GetHandleId(GetPlayerController(p))
    set gSlot = GetHandleId(GetPlayerSlotState(p))
    call SetPlayerTeam(p, 3)
    set gTeam = GetPlayerTeam(p)
    call SetPlayerStartLocation(p, 5)
    set gStart = GetPlayerStartLocation(p)
    call SetPlayerColor(p, ConvertPlayerColor(6))
    set gColor = GetHandleId(GetPlayerColor(p))
    set gCtrlSet = GetHandleId(GetPlayerController(p))
    set gNullRace = GetHandleId(GetPlayerRace(null))
    set gNullCtrl = GetHandleId(GetPlayerController(null))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    let name = host
        .vm()
        .global("gName")
        .and_then(|v| v.as_string_id())
        .map(|id| host.vm().strings().get(id).to_string());
    assert_eq!(name.as_deref(), Some("Alpha"));
    // Race, slot state and color defaults are placeholders until the map sets them (UNSURE).
    assert_eq!(int(&host, "gRace"), 1);
    assert_eq!(int(&host, "gSlot"), 0);
    assert_eq!(int(&host, "gCtrl"), 1);
    assert_eq!(int(&host, "gCtrlSet"), 1);
    assert_eq!(int(&host, "gTeam"), 3);
    assert_eq!(int(&host, "gStart"), 5);
    assert_eq!(int(&host, "gColor"), 6);
    // A null player: race RACE_HUMAN (1) and controller MAP_CONTROL_NONE (5) (CONFIRMED).
    assert_eq!(int(&host, "gNullRace"), 1);
    assert_eq!(int(&host, "gNullCtrl"), 5);
}

#[test]
fn alliances_are_one_directional_and_typed() {
    let src = r#"
globals
    boolean gAllyPQ = true
    boolean gAllyQP = true
    boolean gEnemyQP = false
    boolean gSelfAlly = false
    boolean gSelfEnemy = true
    boolean gOtherType = true
    boolean gTypedSet = false
    boolean gNullEnemy = true
    boolean gNullAlly = true
endglobals

function Main takes nothing returns nothing
    local player p = Player(0)
    local player q = Player(1)
    call SetPlayerAlliance(p, q, ConvertAllianceType(0), true)
    set gAllyPQ = IsPlayerAlly(p, q)
    set gAllyQP = IsPlayerAlly(q, p)
    set gEnemyQP = IsPlayerEnemy(q, p)
    set gSelfAlly = IsPlayerAlly(p, p)
    set gSelfEnemy = IsPlayerEnemy(p, p)
    // Another alliance type does not change the passive one.
    call SetPlayerAlliance(p, q, ConvertAllianceType(5), true)
    set gOtherType = IsPlayerAlly(p, q)
    set gTypedSet = GetPlayerAlliance(p, q, ConvertAllianceType(5))
    set gNullEnemy = IsPlayerEnemy(null, q)
    set gNullAlly = IsPlayerAlly(p, null)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert!(boolean(&host, "gAllyPQ"));
    // The reverse relation is not set: only the row of the first player changes (CONFIRMED structure).
    assert!(!boolean(&host, "gAllyQP"));
    // So q sees p as an enemy, although p sees q as an ally.
    assert!(boolean(&host, "gEnemyQP"));
    assert!(boolean(&host, "gSelfAlly"));
    assert!(!boolean(&host, "gSelfEnemy"));
    assert!(boolean(&host, "gOtherType"));
    assert!(boolean(&host, "gTypedSet"));
    // A null player is neither an ally nor an enemy (CONFIRMED: both natives return false).
    assert!(!boolean(&host, "gNullEnemy"));
    assert!(!boolean(&host, "gNullAlly"));
}

#[test]
fn setting_a_passive_alliance_off_clears_the_bit() {
    let src = r#"
globals
    boolean gAfterOn = false
    boolean gAfterOff = true
endglobals

function Main takes nothing returns nothing
    call SetPlayerAlliance(Player(2), Player(9), ConvertAllianceType(0), true)
    set gAfterOn = IsPlayerAlly(Player(2), Player(9))
    call SetPlayerAlliance(Player(2), Player(9), ConvertAllianceType(0), false)
    set gAfterOff = IsPlayerAlly(Player(2), Player(9))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert!(boolean(&host, "gAfterOn"));
    assert!(!boolean(&host, "gAfterOff"));
}

#[test]
fn player_states_store_gold_and_lumber_times_ten_with_a_clamp() {
    let src = r#"
globals
    integer gGold = -1
    integer gNeg = -1
    integer gHuge = -1
    integer gGathered = -1
    integer gLumberGathered = -1
    integer gOut = -1
    integer gFood = -1
endglobals

function Main takes nothing returns nothing
    local player p = Player(1)
    call SetPlayerState(p, ConvertPlayerState(1), 500)
    set gGold = GetPlayerState(p, ConvertPlayerState(1))
    call SetPlayerState(p, ConvertPlayerState(1), -5)
    set gNeg = GetPlayerState(p, ConvertPlayerState(1))
    call SetPlayerState(p, ConvertPlayerState(1), 2000000)
    set gHuge = GetPlayerState(p, ConvertPlayerState(1))
    call SetPlayerState(p, ConvertPlayerState(15), 7)
    set gGathered = GetPlayerState(p, ConvertPlayerState(15))
    call SetPlayerState(p, ConvertPlayerState(16), 7)
    set gLumberGathered = GetPlayerState(p, ConvertPlayerState(16))
    // State 26 does not exist: the set does nothing and the get gives 0.
    call SetPlayerState(p, ConvertPlayerState(26), 9)
    set gOut = GetPlayerState(p, ConvertPlayerState(26))
    call SetPlayerState(p, ConvertPlayerState(4), 12)
    set gFood = GetPlayerState(p, ConvertPlayerState(4))
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gGold"), 500);
    assert_eq!(int(&host, "gNeg"), 0);
    // 2000000 * 10 is above 10000000, so the stored value is 10000000 and reads back as 1000000.
    assert_eq!(int(&host, "gHuge"), 1_000_000);
    assert_eq!(int(&host, "gGathered"), 7);
    assert_eq!(int(&host, "gLumberGathered"), 7);
    assert_eq!(int(&host, "gOut"), 0);
    assert_eq!(int(&host, "gFood"), 12);
}

#[test]
fn observer_and_slot_helpers_read_the_host_values() {
    let src = r#"
globals
    boolean gObs = true
    boolean gObsNull = true
    integer gSlot = -1
endglobals

function Main takes nothing returns nothing
    set gObs = IsPlayerObserver(Player(4))
    set gObsNull = IsPlayerObserver(null)
    set gSlot = GetHandleId(GetPlayerSlotState(Player(4)))
endfunction
"#;
    let mut host = host_for(src);
    host.game_mut().player_mut(4).expect("slot 4").observer = true;
    host.game_mut().player_mut(4).expect("slot 4").slot_state = 1;
    run(&mut host);
    assert!(boolean(&host, "gObs"));
    assert!(!boolean(&host, "gObsNull"));
    assert_eq!(int(&host, "gSlot"), 1);
}
