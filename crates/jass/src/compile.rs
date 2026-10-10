//! Compiles checked JASS programs to bytecode (`bytecode::Instr`).
//!
//! The input is the same slice of programs that `check::check` accepts (common.j, blizzard.j,
//! the map script, ...). The programs are type checked first; any diagnostic aborts compilation.
//!
//! Code shape:
//! - Each script function becomes a `FuncDef`. Parameters occupy slots `0..n`, scalar locals get
//!   their own slots, and temporaries are allocated above them with stack discipline (reset after
//!   each statement).
//! - Array variables (globals and locals) are not slots; they are indexed by the `imm` of the
//!   array opcodes.
//! - Implicit integer-to-real widening is made explicit with `IntToReal`, so the VM never sees a
//!   mixed integer/real operation.
//! - Global initializers are compiled into one synthetic function, `Module::init`, that the VM
//!   runs once when it is created.
//!
//! UNVERIFIED vs retail: evaluation order is left to right, and for `set a[i] = v` the index is
//! evaluated before the value. `debug` statements are not compiled (the AST documents them as
//! skipped in release builds).

use std::collections::BTreeMap;

use crate::ast::{self, ident_key, BinOp, Expr, Program, Stmt};
use crate::bytecode::{Instr, Op, NO_SLOT};
use crate::check::{self, Diagnostic};

/// Number of elements of every array variable (globals and locals).
pub const ARRAY_SIZE: usize = 8192;

/// Runtime type of a value. `Null` appears only as the type of the `null` literal and is never a
/// declared type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Integer,
    Real,
    Boolean,
    String,
    /// `handle` and every user type that extends it.
    Handle,
    Code,
    Null,
}

impl Ty {
    fn from_name(name: &str) -> Ty {
        match ident_key(name).as_str() {
            "integer" => Ty::Integer,
            "real" => Ty::Real,
            "boolean" => Ty::Boolean,
            "string" => Ty::String,
            "code" => Ty::Code,
            _ => Ty::Handle,
        }
    }
}

/// A compiled script function.
#[derive(Debug, Clone, PartialEq)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<Ty>,
    /// `None` for `returns nothing`.
    pub ret: Option<Ty>,
    /// Number of frame slots (parameters, locals and temporaries).
    pub num_slots: usize,
    /// Scalar locals as (slot, type). Each frame starts them at the type's default value.
    pub locals: Vec<(u8, Ty)>,
    /// Element type of each array local, indexed by the array opcodes' `imm`.
    pub local_arrays: Vec<Ty>,
    pub code: Vec<Instr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDef {
    pub name: String,
    pub ty: Ty,
    pub array: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDef {
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Option<Ty>,
}

/// A compiled program: functions, globals, natives and the constant string table.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub functions: Vec<FuncDef>,
    pub function_index: BTreeMap<String, u32>,
    pub globals: Vec<GlobalDef>,
    pub global_index: BTreeMap<String, u32>,
    pub natives: Vec<NativeDef>,
    pub native_index: BTreeMap<String, u32>,
    /// Index 0 is always the empty (null) string.
    pub strings: Vec<String>,
    /// Function that initializes the globals; the VM runs it once.
    pub init: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompileError {
    /// The programs did not type check.
    Check(Vec<Diagnostic>),
    /// Something the compiler cannot express, e.g. a native used as a code value, or more than
    /// 255 slots in one function.
    Unsupported { line: u32, message: String },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Check(d) => {
                write!(f, "{} type error(s), first: {}", d.len(), d[0].message)
            }
            CompileError::Unsupported { line, message } => {
                write!(f, "line {line}: unsupported: {message}")
            }
        }
    }
}

impl std::error::Error for CompileError {}

type CResult<T> = Result<T, CompileError>;

/// Compiles `programs` (in load order) into a `Module`.
pub fn compile(programs: &[Program]) -> Result<Module, CompileError> {
    let diags = check::check(programs);
    if !diags.is_empty() {
        return Err(CompileError::Check(diags));
    }

    let mut syms = Symbols::default();
    let mut module = Module {
        functions: Vec::new(),
        function_index: BTreeMap::new(),
        globals: Vec::new(),
        global_index: BTreeMap::new(),
        natives: Vec::new(),
        native_index: BTreeMap::new(),
        strings: vec![String::new()],
        init: 0,
    };
    syms.intern(""); // id 0 is the null string

    // Declarations first, so every call and global reference resolves regardless of order.
    let mut script_funcs: Vec<&ast::Function> = Vec::new();
    let mut global_inits: Vec<(u32, &ast::Global)> = Vec::new();
    for p in programs {
        for g in &p.globals {
            let id = module.globals.len() as u32;
            let ty = Ty::from_name(&g.ty);
            module.globals.push(GlobalDef {
                name: g.name.clone(),
                ty,
                array: g.array,
            });
            module.global_index.insert(ident_key(&g.name), id);
            syms.globals.insert(
                ident_key(&g.name),
                GlobalInfo {
                    id,
                    ty,
                    array: g.array,
                },
            );
            if g.init.is_some() {
                global_inits.push((id, g));
            }
        }
        for n in &p.natives {
            let id = module.natives.len() as u32;
            let params: Vec<Ty> = n.sig.params.iter().map(|p| Ty::from_name(&p.ty)).collect();
            let ret = n.sig.returns.as_deref().map(Ty::from_name);
            module.natives.push(NativeDef {
                name: n.sig.name.clone(),
                params: params.clone(),
                ret,
            });
            module.native_index.insert(ident_key(&n.sig.name), id);
            syms.funcs.insert(
                ident_key(&n.sig.name),
                Callee {
                    script: false,
                    id,
                    params,
                    ret,
                },
            );
        }
        for f in &p.functions {
            let id = script_funcs.len() as u32;
            let params: Vec<Ty> = f.sig.params.iter().map(|p| Ty::from_name(&p.ty)).collect();
            let ret = f.sig.returns.as_deref().map(Ty::from_name);
            syms.funcs.insert(
                ident_key(&f.sig.name),
                Callee {
                    script: true,
                    id,
                    params,
                    ret,
                },
            );
            script_funcs.push(f);
        }
    }

    for f in &script_funcs {
        let def = compile_function(&mut syms, f)?;
        module
            .function_index
            .insert(ident_key(&f.sig.name), module.functions.len() as u32);
        module.functions.push(def);
    }

    // Global initializer function.
    let mut init = FnCompiler::new(&mut syms, None, 0);
    for (gid, g) in global_inits {
        init.line = g.line;
        let init_expr = g.init.as_ref().expect("filtered above");
        let mark = init.next;
        let (s, t) = init.expr(init_expr)?;
        let ty = module.globals[gid as usize].ty;
        let s = init.coerce(s, t, ty)?;
        init.emit(Instr::new(Op::GlobalSet).imm(gid).right(s));
        init.next = mark;
    }
    init.emit(Instr::new(Op::Return));
    let init_def = init.finish("<globals>".to_string(), Vec::new());
    module.init = module.functions.len() as u32;
    module.functions.push(init_def);

    module.strings = syms.strings;
    Ok(module)
}

fn compile_function(syms: &mut Symbols, f: &ast::Function) -> CResult<FuncDef> {
    let params: Vec<Ty> = f.sig.params.iter().map(|p| Ty::from_name(&p.ty)).collect();
    let ret = f.sig.returns.as_deref().map(Ty::from_name);
    let mut fc = FnCompiler::new(syms, ret, params.len());
    fc.line = f.line;
    for (i, (p, ty)) in f.sig.params.iter().zip(&params).enumerate() {
        fc.locals
            .insert(ident_key(&p.name), LocalVar::Slot(i as u8, *ty));
    }

    for l in &f.locals {
        fc.line = l.line;
        let ty = Ty::from_name(&l.ty);
        if l.array {
            let idx = fc.local_arrays.len() as u32;
            fc.local_arrays.push(ty);
            fc.locals
                .insert(ident_key(&l.name), LocalVar::Array(idx, ty));
        } else {
            let slot = fc.alloc()?;
            fc.locals
                .insert(ident_key(&l.name), LocalVar::Slot(slot, ty));
            fc.local_defaults.push((slot, ty));
            if let Some(init) = &l.init {
                let mark = fc.next;
                let (s, t) = fc.expr(init)?;
                let s = fc.coerce(s, t, ty)?;
                if s != slot {
                    fc.emit(Instr::new(Op::Move).left(s).dest(slot));
                }
                fc.next = mark;
            }
        }
    }

    fc.block(&f.body)?;
    // Falling off the end returns the default value of the return type.
    match ret {
        None => {
            fc.emit(Instr::new(Op::Return));
        }
        Some(t) => {
            let s = fc.alloc()?;
            fc.emit_default(t, s);
            fc.emit(Instr::new(Op::Return).right(s));
        }
    }
    Ok(fc.finish(f.sig.name.clone(), params))
}

#[derive(Debug, Clone, Copy)]
struct GlobalInfo {
    id: u32,
    ty: Ty,
    array: bool,
}

#[derive(Debug, Clone)]
struct Callee {
    script: bool,
    id: u32,
    params: Vec<Ty>,
    ret: Option<Ty>,
}

/// Module-wide symbol information used while compiling bodies.
#[derive(Debug, Default)]
struct Symbols {
    globals: BTreeMap<String, GlobalInfo>,
    funcs: BTreeMap<String, Callee>,
    strings: Vec<String>,
    string_index: BTreeMap<String, u32>,
}

impl Symbols {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.string_index.get(s) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.string_index.insert(s.to_string(), id);
        id
    }
}

#[derive(Debug, Clone, Copy)]
enum LocalVar {
    Slot(u8, Ty),
    Array(u32, Ty),
}

/// Compiles one function body. Owns the code buffer; `finish` produces the `FuncDef`.
struct FnCompiler<'a> {
    syms: &'a mut Symbols,
    locals: BTreeMap<String, LocalVar>,
    local_arrays: Vec<Ty>,
    local_defaults: Vec<(u8, Ty)>,
    ret: Option<Ty>,
    code: Vec<Instr>,
    /// Next free slot (temporaries live at and above this).
    next: u8,
    /// One past the highest slot ever used.
    high: usize,
    /// Exit jumps of each enclosing loop, patched when the loop ends.
    loops: Vec<Vec<usize>>,
    line: u32,
}

impl<'a> FnCompiler<'a> {
    fn new(syms: &'a mut Symbols, ret: Option<Ty>, nparams: usize) -> Self {
        FnCompiler {
            syms,
            locals: BTreeMap::new(),
            local_arrays: Vec::new(),
            local_defaults: Vec::new(),
            ret,
            code: Vec::new(),
            next: nparams as u8,
            high: nparams,
            loops: Vec::new(),
            line: 0,
        }
    }

    fn finish(self, name: String, params: Vec<Ty>) -> FuncDef {
        FuncDef {
            name,
            params,
            ret: self.ret,
            num_slots: self.high,
            locals: self.local_defaults,
            local_arrays: self.local_arrays,
            code: self.code,
        }
    }

    fn unsupported<T>(&self, message: impl Into<String>) -> CResult<T> {
        Err(CompileError::Unsupported {
            line: self.line,
            message: message.into(),
        })
    }

    fn emit(&mut self, i: Instr) -> usize {
        self.code.push(i);
        self.code.len() - 1
    }

    fn pc(&self) -> u32 {
        self.code.len() as u32
    }

    fn alloc(&mut self) -> CResult<u8> {
        if self.next == NO_SLOT {
            return self.unsupported("function needs more than 255 slots");
        }
        let s = self.next;
        self.next += 1;
        self.high = self.high.max(self.next as usize);
        Ok(s)
    }

    fn alloc_n(&mut self, n: usize) -> CResult<u8> {
        let base = self.next;
        for _ in 0..n {
            self.alloc()?;
        }
        Ok(base)
    }

    fn emit_default(&mut self, ty: Ty, dest: u8) {
        let i = match ty {
            Ty::Integer => Instr::new(Op::LoadInt).imm(0),
            Ty::Real => Instr::new(Op::LoadReal).imm(0f32.to_bits()),
            Ty::Boolean => Instr::new(Op::LoadBool).imm(0),
            Ty::String => Instr::new(Op::LoadString).imm(0),
            Ty::Handle | Ty::Code | Ty::Null => Instr::new(Op::LoadNull),
        };
        self.emit(i.dest(dest));
    }

    /// Makes a value of type `from` usable as `to`. Only integer-to-real widening changes the code.
    fn coerce(&mut self, slot: u8, from: Ty, to: Ty) -> CResult<u8> {
        if from == Ty::Integer && to == Ty::Real {
            let d = self.alloc()?;
            self.emit(Instr::new(Op::IntToReal).left(slot).dest(d));
            return Ok(d);
        }
        Ok(slot)
    }

    fn block(&mut self, body: &[Stmt]) -> CResult<()> {
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> CResult<()> {
        let mark = self.next;
        match s {
            Stmt::Set {
                name,
                index,
                value,
                line,
            } => {
                self.line = *line;
                let idx = match index {
                    Some(e) => Some(self.expr(e)?.0),
                    None => None,
                };
                let (vs, vt) = self.expr(value)?;
                if let Some(local) = self.locals.get(&ident_key(name)).copied() {
                    match local {
                        LocalVar::Slot(d, ty) => {
                            let v = self.coerce(vs, vt, ty)?;
                            if v != d {
                                self.emit(Instr::new(Op::Move).left(v).dest(d));
                            }
                        }
                        LocalVar::Array(ai, ty) => {
                            let v = self.coerce(vs, vt, ty)?;
                            let i = self.need_index(idx)?;
                            self.emit(Instr::new(Op::LocalArraySet).imm(ai).left(i).right(v));
                        }
                    }
                } else {
                    let g = self.global(name)?;
                    let v = self.coerce(vs, vt, g.ty)?;
                    if g.array {
                        let i = self.need_index(idx)?;
                        self.emit(Instr::new(Op::GlobalArraySet).imm(g.id).left(i).right(v));
                    } else {
                        self.emit(Instr::new(Op::GlobalSet).imm(g.id).right(v));
                    }
                }
            }
            Stmt::Call { name, args, line } => {
                self.line = *line;
                self.call(name, args, false)?;
            }
            Stmt::If {
                branches,
                otherwise,
                line,
            } => {
                self.line = *line;
                let mut end_jumps = Vec::new();
                for (cond, body) in branches {
                    let m = self.next;
                    let (c, _) = self.expr(cond)?;
                    let jf = self.emit(Instr::new(Op::JumpIfFalse).left(c));
                    self.next = m;
                    self.block(body)?;
                    end_jumps.push(self.emit(Instr::new(Op::Jump)));
                    let here = self.pc();
                    self.code[jf].imm = here;
                }
                self.block(otherwise)?;
                let end = self.pc();
                for j in end_jumps {
                    self.code[j].imm = end;
                }
            }
            Stmt::Loop { body, line } => {
                self.line = *line;
                let start = self.pc();
                self.loops.push(Vec::new());
                self.block(body)?;
                self.emit(Instr::new(Op::Jump).imm(start));
                let end = self.pc();
                for j in self.loops.pop().unwrap_or_default() {
                    self.code[j].imm = end;
                }
            }
            Stmt::ExitWhen { cond, line } => {
                self.line = *line;
                if self.loops.is_empty() {
                    return self.unsupported("'exitwhen' outside of a loop");
                }
                let (c, _) = self.expr(cond)?;
                let j = self.emit(Instr::new(Op::JumpIfTrue).left(c));
                if let Some(exits) = self.loops.last_mut() {
                    exits.push(j);
                }
            }
            Stmt::Return { value, line } => {
                self.line = *line;
                match value {
                    Some(v) => {
                        let (s, t) = self.expr(v)?;
                        let s = match self.ret {
                            Some(rt) => self.coerce(s, t, rt)?,
                            None => s,
                        };
                        self.emit(Instr::new(Op::Return).right(s));
                    }
                    None => {
                        self.emit(Instr::new(Op::Return));
                    }
                }
            }
            // UNVERIFIED vs retail: `debug` statements are skipped (release-build behaviour).
            Stmt::Debug(_) => {}
        }
        self.next = mark;
        Ok(())
    }

    fn need_index(&self, idx: Option<u8>) -> CResult<u8> {
        match idx {
            Some(i) => Ok(i),
            None => self.unsupported("array used without an index"),
        }
    }

    fn global(&self, name: &str) -> CResult<GlobalInfo> {
        match self.syms.globals.get(&ident_key(name)) {
            Some(g) => Ok(*g),
            None => self.unsupported(format!("unknown variable '{name}'")),
        }
    }

    /// Compiles a call. Returns the result slot (or `NO_SLOT` when `want` is false) and the type.
    fn call(&mut self, name: &str, args: &[Expr], want: bool) -> CResult<(u8, Ty)> {
        let Some(callee) = self.syms.funcs.get(&ident_key(name)).cloned() else {
            return self.unsupported(format!("unknown function '{name}'"));
        };
        let saved = self.next;
        let dest = if want {
            match callee.ret {
                Some(_) => self.alloc()?,
                None => return self.unsupported(format!("'{name}' returns nothing")),
            }
        } else {
            NO_SLOT
        };
        let base = self.alloc_n(args.len())?;
        let mark = self.next;
        for (i, (arg, pty)) in args.iter().zip(&callee.params).enumerate() {
            let (s, t) = self.expr(arg)?;
            let s = self.coerce(s, t, *pty)?;
            let target = base + i as u8;
            if s != target {
                self.emit(Instr::new(Op::Move).left(s).dest(target));
            }
            self.next = mark;
        }
        let argc = args.len() as u32;
        let op = if callee.script {
            Op::CallScript
        } else {
            Op::CallNative
        };
        let ins = Instr {
            right: argc as u8,
            left: base,
            dest,
            op,
            imm: callee.id,
        };
        self.emit(ins);
        self.next = saved;
        if dest != NO_SLOT {
            self.next = dest + 1;
        }
        Ok((dest, callee.ret.unwrap_or(Ty::Null)))
    }

    fn expr(&mut self, e: &Expr) -> CResult<(u8, Ty)> {
        match e {
            Expr::Int(i) => {
                let d = self.alloc()?;
                self.emit(Instr::new(Op::LoadInt).imm(*i as u32).dest(d));
                Ok((d, Ty::Integer))
            }
            Expr::Real(r) => {
                let d = self.alloc()?;
                self.emit(Instr::new(Op::LoadReal).imm(r.to_bits()).dest(d));
                Ok((d, Ty::Real))
            }
            Expr::Str(s) => {
                let id = self.syms.intern(s);
                let d = self.alloc()?;
                self.emit(Instr::new(Op::LoadString).imm(id).dest(d));
                Ok((d, Ty::String))
            }
            Expr::Bool(b) => {
                let d = self.alloc()?;
                self.emit(Instr::new(Op::LoadBool).imm(*b as u32).dest(d));
                Ok((d, Ty::Boolean))
            }
            Expr::Null => {
                let d = self.alloc()?;
                self.emit(Instr::new(Op::LoadNull).dest(d));
                Ok((d, Ty::Null))
            }
            Expr::Var(name) => {
                if let Some(local) = self.locals.get(&ident_key(name)).copied() {
                    return match local {
                        LocalVar::Slot(s, ty) => Ok((s, ty)),
                        LocalVar::Array(..) => self.unsupported("array used without an index"),
                    };
                }
                let g = self.global(name)?;
                if g.array {
                    return self.unsupported("array used without an index");
                }
                let d = self.alloc()?;
                self.emit(Instr::new(Op::GlobalGet).imm(g.id).dest(d));
                Ok((d, g.ty))
            }
            Expr::Index(name, idx) => {
                let (i, _) = self.expr(idx)?;
                if let Some(local) = self.locals.get(&ident_key(name)).copied() {
                    let LocalVar::Array(ai, ty) = local else {
                        return self.unsupported(format!("'{name}' is not an array"));
                    };
                    let d = self.alloc()?;
                    self.emit(Instr::new(Op::LocalArrayGet).imm(ai).left(i).dest(d));
                    return Ok((d, ty));
                }
                let g = self.global(name)?;
                if !g.array {
                    return self.unsupported(format!("'{name}' is not an array"));
                }
                let d = self.alloc()?;
                self.emit(Instr::new(Op::GlobalArrayGet).imm(g.id).left(i).dest(d));
                Ok((d, g.ty))
            }
            Expr::Call(name, args) => self.call(name, args, true),
            Expr::FuncRef(name) => {
                let Some(callee) = self.syms.funcs.get(&ident_key(name)).cloned() else {
                    return self.unsupported(format!("unknown function '{name}'"));
                };
                if !callee.script {
                    return self.unsupported(format!(
                        "native '{name}' used as a code value (not supported)"
                    ));
                }
                let d = self.alloc()?;
                self.emit(Instr::new(Op::LoadCode).imm(callee.id).dest(d));
                Ok((d, Ty::Code))
            }
            Expr::Neg(inner) => {
                let (s, t) = self.expr(inner)?;
                let d = self.alloc()?;
                let op = if t == Ty::Real { Op::NegR } else { Op::NegI };
                self.emit(Instr::new(op).left(s).dest(d));
                Ok((d, t))
            }
            Expr::Not(inner) => {
                let (s, _) = self.expr(inner)?;
                let d = self.alloc()?;
                self.emit(Instr::new(Op::Not).left(s).dest(d));
                Ok((d, Ty::Boolean))
            }
            Expr::Binary(op, l, r) => self.binary(*op, l, r),
        }
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> CResult<(u8, Ty)> {
        let (ls, lt) = self.expr(l)?;
        let (rs, rt) = self.expr(r)?;
        let numeric_real = lt == Ty::Real || rt == Ty::Real;
        // Operands of arithmetic and ordering are widened to real when either side is real.
        let (ls, rs, num_ty) = if numeric_real {
            let ls = self.coerce(ls, lt, Ty::Real)?;
            let rs = self.coerce(rs, rt, Ty::Real)?;
            (ls, rs, Ty::Real)
        } else {
            (ls, rs, Ty::Integer)
        };
        let (opcode, result) = match op {
            BinOp::And => (Op::And, Ty::Boolean),
            BinOp::Or => (Op::Or, Ty::Boolean),
            BinOp::Eq => (Op::Eq, Ty::Boolean),
            BinOp::Ne => (Op::Ne, Ty::Boolean),
            BinOp::Lt => (Op::Lt, Ty::Boolean),
            BinOp::Le => (Op::Le, Ty::Boolean),
            BinOp::Gt => (Op::Gt, Ty::Boolean),
            BinOp::Ge => (Op::Ge, Ty::Boolean),
            BinOp::Add if lt == Ty::String && rt == Ty::String => (Op::Concat, Ty::String),
            BinOp::Add => arith(num_ty, Op::AddI, Op::AddR),
            BinOp::Sub => arith(num_ty, Op::SubI, Op::SubR),
            BinOp::Mul => arith(num_ty, Op::MulI, Op::MulR),
            BinOp::Div => arith(num_ty, Op::DivI, Op::DivR),
        };
        let d = self.alloc()?;
        self.emit(Instr::new(opcode).left(ls).right(rs).dest(d));
        Ok((d, result))
    }
}

fn arith(num_ty: Ty, int_op: Op, real_op: Op) -> (Op, Ty) {
    if num_ty == Ty::Real {
        (real_op, Ty::Real)
    } else {
        (int_op, Ty::Integer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_a_function_with_params_and_return() {
        let p = crate::parse(
            "function Add takes integer a, integer b returns integer\n\
             return a + b\n\
             endfunction",
        )
        .unwrap();
        let m = compile(&[p]).unwrap();
        let f = &m.functions[m.function_index["add"] as usize];
        assert_eq!(f.params, vec![Ty::Integer, Ty::Integer]);
        assert_eq!(f.ret, Some(Ty::Integer));
        assert_eq!(f.code[0].op, Op::AddI);
        assert_eq!(f.code[0].left, 0);
        assert_eq!(f.code[0].right, 1);
        assert_eq!(f.code.last().map(|i| i.op), Some(Op::Return));
    }

    #[test]
    fn widens_integer_to_real() {
        let p = crate::parse(
            "function F takes nothing returns real\n\
             local integer i = 2\n\
             return i + 0.5\n\
             endfunction",
        )
        .unwrap();
        let m = compile(&[p]).unwrap();
        let f = &m.functions[0];
        assert!(f.code.iter().any(|i| i.op == Op::IntToReal));
        assert!(f.code.iter().any(|i| i.op == Op::AddR));
    }

    #[test]
    fn type_errors_block_compilation() {
        let p = crate::parse("function F takes nothing returns nothing\nset x = 1\nendfunction")
            .unwrap();
        assert!(matches!(compile(&[p]), Err(CompileError::Check(_))));
    }
}
