//! JASS host: runs a [`jass::vm::Vm`] on the game clock and timer queue.
//!
//! Sleeping threads are woken by timers. `TriggerSleepAction(t)` stores the raw seconds (no
//! conversion, no minimum). The thread's wake time is `now + t` in f32, the same value the VM
//! itself stores for the sleeping thread, and the host schedules a timer at exactly that time.
//!
//! When a timer fires at time `e`, the host calls `Vm::step(e)`. The VM wakes every sleeping thread
//! whose wake time is `<= e`, which are the threads whose timers are firing. Timers fire in expiry
//! order, so threads resume in expiry order (spec 4.5).
//!
//! UNVERIFIED / known deviation: `Vm::step` also runs every runnable (budget-exhausted or yielded)
//! thread. A pump that fires `k` timers therefore runs runnable threads `k` times instead of once.
//! A pump with no fired timers runs them once, as a plain step does. Removing the deviation needs a
//! `Vm` entry point that resumes one thread, which lives in `crates/jass`.
//!
//! Also UNVERIFIED: the wake time uses the VM's f32 add, not the soft-float add used for the clock.

use std::cell::RefCell;
use std::rc::Rc;

use jass::compile::Module;
use jass::vm::{NativeRegistry, NativeResult, RunStatus, ThreadId, Value, Vm, VmError};

use crate::timer::{AgileClock, TimerQueue};

/// A JASS VM driven by the agile clock and a timer queue.
pub struct ScriptHost {
    vm: Vm,
    clock: AgileClock,
    timers: TimerQueue<()>,
    /// Wake times of `TriggerSleepAction` calls not yet turned into timers.
    pending_sleeps: Rc<RefCell<Vec<f32>>>,
}

impl ScriptHost {
    /// Creates the VM for `module`. `TriggerSleepAction` is replaced by a version that also records
    /// the wake time for the host.
    pub fn new(module: Module, mut natives: NativeRegistry) -> Result<Self, VmError> {
        let pending_sleeps: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(Vec::new()));
        let record = Rc::clone(&pending_sleeps);
        natives.register("TriggerSleepAction", move |ctx, a| {
            let seconds = a[0].as_real().unwrap_or(0.0).max(0.0);
            // Same expression as the VM's own wake time (`now + seconds`, f32).
            record.borrow_mut().push(ctx.now() + seconds);
            NativeResult::Sleep(seconds)
        });
        let vm = Vm::new(module, natives)?;
        let mut host = ScriptHost {
            vm,
            clock: AgileClock::new(),
            timers: TimerQueue::new(),
            pending_sleeps,
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
    pub fn timers(&self) -> &TimerQueue<()> {
        &self.timers
    }

    /// Queues a thread running `name`. It first runs on the next [`ScriptHost::step`].
    pub fn start(&mut self, name: &str, args: &[Value]) -> Result<ThreadId, RunStatus> {
        self.vm.start(name, args)
    }

    /// Advances the clock by one step and fires every timer due by then.
    ///
    /// Each fired timer runs the VM at its expiry. If no timer fires, the VM runs once at the new
    /// clock time. Returns the number of sleep timers that fired.
    pub fn step(&mut self) -> usize {
        self.clock.advance_step();
        let target = self.clock.now();
        let mut fired = 0usize;
        let vm = &mut self.vm;
        let pending = &self.pending_sleeps;
        self.timers.pump(target, |queue, event| {
            fired += 1;
            vm.step(event.expiry);
            drain_pending(pending, queue);
        });
        if fired == 0 {
            self.vm.step(target);
            self.schedule_pending();
        }
        fired
    }

    fn schedule_pending(&mut self) {
        drain_pending(&self.pending_sleeps, &mut self.timers);
    }
}

fn drain_pending(pending: &RefCell<Vec<f32>>, queue: &mut TimerQueue<()>) {
    for until in pending.borrow_mut().drain(..) {
        queue.start_at(until, ());
    }
}
