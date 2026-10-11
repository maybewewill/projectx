//! Game-state natives through `ScriptHost` (`natives/gamestate.rs`).
//!
//! The scripts use integer-valued reals (`I2R`), so they need no lookup tables for literals. The checks are on the
//! round trips of the values, the range checks of the indexed states, the map-flag gate of `SetGameSpeed`, the
//! events that `EndGame` and the other flow natives log, and the handle-free nature of the getters.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::gamestate::TOURNAMENT_FINISH_SOON;
use sim::natives::misc::MiscEvent;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type location extends handle
type timer extends handle

native I2R takes integer i returns real
native R2I takes real r returns integer
native Player takes integer i returns player
native Location takes real x, real y returns location

native SetGameSpeed takes integer s returns nothing
native GetGameSpeed takes nothing returns integer
native SetGameDifficulty takes integer d returns nothing
native GetGameDifficulty takes nothing returns integer
native SetDefaultDifficulty takes integer d returns nothing
native GetDefaultDifficulty takes nothing returns integer
native GetAIDifficulty takes player p returns integer
native SetGamePlacement takes integer p returns nothing
native GetGamePlacement takes nothing returns integer
native GetGameTypeSelected takes nothing returns integer
native IsGameTypeSupported takes integer t returns boolean
native SetGameTypeSupported takes integer t, boolean flag returns nothing
native IsMapFlagSet takes integer f returns boolean
native SetMapFlag takes integer f, boolean flag returns nothing
native GetResourceDensity takes nothing returns integer
native SetResourceDensity takes integer d returns nothing
native GetCreatureDensity takes nothing returns integer
native SetCreatureDensity takes integer d returns nothing
native GetFloatGameState takes integer s returns real
native SetFloatGameState takes integer s, real v returns nothing
native GetIntegerGameState takes integer s returns integer
native SetIntegerGameState takes integer s, integer v returns nothing
native SetMapName takes string n returns nothing
native SetMapDescription takes string n returns nothing
native GetSaveBasicFilename takes nothing returns string
native CopySaveGame takes string a, string b returns boolean
native EndGame takes boolean victory returns nothing
native RestartGame takes boolean keep returns nothing
native ReloadGame takes nothing returns nothing
native PauseGame takes boolean flag returns nothing
native DisableRestartMission takes boolean flag returns nothing
native SuspendTimeOfDay takes boolean flag returns nothing
native IsNoVictoryCheat takes nothing returns boolean
native IsNoDefeatCheat takes nothing returns boolean
native VersionGet takes nothing returns integer
native VersionCompatible takes integer v returns boolean
native VersionSupported takes integer v returns boolean
native GetTournamentFinishSoonTimeRemaining takes nothing returns real
native GetTournamentFinishNowRule takes nothing returns integer
native GetTournamentFinishNowPlayer takes nothing returns player
native GetTournamentScore takes player p returns integer
native SetCampaignAvailable takes integer i, boolean flag returns nothing
native SetMissionAvailable takes integer c, integer m, boolean flag returns nothing
native SetTutorialCleared takes boolean flag returns nothing
native ForceCampaignSelectScreen takes nothing returns nothing
native CreateTimer takes nothing returns timer
native TimerStart takes handle t, real timeout, boolean periodic, code handlerFunc returns nothing
native GetHandleId takes handle h returns integer
"#;

/// The permutation table is not needed by these natives, so the host runs without it.
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
boolean ok = false
boolean ok2 = false
real r1 = 0.0
real r2 = 0.0
endglobals
"#;

#[test]
fn speed_and_difficulty_round_trip_and_the_speed_lock() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set a = GetGameSpeed()
    call SetGameSpeed(3)
    set b = GetGameSpeed()
    call SetMapFlag(16384, true)
    call SetGameSpeed(0)
    set c = GetGameSpeed()
    set ok = IsMapFlagSet(16384)
    call SetMapFlag(16384, false)
    call SetGameSpeed(4)
    set ok2 = GetGameSpeed() == 4
    call SetGameDifficulty(2)
    call SetDefaultDifficulty(3)
endfunction
"#
    ));
    run(&mut host);
    // The default is 2 (normal, UNSURE), then the set value is read back.
    assert_eq!(int(&host, "a"), 2);
    assert_eq!(int(&host, "b"), 3);
    // While the flag is set, SetGameSpeed is ignored.
    assert_eq!(int(&host, "c"), 3);
    assert!(boolean(&host, "ok"));
    assert!(boolean(&host, "ok2"));
    assert_eq!(host.game().game_state().difficulty, 2);
    assert_eq!(host.game().game_state().default_difficulty, 3);
}

#[test]
fn game_type_map_flags_and_densities_round_trip() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call SetGameTypeSupported(2, true)
    call SetGameTypeSupported(4, true)
    call SetGameTypeSupported(4, false)
    set ok = IsGameTypeSupported(2)
    set ok2 = IsGameTypeSupported(4)
    call SetGamePlacement(5)
    set a = GetGamePlacement()
    call SetResourceDensity(2)
    call SetCreatureDensity(3)
    set b = GetResourceDensity()
    set c = GetCreatureDensity()
    call SetMapFlag(1, true)
    call SetMapFlag(2, true)
    call SetMapFlag(1, false)
endfunction
"#
    ));
    run(&mut host);
    assert!(boolean(&host, "ok"));
    assert!(!boolean(&host, "ok2"));
    assert_eq!(int(&host, "a"), 5);
    assert_eq!(int(&host, "b"), 2);
    assert_eq!(int(&host, "c"), 3);
    assert_eq!(host.game().game_state().map_flags, 2);
}

#[test]
fn indexed_game_states_only_accept_their_indices() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call SetIntegerGameState(0, 7)
    call SetIntegerGameState(1, 9)
    call SetIntegerGameState(2, 99)
    set a = GetIntegerGameState(0)
    set b = GetIntegerGameState(1)
    set c = GetIntegerGameState(2)
    call SetFloatGameState(2, I2R(5))
    set r1 = GetFloatGameState(2)
    call SetFloatGameState(1, I2R(8))
    set r2 = GetFloatGameState(1)
endfunction
"#
    ));
    run(&mut host);
    assert_eq!(int(&host, "a"), 7);
    assert_eq!(int(&host, "b"), 9);
    // Integer index 2 is out of range: it reads 0 and the store is ignored.
    assert_eq!(int(&host, "c"), 0);
    assert_eq!(real(&host, "r1"), 5.0);
    // Float index 1 is out of range: it reads 0.0.
    assert_eq!(real(&host, "r2"), 0.0);
}

#[test]
fn end_game_and_restart_log_their_requests() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    call SetMapName("Test map")
    call EndGame(true)
    call RestartGame(false)
    call ReloadGame()
    call PauseGame(true)
    call SuspendTimeOfDay(true)
    call DisableRestartMission(true)
    call ForceCampaignSelectScreen()
    set ok = CopySaveGame("a", "b")
endfunction
"#
    ));
    run(&mut host);
    let state = host.game().game_state().clone();
    assert_eq!(state.map_name, "Test map");
    assert_eq!(state.ended, Some(true));
    assert!(state.paused);
    assert!(state.time_of_day_suspended);
    assert!(state.restart_mission_disabled);
    assert!(!boolean(&host, "ok"), "CopySaveGame has no save files");
    let events = host.game().misc_events().to_vec();
    assert_eq!(
        events,
        vec![
            MiscEvent::EndGame { result: true },
            MiscEvent::RestartGame {
                keep_settings: false
            },
            MiscEvent::ReloadGame,
            MiscEvent::PauseGame { paused: true },
            MiscEvent::ForceCampaignSelect,
            MiscEvent::CopySaveGame,
        ]
    );
}

#[test]
fn version_and_cheats_have_the_documented_values() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set a = VersionGet()
    set ok = VersionCompatible(1)
    set ok2 = VersionCompatible(7)
endfunction
"#
    ));
    run(&mut host);
    assert_eq!(int(&host, "a"), 1);
    assert!(boolean(&host, "ok"));
    assert!(!boolean(&host, "ok2"));
    // The host can set the cheat flags, and the natives read them back.
    host.game_mut().game_state_mut().no_defeat_cheat = true;
    assert!(!host.game().game_state().no_victory_cheat);
    assert!(host.game().game_state().no_defeat_cheat);
}

#[test]
fn tournament_values_are_zero_until_the_host_sets_a_phase() {
    let mut host = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set r1 = GetTournamentFinishSoonTimeRemaining()
    set a = GetTournamentFinishNowRule()
    set ok = GetTournamentFinishNowPlayer() == null
    set b = GetTournamentScore(Player(2))
endfunction
"#
    ));
    run(&mut host);
    assert_eq!(real(&host, "r1"), 0.0);
    assert_eq!(int(&host, "a"), 0);
    assert!(boolean(&host, "ok"));
    assert_eq!(int(&host, "b"), 0);

    let mut host2 = host_for(&format!(
        r#"{GLOBALS}
function main takes nothing returns nothing
    set r1 = GetTournamentFinishSoonTimeRemaining()
endfunction
"#
    ));
    {
        let mut g = host2.game_mut();
        let t = &mut g.game_state_mut().tournament;
        t.phase = TOURNAMENT_FINISH_SOON;
        t.finish_soon_remaining = F32::from_bits(0x41A0_0000);
    }
    run(&mut host2);
    assert_eq!(real(&host2, "r1"), 20.0);
}

#[test]
fn campaign_setters_do_not_create_handles() {
    // Interleaved with a timer: the campaign natives take no handle, so the next timer keeps the next id.
    let mut host = host_for(&format!(
        r#"{GLOBALS}
globals
timer t1
timer t2
endglobals
function main takes nothing returns nothing
    set t1 = CreateTimer()
    call SetCampaignAvailable(1, true)
    call SetMissionAvailable(1, 2, false)
    call SetTutorialCleared(true)
    set t2 = CreateTimer()
    set a = GetHandleId(t1)
    set b = GetHandleId(t2)
endfunction
"#
    ));
    run(&mut host);
    assert_eq!(int(&host, "b"), int(&host, "a") + 1);
    assert!(host.game().game_state().campaign.len() == 3);
}
