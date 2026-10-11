//! AI host: one JASS VM per computer player, stepped after the main VM on every 0.1 s tick.
//!
//! Model (see the native notes in `natives/ai.rs` for the per-native status):
//!
//! * The main script starts a player's AI with `StartMeleeAI` or `StartCampaignAI`. The native marks the player as
//!   AI-controlled and queues a [`StartRequest`] in [`AiShared`]. It does not build the VM itself, because it runs
//!   inside the main VM's step. [`AiHost::step`] builds the VM at the start of the next step (the same tick, after
//!   the main VM).
//! * The AI source comes from the host: the common AI script (the user's `common.ai`) plus the named script
//!   (`AiLibrary`). Both are parsed and compiled as one module, one module per AI VM, so every AI VM has its own
//!   globals and threads.
//! * Each AI VM gets the world natives (units, players, groups, items, ...) registered on the shared [`Game`], without
//!   the timer and trigger natives, because their expiries run on the main host's queue. Its sleeps are the VM's own
//!   sleeps (`NativeResult::Sleep`), resumed by [`Vm::step`] at the time the AI host passes in.
//! * `CommandAI` from the main script pushes a `(command, data)` pair onto the AI player's command stack (CONFIRMED).
//!   The AI reads the newest pair first with `GetLastCommand` and `GetLastData` (-1 when empty), counts them with
//!   `CommandsWaiting` and drops the newest with `PopLastCommand`.
//!
//! What the binary shows for the AI VM (decompile of `FUN_6f2db0f0`, `FUN_6f2db4b0`, `FUN_6f2da590`, `FUN_6f2d0f90`):
//!
//! * The start loads `common.j`, `common.ai` and the race script into one VM for the player and starts one thread with
//!   code 0 (CONFIRMED: one `StartThread` call on the start path). The model starts `config` and then `main`. Whether
//!   the engine runs `config` is UNSURE, and the host compiles `common.j` before `common.ai` when the library has a prelude (`AiLibrary::set_prelude`).
//! * Each AI has six thread slots (`+0x254`, index 0 to 5, LIKELY): a `StartThread` takes a free slot and is dropped when
//!   none is free. The model does not cap threads (the freeing rule is UNSURE).
//! * `Sleep` passes its duration to the thread's agent timer (`FUN_6f2d0f90`, a virtual call with the duration). The
//!   AI dispatcher `FUN_6f2db4b0` runs the AI agents in turn under a budget that starts at 5000 ms and schedules the
//!   next run at the agent's time plus 100 ms (LIKELY: the AI cycle is at least 0.1 s; the exact base is UNSURE).
//!
//! Ordering (UNSURE against the original; the list walk direction of the dispatcher is not decoded): the main VM runs
//! first in a tick, then the AI VMs run in ascending player index, each once with `Vm::step` at the same time, every
//! 0.1 s. Start requests are built before the first AI step. A player whose pause counter is above zero
//! (`PauseCompAI`) is skipped (UNSURE: the skip is inferred from the counter, not read).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use jass::compile::Module;
use jass::vm::{NativeRegistry, RunStatus, ThreadId, Vm};
use softfloat::{Tables, F32};

use crate::natives::game::Game;

/// The most commands an AI queue holds. A `CommandAI` when the queue is full is dropped (CONFIRMED: the push
/// checks `count != 12`, decompile `FUN_6f2d4990`).
pub const COMMAND_QUEUE_LIMIT: usize = 12;

/// The kind of AI a player runs: `StartMeleeAI` or `StartCampaignAI`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartKind {
    Melee,
    Campaign,
}

/// A value that an AI native received, kept for the command log.
#[derive(Debug, Clone, PartialEq)]
pub enum AiArg {
    Null,
    Int(i32),
    Real(F32),
    Bool(bool),
    Str(String),
    Handle(u32),
    Code(u32),
}

/// One entry of the deterministic AI command log: a native call that the model records without acting on it.
#[derive(Debug, Clone, PartialEq)]
pub struct AiCommand {
    /// The player whose AI made the call.
    pub player: u32,
    /// The native's name.
    pub name: &'static str,
    /// The arguments, in call order.
    pub args: Vec<AiArg>,
}

/// A request from `StartMeleeAI` or `StartCampaignAI`, served by [`AiHost::step`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRequest {
    pub player: u32,
    pub script: String,
    pub kind: StartKind,
}

/// What the AI host did, in order. Read with [`AiShared::events`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiEvent {
    /// A start was refused by the main VM (bad player, or the player already runs an AI).
    Refused {
        player: u32,
        script: String,
        reason: &'static str,
    },
    /// The AI VM for `player` was created.
    Started {
        player: u32,
        script: String,
        kind: StartKind,
    },
    /// The VM could not be built: the script is missing, or the source does not parse or compile.
    BuildFailed {
        player: u32,
        script: String,
        error: String,
    },
    /// The script has no `main` function, so the AI runs only its `config` function.
    NoMain { player: u32 },
}

/// The per-player AI state that the main VM and the AI VMs share.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiControl {
    /// Set by `StartMeleeAI` / `StartCampaignAI` (the player runs an AI from then on).
    pub active: bool,
    /// The kind of the start that set `active`.
    pub kind: Option<StartKind>,
    /// `PauseCompAI` counter: `+1` for `true`, `-1` (not below zero) for `false`. An AI with a count above zero is
    /// not stepped (UNSURE).
    pub pause: i32,
    /// `(command, data)` pairs that `CommandAI` queued, oldest first, at most [`COMMAND_QUEUE_LIMIT`]. The AI reads
    /// the newest pair first: the queue is a stack (CONFIRMED: `FUN_6f2d4990` pushes at the count, `GetLastCommand`
    /// reads the entry at the count, `PopLastCommand` lowers the count).
    pub queue: Vec<(i32, i32)>,
    /// `CommandAI` calls dropped because the queue was full.
    pub dropped_commands: u32,
    /// The AI state's flag word (`+0x2D0`): the bits that the boolean setters change. See `natives::ai::FLAG_BITS`.
    pub flags: u32,
    /// The captain record's flag word (`+0x6C`): bit 1 full, bit 2 retreating, `0x4000` pathing disabled, `0x8000`
    /// amphibious (CONFIRMED bits, as the natives and the captain queries read them).
    pub captain_flags: u32,
    /// Values stored by the AI's setter natives that are not flag bits (code and integer settings), by native name.
    pub settings: BTreeMap<&'static str, AiArg>,
    /// The alliance target handle (`SetAllianceTarget`), 0 when none.
    pub alliance_target: u32,
    /// The attack captain's group size (captain record `+0xBC`, read by `CaptainIsEmpty` and `CaptainGroupSize`).
    /// Nothing in the sim forms captain groups yet, so it stays 0 unless a host sets it (UNSURE: the writer of the
    /// field was not found).
    pub captain_group_size: i32,
    /// Whether the captain has an enemy reference (`0x6F2D0A00`: either of the two refs at `+0x114`, `+0x120`
    /// resolves), by captain: index 1 is the attack captain (`CaptainInCombat(true)`), index 0 the defense captain.
    /// No combat tracking sets it (UNSURE).
    pub captain_in_combat: [bool; 2],
    /// Whether the player's threat reference (player record `+0x164`) resolves (`TownThreatened`). No threat scan sets
    /// it (UNSURE).
    pub town_threatened: bool,
}

/// The state shared by the main VM's AI natives, the AI VMs and the host.
#[derive(Debug, Default)]
pub struct AiShared {
    /// Per-player control state, by player index (ascending).
    pub controls: BTreeMap<u32, AiControl>,
    /// Start requests not yet served by [`AiHost::step`].
    pub requests: Vec<StartRequest>,
    /// The command log, in call order.
    pub commands: Vec<AiCommand>,
    /// Calls to natives that are listed as not implemented, by name, with the number of calls.
    pub unimplemented: BTreeMap<&'static str, u32>,
    /// Start and build events, in order.
    pub events: Vec<AiEvent>,
    /// The town sites and town slots (`ai_towns.rs`).
    pub towns: crate::ai_towns::Towns,
}

/// The shared AI state, as the natives and the host hold it.
pub type SharedAi = Rc<RefCell<AiShared>>;

/// A new, empty shared AI state.
pub fn new_shared() -> SharedAi {
    Rc::new(RefCell::new(AiShared::default()))
}

impl AiShared {
    /// The control state of `player`, if it was ever touched.
    pub fn control(&self, player: u32) -> Option<&AiControl> {
        self.controls.get(&player)
    }

    /// The control state of `player`, created on first use.
    pub fn control_mut(&mut self, player: u32) -> &mut AiControl {
        self.controls.entry(player).or_default()
    }

    /// Whether `player` runs an AI (a start was accepted for it).
    pub fn is_active(&self, player: u32) -> bool {
        self.control(player).is_some_and(|c| c.active)
    }
}

/// The AI script sources the host provides: the common script and the named scripts. Names are case-insensitive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiLibrary {
    prelude: String,
    common: String,
    scripts: BTreeMap<String, String>,
    /// The natives the game has registered when it compiles an AI script (common + AI natives). When set,
    /// `native` declarations are checked against it like the original does.
    natives: Option<jass::check::NativeRegistry>,
}

impl AiLibrary {
    /// A library whose common script is `common` (the user's `common.ai`).
    pub fn new(common: &str) -> Self {
        AiLibrary {
            prelude: String::new(),
            common: common.to_string(),
            scripts: BTreeMap::new(),
            natives: None,
        }
    }

    /// Checks every `native` declaration of the compiled AI scripts against `registry` (see
    /// `jass::check::CheckOptions::natives`).
    pub fn set_native_registry(&mut self, registry: jass::check::NativeRegistry) {
        self.natives = Some(registry);
    }

    /// The native registry, when one was set.
    pub fn native_registry(&self) -> Option<&jass::check::NativeRegistry> {
        self.natives.as_ref()
    }

    /// Sets the source compiled before the common script in every AI VM: the map's `common.j`, which declares the
    /// natives and types that `common.ai` and the race scripts use (the AI VM is built from `common.j`,
    /// `common.ai` and the named script). Empty by default, which compiles the common script and the named script
    /// alone.
    pub fn set_prelude(&mut self, source: &str) {
        self.prelude = source.to_string();
    }

    /// Adds (or replaces) the named script `name`, for example `human.ai`.
    pub fn add_script(&mut self, name: &str, source: &str) {
        self.scripts
            .insert(name.to_ascii_lowercase(), source.to_string());
    }

    /// The source of the named script.
    pub fn script(&self, name: &str) -> Option<&str> {
        self.scripts
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// The common script source.
    pub fn common(&self) -> &str {
        &self.common
    }

    /// The prelude source (`common.j`), empty when none was set.
    pub fn prelude(&self) -> &str {
        &self.prelude
    }
}

/// Parses and compiles the prelude (when not empty), the common script and one named script as one module.
fn build_module(
    prelude: &str,
    common: &str,
    script: &str,
    tables: Option<&Tables>,
    natives: Option<&jass::check::NativeRegistry>,
) -> Result<Module, String> {
    let mut programs = Vec::with_capacity(3);
    if !prelude.is_empty() {
        programs.push(jass::parse_with(prelude, tables).map_err(|e| format!("prelude: {e:?}"))?);
    }
    programs.push(jass::parse_with(common, tables).map_err(|e| format!("common: {e:?}"))?);
    programs.push(jass::parse_with(script, tables).map_err(|e| format!("script: {e:?}"))?);
    let opts = jass::check::CheckOptions {
        lenient_returns: false,
        natives: natives.cloned(),
    };
    jass::compile::compile_with(&programs, &opts).map_err(|e| format!("{e:?}"))
}

/// Runs one JASS VM per AI player, stepped after the main VM.
pub struct AiHost {
    game: Rc<RefCell<Game>>,
    shared: SharedAi,
    library: AiLibrary,
    tables: Option<Tables>,
    vms: BTreeMap<u32, Vm>,
    failures: Vec<(u32, ThreadId, RunStatus)>,
}

impl AiHost {
    /// Creates the host over the main VM's `game` (see `ScriptHost::with_game`) and the shared AI state that the
    /// main VM's AI natives were registered with (`natives::ai::register_main`).
    pub fn new(
        game: Rc<RefCell<Game>>,
        shared: SharedAi,
        library: AiLibrary,
        tables: Option<Tables>,
    ) -> Self {
        AiHost {
            game,
            shared,
            library,
            tables,
            vms: BTreeMap::new(),
            failures: Vec::new(),
        }
    }

    /// The AI VM of `player`, if one was built.
    pub fn vm(&self, player: u32) -> Option<&Vm> {
        self.vms.get(&player)
    }

    /// Mutable access to the AI VM of `player` (for a host that wants to resume or inspect it).
    pub fn vm_mut(&mut self, player: u32) -> Option<&mut Vm> {
        self.vms.get_mut(&player)
    }

    /// The players that have an AI VM, ascending.
    pub fn players(&self) -> Vec<u32> {
        self.vms.keys().copied().collect()
    }

    /// Threads of the AI VMs that aborted, with the player that ran them, in the order they failed.
    pub fn failures(&self) -> &[(u32, ThreadId, RunStatus)] {
        &self.failures
    }

    /// The shared AI state (command log, queues, events).
    pub fn shared(&self) -> &SharedAi {
        &self.shared
    }

    /// The game the AI natives read (the main VM's game).
    pub fn game(&self) -> &Rc<RefCell<Game>> {
        &self.game
    }

    /// The main clock restarted: every sleeping AI thread loses `horizon`, as the main VM's sleeps do
    /// (`Vm::rebase_sleeps`, lockstep.md section 14).
    pub fn rebase_sleeps(&mut self, horizon: F32) {
        for vm in self.vms.values_mut() {
            vm.rebase_sleeps(horizon);
        }
    }

    /// Runs one AI step at game time `now`: builds the VMs of the pending start requests, then runs each AI VM once
    /// in ascending player index. Returns the number of AI VMs that ran.
    pub fn step(&mut self, now: F32) -> usize {
        let requests = std::mem::take(&mut self.shared.borrow_mut().requests);
        for request in requests {
            self.start(request, now);
        }
        let mut ran = 0usize;
        let players: Vec<u32> = self.vms.keys().copied().collect();
        for player in players {
            let paused = self
                .shared
                .borrow()
                .control(player)
                .is_some_and(|c| c.pause > 0);
            if paused {
                continue;
            }
            if let Some(vm) = self.vms.get_mut(&player) {
                vm.step(now);
                ran += 1;
                for (thread, status) in vm.take_failures() {
                    self.failures.push((player, thread, status));
                }
            }
        }
        ran
    }

    fn start(&mut self, request: StartRequest, now: F32) {
        let StartRequest {
            player,
            script,
            kind,
        } = request;
        let source = match self.library.script(&script) {
            Some(s) => s.to_string(),
            None => {
                self.event(AiEvent::BuildFailed {
                    player,
                    script,
                    error: "script not in the library".to_string(),
                });
                return;
            }
        };
        let module = match build_module(
            self.library.prelude(),
            self.library.common(),
            &source,
            self.tables.as_ref(),
            self.library.native_registry(),
        ) {
            Ok(m) => m,
            Err(error) => {
                self.event(AiEvent::BuildFailed {
                    player,
                    script,
                    error,
                });
                return;
            }
        };
        let mut natives = NativeRegistry::with_builtins();
        crate::natives::register_world(&mut natives, &self.game);
        crate::natives::ai::register_ai(&mut natives, &self.game, &self.shared, player);
        let mut vm = match Vm::new(module, natives, self.tables.clone()) {
            Ok(vm) => vm,
            Err(e) => {
                self.event(AiEvent::BuildFailed {
                    player,
                    script,
                    error: format!("{e:?}"),
                });
                return;
            }
        };
        vm.set_now(now);
        // `config` runs first, as a thread of its own, then `main` (UNSURE: order and form not in the spec).
        let _ = vm.start("config", &[]);
        if vm.start("main", &[]).is_err() {
            self.event(AiEvent::NoMain { player });
        }
        self.vms.insert(player, vm);
        self.event(AiEvent::Started {
            player,
            script,
            kind,
        });
    }

    fn event(&mut self, e: AiEvent) {
        self.shared.borrow_mut().events.push(e);
    }
}
