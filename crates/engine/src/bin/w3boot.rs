//! `w3boot`: boots a map and prints a short report.
//!
//! ```text
//! w3boot [--dll Game.dll] (--common common.j --blizzard blizzard.j | --mpq War3.mpq [--mpq War3x.mpq])
//!        [--ai-common common.ai] [--ai-script NAME=PATH]... [--computer SLOT[:RACE]]... [--ai-log N]
//!        [--seed S] [--steps N] [--buildings] [--check-natives] map.w3x
//! w3boot --replay FILE.w3g --check-sync
//! w3boot --replay FILE.w3g --sim-sync [--sync-trace] [--sync-slots N] [--sync-after] [--sync-defer-main]
//!        [--sync-constant K] [--pr-off MASK] [--ai-delay N] [--ai-period N] --dll Game.dll --mpq ... MAP.w3x
//! ```
//!
//! `--check-sync` needs no map or archives: it walks the replay's time slots and compares the stored 0x22 sync
//! records with the model of `sim::sync` (net checksum, tick clock, record rule) and prints the first mismatch
//! and the number of record pairs predicted exactly (`engine::sync_check`, `docs/spec/lockstep.md` section 15).
//!
//! `--sim-sync` boots the map from the replay, runs the whole simulation slot by slot and compares the 0x22 record computed from
//! the simulated state with every record of the file: absolute and constant-calibrated matches, matching pairs, and the
//! hidden-word event timelines (`engine::sim_sync`, `docs/spec/lockstep.md` section 18). `--sync-trace` prints one line per slot.
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
    /// `--sim-sync`: with `--replay` and a map, run the full engine and compare every 0x22 record.
    sim_sync: bool,
    sync_trace: bool,
    sync_after: bool,
    sync_slots: usize,
    sync_constant: u32,
    sync_defer_main: bool,
    pr_off: u32,
    sync_register: bool,
    ai_delay: u64,
    ai_period: u64,
    steps: u64,
    /// `--wipe SLOT@STEP`: at STEP set the life of every structure of SLOT to zero (test aid for the melee defeat check).
    wipe: Vec<(u32, u64)>,
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
            "--sim-sync" => args.sim_sync = true,
            "--sync-trace" => args.sync_trace = true,
            "--sync-after" => args.sync_after = true,
            "--sync-defer-main" => args.sync_defer_main = true,
            "--sync-register" => args.sync_register = true,
            "--pr-off" => args.pr_off = parse_seed(&value("--pr-off")?)?,
            "--ai-delay" => {
                let v = value("--ai-delay")?;
                args.ai_delay = v.parse().map_err(|_| format!("--ai-delay: not a number: {v}"))?;
            }
            "--ai-period" => {
                let v = value("--ai-period")?;
                args.ai_period = v.parse().map_err(|_| format!("--ai-period: not a number: {v}"))?;
            }
            "--sync-slots" => {
                let v = value("--sync-slots")?;
                args.sync_slots = v.parse().map_err(|_| format!("--sync-slots: not a number: {v}"))?;
            }
            "--sync-constant" => args.sync_constant = parse_seed(&value("--sync-constant")?)?,
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
            "--wipe" => {
                let v = value("--wipe")?;
                let (a, b) = v.split_once('@').ok_or("--wipe: expected SLOT@STEP")?;
                args.wipe.push((
                    a.parse().map_err(|_| "--wipe: bad slot".to_string())?,
                    b.parse().map_err(|_| "--wipe: bad step".to_string())?,
                ));
            }
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
    if args.sim_sync && args.boot.replay.is_none() {
        return Err("--sim-sync needs --replay FILE.w3g".to_string());
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
    if args.sim_sync {
        let path = args.boot.replay.as_deref().unwrap_or_default();
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let parsed = w3g::Replay::parse(&bytes).map_err(|e| format!("--replay {path}: {e}"))?;
        let inputs = cli::inputs(&args.boot)?;
        let (mut engine, setup) = engine::Engine::boot_from_replay(&bytes, inputs).map_err(|e| e.to_string())?;
        let opts = engine::sim_sync::Options {
            after_commands: args.sync_after,
            max_slots: args.sync_slots,
            trace: args.sync_trace,
            constant: args.sync_constant,
            defer_main: args.sync_defer_main,
            register_units: args.sync_register,
            draw_index: draw_index_for(&args, setup.seed),
        };
        engine.host().game_mut().set_pr_disabled(args.pr_off);
        engine.set_ai_cadence(args.ai_delay, args.ai_period);
        let mut report = engine::sim_sync::run(&mut engine, &parsed, &opts);
        report.notes.extend(setup.notes.iter().cloned());
        print!("{}", report.render());
        {
            let game = engine.host().game();
            let st = game.pr_stats();
            println!(
                "pr hooks: unit created {}  mover start {}  leg restart {}  idle {}  walk release {}  missile launch {}  unit removed {}",
                st[0], st[1], st[2], st[3], st[4], st[5], st[6]
            );
            for p in 0..2u32 {
                println!("player {p}: {}", game.player_report(p));
            }
            let mut table: std::collections::BTreeMap<(u32, u32), u32> = std::collections::BTreeMap::new();
            for &(k, v, _) in &game.kill_log {
                *table.entry((k, v)).or_default() += 1;
            }
            for ((k, v), n) in table {
                println!("kills: {k} -> {v}: {n}");
            }
            let mut by_owner: std::collections::BTreeMap<(u32, String), u32> = std::collections::BTreeMap::new();
            for (_, u) in game.unit_objects() {
                if !u.dead && !u.removed {
                    *by_owner.entry((u.owner, format!("{:08x}", u.type_id))).or_default() += 1;
                }
            }
            for ((o, t), n) in by_owner {
                println!("units: p{o} {t} x{n}");
            }
        }
        return Ok(());
    }
    let mut engine = cli::boot(&args.boot)?;
    let mut last_pvp = 0usize;
    for i in 0..args.steps {
        engine.step();
        for &(slot, at) in &args.wipe {
            if i == at {
                let n = engine.host_mut().kill_units_zero_life(slot);
                println!("wipe: {n} structures of p{slot} set to zero life at step {i}");
            }
        }
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
        // Game results (RemovePlayer) and living structures per player.
        {
            let game = engine.host().game();
            for p in 0..12u32 {
                let structures = game
                    .unit_objects()
                    .filter(|(_, u)| u.owner == p && !u.dead && !u.removed && game.unit_types().is_building(u.type_id))
                    .count();
                let result = game.lane.result_of(p);
                if structures > 0 || result != 3 || args.boot.computers.iter().any(|&(s, _)| s == p) {
                    println!(
                        "result p{p}: game result {} (0 victory, 1 defeat, 2 tie, 3 none), slot state {}, structures {structures}",
                        result,
                        game.player(p).slot_state
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

/// Hash of the global generator after n draws from `seed`, for n below 400,000 (the trace shows n).
fn draw_index_for(args: &Args, seed: u32) -> std::collections::HashMap<u32, u32> {
    let mut map = std::collections::HashMap::new();
    let Some(path) = args.boot.dll.as_deref() else { return map };
    let Ok(bytes) = std::fs::read(path) else { return map };
    let Ok(table) = sim::rng::RngTable::from_game_dll(&bytes) else { return map };
    let mut rng = sim::rng::Rng::seeded(seed);
    for n in 0..400_000u32 {
        map.entry(sim::sync::rng_state_hash(&rng)).or_insert(n);
        rng.next_u32(&table);
    }
    map
}
