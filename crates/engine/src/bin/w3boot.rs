//! `w3boot`: boots a map and prints a short report.
//!
//! ```text
//! w3boot [--dll Game.dll] (--common common.j --blizzard blizzard.j | --mpq War3.mpq [--mpq War3x.mpq])
//!        [--ai-common common.ai] [--ai-script NAME=PATH]... [--computer SLOT[:RACE]]... [--ai-log N]
//!        [--seed S] [--steps N] [--check-natives] map.w3x
//! w3boot --replay FILE.w3g --check-sync
//! ```
//!
//! `--check-sync` needs no map or archives: it walks the replay's time slots and compares the stored 0x22 sync
//! records with the model of `sim::sync` (net checksum, tick clock, record rule) and prints the first mismatch
//! and the number of record pairs predicted exactly (`engine::sync_check`, `docs/spec/lockstep.md` section 15).
//!
//! `--mpq` reads `Scripts\common.j` and `Scripts\Blizzard.j` from the archives (later ones override),
//! and the AI scripts `Scripts\common.ai`, `human.ai`, `orc.ai`, `undead.ai` and `elf.ai` when they are there.
//! The same archives give the Misc constants and the unit rows (`Units\MiscData.txt`, `Units\UnitBalance.slk`, ...);
//! the damage model is `SpecDamage` only with `--dll` as well (see `engine::data`).
//! `--ai-common` and `--ai-script NAME=PATH` give AI scripts as files; they override the archive ones.
//! `--seed` is the raw game-start seed (decimal or `0x` hex). Without it, `--replay FILE.w3g` takes the
//! seed from the replay's game-start record (link UNSURE, `docs/spec/game-start.md`); otherwise 0.
//! See `engine::Inputs::rng_seed` and `engine::game_start`.

use std::process::ExitCode;

use engine::{Engine, Inputs};

const USAGE: &str = "usage: w3boot [--dll Game.dll] (--common common.j --blizzard blizzard.j | \
--mpq War3.mpq [--mpq War3x.mpq]) [--ai-common common.ai] [--ai-script NAME=PATH]... \
[--seed S] [--steps N] [--check-natives] map.w3x";

#[derive(Debug, Default)]
struct Args {
    dll: Option<String>,
    common: Option<String>,
    blizzard: Option<String>,
    mpqs: Vec<String>,
    ai_common: Option<String>,
    ai_scripts: Vec<(String, String)>,
    seed: Option<u32>,
    replay: Option<String>,
    steps: u64,
    /// `--computer SLOT[:RACE]`: make a slot a computer player (RACE human|orc|undead|nightelf, else the map's).
    computers: Vec<(u32, Option<u32>)>,
    /// `--ai-log N`: print the first N entries of the AI command log (0: none).
    ai_log: usize,
    /// `--check-natives`: reject `native` lines that the 1.26a native table does not have (like the game).
    check_natives: bool,
    /// `--check-sync`: with `--replay`, compare the replay's 0x22 sync records with the model (no map needed).
    check_sync: bool,
    map: Option<String>,
}

/// A `NAME=PATH` argument of `--ai-script`.
fn parse_ai_script(text: &str) -> Result<(String, String), String> {
    match text.split_once('=') {
        Some((name, path)) if !name.is_empty() && !path.is_empty() => {
            Ok((name.to_string(), path.to_string()))
        }
        _ => Err(format!("--ai-script: expected NAME=PATH, got {text}")),
    }
}

/// A `SLOT[:RACE]` argument of `--computer` (RACE: human 1, orc 2, undead 3, nightelf 4).
fn parse_computer(text: &str) -> Result<(u32, Option<u32>), String> {
    let (slot, race) = match text.split_once(':') {
        Some((s, r)) => (s, Some(r)),
        None => (text, None),
    };
    let slot = slot
        .parse::<u32>()
        .map_err(|_| format!("--computer: bad slot {text}"))?;
    let race = match race {
        None => None,
        Some("human") => Some(1),
        Some("orc") => Some(2),
        Some("undead") => Some(3),
        Some("nightelf") => Some(4),
        Some(other) => return Err(format!("--computer: unknown race {other}")),
    };
    Ok((slot, race))
}

/// A 32-bit seed in decimal or `0x` hex.
fn parse_seed(text: &str) -> Result<u32, String> {
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse::<u32>(),
    };
    parsed.map_err(|_| format!("--seed: not a 32-bit number: {text}"))
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--dll" => args.dll = Some(value("--dll")?),
            "--common" => args.common = Some(value("--common")?),
            "--blizzard" => args.blizzard = Some(value("--blizzard")?),
            "--mpq" => args.mpqs.push(value("--mpq")?),
            "--ai-common" => args.ai_common = Some(value("--ai-common")?),
            "--ai-script" => args
                .ai_scripts
                .push(parse_ai_script(&value("--ai-script")?)?),
            "--check-natives" => args.check_natives = true,
            "--check-sync" => args.check_sync = true,
            "--seed" => args.seed = Some(parse_seed(&value("--seed")?)?),
            "--replay" => args.replay = Some(value("--replay")?),
            "--ai-log" => {
                let v = value("--ai-log")?;
                args.ai_log = v
                    .parse()
                    .map_err(|_| format!("--ai-log: not a number: {v}"))?;
            }
            "--computer" => args.computers.push(parse_computer(&value("--computer")?)?),
            "--steps" => {
                let v = value("--steps")?;
                args.steps = v
                    .parse()
                    .map_err(|_| format!("--steps: not a number: {v}"))?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => {
                if args.map.replace(path.to_string()).is_some() {
                    return Err("more than one map file given".to_string());
                }
            }
        }
    }
    if args.check_sync {
        if args.replay.is_none() {
            return Err("--check-sync needs --replay FILE.w3g".to_string());
        }
        return Ok(args);
    }
    if args.map.is_none() {
        return Err("no map file given".to_string());
    }
    let have_paths = args.common.is_some() && args.blizzard.is_some();
    if !have_paths && args.mpqs.is_empty() {
        return Err("give --common and --blizzard, or --mpq".to_string());
    }
    if args.common.is_some() != args.blizzard.is_some() {
        return Err("--common and --blizzard go together".to_string());
    }
    Ok(args)
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn inputs(args: &Args) -> Result<Inputs, String> {
    let map = read(args.map.as_deref().unwrap_or_default())?;
    let replay = match &args.replay {
        Some(path) => Some(
            w3g::Replay::parse(&read(path)?).map_err(|e| format!("--replay {path}: {e}"))?,
        ),
        None => None,
    };
    let start = engine::game_start::resolve(args.seed, replay.as_ref());
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
        _ => engine::scripts_from_archives(&refs).map_err(|e| e.to_string())?,
    };
    // AI scripts: the archives first, then the explicit files, which replace them.
    let mut ai = engine::ai_scripts_from_archives(&refs).map_err(|e| e.to_string())?;
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

fn run(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    if args.check_sync {
        let path = args.replay.as_deref().unwrap_or_default();
        let replay =
            w3g::Replay::parse(&read(path)?).map_err(|e| format!("--replay {path}: {e}"))?;
        print!("{}", engine::sync_check::check_replay(&replay).render());
        return Ok(());
    }
    let mut engine = Engine::boot(inputs(&args)?).map_err(|e| e.to_string())?;
    for &(slot, race) in &args.computers {
        let host = engine.host();
        let mut game = host.game_mut();
        let p = game
            .player_mut(slot)
            .ok_or_else(|| format!("--computer: no slot {slot}"))?;
        p.controller = engine::map::CONTROL_COMPUTER;
        p.slot_state = 1;
        if let Some(r) = race {
            p.race = r;
        }
    }
    for i in 0..args.steps {
        engine.step();
        // One status line per game minute for every computer slot.
        if (i + 1) % 600 == 0 {
            for &(slot, _) in &args.computers {
                let line = engine.host().game().production_summary(slot);
                println!("t={:4}s p{slot}: {line}", (i + 1) / 10);
            }
        }
    }
    print!("{}", engine.report());
    if args.ai_log > 0 {
        let shared = engine.ai().shared().borrow();
        for (i, c) in shared.commands.iter().take(args.ai_log).enumerate() {
            println!("ai-log {i:4}: p{} {} {:?}", c.player, c.name, c.args);
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("w3boot: {e}");
            ExitCode::FAILURE
        }
    }
}
