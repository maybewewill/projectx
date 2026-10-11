//! Boots a map from a replay's game setup: seed, slots, teams, colors and races from the `.w3g` file.
//!
//! The replay gives the game-start data the original reads when a replay is loaded
//! (`docs/spec/replay-load.md`):
//!
//! - the seed: the random seed field of the `0x19` record ([`setup_of`]), passed through the
//!   fixed-seed rule of the setup tail (`sim::rng::game_start_seed`, CONFIRMED rule; whether the replay's
//!   stat-string flags are the settings word it tests is LIKELY, not CONFIRMED);
//! - the slots: one per record of the `0x19` body. Slot `n` becomes JASS player `n`
//!   ([`apply_slots`], LIKELY mapping); the occupant name comes from the player record with the same
//!   player id.
//!
//! Not modelled (see the spec): the game speed (the speed bits of the stat-string flags are not
//! located), handicap and AI strength (the simulation's player object has no field for them, so
//! they are only reported in [`ReplaySetup::slots`]), the random race draw, start locations, and the
//! duplicate and fold paths of the original's setup.

use sim::natives::game::PLAYER_SLOTS;
use sim::natives::Game;

use crate::map::{CONTROL_COMPUTER, CONTROL_NONE, CONTROL_USER};
use crate::{Engine, Error, Inputs};

/// `RACE_HUMAN` (LIKELY: `ConvertRace` order of `common.j`, not checked against a file).
pub const RACE_HUMAN: u32 = 1;
/// `RACE_ORC`.
pub const RACE_ORC: u32 = 2;
/// `RACE_UNDEAD`.
pub const RACE_UNDEAD: u32 = 3;
/// `RACE_NIGHTELF`.
pub const RACE_NIGHTELF: u32 = 4;

/// The slot status of an occupied slot (LIKELY: 0 empty, 1 closed, 2 occupied, as in the public format).
pub const SLOT_USED: u8 = 2;

/// The settings flag that selects the fixed seed `0x77617233` (CONFIRMED rule, see `sim::rng`).
pub const FIXED_SEED_FLAG: u32 = 0x8000;

/// Maps a replay race byte to the JASS race value (LIKELY bit layout: 0x01 human, 0x02 orc, 0x04 night
/// elf, 0x08 undead). A random race (0x20) or any other value gives `None`: the original's random
/// draw is not modelled.
pub fn race_of(raw: u8) -> Option<u32> {
    match raw {
        0x01 => Some(RACE_HUMAN),
        0x02 => Some(RACE_ORC),
        0x04 => Some(RACE_NIGHTELF),
        0x08 => Some(RACE_UNDEAD),
        _ => None,
    }
}

/// Why a replay's game setup is refused. The original refuses these records; the setup then takes its
/// other path, which is not modelled (`docs/spec/game-start.md`, section 4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    /// The record has no slots (the acceptance check `0x6F5BE74F`).
    #[error("the replay's game-start record has no slots")]
    NoSlots,
    /// More start spots than slots (the unsigned compare at `0x6F5BE75A`).
    #[error("the replay's game-start record has {start_spots} start spots for {slots} slots")]
    StartSpotsAboveSlots { slots: usize, start_spots: u8 },
}

/// What a replay sets up before the first step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaySetup {
    /// The value `S` for `sim::Streams::start_game` (after the fixed-seed rule).
    pub seed: u32,
    /// The random seed field of the `0x19` record, before the fixed-seed rule.
    pub record_seed: u32,
    /// The settings flags (the stat-string flag word, `w3g::GameSettings::flags`).
    pub settings_flags: u32,
    /// The map path stored in the replay (for example `Maps\\Download\\x.w3x`).
    pub map_path: String,
    /// The host name stored in the replay.
    pub host_name: String,
    /// The `0x19` slot records, in slot order. Slot `n` is JASS player `n`.
    pub slots: Vec<w3g::Slot>,
    /// The player records (host and `0x16` records) for the occupant names.
    pub players: Vec<w3g::PlayerRecord>,
    /// The first human slot, or `None` when the record has no human.
    pub local_player: Option<u32>,
    /// What was applied and what was not (speed, random race, fixed seed, missing RNG table).
    pub notes: Vec<String>,
}

/// Derives the game setup of a parsed replay (no engine needed).
pub fn setup_of(replay: &w3g::Replay) -> Result<ReplaySetup, SetupError> {
    let game = &replay.game;
    if game.slots.is_empty() {
        return Err(SetupError::NoSlots);
    }
    if usize::from(game.start_spot_count) > game.slots.len() {
        return Err(SetupError::StartSpotsAboveSlots {
            slots: game.slots.len(),
            start_spots: game.start_spot_count,
        });
    }
    let flags = game.settings.flags;
    let seed = sim::rng::game_start_seed(flags, game.random_seed);
    let mut notes = Vec::new();
    if flags & FIXED_SEED_FLAG != 0 {
        notes.push(format!(
            "settings flags {flags:#010x} have the fixed-seed bit: seed {seed:#010x} replaces the record seed (LIKELY link)"
        ));
    }
    let local_player = game
        .slots
        .iter()
        .position(|s| s.status == SLOT_USED && s.computer == 0)
        .map(|n| n as u32);
    let mut players = vec![game.host.clone()];
    players.extend(game.players.iter().cloned());
    Ok(ReplaySetup {
        seed,
        record_seed: game.random_seed,
        settings_flags: flags,
        map_path: String::from_utf8_lossy(&game.settings.map_path).into_owned(),
        host_name: String::from_utf8_lossy(&game.settings.host_name).into_owned(),
        slots: game.slots.clone(),
        players,
        local_player,
        notes,
    })
}

/// The name of the occupant of a slot: the player record with the same player id.
fn occupant_name(players: &[w3g::PlayerRecord], player_id: u8) -> String {
    players
        .iter()
        .find(|p| p.player_id == player_id)
        .map(|p| String::from_utf8_lossy(&p.name).into_owned())
        .unwrap_or_default()
}

/// Writes the replay's slots into the first 16 player objects (CONFIRMED: `PLAYER_SLOTS` objects).
///
/// Slot `n` is player `n`: name (from the occupant's player record), race (the random race keeps the
/// value the map gave: UNSURE), color, controller, slot state (1 for an occupied slot, 0 otherwise),
/// team and start location (`n`, the same placeholder as the map path). Players without a slot record
/// become empty. Returns the local player (the first occupied human slot).
pub fn apply_slots(game: &mut Game, setup: &ReplaySetup) -> Option<u32> {
    let mut local = None;
    for n in 0..PLAYER_SLOTS {
        let Some(p) = game.player_mut(n as u32) else {
            continue;
        };
        match setup.slots.get(n) {
            Some(s) => {
                let occupied = s.status == SLOT_USED;
                let controller = if !occupied {
                    CONTROL_NONE
                } else if s.computer != 0 {
                    CONTROL_COMPUTER
                } else {
                    CONTROL_USER
                };
                p.name = occupant_name(&setup.players, s.player_id);
                if let Some(race) = race_of(s.race) {
                    p.race = race;
                }
                p.color = u32::from(s.color);
                p.controller = controller;
                p.slot_state = u32::from(occupied);
                p.team = i32::from(s.team);
                p.start_location = n as i32;
                if controller == CONTROL_USER && local.is_none() {
                    local = Some(n as u32);
                }
            }
            None => {
                p.name.clear();
                p.controller = CONTROL_NONE;
                p.slot_state = 0;
                p.team = 0;
                p.start_location = n as i32;
            }
        }
    }
    local
}

impl Engine {
    /// Boots the map of `inputs` with the game setup of a replay (`replay` is the `.w3g` file).
    ///
    /// The seed is set before the boot ([`Inputs::rng_seed`], used when a `Game.dll` table is given), and
    /// the slots of the replay replace the w3i player setup after the boot. The returned [`ReplaySetup`]
    /// lists what was applied and the notes.
    pub fn boot_from_replay(replay: &[u8], mut inputs: Inputs) -> Result<(Engine, ReplaySetup), Error> {
        let parsed = w3g::Replay::parse(replay)?;
        let mut setup = setup_of(&parsed)?;
        if inputs.game_dll.is_none() {
            setup
                .notes
                .push("no Game.dll: the replay seed is not applied (no RNG table)".to_string());
        }
        inputs.rng_seed = setup.seed;
        let engine = Engine::boot(inputs)?;
        let local = {
            let host = engine.host();
            let mut game = host.game_mut();
            apply_slots(&mut game, &setup)
        };
        if let Some(local) = local.or(setup.local_player) {
            engine.host().game_mut().set_local_player(local);
        }
        Ok((engine, setup))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        player_id: u8,
        status: u8,
        computer: u8,
        team: u8,
        color: u8,
        race: u8,
        handicap: u8,
    ) -> [u8; 9] {
        [player_id, 100, status, computer, team, color, race, 1, handicap]
    }

    /// A `0x19` body with the given slot records, seed, select mode and start spots.
    fn start_body(slots: &[[u8; 9]], seed: u32, start_spots: u8) -> Vec<u8> {
        let mut rec = vec![slots.len() as u8];
        for s in slots {
            rec.extend_from_slice(s);
        }
        rec.extend_from_slice(&seed.to_le_bytes());
        rec.extend_from_slice(&[0x09, start_spots]);
        rec
    }

    fn replay_from(flags: u32, slots: &[[u8; 9]], seed: u32, start_spots: u8) -> w3g::Replay {
        let mut body = Vec::new();
        body.extend_from_slice(&0x110u32.to_le_bytes());
        body.extend_from_slice(&[0x00, 1]);
        body.extend_from_slice(b"Alice\0");
        body.extend_from_slice(&[1, 0]);
        body.extend_from_slice(b"Test Game\0");
        body.push(0);
        let mut settings = Vec::new();
        settings.extend_from_slice(&flags.to_le_bytes());
        settings.push(0);
        settings.extend_from_slice(&116u16.to_le_bytes());
        settings.extend_from_slice(&84u16.to_le_bytes());
        settings.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        settings.extend_from_slice(b"Maps\\Download\\x.w3x\0Alice\0\0");
        body.extend_from_slice(&w3g::encode_string(&settings));
        body.push(0);
        body.extend_from_slice(&(slots.len() as u32).to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        // A second player record (0x16): id 2, name "Bob", one extra byte, then the unknown dword.
        body.extend_from_slice(&[0x16, 2]);
        body.extend_from_slice(b"Bob\0");
        body.extend_from_slice(&[1, 0]);
        body.extend_from_slice(&0u32.to_le_bytes());
        let rec = start_body(slots, seed, start_spots);
        body.push(0x19);
        body.extend_from_slice(&(rec.len() as u16).to_le_bytes());
        body.extend_from_slice(&rec);
        w3g::Replay::parse(&synthetic_file(&body)).expect("synthetic replay parses")
    }

    /// Wraps a body in one zlib block (header version 1, size 0x44).
    fn synthetic_file(body: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(body).unwrap();
        let data = z.finish().unwrap();
        let mut f = Vec::new();
        f.extend_from_slice(w3g::MAGIC);
        f.extend_from_slice(&0x44u32.to_le_bytes());
        f.extend_from_slice(&((0x44 + 8 + data.len()) as u32).to_le_bytes());
        f.extend_from_slice(&1u32.to_le_bytes());
        f.extend_from_slice(&(body.len() as u32).to_le_bytes());
        f.extend_from_slice(&1u32.to_le_bytes());
        f.extend_from_slice(b"PX3W");
        f.extend_from_slice(&26u32.to_le_bytes());
        f.extend_from_slice(&6059u16.to_le_bytes());
        f.extend_from_slice(&0x8000u16.to_le_bytes());
        f.extend_from_slice(&350u32.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&(data.len() as u16).to_le_bytes());
        f.extend_from_slice(&(body.len() as u16).to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&data);
        f
    }

    fn three_slots() -> Vec<[u8; 9]> {
        vec![
            record(1, 2, 0, 0, 0, 0x01, 100),
            record(2, 2, 1, 1, 1, 0x02, 50),
            record(3, 0, 0, 2, 2, 0x20, 100),
        ]
    }

    #[test]
    fn race_bits_map_to_the_four_races() {
        assert_eq!(race_of(0x01), Some(RACE_HUMAN));
        assert_eq!(race_of(0x02), Some(RACE_ORC));
        assert_eq!(race_of(0x04), Some(RACE_NIGHTELF));
        assert_eq!(race_of(0x08), Some(RACE_UNDEAD));
        assert_eq!(race_of(0x20), None);
        assert_eq!(race_of(0x03), None);
    }

    #[test]
    fn record_seed_is_used_without_the_fixed_seed_bit() {
        let r = replay_from(0x0001_0000, &three_slots(), 0x1234_5678, 2);
        let s = setup_of(&r).unwrap();
        assert_eq!(s.seed, 0x1234_5678);
        assert_eq!(s.record_seed, 0x1234_5678);
        assert!(s.notes.is_empty());
    }

    #[test]
    fn fixed_seed_bit_replaces_the_record_seed() {
        let r = replay_from(FIXED_SEED_FLAG, &three_slots(), 0x1234_5678, 2);
        let s = setup_of(&r).unwrap();
        assert_eq!(s.seed, sim::rng::FIXED_GAME_SEED);
        assert_eq!(s.record_seed, 0x1234_5678);
        assert_eq!(s.notes.len(), 1);
    }

    #[test]
    fn local_player_is_the_first_occupied_human() {
        let r = replay_from(0, &three_slots(), 1, 2);
        let s = setup_of(&r).unwrap();
        assert_eq!(s.local_player, Some(0));
        assert_eq!(s.host_name, "Alice");
        assert_eq!(s.map_path, "Maps\\Download\\x.w3x");
        // Slot 0 has no computer, slot 1 is a computer, slot 2 is empty: only slot 0 is local.
        let computers_only = vec![record(1, 2, 1, 0, 0, 0x01, 100), record(2, 2, 0, 1, 1, 0x02, 100)];
        let r = replay_from(0, &computers_only, 1, 1);
        assert_eq!(setup_of(&r).unwrap().local_player, Some(1));
    }

    #[test]
    fn refuses_no_slots_and_too_many_start_spots() {
        let none = replay_from(0, &[], 1, 0);
        assert_eq!(setup_of(&none), Err(SetupError::NoSlots));
        let spots = replay_from(0, &three_slots(), 1, 4);
        assert_eq!(
            setup_of(&spots),
            Err(SetupError::StartSpotsAboveSlots {
                slots: 3,
                start_spots: 4
            })
        );
    }

    #[test]
    fn apply_slots_sets_players_and_empties_the_rest() {
        let r = replay_from(0, &three_slots(), 1, 2);
        let s = setup_of(&r).unwrap();
        let mut game = Game::new();
        let local = apply_slots(&mut game, &s);
        assert_eq!(local, Some(0));
        let p0 = game.player(0);
        assert_eq!(p0.name, "Alice");
        assert_eq!(p0.race, RACE_HUMAN);
        assert_eq!(p0.controller, CONTROL_USER);
        assert_eq!(p0.slot_state, 1);
        assert_eq!(p0.team, 0);
        let p1 = game.player(1);
        assert_eq!(p1.name, "Bob");
        assert_eq!(p1.race, RACE_ORC);
        assert_eq!(p1.controller, CONTROL_COMPUTER);
        assert_eq!(p1.team, 1);
        assert_eq!(p1.color, 1);
        // Slot 2 is empty (status 0): not playing, no name.
        let p2 = game.player(2);
        assert_eq!(p2.slot_state, 0);
        assert_eq!(p2.controller, CONTROL_NONE);
        // Players past the record are empty.
        let p5 = game.player(5);
        assert_eq!(p5.slot_state, 0);
        assert_eq!(p5.controller, CONTROL_NONE);
    }

    #[test]
    fn random_race_keeps_the_map_value() {
        let r = replay_from(0, &three_slots(), 1, 2);
        let s = setup_of(&r).unwrap();
        let mut game = Game::new();
        game.player_mut(2).unwrap().race = RACE_NIGHTELF;
        apply_slots(&mut game, &s);
        assert_eq!(game.player(2).race, RACE_NIGHTELF);
    }
}
