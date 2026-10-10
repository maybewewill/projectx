//! JASS virtual machine: values, threads, natives and a deterministic scheduler.
//!
//! Behaviour follows docs/spec/jass-vm.md where the spec is CONFIRMED or LIKELY. Everything else
//! is marked `UNVERIFIED vs retail` in the code and listed in the crate report.
//!
//! Determinism: no wall clock, no randomness, no hash-map iteration in observable behaviour.
//! Game time is supplied by the caller through `Vm::step(now)` or `Vm::resume(id, now)`.
//!
//! Real numbers are `softfloat::F32` bit patterns and every real operation goes through the
//! bit-exact helpers of the `softfloat` crate. The helpers that use the game's lookup tables
//! (division, trigonometry, powers, logarithms, square root) need a `softfloat::Tables`
//! read from the user's own `Game.dll`. Without tables those operations stop the thread with
//! `RunStatus::MissingTables`; they never fall back to IEEE math.

use std::any::Any;
use std::collections::{BTreeMap, VecDeque};

use crate::ast::ident_key;
use std::rc::Rc;

use softfloat::{arith, conv, Tables, F32};

use crate::bytecode::{Instr, Op};
use crate::compile::{Module, Ty, ARRAY_SIZE};

/// Instructions a thread may execute per scheduler pass.
///
/// LIKELY: this is the "op limit" of the original's per-run instruction counter (the spec finds
/// the constant 300000 at every top-level call site). Whether a real game run hits it the same way
/// is UNVERIFIED.
pub const DEFAULT_BUDGET: u32 = 300_000;

/// Native-initiated synchronous thread starts (ExecuteFunc) nest on the Rust stack. Beyond this
/// depth the new thread is queued for the scheduler instead of run inline.
/// UNVERIFIED vs retail: the original's nesting limit is unknown; this only bounds recursion here.
const MAX_NESTED_RUNS: u32 = 16;

// ---------------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------------

/// A JASS value.
///
/// Null handling: `Null` is the `null` literal. For handle, string and code variables the default
/// (zero) value is `Handle(0)` / `String(0)` / `Null`; all of these compare equal to `null`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Null,
    Integer(i32),
    /// IEEE binary32 bit pattern, operated on by `softfloat`.
    Real(F32),
    Boolean(bool),
    /// Index into the string table. 0 is the null (empty) string.
    String(u32),
    /// Index into the handle table (natives create handles).
    Handle(u32),
    /// Script function id.
    Code(u32),
}

impl Value {
    /// The default value of a variable of type `ty`.
    pub fn default_for(ty: Ty) -> Value {
        match ty {
            Ty::Integer => Value::Integer(0),
            Ty::Real => Value::Real(F32::ZERO),
            Ty::Boolean => Value::Boolean(false),
            Ty::String => Value::String(0),
            Ty::Handle => Value::Handle(0),
            Ty::Code | Ty::Null => Value::Null,
        }
    }

    pub fn as_integer(self) -> Option<i32> {
        match self {
            Value::Integer(i) => Some(i),
            _ => None,
        }
    }

    pub fn as_real(self) -> Option<F32> {
        match self {
            Value::Real(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_boolean(self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(b),
            _ => None,
        }
    }

    /// The string id; `Null` counts as the null string (id 0).
    pub fn as_string_id(self) -> Option<u32> {
        match self {
            Value::String(id) => Some(id),
            Value::Null => Some(0),
            _ => None,
        }
    }

    /// The handle id; `Null` counts as the null handle (0).
    pub fn as_handle(self) -> Option<u32> {
        match self {
            Value::Handle(h) => Some(h),
            Value::Null => Some(0),
            _ => None,
        }
    }

    /// Whether the value is null, or the zero value of a handle or string.
    fn is_null_like(self) -> bool {
        matches!(self, Value::Null | Value::Handle(0) | Value::String(0))
    }

    /// Whether this value may be stored in a slot or parameter of type `ty`.
    fn fits(self, ty: Ty) -> bool {
        matches!(
            (self, ty),
            (Value::Null, _)
                | (Value::Integer(_), Ty::Integer)
                | (Value::Real(_), Ty::Real)
                | (Value::Boolean(_), Ty::Boolean)
                | (Value::String(_), Ty::String)
                | (Value::Handle(_), Ty::Handle)
                | (Value::Code(_), Ty::Code)
        )
    }
}

/// JASS `==`. Null equals any null-like value. Reals compare with `softfloat::arith::approx_eq`
/// (`|a - b| < 0.001`, the original's real equality, spec softfloat 2 and 3.4). An integer against
/// a real converts the integer with `I2R` first (UNVERIFIED vs retail: the original's mixed-type
/// behaviour is not in the spec).
pub fn jass_equal(a: Value, b: Value) -> bool {
    if a.is_null_like() || b.is_null_like() {
        return a.is_null_like() && b.is_null_like();
    }
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x == y,
        (Value::Real(x), Value::Real(y)) => arith::approx_eq(x, y),
        (Value::Integer(x), Value::Real(y)) | (Value::Real(y), Value::Integer(x)) => {
            arith::approx_eq(conv::i2r(x), y)
        }
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Handle(x), Value::Handle(y)) => x == y,
        (Value::Code(x), Value::Code(y)) => x == y,
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------------------------

/// Interned string table. Equal contents always get the same id, so string equality is id equality.
///
/// UNVERIFIED vs retail: the original reference-counts entries and frees them. Here entries are
/// never freed, so the table only grows.
#[derive(Debug, Clone, Default)]
pub struct StringTable {
    list: Vec<String>,
    index: BTreeMap<String, u32>,
}

impl StringTable {
    /// A table holding only the null (empty) string at id 0.
    pub fn new() -> Self {
        let mut t = StringTable::default();
        t.list.push(String::new());
        t.index.insert(String::new(), 0);
        t
    }

    /// Builds a table from the module's symbol names: entry i is symbol i, so a string literal's
    /// value is its symbol id (see `bytecode::Op::LoadConst`). A name that occurs twice keeps its
    /// first id for lookups.
    pub fn from_symbols(list: &[String]) -> Self {
        let mut t = StringTable::default();
        for (i, s) in list.iter().enumerate() {
            t.list.push(s.clone());
            t.index.entry(s.clone()).or_insert(i as u32);
        }
        if t.list.is_empty() {
            t.list.push(String::new());
        }
        t.index.entry(String::new()).or_insert(0);
        t
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = self.list.len() as u32;
        self.list.push(s.to_string());
        self.index.insert(s.to_string(), id);
        id
    }

    /// The id of `s` if it has been interned.
    pub fn find(&self, s: &str) -> Option<u32> {
        self.index.get(&ident_key(s)).copied()
    }

    /// The id of exactly this text (no case folding).
    pub fn exact_id(&self, s: &str) -> Option<u32> {
        self.index.get(s).copied()
    }

    pub fn get(&self, id: u32) -> &str {
        self.list.get(id as usize).map_or("", String::as_str)
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

// ---------------------------------------------------------------------------------------------
// Natives
// ---------------------------------------------------------------------------------------------

/// What a native asks the VM to do after it returns.
pub enum NativeResult {
    /// Return this value (ignored by the caller for `returns nothing`).
    Return(Value),
    /// Suspend the calling thread for this many seconds of game time. The call completes with no
    /// value; the thread continues after the call once the time has passed.
    Sleep(F32),
    /// Give up the rest of this pass. The call completes with no value.
    Yield,
    /// Abort the calling thread with this status.
    Fail(RunStatus),
}

/// Context handed to natives: string table, game time, lookup tables, the calling thread and the
/// function namespace. Natives can also run script functions ([`NativeCtx::call_func`]) and start
/// action chains ([`NativeCtx::spawn_chain`]); both need the machine, so they are methods here.
pub struct NativeCtx<'a> {
    m: &'a mut Machine,
    thread: ThreadId,
    key: u32,
    spawns: Vec<Spawn>,
}

/// A chain of script functions requested by a native. It starts after the native returns.
struct Spawn {
    chain: Vec<u32>,
    key: u32,
}

impl NativeCtx<'_> {
    /// The VM extension of type `T` (see [`Vm::insert_extension`]), or `None` if none was inserted.
    /// This is how game-state crates give natives access to their own state (e.g. the handle table)
    /// without the VM depending on them.
    pub fn extension<T: Any>(&mut self) -> Option<&mut T> {
        self.m
            .extensions
            .iter_mut()
            .find_map(|e| (**e).downcast_mut::<T>())
    }

    /// Current game time in seconds.
    pub fn now(&self) -> F32 {
        self.m.now
    }

    /// The soft-float lookup tables, or `None` when the VM was created without them. A native that
    /// needs them returns `NativeResult::Fail(RunStatus::MissingTables)` in that case.
    pub fn tables(&self) -> Option<&Tables> {
        self.m.tables.as_ref()
    }

    /// The thread that is running the native.
    pub fn thread(&self) -> ThreadId {
        self.thread
    }

    /// The context key of the calling thread (see [`Vm::run_chain`]). Threads that a native starts
    /// from this one inherit the key unless they are given another one.
    pub fn thread_key(&self) -> u32 {
        self.key
    }

    /// The game time at which a sleep of `seconds` that starts now ends: `now + seconds` with the
    /// soft-float add. The VM and the hosts use this same expression for wake times.
    pub fn wake_time(&self, seconds: F32) -> F32 {
        wake_time(self.m.now, seconds)
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        self.m.strings.intern(s)
    }

    pub fn string(&self, id: u32) -> &str {
        self.m.strings.get(id)
    }

    /// Script function id by name.
    pub fn function_id(&self, name: &str) -> Option<u32> {
        self.m.module.function_index.get(&ident_key(name)).copied()
    }

    /// Runs script function `func` synchronously in a new thread and returns its value. The new
    /// thread inherits this thread's context key. A function that sleeps or yields returns `Err`
    /// with that stop (its thread is dropped). Budget exhaustion is resumed transparently.
    pub fn call_func(&mut self, func: u32, args: &[Value]) -> Result<Value, RunStatus> {
        self.m.call_thread(func, args, self.key)
    }

    /// Like [`NativeCtx::call_func`], with the new thread's context key given explicitly.
    pub fn call_func_keyed(
        &mut self,
        func: u32,
        args: &[Value],
        key: u32,
    ) -> Result<Value, RunStatus> {
        self.m.call_thread(func, args, key)
    }

    /// Requests a new thread running script function `func`. It starts right after the native
    /// returns, before the calling thread continues (see [`NativeCtx::spawn_chain`]).
    pub fn spawn(&mut self, func: u32) {
        self.spawn_chain(vec![func], self.key);
    }

    /// Requests a thread that runs the functions of `chain` one after another (no arguments, no
    /// results). It starts right after the native returns and runs synchronously until its first
    /// stop (a sleep, a yield or its end). A sleep inside one function suspends the rest of the
    /// chain, which continues when the thread is resumed. `key` is the new thread's context key.
    pub fn spawn_chain(&mut self, chain: Vec<u32>, key: u32) {
        self.spawns.push(Spawn { chain, key });
    }
}

/// A native implementation. Arguments are already checked against the declaration.
///
/// Shared (`Rc`) so that the VM can keep the native alive while it runs a nested script function.
pub type NativeFn = Rc<dyn Fn(&mut NativeCtx<'_>, &[Value]) -> NativeResult>;

/// Natives by name. Bound to the module's declarations when the VM is created.
#[derive(Default)]
pub struct NativeRegistry {
    entries: Vec<(String, NativeFn)>,
    index: BTreeMap<String, usize>,
}

impl NativeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry with the built-in natives used by the tests and by simple scripts: every pure
    /// native (`natives::register_pure`, which includes `I2R`, `R2I`, `I2S`, `S2I`), plus
    /// `TriggerSleepAction` and `ExecuteFunc`.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        crate::natives::register_pure(&mut r);
        r.register("TriggerSleepAction", |_, a| {
            let seconds = a.first().and_then(|v| v.as_real()).unwrap_or(F32::ZERO);
            NativeResult::Sleep(clamp_sleep(seconds))
        });
        // UNVERIFIED vs retail: exact-name lookup; an unknown name aborts the calling thread.
        r.register("ExecuteFunc", |ctx, a| {
            let id = a[0].as_string_id().unwrap_or(0);
            let name = ctx.string(id).to_string();
            match ctx.function_id(&name) {
                Some(f) => {
                    ctx.spawn(f);
                    NativeResult::Return(Value::Null)
                }
                None => NativeResult::Fail(RunStatus::FunctionNotFound),
            }
        });
        r
    }

    /// Adds a native, replacing any earlier one with the same name.
    pub fn register<F>(&mut self, name: &str, f: F)
    where
        F: Fn(&mut NativeCtx<'_>, &[Value]) -> NativeResult + 'static,
    {
        let boxed: NativeFn = Rc::new(f);
        let name = &ident_key(name);
        match self.index.get(name) {
            Some(&i) => self.entries[i].1 = boxed,
            None => {
                self.index.insert(name.to_string(), self.entries.len());
                self.entries.push((name.to_string(), boxed));
            }
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.index.get(&ident_key(name)).copied()
    }
}

// ---------------------------------------------------------------------------------------------
// Threads and frames
// ---------------------------------------------------------------------------------------------

/// Why `Machine::run` stopped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunStatus {
    /// The outermost function returned. Its value is in `Thread::result`.
    Finished,
    /// The instruction budget ran out. The thread is still runnable and resumes exactly where it stopped.
    BudgetExhausted,
    /// A native asked to sleep for this many seconds.
    Sleep(F32),
    /// A native yielded.
    Yield,
    /// A call target does not exist, or a native is not bound.
    FunctionNotFound,
    /// A null value was used where a value is required.
    NullError,
    /// Integer or real division by zero (the thread is aborted; other threads continue).
    DivideByZero,
    /// Array index outside `0..ARRAY_SIZE`.
    /// UNVERIFIED vs retail: the original's behaviour here is not in the spec.
    IndexOutOfRange,
    /// A value had the wrong runtime type for an operation or a native signature. Cannot happen for
    /// code that passed the type checker; reported instead of panicking.
    TypeError,
    /// A real division or a table-based real function ran in a VM created without the `softfloat`
    /// lookup tables (`Vm::new` got `None`). The thread is aborted; nothing falls back to IEEE math.
    MissingTables,
    /// A native tried to run a script function inside too many nested runs (see `MAX_NESTED_RUNS`).
    /// UNVERIFIED vs retail: the original has no such limit that is known.
    NestingLimit,
}

/// Identifies a thread. Ids are assigned in creation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Runnable,
    /// Not scheduled until game time reaches `until` (`now + seconds`, soft-float add).
    Sleeping {
        until: F32,
    },
}

#[derive(Debug)]
struct Frame {
    func: u32,
    pc: usize,
    slots: Vec<Value>,
    /// Scalar locals and parameters, keyed by symbol id (`DeclLocal`, `ParamBind`).
    vars: Vec<(u32, Value)>,
    /// Local arrays, keyed by symbol id (`DeclLocal` with an array type code).
    arrays: Vec<(u32, Vec<Value>)>,
}

/// An interpreter thread: a call stack of frames plus scheduling state.
#[derive(Debug)]
pub struct Thread {
    id: ThreadId,
    frames: Vec<Frame>,
    state: State,
    /// Return value of the outermost function once it has finished.
    result: Option<Value>,
    /// Set when the thread finished or failed; the scheduler drops it.
    dead: bool,
    /// Context key given by the host (0 for none). Natives read it through `NativeCtx::thread_key`.
    key: u32,
    /// Functions still to run after the current one returns (a chain, see `NativeCtx::spawn_chain`).
    chain: VecDeque<u32>,
    /// The value stack: pushed call arguments (`Push`, `ParamBind`, `PopArgs`).
    stack: Vec<Value>,
}

impl Thread {
    pub fn id(&self) -> ThreadId {
        self.id
    }

    /// Depth of the call stack (0 once the thread has finished).
    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    /// The host's context key of this thread (0 for none).
    pub fn key(&self) -> u32 {
        self.key
    }
}

fn make_frame(module: &Module, func: u32) -> Frame {
    Frame {
        func,
        pc: 0,
        slots: vec![Value::Null; module.functions[func as usize].num_slots],
        vars: Vec::new(),
        arrays: Vec::new(),
    }
}

/// A new thread at `func`. The arguments are pushed on its value stack; the function's `ParamBind`
/// instructions read them from there.
fn new_thread(module: &Module, id: ThreadId, func: u32, args: &[Value]) -> Thread {
    Thread {
        id,
        frames: vec![make_frame(module, func)],
        state: State::Runnable,
        result: None,
        dead: false,
        key: 0,
        chain: VecDeque::new(),
        stack: args.to_vec(),
    }
}

// ---------------------------------------------------------------------------------------------
// Machine: everything the interpreter needs except the thread list
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
enum GlobalCell {
    Scalar(Value),
    Array(Vec<Value>),
}

struct Machine {
    module: Module,
    /// Decoded code per function (see `verify`): jump operands are instruction indices, call and
    /// function-reference operands are function indices.
    code: Vec<Vec<Instr>>,
    /// Global index per symbol id (`u32::MAX` when the symbol is not a global).
    global_by_sym: Vec<u32>,
    strings: StringTable,
    /// Host state shared with natives, one value per type (see [`Vm::insert_extension`]).
    extensions: Vec<Box<dyn Any>>,
    /// String value of each string literal symbol: the canonical id of its text (so `""` is the
    /// null string, id 0, and equal texts compare equal).
    sym_string: Vec<u32>,
    globals: Vec<GlobalCell>,
    registry: NativeRegistry,
    /// Per module native: the index of its implementation in `registry`, if bound.
    bindings: Vec<Option<usize>>,
    /// Lookup tables of the soft-float helpers (`None` when not loaded).
    tables: Option<Tables>,
    now: F32,
    budget: u32,
    nest: u32,
    next_thread: u32,
    /// Threads created while running (nested starts that did not finish). The scheduler adopts them.
    spawned: Vec<Thread>,
    failures: Vec<(ThreadId, RunStatus)>,
}
/// Reads a slot. Out-of-range reads as null.
fn rd(th: &Thread, slot: usize) -> Value {
    th.frames
        .last()
        .and_then(|f| f.slots.get(slot).copied())
        .unwrap_or(Value::Null)
}

/// Writes a slot. Writes out of range are discarded.
fn wr(th: &mut Thread, slot: u8, v: Value) {
    if let Some(f) = th.frames.last_mut() {
        if let Some(s) = f.slots.get_mut(slot as usize) {
            *s = v;
        }
    }
}

fn int_of(v: Value) -> Result<i32, RunStatus> {
    match v {
        Value::Integer(i) => Ok(i),
        Value::Null => Err(RunStatus::NullError),
        _ => Err(RunStatus::TypeError),
    }
}

fn real_of(v: Value) -> Result<F32, RunStatus> {
    match v {
        Value::Real(r) => Ok(r),
        Value::Null => Err(RunStatus::NullError),
        _ => Err(RunStatus::TypeError),
    }
}

/// A numeric operand of a comparison as a real: integers go through `I2R`.
fn num_real(v: Value) -> Result<F32, RunStatus> {
    match v {
        Value::Integer(i) => Ok(conv::i2r(i)),
        other => real_of(other),
    }
}

/// The wake time of a sleep that starts at `now`: `now + seconds` with the soft-float add.
pub fn wake_time(now: F32, seconds: F32) -> F32 {
    arith::add(now, seconds)
}

/// Sleep durations as the VM stores them: a negative or NaN duration becomes `0.0`
/// (the `max(0)` clamp of `TriggerSleepAction`; UNVERIFIED vs retail, see the spec's open items).
pub fn clamp_sleep(seconds: F32) -> F32 {
    if seconds.to_bits() & 0x8000_0000 != 0 || seconds.to_f32().is_nan() {
        F32::ZERO
    } else {
        seconds
    }
}

fn bool_of(v: Value) -> Result<bool, RunStatus> {
    match v {
        Value::Boolean(b) => Ok(b),
        Value::Null => Err(RunStatus::NullError),
        _ => Err(RunStatus::TypeError),
    }
}

impl Machine {
    fn alloc_id(&mut self) -> ThreadId {
        let id = ThreadId(self.next_thread);
        self.next_thread += 1;
        id
    }

    /// Runs `th` for at most `budget` instructions. Returns why it stopped. A stop that is not
    /// `Finished`, `Sleep` or `Yield` is an error and the thread must be dropped. Every executed
    /// instruction costs one unit of budget, labels and declarations included.
    fn run(&mut self, th: &mut Thread, budget: u32) -> RunStatus {
        let mut left = budget;
        loop {
            if th.frames.is_empty() {
                return RunStatus::Finished;
            }
            if left == 0 {
                return RunStatus::BudgetExhausted;
            }
            left -= 1;
            let ins = {
                let Some(f) = th.frames.last_mut() else {
                    return RunStatus::Finished;
                };
                let ins = self.code[f.func as usize]
                    .get(f.pc)
                    .copied()
                    .unwrap_or(Instr::new(Op::Return));
                f.pc += 1;
                ins
            };
            if let Err(stop) = self.exec(th, ins) {
                return stop;
            }
        }
    }

    /// Executes one instruction. `Err` ends the run: `Finished`, `Sleep` and `Yield` are normal stops,
    /// everything else is an error.
    fn exec(&mut self, th: &mut Thread, ins: Instr) -> Result<(), RunStatus> {
        let l = ins.left;
        let r = ins.right;
        let d = ins.dest;
        match ins.op {
            Op::End => return Err(RunStatus::Finished),
            Op::FuncHdr
            | Op::Label
            | Op::TypeParent
            | Op::TypeName
            | Op::DeclGlobal
            | Op::DeclConstGlobal => {}
            Op::DebugSkip | Op::Jump => set_pc(th, ins.imm),
            Op::FuncEnd => return self.do_return(th, Value::Null),
            Op::Return => {
                let v = rd(th, 0);
                return self.do_return(th, v);
            }
            Op::DeclLocal => {
                let Some(f) = th.frames.last_mut() else {
                    return Err(RunStatus::TypeError);
                };
                // Array type codes are the element code plus 5 (integer array = 9).
                if d >= 9 {
                    let elem = Value::default_for(ty_of_code(d - 5));
                    f.arrays.push((ins.imm, vec![elem; ARRAY_SIZE]));
                } else {
                    f.vars.push((ins.imm, Value::default_for(ty_of_code(d))));
                }
            }
            Op::ParamBind => {
                let v = th
                    .stack
                    .len()
                    .checked_sub(l as usize)
                    .and_then(|i| th.stack.get(i))
                    .copied()
                    .ok_or(RunStatus::TypeError)?;
                let Some(f) = th.frames.last_mut() else {
                    return Err(RunStatus::TypeError);
                };
                f.vars.push((ins.imm, v));
            }
            Op::PopArgs => {
                let n = d as usize;
                let len = th.stack.len();
                if n > len {
                    return Err(RunStatus::TypeError);
                }
                th.stack.truncate(len - n);
            }
            Op::LoadConst => {
                let v = match l {
                    4 => Value::Integer(ins.imm as i32),
                    5 => Value::Real(F32::from_bits(ins.imm)),
                    6 => Value::String(self.sym_string.get(ins.imm as usize).copied().unwrap_or(0)),
                    8 => Value::Boolean(ins.imm != 0),
                    // 0 (default return), 2 and the handle/code null literals.
                    _ => Value::Null,
                };
                wr(th, d, v);
            }
            Op::Move => {
                let v = rd(th, l as usize);
                wr(th, d, v);
            }
            Op::LoadVar => {
                let v = self.load_var(th, ins.imm)?;
                wr(th, d, v);
            }
            Op::StoreVar => {
                let v = rd(th, d as usize);
                self.store_var(th, ins.imm, v)?;
            }
            Op::LoadFuncRef => wr(th, d, Value::Code(ins.imm)),
            Op::ArrayLoad => {
                let i = array_index(int_of(rd(th, l as usize))?)?;
                let v = self.array_get(th, ins.imm, i)?;
                wr(th, d, v);
            }
            Op::ArrayStore => {
                // The index is in `dest`, the value in `left`.
                let i = array_index(int_of(rd(th, d as usize))?)?;
                let v = rd(th, l as usize);
                self.array_set(th, ins.imm, i, v)?;
            }
            Op::Push => {
                let v = rd(th, d as usize);
                th.stack.push(v);
            }
            Op::Pop => {
                let v = th.stack.pop().ok_or(RunStatus::TypeError)?;
                wr(th, d, v);
            }
            Op::IntToReal => {
                let v = conv::i2r(int_of(rd(th, d as usize))?);
                wr(th, d, Value::Real(v));
            }
            Op::Neg => {
                let v = match rd(th, d as usize) {
                    Value::Integer(i) => Value::Integer(i.wrapping_neg()),
                    Value::Real(x) => Value::Real(arith::neg(x)),
                    other => return Err(operand_error(other)),
                };
                wr(th, d, v);
            }
            Op::Not => {
                let v = !bool_of(rd(th, d as usize))?;
                wr(th, d, Value::Boolean(v));
            }
            Op::Add | Op::Sub | Op::Mul | Op::Div => {
                let (a, b) = (rd(th, l as usize), rd(th, r as usize));
                let v = self.arith(ins.op, a, b)?;
                wr(th, d, v);
            }
            Op::Eq => {
                let v = jass_equal(rd(th, l as usize), rd(th, r as usize));
                wr(th, d, Value::Boolean(v));
            }
            Op::Ne => {
                let v = !jass_equal(rd(th, l as usize), rd(th, r as usize));
                wr(th, d, Value::Boolean(v));
            }
            Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                let (a, b) = (rd(th, l as usize), rd(th, r as usize));
                let v = match (a, b) {
                    (Value::Integer(x), Value::Integer(y)) => match ins.op {
                        Op::Lt => x < y,
                        Op::Le => x <= y,
                        Op::Gt => x > y,
                        _ => x >= y,
                    },
                    // Reals use the ordered soft-float compares: false when either side is NaN.
                    _ => {
                        let x = num_real(a)?;
                        let y = num_real(b)?;
                        match ins.op {
                            Op::Lt => arith::lt(x, y),
                            Op::Le => arith::le(x, y),
                            Op::Gt => arith::gt(x, y),
                            _ => arith::le(y, x),
                        }
                    }
                };
                wr(th, d, Value::Boolean(v));
            }
            Op::JumpIfFalse => {
                if !bool_of(rd(th, d as usize))? {
                    set_pc(th, ins.imm);
                }
            }
            Op::JumpIfTrue => {
                if bool_of(rd(th, d as usize))? {
                    set_pc(th, ins.imm);
                }
            }
            Op::CallScript => {
                // Arguments are already on the value stack; the callee's ParamBind instructions read them.
                th.frames.push(make_frame(&self.module, ins.imm));
            }
            Op::CallNative => {
                let idx = ins.imm as usize;
                let Some(def) = self.module.natives.get(idx) else {
                    return Err(RunStatus::FunctionNotFound);
                };
                let params = def.params.clone();
                let ret = def.ret;
                let base = th
                    .stack
                    .len()
                    .checked_sub(params.len())
                    .ok_or(RunStatus::TypeError)?;
                let args: Vec<Value> = th.stack[base..].to_vec();
                check_signature(&params, &args)?;
                let Some(bound) = self.bindings.get(idx).copied().flatten() else {
                    return Err(RunStatus::FunctionNotFound);
                };
                let native = Rc::clone(&self.registry.entries[bound].1);
                let (result, spawns) = {
                    let mut ctx = NativeCtx {
                        m: self,
                        thread: th.id,
                        key: th.key,
                        spawns: Vec::new(),
                    };
                    let res = native(&mut ctx, &args);
                    (res, ctx.spawns)
                };
                // Synchronous starts requested by the native run before the caller continues.
                for sp in spawns {
                    self.start_chain(sp.chain, sp.key);
                }
                match result {
                    NativeResult::Return(v) => {
                        if let Some(t) = ret {
                            if !v.fits(t) {
                                return Err(RunStatus::TypeError);
                            }
                        }
                        wr(th, 0, v);
                    }
                    NativeResult::Sleep(seconds) => return Err(RunStatus::Sleep(seconds)),
                    NativeResult::Yield => return Err(RunStatus::Yield),
                    NativeResult::Fail(status) => return Err(status),
                }
            }
        }
        Ok(())
    }

    /// Pops the current frame with return value `v`. The caller finds it in its slot 0. The value
    /// is converted to the function's return type when it is null (falling off the end).
    fn do_return(&mut self, th: &mut Thread, v: Value) -> Result<(), RunStatus> {
        let Some(popped) = th.frames.pop() else {
            return Err(RunStatus::Finished);
        };
        let v = match (v, self.module.functions[popped.func as usize].ret) {
            (Value::Null, Some(t)) => Value::default_for(t),
            _ => v,
        };
        if let Some(caller) = th.frames.last_mut() {
            if let Some(s) = caller.slots.get_mut(0) {
                *s = v;
            }
            return Ok(());
        }
        // A chain continues with its next function; the chain's result is discarded.
        if let Some(next) = th.chain.pop_front() {
            th.frames.push(make_frame(&self.module, next));
            return Ok(());
        }
        th.result = Some(v);
        Err(RunStatus::Finished)
    }

    fn load_var(&self, th: &Thread, sym: u32) -> Result<Value, RunStatus> {
        if let Some(f) = th.frames.last() {
            if let Some(&(_, v)) = f.vars.iter().rev().find(|(s, _)| *s == sym) {
                return Ok(v);
            }
        }
        match self.global_of(sym).and_then(|g| self.globals.get(g)) {
            Some(GlobalCell::Scalar(v)) => Ok(*v),
            _ => Err(RunStatus::TypeError),
        }
    }

    fn store_var(&mut self, th: &mut Thread, sym: u32, v: Value) -> Result<(), RunStatus> {
        if let Some(f) = th.frames.last_mut() {
            if let Some(e) = f.vars.iter_mut().rev().find(|(s, _)| *s == sym) {
                e.1 = v;
                return Ok(());
            }
        }
        match self.global_of(sym).and_then(|g| self.globals.get_mut(g)) {
            Some(GlobalCell::Scalar(slot)) => {
                *slot = v;
                Ok(())
            }
            _ => Err(RunStatus::TypeError),
        }
    }

    fn array_get(&self, th: &Thread, sym: u32, i: usize) -> Result<Value, RunStatus> {
        if let Some(f) = th.frames.last() {
            if let Some((_, a)) = f.arrays.iter().rev().find(|(s, _)| *s == sym) {
                return Ok(a[i]);
            }
        }
        match self.global_of(sym).and_then(|g| self.globals.get(g)) {
            Some(GlobalCell::Array(a)) => Ok(a[i]),
            _ => Err(RunStatus::TypeError),
        }
    }

    fn array_set(
        &mut self,
        th: &mut Thread,
        sym: u32,
        i: usize,
        v: Value,
    ) -> Result<(), RunStatus> {
        if let Some(f) = th.frames.last_mut() {
            if let Some((_, a)) = f.arrays.iter_mut().rev().find(|(s, _)| *s == sym) {
                a[i] = v;
                return Ok(());
            }
        }
        match self.global_of(sym).and_then(|g| self.globals.get_mut(g)) {
            Some(GlobalCell::Array(a)) => {
                a[i] = v;
                Ok(())
            }
            _ => Err(RunStatus::TypeError),
        }
    }

    fn global_of(&self, sym: u32) -> Option<usize> {
        match self.global_by_sym.get(sym as usize) {
            Some(&g) if g != u32::MAX => Some(g as usize),
            _ => None,
        }
    }

    /// Arithmetic on values of one type: integers wrap, reals use soft-float, and `+` on strings
    /// concatenates. Division by zero aborts the thread.
    fn arith(&mut self, op: Op, a: Value, b: Value) -> Result<Value, RunStatus> {
        Ok(match (a, b) {
            (Value::Integer(x), Value::Integer(y)) => Value::Integer(match op {
                Op::Add => x.wrapping_add(y),
                Op::Sub => x.wrapping_sub(y),
                Op::Mul => x.wrapping_mul(y),
                _ => {
                    if y == 0 {
                        return Err(RunStatus::DivideByZero);
                    }
                    x.wrapping_div(y)
                }
            }),
            (Value::Real(x), Value::Real(y)) => match op {
                Op::Add => Value::Real(arith::add(x, y)),
                Op::Sub => Value::Real(arith::sub(x, y)),
                Op::Mul => Value::Real(arith::mul(x, y)),
                _ => {
                    // Zero divisor: either sign of zero aborts the thread (spec jass-vm 3).
                    if y.to_bits() & 0x7fff_ffff == 0 {
                        return Err(RunStatus::DivideByZero);
                    }
                    let tables = self.tables.as_ref().ok_or(RunStatus::MissingTables)?;
                    Value::Real(arith::div(x, y, tables))
                }
            },
            (Value::String(_) | Value::Null, Value::String(_) | Value::Null) if op == Op::Add => {
                let sa = a.as_string_id().unwrap_or(0);
                let sb = b.as_string_id().unwrap_or(0);
                let s = format!("{}{}", self.strings.get(sa), self.strings.get(sb));
                Value::String(self.strings.intern(&s))
            }
            _ => return Err(operand_error(if a == Value::Null { a } else { b })),
        })
    }

    /// Runs a chain of functions started by a native to its first stop, right now. A thread that is
    /// not finished is queued in `spawned` for the scheduler. A chain whose first function does not
    /// exist (or takes arguments) fails the thread, as a call to a missing function does.
    fn start_chain(&mut self, chain: Vec<u32>, key: u32) {
        let id = self.alloc_id();
        let Some(first) = chain.first().copied() else {
            return;
        };
        let mut child = new_thread(&self.module, id, first, &[]);
        child.key = key;
        child.chain = chain[1..].iter().copied().collect();
        if self.nest >= MAX_NESTED_RUNS {
            self.spawned.push(child);
            return;
        }
        self.nest += 1;
        let stop = self.run(&mut child, self.budget);
        self.nest -= 1;
        match stop {
            RunStatus::Finished => {}
            RunStatus::Sleep(seconds) => {
                child.state = State::Sleeping {
                    until: wake_time(self.now, seconds),
                };
                self.spawned.push(child);
            }
            RunStatus::Yield | RunStatus::BudgetExhausted => self.spawned.push(child),
            err => self.failures.push((id, err)),
        }
    }

    /// Runs script function `func` to completion in a new thread with context key `key` and returns
    /// its value. Used by natives (see `NativeCtx::call_func`) and by hosts (see `Vm::call_code`).
    fn call_thread(&mut self, func: u32, args: &[Value], key: u32) -> Result<Value, RunStatus> {
        if self.nest >= MAX_NESTED_RUNS {
            return Err(RunStatus::NestingLimit);
        }
        let id = self.alloc_id();
        let mut th = new_thread(&self.module, id, func, args);
        th.key = key;
        self.nest += 1;
        let budget = self.budget;
        let stop = loop {
            match self.run(&mut th, budget) {
                RunStatus::BudgetExhausted => continue,
                other => break other,
            }
        };
        self.nest -= 1;
        match stop {
            RunStatus::Finished => Ok(th.result.unwrap_or(Value::Null)),
            other => Err(other),
        }
    }
}

fn set_pc(th: &mut Thread, target: u32) {
    if let Some(f) = th.frames.last_mut() {
        f.pc = target as usize;
    }
}

/// The value type of a type code (`bytecode::ty`). Unknown codes are handles.
fn ty_of_code(c: u8) -> Ty {
    match c {
        3 => Ty::Code,
        4 => Ty::Integer,
        5 => Ty::Real,
        6 => Ty::String,
        8 => Ty::Boolean,
        _ => Ty::Handle,
    }
}

/// The error for an operand that is null or of the wrong type.
fn operand_error(v: Value) -> RunStatus {
    if v == Value::Null {
        RunStatus::NullError
    } else {
        RunStatus::TypeError
    }
}

fn array_index(i: i32) -> Result<usize, RunStatus> {
    if (0..ARRAY_SIZE as i32).contains(&i) {
        Ok(i as usize)
    } else {
        Err(RunStatus::IndexOutOfRange)
    }
}

fn check_signature(params: &[Ty], args: &[Value]) -> Result<(), RunStatus> {
    if params.len() != args.len() {
        return Err(RunStatus::TypeError);
    }
    for (p, a) in params.iter().zip(args) {
        if !a.fits(*p) {
            return Err(RunStatus::TypeError);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

/// Why `Vm::resume` did not run a thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResumeError {
    /// No live thread has this id (it finished, failed, or never existed).
    NoSuchThread(ThreadId),
    /// The thread is sleeping and `now` is before its wake time `until`.
    NotDue { until: F32 },
}

/// Errors from creating a VM.
#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    /// The module failed verification (an operand refers to something that does not exist).
    Invalid {
        function: String,
        pc: usize,
        reason: String,
    },
    /// The global initializers did not finish (they suspended or failed).
    InitFailed(RunStatus),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::Invalid {
                function,
                pc,
                reason,
            } => write!(f, "invalid bytecode in '{function}' at {pc}: {reason}"),
            VmError::InitFailed(s) => write!(f, "global initialization failed: {s:?}"),
        }
    }
}

impl std::error::Error for VmError {}

/// Checks the module and decodes its code for the interpreter: jump operands become instruction
/// indices, call and function-reference operands become function indices.
fn verify(m: &Module) -> Result<Vec<Vec<Instr>>, VmError> {
    let mut sym_func: BTreeMap<u32, u32> = BTreeMap::new();
    for (i, f) in m.functions.iter().enumerate().skip(1) {
        sym_func.entry(f.sym).or_insert(i as u32);
    }
    let mut out = Vec::with_capacity(m.functions.len());
    for f in &m.functions {
        let bad = |pc: usize, reason: String| VmError::Invalid {
            function: f.name.clone(),
            pc,
            reason,
        };
        let mut labels: BTreeMap<u32, u32> = BTreeMap::new();
        for (pc, ins) in f.code.iter().enumerate() {
            if ins.op == Op::Label {
                labels.entry(ins.imm).or_insert(pc as u32);
            }
        }
        let mut code = f.code.clone();
        for (pc, ins) in code.iter_mut().enumerate() {
            match ins.op {
                Op::Jump | Op::JumpIfTrue | Op::JumpIfFalse | Op::DebugSkip => {
                    let Some(&target) = labels.get(&ins.imm) else {
                        return Err(bad(
                            pc,
                            format!("label {} is not in this function", ins.imm),
                        ));
                    };
                    ins.imm = target;
                }
                Op::CallScript | Op::LoadFuncRef => {
                    let Some(&fi) = sym_func.get(&ins.imm) else {
                        return Err(bad(pc, format!("function symbol {} not found", ins.imm)));
                    };
                    ins.imm = fi;
                }
                Op::CallNative => {
                    if ins.imm as usize >= m.natives.len() {
                        return Err(bad(pc, format!("native {} out of range", ins.imm)));
                    }
                }
                Op::LoadConst if ins.left == 6 && ins.imm as usize >= m.symbols.len() => {
                    return Err(bad(pc, format!("string symbol {} out of range", ins.imm)));
                }
                _ => {}
            }
        }
        out.push(code);
    }
    if m.init as usize >= m.functions.len() {
        return Err(VmError::Invalid {
            function: "<init>".into(),
            pc: 0,
            reason: "init function out of range".into(),
        });
    }
    Ok(out)
}

/// The virtual machine: a module instance, its globals and its thread list.
pub struct Vm {
    m: Machine,
    threads: Vec<Thread>,
}

impl Vm {
    /// Creates the VM: verifies the module, binds natives, initializes globals and runs the global
    /// initializers to completion.
    ///
    /// `tables` are the `softfloat` lookup tables read from the user's `Game.dll`. Pass `None` when
    /// they are not available: real division and the table-based natives then stop their thread with
    /// `RunStatus::MissingTables` (and a global initializer that needs them fails `Vm::new`).
    pub fn new(
        mut module: Module,
        natives: NativeRegistry,
        tables: Option<Tables>,
    ) -> Result<Vm, VmError> {
        // Names are case-insensitive, so the module's name maps are keyed by folded names even when
        // the module was built by hand.
        module.function_index = module
            .function_index
            .into_iter()
            .map(|(k, v)| (ident_key(&k), v))
            .collect();
        module.global_index = module
            .global_index
            .into_iter()
            .map(|(k, v)| (ident_key(&k), v))
            .collect();
        let code = verify(&module)?;
        let mut global_by_sym = vec![u32::MAX; module.symbols.len()];
        for (i, g) in module.globals.iter().enumerate() {
            if let Some(slot) = global_by_sym.get_mut(g.sym as usize) {
                *slot = i as u32;
            }
        }
        let bindings: Vec<Option<usize>> = module
            .natives
            .iter()
            .map(|n| natives.find(&n.name))
            .collect();
        let globals = module
            .globals
            .iter()
            .map(|g| {
                if g.array {
                    GlobalCell::Array(vec![Value::default_for(g.ty); ARRAY_SIZE])
                } else {
                    GlobalCell::Scalar(Value::default_for(g.ty))
                }
            })
            .collect();
        let strings = StringTable::from_symbols(&module.symbols);
        let sym_string: Vec<u32> = module
            .symbols
            .iter()
            .map(|s| strings.exact_id(s).unwrap_or(0))
            .collect();
        let init = module.init;
        let mut vm = Vm {
            m: Machine {
                module,
                code,
                global_by_sym,
                sym_string,
                strings,
                extensions: Vec::new(),
                globals,
                registry: natives,
                bindings,
                tables,
                now: F32::ZERO,
                budget: DEFAULT_BUDGET,
                nest: 0,
                next_thread: 0,
                spawned: Vec::new(),
                failures: Vec::new(),
            },
            threads: Vec::new(),
        };
        vm.call_id(init, &[]).map_err(VmError::InitFailed)?;
        Ok(vm)
    }

    /// Stores host state for natives to reach through [`NativeCtx::extension`]. One value per type:
    /// inserting a second value of the same type replaces the first. The state is not visible to
    /// natives that run inside `Vm::new` (global initializers), because the VM does not exist yet
    /// when it is created; insert it right after `Vm::new`.
    pub fn insert_extension<T: Any>(&mut self, value: T) {
        let slot = self.m.extensions.iter().position(|e| e.is::<T>());
        match slot {
            Some(i) => self.m.extensions[i] = Box::new(value),
            None => self.m.extensions.push(Box::new(value)),
        }
    }

    /// The host state of type `T`, if inserted.
    pub fn extension<T: Any>(&self) -> Option<&T> {
        self.m
            .extensions
            .iter()
            .find_map(|e| (**e).downcast_ref::<T>())
    }

    /// Mutable access to the host state of type `T`, if inserted.
    pub fn extension_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.m
            .extensions
            .iter_mut()
            .find_map(|e| (**e).downcast_mut::<T>())
    }

    /// Instructions per thread per scheduler pass.
    pub fn budget(&self) -> u32 {
        self.m.budget
    }

    pub fn set_budget(&mut self, budget: u32) {
        self.m.budget = budget;
    }

    /// Game time as of the last `step` or `resume`.
    pub fn now(&self) -> F32 {
        self.m.now
    }

    /// Sets the game time that the next [`Vm::call_func`] runs at. Hosts that run script code at a
    /// timer's expiry set it to that expiry first (`run_chain` and `resume` set it themselves).
    pub fn set_now(&mut self, now: F32) {
        self.m.now = now;
    }

    /// Whether the VM has the `softfloat` lookup tables.
    pub fn has_tables(&self) -> bool {
        self.m.tables.is_some()
    }

    pub fn module(&self) -> &Module {
        &self.m.module
    }

    /// Names of the module's declared natives that no registry entry is bound to, in declaration
    /// order. A call to one of them aborts the calling thread with `RunStatus::FunctionNotFound`.
    pub fn unbound_natives(&self) -> Vec<&str> {
        self.m
            .module
            .natives
            .iter()
            .zip(self.m.bindings.iter())
            .filter(|(_, bound)| bound.is_none())
            .map(|(def, _)| def.name.as_str())
            .collect()
    }

    pub fn strings(&self) -> &StringTable {
        &self.m.strings
    }

    /// Number of live threads (queued, sleeping or runnable).
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    /// Threads that ended with an error since the last `take_failures`.
    pub fn failures(&self) -> &[(ThreadId, RunStatus)] {
        &self.m.failures
    }

    pub fn take_failures(&mut self) -> Vec<(ThreadId, RunStatus)> {
        std::mem::take(&mut self.m.failures)
    }

    /// Value of a scalar global.
    pub fn global(&self, name: &str) -> Option<Value> {
        let id = *self.m.module.global_index.get(&ident_key(name))? as usize;
        match self.m.globals.get(id)? {
            GlobalCell::Scalar(v) => Some(*v),
            GlobalCell::Array(_) => None,
        }
    }

    /// Element of an array global.
    pub fn global_element(&self, name: &str, index: usize) -> Option<Value> {
        let id = *self.m.module.global_index.get(&ident_key(name))? as usize;
        match self.m.globals.get(id)? {
            GlobalCell::Array(a) => a.get(index).copied(),
            GlobalCell::Scalar(_) => None,
        }
    }

    /// Queues a new thread running `name` with `args`. It first runs on the next `step`, after the
    /// threads that already exist, so the order of starts is the order of scheduling.
    pub fn start(&mut self, name: &str, args: &[Value]) -> Result<ThreadId, RunStatus> {
        let func = *self
            .m
            .module
            .function_index
            .get(&ident_key(name))
            .ok_or(RunStatus::FunctionNotFound)?;
        self.check_args(func, args)?;
        let id = self.m.alloc_id();
        let t = new_thread(&self.m.module, id, func, args);
        self.threads.push(t);
        Ok(id)
    }

    /// Runs one scheduler pass at game time `now`.
    ///
    /// Threads run in list order (insertion order). A sleeping thread is skipped until
    /// `now >= wake time`. A thread that exhausts its budget or yields stays in place. A thread that
    /// finishes or fails is removed. Threads started during the pass are appended after the pass.
    pub fn step(&mut self, now: F32) {
        self.m.now = now;
        for i in 0..self.threads.len() {
            if self.is_due(i, now) {
                self.run_slot(i);
            }
        }
        self.finish_pass();
    }

    /// Runs exactly one thread, `id`, at game time `now`, and nothing else.
    ///
    /// This is how a host wakes a thread whose sleep timer fired: the thread runs once, at the
    /// timer's expiry, and the other threads keep their state. A sleeping thread can only be resumed
    /// once `now >= wake time`; otherwise nothing runs and `ResumeError::NotDue` is returned. Threads
    /// started by the resumed thread are appended to the list after it, as in `step`.
    pub fn resume(&mut self, id: ThreadId, now: F32) -> Result<(), ResumeError> {
        let i = self
            .threads
            .iter()
            .position(|t| t.id == id)
            .ok_or(ResumeError::NoSuchThread(id))?;
        if let State::Sleeping { until } = self.threads[i].state {
            if arith::lt(now, until) {
                return Err(ResumeError::NotDue { until });
            }
        }
        self.m.now = now;
        self.run_slot(i);
        self.finish_pass();
        Ok(())
    }

    /// Whether thread `i` may run at `now` (runnable, or sleeping and its wake time has come).
    fn is_due(&self, i: usize, now: F32) -> bool {
        match self.threads[i].state {
            State::Runnable => true,
            State::Sleeping { until } => !arith::lt(now, until),
        }
    }

    /// Runs thread `i` until it stops, and records the outcome in its state.
    fn run_slot(&mut self, i: usize) {
        let now = self.m.now;
        let budget = self.m.budget;
        let th = &mut self.threads[i];
        th.state = State::Runnable;
        let stop = self.m.run(th, budget);
        match stop {
            RunStatus::BudgetExhausted | RunStatus::Yield => {}
            RunStatus::Sleep(seconds) => {
                th.state = State::Sleeping {
                    until: wake_time(now, seconds),
                };
            }
            RunStatus::Finished => th.dead = true,
            err => {
                th.dead = true;
                self.m.failures.push((th.id, err));
            }
        }
    }

    /// Drops finished and failed threads and appends the threads started during the pass.
    fn finish_pass(&mut self) {
        self.threads.retain(|t| !t.dead);
        self.threads.append(&mut self.m.spawned);
    }

    /// Runs a chain of script functions (no arguments) as one new thread with context key `key`,
    /// at game time `now`, and queues it for the scheduler if it has not finished. The first
    /// function runs right away, until its first stop, like a native's `spawn_chain`. Hosts use this
    /// for work that starts at a timer's expiry (for example a timer handler).
    pub fn run_chain(&mut self, chain: &[u32], key: u32, now: F32) {
        self.m.now = now;
        self.m.start_chain(chain.to_vec(), key);
        self.finish_pass();
    }

    /// Runs script function `func` with `args` to completion, in a new thread with context key `key`,
    /// at the current game time, and returns its value. A suspension or an error is returned as
    /// `Err`. Hosts use this for condition functions; natives use `NativeCtx::call_func`.
    pub fn call_func(&mut self, func: u32, args: &[Value], key: u32) -> Result<Value, RunStatus> {
        self.check_args(func, args)?;
        self.m.call_thread(func, args, key)
    }

    /// Runs `name` synchronously to completion in a private thread and returns its value
    /// (`Null` for `returns nothing`). Budget exhaustion is resumed transparently. A suspension
    /// (`Sleep`/`Yield`) or an error is returned as `Err`, and the thread is discarded.
    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, RunStatus> {
        let func = *self
            .m
            .module
            .function_index
            .get(&ident_key(name))
            .ok_or(RunStatus::FunctionNotFound)?;
        self.check_args(func, args)?;
        self.call_id(func, args)
    }

    fn check_args(&self, func: u32, args: &[Value]) -> Result<(), RunStatus> {
        let f = &self.m.module.functions[func as usize];
        check_signature(&f.params, args)
    }

    fn call_id(&mut self, func: u32, args: &[Value]) -> Result<Value, RunStatus> {
        let id = self.m.alloc_id();
        let mut th = new_thread(&self.m.module, id, func, args);
        let budget = self.m.budget;
        loop {
            match self.m.run(&mut th, budget) {
                RunStatus::BudgetExhausted => continue,
                RunStatus::Finished => return Ok(th.result.unwrap_or(Value::Null)),
                stop => return Err(stop),
            }
        }
    }
}
