//! Semantic analysis (type checking) for JASS programs.
//!
//! Several programs (e.g. common.j, blizzard.j, war3map.j) are checked as one combined namespace,
//! in the order given. Problems are collected, not raised at the first one, and the diagnostics come out
//! in a fixed order (types, declarations, global initializers, function bodies), so the output is
//! deterministic. When more than one program is checked, each message is prefixed with
//! `program N:` (1-based position in the slice) because `Diagnostic` only carries a line.
//!
//! Where a rule is the common-sense choice rather than a verified one, the code says so with an
//! `// UNVERIFIED vs retail:` comment.

use std::collections::BTreeMap;

use crate::ast::*;

/// A semantic error. `line` is the source line of the declaration or statement that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: u32,
    pub message: String,
}

fn message(multi: bool, prog: usize, msg: String) -> String {
    if multi {
        format!("program {}: {msg}", prog + 1)
    } else {
        msg
    }
}

/// Builtin type names (`handle` is the root of all handle types and is handled separately).
const VALUE_TYPES: [&str; 6] = ["integer", "real", "boolean", "string", "code", "nothing"];

/// A position in the declaration stream: (program index, line). Positions order declarations.
type Pos = (usize, u32);

/// The position after every declaration (used for function bodies, where all types are visible).
const END: Pos = (usize::MAX, u32::MAX);

/// The set of known type names and the `extends` relation between them.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// Every known type. Builtins map to `None`; user types map to their declared parent.
    /// After `build` every user type's parent chain ends at `handle`.
    parents: BTreeMap<String, Option<String>>,
    /// Where each user type is declared. Builtins have no entry.
    positions: BTreeMap<String, Pos>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    /// An environment that only contains the builtin types.
    pub fn new() -> Self {
        let mut parents = BTreeMap::new();
        for name in VALUE_TYPES.iter().chain(std::iter::once(&"handle")) {
            parents.insert((*name).to_string(), None);
        }
        Self {
            parents,
            positions: BTreeMap::new(),
        }
    }

    /// Declares the types of all `programs` in stream order. A type may name only the builtins and
    /// types declared above it (harness t02, t05, t08 in `tools/emu/jasscomp`); a bad declaration is
    /// repaired so that later checks see a consistent hierarchy, and each repair is reported.
    ///
    /// Repairs: a duplicate name is ignored (the first declaration wins); an unknown parent, or a
    /// builtin parent other than `handle`, is replaced with `handle`.
    pub fn build(programs: &[Program]) -> (TypeEnv, Vec<Diagnostic>) {
        let multi = programs.len() > 1;
        let mut env = TypeEnv::new();
        let mut diags = Vec::new();
        for (pi, p) in programs.iter().enumerate() {
            for t in &p.types {
                let key = ident_key(&t.name);
                let mut parent = ident_key(&t.parent);
                if env.parents.contains_key(&key) {
                    let msg = format!("duplicate type '{}'", t.name);
                    diags.push(Diagnostic {
                        line: t.line,
                        message: message(multi, pi, msg),
                    });
                    continue;
                }
                if !env.parents.contains_key(&parent) {
                    let msg = format!("type '{}' extends unknown type '{}'", t.name, t.parent);
                    diags.push(Diagnostic {
                        line: t.line,
                        message: message(multi, pi, msg),
                    });
                    parent = "handle".to_string();
                } else if parent != "handle" && VALUE_TYPES.contains(&parent.as_str()) {
                    // UNVERIFIED vs retail: user types may only extend handle-derived types
                    // (e.g. `extends integer` is rejected).
                    let msg = format!(
                        "type '{}' must extend a handle type, not '{}'",
                        t.name, t.parent
                    );
                    diags.push(Diagnostic {
                        line: t.line,
                        message: message(multi, pi, msg),
                    });
                    parent = "handle".to_string();
                }
                env.parents.insert(key.clone(), Some(parent));
                env.positions.insert(key, (pi, t.line));
            }
        }
        (env, diags)
    }

    /// Whether the type `name` is known at position `at`: a builtin, or a user type declared above it.
    fn declared_before(&self, name: &str, at: Pos) -> bool {
        let key = ident_key(name);
        match self.positions.get(&key) {
            Some(&declared) => declared < at,
            None => self.parents.contains_key(&key),
        }
    }

    /// Whether the type `name` is known (builtin or user-declared).
    pub fn is_known(&self, name: &str) -> bool {
        self.parents.contains_key(&ident_key(name))
    }

    /// `a` is `b`, or `a` reaches `b` by following `extends`. `integer` is not a subtype of `real`;
    /// that widening is handled by assignability.
    pub fn is_subtype(&self, a: &str, b: &str) -> bool {
        let b = ident_key(b);
        let mut cur = ident_key(a);
        for _ in 0..=self.parents.len() {
            if cur == b {
                return true;
            }
            match self.parents.get(&cur) {
                Some(Some(parent)) => cur = parent.clone(),
                _ => return false,
            }
        }
        false
    }

    /// `handle` itself, or any type that extends it.
    pub fn is_handle_derived(&self, name: &str) -> bool {
        self.is_subtype(name, "handle")
    }

    fn assignable(&self, from: &Ty, to: &Ty) -> bool {
        match (from, to) {
            (a, b) if a == b => true,
            // `null` is accepted by handle-derived types, string and code.
            (Ty::Null, Ty::String | Ty::Code) => true,
            (Ty::Null, Ty::Named(n)) => self.is_handle_derived(n),
            // Implicit widening: integer to real, never the other way round.
            (Ty::Integer, Ty::Real) => true,
            (Ty::Named(a), Ty::Named(b)) => self.is_subtype(a, b),
            _ => false,
        }
    }
}

/// A resolved type. `None` in `Option<Ty>` means the type was unknown and already reported;
/// checks involving it are skipped so one bad declaration does not cause a cascade.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Integer,
    Real,
    Boolean,
    String,
    Code,
    Nothing,
    Null,
    /// `handle` or a user type.
    Named(String),
}

impl Ty {
    fn name(&self) -> String {
        match self {
            Ty::Integer => "integer".into(),
            Ty::Real => "real".into(),
            Ty::Boolean => "boolean".into(),
            Ty::String => "string".into(),
            Ty::Code => "code".into(),
            Ty::Nothing => "nothing".into(),
            Ty::Null => "null".into(),
            Ty::Named(n) => n.clone(),
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(self, Ty::Integer | Ty::Real)
    }
}

fn resolve_type(env: &TypeEnv, name: &str, at: Pos) -> Option<Ty> {
    let key = ident_key(name);
    match key.as_str() {
        "integer" => Some(Ty::Integer),
        "real" => Some(Ty::Real),
        "boolean" => Some(Ty::Boolean),
        "string" => Some(Ty::String),
        "code" => Some(Ty::Code),
        "nothing" => Some(Ty::Nothing),
        _ if env.declared_before(&key, at) => Some(Ty::Named(key)),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct VarInfo {
    ty: Option<Ty>,
    array: bool,
    constant: bool,
    /// Position in the global declaration order (all programs combined).
    order: usize,
}

#[derive(Debug, Clone)]
struct FuncInfo {
    /// Position in the declaration order (shared with globals).
    order: usize,
    params: Vec<Option<Ty>>,
    /// `Some(Ty::Nothing)` for `returns nothing`; `None` if the return type was unknown.
    ret: Option<Ty>,
}

/// Per-function context for expressions and statements.
struct Ctx {
    locals: BTreeMap<String, VarInfo>,
    /// Declarations (globals, natives, functions) with `order >= limit` are not visible yet. A global
    /// initializer sees what is declared above it; a function body sees itself and what is above it
    /// (harness o01, o03, o09, o11, o12, q09: a name used before its declaration is an error).
    limit: usize,
    ret: Option<Ty>,
}

struct Checker {
    env: TypeEnv,
    globals: BTreeMap<String, VarInfo>,
    funcs: BTreeMap<String, FuncInfo>,
    diags: Vec<Diagnostic>,
    multi: bool,
    prog: usize,
    line: u32,
    loop_depth: u32,
}

/// Checks `programs` (in load order) and returns every problem found. An empty result means the
/// programs are well typed under the rules documented in this module.
pub fn check(programs: &[Program]) -> Vec<Diagnostic> {
    let multi = programs.len() > 1;
    let (env, type_diags) = TypeEnv::build(programs);
    let mut c = Checker {
        env,
        globals: BTreeMap::new(),
        funcs: BTreeMap::new(),
        diags: type_diags,
        multi,
        prog: 0,
        line: 0,
        loop_depth: 0,
    };

    // Phase 1: declare every global, native and function signature, in program order. Each one takes
    // the next position, and a name is visible only from its position on (see `Ctx::limit`).
    let mut globals = Vec::new();
    let mut functions = Vec::new();
    let mut order = 0usize;
    for (pi, p) in programs.iter().enumerate() {
        c.prog = pi;
        for g in &p.globals {
            c.line = g.line;
            let info = VarInfo {
                ty: c.var_type(&g.ty, (pi, g.line)),
                array: g.array,
                constant: g.constant,
                order,
            };
            if g.constant && g.init.is_none() {
                c.err(format!("constant '{}' needs an initializer", g.name));
            }
            let key = ident_key(&g.name);
            if c.globals.contains_key(&key) || c.funcs.contains_key(&key) {
                c.err(format!("duplicate global '{}'", g.name));
            } else {
                c.globals.insert(key, info.clone());
            }
            globals.push((pi, g, info));
            order += 1;
        }
        for n in &p.natives {
            c.line = n.line;
            let info = FuncInfo {
                order,
                ..c.func_info(&n.sig, (pi, n.line))
            };
            c.declare_func(&n.sig.name, info);
            order += 1;
        }
        for f in &p.functions {
            c.line = f.line;
            let info = FuncInfo {
                order,
                ..c.func_info(&f.sig, (pi, f.line))
            };
            c.declare_func(&f.sig.name, info.clone());
            functions.push((pi, f, info));
            order += 1;
        }
    }

    // Phase 2: global initializers, in declaration order.
    for (pi, g, info) in &globals {
        c.prog = *pi;
        c.line = g.line;
        let Some(init) = &g.init else { continue };
        if g.array {
            c.err(format!("array '{}' cannot have an initializer", g.name));
        }
        let ctx = Ctx {
            locals: BTreeMap::new(),
            limit: info.order,
            ret: None,
        };
        let t = c.expr(&ctx, init);
        c.require_assignable(&t, &info.ty);
    }

    // Phase 3: function bodies.
    for (pi, f, info) in &functions {
        c.prog = *pi;
        c.line = f.line;
        c.loop_depth = 0;
        let mut ctx = Ctx {
            locals: BTreeMap::new(),
            limit: info.order + 1,
            ret: info.ret.clone(),
        };
        for (p, ty) in f.sig.params.iter().zip(&info.params) {
            let v = VarInfo {
                ty: ty.clone(),
                array: false,
                constant: false,
                order: 0,
            };
            c.add_local(&mut ctx, &p.name, v);
        }
        for l in &f.locals {
            c.line = l.line;
            let ty = c.var_type(&l.ty, END);
            if l.array && l.init.is_some() {
                c.err(format!("array '{}' cannot have an initializer", l.name));
            }
            if let Some(init) = &l.init {
                let t = c.expr(&ctx, init);
                c.require_assignable(&t, &ty);
            }
            let v = VarInfo {
                ty,
                array: l.array,
                constant: false,
                order: 0,
            };
            c.add_local(&mut ctx, &l.name, v);
        }
        c.stmts(&ctx, &f.body);
    }
    c.diags
}

impl Checker {
    fn err(&mut self, msg: impl Into<String>) {
        let message = message(self.multi, self.prog, msg.into());
        self.diags.push(Diagnostic {
            line: self.line,
            message,
        });
    }

    /// Resolves a type used for a variable (global, local or parameter).
    fn var_type(&mut self, name: &str, at: Pos) -> Option<Ty> {
        match resolve_type(&self.env, name, at) {
            Some(Ty::Nothing) => {
                self.err("'nothing' is not a variable type");
                None
            }
            Some(t) => Some(t),
            None => {
                self.err(format!("unknown type '{name}'"));
                None
            }
        }
    }

    fn func_info(&mut self, sig: &Signature, at: Pos) -> FuncInfo {
        let params: Vec<Option<Ty>> = sig
            .params
            .iter()
            .map(|p| self.var_type(&p.ty, at))
            .collect();
        let ret = match &sig.returns {
            None => Some(Ty::Nothing),
            Some(n) => self.var_type(n, at),
        };
        FuncInfo {
            order: 0,
            params,
            ret,
        }
    }

    fn declare_func(&mut self, name: &str, info: FuncInfo) {
        // Functions, natives and globals share one namespace (harness o17, o20, q05).
        let key = ident_key(name);
        if self.funcs.contains_key(&key) || self.globals.contains_key(&key) {
            self.err(format!("duplicate function or native '{name}'"));
        } else {
            self.funcs.insert(key, info);
        }
    }

    /// The function `name` if it is visible from `ctx` (declared above the context, or the function
    /// itself).
    fn visible_func(&self, ctx: &Ctx, name: &str) -> Option<FuncInfo> {
        self.funcs
            .get(&ident_key(name))
            .filter(|f| f.order < ctx.limit)
            .cloned()
    }

    fn add_local(&mut self, ctx: &mut Ctx, name: &str, v: VarInfo) {
        let key = ident_key(name);
        // A local or parameter may shadow a global (harness q08, r04). It may not take the name of a
        // function that is already declared, the function being checked included (harness q02, q03,
        // q12, r03). It may take the name of a later function (harness q01).
        if self.funcs.get(&key).is_some_and(|f| f.order < ctx.limit) {
            self.err(format!("'{name}' is already the name of a function"));
        }
        // A repeated local or parameter replaces the earlier one, so the last declaration sets the
        // type (harness p01, p06, p03, r01, r02).
        ctx.locals.insert(key, v);
    }

    fn var(&mut self, ctx: &Ctx, name: &str) -> Option<VarInfo> {
        let key = ident_key(name);
        let found = ctx.locals.get(&key).cloned().or_else(|| {
            self.globals
                .get(&key)
                .filter(|v| v.order < ctx.limit)
                .cloned()
        });
        if found.is_none() {
            self.err(format!("undefined variable '{name}'"));
        }
        found
    }

    fn require_assignable(&mut self, from: &Option<Ty>, to: &Option<Ty>) {
        if let (Some(f), Some(t)) = (from, to) {
            if !self.env.assignable(f, t) {
                let msg = format!("cannot convert {} to {}", f.name(), t.name());
                self.err(msg);
            }
        }
    }

    fn expect_bool(&mut self, t: &Option<Ty>, what: &str) {
        if let Some(t) = t {
            if *t != Ty::Boolean {
                let msg = format!("{what}, found {}", t.name());
                self.err(msg);
            }
        }
    }

    fn expect_index(&mut self, t: &Option<Ty>) {
        if let Some(t) = t {
            if *t != Ty::Integer {
                let msg = format!("array index must be integer, found {}", t.name());
                self.err(msg);
            }
        }
    }

    fn expr(&mut self, ctx: &Ctx, e: &Expr) -> Option<Ty> {
        match e {
            Expr::Int(_) => Some(Ty::Integer),
            Expr::Real(_) => Some(Ty::Real),
            Expr::Str(_) => Some(Ty::String),
            Expr::Bool(_) => Some(Ty::Boolean),
            Expr::Null => Some(Ty::Null),
            Expr::Var(name) => {
                let v = self.var(ctx, name)?;
                if v.array {
                    self.err(format!("array '{name}' used without an index"));
                    return None;
                }
                v.ty
            }
            Expr::Index(name, idx) => {
                let it = self.expr(ctx, idx);
                let v = self.var(ctx, name)?;
                if !v.array {
                    self.err(format!("'{name}' is not an array"));
                    return None;
                }
                self.expect_index(&it);
                v.ty
            }
            Expr::Call(name, args) => match self.call(ctx, name, args) {
                Some(Ty::Nothing) => {
                    self.err(format!("'{name}' returns nothing and has no value"));
                    None
                }
                t => t,
            },
            Expr::FuncRef(name) => match self.visible_func(ctx, name).map(|f| f.params.len()) {
                None => {
                    self.err(format!("undefined function '{name}'"));
                    None
                }
                // UNVERIFIED vs retail: natives may be referenced with `function`, and the referenced
                // function's return type is not restricted.
                Some(n) if n > 0 => {
                    self.err(format!(
                        "function '{name}' takes parameters and cannot be used as code"
                    ));
                    None
                }
                Some(_) => Some(Ty::Code),
            },
            Expr::Neg(inner) => match self.expr(ctx, inner) {
                Some(t @ (Ty::Integer | Ty::Real)) => Some(t),
                None => None,
                Some(t) => {
                    self.err(format!("cannot negate {}", t.name()));
                    None
                }
            },
            Expr::Not(inner) => {
                let t = self.expr(ctx, inner);
                self.expect_bool(&t, "'not' needs a boolean");
                Some(Ty::Boolean)
            }
            Expr::Binary(op, l, r) => {
                let lt = self.expr(ctx, l);
                let rt = self.expr(ctx, r);
                self.binary(*op, &lt, &rt)
            }
        }
    }

    fn binary(&mut self, op: BinOp, lt: &Option<Ty>, rt: &Option<Ty>) -> Option<Ty> {
        let (Some(a), Some(b)) = (lt.as_ref(), rt.as_ref()) else {
            // An operand already failed. Keep the result type of the operator so that
            // the enclosing expression does not report a second error.
            return match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => None,
                _ => Some(Ty::Boolean),
            };
        };
        let numeric = a.is_numeric() && b.is_numeric();
        // Integer op integer stays integer; any real operand promotes the result to real.
        let promoted = if *a == Ty::Integer && *b == Ty::Integer {
            Ty::Integer
        } else {
            Ty::Real
        };
        match op {
            BinOp::And | BinOp::Or => {
                if *a != Ty::Boolean || *b != Ty::Boolean {
                    let msg = format!(
                        "'and'/'or' need boolean operands, found {} and {}",
                        a.name(),
                        b.name()
                    );
                    self.err(msg);
                }
                Some(Ty::Boolean)
            }
            BinOp::Eq | BinOp::Ne => {
                // Compatible means assignable in at least one direction (so `null` and
                // integer/real mixes work, unrelated handle types do not).
                if !(self.env.assignable(a, b) || self.env.assignable(b, a)) {
                    let msg = format!("cannot compare {} and {}", a.name(), b.name());
                    self.err(msg);
                }
                Some(Ty::Boolean)
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if !numeric {
                    let msg = format!(
                        "ordering needs numbers, found {} and {}",
                        a.name(),
                        b.name()
                    );
                    self.err(msg);
                }
                Some(Ty::Boolean)
            }
            BinOp::Add => {
                if *a == Ty::String && *b == Ty::String {
                    // UNVERIFIED vs retail: `string + integer` (or any non-string operand) is rejected.
                    Some(Ty::String)
                } else if numeric {
                    Some(promoted)
                } else {
                    let msg = format!("invalid operands to '+': {} and {}", a.name(), b.name());
                    self.err(msg);
                    None
                }
            }
            BinOp::Sub | BinOp::Mul | BinOp::Div => {
                if numeric {
                    Some(promoted)
                } else {
                    let msg = format!(
                        "arithmetic needs numbers, found {} and {}",
                        a.name(),
                        b.name()
                    );
                    self.err(msg);
                    None
                }
            }
        }
    }

    /// Checks a call. Returns the callee's return type (`Some(Ty::Nothing)` for `returns nothing`),
    /// so the caller decides whether a value is required.
    fn call(&mut self, ctx: &Ctx, name: &str, args: &[Expr]) -> Option<Ty> {
        let arg_tys: Vec<Option<Ty>> = args.iter().map(|a| self.expr(ctx, a)).collect();
        let Some(f) = self.visible_func(ctx, name) else {
            self.err(format!("undefined function '{name}'"));
            return None;
        };
        if f.params.len() != args.len() {
            let msg = format!(
                "'{name}' takes {} argument(s), got {}",
                f.params.len(),
                args.len()
            );
            self.err(msg);
        } else {
            for (i, (p, a)) in f.params.iter().zip(&arg_tys).enumerate() {
                if let (Some(p), Some(a)) = (p, a) {
                    if !self.env.assignable(a, p) {
                        let msg = format!(
                            "argument {} of '{name}': cannot convert {} to {}",
                            i + 1,
                            a.name(),
                            p.name()
                        );
                        self.err(msg);
                    }
                }
            }
        }
        f.ret
    }

    fn stmts(&mut self, ctx: &Ctx, body: &[Stmt]) {
        for s in body {
            self.stmt(ctx, s);
        }
    }

    fn stmt(&mut self, ctx: &Ctx, s: &Stmt) {
        match s {
            Stmt::Set {
                name,
                index,
                value,
                line,
            } => {
                self.line = *line;
                let it = index.as_ref().map(|i| self.expr(ctx, i));
                let vt = self.expr(ctx, value);
                let Some(v) = self.var(ctx, name) else { return };
                if v.constant {
                    self.err(format!("cannot assign to constant '{name}'"));
                    return;
                }
                match (index.is_some(), v.array) {
                    (false, true) => {
                        self.err(format!("array '{name}' assigned without an index"));
                        return;
                    }
                    (true, false) => {
                        self.err(format!("'{name}' is not an array"));
                        return;
                    }
                    _ => {}
                }
                if let Some(it) = &it {
                    self.expect_index(it);
                }
                self.require_assignable(&vt, &v.ty);
            }
            Stmt::Call { name, args, line } => {
                self.line = *line;
                // The result of a call used as a statement is discarded, including for
                // `returns nothing` and for values.
                self.call(ctx, name, args);
            }
            Stmt::If {
                branches,
                otherwise,
                line,
            } => {
                self.line = *line;
                for (cond, body) in branches {
                    let t = self.expr(ctx, cond);
                    self.expect_bool(&t, "'if'/'elseif' condition needs a boolean");
                    self.stmts(ctx, body);
                }
                self.stmts(ctx, otherwise);
            }
            Stmt::Loop { body, line } => {
                self.line = *line;
                self.loop_depth += 1;
                self.stmts(ctx, body);
                self.loop_depth -= 1;
            }
            Stmt::ExitWhen { cond, line } => {
                self.line = *line;
                if self.loop_depth == 0 {
                    self.err("'exitwhen' outside of a loop");
                }
                let t = self.expr(ctx, cond);
                self.expect_bool(&t, "'exitwhen' condition needs a boolean");
            }
            Stmt::Return { value, line } => {
                self.line = *line;
                let vt: Option<Option<Ty>> = value.as_ref().map(|v| self.expr(ctx, v));
                let Some(rt) = &ctx.ret else { return };
                match (rt, &vt) {
                    (Ty::Nothing, Some(_)) => {
                        self.err("'return' with a value in a function that returns nothing");
                    }
                    (Ty::Nothing, None) => {}
                    (rt, None) => {
                        let msg = format!("'return' without a value, expected {}", rt.name());
                        self.err(msg);
                    }
                    (rt, Some(t)) => self.require_assignable(t, &Some(rt.clone())),
                }
            }
            // UNVERIFIED vs retail: `debug` statements are type checked even though release builds skip them.
            Stmt::Debug(inner) => self.stmt(ctx, inner),
        }
    }
}
