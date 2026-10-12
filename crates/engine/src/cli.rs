//! The file-based boot path shared by `w3boot` and `viewer --sim-seconds`: reads the map, the
//! archives (or `common.j` and `blizzard.j` as files), the AI scripts and the optional replay, boots
//! the engine and marks the `--computer` slots. See `bin/w3boot.rs` for the flags.

use crate::{Engine, Inputs};

/// What the file-based boot needs: paths as the command line gives them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BootArgs {
    /// The map file (`.w3x`).
    pub map: Option<String>,
    /// `Game.dll`: the soft-float tables and the RNG table come from it.
    pub dll: Option<String>,
    /// `common.j` and `blizzard.j` as files (both or neither; otherwise they come from `mpqs`).
    pub common: Option<String>,
    pub blizzard: Option<String>,
    /// The archives in load order (later ones override).
    pub mpqs: Vec<String>,
    /// `common.ai` and named AI scripts as files; they override the archive ones.
    pub ai_common: Option<String>,
    pub ai_scripts: Vec<(String, String)>,
    /// The raw game-start seed (see `Inputs::rng_seed`).
    pub seed: Option<u32>,
    /// A replay whose game-start record gives the seed when `seed` is absent.
    pub replay: Option<String>,
    /// `--computer SLOT[:RACE]`: the slot becomes a computer player; RACE is the 1.26a race id.
    pub computers: Vec<(u32, Option<u32>)>,
    /// Reject `native` lines the 1.26a table does not have.
    pub check_natives: bool,
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

/// The engine inputs for `args` (the same reads and choices as `w3boot`).
pub fn inputs(args: &BootArgs) -> Result<Inputs, String> {
    let map = read(args.map.as_deref().ok_or("no map file given")?)?;
    let replay = match &args.replay {
        Some(path) => Some(
            w3g::Replay::parse(&read(path)?).map_err(|e| format!("--replay {path}: {e}"))?,
        ),
        None => None,
    };
    let start = crate::game_start::resolve(args.seed, replay.as_ref());
    let game_dll = args.dll.as_deref().map(read).transpose()?;
    let archives = args
        .mpqs
        .iter()
        .map(|p| read(p))
        .collect::<Result<Vec<_>, _>>()?;
    let refs: Vec<&[u8]> = archives.iter().map(Vec::as_slice).collect();
    let (common_j, blizzard_j) = match (&args.common, &args.blizzard) {
        (Some(c), Some(b)) => (
            String::from_utf8_lossy(&read(c)?).into_owned(),
            String::from_utf8_lossy(&read(b)?).into_owned(),
        ),
        _ => crate::scripts_from_archives(&refs).map_err(|e| e.to_string())?,
    };
    // AI scripts: the archives first, then the explicit files, which replace them.
    let mut ai = crate::ai_scripts_from_archives(&refs).map_err(|e| e.to_string())?;
    if let Some(path) = &args.ai_common {
        ai.common = Some(String::from_utf8_lossy(&read(path)?).into_owned());
    }
    for (name, path) in &args.ai_scripts {
        ai.scripts.push((
            name.clone(),
            String::from_utf8_lossy(&read(path)?).into_owned(),
        ));
    }
    Ok(Inputs {
        map,
        game_dll,
        common_j,
        blizzard_j,
        rng_seed: start.seed,
        ai_common: ai.common,
        ai_scripts: ai.scripts,
        game_archives: archives,
        check_natives: args.check_natives,
    })
}

/// Marks the `--computer` slots of a booted engine (controller computer, slot state playing, race).
pub fn apply_computers(engine: &Engine, computers: &[(u32, Option<u32>)]) -> Result<(), String> {
    let host = engine.host();
    let mut game = host.game_mut();
    for &(slot, race) in computers {
        let p = game
            .player_mut(slot)
            .ok_or_else(|| format!("--computer: no slot {slot}"))?;
        p.controller = crate::map::CONTROL_COMPUTER;
        p.slot_state = 1;
        if let Some(r) = race {
            p.race = r;
        }
    }
    Ok(())
}

/// Boots the engine from `args` and marks its computer slots.
pub fn boot(args: &BootArgs) -> Result<Engine, String> {
    let engine = Engine::boot(inputs(args)?).map_err(|e| e.to_string())?;
    apply_computers(&engine, &args.computers)?;
    Ok(engine)
}
