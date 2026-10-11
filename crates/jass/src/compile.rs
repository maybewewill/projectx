//! Compiles checked JASS programs to the original compiler's bytecode (see `bytecode.rs`).
//!
//! The emitted stream is the one of the original (Game.dll 1.26a), instruction for instruction:
//! type declarations first, then the globals initializer (if there are globals), then the functions
//! in source order, then `End`. The rules below were read from streams the original produced for
//! test texts (`tools/emu/jasscomp/parity.py` compares them):
//!
//! - Symbols: one table, case-insensitive, ids from 2 in order of appearance in the source. String
//!   literals share it. Type parents (`type x extends handle`) are interned after all other names,
//!   and the globals initializer takes the last id.
//! - Temporaries come from a counter that runs over the whole script (slots 1..=255, wrapping to 1).
//!   Declarations take no slot. Slot 0 is the return slot.
//! - Labels are numbered per script from 1. An `if` allocates its two labels after its condition,
//!   a `loop` allocates its top and exit labels before its body, `and`/`or` allocate the false/true
//!   label before the right operand and the end label after it.
//! - An `if` whose then-part and else-part are both empty emits only its condition. An empty `loop`
//!   emits nothing.
//! - Binary operators: left operand, PUSH, right operand, POP into a fresh slot, then the operator
//!   with dest = that slot. Integer operands of a mixed operation are converted with IntToReal
//!   right after they are evaluated.
//! - Calls: arguments are evaluated and pushed, CALL, then POP_ARGS with the count. The result is
//!   slot 0.
//!
//! UNVERIFIED: the native call encoding (see `bytecode.rs`) and the runtime meaning of `debug`.

use std::collections::BTreeMap;

use crate::ast::{self, ident_key, BinOp, Expr, Program, Stmt};
use crate::bytecode::{ty as tc, Instr, Op};
use crate::check::{self, Diagnostic};

/// Number of elements of every array variable (globals and locals).
pub const ARRAY_SIZE: usize = 8192;

/// Runtime type of a value. `Null` appears only as the static type of the `null` literal.
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
    /// The type named by a JASS type name (case-insensitive). User types are handles.
    pub fn from_name(name: &str) -> Ty {
        match ident_key(name).as_str() {
            "integer" => Ty::Integer,
            "real" => Ty::Real,
            "boolean" => Ty::Boolean,
            "string" => Ty::String,
            "code" => Ty::Code,
            _ => Ty::Handle,
        }
    }

    /// The type code the original writes for this type (`bytecode::ty`).
    pub fn code(self) -> u8 {
        match self {
            Ty::Integer => tc::INTEGER,
            Ty::Real => tc::REAL,
            Ty::Boolean => tc::BOOLEAN,
            Ty::String => tc::STRING,
            Ty::Handle => tc::HANDLE,
            Ty::Code => tc::CODE,
            Ty::Null => tc::NOTHING,
        }
    }
}

/// A compiled script function. `code` is the function's part of the stream: `FuncHdr` first,
/// `FuncEnd` last. Jump targets are label ids (see `Op::Label`).
#[derive(Debug, Clone, PartialEq)]
pub struct FuncDef {
    pub name: String,
    /// Symbol id of the name (what `CallScript` and `LoadFuncRef` refer to).
    pub sym: u32,
    pub params: Vec<Ty>,
    /// `None` for `returns nothing`.
    pub ret: Option<Ty>,
    /// Frame size: one more than the highest temporary slot used.
    pub num_slots: usize,
    pub code: Vec<Instr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDef {
    pub name: String,
    pub sym: u32,
    pub ty: Ty,
    pub array: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDef {
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Option<Ty>,
}

/// A compiled program.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Index 0 is the globals initializer (empty when there are no globals); the rest are the
    /// script functions in source order.
    pub functions: Vec<FuncDef>,
    /// Folded function name (`ast::ident_key`) to index in `functions`.
    pub function_index: BTreeMap<String, u32>,
    pub globals: Vec<GlobalDef>,
    /// Folded name to index in `globals`.
    pub global_index: BTreeMap<String, u32>,
    pub natives: Vec<NativeDef>,
    /// Folded name to index in `natives`.
    pub native_index: BTreeMap<String, u32>,
    /// Symbol id to the spelling of its first occurrence (ids 0 and 1 are unused).
    pub symbols: Vec<String>,
    /// Type declarations, emitted before everything else.
    pub prelude: Vec<Instr>,
    /// Index of the globals initializer in `functions` (always 0).
    pub init: u32,
}

impl Module {
    /// The flat instruction stream exactly as the original lays it out.
    pub fn stream(&self) -> Vec<Instr> {
        let mut v = self.prelude.clone();
        for f in &self.functions {
            v.extend_from_slice(&f.code);
        }
        v.push(Instr::new(Op::End));
        v
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompileError {
    /// The programs did not type check.
    Check(Vec<Diagnostic>),
    /// Something the compiler cannot express.
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

// ---------------------------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------------------------

/// The script-wide name table. Names fold case; the spelling kept is the first one seen.
#[derive(Debug)]
struct Symbols {
    ids: BTreeMap<String, u32>,
    names: Vec<String>,
}

impl Symbols {
    fn new() -> Self {
        Symbols {
            ids: BTreeMap::new(),
            names: vec![String::new(), String::new()],
        }
    }

    fn intern(&mut self, name: &str) -> u32 {
        let key = ident_key(name);
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(name.to_string());
        self.ids.insert(key, id);
        id
    }

    /// An id that no name refers to (the globals initializer).
    fn anonymous(&mut self) -> u32 {
        self.names.push(String::new());
        (self.names.len() - 1) as u32
    }

    fn get(&self, name: &str) -> Option<u32> {
        self.ids.get(&ident_key(name)).copied()
    }
}

/// Interns every name in the order the original's parser meets them. Items of one program are
/// visited by source line (types, globals, natives and functions interleave in the source).
fn assign_symbols(programs: &[Program], syms: &mut Symbols, parents: &mut Vec<String>) {
    enum Item<'a> {
        Type(&'a ast::TypeDef),
        Global(&'a ast::Global),
        Native(&'a ast::Native),
        Function(&'a ast::Function),
    }
    for p in programs {
        let mut items: Vec<(u32, Item<'_>)> = Vec::new();
        items.extend(p.types.iter().map(|t| (t.line, Item::Type(t))));
        items.extend(p.globals.iter().map(|g| (g.line, Item::Global(g))));
        items.extend(p.natives.iter().map(|n| (n.line, Item::Native(n))));
        items.extend(p.functions.iter().map(|f| (f.line, Item::Function(f))));
        items.sort_by_key(|(line, _)| *line);
        for (_, item) in items {
            match item {
                Item::Type(t) => {
                    syms.intern(&t.name);
                    parents.push(t.parent.clone());
                }
                Item::Global(g) => {
                    syms.intern(&g.name);
                    if let Some(e) = &g.init {
                        walk_expr(e, syms);
                    }
                }
                Item::Native(n) => {
                    syms.intern(&n.sig.name);
                    for prm in &n.sig.params {
                        syms.intern(&prm.name);
                    }
                }
                Item::Function(f) => {
                    syms.intern(&f.sig.name);
                    for prm in &f.sig.params {
                        syms.intern(&prm.name);
                    }
                    for l in &f.locals {
                        syms.intern(&l.name);
                        if let Some(e) = &l.init {
                            walk_expr(e, syms);
                        }
                    }
                    walk_stmts(&f.body, syms);
                }
            }
        }
    }
}

fn walk_stmts(body: &[Stmt], syms: &mut Symbols) {
    for s in body {
        match s {
            Stmt::Set {
                name, index, value, ..
            } => {
                syms.intern(name);
                if let Some(i) = index {
                    walk_expr(i, syms);
                }
                walk_expr(value, syms);
            }
            Stmt::Call { name, args, .. } => {
                syms.intern(name);
                for a in args {
                    walk_expr(a, syms);
                }
            }
            Stmt::If {
                branches,
                otherwise,
                ..
            } => {
                for (c, b) in branches {
                    walk_expr(c, syms);
                    walk_stmts(b, syms);
                }
                walk_stmts(otherwise, syms);
            }
            Stmt::Loop { body, .. } => walk_stmts(body, syms),
            Stmt::ExitWhen { cond, .. } => walk_expr(cond, syms),
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    walk_expr(v, syms);
                }
            }
            Stmt::Debug(inner) => walk_stmts(std::slice::from_ref(inner.as_ref()), syms),
        }
    }
}

fn walk_expr(e: &Expr, syms: &mut Symbols) {
    match e {
        Expr::Str(s) => {
            syms.intern(s);
        }
        Expr::Var(n) | Expr::FuncRef(n) => {
            syms.intern(n);
        }
        Expr::Index(n, i) => {
            syms.intern(n);
            walk_expr(i, syms);
        }
        Expr::Call(n, args) => {
            syms.intern(n);
            for a in args {
                walk_expr(a, syms);
            }
        }
        Expr::Neg(x) | Expr::Not(x) => walk_expr(x, syms),
        Expr::Binary(_, l, r) => {
            walk_expr(l, syms);
            walk_expr(r, syms);
        }
        Expr::Int(_) | Expr::Real(_) | Expr::Bool(_) | Expr::Null => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Script-wide code generation state
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Callee {
    script: bool,
    /// Symbol of a script function.
    sym: u32,
    /// Index of a native.
    idx: u32,
    params: Vec<Ty>,
    ret: Option<Ty>,
}

#[derive(Debug, Clone, Copy)]
struct GlobalInfo {
    sym: u32,
    ty: Ty,
    array: bool,
}

/// State shared by all functions of one script: names, temporaries and labels.
struct Gen {
    syms: Symbols,
    funcs: BTreeMap<String, Callee>,
    globals: BTreeMap<String, GlobalInfo>,
    /// Next temporary slot (1..=255, wraps to 1).
    slot: u8,
    /// Next label id.
    label: u32,
}

impl Gen {
    fn alloc_slot(&mut self) -> u8 {
        let s = self.slot;
        self.slot = if s == 255 { 1 } else { s + 1 };
        s
    }

    fn new_label(&mut self) -> u32 {
        let l = self.label;
        self.label += 1;
        l
    }
}

/// A variable reference as seen from the current function.
#[derive(Debug, Clone, Copy)]
struct VarRef {
    sym: u32,
    ty: Ty,
    array: bool,
}

/// Compiles one function body (or the globals initializer).
struct FnCx<'a> {
    g: &'a mut Gen,
    code: Vec<Instr>,
    /// Folded name to (type, is array) of the function's parameters and locals.
    locals: BTreeMap<String, (Ty, bool)>,
    /// Exit label of each enclosing loop.
    loops: Vec<u32>,
    /// Highest temporary slot allocated by this function.
    high: u8,
    line: u32,
}

impl<'a> FnCx<'a> {
    fn new(g: &'a mut Gen) -> Self {
        FnCx {
            g,
            code: Vec::new(),
            locals: BTreeMap::new(),
            loops: Vec::new(),
            high: 0,
            line: 0,
        }
    }

    fn emit(&mut self, i: Instr) {
        self.code.push(i);
    }

    fn alloc(&mut self) -> u8 {
        let s = self.g.alloc_slot();
        self.high = self.high.max(s);
        s
    }

    fn label(&mut self) -> u32 {
        self.g.new_label()
    }

    fn unsupported<T>(&self, message: impl Into<String>) -> CResult<T> {
        Err(CompileError::Unsupported {
            line: self.line,
            message: message.into(),
        })
    }

    fn sym(&self, name: &str) -> CResult<u32> {
        match self.g.syms.get(name) {
            Some(s) => Ok(s),
            None => self.unsupported(format!("name '{name}' was not interned")),
        }
    }

    fn var(&self, name: &str) -> CResult<VarRef> {
        let key = ident_key(name);
        if let Some(&(ty, array)) = self.locals.get(&key) {
            return Ok(VarRef {
                sym: self.sym(name)?,
                ty,
                array,
            });
        }
        match self.g.globals.get(&key) {
            Some(g) => Ok(VarRef {
                sym: g.sym,
                ty: g.ty,
                array: g.array,
            }),
            None => self.unsupported(format!("unknown variable '{name}'")),
        }
    }

    fn var_ty(&self, name: &str) -> Ty {
        let key = ident_key(name);
        if let Some(&(ty, _)) = self.locals.get(&key) {
            return ty;
        }
        self.g.globals.get(&key).map_or(Ty::Null, |g| g.ty)
    }

    /// The type of an expression, computed without generating code. Used to decide conversions and
    /// the context of a `null` literal.
    fn static_ty(&self, e: &Expr) -> Ty {
        match e {
            Expr::Int(_) => Ty::Integer,
            Expr::Real(_) => Ty::Real,
            Expr::Str(_) => Ty::String,
            Expr::Bool(_) => Ty::Boolean,
            Expr::Null => Ty::Null,
            Expr::Var(n) | Expr::Index(n, _) => self.var_ty(n),
            Expr::Call(n, _) => self
                .g
                .funcs
                .get(&ident_key(n))
                .and_then(|c| c.ret)
                .unwrap_or(Ty::Null),
            Expr::FuncRef(_) => Ty::Code,
            Expr::Neg(x) => self.static_ty(x),
            Expr::Not(_) => Ty::Boolean,
            Expr::Binary(op, l, r) => match op {
                BinOp::And
                | BinOp::Or
                | BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge => Ty::Boolean,
                _ => {
                    let (lt, rt) = (self.static_ty(l), self.static_ty(r));
                    if *op == BinOp::Add && lt == Ty::String {
                        Ty::String
                    } else if lt == Ty::Real || rt == Ty::Real {
                        Ty::Real
                    } else {
                        Ty::Integer
                    }
                }
            },
        }
    }

    /// Integer-to-real conversion of `slot` in place, when needed.
    fn coerce(&mut self, slot: u8, from: Ty, to: Ty) -> u8 {
        if from == Ty::Integer && to == Ty::Real {
            self.emit(Instr::new(Op::IntToReal).dest(slot));
        }
        slot
    }

    /// Expression code. Returns the slot that holds the value and its type. `hint` is the type of
    /// the other operand of `==`/`!=`, which types a `null` literal.
    fn expr(&mut self, e: &Expr, hint: Option<Ty>) -> CResult<(u8, Ty)> {
        match e {
            Expr::Int(i) => {
                let d = self.alloc();
                self.emit(
                    Instr::new(Op::LoadConst)
                        .left(tc::INTEGER)
                        .dest(d)
                        .imm(*i as u32),
                );
                Ok((d, Ty::Integer))
            }
            Expr::Real(r) => {
                let d = self.alloc();
                self.emit(
                    Instr::new(Op::LoadConst)
                        .left(tc::REAL)
                        .dest(d)
                        .imm(r.to_bits()),
                );
                Ok((d, Ty::Real))
            }
            Expr::Str(s) => {
                let sym = self.sym(s)?;
                let d = self.alloc();
                self.emit(Instr::new(Op::LoadConst).left(tc::STRING).dest(d).imm(sym));
                Ok((d, Ty::String))
            }
            Expr::Bool(b) => {
                let d = self.alloc();
                self.emit(
                    Instr::new(Op::LoadConst)
                        .left(tc::BOOLEAN)
                        .dest(d)
                        .imm(*b as u32),
                );
                Ok((d, Ty::Boolean))
            }
            Expr::Null => {
                let code = match hint {
                    Some(t) if t != Ty::Null => t.code(),
                    _ => tc::NULL,
                };
                let d = self.alloc();
                self.emit(Instr::new(Op::LoadConst).left(code).dest(d));
                Ok((d, Ty::Null))
            }
            Expr::Var(n) => {
                let v = self.var(n)?;
                if v.array {
                    return self.unsupported("array used without an index");
                }
                let d = self.alloc();
                self.emit(Instr::new(Op::LoadVar).left(v.ty.code()).dest(d).imm(v.sym));
                Ok((d, v.ty))
            }
            Expr::Index(n, idx) => {
                let v = self.var(n)?;
                if !v.array {
                    return self.unsupported(format!("'{n}' is not an array"));
                }
                let (i, _) = self.expr(idx, None)?;
                let d = self.alloc();
                self.emit(
                    Instr::new(Op::ArrayLoad)
                        .right(v.ty.code())
                        .left(i)
                        .dest(d)
                        .imm(v.sym),
                );
                Ok((d, v.ty))
            }
            Expr::Call(n, args) => self.call(n, args),
            Expr::FuncRef(n) => {
                let callee = self.callee(n)?;
                if !callee.script {
                    return self
                        .unsupported(format!("native '{n}' used as a code value (not supported)"));
                }
                let d = self.alloc();
                self.emit(Instr::new(Op::LoadFuncRef).left(3).dest(d).imm(callee.sym));
                Ok((d, Ty::Code))
            }
            Expr::Neg(x) => {
                let (s, t) = self.expr(x, None)?;
                self.emit(Instr::new(Op::Neg).dest(s));
                Ok((s, t))
            }
            Expr::Not(x) => {
                let (s, _) = self.expr(x, None)?;
                self.emit(Instr::new(Op::Not).dest(s));
                Ok((s, Ty::Boolean))
            }
            Expr::Binary(op, l, r) => self.binary(*op, l, r),
        }
    }

    fn callee(&self, name: &str) -> CResult<Callee> {
        match self.g.funcs.get(&ident_key(name)) {
            Some(c) => Ok(c.clone()),
            None => self.unsupported(format!("unknown function '{name}'")),
        }
    }

    /// A call. Arguments are pushed, then CALL, then POP_ARGS. The result is in slot 0.
    fn call(&mut self, name: &str, args: &[Expr]) -> CResult<(u8, Ty)> {
        let c = self.callee(name)?;
        for (i, a) in args.iter().enumerate() {
            let (s, t) = self.expr(a, None)?;
            let to = c.params.get(i).copied().unwrap_or(t);
            let s = self.coerce(s, t, to);
            self.emit(Instr::new(Op::Push).dest(s));
        }
        if c.script {
            self.emit(Instr::new(Op::CallScript).imm(c.sym));
        } else {
            self.emit(Instr::new(Op::CallNative).imm(c.idx));
        }
        if !args.is_empty() {
            self.emit(Instr::new(Op::PopArgs).dest(args.len() as u8));
        }
        Ok((0, c.ret.unwrap_or(Ty::Null)))
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> CResult<(u8, Ty)> {
        if op == BinOp::And || op == BinOp::Or {
            return self.logical(op == BinOp::Or, l, r);
        }
        let (lst, rst) = (self.static_ty(l), self.static_ty(r));
        let equality = op == BinOp::Eq || op == BinOp::Ne;
        let (ls, lt) = self.expr(l, equality.then_some(rst))?;
        let ls = if lt == Ty::Integer && rst == Ty::Real {
            self.coerce(ls, lt, Ty::Real)
        } else {
            ls
        };
        self.emit(Instr::new(Op::Push).dest(ls));
        let (rs, rt) = self.expr(r, equality.then_some(lst))?;
        let rs = if rt == Ty::Integer && lt == Ty::Real {
            self.coerce(rs, rt, Ty::Real)
        } else {
            rs
        };
        let p = self.alloc();
        self.emit(Instr::new(Op::Pop).dest(p));
        let opcode = match op {
            BinOp::Add => Op::Add,
            BinOp::Sub => Op::Sub,
            BinOp::Mul => Op::Mul,
            BinOp::Div => Op::Div,
            BinOp::Eq => Op::Eq,
            BinOp::Ne => Op::Ne,
            BinOp::Lt => Op::Lt,
            BinOp::Le => Op::Le,
            BinOp::Gt => Op::Gt,
            BinOp::Ge => Op::Ge,
            BinOp::And | BinOp::Or => unreachable!("handled by logical"),
        };
        self.emit(Instr::new(opcode).right(rs).left(p).dest(p));
        let ty = match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Ty::Boolean,
            BinOp::Add if lt == Ty::String => Ty::String,
            _ if lt == Ty::Real || rt == Ty::Real => Ty::Real,
            _ => Ty::Integer,
        };
        Ok((p, ty))
    }

    /// Short-circuit `and` / `or` with jumps. The result is the right operand's slot; the
    /// short-circuit path loads the constant into that same slot.
    fn logical(&mut self, is_or: bool, l: &Expr, r: &Expr) -> CResult<(u8, Ty)> {
        let (ls, _) = self.expr(l, None)?;
        let short = self.label();
        let jump = if is_or {
            Op::JumpIfTrue
        } else {
            Op::JumpIfFalse
        };
        self.emit(Instr::new(jump).dest(ls).imm(short));
        let (rs, _) = self.expr(r, None)?;
        let end = self.label();
        self.emit(Instr::new(Op::Jump).imm(end));
        self.emit(Instr::new(Op::Label).imm(short));
        self.emit(
            Instr::new(Op::LoadConst)
                .left(tc::BOOLEAN)
                .dest(rs)
                .imm(is_or as u32),
        );
        self.emit(Instr::new(Op::Label).imm(end));
        Ok((rs, Ty::Boolean))
    }

    fn block(&mut self, body: &[Stmt]) -> CResult<()> {
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> CResult<()> {
        match s {
            Stmt::Set {
                name,
                index,
                value,
                line,
            } => {
                self.line = *line;
                let v = self.var(name)?;
                match index {
                    None => {
                        let (vs, vt) = self.expr(value, None)?;
                        let vs = self.coerce(vs, vt, v.ty);
                        self.emit(Instr::new(Op::StoreVar).dest(vs).imm(v.sym));
                    }
                    Some(ix) => {
                        let (is, _) = self.expr(ix, None)?;
                        self.emit(Instr::new(Op::Push).dest(is));
                        let (vs, vt) = self.expr(value, None)?;
                        let vs = self.coerce(vs, vt, v.ty);
                        let p = self.alloc();
                        self.emit(Instr::new(Op::Pop).dest(p));
                        self.emit(Instr::new(Op::ArrayStore).left(vs).dest(p).imm(v.sym));
                    }
                }
            }
            Stmt::Call { name, args, line } => {
                self.line = *line;
                self.call(name, args)?;
            }
            Stmt::If {
                branches,
                otherwise,
                line,
            } => {
                self.line = *line;
                self.chain(branches, otherwise)?;
            }
            Stmt::Loop { body, line } => {
                self.line = *line;
                if body.is_empty() {
                    return Ok(());
                }
                let top = self.label();
                let exit = self.label();
                self.emit(Instr::new(Op::Label).imm(top));
                self.loops.push(exit);
                self.block(body)?;
                self.loops.pop();
                self.emit(Instr::new(Op::Jump).imm(top));
                self.emit(Instr::new(Op::Label).imm(exit));
            }
            Stmt::ExitWhen { cond, line } => {
                self.line = *line;
                let Some(&exit) = self.loops.last() else {
                    return self.unsupported("'exitwhen' outside of a loop");
                };
                let (c, _) = self.expr(cond, None)?;
                self.emit(Instr::new(Op::JumpIfTrue).dest(c).imm(exit));
            }
            Stmt::Return { value, line } => {
                self.line = *line;
                match value {
                    Some(v) => {
                        let (s, _) = self.expr(v, None)?;
                        self.emit(Instr::new(Op::Move).left(s).dest(0));
                        self.emit(Instr::new(Op::Return));
                    }
                    None => self.emit(Instr::new(Op::Return)),
                }
            }
            Stmt::Debug(inner) => {
                let skip = self.label();
                self.emit(Instr::new(Op::DebugSkip).imm(skip));
                self.stmt(inner)?;
                self.emit(Instr::new(Op::Label).imm(skip));
            }
        }
        Ok(())
    }

    /// `if` / `elseif` / `else`. An `elseif` is an `if` in the else part.
    fn chain(&mut self, branches: &[(Expr, Vec<Stmt>)], otherwise: &[Stmt]) -> CResult<()> {
        let Some(((cond, body), rest)) = branches.split_first() else {
            return self.block(otherwise);
        };
        if body.is_empty() && rest.is_empty() && otherwise.is_empty() {
            self.expr(cond, None)?;
            return Ok(());
        }
        let (c, _) = self.expr(cond, None)?;
        let next = self.label();
        let end = self.label();
        self.emit(Instr::new(Op::JumpIfFalse).dest(c).imm(next));
        self.block(body)?;
        self.emit(Instr::new(Op::Jump).imm(end));
        self.emit(Instr::new(Op::Label).imm(next));
        self.chain(rest, otherwise)?;
        self.emit(Instr::new(Op::Label).imm(end));
        Ok(())
    }

    fn finish(self, name: String, sym: u32, params: Vec<Ty>, ret: Option<Ty>) -> FuncDef {
        FuncDef {
            name,
            sym,
            params,
            ret,
            num_slots: self.high as usize + 1,
            code: self.code,
        }
    }
}

fn compile_function(g: &mut Gen, f: &ast::Function) -> CResult<FuncDef> {
    let mut cx = FnCx::new(g);
    cx.line = f.line;
    let sym = cx.sym(&f.sig.name)?;
    let ret = f.sig.returns.as_deref().map(Ty::from_name);
    let params: Vec<Ty> = f.sig.params.iter().map(|p| Ty::from_name(&p.ty)).collect();
    cx.emit(
        Instr::new(Op::FuncHdr)
            .dest(ret.map_or(tc::NOTHING, Ty::code))
            .imm(sym),
    );
    let n = f.sig.params.len();
    for (i, (p, ty)) in f.sig.params.iter().zip(&params).enumerate() {
        cx.locals.insert(ident_key(&p.name), (*ty, false));
        let psym = cx.sym(&p.name)?;
        cx.emit(
            Instr::new(Op::ParamBind)
                .left((n - i) as u8)
                .dest(ty.code())
                .imm(psym),
        );
    }
    for l in &f.locals {
        cx.line = l.line;
        let ty = Ty::from_name(&l.ty);
        let lsym = cx.sym(&l.name)?;
        cx.locals.insert(ident_key(&l.name), (ty, l.array));
        if l.array {
            if ty == Ty::Code {
                return cx.unsupported("code arrays are not supported");
            }
            cx.emit(
                Instr::new(Op::DeclLocal)
                    .dest(ty.code() + tc::ARRAY_OFFSET)
                    .imm(lsym),
            );
        } else {
            cx.emit(Instr::new(Op::DeclLocal).dest(ty.code()).imm(lsym));
            if let Some(init) = &l.init {
                let (s, t) = cx.expr(init, None)?;
                let s = cx.coerce(s, t, ty);
                cx.emit(Instr::new(Op::StoreVar).dest(s).imm(lsym));
            }
        }
    }
    cx.block(&f.body)?;
    // Falling off the end: the default value (slot 0 is loaded with the type-0 null constant).
    cx.emit(Instr::new(Op::LoadConst).left(tc::NOTHING).dest(0));
    cx.emit(Instr::new(Op::Return));
    cx.emit(Instr::new(Op::FuncEnd));
    Ok(cx.finish(f.sig.name.clone(), sym, params, ret))
}

fn compile_init(g: &mut Gen, programs: &[Program], sym: u32) -> CResult<FuncDef> {
    let mut cx = FnCx::new(g);
    cx.emit(Instr::new(Op::FuncHdr).imm(sym));
    for p in programs {
        for gl in &p.globals {
            cx.line = gl.line;
            let info = cx.g.globals[&ident_key(&gl.name)];
            let code = if info.array {
                info.ty.code() + tc::ARRAY_OFFSET
            } else {
                info.ty.code()
            };
            let op = if gl.constant {
                Op::DeclConstGlobal
            } else {
                Op::DeclGlobal
            };
            cx.emit(Instr::new(op).dest(code).imm(info.sym));
            if let Some(init) = &gl.init {
                let (s, t) = cx.expr(init, None)?;
                let s = cx.coerce(s, t, info.ty);
                cx.emit(Instr::new(Op::StoreVar).dest(s).imm(info.sym));
            }
        }
    }
    cx.emit(Instr::new(Op::Return));
    cx.emit(Instr::new(Op::FuncEnd));
    Ok(cx.finish("<globals>".to_string(), sym, Vec::new(), None))
}

/// Compiles `programs` (in load order) into a `Module`.
pub fn compile(programs: &[Program]) -> Result<Module, CompileError> {
    compile_with(programs, &check::CheckOptions::default())
}

/// Like [`compile`], with checker options (the native registry of the game, lenient returns).
pub fn compile_with(
    programs: &[Program],
    opts: &check::CheckOptions,
) -> Result<Module, CompileError> {
    let diags = check::check_with(programs, opts);
    if !diags.is_empty() {
        return Err(CompileError::Check(diags));
    }

    let mut syms = Symbols::new();
    let mut parents = Vec::new();
    assign_symbols(programs, &mut syms, &mut parents);
    for p in &parents {
        syms.intern(p);
    }
    let has_globals = programs.iter().any(|p| !p.globals.is_empty());
    let init_sym = if has_globals { syms.anonymous() } else { 0 };

    let mut g = Gen {
        syms,
        funcs: BTreeMap::new(),
        globals: BTreeMap::new(),
        slot: 1,
        label: 1,
    };
    let mut globals = Vec::new();
    let mut natives = Vec::new();
    for p in programs {
        for gl in &p.globals {
            let sym = g.syms.get(&gl.name).unwrap_or(0);
            let ty = Ty::from_name(&gl.ty);
            globals.push(GlobalDef {
                name: gl.name.clone(),
                sym,
                ty,
                array: gl.array,
            });
            g.globals.insert(
                ident_key(&gl.name),
                GlobalInfo {
                    sym,
                    ty,
                    array: gl.array,
                },
            );
        }
        for n in &p.natives {
            let idx = natives.len() as u32;
            let params: Vec<Ty> = n.sig.params.iter().map(|p| Ty::from_name(&p.ty)).collect();
            let ret = n.sig.returns.as_deref().map(Ty::from_name);
            natives.push(NativeDef {
                name: n.sig.name.clone(),
                params: params.clone(),
                ret,
            });
            g.funcs.insert(
                ident_key(&n.sig.name),
                Callee {
                    script: false,
                    sym: 0,
                    idx,
                    params,
                    ret,
                },
            );
        }
        for f in &p.functions {
            let params: Vec<Ty> = f.sig.params.iter().map(|p| Ty::from_name(&p.ty)).collect();
            let ret = f.sig.returns.as_deref().map(Ty::from_name);
            let sym = g.syms.get(&f.sig.name).unwrap_or(0);
            g.funcs.insert(
                ident_key(&f.sig.name),
                Callee {
                    script: true,
                    sym,
                    idx: 0,
                    params,
                    ret,
                },
            );
        }
    }

    let mut prelude = Vec::new();
    for p in programs {
        for t in &p.types {
            let parent = g.syms.get(&t.parent).unwrap_or(0);
            let name = g.syms.get(&t.name).unwrap_or(0);
            prelude.push(Instr::new(Op::TypeParent).imm(parent));
            prelude.push(Instr::new(Op::TypeName).imm(name));
        }
    }

    let init = if has_globals {
        compile_init(&mut g, programs, init_sym)?
    } else {
        FuncDef {
            name: "<globals>".to_string(),
            sym: 0,
            params: Vec::new(),
            ret: None,
            num_slots: 1,
            code: Vec::new(),
        }
    };
    let mut functions = vec![init];
    let mut function_index = BTreeMap::new();
    for p in programs {
        for f in &p.functions {
            let def = compile_function(&mut g, f)?;
            function_index.insert(ident_key(&f.sig.name), functions.len() as u32);
            functions.push(def);
        }
    }

    let global_index = globals
        .iter()
        .enumerate()
        .map(|(i, d)| (ident_key(&d.name), i as u32))
        .collect();
    let native_index = natives
        .iter()
        .enumerate()
        .map(|(i, d)| (ident_key(&d.name), i as u32))
        .collect();
    Ok(Module {
        functions,
        function_index,
        globals,
        global_index,
        natives,
        native_index,
        symbols: g.syms.names,
        prelude,
        init: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(f: &FuncDef) -> Vec<Op> {
        f.code.iter().map(|i| i.op).collect()
    }

    #[test]
    fn empty_function_frame() {
        let p = crate::parse("function F takes nothing returns nothing\nendfunction").unwrap();
        let m = compile(&[p]).unwrap();
        assert_eq!(
            ops(&m.functions[1]),
            vec![Op::FuncHdr, Op::LoadConst, Op::Return, Op::FuncEnd]
        );
        assert_eq!(m.stream().last().map(|i| i.op), Some(Op::End));
    }

    #[test]
    fn binary_op_pushes_and_pops() {
        let p = crate::parse(
            "function F takes nothing returns nothing\nlocal integer a = 1 + 2\nendfunction",
        )
        .unwrap();
        let m = compile(&[p]).unwrap();
        assert_eq!(
            ops(&m.functions[1])[1..],
            [
                Op::DeclLocal,
                Op::LoadConst,
                Op::Push,
                Op::LoadConst,
                Op::Pop,
                Op::Add,
                Op::StoreVar,
                Op::LoadConst,
                Op::Return,
                Op::FuncEnd
            ]
        );
    }

    #[test]
    fn integer_to_real_is_converted_in_place() {
        let p =
            crate::parse("function F takes nothing returns nothing\nlocal real r = 1\nendfunction")
                .unwrap();
        let m = compile(&[p]).unwrap();
        assert!(m.functions[1].code.iter().any(|i| i.op == Op::IntToReal));
    }

    #[test]
    fn type_errors_block_compilation() {
        let p = crate::parse("function F takes nothing returns nothing\nset x = 1\nendfunction")
            .unwrap();
        assert!(matches!(compile(&[p]), Err(CompileError::Check(_))));
    }
}
