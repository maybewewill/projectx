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

use std::collections::{BTreeMap, BTreeSet};

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

/// The set of known type names and the `extends` relation between them.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// Every known type. Builtins map to `None`; user types map to their declared parent.
    /// After `build` every user type's parent chain ends at `handle`.
    parents: BTreeMap<String, Option<String>>,
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
        Self { parents }
    }

    /// Declares the types of all `programs` (in order) and repairs bad declarations so that later
    /// checks see a consistent hierarchy. The returned diagnostics describe each repair.
    ///
    /// Repairs: a duplicate name is ignored (the first declaration wins); an unknown parent, a cyclic
    /// parent, or a builtin parent other than `handle` is replaced with `handle`.
    pub fn build(programs: &[Program]) -> (TypeEnv, Vec<Diagnostic>) {
        let multi = programs.len() > 1;
        let mut env = TypeEnv::new();
        let mut diags = Vec::new();
        // User types in declaration order, with the program index and line they were declared at.
        let mut decls: Vec<(String, usize, u32)> = Vec::new();

        // Pass 1: names. Declaration order does not matter, so `type a extends b` may precede `b`.
        for (pi, p) in programs.iter().enumerate() {
            for t in &p.types {
                if env.parents.contains_key(&t.name) {
                    let msg = format!("duplicate type '{}'", t.name);
                    diags.push(Diagnostic {
                        line: t.line,
                        message: message(multi, pi, msg),
                    });
                } else {
                    env.parents.insert(t.name.clone(), Some(t.parent.clone()));
                    decls.push((t.name.clone(), pi, t.line));
                }
            }
        }

        // Pass 2: every parent must be a known type.
        for (name, pi, line) in &decls {
            let parent = env.parents[name].clone().unwrap_or_default();
            if !env.parents.contains_key(&parent) {
                let msg = format!("type '{name}' extends unknown type '{parent}'");
                diags.push(Diagnostic {
                    line: *line,
                    message: message(multi, *pi, msg),
                });
                env.parents.insert(name.clone(), Some("handle".to_string()));
            }
        }

        // Pass 3: no cycles. Walk up from each type; a walk that returns to its start is a cycle.
        // The `seen` set stops walks that run into a cycle not containing the start.
        for (name, pi, line) in &decls {
            let mut seen = BTreeSet::new();
            let mut cur = name.as_str();
            let mut cyclic = false;
            while let Some(Some(parent)) = env.parents.get(cur) {
                if parent == name {
                    cyclic = true;
                    break;
                }
                if !seen.insert(parent.as_str()) {
                    break;
                }
                cur = parent.as_str();
            }
            if cyclic {
                let msg = format!("cyclic type declaration '{name}'");
                diags.push(Diagnostic {
                    line: *line,
                    message: message(multi, *pi, msg),
                });
                env.parents.insert(name.clone(), Some("handle".to_string()));
            }
        }

        // Pass 4: a user type must derive from `handle`.
        // UNVERIFIED vs retail: that user types may only extend handle-derived types (e.g. `extends integer` is rejected).
        for (name, pi, line) in &decls {
            let parent = env.parents[name].clone().unwrap_or_default();
            if parent != "handle" && VALUE_TYPES.contains(&parent.as_str()) {
                let msg = format!("type '{name}' must extend a handle type, not '{parent}'");
                diags.push(Diagnostic {
                    line: *line,
                    message: message(multi, *pi, msg),
                });
                env.parents.insert(name.clone(), Some("handle".to_string()));
            }
        }
        (env, diags)
    }

    /// Whether the type `name` is known (builtin or user-declared).
    pub fn is_known(&self, name: &str) -> bool {
        self.parents.contains_key(name)
    }

    /// `a` is `b`, or `a` reaches `b` by following `extends`. `integer` is not a subtype of `real`;
    /// that widening is handled by assignability.
    pub fn is_subtype(&self, a: &str, b: &str) -> bool {
        let mut cur = a;
        for _ in 0..=self.parents.len() {
            if cur == b {
                return true;
            }
            match self.parents.get(cur) {
                Some(Some(parent)) => cur = parent.as_str(),
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

fn resolve_type(env: &TypeEnv, name: &str) -> Option<Ty> {
    match name {
        "integer" => Some(Ty::Integer),
        "real" => Some(Ty::Real),
        "boolean" => Some(Ty::Boolean),
        "string" => Some(Ty::String),
        "code" => Some(Ty::Code),
        "nothing" => Some(Ty::Nothing),
        _ if env.is_known(name) => Some(Ty::Named(name.to_string())),
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
    params: Vec<Option<Ty>>,
    /// `Some(Ty::Nothing)` for `returns nothing`; `None` if the return type was unknown.
    ret: Option<Ty>,
}

/// Per-function context for expressions and statements.
struct Ctx {
    locals: BTreeMap<String, VarInfo>,
    /// Globals with `order >= global_limit` are not visible. Used for global initializers.
    // UNVERIFIED vs retail: a global initializer may refer only to globals declared before it.
    global_limit: usize,
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

    // Phase 1: declare every global, native and function signature, in program order.
    // UNVERIFIED vs retail: functions and natives are visible everywhere, including before their
    // declaration and from other programs (no forward declarations required).
    let mut globals = Vec::new();
    let mut functions = Vec::new();
    let mut order = 0usize;
    for (pi, p) in programs.iter().enumerate() {
        c.prog = pi;
        for g in &p.globals {
            c.line = g.line;
            let info = VarInfo {
                ty: c.var_type(&g.ty),
                array: g.array,
                constant: g.constant,
                order,
            };
            if g.constant && g.init.is_none() {
                c.err(format!("constant '{}' needs an initializer", g.name));
            }
            if c.globals.contains_key(&g.name) {
                c.err(format!("duplicate global '{}'", g.name));
            } else {
                c.globals.insert(g.name.clone(), info.clone());
            }
            globals.push((pi, g, info));
            order += 1;
        }
        for n in &p.natives {
            c.line = n.line;
            let info = c.func_info(&n.sig);
            c.declare_func(&n.sig.name, info);
        }
        for f in &p.functions {
            c.line = f.line;
            let info = c.func_info(&f.sig);
            c.declare_func(&f.sig.name, info.clone());
            functions.push((pi, f, info));
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
            global_limit: info.order,
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
            global_limit: usize::MAX,
            ret: info.ret.clone(),
        };
        for (p, ty) in f.sig.params.iter().zip(&info.params) {
            let v = VarInfo {
                ty: ty.clone(),
                array: false,
                constant: false,
                order: 0,
            };
            c.add_local(&mut ctx, &p.name, v, "parameter");
        }
        for l in &f.locals {
            c.line = l.line;
            let ty = c.var_type(&l.ty);
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
            c.add_local(&mut ctx, &l.name, v, "local");
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
    fn var_type(&mut self, name: &str) -> Option<Ty> {
        match resolve_type(&self.env, name) {
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

    fn func_info(&mut self, sig: &Signature) -> FuncInfo {
        let params: Vec<Option<Ty>> = sig.params.iter().map(|p| self.var_type(&p.ty)).collect();
        let ret = match &sig.returns {
            None => Some(Ty::Nothing),
            Some(n) => self.var_type(n),
        };
        FuncInfo { params, ret }
    }

    fn declare_func(&mut self, name: &str, info: FuncInfo) {
        // Functions and natives share one namespace.
        if self.funcs.contains_key(name) {
            self.err(format!("duplicate function or native '{name}'"));
        } else {
            self.funcs.insert(name.to_string(), info);
        }
    }

    fn add_local(&mut self, ctx: &mut Ctx, name: &str, v: VarInfo, what: &str) {
        // UNVERIFIED vs retail: a local or parameter may shadow a global of the same name.
        if ctx.locals.contains_key(name) {
            self.err(format!("duplicate {what} '{name}'"));
        } else {
            ctx.locals.insert(name.to_string(), v);
        }
    }

    fn var(&mut self, ctx: &Ctx, name: &str) -> Option<VarInfo> {
        let found = ctx.locals.get(name).cloned().or_else(|| {
            self.globals
                .get(name)
                .filter(|v| v.order < ctx.global_limit)
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
            Expr::FuncRef(name) => match self.funcs.get(name).map(|f| f.params.len()) {
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
        let Some(f) = self.funcs.get(name).cloned() else {
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
