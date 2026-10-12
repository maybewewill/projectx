//! `w3boot`: boots a map and prints a short report.
//!
//! ```text
//! w3boot [--dll Game.dll] (--common common.j --blizzard blizzard.j | --mpq War3.mpq [--mpq War3x.mpq])
//!        [--ai-common common.ai] [--ai-script NAME=PATH]... [--computer SLOT[:RACE]]... [--ai-log N]
//!        [--seed S] [--steps N] [--buildings] [--check-natives] map.w3x
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
//! See `engine::Inputs::rng_seed` and `engine::game_start`. The file reads and the boot itself are
//! `engine::cli`, which `viewer --sim-seconds` shares.

use std::process::ExitCode;

use engine::cli::{self, BootArgs};

const USAGE: &str = "usage: w3boot [--dll Game.dll] (--common common.j --blizzard blizzard.j | \
--mpq War3.mpq [--mpq War3x.mpq]) [--ai-common common.ai] [--ai-script NAME=PATH]... \
[--seed S] [--steps N] [--buildings] [--check-natives] map.w3x";

#[derive(Debug, Default)]
struct Args {
    boot: BootArgs,
    /// `--ai-log N`: print the first N entries of the AI command log (0: none).
    ai_log: usize,
    /// `--buildings`: print the structures of every computer slot after the run.
    buildings: bool,
    /// `--check-sync`: with `--replay`, compare the replay's 0x22 sync records with the model (no map needed).
    check_sync: bool,
    steps: u64,
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
            "--dll" => args.boot.dll = Some(value("--dll")?),
            "--common" => args.boot.common = Some(value("--common")?),
            "--blizzard" => args.boot.blizzard = Some(value("--blizzard")?),
            "--mpq" => args.boot.mpqs.push(value("--mpq")?),
            "--ai-common" => args.boot.ai_common = Some(value("--ai-common")?),
            "--ai-script" => args
                .boot
                .ai_scripts
                .push(parse_ai_script(&value("--ai-script")?)?),
            "--check-natives" => args.boot.check_natives = true,
            "--check-sync" => args.check_sync = true,
            "--buildings" => args.buildings = true,
            "--seed" => args.boot.seed = Some(parse_seed(&value("--seed")?)?),
            "--replay" => args.boot.replay = Some(value("--replay")?),
            "--ai-log" => {
                let v = value("--ai-log")?;
                args.ai_log = v
                    .parse()
                    .map_err(|_| format!("--ai-log: not a number: {v}"))?;
            }
            "--computer" => args
                .boot
                .computers
                .push(parse_computer(&value("--computer")?)?),
            "--steps" => {
                let v = value("--steps")?;
                args.steps = v
                    .parse()
                    .map_err(|_| format!("--steps: not a number: {v}"))?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => {
                if args.boot.map.replace(path.to_string()).is_some() {
                    return Err("more than one map file given".to_string());
                }
            }
        }
    }
    if args.check_sync {
        if args.boot.replay.is_none() {
            return Err("--check-sync needs --replay FILE.w3g".to_string());
        }
        return Ok(args);
    }
    if args.boot.map.is_none() {
        return Err("no map file given".to_string());
    }
    let have_paths = args.boot.common.is_some() && args.boot.blizzard.is_some();
    if !have_paths && args.boot.mpqs.is_empty() {
        return Err("give --common and --blizzard, or --mpq".to_string());
    }
    if args.boot.common.is_some() != args.boot.blizzard.is_some() {
        return Err("--common and --blizzard go together".to_string());
    }
    Ok(args)
}

fn run(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    if args.check_sync {
        let path = args.boot.replay.as_deref().unwrap_or_default();
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let replay = w3g::Replay::parse(&bytes).map_err(|e| format!("--replay {path}: {e}"))?;
        print!("{}", engine::sync_check::check_replay(&replay).render());
        return Ok(());
    }
    let mut engine = cli::boot(&args.boot)?;
    let mut last_pvp = 0usize;
    for i in 0..args.steps {
        engine.step();
        // Player-versus-player kills as they happen (both killer and victim a real player slot).
        let pvp = engine
            .host()
            .game()
            .kill_log
            .iter()
            .filter(|&&(k, v, _)| k < 12 && v < 12 && k != v)
            .count();
        if pvp != last_pvp {
            if last_pvp == 0 {
                println!("first player-vs-player kill at step {} ({:.1} s)", i + 1, (i + 1) as f32 / 10.0);
            }
            last_pvp = pvp;
        }
        // One status line per game minute for every computer slot.
        if (i + 1) % 600 == 0 {
            for &(slot, _) in &args.boot.computers {
                let line = engine.host().game().production_summary(slot);
                println!("t={:4}s p{slot}: {line}", (i + 1) / 10);
                let shared = engine.ai().shared().borrow();
                if let Some(c) = shared.control(slot).map(|c| &c.captains[0]) {
                    println!(
                        "         captain: {} members, mode {:?}, combat {}, full {}, departures {}",
                        c.members.len(),
                        c.mode,
                        c.in_combat,
                        c.flags & 1,
                        c.departures.len()
                    );
                    println!("         {}", sim::captains::describe(&engine.host().game(), c));
                }
            }
        }
    }
    if args.buildings {
        for &(slot, _) in &args.boot.computers {
            for (name, x, y, wip) in engine.host().game().building_positions(slot) {
                println!("building p{slot} {name:<16} ({x:9.1}, {y:9.1}){}", if wip { " *" } else { "" });
            }
        }
    }
    print!("{}", engine.report());
    {
        let shared = engine.ai().shared().borrow();
        for &(slot, _) in &args.boot.computers {
            println!("final p{slot}: {}", engine.host().game().player_report(slot));
            let learned: Vec<String> = shared
                .skills_learned
                .iter()
                .filter(|&&(p, _, _)| p == slot)
                .map(|&(_, _, c)| String::from_utf8_lossy(&c.to_be_bytes()).into_owned())
                .collect();
            println!("hero skills chosen by the callback p{slot}: {}", learned.join(" "));
            if let Some(c) = shared.control(slot) {
                for d in &c.captains[0].departures {
                    let kinds: Vec<String> = d
                        .types
                        .iter()
                        .map(|t| String::from_utf8_lossy(&t.to_be_bytes()).into_owned())
                        .collect();
                    println!(
                        "departure p{slot}: t={:.1}s size {} goal {:?} units {}",
                        d.at,
                        d.size,
                        d.goal,
                        kinds.join(",")
                    );
                }
            }
        }
        // Deaths by (killer owner -> victim owner).
        let mut table: std::collections::BTreeMap<(u32, u32), u32> = std::collections::BTreeMap::new();
        for &(k, v, _) in &engine.host().game().kill_log {
            *table.entry((k, v)).or_default() += 1;
        }
        for ((k, v), n) in table {
            let killer = if k == u32::MAX { "none".to_string() } else { format!("p{k}") };
            println!("kills: {killer} -> p{v}: {n}");
        }
    }
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
