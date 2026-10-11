//! Game ("agile") clock and the timer queue of the 1.26a engine (see `docs/spec/game-loop-2.md`).
//!
//! * [`AgileClock`]: game time in seconds as an f32 bit pattern. It advances by
//!   [`STEP`] (0.1 s, bits `0x3DCCCCCD`) per simulation step, using the truncating
//!   soft-float add from the `softfloat` crate (`softfloat::arith::add`).
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

/// Game time in seconds, kept as an f32 bit pattern.
///
/// Starts at 0.0. Each [`AgileClock::advance_step`] adds [`STEP`] with the truncating
/// soft-float add, so the value after `n` steps is the result of `n` soft-float adds
/// (not `n * 0.1` in IEEE arithmetic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgileClock {
    bits: u32,
}

impl AgileClock {
    /// A clock at 0.0 seconds.
    pub fn new() -> Self {
        AgileClock { bits: 0 }
    }

    /// Raw f32 bit pattern of the current time.
    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// Current time in seconds.
    pub fn now(&self) -> f32 {
        f32::from_bits(self.bits)
    }

    /// Advances the clock by one step.
    pub fn advance_step(&mut self) {
        self.advance(STEP_BITS);
    }

    /// Advances the clock by an f32 increment given as a bit pattern, with one soft-float add.
    /// The frame driver's per-tick increment is `lockstep::TICK_INCREMENT_BITS` (0.005 s,
    /// lockstep.md section 9).
    pub fn advance(&mut self, increment_bits: u32) {
        self.bits = arith::add(F32::from_bits(self.bits), F32::from_bits(increment_bits)).to_bits();
    }
}

/// Clamps a timeout the way the original does (spec 3.3, step 5): negative values
/// become +0.0. NaN also becomes +0.0 (UNVERIFIED: the original's NaN behaviour is not
/// in the spec; comparisons with NaN are false, so the clamp keeps NaN out of the heap).
fn clamp_timeout(t: f32) -> f32 {
    if t.is_nan() || t <= 0.0 {
        0.0
    } else {
        t
    }
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
    /// Negative timeouts (and NaN) are clamped to 0.0. For a periodic timer the same
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
                let next = expiry_for(e.expiry, period);
                if next > e.expiry {
                    self.push_entry(next, e.id, Some(period), e.payload.clone());
                } else {
                    let seq = self.alloc_seq();
                    self.deferred.push(Entry {
                        expiry: next,
                        seq,
                        id: e.id,
                        period: Some(period),
                        payload: e.payload.clone(),
                    });
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
        assert_eq!(clamp_timeout(-3.0), 0.0);
        assert_eq!(clamp_timeout(-0.0).to_bits(), 0);
        assert_eq!(clamp_timeout(f32::NAN), 0.0);
        assert_eq!(clamp_timeout(0.5), 0.5);
        assert_eq!(expiry_for(1.0, f32::INFINITY), f32::INFINITY);
    }
}
