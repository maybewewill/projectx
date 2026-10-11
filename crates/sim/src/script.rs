//! JASS host: runs a [`jass::vm::Vm`] on the game clock, the timer queue and the trigger state.
//!
//! `TriggerSleepAction(t)` stores the sleep in the VM as usual (`t` clamped to `>= 0`) and queues
//! the thread's wake time (`now + t`, soft-float add, the same expression the VM uses) in the
//! timer queue (see `natives`). Timer handles, timer expiries and trigger events are queued in the
//! same queue.
//!
//! The basic unit of time is one game tick ([`ScriptHost::step_ticks`]). For each tick, in this order:
//!
//! 1. Advances the agile clock by one tick: one soft-float add of 0.005 s
//!    ([`lockstep::TICK_INCREMENT_BITS`], lockstep.md sections 9 and 10).
//! 2. Runs the timer pump to the new clock value. It pops every queue entry with expiry `<= target`,
//!    in expiry order and then by insertion order. A sleep resumes exactly its own thread at the
//!    expiry ([`Vm::resume`]). A timer expiry runs the timer's handler and fires its listening
//!    triggers. A trigger event fires its trigger. Each of these runs at the expiry as the game time,
//!    so timers started from there are relative to it. Entries queued by a callback with an expiry
//!    `<= target` are popped in the same tick. When the pump ends, the queue time and the VM time are
//!    `target`.
//!
//! The unit movement walk is not a tick step: it is the repeating timer of message 0x8026A, which
//! fires in the pump at its own expiry every [`lockstep::MOVE_PERIOD_BITS`] = 0.03 s of agile clock
//! (six ticks; lockstep.md section 11.2). Its callback is [`Game::tick_movement`].
//!
//! Every [`lockstep::TICKS_PER_BATCH`] ticks (the boundary is counted from the host's creation, so
//! the first batch ends at tick 20), the host also runs its combat tick of units with orders
//! ([`Game::tick_combat`]). This is the only per-batch work left; its cadence is UNSURE (lockstep.md 11.7).
//!
//! 3. Continuation (lockstep.md section 12.3). After the pump, every tick, the threads that stopped on
//!    their budget or yielded run again, but only when some thread is runnable ([`Vm::continue_runnable`]).
//!    Sleeping threads are never run here.
//!
//! JASS threads run at once in three cases (lockstep.md section 12): when they are started
//! ([`ScriptHost::start`], [`Vm::start_now`]), when a sleep or timer or trigger event fires in the
//! pump at its expiry, and when a native starts a trigger (the native's own chain, run as it returns).
//! There is no JASS pass on a batch cadence.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use jass::compile::Module;
use jass::vm::{NativeRegistry, ResumeError, RunStatus, ThreadId, Value, Vm, VmError};
use softfloat::{Tables, F32};

use crate::combat;
use crate::lockstep;
use crate::natives::{self, Game, Payload};
use crate::rng::{RngTable, Streams};
use crate::timer::{self, Advance, AgileClock, TimerQueue};

/// A JASS VM driven by the agile clock, the timer queue and the trigger state.
pub struct ScriptHost {
    vm: Vm,
    clock: AgileClock,
    /// Game ticks run since the host was created.
    ticks: u64,
    game: Rc<RefCell<Game>>,
    /// Resumes the host refused. Empty in normal operation (see [`ScriptHost::misfires`]).
    misfires: Vec<(ThreadId, ResumeError)>,
}

impl ScriptHost {
    /// Creates the VM for `module` with the soft-float `tables` (`None` if they are not loaded; see
    /// [`Vm::new`]). The timer, trigger and sleep natives of [`crate::natives`] are registered on
    /// `natives`, replacing any native with the same name (`TriggerSleepAction` included).
    pub fn new(
        module: Module,
        natives: NativeRegistry,
        tables: Option<Tables>,
    ) -> Result<Self, VmError> {
        Self::with_game(module, natives, tables, Rc::new(RefCell::new(Game::new())))
    }

    /// Like [`ScriptHost::new`], but on a game the caller already holds. The caller can register natives that
    /// need the game (the AI natives of `natives::ai::register_main`) before the VM exists, and share the game
    /// with an `ai::AiHost`.
    pub fn with_game(
        module: Module,
        mut natives: NativeRegistry,
        tables: Option<Tables>,
        game: Rc<RefCell<Game>>,
    ) -> Result<Self, VmError> {
        game.borrow_mut().set_tables(tables.clone());
        game.borrow_mut().set_vision_tables(tables.clone());
        natives::register(&mut natives, &game);
        {
            // The movement walk is a repeating timer (message 0x8026A, lockstep.md section 11.2), armed once per
            // game: a second host on the same game does not arm it again.
            let mut g = game.borrow_mut();
            if !g.movement_armed {
                g.movement_armed = true;
                g.timers.start(
                    f32::from_bits(lockstep::MOVE_PERIOD_BITS),
                    true,
                    Payload::Movement,
                );
            }
            // The vision refresh is its own repeating timer (message 0x80269, 0.4 s; vision.md section 11).
            if !g.vision_armed {
                g.vision_armed = true;
                g.timers.start(
                    f32::from_bits(lockstep::VISION_PERIOD_BITS),
                    true,
                    Payload::Vision,
                );
            }
        }
        let vm = Vm::new(module, natives, tables)?;
        Ok(ScriptHost {
            vm,
            clock: AgileClock::new(),
            ticks: 0,
            game,
            misfires: Vec::new(),
        })
    }

    /// The shared game state, as an `Rc` (for an `ai::AiHost` or a host that reads it).
    pub fn game_handle(&self) -> Rc<RefCell<Game>> {
        Rc::clone(&self.game)
    }

    /// The JASS VM (globals, thread count, failures).
    pub fn vm(&self) -> &Vm {
        &self.vm
    }

    /// Mutable access to the VM, for example to take failures.
    pub fn vm_mut(&mut self) -> &mut Vm {
        &mut self.vm
    }

    /// The game clock.
    pub fn clock(&self) -> &AgileClock {
        &self.clock
    }

    /// The timer queue (sleeps, timers and trigger events), for inspection.
    pub fn timers(&self) -> Ref<'_, TimerQueue<Payload>> {
        Ref::map(self.game.borrow(), |g| &g.timers)
    }

    /// The shared game state (timer and trigger objects and handles), for inspection.
    pub fn game(&self) -> Ref<'_, Game> {
        self.game.borrow()
    }

    /// Mutable access to the shared game state, for the host's map setup (player slots, the local
    /// player). Do not hold it while a script runs.
    pub fn game_mut(&self) -> RefMut<'_, Game> {
        self.game.borrow_mut()
    }

    /// Sets the playable area that rect coordinates are clamped to (`None` disables the clamp).
    pub fn set_playable(&self, playable: Option<natives::Playable>) {
        self.game.borrow_mut().set_playable(playable);
    }

    /// Gives the host's random generator its permutation table (from the user's `Game.dll`). Until
    /// this is called the random natives stop their thread with `MissingTables`.
    pub fn set_rng_table(&self, table: RngTable) {
        self.game.borrow_mut().set_rng_table(Some(table));
    }

    /// Game-start seeding with the seed `s` (see [`Streams::start_game`]). Returns `false`, and does
    /// nothing, when no RNG table was set. The seed itself is an input: the fold over the player slots
    /// that the original computes is not implemented (`docs/spec/rng.md` section 4.1).
    pub fn start_game_rng(&self, s: u32) -> bool {
        let mut g = self.game.borrow_mut();
        match g.rng_parts() {
            Some((table, streams)) => {
                streams.start_game(table, s);
                true
            }
            None => false,
        }
    }

    /// The random generator state (global generator and streams), for inspection.
    pub fn rng(&self) -> Ref<'_, Streams> {
        Ref::map(self.game.borrow(), |g| g.rng())
    }

    /// Resumes the host refused. Always empty unless the host and the VM disagree about a wake time,
    /// which the shared soft-float expression prevents.
    pub fn misfires(&self) -> &[(ThreadId, ResumeError)] {
        &self.misfires
    }

    /// Starts a thread running `name` and runs it at once, at the current game time, until its first
    /// stop (lockstep.md section 12.1). The original runs a started script from the calling site, not at
    /// a later batch. Threads it starts run after it, in the same call.
    pub fn start(&mut self, name: &str, args: &[Value]) -> Result<ThreadId, RunStatus> {
        let now = F32::from_bits(self.clock.now().to_bits());
        self.vm.start_now(name, args, now)
    }

    /// Game ticks run since the host was created.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Whether the last tick run ended a batch (see [`lockstep::TICKS_PER_BATCH`]). A host that has
    /// run no tick is at a boundary.
    pub fn at_batch_boundary(&self) -> bool {
        self.ticks.is_multiple_of(lockstep::TICKS_PER_BATCH)
    }

    /// Ticks until the next batch boundary, in `1..=TICKS_PER_BATCH`. A caller that runs other
    /// per-step work (the AI host) splits its ticks here, so that work runs at the same boundaries.
    pub fn ticks_to_batch(&self) -> u64 {
        lockstep::TICKS_PER_BATCH - self.ticks % lockstep::TICKS_PER_BATCH
    }

    /// Runs one batch: [`lockstep::TICKS_PER_BATCH`] ticks (100 ms of game time). See the module
    /// documentation. Returns the number of queue entries that fired.
    pub fn step(&mut self) -> usize {
        self.step_ticks(lockstep::TICKS_PER_BATCH)
    }

    /// Runs `n` game ticks. Each tick adds 0.005 s to the clock (one soft-float add) and runs the
    /// timer pump to the new time. The per-step work runs at each batch boundary. Returns the number
    /// of queue entries that fired (sleeps, timer expiries and trigger events).
    ///
    /// `n` is the tick count of a turn: `lockstep::TurnReport::game_ticks` when a replay is played.
    pub fn step_ticks(&mut self, n: u64) -> usize {
        let mut fired = 0usize;
        for _ in 0..n {
            let target = match self.clock.plan(lockstep::TICK_INCREMENT_BITS) {
                Advance::Within(bits) => {
                    self.clock.commit(Advance::Within(bits));
                    f32::from_bits(bits)
                }
                Advance::Rollover { excess } => {
                    // The clock reaches the horizon (300 s): fire everything due up to it, shift every pending
                    // expiry and sleep by it, and restart from the excess (`0x6F46E520`, lockstep.md section 14).
                    fired += self.pump(f32::from_bits(timer::HORIZON_BITS));
                    self.game.borrow_mut().timers.rebase(f32::from_bits(excess));
                    self.vm.rebase_sleeps(F32::from_bits(timer::HORIZON_BITS));
                    self.clock.commit(Advance::Rollover { excess });
                    f32::from_bits(excess)
                }
            };
            self.ticks = self.ticks.saturating_add(1);
            fired += self.pump(target);
            self.game.borrow_mut().production_tick(self.clock.absolute());
            self.continue_threads(target);
            if self.at_batch_boundary() {
                self.batch();
            }
        }
        fired
    }

    /// Runs the timer pump to `target` (the clock value of this tick) and returns the number of
    /// entries that fired. The VM time is `target` when it returns.
    fn pump(&mut self, target: f32) -> usize {
        let mut fired = 0usize;
        let row = self.clock.row() as i32;
        loop {
            let next = self.game.borrow_mut().timers.pop_due(target);
            let Some(event) = next else {
                break;
            };
            // The movement walk and the vision refresh are engine passes, not script events: not counted.
            if !matches!(event.payload, Payload::Movement | Payload::Vision) {
                fired += 1;
            }
            let expiry = F32::from_bits(event.expiry.to_bits());
            // Positions are evaluated at the queue time of the callback (movement-6.md section 6).
            self.game.borrow_mut().set_game_clock(expiry, row);
            match event.payload {
                Payload::Sleep(thread) => {
                    if let Err(e) = self.vm.resume(thread, expiry) {
                        self.misfires.push((thread, e));
                    }
                }
                Payload::Timer(timer) => {
                    self.vm.set_now(expiry);
                    natives::expire_timer(&self.game, &mut self.vm, timer);
                }
                Payload::Event(ev) => {
                    self.vm.set_now(expiry);
                    let trigger = match self.game.borrow().object(ev) {
                        natives::game::Object::Event(e) => Some(e.trigger),
                        _ => None,
                    };
                    if let Some(trigger) = trigger {
                        natives::fire_trigger(&self.game, &mut self.vm, trigger, Some(ev));
                    }
                }
                Payload::TimedLife { unit, serial } => {
                    self.vm.set_now(expiry);
                    natives::expire_timed_life(&self.game, &mut self.vm, unit, serial);
                }
                Payload::Attack { unit, msg } => {
                    self.vm.set_now(expiry);
                    combat::expire_attack_timer(&self.game, &mut self.vm, unit, msg);
                }
                Payload::DamagePoint(id) => {
                    self.vm.set_now(expiry);
                    combat::expire_point_damage(&self.game, &mut self.vm, id);
                }
                Payload::Movement => {
                    // The movement walk runs at its own expiry (the game time during the callback).
                    self.vm.set_now(expiry);
                    self.game.borrow_mut().tick_movement();
                }
                Payload::Vision => {
                    // The vision refresh (0x80269) at its own expiry.
                    self.vm.set_now(expiry);
                    self.game.borrow_mut().vision_refresh();
                }
            }
        }
        self.vm.set_now(F32::from_bits(target.to_bits()));
        self.game.borrow_mut().set_game_clock(F32::from_bits(target.to_bits()), row);
        fired
    }

    /// The per-step work at a batch boundary: the combat tick of units with orders. Movement is the
    /// movement timer in [`ScriptHost::pump`]; JASS threads run in the pump (sleep wake-ups, events,
    /// timer handlers) and when they start (section 12).
    fn batch(&mut self) {
        let mut game = self.game.borrow_mut();
        game.tick_combat();
        game.production_batch();
    }

    /// Continues the threads that stopped on their budget or yielded (section 12.3). Runs at the end of
    /// every tick, only when some thread is runnable, and never touches sleeping threads (their wake-up
    /// is the pump's [`Vm::resume`]).
    fn continue_threads(&mut self, target: f32) {
        if self.vm.has_runnable() {
            self.vm.continue_runnable(F32::from_bits(target.to_bits()));
        }
    }
}
