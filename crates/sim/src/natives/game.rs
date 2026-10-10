//! Game state behind the timer and trigger natives: an arena of game objects (timers, triggers,
//! boolexprs, trigger conditions and actions, events), the handle table that gives them ids, the
//! timer queue and the context keys of running script threads.
//!
//! Objects are never removed from the arena. A destroyed object keeps its [`ObjKey`] but has no live
//! handle any more, so every native that receives a handle goes through [`Game::handle_object`] and
//! sees `None` for a destroyed or stale id.
//!
//! Context keys: each script thread started by a timer handler or a trigger carries a key (see
//! `jass::vm::NativeCtx::thread_key`). The key names a [`Ctx`] (which timer or which trigger and
//! event the thread runs for), so natives such as `GetTriggeringTrigger` can answer per thread.
//! Keys are interned in creation order (1, 2, ...), so the same `Ctx` always gets the same key.

use std::collections::BTreeMap;

use jass::vm::{RunStatus, ThreadId, Value};
use softfloat::F32;

use crate::handles::HandleTable;
use crate::timer::{TimerId, TimerQueue};

/// Index of a game object in the arena. The handle table keys objects by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjKey(pub u32);

/// What a queued timer entry is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// A `TriggerSleepAction`: the script thread resumes at the expiry.
    Sleep(ThreadId),
    /// A timer handle expires: its handler runs and the triggers that listen to it fire.
    Timer(ObjKey),
    /// A timer event registered with `TriggerRegisterTimerEvent` fires its trigger.
    Event(ObjKey),
}

/// The timer or trigger that a script thread runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ctx {
    /// A trigger's condition or action chain, with the event that started it (if any).
    Trigger {
        trigger: ObjKey,
        event: Option<ObjKey>,
    },
    /// A timer handler started at the timer's expiry.
    Timer(ObjKey),
}

/// A timer handle (`timer` in JASS).
#[derive(Debug, Clone)]
pub struct TimerObj {
    /// The timeout as it was given to `TimerStart` (not clamped). Returned by `TimerGetTimeout`.
    pub timeout: F32,
    pub periodic: bool,
    /// The handler given to `TimerStart`, if any.
    pub handler: Option<u32>,
    /// The queue entry while the timer runs. `None` when paused, expired or never started.
    pub active: Option<TimerId>,
    /// Seconds left while paused (and the value a `ResumeTimer` restarts with).
    pub paused_remaining: F32,
    /// Triggers registered with `TriggerRegisterTimerExpireEvent`: `(trigger, event)`, in
    /// registration order.
    pub listeners: Vec<(ObjKey, ObjKey)>,
}

impl Default for TimerObj {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerObj {
    pub fn new() -> Self {
        TimerObj {
            timeout: F32::ZERO,
            periodic: false,
            handler: None,
            active: None,
            paused_remaining: F32::ZERO,
            listeners: Vec::new(),
        }
    }
}

/// A `trigger` handle with its lists.
#[derive(Debug, Clone)]
pub struct TriggerObj {
    /// Set by `EnableTrigger` and `DisableTrigger`. New triggers are enabled.
    pub enabled: bool,
    /// Storage for `TriggerWaitOnSleeps`. It does not change the execution path yet.
    pub wait_on_sleeps: bool,
    /// `TriggerCondition` objects, in the order they were added.
    pub conditions: Vec<ObjKey>,
    /// `TriggerAction` objects, in the order they were added.
    pub actions: Vec<ObjKey>,
    /// `Event` objects registered on this trigger.
    pub events: Vec<ObjKey>,
    /// Evaluation count (`GetTriggerEvalCount`). Wraps like a JASS integer.
    pub evals: i32,
    /// Execution count (`GetTriggerExecCount`). Wraps like a JASS integer.
    pub execs: i32,
}

impl Default for TriggerObj {
    fn default() -> Self {
        Self::new()
    }
}

impl TriggerObj {
    pub fn new() -> Self {
        TriggerObj {
            enabled: true,
            wait_on_sleeps: false,
            conditions: Vec::new(),
            actions: Vec::new(),
            events: Vec::new(),
            evals: 0,
            execs: 0,
        }
    }
}

/// The kind of a `boolexpr` (also the `conditionfunc` and `filterfunc` handle types).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolExpr {
    /// `Condition(code)` or `Filter(code)`: calls the code, which returns a boolean.
    Leaf(u32),
    /// `And(a, b)`.
    And(ObjKey, ObjKey),
    /// `Or(a, b)`.
    Or(ObjKey, ObjKey),
    /// `Not(a)`.
    Not(ObjKey),
}

/// A condition of a trigger: one `boolexpr`, referenced by the trigger.
#[derive(Debug, Clone, Copy)]
pub struct Condition {
    pub trigger: ObjKey,
    pub boolexpr: ObjKey,
}

/// An action of a trigger: one function, run by the trigger's action chain.
#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub trigger: ObjKey,
    pub code: u32,
}

/// Where an event comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    /// `TriggerRegisterTimerEvent`: a timer of its own, which the queue re-arms when periodic.
    Timer(Option<TimerId>),
    /// `TriggerRegisterTimerExpireEvent`: fires when this timer handle expires.
    Expire(ObjKey),
}

/// A trigger event (`event` in JASS).
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub trigger: ObjKey,
    pub source: EventSource,
}

/// A handle type the simulation models. Typed natives (`SaveTimerHandle`, `LoadLocationHandle`, ...)
/// accept a handle only when its object has this kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    Timer,
    Trigger,
    BoolExpr,
    Event,
    Condition,
    Action,
    Location,
    Rect,
    Hashtable,
    GameCache,
}

/// A `location` handle: a point with raw coordinates (no clamping, no terrain height).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationObj {
    pub x: F32,
    pub y: F32,
}

/// A `rect` handle. The four edges are stored raw; `SetRect` and friends sort and clamp them first
/// (see `natives/location.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectObj {
    pub min_x: F32,
    pub min_y: F32,
    pub max_x: F32,
    pub max_y: F32,
}

/// A reference held by a saved handle value. `owned` is the original's entry flag: when it is set the
/// table holds one reference to `id` (taken with `add_ref`, dropped with `release`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedHandle {
    pub id: u32,
    pub owned: bool,
}

/// The saved values of one hashtable or game cache. Every value kind has its own map, keyed by the
/// pair (parent key, child key). Missing keys read as the default of the kind (0, 0.0, false, "").
#[derive(Debug, Clone)]
pub struct Slots<P: Ord + Clone, C: Ord + Clone> {
    pub ints: BTreeMap<(P, C), i32>,
    pub reals: BTreeMap<(P, C), F32>,
    pub bools: BTreeMap<(P, C), bool>,
    pub strings: BTreeMap<(P, C), String>,
    pub handles: BTreeMap<(P, C), SavedHandle>,
}

impl<P: Ord + Clone, C: Ord + Clone> Slots<P, C> {
    pub fn new() -> Self {
        Slots {
            ints: BTreeMap::new(),
            reals: BTreeMap::new(),
            bools: BTreeMap::new(),
            strings: BTreeMap::new(),
            handles: BTreeMap::new(),
        }
    }

    /// Removes every value of parent `parent` (all kinds). Returns the ids whose references the
    /// table held, in key order; the caller releases them.
    pub fn remove_parent(&mut self, parent: &P) -> Vec<u32> {
        let released = self
            .handles
            .iter()
            .filter(|((p, _), h)| p == parent && h.owned)
            .map(|(_, h)| h.id)
            .collect();
        self.ints.retain(|(p, _), _| p != parent);
        self.reals.retain(|(p, _), _| p != parent);
        self.bools.retain(|(p, _), _| p != parent);
        self.strings.retain(|(p, _), _| p != parent);
        self.handles.retain(|(p, _), _| p != parent);
        released
    }

    /// Removes every value. Returns the ids whose references the table held, in key order.
    pub fn clear(&mut self) -> Vec<u32> {
        let released = self
            .handles
            .values()
            .filter(|h| h.owned)
            .map(|h| h.id)
            .collect();
        self.ints.clear();
        self.reals.clear();
        self.bools.clear();
        self.strings.clear();
        self.handles.clear();
        released
    }
}

impl<P: Ord + Clone, C: Ord + Clone> Default for Slots<P, C> {
    fn default() -> Self {
        Self::new()
    }
}

/// The playable area, as the original keeps it in its game-state struct (min x, min y, max x, max y).
/// Rect coordinates are clamped to it. The map loader does not fill it in yet, so until a host sets
/// it ([`Game::set_playable`]) rects are not clamped (UNSURE, see `natives/location.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playable {
    pub min_x: F32,
    pub min_y: F32,
    pub max_x: F32,
    pub max_y: F32,
}

/// One game object of the arena.
#[derive(Debug, Clone)]
pub enum Object {
    Timer(TimerObj),
    Trigger(TriggerObj),
    BoolExpr(BoolExpr),
    Condition(Condition),
    Action(Action),
    Event(Event),
    Location(LocationObj),
    Rect(RectObj),
    /// A hashtable: saved values keyed by (parent, child) integers.
    Hashtable(super::hashtable::HashtableObj),
    /// A game cache: saved values keyed by (mission, key) strings.
    GameCache(Slots<String, String>),
}

impl Object {
    /// The handle type of this object.
    pub fn handle_kind(&self) -> HandleKind {
        match self {
            Object::Timer(_) => HandleKind::Timer,
            Object::Trigger(_) => HandleKind::Trigger,
            Object::BoolExpr(_) => HandleKind::BoolExpr,
            Object::Condition(_) => HandleKind::Condition,
            Object::Action(_) => HandleKind::Action,
            Object::Event(_) => HandleKind::Event,
            Object::Location(_) => HandleKind::Location,
            Object::Rect(_) => HandleKind::Rect,
            Object::Hashtable(_) => HandleKind::Hashtable,
            Object::GameCache(_) => HandleKind::GameCache,
        }
    }
}

/// The timer and trigger state of one game. Shared between the natives and the host through an
/// `Rc<RefCell<Game>>`. Natives must not keep a borrow of it while they run script code.
#[derive(Debug)]
pub struct Game {
    objects: Vec<Object>,
    handles: HandleTable<ObjKey>,
    /// Pending timers: sleeps, timer expiries and trigger events. Its time is the last expiry or
    /// pump target.
    pub timers: TimerQueue<Payload>,
    /// `Ctx` of key `i + 1` is `contexts[i]`.
    contexts: Vec<Ctx>,
    context_keys: BTreeMap<Ctx, u32>,
    /// Game caches by name. A cache is created by `InitGameCache` and never destroyed.
    caches: BTreeMap<String, ObjKey>,
    /// The playable area that rect coordinates are clamped to, or `None` (no clamp) until a host sets it.
    playable: Option<Playable>,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        Game {
            objects: Vec::new(),
            handles: HandleTable::new(),
            timers: TimerQueue::new(),
            contexts: Vec::new(),
            context_keys: BTreeMap::new(),
            caches: BTreeMap::new(),
            playable: None,
        }
    }

    /// Sets the playable area (`None` disables the rect clamp). See [`Playable`].
    pub fn set_playable(&mut self, playable: Option<Playable>) {
        self.playable = playable;
    }

    /// The playable area, if one is set.
    pub fn playable(&self) -> Option<Playable> {
        self.playable
    }

    /// The game cache named `name`, if `InitGameCache` created it and it is still alive.
    pub fn cache_named(&self, name: &str) -> Option<ObjKey> {
        self.caches.get(name).copied().filter(|&k| self.is_alive(k))
    }

    /// Records `key` as the game cache named `name`.
    pub fn set_cache_named(&mut self, name: &str, key: ObjKey) {
        self.caches.insert(name.to_string(), key);
    }

    /// Number of objects ever created (destroyed ones included).
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// The handle table (for inspection and tests).
    pub fn handles(&self) -> &HandleTable<ObjKey> {
        &self.handles
    }

    /// The object behind `key`. Panics for a key this game did not create.
    pub fn object(&self, key: ObjKey) -> &Object {
        &self.objects[key.0 as usize]
    }

    /// Mutable access to the object behind `key`.
    pub fn object_mut(&mut self, key: ObjKey) -> &mut Object {
        &mut self.objects[key.0 as usize]
    }

    /// Adds an object and gives it a handle with the given flag (see `HandleTable::create`). Returns
    /// the object's key and its handle id.
    pub fn add_object(&mut self, object: Object, flag: u32) -> (ObjKey, u32) {
        let key = ObjKey(self.objects.len() as u32);
        self.objects.push(object);
        let id = self.handles.create(key, flag);
        (key, id)
    }

    /// The live object of a handle id, or `None` for the null handle, a freed id, or an object that
    /// was destroyed.
    pub fn handle_object(&self, id: u32) -> Option<ObjKey> {
        self.handles.get(id).copied()
    }

    /// The live object of handle `id` if it has the handle type `kind` (the typed check of the
    /// original's natives: a null, freed or wrong-type handle gives `None`).
    pub fn handle_of_kind(&self, id: u32, kind: HandleKind) -> Option<ObjKey> {
        let k = self.handle_object(id)?;
        (self.object(k).handle_kind() == kind).then_some(k)
    }

    /// Adds one reference to handle `id` (the original's AddRef, used by hashtable saves). Null and
    /// unknown ids are ignored.
    pub fn add_ref(&mut self, id: u32) {
        self.handles.add_ref(id);
    }

    /// Drops one reference to handle `id`. The id is freed when its count reaches 0. Null and unknown
    /// ids are ignored.
    pub fn release(&mut self, id: u32) {
        self.handles.release(id);
    }

    /// Drops one reference to each id in `ids`, in order.
    pub fn release_all(&mut self, ids: &[u32]) {
        for &id in ids {
            self.handles.release(id);
        }
    }

    /// The handle id of a live object, or 0 when it has none.
    pub fn handle_of(&self, key: ObjKey) -> u32 {
        self.handles.id_of(&key).unwrap_or(0)
    }

    /// Whether the object still has a live handle.
    pub fn is_alive(&self, key: ObjKey) -> bool {
        self.handles.id_of(&key).is_some()
    }

    /// Releases the object's creation reference (its handle is freed unless other holders keep the
    /// id reserved). The object's data stays in the arena for the scripts that still hold it.
    pub fn kill(&mut self, key: ObjKey) {
        self.handles.object_died(&key);
    }

    /// The context key of `ctx`, interned: the same context always gets the same key.
    pub fn context_key(&mut self, ctx: Ctx) -> u32 {
        if let Some(&k) = self.context_keys.get(&ctx) {
            return k;
        }
        self.contexts.push(ctx);
        let k = self.contexts.len() as u32;
        self.context_keys.insert(ctx, k);
        k
    }

    /// The context of a thread key, or `None` for key 0 and unknown keys.
    pub fn context(&self, key: u32) -> Option<Ctx> {
        let i = key.checked_sub(1)? as usize;
        self.contexts.get(i).copied()
    }
}

/// How a native or the host runs script code: synchronously (conditions), or as a chain that runs
/// until its first stop (trigger actions and timer handlers). Implemented for the native context
/// (where a chain starts when the native returns) and for the VM (where it starts at once).
pub trait ScriptRuntime {
    /// Runs the no-argument function `func` to completion and returns its value.
    fn call_keyed(&mut self, func: u32, key: u32) -> Result<Value, RunStatus>;
    /// Starts a thread that runs `chain` one function after another, with context key `key`.
    fn chain_keyed(&mut self, chain: Vec<u32>, key: u32);
}

impl ScriptRuntime for jass::vm::NativeCtx<'_> {
    fn call_keyed(&mut self, func: u32, key: u32) -> Result<Value, RunStatus> {
        self.call_func_keyed(func, &[], key)
    }

    fn chain_keyed(&mut self, chain: Vec<u32>, key: u32) {
        self.spawn_chain(chain, key);
    }
}

impl ScriptRuntime for jass::vm::Vm {
    fn call_keyed(&mut self, func: u32, key: u32) -> Result<Value, RunStatus> {
        self.call_func(func, &[], key)
    }

    fn chain_keyed(&mut self, chain: Vec<u32>, key: u32) {
        let now = self.now();
        self.run_chain(&chain, key, now);
    }
}
