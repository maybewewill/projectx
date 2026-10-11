//! Booting a synthetic map with the game setup of a synthetic `.w3g` replay (`Engine::boot_from_replay`).
//! The replay bytes are built here with the layout of `docs/spec/replay-load.md`; the expected values
//! come from those rules, not from the original.

mod common;

use common::{map_file, w3i, PlayerRec, BLIZZARD_J, COMMON_J, WAR3MAP_J};
use engine::replay_setup::{SetupError, FIXED_SEED_FLAG};
use engine::{Engine, Error, Inputs};

const BOUNDS: [f32; 8] = [-1024.0, 1024.0, -896.0, 896.0, 0.0, 0.0, 0.0, 0.0];

/// The map has three w3i players (human, computer, empty); the replay decides the setup.
fn synthetic_map() -> Vec<u8> {
    let players = [
        PlayerRec {
            number: 0,
            player_type: 1,
            race: 4,
            name: "MapHuman",
            start: (-100.0, 50.0),
        },
        PlayerRec {
            number: 1,
            player_type: 2,
            race: 2,
            name: "MapComputer",
            start: (300.0, -20.0),
        },
        PlayerRec {
            number: 2,
            player_type: 0,
            race: 1,
            name: "",
            start: (0.0, 0.0),
        },
    ];
    let info = w3i(
        "Replay Test",
        BOUNDS,
        &players,
        &[(0b011, "Force A"), (0b100, "Force B")],
    );
    map_file(info, WAR3MAP_J)
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
    }
}

/// One 9-byte slot record: player id, download, status, computer, team, color, race, AI strength, handicap.
fn slot(
    player_id: u8,
    status: u8,
    computer: u8,
    team: u8,
    color: u8,
    race: u8,
    handicap: u8,
) -> [u8; 9] {
    [
        player_id, 100, status, computer, team, color, race, 1, handicap,
    ]
}

/// A replay file (header version 1) with a host "Alice", a player record "Bob" (id 2), the given
/// settings flags, the slot records, the seed and the start spot count.
fn replay_file(flags: u32, slots: &[[u8; 9]], seed: u32, start_spots: u8) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0x110u32.to_le_bytes());
    body.extend_from_slice(&[0x00, 1]);
    body.extend_from_slice(b"Alice\0");
    body.extend_from_slice(&[1, 0]);
    body.extend_from_slice(b"Replay Game\0");
    body.push(0);
    let mut settings = Vec::new();
    settings.extend_from_slice(&flags.to_le_bytes());
    settings.push(0);
    settings.extend_from_slice(&116u16.to_le_bytes());
    settings.extend_from_slice(&84u16.to_le_bytes());
    settings.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    settings.extend_from_slice(b"Maps\\Download\\replay.w3x\0Alice\0\0");
    body.extend_from_slice(&w3g::encode_string(&settings));
    body.push(0);
    body.extend_from_slice(&(slots.len() as u32).to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&[0x16, 2]);
    body.extend_from_slice(b"Bob\0");
    body.extend_from_slice(&[1, 0]);
    body.extend_from_slice(&0u32.to_le_bytes());
    let mut rec = vec![slots.len() as u8];
    for s in slots {
        rec.extend_from_slice(s);
    }
    rec.extend_from_slice(&seed.to_le_bytes());
    rec.extend_from_slice(&[0x09, start_spots]);
    body.push(0x19);
    body.extend_from_slice(&(rec.len() as u16).to_le_bytes());
    body.extend_from_slice(&rec);

    w3g::write_file(&body, body.len().max(1), 6059, 0x8000, 350)
}

fn three_slots() -> Vec<[u8; 9]> {
    vec![
        slot(1, 2, 0, 0, 0, 0x01, 100),
        slot(2, 2, 1, 1, 1, 0x02, 50),
        slot(3, 0, 0, 2, 2, 0x20, 100),
    ]
}

#[test]
fn replay_slots_replace_the_map_players() {
    let file = replay_file(0, &three_slots(), 0x1357_9BDF, 2);
    let (engine, setup) = Engine::boot_from_replay(&file, inputs(synthetic_map())).expect("boots");
    assert_eq!(setup.seed, 0x1357_9BDF);
    assert_eq!(setup.local_player, Some(0));
    let report = engine.report();
    let line = |slot: u32| {
        report
            .players
            .iter()
            .find(|p| p.slot == slot)
            .expect("line")
    };
    // Slot 0: the replay's human (named by its player record) replaces the map's "MapHuman".
    assert_eq!(line(0).name, "Alice");
    assert_eq!(line(0).race, 1);
    assert_eq!(line(0).team, 0);
    assert_eq!(line(0).slot_state, 1);
    // Slot 1: the replay's computer on team 1, race orc (the map said night elf / orc).
    assert_eq!(line(1).name, "Bob");
    assert_eq!(line(1).race, 2);
    assert_eq!(line(1).team, 1);
    assert_eq!(line(1).controller, engine::map::CONTROL_COMPUTER);
    // Slot 2: empty in the replay.
    assert_eq!(line(2).slot_state, 0);
    assert_eq!(line(2).controller, engine::map::CONTROL_NONE);
    assert!(setup.notes.iter().any(|n| n.contains("no Game.dll")));
}

#[test]
fn fixed_seed_flag_gives_the_fixed_seed() {
    let file = replay_file(FIXED_SEED_FLAG, &three_slots(), 0x1357_9BDF, 2);
    let (_engine, setup) = Engine::boot_from_replay(&file, inputs(synthetic_map())).expect("boots");
    assert_eq!(setup.seed, sim::rng::FIXED_GAME_SEED);
    assert_eq!(setup.record_seed, 0x1357_9BDF);
}

#[test]
fn refused_setup_is_an_error_not_a_boot() {
    let file = replay_file(0, &three_slots(), 1, 4);
    match Engine::boot_from_replay(&file, inputs(synthetic_map())) {
        Err(Error::ReplaySetup(SetupError::StartSpotsAboveSlots {
            slots: 3,
            start_spots: 4,
        })) => {}
        Err(e) => panic!("unexpected error: {e}"),
        Ok(_) => panic!("a refused record must not boot"),
    }
}

#[test]
fn garbage_is_a_replay_error() {
    match Engine::boot_from_replay(b"not a replay", inputs(synthetic_map())) {
        Err(Error::Replay(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
        Ok(_) => panic!("garbage must not boot"),
    }
}
