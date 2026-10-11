//! Game ("agile") clock and the timer queue of the 1.26a engine (see `docs/spec/game-loop-2.md`).
//!
//! * [`AgileClock`]: game time in seconds as an f32 bit pattern. The host advances it by one game
//!   tick per soft-float add of 0.005 s ([`crate::lockstep::TICK_INCREMENT_BITS`], lockstep.md
//!   section 9), using the truncating add from the `softfloat` crate (`softfloat::arith::add`).
//!   [`AgileClock::advance_step`] is the old 0.1 s step, kept for tests that model it.
//! * [`TimerQueue`]: a 1-based binary min-heap ordered by expiry, then by a
//!   monotonically increasing sequence number (FIFO for equal expiry). The
//!   original is a `CPriorityQ` with the same two keys (spec 3.4).
//!
//! Heap layout note: the exact layout of the original's heap matters only if the
//! iteration order of equal `(expiry, seq)` pairs could differ. It cannot, because
//! `seq` is unique per insertion, so any correct min-heap pops the same sequence.
//! The sift code below is still explicit so that the tie behaviour is visible.
//!
//! Decisions marked UNVERIFIED are not confirmed against the original binary.

use softfloat::{arith, F32};

/// One simulation step in seconds: the f32 constant 0.1 (`0x6FAAE4A0` in Game.dll).
pub const STEP_BITS: u32 = 0x3DCC_CCCD;

/// [`STEP_BITS`] as a float.
pub const STEP: f32 = f32::from_bits(STEP_BITS);

/// The queue horizon `300.0` f32 (`0x6FAAE594`, filled in at start-up): the agile clock restarts when it reaches it
/// (lockstep.md section 14).
pub const HORIZON_BITS: u32 = 0x4396_0000;

/// The float at `0x6FAAE47C` (0.001 after start-up): a timeout at or above the horizon is cut to `horizon - 0.001`.
pub const MILLI_BITS: u32 = 0x3A83_126F;

/// The float at `0x6FAAE61C` (8e-7 after start-up): an excess past the horizon below this is dropped to zero.
pub const MIN_EXCESS_BITS: u32 = 0x3556_BF95;

/// What the next clock add does (`0x6F46E520`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    /// The sum stays below the horizon: the clock becomes this bit pattern, then the timer pump runs.
    Within(u32),
    /// The sum reaches the horizon: pump to the horizon, rebase every pending expiry by it, then the clock becomes
    /// `excess` (row + 1) and the pump runs again.
    Rollover { excess: u32 },
}

/// Game time in seconds, kept as an f32 bit pattern plus the row (restart count).
///
/// Starts at 0.0, row 0. Each advance adds one increment with the truncating soft-float add, so the value
/// after `n` advances is the result of `n` soft-float adds (not `n * increment` in IEEE arithmetic).
/// When a sum reaches [`HORIZON_BITS`] (300 s) the clock restarts: it keeps the excess over the horizon and the row
/// counts the restarts (`0x6F46E520`, `0x6F469D40`; lockstep.md section 14). The host's tick is
/// [`crate::lockstep::TICK_INCREMENT_BITS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgileClock {
    bits: u32,
    row: u32,
}

impl AgileClock {
    /// A clock at 0.0 seconds, row 0.
    pub fn new() -> Self {
        AgileClock { bits: 0, row: 0 }
    }

    /// Raw f32 bit pattern of the current time within the row.
    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// Current time in seconds within the row (below 300.0).
    pub fn now(&self) -> f32 {
        f32::from_bits(self.bits)
    }

    /// Number of restarts so far (`queue + 0x44`).
    pub fn row(&self) -> u32 {
        self.row
    }

    /// Elapsed game seconds, `300 * row + now`, as an f64 (for reports; the original never forms this sum).
    pub fn elapsed_seconds(&self) -> f64 {
        f64::from(self.row) * 300.0 + f64::from(self.now())
    }

    /// `elapsed_seconds` as f32: a monotone time base for models that keep absolute times of their own.
    pub fn absolute(&self) -> f32 {
        self.elapsed_seconds() as f32
    }

    /// Decides the next add without changing the clock: `tmp = clock + inc` (soft add); a sum below the horizon is kept,
    /// otherwise the excess is `tmp - horizon`, dropped to 0 when its magnitude is below [`MIN_EXCESS_BITS`].
    pub fn plan(&self, increment_bits: u32) -> Advance {
        let tmp = arith::add(F32::from_bits(self.bits), F32::from_bits(increment_bits));
        let horizon = F32::from_bits(HORIZON_BITS);
        if tmp.to_f32() < horizon.to_f32() {
            return Advance::Within(tmp.to_bits());
        }
        let excess = arith::sub(tmp, horizon);
        let magnitude = arith::sub(excess, F32::ZERO).to_bits() & 0x7FFF_FFFF;
        let excess = if f32::from_bits(magnitude) < f32::from_bits(MIN_EXCESS_BITS) {
            0
        } else {
            excess.to_bits()
        };
        Advance::Rollover { excess }
    }

    /// Applies a planned add: the clock takes the new time, a rollover also counts a row.
    pub fn commit(&mut self, adv: Advance) {
        match adv {
            Advance::Within(bits) => self.bits = bits,
            Advance::Rollover { excess } => {
                self.bits = excess;
                self.row += 1;
            }
        }
    }

    /// Advances the clock by an f32 increment given as a bit pattern (plan + commit). A host with a timer queue must
    /// not use this across the horizon: it has to pump and rebase in between (see [`Advance`]).
    pub fn advance(&mut self, increment_bits: u32) -> Advance {
        let adv = self.plan(increment_bits);
        self.commit(adv);
        adv
    }

    /// Advances the clock by one 0.1 s step (one soft-float add of [`STEP_BITS`]). This is not what
    /// the host does any more: the host adds one 0.005 s tick per tick (see [`AgileClock::advance`]).
    /// It is kept for the tests and probes that model the old step.
    pub fn advance_step(&mut self) {
        self.advance(STEP_BITS);
    }
}

/// Minimum timeout: the clamp in `0x6F477F80` compares against the float at `0x6FAAE478`,
/// which is 0.0 in the file but **1e-4** (0x38D1B717) after the start-up initialiser
/// `0x6F84DC90` (runtime_consts). Corrects game-loop-2.md 3.3 step 5.
pub const MIN_TIMEOUT_BITS: u32 = 0x38D1_B717;

/// Clamps a timeout the way the timer start does (`0x6F477FF0` then `0x6F477F80`, Unicorn-checked,
/// `tools/emu/agile_clock_emu.py`):
///
/// 1. a timeout at or above the horizon (300.0), infinity included, becomes `horizon - 0.001` (soft sub): the queue
///    cannot hold a longer one. NaN is kept.
/// 2. a timeout strictly below 1e-4 (including zero and negatives) becomes exactly 1e-4; 1e-4 itself is kept.
///    (`fld t; fld min; fcompp; test ah,0x41; jne keep`; the original keeps NaN here too.)
///
/// NaN still becomes the minimum here (UNVERIFIED: a NaN expiry's heap order is not modelled).
pub fn clamp_timeout(t: f32) -> f32 {
    let min = f32::from_bits(MIN_TIMEOUT_BITS);
    let mut t = t;
    if !t.is_nan() && t >= f32::from_bits(HORIZON_BITS) {
        t = arith::sub(F32::from_bits(HORIZON_BITS), F32::from_bits(MILLI_BITS)).to_f32();
    }
    if t.is_nan() || t < min {
        min
    } else {
        t
    }
}

/// The seconds a sleeping thread waits: the timeout of the agent timer a sleep starts (`0x6F2DC6C0` posts it with
/// the same start routine as `TimerStart`), so the same clamps apply ([`clamp_timeout`]). The VM and the queue both
/// add this to the same clock, so the thread's wake time and the timer's expiry are the same float.
pub fn sleep_seconds(seconds: F32) -> F32 {
    F32::from_bits(clamp_timeout(seconds.to_f32()).to_bits())
}

/// Expiry for a timeout started at `now`: `now + timeout` with the soft-float add.
///
/// UNVERIFIED: infinite timeouts never expire (expiry = +inf). Soft-float has no
/// infinity, so this case cannot go through it. Timeouts beyond about 2^31 seconds
/// are not checked against the soft-float overflow behaviour.
fn expiry_for(now: f32, timeout: f32) -> f32 {
    if timeout.is_finite() {
        arith::add(
            F32::from_bits(now.to_bits()),
            F32::from_bits(timeout.to_bits()),
        )
        .to_f32()
    } else {
        f32::INFINITY
    }
}

/// Identifies a timer. Assigned in creation order and kept across periodic re-arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerId(pub u32);

/// A timer that fired, as passed to the [`TimerQueue::pump`] callback.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired<P> {
    pub id: TimerId,
    /// The timer's expiry, which is also the game time during the callback.
    pub expiry: f32,
    pub payload: P,
}

#[derive(Debug, Clone)]
struct Entry<P> {
    expiry: f32,
    /// Insertion order, used to break ties in expiry (FIFO).
    seq: u64,
    id: TimerId,
    /// `Some(period)` for periodic timers.
    period: Option<f32>,
    payload: P,
}

/// Strict "fires before" order: smaller expiry first, then smaller sequence number.
///
/// Expiries are compared as floats. `-0.0` and `+0.0` are equal. Expiries are never NaN
/// (see [`clamp_timeout`] and [`TimerQueue::start_at`]). Because `seq` is unique, two
/// different entries never compare equal.
fn before<P>(a: &Entry<P>, b: &Entry<P>) -> bool {
    a.expiry < b.expiry || (a.expiry == b.expiry && a.seq < b.seq)
}

/// Min-heap of timers with explicit 1-based indexing: the root is index 1, the parent
/// of `i` is `i / 2`, and the children of `i` are `2i` and `2i + 1`.
#[derive(Debug, Clone)]
pub struct TimerQueue<P> {
    now: f32,
    /// Element at 1-based index `i` lives at `heap[i - 1]`.
    heap: Vec<Entry<P>>,
    /// Periodic re-arms that would not move past their expiry (period 0). They are
    /// kept out of the heap during the pump so the pump terminates, and go back in
    /// when it ends. UNVERIFIED: the original's handling of a zero period is unknown.
    deferred: Vec<Entry<P>>,
    next_seq: u64,
    next_id: u32,
}

impl<P: Clone> Default for TimerQueue<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Clone> TimerQueue<P> {
    /// An empty queue at time 0.0.
    pub fn new() -> Self {
        TimerQueue {
            now: 0.0,
            heap: Vec::new(),
            deferred: Vec::new(),
            next_seq: 0,
            next_id: 0,
        }
    }

    /// Current queue time: the last pump target, or the expiry of the timer being fired.
    pub fn now(&self) -> f32 {
        self.now
    }

    /// Number of pending timers.
    pub fn len(&self) -> usize {
        self.heap.len() + self.deferred.len()
    }

    /// Whether no timers are pending.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Expiry of the next timer to fire, if any.
    pub fn next_expiry(&self) -> Option<f32> {
        self.heap.first().map(|e| e.expiry)
    }

    /// Starts a timer that fires `timeout_seconds` after the current time.
    ///
    /// Timeouts below 1e-4 (and NaN) are clamped to 1e-4 ([`MIN_TIMEOUT_BITS`]). For a periodic timer the same
    /// clamped value is the period. Returns the new timer's id.
    pub fn start(&mut self, timeout_seconds: f32, periodic: bool, payload: P) -> TimerId {
        let timeout = clamp_timeout(timeout_seconds);
        let expiry = expiry_for(self.now, timeout);
        let period = periodic.then_some(timeout);
        let id = self.alloc_id();
        self.push_entry(expiry, id, period, payload);
        id
    }

    /// Starts a timer that fires at the absolute time `expiry`.
    ///
    /// Used when the expiry was computed elsewhere (see `ScriptHost`). An expiry before
    /// the current time, or NaN, is clamped to the current time (UNVERIFIED: the original
    /// does not expose this path).
    pub fn start_at(&mut self, expiry: f32, payload: P) -> TimerId {
        let expiry = if expiry.is_nan() || expiry < self.now {
            self.now
        } else {
            expiry
        };
        let id = self.alloc_id();
        self.push_entry(expiry, id, None, payload);
        id
    }

    /// Removes a pending timer. Returns `false` if it is not pending (already fired or cancelled).
    ///
    /// This is a linear search, so it costs O(n). The original is not known to differ in
    /// observable behaviour.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        if let Some(i) = self.heap.iter().position(|e| e.id == id) {
            self.remove_at(i + 1);
            return true;
        }
        if let Some(i) = self.deferred.iter().position(|e| e.id == id) {
            self.deferred.remove(i);
            return true;
        }
        false
    }

    /// Fires every timer with expiry `<= target`, in heap order.
    ///
    /// For each fired timer, the queue time is set to its expiry and `fire` is called with
    /// the queue (so the callback can start or cancel timers, relative to that expiry) and
    /// the fired event. When the pump finishes, the queue time is `target`.
    ///
    /// Periodic timers are re-armed at `expiry + period` before their callback runs, so the
    /// callback can cancel them. If the re-arm would not move the expiry forward (period 0),
    /// the re-arm waits until the pump ends (UNVERIFIED).
    ///
    /// A `target` before the current time (or NaN) does nothing.
    pub fn pump<F>(&mut self, target: f32, mut fire: F)
    where
        F: FnMut(&mut Self, Fired<P>),
    {
        while let Some(ev) = self.pop_due(target) {
            fire(self, ev);
        }
    }

    /// Takes the next timer that is due at or before `target`, or `None` when none is left.
    ///
    /// This is [`TimerQueue::pump`] one event at a time, for hosts whose callbacks need to reach
    /// state that owns this queue (the queue must not be borrowed while they run). The queue time is
    /// the returned expiry until the next call; when `None` is returned the queue time is `target`.
    /// A periodic timer is re-armed exactly as in `pump`. A `target` before the current time (or
    /// NaN) returns `None` and changes nothing.
    pub fn pop_due(&mut self, target: f32) -> Option<Fired<P>> {
        if target.is_nan() || target < self.now {
            return None;
        }
        if self.heap.first().is_some_and(|top| top.expiry <= target) {
            let e = self.pop_min();
            if let Some(period) = e.period {
                // The re-arm (`0x6F46CC80`) keeps the element, so its sequence number stays the one it got when it
                // started: among equal expiries a periodic timer is ordered by its first start, not by its re-arms.
                let next = expiry_for(e.expiry, period);
                let again = Entry {
                    expiry: next,
                    seq: e.seq,
                    id: e.id,
                    period: Some(period),
                    payload: e.payload.clone(),
                };
                if next > e.expiry {
                    self.push_heap(again);
                } else {
                    self.deferred.push(again);
                }
            }
            self.now = e.expiry;
            return Some(Fired {
                id: e.id,
                expiry: e.expiry,
                payload: e.payload,
            });
        }
        self.now = target;
        for e in std::mem::take(&mut self.deferred) {
            self.push_heap(e);
        }
        None
    }

    /// The expiry of a pending timer, or `None` if it is not pending (fired or cancelled).
    /// Linear search, like [`TimerQueue::cancel`].
    pub fn expiry_of(&self, id: TimerId) -> Option<f32> {
        self.heap
            .iter()
            .chain(self.deferred.iter())
            .find(|e| e.id == id)
            .map(|e| e.expiry)
    }

    /// The clock restart (`0x6F469D40`): every pending expiry loses the horizon (soft sub, in heap order) and the queue
    /// time becomes `new_now` (the excess the clock restarts from). The host pumps to the horizon first and pumps again
    /// afterwards ([`Advance::Rollover`]). Expiries here are above the horizon and below twice of it, so the sub is
    /// exact and the heap order is kept.
    pub fn rebase(&mut self, new_now: f32) {
        let horizon = F32::from_bits(HORIZON_BITS);
        for e in self.heap.iter_mut().chain(self.deferred.iter_mut()) {
            if e.expiry.is_finite() {
                e.expiry = arith::sub(F32::from_bits(e.expiry.to_bits()), horizon).to_f32();
            }
        }
        self.now = new_now;
    }

    fn alloc_id(&mut self) -> TimerId {
        let id = TimerId(self.next_id);
        self.next_id += 1;
        id
    }

    fn alloc_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    fn push_entry(&mut self, expiry: f32, id: TimerId, period: Option<f32>, payload: P) {
        let seq = self.alloc_seq();
        self.push_heap(Entry {
            expiry,
            seq,
            id,
            period,
            payload,
        });
    }

    fn push_heap(&mut self, e: Entry<P>) {
        self.heap.push(e);
        let n = self.heap.len();
        self.sift_up(n);
    }

    /// Removes and returns the root (1-based index 1).
    fn pop_min(&mut self) -> Entry<P> {
        let n = self.heap.len();
        self.heap.swap(0, n - 1);
        let e = self.heap.pop().expect("pop_min on empty heap");
        if !self.heap.is_empty() {
            self.sift_down(1);
        }
        e
    }

    /// Removes the element at 1-based index `i`.
    fn remove_at(&mut self, i: usize) {
        let n = self.heap.len();
        self.heap.swap(i - 1, n - 1);
        self.heap.pop();
        if i < n {
            let j = self.sift_up(i);
            self.sift_down(j);
        }
    }

    /// Moves the element at 1-based index `i` up while it fires before its parent.
    /// Returns its final 1-based index.
    fn sift_up(&mut self, mut i: usize) -> usize {
        while i > 1 {
            let p = i / 2;
            if before(&self.heap[i - 1], &self.heap[p - 1]) {
                self.heap.swap(i - 1, p - 1);
                i = p;
            } else {
                break;
            }
        }
        i
    }

    /// Moves the element at 1-based index `i` down while a child fires before it.
    /// The left child is preferred only when it is strictly before the right one.
    fn sift_down(&mut self, mut i: usize) {
        let n = self.heap.len();
        loop {
            let l = 2 * i;
            let r = l + 1;
            let mut m = i;
            if l <= n && before(&self.heap[l - 1], &self.heap[m - 1]) {
                m = l;
            }
            if r <= n && before(&self.heap[r - 1], &self.heap[m - 1]) {
                m = r;
            }
            if m == i {
                break;
            }
            self.heap.swap(i - 1, m - 1);
            i = m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_starts_at_zero_and_first_step_is_exact() {
        let mut c = AgileClock::new();
        assert_eq!(c.bits(), 0);
        c.advance_step();
        // 0 + x returns x in the soft-float add.
        assert_eq!(c.bits(), STEP_BITS);
        assert_eq!(c.now(), 0.1f32);
    }

    #[test]
    fn clock_second_step_is_exact_doubling() {
        let mut c = AgileClock::new();
        c.advance_step();
        c.advance_step();
        // 0.1 + 0.1 = 0.2 exactly (same mantissa, exponent + 1).
        assert_eq!(c.bits(), 0x3E4C_CCCD);
    }

    #[test]
    fn clamp_and_expiry_helpers() {
        let min = f32::from_bits(MIN_TIMEOUT_BITS);
        assert_eq!(clamp_timeout(-3.0).to_bits(), MIN_TIMEOUT_BITS);
        assert_eq!(clamp_timeout(-0.0).to_bits(), MIN_TIMEOUT_BITS);
        assert_eq!(clamp_timeout(0.0).to_bits(), MIN_TIMEOUT_BITS);
        assert_eq!(clamp_timeout(f32::from_bits(MIN_TIMEOUT_BITS - 1)).to_bits(), MIN_TIMEOUT_BITS);
        assert_eq!(clamp_timeout(min).to_bits(), MIN_TIMEOUT_BITS);
        assert_eq!(clamp_timeout(f32::NAN).to_bits(), MIN_TIMEOUT_BITS);
        assert_eq!(clamp_timeout(0.5), 0.5);
        assert_eq!(expiry_for(1.0, f32::INFINITY), f32::INFINITY);
    }

    #[test]
    fn timeouts_from_the_horizon_up_are_cut_below_it() {
        // Unicorn (tools/emu/agile_clock_emu.py probe): 300, 600 and 1e6 all start with 299.999.
        let cut = 299.999f32;
        for t in [300.0f32, 600.0, 1e6, f32::INFINITY] {
            assert_eq!(clamp_timeout(t).to_bits(), cut.to_bits(), "{t}");
        }
        assert_eq!(clamp_timeout(299.0), 299.0);
        assert_eq!(clamp_timeout(f32::from_bits(0x4395_FFFF)), f32::from_bits(0x4395_FFFF));
    }

    #[test]
    fn clock_restarts_at_the_horizon_with_the_excess() {
        let mut c = AgileClock::new();
        c.bits = 0x4395_FFFF; // just below 300.0
        let adv = c.plan(0x3BA3_D70A);
        let Advance::Rollover { excess } = adv else { panic!("no rollover: {adv:?}") };
        // 299.99997 + 0.005 - 300.0 = 0.00497...
        assert!((f32::from_bits(excess) - 0.00497).abs() < 1e-4, "{}", f32::from_bits(excess));
        c.commit(adv);
        assert_eq!((c.row(), c.bits()), (1, excess));
        // A sum of exactly 300.0 also restarts, with a zero excess (the 8e-7 floor never matters at this magnitude:
        // the spacing of floats near 300 is 3e-5).
        let mut d = AgileClock::new();
        d.bits = 0x4395_0000; // 298.0
        assert_eq!(d.plan(0x4000_0000), Advance::Rollover { excess: 0 }); // + 2.0
    }

    #[test]
    fn rebase_shifts_every_pending_expiry() {
        let mut q: TimerQueue<u32> = TimerQueue::new();
        q.start_at(310.0, 1);
        q.start_at(450.5, 2);
        q.rebase(5.0);
        assert_eq!(q.now(), 5.0);
        assert_eq!(q.next_expiry(), Some(10.0));
        let mut fired = Vec::new();
        q.pump(500.0, |_, ev| fired.push((ev.payload, ev.expiry)));
        assert_eq!(fired, vec![(1, 10.0), (2, 150.5)]);
    }

    #[test]
    fn periodic_rearm_keeps_its_first_sequence_number() {
        // Two timers meet at the same expiry; the periodic one started first, so it fires first at every meeting,
        // even though its re-arm is later than the other timer's start (the re-arm reuses the element, 0x6F46CC80).
        let mut q: TimerQueue<u32> = TimerQueue::new();
        q.start(1.0, true, 1);
        q.start(2.0, true, 2);
        let mut order = Vec::new();
        q.pump(4.0, |_, ev| order.push((ev.payload, ev.expiry)));
        assert_eq!(order, vec![(1, 1.0), (1, 2.0), (2, 2.0), (1, 3.0), (1, 4.0), (2, 4.0)]);
    }
}
