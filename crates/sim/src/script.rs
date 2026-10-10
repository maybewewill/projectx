//! JASS host: runs a [`jass::vm::Vm`] on the game clock and timer queue.
//!
//! `TriggerSleepAction(t)` stores the sleep in the VM as usual (`t` clamped to `>= 0`). The host
//! also records the thread and the wake time `now + t` (soft-float add, [`jass::vm::ScriptHost`]
//! uses the same expression as the VM) and starts a timer for that thread at that time.
//!
//! One host step ([`ScriptHost::step`]) does, in this order:
//!
//! 1. Advances the agile clock by one step (0.1 s, soft-float add) to `target`.
//! 2. Fires every timer with expiry `<= target`, in expiry order and then by insertion order
//!    (the [`TimerQueue`] order). Each fired timer resumes exactly its own thread at the timer's
//!    expiry with [`Vm::resume`]. Other threads are not touched. A sleep started by a resumed thread
//!    with an expiry `<= target` fires in the same pump, after the timers that were already due.
//! 3. Runs one scheduler pass at `target` ([`Vm::step`]): every runnable thread (budget exhausted,
//!    yielded, or started during the step) runs once, in list order. Sleeping threads that are not
//!    due are skipped.
//!
//! So a thread that was resumed in step 2 and used up its budget gets one more budget slice in step 3
//! of the same host step. This follows from the order above, and it is deterministic. It is
//! UNVERIFIED against the original, whose exact resume order is not in the spec (game-loop-2 4.5).
//!
//! UNVERIFIED: the wake time is stored as f32 bits in the timer queue (the queue's API is f32). The
//! value is the soft-float result, so no information is lost.

use std::cell::RefCell;
use std::rc::Rc;

use jass::compile::Module;
use jass::vm::{
    clamp_sleep, NativeRegistry, NativeResult, ResumeError, RunStatus, ThreadId, Value, Vm, VmError,
};
use softfloat::{Tables, F32};

use crate::timer::{AgileClock, TimerQueue};

/// A JASS VM driven by the agile clock and a timer queue.
pub struct ScriptHost {
    vm: Vm,
    clock: AgileClock,
    timers: TimerQueue<ThreadId>,
    /// `(thread, wake time)` of `TriggerSleepAction` calls not yet turned into timers.
    pending_sleeps: Rc<RefCell<Vec<(ThreadId, f32)>>>,
    /// Resumes the host refused. Empty in normal operation (see [`ScriptHost::misfires`]).
    misfires: Vec<(ThreadId, ResumeError)>,
}

impl ScriptHost {
    /// Creates the VM for `module` with the soft-float `tables` (`None` if they are not loaded; see
    /// [`Vm::new`]). `TriggerSleepAction` is replaced by a version that also records the wake time
    /// for the host.
    pub fn new(
        module: Module,
        mut natives: NativeRegistry,
        tables: Option<Tables>,
    ) -> Result<Self, VmError> {
        let pending_sleeps: Rc<RefCell<Vec<(ThreadId, f32)>>> = Rc::new(RefCell::new(Vec::new()));
        let record = Rc::clone(&pending_sleeps);
        natives.register("TriggerSleepAction", move |ctx, a| {
            let seconds = clamp_sleep(a.first().and_then(|v| v.as_real()).unwrap_or(F32::ZERO));
            // The VM's own wake time for this sleep: `now + seconds` (soft-float add).
            let wake = ctx.wake_time(seconds);
            record.borrow_mut().push((ctx.thread(), wake.to_f32()));
            NativeResult::Sleep(seconds)
        });
        let vm = Vm::new(module, natives, tables)?;
        let mut host = ScriptHost {
            vm,
            clock: AgileClock::new(),
            timers: TimerQueue::new(),
            pending_sleeps,
            misfires: Vec::new(),
        };
        host.schedule_pending();
        Ok(host)
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

    /// The timer queue (for inspection).
    pub fn timers(&self) -> &TimerQueue<ThreadId> {
        &self.timers
    }

    /// Timers whose resume was refused by the VM. Always empty unless the host and the VM disagree
    /// about a wake time, which the shared soft-float expression prevents.
    pub fn misfires(&self) -> &[(ThreadId, ResumeError)] {
        &self.misfires
    }

    /// Queues a thread running `name`. It first runs on the next [`ScriptHost::step`].
    pub fn start(&mut self, name: &str, args: &[Value]) -> Result<ThreadId, RunStatus> {
        self.vm.start(name, args)
    }

    /// Runs one host step (see the module documentation for the order) and returns the number of
    /// sleep timers that fired.
    pub fn step(&mut self) -> usize {
        self.clock.advance_step();
        let target = self.clock.now();
        let mut fired = 0usize;
        let vm = &mut self.vm;
        let pending = &self.pending_sleeps;
        let misfires = &mut self.misfires;
        self.timers.pump(target, |queue, event| {
            fired += 1;
            let at = F32::from_bits(event.expiry.to_bits());
            if let Err(e) = vm.resume(event.payload, at) {
                misfires.push((event.payload, e));
            }
            drain_pending(pending, queue);
        });
        self.vm.step(F32::from_bits(target.to_bits()));
        self.schedule_pending();
        fired
    }

    fn schedule_pending(&mut self) {
        drain_pending(&self.pending_sleeps, &mut self.timers);
    }
}

fn drain_pending(pending: &RefCell<Vec<(ThreadId, f32)>>, queue: &mut TimerQueue<ThreadId>) {
    for (thread, until) in pending.borrow_mut().drain(..) {
        queue.start_at(until, thread);
    }
}
