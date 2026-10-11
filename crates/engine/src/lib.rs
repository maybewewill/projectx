//! Boots a Warcraft III 1.26a map the way the game does, on top of the existing crates.
//!
//! Inputs: the map (`.w3m` / `.w3x`, an MPQ with the 512-byte header), the user's `Game.dll`
//! (optional: it provides the soft-float lookup tables and the RNG permutation table), and the
//! `common.j` and `blizzard.j` sources (paths, or extracted from `War3.mpq` / `War3x.mpq`).
//!
//! Boot order ([`Engine::boot`]):
//!
//! 1. Read `war3map.w3i` (player slots, playable area, names) and `war3map.j`.
//! 2. Parse `common.j`, `blizzard.j`, `war3map.j` in that order with `jass::parse_with` (the
//!    tables give the original's bits for real literals), check and compile them as one program.
//! 3. Create the [`sim::ScriptHost`]. Map setup (playable area, player slots, local player) is applied
//!    before any script code runs. With a `Game.dll`, the RNG table is handed to the host and the
//!    generator is seeded with [`Inputs::rng_seed`] (game start, `sim::Streams::start_game`).
//! 4. Create the AI host ([`sim::ai::AiHost`]) over the same game. The main VM is built with the AI natives
//!    of `sim::natives::ai::register_main`. The AI library holds the `common.ai` script and the race scripts
//!    from [`Inputs`], and `common.j` is compiled in front of `common.ai` in every AI VM.
//! 5. Run `config` synchronously, then queue `main` as a thread that starts on the first
//!    [`Engine::step`]. UNSURE: in the real game `config` runs when the map is loaded (lobby to
//!    game start), and `main` runs at game start, where it calls `InitBlizzard`, `InitGlobals`,
//!    `InitCustomTriggers` and `RunInitializationTriggers`. Both are also callable separately
//!    ([`Engine::run_config`], [`Engine::start_main`]).
//!
//! [`Engine::step_ticks`] advances the game clock one game tick (0.005 s, one soft-float add) at a
//! time, with the timer pump after each tick (`sim::ScriptHost::step_ticks`). Unit movement is a timer
//! in that pump: the walk fires every 0.03 s of game time (`sim::lockstep::MOVE_PERIOD_BITS`, lockstep.md
//! section 11.2). Every [`TICKS_PER_BATCH`] ticks (100 ms, UNSURE cadence, lockstep.md sections 11.5-11.7)
//! the main host runs its combat tick and scheduler pass, and then the AI host runs once at the same game
//! time (`sim::ai::AiHost::step`). [`Engine::step`] is one batch.
//!
//! Missing natives: a native that is declared but has no implementation is reported, and a
//! script call to it aborts the calling thread (`RunStatus::FunctionNotFound`). The report lists
//! the natives that compiled code calls and that are not implemented, sorted by name.
//!
//! Nothing from the game is committed: the tests build synthetic archives in memory.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::rc::Rc;

use jass::bytecode::Op;
use jass::compile::{compile, CompileError};
use jass::vm::{NativeRegistry, RunStatus, ThreadId, VmError};
use mpq::Archive;
use sim::ai::{AiEvent, AiHost, AiLibrary};
use sim::commands::{Command, CommandState, Outcome};
use sim::lockstep::{TurnReport, TICKS_PER_BATCH};
use sim::{Game, RngTable, ScriptHost};
use softfloat::{Tables, F32};
use w3map::MapInfo;

pub mod data;
pub mod map;
pub mod pathing;
pub mod replay;
pub mod scripts;

pub use map::{apply_players, controller_of, playable_area};
pub use pathing::PathingSummary;
pub use scripts::{
    ai_scripts_from_archives, scripts_from_archives, AiSources, BLIZZARD_J, COMMON_J,
};

/// Errors while booting a map.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("archive {index}: {source}")]
    Archive { index: usize, source: mpq::Error },
    #[error("{what}: {name} not found in the archives")]
    MissingFile { what: &'static str, name: String },
    #[error("war3map.w3i: {0}")]
    Info(#[from] w3map::Error),
    #[error("{file}: {source}")]
    Parse {
        file: &'static str,
        source: jass::Error,
    },
    #[error("script compile: {0}")]
    Compile(CompileError),
    #[error("script VM: {0}")]
    Vm(#[from] VmError),
    #[error("Game.dll tables: {0:?}")]
    Tables(softfloat::TablesError),
    #[error("Game.dll RNG table: {0:?}")]
    Rng(sim::RngError),
}

/// The inputs of [`Engine::boot`].
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// The map file bytes.
    pub map: Vec<u8>,
    /// The user's `Game.dll`. Without it the soft-float tables are missing: real division and the
    /// table-based natives stop their thread, and real literals are only correctly rounded.
    pub game_dll: Option<Vec<u8>>,
    /// The `common.j` source.
    pub common_j: String,
    /// The `blizzard.j` source.
    pub blizzard_j: String,
    /// The raw game-start seed `S` for the random generator (`GetRandomInt` and the streams are
    /// seeded from it, see `sim::Streams::start_game`). The replay's random seed field is the
    /// intended source. UNSURE: the original computes `S` as a rotate-and-xor fold over the player
    /// slots (`docs/spec/rng.md` section 4.1), which is not implemented, so the caller supplies `S`.
    /// Only used when `game_dll` is given (the generator needs its table).
    pub rng_seed: u32,
    /// The `common.ai` source (the common AI script of the AI VMs), or `None`. Without it an AI VM
    /// is built from `common.j` and the named script alone, so a script that uses the AI natives
    /// declared in `common.ai` fails to compile (`StartMeleeAI` then reports a build failure).
    pub ai_common: Option<String>,
    /// The named AI scripts, as `(name, source)`, for example `("human.ai", source)`. A
    /// `StartMeleeAI(player, "human.ai")` runs the script with that name (case-insensitive). A later
    /// entry with the same name replaces an earlier one.
    pub ai_scripts: Vec<(String, String)>,
    /// The user's game archives in load order (`War3.mpq`, then `War3x.mpq`). They give the `[Misc]` layers
    /// (`Units\MiscData.txt`, `Units\MiscGame.txt`) and the unit rows (`Units\UnitBalance.slk`,
    /// `Units\UnitWeapons.slk`). Empty: the Misc values and unit rows are defaults and the damage stays the placeholder.
    pub game_archives: Vec<Vec<u8>>,
}

/// A booted map: the script host, the map info and the boot results.
pub struct Engine {
    host: ScriptHost,
    ai: AiHost,
    info: MapInfo,
    rng: Option<RngTable>,
    functions: usize,
    globals: usize,
    natives_declared: usize,
    natives_referenced: usize,
    missing_natives: Vec<String>,
    pathing: Option<PathingSummary>,
    config_status: String,
    main_status: String,
    /// Batches run (each one is [`TICKS_PER_BATCH`] ticks; the AI host runs once per batch).
    steps: u64,
    /// Game ticks run.
    ticks: u64,
    /// `main` is queued for the first step (set at boot, started by the first [`Engine::step_ticks`]).
    main_pending: bool,
    timers_fired: u64,
    notes: Vec<String>,
    combat: data::CombatSetup,
    /// Replay command state: selections, hotkey groups, the object table and the speed setting.
    commands: CommandState,
}

/// What the AI host did, as the report shows it (see [`sim::ai::AiShared`] for the full logs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiReport {
    /// The players that have an AI VM, ascending.
    pub started: Vec<u32>,
    /// The start events, in order (refused starts, build failures, started AIs).
    pub events: Vec<AiEvent>,
    /// AI-table natives that were called but are not implemented: `(name, calls)`, sorted by name.
    pub unimplemented: Vec<(String, u32)>,
    /// Entries in the AI command log (calls the AI VMs made that the model records).
    pub logged_calls: usize,
    /// AI threads that aborted, with the player that ran them.
    pub failures: Vec<(u32, RunStatus)>,
}

/// What the report shows for one player slot (from `w3i`, with the state the host set).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerLine {
    pub slot: u32,
    pub name: String,
    pub race: u32,
    pub controller: u32,
    pub slot_state: u32,
    pub team: i32,
    pub start: (f32, f32),
}

/// A summary of a booted engine (see the `Display` impl for the printed form).
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub map_name: String,
    pub author: String,
    pub playable_width: u32,
    pub playable_height: u32,
    pub players: Vec<PlayerLine>,
    pub functions: usize,
    pub globals: usize,
    pub natives_declared: usize,
    pub natives_referenced: usize,
    pub natives_missing: Vec<String>,
    pub config: String,
    pub main: String,
    /// Batches run (see [`Engine::step_ticks`]).
    pub steps: u64,
    /// Game ticks run (0.005 s each).
    pub ticks: u64,
    pub timers_fired: u64,
    pub game_time: f32,
    /// The pathing data installed at boot, or `None` (straight-line movement).
    pub pathing: Option<PathingSummary>,
    /// The combat setup: the damage model in use (and why the placeholder is kept, if it is), and the unit rows.
    pub combat: data::CombatSetup,
    /// The AI host's summary (no AI players when no AI was started).
    pub ai: AiReport,
    pub notes: Vec<String>,
}

impl Engine {
    /// Boots the map from `inputs` (see the module documentation for the order).
    ///
    /// `config` runs at boot. A failure there (for example a missing native) does not fail the
    /// boot: it is recorded in [`Report::config`] and the rest still runs.
    pub fn boot(inputs: Inputs) -> Result<Engine, Error> {
        let mut notes = Vec::new();
        let (tables, rng) = match &inputs.game_dll {
            Some(dll) => (
                Some(Tables::from_game_dll(dll).map_err(Error::Tables)?),
                Some(RngTable::from_game_dll(dll).map_err(Error::Rng)?),
            ),
            None => {
                notes.push("no Game.dll: soft-float tables and RNG table are missing".to_string());
                (None, None)
            }
        };

        let archive =
            Archive::open(&inputs.map).map_err(|source| Error::Archive { index: 0, source })?;
        let info_bytes = read_map_file(&archive, &["war3map.w3i"], "war3map.w3i")?;
        let info = MapInfo::parse(&info_bytes)?;
        let war3map = read_map_file(&archive, &["war3map.j", "scripts\\war3map.j"], "war3map.j")?;
        let war3map = String::from_utf8_lossy(&war3map).into_owned();
        let wpm = match archive.read("war3map.wpm") {
            Ok(data) => Some(data),
            Err(mpq::Error::NotFound(_)) => None,
            Err(source) => return Err(Error::Archive { index: 0, source }),
        };

        let programs = [
            ("common.j", inputs.common_j.as_str()),
            ("blizzard.j", inputs.blizzard_j.as_str()),
            ("war3map.j", war3map.as_str()),
        ]
        .iter()
        .map(|&(file, src)| {
            jass::parse_with(src, tables.as_ref()).map_err(|source| Error::Parse { file, source })
        })
        .collect::<Result<Vec<_>, _>>()?;
        let module = compile(&programs).map_err(Error::Compile)?;

        let functions = module.functions.len();
        let globals = module.globals.len();
        let natives_declared = module.natives.len();

        // The main VM's AI natives (StartMeleeAI, CommandAI, ...) share the game and the AI state with the AI host.
        let game = Rc::new(RefCell::new(Game::new()));
        let ai_shared = sim::ai::new_shared();
        let mut natives = NativeRegistry::with_builtins();
        sim::natives::ai::register_main(&mut natives, &game, &ai_shared);
        let host = ScriptHost::with_game(module, natives, tables.clone(), Rc::clone(&game))?;
        let mut library = AiLibrary::new(inputs.ai_common.as_deref().unwrap_or(""));
        if inputs.ai_common.is_none() {
            notes.push("no common.ai: AI VMs are built without it".to_string());
        }
        library.set_prelude(&inputs.common_j);
        for (name, source) in &inputs.ai_scripts {
            library.add_script(name, source);
        }
        let ai = AiHost::new(game, ai_shared, library, tables.clone());
        let playable = playable_area(&info);
        host.set_playable(Some(playable));
        apply_players(&mut host.game_mut(), &info, &mut notes);
        let pathing = match &wpm {
            Some(wpm) => pathing::install(
                &mut host.game_mut(),
                wpm,
                (playable.min_x, playable.min_y),
                (info.playable_width, info.playable_height),
                &mut notes,
            ),
            None => {
                notes.push("no war3map.wpm: units move in straight lines".to_string());
                None
            }
        };
        if let Some(table) = &rng {
            host.set_rng_table(table.clone());
            // Game start: the global generator and the streams are seeded from the game seed.
            host.start_game_rng(inputs.rng_seed);
            notes.push(format!(
                "RNG seeded with the raw game seed {:#010x} (slot fold not implemented)",
                inputs.rng_seed
            ));
        }

        let (natives_referenced, natives_missing) = native_usage(host.vm().module(), &host);

        let game_refs: Vec<&[u8]> = inputs.game_archives.iter().map(Vec::as_slice).collect();
        let game_data = data::load(&game_refs, &archive, tables.as_ref())?;
        notes.extend(game_data.notes.iter().cloned());
        let combat = data::install(&mut host.game_mut(), &game_data, tables.as_ref());

        let mut engine = Engine {
            host,
            info,
            rng,
            functions,
            globals,
            natives_declared,
            natives_referenced,
            missing_natives: natives_missing,
            pathing,
            config_status: String::new(),
            main_status: String::new(),
            main_pending: false,
            steps: 0,
            ticks: 0,
            timers_fired: 0,
            notes,
            ai,
            combat,
            commands: CommandState::new(),
        };
        // Failures are kept in the status lines of the report, so the boot itself succeeds.
        let _ = engine.run_config();
        // `main` waits for the first step, when the host is set up (UNSURE: the real game starts it at
        // game start). Starting it at boot would run it before the caller can add unit types to the game.
        engine.main_pending = true;
        engine.main_status = "queued for the first step".to_string();
        Ok(engine)
    }

    /// Runs `config` now, synchronously (UNSURE: the real game runs it at map load). A `config` that
    /// sleeps is discarded by the synchronous call and reported as an error.
    pub fn run_config(&mut self) -> Result<(), RunStatus> {
        let result = self.host.vm_mut().call("config", &[]).map(|_| ());
        self.config_status = describe(&result);
        result
    }

    /// Starts `main` as a thread and runs it at once, until its first stop (lockstep.md section 12.1;
    /// UNSURE: the real game starts `main` at game start). Calling it twice runs `main` twice.
    pub fn start_main(&mut self) -> Result<ThreadId, RunStatus> {
        let result = self.host.start("main", &[]);
        self.main_status = match &result {
            Ok(id) => format!("started as thread {}", id.0),
            Err(e) => format!("not started: {e:?}"),
        };
        result
    }

    /// Advances the game by one batch of [`TICKS_PER_BATCH`] ticks (100 ms of game time). Same as
    /// [`Engine::step_ticks`] with that count. Returns the number of queue entries of the main host
    /// that fired.
    pub fn step(&mut self) -> usize {
        self.step_ticks(TICKS_PER_BATCH)
    }

    /// Advances the game by `n` game ticks (0.005 s each). The main host runs every tick (clock add,
    /// timer pump). After each batch boundary the AI host runs at the same game time, after the main
    /// host's batch work. A replay passes `TurnReport::game_ticks` here, after applying the turn's
    /// commands. Returns the number of queue entries of the main host that fired.
    pub fn step_ticks(&mut self, n: u64) -> usize {
        if self.main_pending {
            self.main_pending = false;
            let _ = self.start_main();
        }
        let mut fired = 0usize;
        let mut left = n;
        while left > 0 {
            let k = left.min(self.host.ticks_to_batch());
            fired += self.host.step_ticks(k);
            left -= k;
            if self.host.at_batch_boundary() {
                let now = F32::from_bits(self.host.clock().now().to_bits());
                self.ai.step(now);
                self.steps += 1;
            }
        }
        self.ticks = self.host.ticks();
        self.timers_fired += fired as u64;
        fired
    }

    /// Runs one replay turn: applies its commands in stream order, then runs the game ticks it produced
    /// (lockstep.md section 3; see [`sim::lockstep::TurnScheduler`]). Returns the timers fired by the ticks.
    pub fn run_turn(&mut self, turn: &TurnReport<Command>) -> usize {
        self.apply_commands(&turn.actions);
        self.step_ticks(turn.game_ticks)
    }

    /// Applies commands in the given order and returns their outcomes (one per command).
    pub fn apply_commands(&mut self, commands: &[(u8, Command)]) -> Vec<Outcome> {
        let mut game = self.host.game_mut();
        commands
            .iter()
            .map(|(_, cmd)| self.commands.apply(&mut game, cmd))
            .collect()
    }

    /// The replay command state (selections, hotkey groups, the object table, the speed setting).
    pub fn commands(&self) -> &CommandState {
        &self.commands
    }

    /// Mutable access to the replay command state (to register the object pairs of units).
    pub fn commands_mut(&mut self) -> &mut CommandState {
        &mut self.commands
    }

    /// The script host (VM, clock, timers and game state).
    pub fn host(&self) -> &ScriptHost {
        &self.host
    }

    /// Mutable access to the script host.
    pub fn host_mut(&mut self) -> &mut ScriptHost {
        &mut self.host
    }

    /// The AI host (one JASS VM per AI player, and the shared AI state).
    pub fn ai(&self) -> &AiHost {
        &self.ai
    }

    /// Mutable access to the AI host.
    pub fn ai_mut(&mut self) -> &mut AiHost {
        &mut self.ai
    }

    /// The map info read from `war3map.w3i`.
    pub fn map_info(&self) -> &MapInfo {
        &self.info
    }

    /// The RNG permutation table, if a `Game.dll` was given. The host has its own copy for the
    /// random natives.
    pub fn rng_table(&self) -> Option<&RngTable> {
        self.rng.as_ref()
    }

    /// Notes from the boot (things that were skipped or are not known).
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// The summary of the boot and of the steps run so far.
    pub fn report(&self) -> Report {
        let game = self.host.game();
        let players = self
            .info
            .players
            .iter()
            .filter(|p| (p.number as usize) < sim::natives::game::PLAYER_SLOTS)
            .map(|p| {
                let s = game.player(p.number);
                PlayerLine {
                    slot: p.number,
                    name: s.name.clone(),
                    race: s.race,
                    controller: s.controller,
                    slot_state: s.slot_state,
                    team: s.team,
                    start: (p.start_x, p.start_y),
                }
            })
            .collect();
        Report {
            map_name: self.info.name.clone(),
            author: self.info.author.clone(),
            playable_width: self.info.playable_width,
            playable_height: self.info.playable_height,
            players,
            functions: self.functions,
            globals: self.globals,
            natives_declared: self.natives_declared,
            natives_referenced: self.natives_referenced,
            natives_missing: self.missing_natives.clone(),
            config: self.config_status.clone(),
            main: self.main_status.clone(),
            steps: self.steps,
            ticks: self.ticks,
            timers_fired: self.timers_fired,
            game_time: self.host.clock().now(),
            pathing: self.pathing.clone(),
            combat: self.combat.clone(),
            ai: self.ai_report(),
            notes: self.notes.clone(),
        }
    }

    fn ai_report(&self) -> AiReport {
        let shared = self.ai.shared().borrow();
        AiReport {
            started: self.ai.players(),
            events: shared.events.clone(),
            unimplemented: shared
                .unimplemented
                .iter()
                .map(|(name, calls)| ((*name).to_string(), *calls))
                .collect(),
            logged_calls: shared.commands.len(),
            failures: self
                .ai
                .failures()
                .iter()
                .map(|(player, _, status)| (*player, *status))
                .collect(),
        }
    }
}

fn describe(result: &Result<(), RunStatus>) -> String {
    match result {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("failed: {e:?}"),
    }
}

/// Reads the first of `names` that the map has.
fn read_map_file(
    archive: &Archive<'_>,
    names: &[&str],
    what: &'static str,
) -> Result<Vec<u8>, Error> {
    for name in names {
        match archive.read(name) {
            Ok(data) => return Ok(data),
            Err(mpq::Error::NotFound(_)) => {}
            Err(source) => return Err(Error::Archive { index: 0, source }),
        }
    }
    Err(Error::MissingFile {
        what,
        name: names.join(" or "),
    })
}

/// The number of natives that compiled code calls, and the sorted names of those with no
/// implementation. A native is "referenced" when any compiled function has a `CallNative` to it.
fn native_usage(module: &jass::compile::Module, host: &ScriptHost) -> (usize, Vec<String>) {
    let mut referenced = BTreeSet::new();
    for f in &module.functions {
        for ins in &f.code {
            if ins.op == Op::CallNative {
                referenced.insert(ins.imm as usize);
            }
        }
    }
    let unbound: BTreeSet<&str> = host.vm().unbound_natives().into_iter().collect();
    let missing = referenced
        .iter()
        .filter_map(|&i| module.natives.get(i))
        .map(|def| def.name.as_str())
        .filter(|name| unbound.contains(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect();
    (referenced.len(), missing)
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "map: {:?} by {:?}", self.map_name, self.author)?;
        writeln!(
            f,
            "playable size (w3i): {} x {}",
            self.playable_width, self.playable_height
        )?;
        match &self.pathing {
            Some(p) => writeln!(
                f,
                "pathing: {} x {} cells (32 units each), origin ({:.1}, {:.1})",
                p.width, p.height, p.origin.0, p.origin.1
            )?,
            None => writeln!(f, "pathing: none")?,
        }
        writeln!(f, "players ({}):", self.players.len())?;
        for p in &self.players {
            writeln!(
                f,
                "  slot {:2}: {:?} race={} controller={} state={} team={} start=({:.1}, {:.1})",
                p.slot, p.name, p.race, p.controller, p.slot_state, p.team, p.start.0, p.start.1
            )?;
        }
        writeln!(
            f,
            "scripts: {} functions, {} globals, {} natives declared, {} referenced",
            self.functions, self.globals, self.natives_declared, self.natives_referenced
        )?;
        writeln!(
            f,
            "natives referenced but not implemented ({}):",
            self.natives_missing.len()
        )?;
        for name in &self.natives_missing {
            writeln!(f, "  {name}")?;
        }
        writeln!(f, "damage model: {}", self.combat.damage_model)?;
        writeln!(f, "unit types with combat rows: {}", self.combat.unit_types)?;
        writeln!(f, "config: {}", self.config)?;
        writeln!(f, "main: {}", self.main)?;
        writeln!(
            f,
            "ticks: {} (batches: {}, game time {:.2} s), timer and sleep entries fired: {}",
            self.ticks, self.steps, self.game_time, self.timers_fired
        )?;
        if self.ai.started.is_empty() {
            writeln!(f, "AI players started: none")?;
        } else {
            let list: Vec<String> = self.ai.started.iter().map(u32::to_string).collect();
            writeln!(
                f,
                "AI players started ({}): {}",
                self.ai.started.len(),
                list.join(", ")
            )?;
        }
        for event in &self.ai.events {
            writeln!(f, "AI event: {event:?}")?;
        }
        writeln!(
            f,
            "AI natives called but not implemented ({}):",
            self.ai.unimplemented.len()
        )?;
        for (name, calls) in &self.ai.unimplemented {
            writeln!(f, "  {name} x{calls}")?;
        }
        writeln!(
            f,
            "AI: {} logged calls, {} aborted threads",
            self.ai.logged_calls,
            self.ai.failures.len()
        )?;
        for (player, status) in &self.ai.failures {
            writeln!(f, "AI thread failed (player {player}): {status:?}")?;
        }
        for note in &self.notes {
            writeln!(f, "note: {note}")?;
        }
        Ok(())
    }
}
