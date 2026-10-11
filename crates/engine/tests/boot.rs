//! Booting synthetic maps: map setup, config and main, timers after steps, missing natives, the
//! script archives and the `w3boot` binary. Expected values come from the rules in `src/lib.rs`
//! and `src/map.rs`, not from the original.

mod common;

use common::{map_file, w3i, PlayerRec, BLIZZARD_J, COMMON_J, WAR3MAP_J};
use engine::{Engine, Error, Inputs};
use jass::vm::Value;
use softfloat::F32;

const BOUNDS: [f32; 8] = [-1024.0, 1024.0, -896.0, 896.0, 0.0, 0.0, 0.0, 0.0];

fn players() -> Vec<PlayerRec> {
    vec![
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
        PlayerRec {
            number: 2,
            player_type: 0,
            race: 1,
            name: "",
            start: (0.0, 0.0),
        },
    ]
}

/// A map with three slots (human, computer, empty) and two forces: `{0, 1}` and `{2}`.
fn synthetic_map(war3map_j: &str) -> Vec<u8> {
    let info = w3i(
        "Synthetic Test",
        BOUNDS,
        &players(),
        &[(0b011, "Force A"), (0b100, "Force B")],
    );
    map_file(info, war3map_j)
}

fn inputs(map: Vec<u8>) -> Inputs {
    Inputs {
        map,
        game_dll: None,
        common_j: COMMON_J.to_string(),
        blizzard_j: BLIZZARD_J.to_string(),
        rng_seed: 0,
        ai_common: None,
        ai_scripts: Vec::new(),
        game_archives: Vec::new(),
        check_natives: false,
    }
}

/// The game's tables, if `GAME_DLL` names a 1.26a `Game.dll`.
fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn boot(map: Vec<u8>) -> Engine {
    match Engine::boot(inputs(map)) {
        Ok(e) => e,
        Err(e) => panic!("boot failed: {e}"),
    }
}

fn global_int(engine: &Engine, name: &str) -> i32 {
    match engine.host().vm().global(name) {
        Some(Value::Integer(v)) => v,
        other => panic!("global {name} is {other:?}, not an integer"),
    }
}

#[test]
fn synthetic_map_boots_and_applies_w3i() {
    let engine = boot(synthetic_map(WAR3MAP_J));
    let report = engine.report();

    assert_eq!(report.map_name, "Synthetic Test");
    assert_eq!(report.author, "engine test");
    assert_eq!(report.players.len(), 3);

    let alice = &report.players[0];
    assert_eq!(alice.name, "Alice");
    assert_eq!(alice.race, 4, "raw w3i race is kept (no mapping, UNSURE)");
    assert_eq!(alice.controller, engine::map::CONTROL_USER);
    assert_eq!(alice.slot_state, 1);
    assert_eq!(alice.team, 0);
    assert_eq!(alice.start, (-100.0, 50.0));

    let computer = &report.players[1];
    assert_eq!(computer.controller, engine::map::CONTROL_COMPUTER);
    assert_eq!(computer.team, 0, "same force as Alice");

    let empty = &report.players[2];
    assert_eq!(empty.controller, engine::map::CONTROL_NONE);
    assert_eq!(empty.slot_state, 0);
    assert_eq!(empty.team, 1, "force B");

    let game = engine.host().game();
    assert_eq!(game.local_player(), 0, "first human slot");
    assert_eq!(game.player(2).start_location, 2);

    let playable = game.playable().expect("playable area set at boot");
    assert_eq!(playable.min_x.to_f32(), -1024.0);
    assert_eq!(playable.max_x.to_f32(), 1024.0);
    assert_eq!(playable.min_y.to_f32(), -896.0);
    assert_eq!(playable.max_y.to_f32(), 896.0);
    assert_eq!(playable.min_x, F32::from_bits((-1024.0f32).to_bits()));
}

#[test]
fn config_runs_at_boot_and_main_on_the_first_step() {
    let mut engine = boot(synthetic_map(WAR3MAP_J));
    assert_eq!(global_int(&engine, "gConfig"), 1, "config ran at boot");
    assert_eq!(global_int(&engine, "gMain"), 0, "main is queued, not run");
    assert_eq!(engine.report().config, "ok");

    engine.step();
    assert_eq!(
        global_int(&engine, "gMain"),
        1,
        "main ran in the first step"
    );
    assert_eq!(
        global_int(&engine, "gBlizzardRan"),
        1,
        "main called InitBlizzard"
    );
}

#[test]
fn timer_started_by_main_fires_after_steps() {
    let mut engine = boot(synthetic_map(WAR3MAP_J));
    for _ in 0..5 {
        engine.step();
    }
    assert_eq!(
        global_int(&engine, "gFired"),
        0,
        "0.5 s timer not due after 0.5 s"
    );

    for _ in 0..10 {
        engine.step();
    }
    assert_eq!(global_int(&engine, "gFired"), 1, "fired once, after 0.5 s");
    assert_eq!(engine.report().timers_fired, 1);

    for _ in 0..10 {
        engine.step();
    }
    assert_eq!(
        global_int(&engine, "gFired"),
        1,
        "a one-shot timer fires once"
    );
}

#[test]
fn missing_natives_are_reported_and_implemented_ones_are_not() {
    let engine = boot(synthetic_map(WAR3MAP_J));
    let report = engine.report();
    // UsesMissing is compiled, so its call counts as a reference. The timer natives are implemented.
    assert_eq!(report.natives_missing, vec!["NoSuchNative".to_string()]);
    assert!(report.natives_referenced >= 3, "{report}");
    assert!(report.functions >= 6, "{report}");
    assert!(report.globals >= 5, "{report}");
    let text = report.to_string();
    assert!(text.contains("NoSuchNative"), "{text}");
}

#[test]
fn failing_config_is_reported_and_boot_continues() {
    let script = WAR3MAP_J.replace("set gConfig = 1", "call UsesMissing()");
    let engine = boot(synthetic_map(&script));
    let report = engine.report();
    assert!(report.config.starts_with("failed"), "{}", report.config);
    assert!(report.main.starts_with("queued"), "{}", report.main);
}

#[test]
fn run_config_and_start_main_are_callable_again() {
    let mut engine = boot(synthetic_map(WAR3MAP_J));
    engine.run_config().expect("config runs again");
    engine.start_main().expect("main queued again");
    engine.step();
    assert_eq!(global_int(&engine, "gBlizzardRan"), 2, "main ran twice");
}

#[test]
fn missing_map_script_is_an_error() {
    let info = w3i("No Script", BOUNDS, &players(), &[]);
    let map = common::mpq(&[("war3map.w3i", info)], 512);
    match Engine::boot(inputs(map)) {
        Err(Error::MissingFile { what, .. }) => assert_eq!(what, "war3map.j"),
        Err(e) => panic!("wrong error: {e}"),
        Ok(_) => panic!("boot without war3map.j should fail"),
    }
}

#[test]
fn script_archives_later_archive_overrides_earlier() {
    let old = common::mpq(
        &[
            ("Scripts\\common.j", b"old common".to_vec()),
            ("Scripts\\Blizzard.j", b"old blizzard".to_vec()),
        ],
        0,
    );
    let new = common::mpq(&[("Scripts\\Blizzard.j", b"new blizzard".to_vec())], 0);
    let (common_j, blizzard_j) =
        engine::scripts_from_archives(&[&old, &new]).expect("scripts found");
    assert_eq!(common_j, "old common");
    assert_eq!(blizzard_j, "new blizzard");
}

#[test]
fn real_literal_uses_the_correctly_rounded_value_without_tables() {
    let engine = boot(synthetic_map(WAR3MAP_J));
    match engine.host().vm().global("gR") {
        Some(Value::Real(r)) => assert_eq!(r.to_bits(), 0x3DCC_CCCD),
        other => panic!("gR is {other:?}"),
    }
    assert!(engine.rng_table().is_none());
    assert!(engine.notes().iter().any(|n| n.contains("no Game.dll")));
}

#[test]
fn game_dll_tables_give_the_original_literal_bits() {
    let Some(dll) = game_dll() else {
        eprintln!("GAME_DLL not set: skipping the Game.dll table test");
        return;
    };
    let engine = Engine::boot(Inputs {
        game_dll: Some(dll),
        ..inputs(synthetic_map(WAR3MAP_J))
    })
    .unwrap_or_else(|e| panic!("boot with Game.dll failed: {e}"));
    match engine.host().vm().global("gR") {
        Some(Value::Real(r)) => assert_eq!(r.to_bits(), 0x3DCC_CCCE, "0.1 is 0x3DCCCCCE"),
        other => panic!("gR is {other:?}"),
    }
    assert!(engine.rng_table().is_some());
}

#[test]
fn boot_seeds_the_generator_from_the_raw_game_seed() {
    let Some(dll) = game_dll() else {
        eprintln!("GAME_DLL not set: skipping the boot RNG seeding test");
        return;
    };
    let seed = 0x1234_5678;
    let engine = Engine::boot(Inputs {
        game_dll: Some(dll.clone()),
        rng_seed: seed,
        ..inputs(synthetic_map(WAR3MAP_J))
    })
    .unwrap_or_else(|e| panic!("boot with Game.dll failed: {e}"));
    // The synthetic script draws no random numbers, so the state is the game-start state.
    let table = sim::RngTable::from_game_dll(&dll).expect("RNG table");
    let mut direct = sim::Streams::default();
    direct.start_game(&table, seed);
    assert_eq!(*engine.host().rng(), direct);
}

#[test]
fn boot_without_game_dll_does_not_seed_the_generator() {
    let engine = boot(synthetic_map(WAR3MAP_J));
    assert_eq!(*engine.host().rng(), sim::Streams::default());
    assert!(engine.notes().iter().any(|n| n.contains("RNG table")));
}

#[test]
fn w3boot_prints_the_report() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("w3boot-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let map_path = dir.join("synthetic.w3x");
    std::fs::write(&map_path, synthetic_map(WAR3MAP_J)).expect("write map");
    let common_path = dir.join("common.j");
    std::fs::write(&common_path, COMMON_J).expect("write common.j");
    let blizzard_path = dir.join("blizzard.j");
    std::fs::write(&blizzard_path, BLIZZARD_J).expect("write blizzard.j");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_w3boot"))
        .arg("--common")
        .arg(&common_path)
        .arg("--blizzard")
        .arg(&blizzard_path)
        .arg("--steps")
        .arg("10")
        .arg(&map_path)
        .output()
        .expect("run w3boot");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("map: \"Synthetic Test\""), "{stdout}");
    assert!(
        stdout.contains("natives referenced but not implemented (1):"),
        "{stdout}"
    );
    assert!(stdout.contains("  NoSuchNative"), "{stdout}");
    assert!(stdout.contains("ticks: 200 (batches: 10,"), "{stdout}");
    assert!(
        stdout.contains("timer and sleep entries fired: 1"),
        "{stdout}"
    );
}

#[test]
fn w3boot_rejects_bad_arguments() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_w3boot"))
        .arg("--steps")
        .arg("abc")
        .output()
        .expect("run w3boot");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a number: abc"));
}
