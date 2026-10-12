//! The agile property engine of Game.dll 1.26a: float properties with modifiers and threshold listeners, driven by a
//! timer queue (docs/spec/agile.md). The construction / training / research / revive countdowns, the building life
//! ramp and the time of day are all properties of this engine; completion is a listener firing, not a timer.
//!
//! Model (all arithmetic through the truncating soft float, in the order of the original):
//!
//! * [`Property`] (`CPoReal`): base `B` (`+0x78`), slope `S` per agile second (`+0x7C`), limits `lo`/`hi`
//!   (`+0x80`/`+0x84`) and the time `t0` at which `B` was valid. Its value now is
//!   `clamp(B + S * w)` with `w` the time since `t0` (`0x6F477180`, `0x6F4885A0`, `0x6F483E70`).
//! * [`Modifier`] (`CPoReContinuousMod`): adds a rate to the slope of a property for a horizon (seconds), then takes
//!   it away again (`0x6F4774E0` creator, `0x6F4A9F80` begin / end).
//! * [`Listener`] (`CPoReThresholdLis`): fires its message when the property reaches a threshold from above (mode 0)
//!   or below (mode 1) within a tolerance of 0.005. It computes the time to the crossing, arms ONE timer for
//!   `min(that, 4.0)` seconds and re-checks when it fires (`0x6F477550` creator, `0x6F4A9C70`, `0x6F4A9B70`).
//! * The timer queue is the world queue at `G+0x14` (`CSiDispatch`): a min-heap on (expiry, sequence) pumped by the
//!   same tick as the agile clock (`0x6F46E9E0`), with the 300 s restart.
//!
//! Everything here was run against the original in Unicorn (`tools/emu/agile_engine_emu.py`); see the CONFIRMED /
//! LIKELY / UNSURE marks in docs/spec/agile.md.

use softfloat::{arith, Tables, F32};

const ZERO: F32 = F32(0);
/// `0.005` (`0x6FAB7778`): the tolerance of a listener (strict `<`).
pub const TOLERANCE_BITS: u32 = 0x3BA3_D70A;
/// `2.0e-7` (`0x6FAAE614`): a rate difference below it is ignored; a horizon below it means "forever".
const TINY_RATE_BITS: u32 = 0x3456_BF95;
/// `8.0e-7` (`0x6FAAE61C`): a slope below it is zero.
const TINY_SLOPE_BITS: u32 = 0x3556_BF95;
/// `1e-4` (`0x6FAAE478`): the shortest timer, and the snap of the elapsed time.
pub const MIN_TIMER_BITS: u32 = 0x38D1_B717;
/// `FLT_MAX` (`0x6FAAE624`): the default horizon.
pub const FOREVER_BITS: u32 = 0x7F7F_FFFF;
/// `4.0` (`0x6FAAE510`): the longest wait of a listener.
const LISTENER_CAP_BITS: u32 = 0x4080_0000;
/// `150.0` (`0x6FAB7774`): the period of the timer of a forever modifier.
const FOREVER_PERIOD_BITS: u32 = 0x4316_0000;
/// `0.1` (`0x6FAAE4A0`): the first timer of a new listener (always cancelled by its first evaluation).
const FIRST_LISTENER_TIMER_BITS: u32 = 0x3DCC_CCCD;
/// `-1.0` (`0x6FAAE464`): "no crossing" from the time-to-cross.
const NEG_ONE_BITS: u32 = 0xBF80_0000;
/// The queue horizon `300.0` (`+0x48` of the queue).
const HORIZON: F32 = F32(0x4396_0000);

fn fl(x: F32) -> f32 {
    f32::from_bits(x.0)
}

/// `|a - b|` as the original forms it: soft sub, sign bit cleared.
fn abs_sub(a: F32, b: F32) -> f32 {
    f32::from_bits(arith::sub(a, b).0 & 0x7FFF_FFFF)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PropId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModId(pub usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LisId(pub usize);

/// A listener that fired: its message goes to the owner (e.g. `0xD01C4` to a countdown object).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fired {
    pub listener: LisId,
    pub message: u32,
    /// The queue time at the moment of the firing (bit pattern; the expiry of the timer that fired, or the creation /
    /// change time).
    pub at: F32,
    /// The row (300 s restarts) of the queue at that moment.
    pub row: u32,
}

#[derive(Debug, Clone)]
struct Property {
    base: F32,
    slope: F32,
    lo: F32,
    hi: F32,
    t0: F32,
    t0_row: u32,
    /// Listeners in notification order (the newest first: the original links a new one at the head).
    subs: Vec<LisId>,
}

#[derive(Debug, Clone)]
struct Modifier {
    prop: PropId,
    rate: F32,
    active: bool,
    timer: Option<usize>,
}

#[derive(Debug, Clone)]
struct Listener {
    prop: PropId,
    mode: u32,
    threshold: F32,
    message: u32,
    timer: Option<usize>,
    alive: bool,
    /// A stand-in for "the listener has a timer" in raw tests ([`Agile::listener_raw`]).
    raw_dummy: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    Modifier(ModId),
    Listener(LisId),
}

#[derive(Debug, Clone)]
struct Entry {
    expiry: F32,
    seq: u32,
    owner: Option<Owner>,
    periodic: bool,
    period: F32,
    cancelled: bool,
}

/// The agile world: the property queue, its clock and every property / modifier / listener.
#[derive(Debug, Clone, Default)]
pub struct Agile {
    now: u32,
    row: u32,
    seq: u32,
    /// Pending entries (cancelled ones stay until they pop, as in the original). Indexed by a stable slot id.
    slots: Vec<Option<Entry>>,
    props: Vec<Property>,
    mods: Vec<Modifier>,
    lis: Vec<Listener>,
    events: Vec<Fired>,
}

impl Agile {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue time (the clock of the queue, which is also the game time inside a timer callback).
    pub fn now(&self) -> F32 {
        F32(self.now)
    }

    /// Number of 300 s restarts so far.
    pub fn row(&self) -> u32 {
        self.row
    }

    /// Sets the clock (tests, and hosts that join a running game).
    pub fn set_clock(&mut self, now: F32, row: u32) {
        self.now = now.0;
        self.row = row;
    }

    /// Pending entries including the cancelled ones (diagnostics).
    pub fn pending(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Takes the listener firings that happened outside a tick (creation, value changes).
    pub fn take_events(&mut self) -> Vec<Fired> {
        std::mem::take(&mut self.events)
    }

    // ------------------------------------------------------------------------------------------------ properties

    /// A property with base value `base` valid now, and limits.
    pub fn new_property(&mut self, base: F32, lo: F32, hi: F32) -> PropId {
        self.props.push(Property {
            base,
            slope: ZERO,
            lo,
            hi,
            t0: F32(self.now),
            t0_row: self.row,
            subs: Vec::new(),
        });
        PropId(self.props.len() - 1)
    }

    /// The time since `t0`, `0x6F483E70`: `snap(now - t0)` (below 1e-4 it is 0), plus `300 * rows` when the rows differ.
    fn elapsed(&self, t0: F32, t0_row: u32) -> F32 {
        let mut w = arith::sub(F32(self.now), t0);
        if f32::from_bits(w.0 & 0x7FFF_FFFF) < f32::from_bits(MIN_TIMER_BITS) {
            w = ZERO;
        }
        if self.row != t0_row {
            let rows = softfloat::conv::i2r(self.row.wrapping_sub(t0_row) as i32);
            w = arith::add(w, arith::mul(HORIZON, rows));
        }
        w
    }

    fn clamp(p: &Property, v: F32) -> F32 {
        if fl(p.lo) > fl(v) {
            p.lo
        } else if fl(p.hi) < fl(v) {
            p.hi
        } else {
            v
        }
    }

    fn eval(&self, p: &Property) -> F32 {
        let w = self.elapsed(p.t0, p.t0_row);
        Self::clamp(p, arith::add(p.base, arith::mul(p.slope, w)))
    }

    /// The value of a property now (`0x6F4773A0`).
    pub fn value(&self, p: PropId) -> F32 {
        self.eval(&self.props[p.0])
    }

    /// The slope of a property (diagnostics).
    pub fn slope(&self, p: PropId) -> F32 {
        self.props[p.0].slope
    }

    /// `0x6F4A90B0`: the base becomes the value now, valid from now.
    fn refresh(&mut self, p: PropId) {
        let v = self.eval(&self.props[p.0]);
        let (now, row) = (F32(self.now), self.row);
        let pr = &mut self.props[p.0];
        pr.base = v;
        pr.t0 = now;
        pr.t0_row = row;
    }

    /// `0x6F4A9320`: adds `delta` to the slope (a slope below 8e-7 becomes zero) and tells the listeners the new base.
    fn add_slope(&mut self, p: PropId, delta: F32, t: &Tables) {
        self.refresh(p);
        let s = arith::add(self.props[p.0].slope, delta);
        let s = if f32::from_bits(arith::sub(s, ZERO).0 & 0x7FFF_FFFF) < f32::from_bits(TINY_SLOPE_BITS) {
            ZERO
        } else {
            s
        };
        self.props[p.0].slope = s;
        let base = self.props[p.0].base;
        let subs = self.props[p.0].subs.clone();
        for l in subs {
            self.listener_value(l, base, t);
        }
    }

    /// Sets the value (`0x6F477350` via `0x6F4A92B0`): the base moves by `x - value` (clamped to the limits) and the
    /// listeners see the old and the new value. The slope is kept.
    pub fn set_value(&mut self, p: PropId, x: F32, t: &Tables) {
        let cur = self.eval(&self.props[p.0]);
        let delta = arith::sub(x, cur);
        self.refresh(p);
        let old = self.props[p.0].base;
        let new = Self::clamp(&self.props[p.0], arith::add(old, delta));
        self.props[p.0].base = new;
        let subs = self.props[p.0].subs.clone();
        for l in subs {
            self.listener_jump(l, old, new, t);
        }
    }

    // -------------------------------------------------------------------------------------------------- timers

    fn arm(&mut self, owner: Owner, duration: F32, periodic: bool, period_override: Option<F32>) -> usize {
        let d = if fl(duration) < f32::from_bits(MIN_TIMER_BITS) || fl(duration).is_nan() {
            F32(MIN_TIMER_BITS)
        } else {
            duration
        };
        self.seq += 1;
        let e = Entry {
            expiry: arith::add(F32(self.now), d),
            seq: self.seq,
            owner: Some(owner),
            periodic,
            period: period_override.unwrap_or(d),
            cancelled: false,
        };
        self.slots.push(Some(e));
        self.slots.len() - 1
    }

    /// A timer that exists only to consume a sequence number (the 1e-4 cleanup entry the original arms when a
    /// listener is linked; it is cancelled at once).
    fn arm_dead(&mut self) {
        self.seq += 1;
        let e = Entry {
            expiry: arith::add(F32(self.now), F32(MIN_TIMER_BITS)),
            seq: self.seq,
            owner: None,
            periodic: true,
            period: F32(MIN_TIMER_BITS),
            cancelled: true,
        };
        self.slots.push(Some(e));
    }

    fn cancel_timer(&mut self, slot: Option<usize>) {
        if let Some(i) = slot {
            if let Some(e) = self.slots[i].as_mut() {
                e.cancelled = true;
            }
        }
    }

    // ---------------------------------------------------------------------------------------------- modifiers

    /// The modifier creator `0x6F4774E0`: `rate` per agile second on `p` for `horizon` seconds (a horizon below 2e-7
    /// means forever). The rate is added to the slope at once.
    pub fn add_modifier(&mut self, p: PropId, rate: F32, horizon: F32, t: &Tables) -> ModId {
        let horizon = if f32::from_bits(arith::sub(horizon, ZERO).0 & 0x7FFF_FFFF) < f32::from_bits(TINY_RATE_BITS) {
            F32(FOREVER_BITS)
        } else {
            horizon
        };
        // Creation sets the rate with a difference test against 0.0: a rate below 2e-7 stays 0.0.
        let stored = if f32::from_bits(arith::sub(ZERO, rate).0 & 0x7FFF_FFFF) < f32::from_bits(TINY_RATE_BITS) {
            ZERO
        } else {
            rate
        };
        let id = ModId(self.mods.len());
        self.mods.push(Modifier { prop: p, rate: stored, active: true, timer: None });
        // Linking sends "begin" to the modifier: the rate joins the slope, the lifetime timer starts.
        self.add_slope(p, stored, t);
        let slot = if fl(horizon) < f32::from_bits(FOREVER_BITS) {
            self.arm(Owner::Modifier(id), horizon, false, None)
        } else {
            self.arm(Owner::Modifier(id), F32(FOREVER_PERIOD_BITS), true, None)
        };
        self.mods[id.0].timer = Some(slot);
        id
    }

    /// Ends a modifier early (the original's destroy path sends "end": the rate leaves the slope).
    pub fn remove_modifier(&mut self, m: ModId, t: &Tables) {
        if !self.mods[m.0].active {
            return;
        }
        let timer = self.mods[m.0].timer.take();
        self.cancel_timer(timer);
        self.end_modifier(m, t);
    }

    fn end_modifier(&mut self, m: ModId, t: &Tables) {
        self.mods[m.0].active = false;
        let (p, rate) = (self.mods[m.0].prop, self.mods[m.0].rate);
        self.add_slope(p, F32(rate.0 ^ 0x8000_0000), t);
    }

    // ----------------------------------------------------------------------------------------------- listeners

    /// The listener creator `0x6F477550`: fires `message` when `p` reaches `threshold` (mode 0: falling, mode 1:
    /// rising). The first evaluation happens at once (the listener can fire inside this call; see
    /// [`Agile::take_events`]).
    pub fn add_listener(&mut self, p: PropId, mode: u32, threshold: F32, message: u32, t: &Tables) -> LisId {
        let id = LisId(self.lis.len());
        self.lis.push(Listener { prop: p, mode, threshold, message, timer: None, alive: true, raw_dummy: None });
        // Creation arms the 0.1 s timer; linking arms (and cancels) the cleanup entry and subscribes at the head.
        let slot = self.arm(Owner::Listener(id), F32(FIRST_LISTENER_TIMER_BITS), true, None);
        self.lis[id.0].timer = Some(slot);
        self.arm_dead();
        self.props[p.0].subs.insert(0, id);
        let v = self.eval(&self.props[p.0]);
        self.listener_value(id, v, t);
        id
    }

    /// Removes a listener (what the owner does when it has completed). UNSURE: the original's destroy path was not
    /// run; this cancels the timer and unsubscribes.
    pub fn remove_listener(&mut self, l: LisId) {
        let timer = self.lis[l.0].timer.take();
        self.cancel_timer(timer);
        self.lis[l.0].alive = false;
        let p = self.lis[l.0].prop;
        self.props[p.0].subs.retain(|&x| x != l);
    }

    fn near(&self, l: LisId, v: F32) -> bool {
        abs_sub(v, self.lis[l.0].threshold) < f32::from_bits(TOLERANCE_BITS)
    }

    fn fire(&mut self, l: LisId) {
        let timer = self.lis[l.0].timer.take();
        self.cancel_timer(timer);
        let message = self.lis[l.0].message;
        self.events.push(Fired { listener: l, message, at: F32(self.now), row: self.row });
    }

    /// `0x6F4A9C70`: the value `v` is within the tolerance of the threshold: fire; otherwise look for the crossing.
    fn listener_value(&mut self, l: LisId, v: F32, t: &Tables) {
        if !self.lis[l.0].alive {
            return;
        }
        if self.near(l, v) {
            self.fire(l);
        } else {
            self.listener_arm(l, t);
        }
    }

    /// `0x6F4A9940`: did the value jump across the threshold (in the direction of the mode)?
    fn crossed(&self, l: LisId, old: F32, new: F32) -> bool {
        let lis = &self.lis[l.0];
        let thr = fl(lis.threshold);
        let near_old = self.near(l, old);
        let near_new = self.near(l, new);
        let has_timer = lis.timer.is_some();
        if lis.mode == 0 {
            let from = if near_old { has_timer } else { thr < fl(old) };
            from && (near_new || thr > fl(new))
        } else {
            // Any other mode takes the rising branch (`0x6F4A9A07`).
            let from = if near_old { has_timer } else { fl(old) < thr };
            from && (near_new || thr < fl(new))
        }
    }

    /// `0x6F4A9CF0`: a jump of the value from `old` to `new`.
    fn listener_jump(&mut self, l: LisId, old: F32, new: F32, t: &Tables) {
        if !self.lis[l.0].alive {
            return;
        }
        if self.crossed(l, old, new) {
            self.fire(l);
        } else {
            self.listener_arm(l, t);
        }
    }

    /// `0x6F4A8FD0`: seconds until the property reaches `thr`: `(thr - value) / slope`, or -1.0 when `thr` is outside
    /// the limits.
    fn time_to_cross(&self, p: PropId, thr: F32, t: &Tables) -> F32 {
        let pr = &self.props[p.0];
        if fl(pr.lo) > fl(thr) || fl(pr.hi) < fl(thr) {
            return F32(NEG_ONE_BITS);
        }
        let v = self.eval(pr);
        arith::div(arith::sub(thr, v), pr.slope, t)
    }

    /// `0x6F4A9B70`: cancel the timer and, when the slope heads for the threshold, arm one for the crossing (at most
    /// 4 seconds ahead).
    fn listener_arm(&mut self, l: LisId, t: &Tables) {
        let timer = self.lis[l.0].timer.take();
        self.cancel_timer(timer);
        let (p, mode, thr) = (self.lis[l.0].prop, self.lis[l.0].mode, self.lis[l.0].threshold);
        let s = self.props[p.0].slope;
        let x = arith::sub(s, ZERO);
        if !(f32::from_bits(TINY_SLOPE_BITS) <= f32::from_bits(x.0 & 0x7FFF_FFFF)) {
            return;
        }
        let heading = if mode == 1 { fl(x) > 0.0 } else if mode == 0 { fl(s) < 0.0 } else { false };
        if !heading {
            return;
        }
        let mut dt = self.time_to_cross(p, thr, t);
        if !(0.0 < fl(dt)) && !(abs_sub(dt, ZERO) < f32::from_bits(TINY_SLOPE_BITS)) {
            return;
        }
        if fl(dt) > fl(F32(LISTENER_CAP_BITS)) {
            dt = F32(LISTENER_CAP_BITS);
        }
        let slot = self.arm(Owner::Listener(l), dt, true, None);
        self.lis[l.0].timer = Some(slot);
    }

    // -------------------------------------------------------------------------------------- raw access (vectors)

    /// A property with every field given (tests against the per-function vectors).
    pub fn property_raw(&mut self, base: F32, slope: F32, lo: F32, hi: F32, t0: F32, t0_row: u32) -> PropId {
        self.props.push(Property { base, slope, lo, hi, t0, t0_row, subs: Vec::new() });
        PropId(self.props.len() - 1)
    }

    /// `(base, slope, t0, t0 row)` of a property.
    pub fn property_state(&self, p: PropId) -> (F32, F32, F32, u32) {
        let pr = &self.props[p.0];
        (pr.base, pr.slope, pr.t0, pr.t0_row)
    }

    /// Adds `delta` to the slope of `p` (`0x6F4A9320`): the base is refreshed first.
    pub fn apply_rate(&mut self, p: PropId, delta: F32, t: &Tables) {
        self.add_slope(p, delta, t);
    }

    /// A listener that exists without creation side effects. `with_timer` gives it a stand-in timer (so that the
    /// "has a timer" test of the crossing detector is true); the stand-in is cancelled by an evaluation.
    pub fn listener_raw(&mut self, p: PropId, mode: u32, threshold: F32, message: u32, with_timer: bool) -> LisId {
        let id = LisId(self.lis.len());
        self.lis.push(Listener { prop: p, mode, threshold, message, timer: None, alive: true, raw_dummy: None });
        if with_timer {
            self.slots.push(Some(Entry {
                expiry: F32(FOREVER_BITS),
                seq: 0,
                owner: None,
                periodic: false,
                period: ZERO,
                cancelled: false,
            }));
            let slot = self.slots.len() - 1;
            self.lis[id.0].timer = Some(slot);
            self.lis[id.0].raw_dummy = Some(slot);
        }
        id
    }

    /// `0x6F4A9C70` with the value `v`.
    pub fn evaluate(&mut self, l: LisId, v: F32, t: &Tables) {
        self.listener_value(l, v, t);
    }

    /// `0x6F4A9CF0`: the value jumped from `old` to `new`.
    pub fn jump(&mut self, l: LisId, old: F32, new: F32, t: &Tables) {
        self.listener_jump(l, old, new, t);
    }

    /// `0x6F4A8FD0`.
    pub fn crossing_time(&self, p: PropId, thr: F32, t: &Tables) -> F32 {
        self.time_to_cross(p, thr, t)
    }

    /// The live timer of a listener other than the raw stand-in: `(expiry, sequence, periodic, period)`.
    pub fn listener_timer(&self, l: LisId) -> Option<(F32, u32, bool, F32)> {
        let slot = self.lis[l.0].timer?;
        if Some(slot) == self.lis[l.0].raw_dummy {
            return None;
        }
        let e = self.slots[slot].as_ref()?;
        Some((e.expiry, e.seq, e.periodic, e.period))
    }

    /// The lifetime timer of a modifier: `(expiry, sequence, periodic, period)`.
    pub fn modifier_timer(&self, m: ModId) -> Option<(F32, u32, bool, F32)> {
        let e = self.slots[self.mods[m.0].timer?].as_ref()?;
        Some((e.expiry, e.seq, e.periodic, e.period))
    }

    /// Whether the raw stand-in timer of a listener was cancelled.
    pub fn raw_timer_cancelled(&self, l: LisId) -> bool {
        self.lis[l.0]
            .raw_dummy
            .and_then(|s| self.slots[s].as_ref())
            .is_some_and(|e| e.cancelled)
    }

    // ----------------------------------------------------------------------------------------------------- tick

    /// One tick of the property queue (`0x6F46E9E0`): the clock gains `inc`, the due timers fire, and at 300 s the
    /// clock restarts. Returns the listeners that fired in this tick.
    pub fn tick(&mut self, inc: F32, t: &Tables) -> Vec<Fired> {
        let tmp = arith::add(F32(self.now), inc);
        if fl(HORIZON) > fl(tmp) {
            self.now = tmp.0;
            self.pump(t);
        } else {
            let excess = arith::sub(tmp, HORIZON);
            let excess = if f32::from_bits(arith::sub(excess, ZERO).0 & 0x7FFF_FFFF) < f32::from_bits(TINY_SLOPE_BITS) {
                ZERO
            } else {
                excess
            };
            self.now = HORIZON.0;
            self.pump(t);
            for e in self.slots.iter_mut().flatten() {
                e.expiry = arith::sub(e.expiry, HORIZON);
            }
            self.row += 1;
            self.now = excess.0;
            self.pump(t);
        }
        std::mem::take(&mut self.events)
    }

    fn next_due(&self, target: F32) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if let Some(e) = s {
                if fl(target) < fl(e.expiry) {
                    continue;
                }
                match best {
                    None => best = Some(i),
                    Some(b) => {
                        let o = self.slots[b].as_ref().unwrap();
                        if fl(e.expiry) < fl(o.expiry) || (fl(e.expiry) == fl(o.expiry) && e.seq < o.seq) {
                            best = Some(i);
                        }
                    }
                }
            }
        }
        best
    }

    /// `0x6F46E910`: pops every entry with expiry <= the clock; the clock is the entry's expiry while it runs.
    fn pump(&mut self, t: &Tables) {
        let target = F32(self.now);
        // The earliest entry overall must pop first, so look at all entries (cancelled ones too).
        while let Some(i) = self.next_due(target) {
            let e = self.slots[i].take().unwrap();
            self.now = e.expiry.0;
            if e.cancelled {
                continue;
            }
            self.run_timer(i, &e, t);
            // Periodic and not cancelled meanwhile: re-arm at expiry + period, same sequence number.
            let still_ours = match e.owner {
                Some(Owner::Modifier(m)) => self.mods[m.0].timer == Some(i),
                Some(Owner::Listener(l)) => self.lis[l.0].timer == Some(i),
                None => false,
            };
            if e.periodic && still_ours {
                let mut again = e.clone();
                again.expiry = arith::add(F32(self.now), e.period);
                self.slots[i] = Some(again);
            }
        }
        self.now = target.0;
    }

    fn run_timer(&mut self, slot: usize, e: &Entry, t: &Tables) {
        match e.owner {
            Some(Owner::Modifier(m)) => {
                if self.mods[m.0].active && !e.periodic {
                    // Lifetime over: the timer is gone, the rate leaves the slope.
                    self.mods[m.0].timer = None;
                    self.end_modifier(m, t);
                }
            }
            Some(Owner::Listener(l)) => {
                if self.lis[l.0].alive && self.lis[l.0].timer == Some(slot) {
                    let p = self.lis[l.0].prop;
                    let v = self.eval(&self.props[p.0]);
                    self.listener_value(l, v, t);
                }
            }
            None => {}
        }
    }
}
