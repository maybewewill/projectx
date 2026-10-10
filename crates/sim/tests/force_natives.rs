//! Force natives through `ScriptHost` (`natives/force.rs`). The expected orders and sets follow the
//! notes of that module: `ForForce` visits slots 0 to 15 in ascending order and re-reads the membership
//! before each one, the `ForceEnum*` natives clear the force and visit slots 0 to 11 only, and
//! `GetEnumPlayer` / `GetFilterPlayer` are scopes on a stack. Integers and handles only, so no lookup
//! tables are needed; with `GAME_DLL` set the source is parsed with the original's tables too.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::{ScriptHost, HANDLE_BASE};
use softfloat::Tables;

const PRELUDE: &str = r#"
type player extends handle
type force extends handle
type boolexpr extends handle
type filterfunc extends boolexpr
type alliancetype extends handle

native Player takes integer i returns player
native CreateForce takes nothing returns force
native DestroyForce takes force f returns nothing
native ForceAddPlayer takes force f, player p returns nothing
native ForceRemovePlayer takes force f, player p returns nothing
native ForceClear takes force f returns nothing
native IsPlayerInForce takes player p, force f returns boolean
native ForceEnumPlayers takes force f, boolexpr filter returns nothing
native ForceEnumPlayersCounted takes force f, boolexpr filter, integer count returns nothing
native ForceEnumAllies takes force f, player p, boolexpr filter returns nothing
native ForceEnumEnemies takes force f, player p, boolexpr filter returns nothing
native ForForce takes force f, code c returns nothing
native GetEnumPlayer takes nothing returns player
native GetFilterPlayer takes nothing returns player
native Filter takes code c returns filterfunc
native SetPlayerAlliance takes player p, player q, alliancetype t, boolean flag returns nothing
native ConvertAllianceType takes integer i returns alliancetype
native GetPlayerId takes player p returns integer
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

fn run(host: &mut ScriptHost) {
    host.start("Main", &[]).unwrap();
    host.step();
}

/// Marks slots `0..n` as playing (a host setup; the slot state is not set by any native).
fn activate(host: &ScriptHost, slots: std::ops::Range<u32>) {
    let mut gm = host.game_mut();
    for i in slots {
        gm.player_mut(i).expect("slot").slot_state = 1;
    }
}

/// The members of force global `name` as the list of slot indices.
fn members(host: &ScriptHost, name: &str) -> Vec<u32> {
    let f = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a force handle"));
    let gm = host.game();
    let key = gm.force_of_handle(f).expect("live force");
    let bits = gm.force(key).members;
    (0..16).filter(|i| bits & (1 << i) != 0).collect()
}

#[test]
fn forforce_visits_members_in_ascending_order() {
    let src = r#"
globals
    force gF = null
    integer gSeq = 0
endglobals

function Visit takes nothing returns nothing
    set gSeq = gSeq * 100 + GetPlayerId(GetEnumPlayer())
endfunction

function Main takes nothing returns nothing
    set gF = CreateForce()
    call ForceAddPlayer(gF, Player(9))
    call ForceAddPlayer(gF, Player(2))
    call ForceAddPlayer(gF, Player(5))
    call ForForce(gF, function Visit)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    // 2, then 5, then 9: 2 * 10000 + 5 * 100 + 9.
    assert_eq!(int(&host, "gSeq"), 20509);
}

#[test]
fn forforce_rereads_membership_and_visits_empty_slots() {
    // The callback for slot 2 adds slot 3 and removes slot 5, so the rest of the loop sees that.
    let src = r#"
globals
    force gF = null
    integer gSeq = 0
endglobals

function Visit takes nothing returns nothing
    local integer id = GetPlayerId(GetEnumPlayer())
    set gSeq = gSeq * 100 + id
    if id == 2 then
        call ForceAddPlayer(gF, Player(3))
        call ForceRemovePlayer(gF, Player(5))
    endif
endfunction

function Main takes nothing returns nothing
    set gF = CreateForce()
    call ForceAddPlayer(gF, Player(2))
    call ForceAddPlayer(gF, Player(5))
    call ForForce(gF, function Visit)
endfunction
"#;
    let mut host = host_for(src);
    // The slots are empty (slot state 0): ForForce does not check the slot state, so they are visited.
    run(&mut host);
    assert_eq!(int(&host, "gSeq"), 203);
}

#[test]
fn get_enum_player_is_null_outside_and_nests_as_a_stack() {
    let src = r#"
globals
    force gOuter = null
    force gInner = null
    integer gOutside = -1
    integer gInside = -1
    integer gAfterInner = -1
    integer gInnerSeen = -1
endglobals

function Inner takes nothing returns nothing
    set gInnerSeen = GetPlayerId(GetEnumPlayer())
endfunction

function Outer takes nothing returns nothing
    local integer before = GetPlayerId(GetEnumPlayer())
    call ForForce(gInner, function Inner)
    set gAfterInner = GetPlayerId(GetEnumPlayer())
    set gInside = before
endfunction

function Main takes nothing returns nothing
    set gOutside = GetHandleId(GetEnumPlayer())
    set gOuter = CreateForce()
    set gInner = CreateForce()
    call ForceAddPlayer(gOuter, Player(6))
    call ForceAddPlayer(gInner, Player(1))
    call ForForce(gOuter, function Outer)
    set gOutside = GetHandleId(GetEnumPlayer())
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gOutside"), 0);
    assert_eq!(int(&host, "gInside"), 6);
    assert_eq!(int(&host, "gInnerSeen"), 1);
    // After the inner loop ends, the outer value is back.
    assert_eq!(int(&host, "gAfterInner"), 6);
}

#[test]
fn force_enum_players_clears_then_filters_by_slot_state_and_filter() {
    let src = r#"
globals
    force gF = null
    boolean gHad15 = true
    boolean gIn15 = true
    boolean gIn2 = false
    boolean gIn3 = false
    boolean gIn5 = false
    boolean gIn6 = false
    boolean gIn0 = true
endglobals

function Big takes nothing returns boolean
    return GetPlayerId(GetFilterPlayer()) >= 2
endfunction

function Main takes nothing returns nothing
    set gF = CreateForce()
    call ForceAddPlayer(gF, Player(15))
    set gHad15 = IsPlayerInForce(Player(15), gF)
    call ForceEnumPlayers(gF, Filter(function Big))
    set gIn15 = IsPlayerInForce(Player(15), gF)
    set gIn0 = IsPlayerInForce(Player(0), gF)
    set gIn2 = IsPlayerInForce(Player(2), gF)
    set gIn3 = IsPlayerInForce(Player(3), gF)
    set gIn5 = IsPlayerInForce(Player(5), gF)
    set gIn6 = IsPlayerInForce(Player(6), gF)
endfunction
"#;
    let mut host = host_for(src);
    activate(&host, 0..8);
    run(&mut host);
    assert!(boolean(&host, "gHad15"));
    // The force was cleared first, so slot 15 is gone (it is not active and not visited).
    assert!(!boolean(&host, "gIn15"));
    // Slots 0 and 1 are active but fail the filter; 2 to 7 pass.
    assert!(!boolean(&host, "gIn0"));
    assert!(boolean(&host, "gIn2"));
    assert!(boolean(&host, "gIn3"));
    assert!(boolean(&host, "gIn5"));
    assert!(boolean(&host, "gIn6"));
    assert_eq!(members(&host, "gF"), vec![2, 3, 4, 5, 6, 7]);
}

#[test]
fn force_enum_players_skips_empty_slots_and_never_visits_slots_12_to_15() {
    let src = r#"
globals
    force gF = null
endglobals

function Main takes nothing returns nothing
    set gF = CreateForce()
    call ForceEnumPlayers(gF, null)
endfunction
"#;
    let mut host = host_for(src);
    // Slots 0 to 2 and 12 to 15 are active. Only 0 to 2 are visited (LIKELY, see force.rs).
    activate(&host, 0..3);
    activate(&host, 12..16);
    run(&mut host);
    assert_eq!(members(&host, "gF"), vec![0, 1, 2]);
}

#[test]
fn force_enum_players_counted_stops_at_the_limit_and_a_negative_count_is_unlimited() {
    let src = r#"
globals
    force gA = null
    force gB = null
    force gC = null
endglobals

function Main takes nothing returns nothing
    set gA = CreateForce()
    set gB = CreateForce()
    set gC = CreateForce()
    call ForceEnumPlayersCounted(gA, null, 2)
    call ForceEnumPlayersCounted(gB, null, 0)
    call ForceEnumPlayersCounted(gC, null, -1)
endfunction
"#;
    let mut host = host_for(src);
    activate(&host, 0..6);
    run(&mut host);
    assert_eq!(members(&host, "gA"), vec![0, 1]);
    assert_eq!(members(&host, "gB"), Vec::<u32>::new());
    assert_eq!(members(&host, "gC"), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn allies_and_enemies_use_the_candidates_own_passive_row() {
    // Player 3 lists 4 as an ally. Player 4 lists 1, but that does not make 1 an ally of 4.
    let src = r#"
globals
    force gAllies = null
    force gEnemies = null
    force gNobody = null
endglobals

function Main takes nothing returns nothing
    set gAllies = CreateForce()
    set gEnemies = CreateForce()
    set gNobody = CreateForce()
    call SetPlayerAlliance(Player(3), Player(4), ConvertAllianceType(0), true)
    call SetPlayerAlliance(Player(4), Player(1), ConvertAllianceType(0), true)
    call ForceEnumAllies(gAllies, Player(4), null)
    call ForceEnumEnemies(gEnemies, Player(4), null)
    call ForceEnumAllies(gNobody, null, null)
endfunction
"#;
    let mut host = host_for(src);
    activate(&host, 0..6);
    run(&mut host);
    // Candidate 3 has 4 in its row, and 4 is passively allied with itself (default).
    assert_eq!(members(&host, "gAllies"), vec![3, 4]);
    // Enemies: active candidates whose row does not contain 4, that is 0, 1, 2 and 5.
    assert_eq!(members(&host, "gEnemies"), vec![0, 1, 2, 5]);
    // A null source adds nobody.
    assert_eq!(members(&host, "gNobody"), Vec::<u32>::new());
}

#[test]
fn enum_filter_sees_the_candidate_and_keeps_the_outer_enum_player() {
    let src = r#"
globals
    force gF = null
    force gG = null
    integer gOuterSeen = -1
    integer gFilterCalls = 0
endglobals

function Keep takes nothing returns boolean
    set gFilterCalls = gFilterCalls + 1
    if GetPlayerId(GetEnumPlayer()) == 0 then
        set gOuterSeen = 0
    endif
    return GetPlayerId(GetFilterPlayer()) != 1
endfunction

function Visit takes nothing returns nothing
    call ForceEnumPlayersCounted(gG, Filter(function Keep), 10)
endfunction

function Main takes nothing returns nothing
    set gF = CreateForce()
    set gG = CreateForce()
    call ForceAddPlayer(gF, Player(0))
    call ForForce(gF, function Visit)
endfunction
"#;
    let mut host = host_for(src);
    activate(&host, 0..3);
    run(&mut host);
    // The filter ran for the three active candidates (0, 1, 2), and GetEnumPlayer inside it was the
    // ForForce player 0 all along.
    assert_eq!(int(&host, "gFilterCalls"), 3);
    assert_eq!(int(&host, "gOuterSeen"), 0);
    assert_eq!(members(&host, "gG"), vec![0, 2]);
}

#[test]
fn force_handles_are_reused_in_lifo_order_and_membership_helpers_work() {
    let src = r#"
globals
    force gA = null
    force gB = null
    integer gIdA = -1
    integer gIdB = -1
    boolean gInA = true
    boolean gInB = true
endglobals

function Main takes nothing returns nothing
    set gA = CreateForce()
    set gIdA = GetHandleId(gA)
    call DestroyForce(gA)
    set gB = CreateForce()
    set gIdB = GetHandleId(gB)
    call ForceAddPlayer(gB, Player(4))
    call ForceRemovePlayer(gB, Player(4))
    set gInB = IsPlayerInForce(Player(4), gB)
    call ForceAddPlayer(gB, Player(8))
    call ForceClear(gB)
    set gInA = IsPlayerInForce(Player(8), gB)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gIdA") as u32, HANDLE_BASE + 1);
    // The freed id goes back on the stack and the next force takes it (CONFIRMED in handles.rs).
    assert_eq!(int(&host, "gIdB"), int(&host, "gIdA"));
    assert!(!boolean(&host, "gInB"));
    assert!(!boolean(&host, "gInA"));
}

#[test]
fn null_and_dead_forces_do_nothing() {
    let src = r#"
globals
    force gF = null
    integer gSeq = 0
    boolean gAfterDestroy = true
endglobals

function Visit takes nothing returns nothing
    set gSeq = gSeq + 1
endfunction

function Main takes nothing returns nothing
    call ForForce(null, function Visit)
    call ForceAddPlayer(null, Player(1))
    set gF = CreateForce()
    call ForceAddPlayer(gF, Player(1))
    call DestroyForce(gF)
    call ForForce(gF, function Visit)
    set gAfterDestroy = IsPlayerInForce(Player(1), gF)
endfunction
"#;
    let mut host = host_for(src);
    run(&mut host);
    assert_eq!(int(&host, "gSeq"), 0);
    assert!(!boolean(&host, "gAfterDestroy"));
}

#[test]
fn an_enumeration_sees_alliance_changes_made_by_the_filter_only_through_later_calls() {
    // A filter may change the alliance state. The relation of candidate 1 is read after its own filter
    // call (the order of the original, LIKELY), so the change is seen for candidate 1.
    let src = r#"
globals
    force gF = null
    force gOut = null
    boolean gNow = false
endglobals

function Keep takes nothing returns boolean
    call SetPlayerAlliance(Player(1), Player(2), ConvertAllianceType(0), true)
    return true
endfunction

function Main takes nothing returns nothing
    set gOut = CreateForce()
    call ForceEnumAllies(gOut, Player(2), Filter(function Keep))
    set gNow = IsPlayerInForce(Player(1), gOut)
endfunction
"#;
    let mut host = host_for(src);
    activate(&host, 0..3);
    run(&mut host);
    assert!(boolean(&host, "gNow"));
}
