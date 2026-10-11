//! The AI host in the engine: a map starts an AI with `StartMeleeAI`, the AI script runs, and
//! `CommandAI` from the map reaches it. Synthetic scripts only; nothing from the game is used unless
//! `GAME_DLL` names a 1.26a `Game.dll` (then the same checks run with the soft-float tables).
//! Expected values follow the rules in `sim::ai` and `engine`, not the original.

mod common;

use common::{map_file, w3i, PlayerRec};
use engine::{Engine, Inputs};
use jass::vm::Value;
use sim::ai::{AiArg, AiEvent, StartKind};

const BOUNDS: [f32; 8] = [-1000.0, 1000.0, -800.0, 800.0, 0.0, 0.0, 0.0, 0.0];

/// The map's `common.j` (also the AI prelude): the types and the main-VM natives the test scripts use.
const COMMON_J: &str = r#"
type player extends handle
type unit extends handle

native Player takes integer i returns player
native TriggerSleepAction takes real timeout returns nothing
native StartMeleeAI takes player whichPlayer, string scriptName returns nothing
native CommandAI takes player whichPlayer, integer command, integer data returns nothing
"#;

/// `common.ai`: the AI natives that the AI scripts call. Natives with no implementation in the model
/// (`GetEnemyBase`) are declared too, so the scripts still compile.
const COMMON_AI: &str = r#"
native CommandsWaiting takes nothing returns integer
native GetLastCommand takes nothing returns integer
native GetLastData takes nothing returns integer
native PopLastCommand takes nothing returns nothing
native Sleep takes real duration returns nothing
native AttackMoveXY takes integer x, integer y returns nothing
native GetEnemyBase takes nothing returns unit
"#;

/// `test.ai`: waits for a command from the map, records it, pops it, then logs an attack-move
/// (a command the model records) and asks for the enemy base (not implemented in the model).
const TEST_AI: &str = r#"
globals
    integer gMain = 0
    integer gMe = 0
    integer gPolls = 0
    integer gCmd = -99
    integer gData = -99
    integer gWaiting = -99
    integer gLeft = -99
    integer gBase = 0
endglobals

function Me takes nothing returns player
    return Player(1)
endfunction

function main takes nothing returns nothing
    set gMain = 1
    if Me() != null then
        set gMe = 1
    endif
    loop
        exitwhen CommandsWaiting() > 0
        set gPolls = gPolls + 1
        call Sleep(0.1)
    endloop
    set gWaiting = CommandsWaiting()
    set gCmd = GetLastCommand()
    set gData = GetLastData()
    call PopLastCommand()
    set gLeft = CommandsWaiting()
    call AttackMoveXY(10, 20)
    if GetEnemyBase() == null then
        set gBase = 1
    endif
endfunction
"#;

/// A map whose `main` is `body` (statements of the map's `main`).
fn map_with_main(body: &str) -> Vec<u8> {
    let players = [
        PlayerRec {
            number: 0,
            player_type: 1,
            race: 4,
            name: "Alice",
            start: (-100.0, 50.0),
        },
        PlayerRec {
            number: 1,
            player_type: 2,
            race: 2,
            name: "Computer",
            start: (300.0, -20.0),
        },
    ];
    let info = w3i("AI Test", BOUNDS, &players, &[(0b11, "Force")]);
    let war3map_j = format!(
        "globals\n    integer gDone = 0\nendglobals\n\nfunction main takes nothing returns nothing\n{body}\nendfunction\n"
    );
    map_file(info, &war3map_j)
}

/// The map of the main AI test: starts `test.ai` for player 1 (computer), sleeps, then commands it.
fn command_map() -> Vec<u8> {
    map_with_main(
        r#"    call StartMeleeAI(Player(1), "test.ai")
    call TriggerSleepAction(0.3)
    call CommandAI(Player(1), 7, 42)
    set gDone = 1"#,
    )
}

fn inputs(map: Vec<u8>, ai_scripts: &[(&str, &str)]) -> Inputs {
    Inputs {
        map,
        game_dll: None,
        common_j: COMMON_J.to_string(),
        blizzard_j: String::new(),
        rng_seed: 0,
        ai_common: Some(COMMON_AI.to_string()),
        ai_scripts: ai_scripts
            .iter()
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect(),
        game_archives: Vec::new(),
    }
}

/// The game's tables, if `GAME_DLL` names a 1.26a `Game.dll`.
fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn boot(mut inputs: Inputs, with_dll: bool) -> Engine {
    if with_dll {
        inputs.game_dll = game_dll();
    }
    Engine::boot(inputs).unwrap_or_else(|e| panic!("boot failed: {e}"))
}

fn run(engine: &mut Engine, steps: usize) {
    for _ in 0..steps {
        engine.step();
    }
}

fn ai_global_int(engine: &Engine, player: u32, name: &str) -> i32 {
    let vm = engine
        .ai()
        .vm(player)
        .unwrap_or_else(|| panic!("no AI VM for player {player}"));
    match vm.global(name) {
        Some(Value::Integer(v)) => v,
        other => panic!("AI global {name} is {other:?}, not an integer"),
    }
}

/// The dispatch checks, with or without the soft-float tables.
fn check_command_reaches_the_ai(with_dll: bool) {
    let mut engine = boot(inputs(command_map(), &[("test.ai", TEST_AI)]), with_dll);
    run(&mut engine, 50);

    let report = engine.report();
    assert_eq!(report.ai.started, vec![1], "{report}");
    assert!(
        report.ai.events.iter().any(|e| matches!(
            e,
            AiEvent::Started { player: 1, script, kind: StartKind::Melee } if script == "test.ai"
        )),
        "{:?}",
        report.ai.events
    );

    // The AI's main runs: it waits for the map's command, then reads it.
    assert_eq!(ai_global_int(&engine, 1, "gMain"), 1);
    assert_eq!(
        ai_global_int(&engine, 1, "gMe"),
        1,
        "common.j's Player is visible to the AI"
    );
    assert!(
        ai_global_int(&engine, 1, "gPolls") >= 1,
        "the AI waited for the command"
    );
    assert_eq!(
        ai_global_int(&engine, 1, "gCmd"),
        7,
        "CommandAI(7, 42) reached the AI"
    );
    assert_eq!(ai_global_int(&engine, 1, "gData"), 42);
    assert_eq!(ai_global_int(&engine, 1, "gWaiting"), 1);
    assert_eq!(
        ai_global_int(&engine, 1, "gLeft"),
        0,
        "PopLastCommand emptied the stack"
    );

    // The AI's own calls: the logged attack-move, and the enemy base (not implemented).
    let commands = engine.ai().shared().borrow().commands.clone();
    let attack = commands
        .iter()
        .find(|c| c.name == "AttackMoveXY")
        .expect("AttackMoveXY logged");
    assert_eq!(attack.player, 1);
    assert_eq!(attack.args, vec![AiArg::Int(10), AiArg::Int(20)]);
    assert_eq!(
        report.ai.unimplemented,
        vec![("GetEnemyBase".to_string(), 1)]
    );
    assert_eq!(
        ai_global_int(&engine, 1, "gBase"),
        1,
        "the unimplemented call returns null"
    );
    assert!(report.ai.failures.is_empty(), "{:?}", report.ai.failures);

    let text = report.to_string();
    assert!(text.contains("AI players started (1): 1"), "{text}");
    assert!(text.contains("GetEnemyBase x1"), "{text}");
}

#[test]
fn start_melee_ai_runs_the_script_and_command_ai_reaches_it() {
    check_command_reaches_the_ai(false);
}

#[test]
fn start_melee_ai_with_game_dll_tables() {
    if game_dll().is_none() {
        eprintln!("GAME_DLL not set: skipping the AI test with the Game.dll tables");
        return;
    }
    check_command_reaches_the_ai(true);
}

#[test]
fn ai_steps_are_deterministic() {
    let run_once = |with_dll: bool| {
        let mut engine = boot(inputs(command_map(), &[("test.ai", TEST_AI)]), with_dll);
        run(&mut engine, 60);
        let commands = engine.ai().shared().borrow().commands.clone();
        let globals: Vec<i32> = ["gPolls", "gCmd", "gData", "gLeft"]
            .iter()
            .map(|g| ai_global_int(&engine, 1, g))
            .collect();
        (engine.report(), commands, globals)
    };
    let first = run_once(false);
    assert_eq!(first, run_once(false));
    assert!(!first.1.is_empty(), "the AI logged calls");

    if game_dll().is_some() {
        let with_dll = run_once(true);
        assert_eq!(with_dll, run_once(true));
    }
}

#[test]
fn missing_script_is_a_build_failure_and_the_map_goes_on() {
    let map = map_with_main(
        r#"    call StartMeleeAI(Player(1), "nothere.ai")
    call TriggerSleepAction(0.2)
    set gDone = 1"#,
    );
    let mut engine = boot(inputs(map, &[("test.ai", TEST_AI)]), false);
    run(&mut engine, 5);
    let report = engine.report();
    assert!(report.ai.started.is_empty(), "{report}");
    assert!(
        report.ai.events.iter().any(|e| matches!(
            e,
            AiEvent::BuildFailed { player: 1, script, .. } if script == "nothere.ai"
        )),
        "{:?}",
        report.ai.events
    );
    assert_eq!(engine.host().vm().global("gDone"), Some(Value::Integer(1)));
}

#[test]
fn script_that_needs_common_ai_fails_without_it() {
    let mut inputs = inputs(command_map(), &[("test.ai", TEST_AI)]);
    inputs.ai_common = None;
    let mut engine = boot(inputs, false);
    run(&mut engine, 5);
    let report = engine.report();
    assert!(report.ai.started.is_empty(), "{report}");
    assert!(
        report
            .ai
            .events
            .iter()
            .any(|e| matches!(e, AiEvent::BuildFailed { player: 1, .. })),
        "{:?}",
        report.ai.events
    );
    assert!(report.notes.iter().any(|n| n.contains("no common.ai")));
}

#[test]
fn w3boot_takes_ai_script_files() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("w3boot-ai-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let map_path = dir.join("ai.w3x");
    std::fs::write(&map_path, command_map()).expect("write map");
    let common_j = dir.join("common.j");
    std::fs::write(&common_j, COMMON_J).expect("write common.j");
    let blizzard_j = dir.join("blizzard.j");
    std::fs::write(&blizzard_j, "").expect("write blizzard.j");
    let common_ai = dir.join("common.ai");
    std::fs::write(&common_ai, COMMON_AI).expect("write common.ai");
    let test_ai = dir.join("test.ai");
    std::fs::write(&test_ai, TEST_AI).expect("write test.ai");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_w3boot"))
        .arg("--common")
        .arg(&common_j)
        .arg("--blizzard")
        .arg(&blizzard_j)
        .arg("--ai-common")
        .arg(&common_ai)
        .arg("--ai-script")
        .arg(format!("test.ai={}", test_ai.display()))
        .arg("--steps")
        .arg("50")
        .arg(&map_path)
        .output()
        .expect("run w3boot");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("AI players started (1): 1"), "{stdout}");
    assert!(stdout.contains("GetEnemyBase x1"), "{stdout}");
    assert!(stdout.contains("AI: "), "{stdout}");
}

#[test]
fn w3boot_rejects_a_bad_ai_script_argument() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_w3boot"))
        .arg("--common")
        .arg("x")
        .arg("--blizzard")
        .arg("y")
        .arg("--ai-script")
        .arg("no-equals-sign")
        .arg("map.w3x")
        .output()
        .expect("run w3boot");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--ai-script"));
}
