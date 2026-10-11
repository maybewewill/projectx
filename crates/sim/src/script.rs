//! JASS host: runs a [`jass::vm::Vm`] on the game clock, the timer queue and the trigger state.
//!
//! `TriggerSleepAction(t)` stores the sleep in the VM as usual (`t` clamped to `>= 0`) and queues
//! the thread's wake time (`now + t`, soft-float add, the same expression the VM uses) in the
//! timer queue (see `natives`). Timer handles, timer expiries and trigger events are queued in the
//! same queue.
//!
//! One host step ([`ScriptHost::step`]) does, in this order:
//!
//! 1. Advances the agile clock by one step (0.1 s, soft-float add) to `target`.
//! 2. Pops every queue entry with expiry `<= target`, in expiry order and then by insertion order.
//!    A sleep resumes exactly its own thread at the expiry ([`Vm::resume`]). A timer expiry runs the
//!    timer's handler and fires its listening triggers. A trigger event fires its trigger. Each of
//!    these runs at the expiry as the game time, so timers started from there are relative to it.
//!    Entries queued by a callback with an expiry `<= target` are popped in the same step.
//! 3. Runs one scheduler pass at `target` ([`Vm::step`]): every runnable thread (budget exhausted,
//!    yielded, or started during the step) runs once, in list order. Sleeping threads that are not
//!    due are skipped.
//!
//! So a thread that was resumed in step 2 and used up its budget gets one more budget slice in step 3
//! of the same host step. This follows from the order above, and it is deterministic. It is
//! UNVERIFIED against the original, whose exact resume order is not in the spec (game-loop-2 4.5).

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use jass::compile::Module;
use jass::vm::{NativeRegistry, ResumeError, RunStatus, ThreadId, Value, Vm, VmError};
use softfloat::{Tables, F32};

use crate::natives::{self, Game, Payload};
use crate::rng::{RngTable, Streams};
use crate::timer::{AgileClock, TimerQueue};

/// A JASS VM driven by the agile clock, the timer queue and the trigger state.
pub struct ScriptHost {
    vm: Vm,
    clock: AgileClock,
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
        mut natives: NativeRegistry,
        tables: Option<Tables>,
    ) -> Result<Self, VmError> {
        let game = Rc::new(RefCell::new(Game::new()));
        game.borrow_mut().set_tables(tables.clone());
        natives::register(&mut natives, &game);
        let vm = Vm::new(module, natives, tables)?;
        Ok(ScriptHost {
            vm,
            clock: AgileClock::new(),
            game,
            misfires: Vec::new(),
        })
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

    /// Queues a thread running `name`. It first runs on the next [`ScriptHost::step`].
    pub fn start(&mut self, name: &str, args: &[Value]) -> Result<ThreadId, RunStatus> {
        self.vm.start(name, args)
    }

    /// Runs one host step (see the module documentation for the order) and returns the number of
    /// queue entries that fired (sleeps, timer expiries and trigger events).
    pub fn step(&mut self) -> usize {
        self.clock.advance_step();
        let target = self.clock.now();
        let mut fired = 0usize;
        loop {
            let next = self.game.borrow_mut().timers.pop_due(target);
            let Some(event) = next else {
                break;
            };
            fired += 1;
            let expiry = F32::from_bits(event.expiry.to_bits());
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
            }
        }
        self.game.borrow_mut().tick_orders();
        self.vm.step(F32::from_bits(target.to_bits()));
        fired
    }
}
