//! JASS virtual machine: values, threads, natives and a deterministic scheduler.
//!
//! Behaviour follows docs/spec/jass-vm.md where the spec is CONFIRMED or LIKELY. Everything else
//! is marked `UNVERIFIED vs retail` in the code and listed in the crate report.
//!
//! Determinism: no wall clock, no randomness, no hash-map iteration in observable behaviour.
//! Game time is supplied by the caller through `Vm::step(now)`.

use std::collections::BTreeMap;

use crate::bytecode::{Instr, Op, NO_SLOT};
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
    /// IEEE binary32.
    Real(f32),
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
            Ty::Real => Value::Real(0.0),
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

    pub fn as_real(self) -> Option<f32> {
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

/// JASS `==`. Null equals any null-like value. Integer against real compares as real.
pub fn jass_equal(a: Value, b: Value) -> bool {
    if a.is_null_like() || b.is_null_like() {
        return a.is_null_like() && b.is_null_like();
    }
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x == y,
        (Value::Real(x), Value::Real(y)) => x == y,
        (Value::Integer(x), Value::Real(y)) | (Value::Real(y), Value::Integer(x)) => x as f32 == y,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Handle(x), Value::Handle(y)) => x == y,
        (Value::Code(x), Value::Code(y)) => x == y,
        _ => false,
    }
}

/// Real arithmetic. Every real operation goes through this module so the rounding model can be
/// replaced in one place.
///
/// TODO (spec 2.5): the original does real arithmetic in software on the f32 bit pattern, and the
/// spec says its multiply truncates the mantissa product instead of rounding to nearest. That is
/// not implemented: these are plain IEEE f32 operations.
pub mod real {
    pub fn add(a: f32, b: f32) -> f32 {
        a + b
    }
    pub fn sub(a: f32, b: f32) -> f32 {
        a - b
    }
    pub fn mul(a: f32, b: f32) -> f32 {
        a * b
    }
    /// The caller checks for a zero divisor first.
    pub fn div(a: f32, b: f32) -> f32 {
        a / b
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

    /// Builds a table from a module's string list (index i keeps id i).
    pub fn from_list(list: &[String]) -> Self {
        let mut t = StringTable::default();
        for s in list {
            t.intern(s);
        }
        if t.list.is_empty() {
            t.list.push(String::new());
            t.index.insert(String::new(), 0);
        }
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
    Sleep(f32),
    /// Give up the rest of this pass. The call completes with no value.
    Yield,
    /// Abort the calling thread with this status.
    Fail(RunStatus),
}

/// Context handed to natives: string table, game time and the function namespace.
pub struct NativeCtx<'a> {
    strings: &'a mut StringTable,
    functions: &'a BTreeMap<String, u32>,
    now: f32,
    spawns: Vec<u32>,
}

impl NativeCtx<'_> {
    /// Current game time in seconds.
    pub fn now(&self) -> f32 {
        self.now
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        self.strings.intern(s)
    }

    pub fn string(&self, id: u32) -> &str {
        self.strings.get(id)
    }

    /// Script function id by name.
    pub fn function_id(&self, name: &str) -> Option<u32> {
        self.functions.get(name).copied()
    }

    /// Requests a new thread running script function `func`. It starts right after the native
    /// returns, before the calling thread continues (synchronous start, see `Machine::start_nested`).
    pub fn spawn(&mut self, func: u32) {
        self.spawns.push(func);
    }
}

/// A native implementation. Arguments are already checked against the declaration.
pub type NativeFn = Box<dyn Fn(&mut NativeCtx<'_>, &[Value]) -> NativeResult>;

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

    /// A registry with the built-in natives used by the tests and by simple scripts:
    /// `I2R`, `R2I`, `I2S`, `S2I`, `TriggerSleepAction`, `ExecuteFunc`.
    pub fn with_builtins() -> Self {
        let mut r = Self::new();
        r.register("I2R", |_, a| {
            NativeResult::Return(Value::Real(a[0].as_integer().unwrap_or(0) as f32))
        });
        // UNVERIFIED vs retail: truncation toward zero (Rust `as`: NaN gives 0, out-of-range saturates).
        r.register("R2I", |_, a| {
            NativeResult::Return(Value::Integer(a[0].as_real().unwrap_or(0.0) as i32))
        });
        r.register("I2S", |ctx, a| {
            let s = a[0].as_integer().unwrap_or(0).to_string();
            NativeResult::Return(Value::String(ctx.intern(&s)))
        });
        // UNVERIFIED vs retail: plain decimal parse, 0 when the text is not a number.
        r.register("S2I", |ctx, a| {
            let id = a[0].as_string_id().unwrap_or(0);
            let v = ctx.string(id).parse::<i32>().unwrap_or(0);
            NativeResult::Return(Value::Integer(v))
        });
        r.register("TriggerSleepAction", |_, a| {
            NativeResult::Sleep(a[0].as_real().unwrap_or(0.0).max(0.0))
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
        let boxed: NativeFn = Box::new(f);
        match self.index.get(name) {
            Some(&i) => self.entries[i].1 = boxed,
            None => {
                self.index.insert(name.to_string(), self.entries.len());
                self.entries.push((name.to_string(), boxed));
            }
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
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
    Sleep(f32),
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
}

/// Identifies a thread. Ids are assigned in creation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Runnable,
    /// Not scheduled until game time reaches `until`.
    Sleeping {
        until: f32,
    },
}

#[derive(Debug)]
struct Frame {
    func: u32,
    pc: usize,
    slots: Vec<Value>,
    arrays: Vec<Vec<Value>>,
    /// Slot of the caller that receives the return value (`NO_SLOT` to discard).
    ret_dest: u8,
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
}

impl Thread {
    pub fn id(&self) -> ThreadId {
        self.id
    }

    /// Depth of the call stack (0 once the thread has finished).
    pub fn depth(&self) -> usize {
        self.frames.len()
    }
}

fn make_frame(module: &Module, func: u32, args: &[Value], ret_dest: u8) -> Frame {
    let f = &module.functions[func as usize];
    let mut slots = vec![Value::Null; f.num_slots];
    for &(slot, ty) in &f.locals {
        if let Some(s) = slots.get_mut(slot as usize) {
            *s = Value::default_for(ty);
        }
    }
    for (slot, &a) in slots.iter_mut().zip(args) {
        *slot = a;
    }
    let arrays = f
        .local_arrays
        .iter()
        .map(|&ty| vec![Value::default_for(ty); ARRAY_SIZE])
        .collect();
    Frame {
        func,
        pc: 0,
        slots,
        arrays,
        ret_dest,
    }
}

fn new_thread(module: &Module, id: ThreadId, func: u32, args: &[Value]) -> Thread {
    Thread {
        id,
        frames: vec![make_frame(module, func, args, NO_SLOT)],
        state: State::Runnable,
        result: None,
        dead: false,
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
    strings: StringTable,
    globals: Vec<GlobalCell>,
    registry: NativeRegistry,
    /// Per module native: the index of its implementation in `registry`, if bound.
    bindings: Vec<Option<usize>>,
    now: f32,
    budget: u32,
    nest: u32,
    next_thread: u32,
    /// Threads created while running (nested starts that did not finish). The scheduler adopts them.
    spawned: Vec<Thread>,
    failures: Vec<(ThreadId, RunStatus)>,
}

/// Reads a slot. Out-of-range (including `NO_SLOT`) reads as null.
fn rd(th: &Thread, slot: usize) -> Value {
    th.frames
        .last()
        .and_then(|f| f.slots.get(slot).copied())
        .unwrap_or(Value::Null)
}

/// Writes a slot. Writes to `NO_SLOT` or out of range are discarded.
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

fn real_of(v: Value) -> Result<f32, RunStatus> {
    match v {
        Value::Real(r) => Ok(r),
        Value::Null => Err(RunStatus::NullError),
        _ => Err(RunStatus::TypeError),
    }
}

fn bool_of(v: Value) -> Result<bool, RunStatus> {
    match v {
        Value::Boolean(b) => Ok(b),
        Value::Null => Err(RunStatus::NullError),
        _ => Err(RunStatus::TypeError),
    }
}

fn str_of(v: Value) -> Result<u32, RunStatus> {
    v.as_string_id().ok_or(RunStatus::TypeError)
}

/// Numeric ordering for `<`, `<=`, `>`, `>=`. `None` for NaN.
fn num_cmp(a: Value, b: Value) -> Result<Option<std::cmp::Ordering>, RunStatus> {
    Ok(match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => Some(x.cmp(&y)),
        (Value::Integer(x), Value::Real(y)) => (x as f32).partial_cmp(&y),
        (Value::Real(x), Value::Integer(y)) => x.partial_cmp(&(y as f32)),
        (Value::Real(x), Value::Real(y)) => x.partial_cmp(&y),
        (Value::Null, _) | (_, Value::Null) => return Err(RunStatus::NullError),
        _ => return Err(RunStatus::TypeError),
    })
}

impl Machine {
    fn alloc_id(&mut self) -> ThreadId {
        let id = ThreadId(self.next_thread);
        self.next_thread += 1;
        id
    }

    /// Runs `th` for at most `budget` instructions. Returns why it stopped. A stop that is not
    /// `Finished`, `Sleep` or `Yield` is an error and the thread must be dropped.
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
                let code = &self.module.functions[f.func as usize].code;
                let ins = code.get(f.pc).copied().unwrap_or(Instr::new(Op::Return));
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
        let l = ins.left as usize;
        let r = ins.right as usize;
        let d = ins.dest;
        match ins.op {
            Op::LoadInt => wr(th, d, Value::Integer(ins.imm as i32)),
            Op::LoadReal => wr(th, d, Value::Real(f32::from_bits(ins.imm))),
            Op::LoadBool => wr(th, d, Value::Boolean(ins.imm != 0)),
            Op::LoadString => wr(th, d, Value::String(ins.imm)),
            Op::LoadNull => wr(th, d, Value::Null),
            Op::LoadCode => wr(th, d, Value::Code(ins.imm)),
            Op::Move => {
                let v = rd(th, l);
                wr(th, d, v);
            }
            Op::GlobalGet => match self.globals.get(ins.imm as usize) {
                Some(GlobalCell::Scalar(v)) => {
                    let v = *v;
                    wr(th, d, v);
                }
                _ => return Err(RunStatus::TypeError),
            },
            Op::GlobalSet => {
                let v = rd(th, r);
                match self.globals.get_mut(ins.imm as usize) {
                    Some(GlobalCell::Scalar(slot)) => *slot = v,
                    _ => return Err(RunStatus::TypeError),
                }
            }
            Op::GlobalArrayGet => {
                let i = array_index(int_of(rd(th, l))?)?;
                let v = match self.globals.get(ins.imm as usize) {
                    Some(GlobalCell::Array(a)) => a[i],
                    _ => return Err(RunStatus::TypeError),
                };
                wr(th, d, v);
            }
            Op::GlobalArraySet => {
                let i = array_index(int_of(rd(th, l))?)?;
                let v = rd(th, r);
                match self.globals.get_mut(ins.imm as usize) {
                    Some(GlobalCell::Array(a)) => a[i] = v,
                    _ => return Err(RunStatus::TypeError),
                }
            }
            Op::LocalArrayGet => {
                let i = array_index(int_of(rd(th, l))?)?;
                let v = local_array(th, ins.imm)?[i];
                wr(th, d, v);
            }
            Op::LocalArraySet => {
                let i = array_index(int_of(rd(th, l))?)?;
                let v = rd(th, r);
                local_array_mut(th, ins.imm)?[i] = v;
            }

            Op::AddI => {
                let v = int_of(rd(th, l))?.wrapping_add(int_of(rd(th, r))?);
                wr(th, d, Value::Integer(v));
            }
            Op::SubI => {
                let v = int_of(rd(th, l))?.wrapping_sub(int_of(rd(th, r))?);
                wr(th, d, Value::Integer(v));
            }
            Op::MulI => {
                let v = int_of(rd(th, l))?.wrapping_mul(int_of(rd(th, r))?);
                wr(th, d, Value::Integer(v));
            }
            Op::DivI => {
                let a = int_of(rd(th, l))?;
                let b = int_of(rd(th, r))?;
                if b == 0 {
                    return Err(RunStatus::DivideByZero);
                }
                wr(th, d, Value::Integer(a.wrapping_div(b)));
            }
            Op::ModI => {
                let a = int_of(rd(th, l))?;
                let b = int_of(rd(th, r))?;
                // Zero divisor gives 0 and no error (spec section 3).
                let v = if b == 0 { 0 } else { a.wrapping_rem(b) };
                wr(th, d, Value::Integer(v));
            }

            Op::AddR => {
                let v = real::add(real_of(rd(th, l))?, real_of(rd(th, r))?);
                wr(th, d, Value::Real(v));
            }
            Op::SubR => {
                let v = real::sub(real_of(rd(th, l))?, real_of(rd(th, r))?);
                wr(th, d, Value::Real(v));
            }
            Op::MulR => {
                let v = real::mul(real_of(rd(th, l))?, real_of(rd(th, r))?);
                wr(th, d, Value::Real(v));
            }
            Op::DivR => {
                let a = real_of(rd(th, l))?;
                let b = real_of(rd(th, r))?;
                if b == 0.0 {
                    return Err(RunStatus::DivideByZero);
                }
                wr(th, d, Value::Real(real::div(a, b)));
            }

            Op::Concat => {
                let a = str_of(rd(th, l))?;
                let b = str_of(rd(th, r))?;
                let s = format!("{}{}", self.strings.get(a), self.strings.get(b));
                let id = self.strings.intern(&s);
                wr(th, d, Value::String(id));
            }

            Op::NegI => {
                let v = int_of(rd(th, l))?.wrapping_neg();
                wr(th, d, Value::Integer(v));
            }
            Op::NegR => {
                let v = -real_of(rd(th, l))?;
                wr(th, d, Value::Real(v));
            }
            Op::Not => {
                let v = !bool_of(rd(th, l))?;
                wr(th, d, Value::Boolean(v));
            }
            Op::IntToReal => {
                let v = int_of(rd(th, l))? as f32;
                wr(th, d, Value::Real(v));
            }
            Op::And => {
                let v = bool_of(rd(th, l))? && bool_of(rd(th, r))?;
                wr(th, d, Value::Boolean(v));
            }
            Op::Or => {
                let v = bool_of(rd(th, l))? || bool_of(rd(th, r))?;
                wr(th, d, Value::Boolean(v));
            }

            Op::Eq => {
                let v = jass_equal(rd(th, l), rd(th, r));
                wr(th, d, Value::Boolean(v));
            }
            Op::Ne => {
                let v = !jass_equal(rd(th, l), rd(th, r));
                wr(th, d, Value::Boolean(v));
            }
            Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                use std::cmp::Ordering::*;
                let ord = num_cmp(rd(th, l), rd(th, r))?;
                let v = match (ins.op, ord) {
                    (_, None) => false,
                    (Op::Lt, Some(o)) => o == Less,
                    (Op::Le, Some(o)) => o != Greater,
                    (Op::Gt, Some(o)) => o == Greater,
                    (_, Some(o)) => o != Less,
                };
                wr(th, d, Value::Boolean(v));
            }

            Op::Jump => set_pc(th, ins.imm),
            Op::JumpIfFalse => {
                if !bool_of(rd(th, l))? {
                    set_pc(th, ins.imm);
                }
            }
            Op::JumpIfTrue => {
                if bool_of(rd(th, l))? {
                    set_pc(th, ins.imm);
                }
            }

            Op::CallScript => {
                let func = ins.imm;
                let n = self
                    .module
                    .functions
                    .get(func as usize)
                    .map(|f| f.params.len());
                let argc = r;
                match n {
                    Some(n) if n == argc => {}
                    _ => return Err(RunStatus::FunctionNotFound),
                }
                let args: Vec<Value> = (0..argc).map(|i| rd(th, l + i)).collect();
                let frame = make_frame(&self.module, func, &args, d);
                th.frames.push(frame);
            }
            Op::CallNative => {
                let idx = ins.imm as usize;
                let Some(def) = self.module.natives.get(idx) else {
                    return Err(RunStatus::FunctionNotFound);
                };
                let args: Vec<Value> = (0..r).map(|i| rd(th, l + i)).collect();
                check_signature(&def.params, &args)?;
                let ret = def.ret;
                let Some(bound) = self.bindings.get(idx).copied().flatten() else {
                    return Err(RunStatus::FunctionNotFound);
                };
                let (result, spawns) = {
                    let mut ctx = NativeCtx {
                        strings: &mut self.strings,
                        functions: &self.module.function_index,
                        now: self.now,
                        spawns: Vec::new(),
                    };
                    let f = &self.registry.entries[bound].1;
                    let res = f(&mut ctx, &args);
                    (res, ctx.spawns)
                };
                // Synchronous starts requested by the native run before the caller continues.
                for func in spawns {
                    self.start_nested(func);
                }
                match result {
                    NativeResult::Return(v) => {
                        if let Some(t) = ret {
                            if !v.fits(t) {
                                return Err(RunStatus::TypeError);
                            }
                        }
                        wr(th, d, v);
                    }
                    NativeResult::Sleep(seconds) => return Err(RunStatus::Sleep(seconds)),
                    NativeResult::Yield => return Err(RunStatus::Yield),
                    NativeResult::Fail(status) => return Err(status),
                }
            }
            Op::Return => {
                let v = rd(th, r);
                let Some(popped) = th.frames.pop() else {
                    return Err(RunStatus::Finished);
                };
                match th.frames.last() {
                    None => {
                        th.result = Some(v);
                        return Err(RunStatus::Finished);
                    }
                    // The caller's slot for the result is recorded in the popped frame.
                    Some(_) => wr(th, popped.ret_dest, v),
                }
            }
        }
        Ok(())
    }

    /// Runs a thread started by a native (ExecuteFunc) to its first stop, right now. A thread that
    /// is not finished is queued in `spawned` for the scheduler.
    fn start_nested(&mut self, func: u32) {
        let id = self.alloc_id();
        if self.nest >= MAX_NESTED_RUNS {
            let t = new_thread(&self.module, id, func, &[]);
            self.spawned.push(t);
            return;
        }
        let mut child = new_thread(&self.module, id, func, &[]);
        self.nest += 1;
        let stop = self.run(&mut child, self.budget);
        self.nest -= 1;
        match stop {
            RunStatus::Finished => {}
            RunStatus::Sleep(seconds) => {
                child.state = State::Sleeping {
                    until: self.now + seconds,
                };
                self.spawned.push(child);
            }
            RunStatus::Yield | RunStatus::BudgetExhausted => self.spawned.push(child),
            err => self.failures.push((id, err)),
        }
    }
}

fn set_pc(th: &mut Thread, target: u32) {
    if let Some(f) = th.frames.last_mut() {
        f.pc = target as usize;
    }
}

fn array_index(i: i32) -> Result<usize, RunStatus> {
    if (0..ARRAY_SIZE as i32).contains(&i) {
        Ok(i as usize)
    } else {
        Err(RunStatus::IndexOutOfRange)
    }
}

fn local_array(th: &Thread, idx: u32) -> Result<&Vec<Value>, RunStatus> {
    th.frames
        .last()
        .and_then(|f| f.arrays.get(idx as usize))
        .ok_or(RunStatus::TypeError)
}

fn local_array_mut(th: &mut Thread, idx: u32) -> Result<&mut Vec<Value>, RunStatus> {
    th.frames
        .last_mut()
        .and_then(|f| f.arrays.get_mut(idx as usize))
        .ok_or(RunStatus::TypeError)
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

/// Checks every immediate and every slot range a `run` would index unchecked.
fn verify(m: &Module) -> Result<(), VmError> {
    for f in &m.functions {
        let bad = |pc: usize, reason: String| VmError::Invalid {
            function: f.name.clone(),
            pc,
            reason,
        };
        if f.num_slots > NO_SLOT as usize {
            return Err(bad(0, "more than 255 slots".into()));
        }
        for (pc, ins) in f.code.iter().enumerate() {
            let imm = ins.imm as usize;
            let fits_jump = imm < f.code.len();
            let r = ins.right as usize;
            let l = ins.left as usize;
            match ins.op {
                Op::Jump | Op::JumpIfFalse | Op::JumpIfTrue => {
                    if !fits_jump {
                        return Err(bad(pc, format!("jump target {imm} out of range")));
                    }
                }
                Op::LoadString => {
                    if imm >= m.strings.len() {
                        return Err(bad(pc, format!("string id {imm} out of range")));
                    }
                }
                Op::LoadCode => {
                    if imm >= m.functions.len() {
                        return Err(bad(pc, format!("function id {imm} out of range")));
                    }
                }
                Op::GlobalGet | Op::GlobalSet => match m.globals.get(imm) {
                    Some(g) if !g.array => {}
                    _ => return Err(bad(pc, format!("bad scalar global {imm}"))),
                },
                Op::GlobalArrayGet | Op::GlobalArraySet => match m.globals.get(imm) {
                    Some(g) if g.array => {}
                    _ => return Err(bad(pc, format!("bad array global {imm}"))),
                },
                Op::LocalArrayGet | Op::LocalArraySet => {
                    if imm >= f.local_arrays.len() {
                        return Err(bad(pc, format!("bad local array {imm}")));
                    }
                }
                Op::CallScript => {
                    if imm >= m.functions.len() {
                        return Err(bad(pc, format!("call target {imm} out of range")));
                    }
                    if l + r > f.num_slots {
                        return Err(bad(pc, "argument slots out of range".into()));
                    }
                }
                Op::CallNative => {
                    if imm >= m.natives.len() {
                        return Err(bad(pc, format!("native {imm} out of range")));
                    }
                    if l + r > f.num_slots {
                        return Err(bad(pc, "argument slots out of range".into()));
                    }
                }
                _ => {}
            }
            for s in [ins.dest, ins.left, ins.right] {
                if s != NO_SLOT && s as usize >= f.num_slots {
                    return Err(bad(pc, format!("slot {s} out of range")));
                }
            }
        }
    }
    if m.init as usize >= m.functions.len() {
        return Err(VmError::Invalid {
            function: "<init>".into(),
            pc: 0,
            reason: "init function out of range".into(),
        });
    }
    Ok(())
}

/// The virtual machine: a module instance, its globals and its thread list.
pub struct Vm {
    m: Machine,
    threads: Vec<Thread>,
}

impl Vm {
    /// Creates the VM: verifies the module, binds natives, initializes globals and runs the global
    /// initializers to completion.
    pub fn new(module: Module, natives: NativeRegistry) -> Result<Vm, VmError> {
        verify(&module)?;
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
        let strings = StringTable::from_list(&module.strings);
        let init = module.init;
        let mut vm = Vm {
            m: Machine {
                module,
                strings,
                globals,
                registry: natives,
                bindings,
                now: 0.0,
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

    /// Instructions per thread per scheduler pass.
    pub fn budget(&self) -> u32 {
        self.m.budget
    }

    pub fn set_budget(&mut self, budget: u32) {
        self.m.budget = budget;
    }

    /// Game time as of the last `step`.
    pub fn now(&self) -> f32 {
        self.m.now
    }

    pub fn module(&self) -> &Module {
        &self.m.module
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
        let id = *self.m.module.global_index.get(name)? as usize;
        match self.m.globals.get(id)? {
            GlobalCell::Scalar(v) => Some(*v),
            GlobalCell::Array(_) => None,
        }
    }

    /// Element of an array global.
    pub fn global_element(&self, name: &str, index: usize) -> Option<Value> {
        let id = *self.m.module.global_index.get(name)? as usize;
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
            .get(name)
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
    pub fn step(&mut self, now: f32) {
        self.m.now = now;
        let budget = self.m.budget;
        for i in 0..self.threads.len() {
            let th = &mut self.threads[i];
            if let State::Sleeping { until } = th.state {
                if now < until {
                    continue;
                }
            }
            th.state = State::Runnable;
            let stop = self.m.run(th, budget);
            match stop {
                RunStatus::BudgetExhausted | RunStatus::Yield => {}
                RunStatus::Sleep(seconds) => {
                    th.state = State::Sleeping {
                        until: now + seconds,
                    };
                }
                RunStatus::Finished => th.dead = true,
                err => {
                    th.dead = true;
                    self.m.failures.push((th.id, err));
                }
            }
        }
        self.threads.retain(|t| !t.dead);
        self.threads.append(&mut self.m.spawned);
    }

    /// Runs `name` synchronously to completion in a private thread and returns its value
    /// (`Null` for `returns nothing`). Budget exhaustion is resumed transparently. A suspension
    /// (`Sleep`/`Yield`) or an error is returned as `Err`, and the thread is discarded.
    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, RunStatus> {
        let func = *self
            .m
            .module
            .function_index
            .get(name)
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
