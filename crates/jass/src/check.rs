//! Semantic analysis (type checking) for JASS programs.
//!
//! The rules here are the ones decoded from the original compiler of Game.dll 1.26a (entry
//! `0x6F458900`, the path the game uses for `common.j` + `Blizzard.j` + `war3map.j` and for
//! `common.j` + `common.ai` + a race script) and checked against it in Unicorn on ~2,500 probe
//! programs; see "Real compile path" in `docs/spec/jass-frontend.md`. The probe corpus is replayed by
//! `tests/original_probes.rs`.
//!
//! Several programs (e.g. common.j, blizzard.j, war3map.j) are checked as one combined namespace,
//! in the order given. Problems are collected, not raised at the first one, and the diagnostics come out
//! in a fixed order (types, declarations, global initializers, function bodies), so the output is
//! deterministic. When more than one program is checked, each message is prefixed with
//! `program N:` (1-based position in the slice) because `Diagnostic` only carries a line.
//! Like the original, a script set with any diagnostic must be rejected as a whole.
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

/// Options of the checker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Use the original's lenient mode for `return` analysis (the flag the game passes as `p5 = 0`
    /// for a hard-coded list of 23 known map hashes at `0x6F9426B8`). Default `false` = strict, which
    /// the game uses for every other map. The strict mode is verified exactly (1,400 random
    /// programs); the lenient mode matched 600 random programs too, but is used by no shipped map.
    pub lenient_returns: bool,
    /// The natives the game registered before the compile. When set, every `native` declaration must
    /// name a registered native (exact spelling) with an identical signature descriptor, like the
    /// original (`native` with an unknown name or a different signature is an error). `None` accepts any
    /// declaration.
    pub natives: Option<NativeRegistry>,
}

/// Names and signature descriptors of the natives the game registers (`(Hunit;I)B` style, see
/// `docs/spec/natives-1.26a.tsv`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeRegistry {
    sigs: BTreeMap<String, String>,
}

impl NativeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one native.
    pub fn insert(&mut self, name: &str, descriptor: &str) {
        self.sigs.insert(name.to_string(), descriptor.to_string());
    }

    /// Reads `name<TAB>descriptor<TAB>impl<TAB>table` lines (`#` starts a comment) and keeps the rows
    /// whose table column is one of `tables` (`common`, `ai`).
    pub fn from_tsv(text: &str, tables: &[&str]) -> Self {
        let mut r = Self::new();
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() >= 4 && tables.contains(&cols[3]) {
                r.insert(cols[0], cols[1]);
            }
        }
        r
    }

    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sigs.is_empty()
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.sigs.get(name).map(String::as_str)
    }
}

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
    /// types declared above it (the original's error 33 for every bad `extends`: unknown name,
    /// forward reference, itself, or a builtin other than `handle`); a bad declaration is repaired so
    /// that later checks see a consistent hierarchy, and each repair is reported.
    ///
    /// Repairs: a duplicate name (a builtin's name or an earlier type's, exact spelling) is ignored
    /// (the first declaration wins); an unknown parent, or a builtin parent other than `handle`, is
    /// replaced with `handle`.
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

    /// Assignment compatibility (`set`, initializers): equal types, `null` into string / code /
    /// handle types, integer into real, a handle type into one of its ancestors.
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

    /// The signature letter used by the native registry.
    fn descriptor(&self) -> String {
        match self {
            Ty::Integer => "I".into(),
            Ty::Real => "R".into(),
            Ty::Boolean => "B".into(),
            Ty::String => "S".into(),
            Ty::Code => "C".into(),
            Ty::Nothing | Ty::Null => "V".into(),
            Ty::Named(n) => format!("H{n};"),
        }
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

// ---- return analysis -------------------------------------------------------------------------
//
// The original does not check `return` statements one by one. Every statement node carries a
// triple (flag, type code, type reference) describing the value returns it contains, and the
// parser merges those triples while it builds the statement lists, `if` nodes and the function
// (nodes `0x6F4614D0` list, `0x6F4617C0` if/else, `0x6F461E30` return, `0x6F461C30` loop,
// `0x6F461D30` exitwhen, validators `0x6F461720`, `0x6F461B80`, `0x6F4611D0`; function setter
// `0x6F461260`). The merge is an adjacency fold, not a set union, so the result depends on the
// order of the statements; that is why a lone `return null` in an `integer` function is an error
// while `if c then return null endif ... return 1` is not (the retail map `(6)BomberCommand` has that).
// Type codes: 2 null, 3 code, 4 integer, 5 real, 6 string, 7 handle (with a reference), 8 boolean.

const T_NULL: u8 = 2;
const T_CODE: u8 = 3;
const T_INT: u8 = 4;
const T_REAL: u8 = 5;
const T_STRING: u8 = 6;
const T_HANDLE: u8 = 7;
const T_BOOL: u8 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Info {
    /// 0 no return, 1 `return`, 2 `return value`.
    flag: u8,
    ty: u8,
    rf: Option<String>,
}

impl Info {
    const NONE: Info = Info {
        flag: 0,
        ty: 0,
        rf: None,
    };
}

/// A statement node as the fold sees it. `exit` marks the `exitwhen` node (kind 48).
#[derive(Debug, Clone)]
struct RStmt {
    exit: bool,
    info: Info,
}

/// A statement-list node: its own aggregate and the last statement (as modified in place).
#[derive(Debug, Clone)]
struct RList {
    info: Info,
    last: RStmt,
    conflict: bool,
}

/// Appends statement `s` to the list `prev` (the list constructor `0x6F4614D0`).
fn list_push(prev: Option<&RList>, mut s: RStmt, strict: bool) -> RList {
    let p = prev.map(|l| &l.last);
    let mut out = Info::NONE;
    let mut conflict = false;
    if !strict {
        out.ty = s.info.ty;
        out.rf = s.info.rf.clone();
        match p {
            None => out.flag = s.info.flag,
            Some(p) if !p.exit => out.flag = s.info.flag,
            _ => {}
        }
        return RList {
            info: out,
            last: s,
            conflict: false,
        };
    }
    if prev.is_none() || p.is_some_and(|p| !p.exit) {
        out.flag = s.info.flag;
    }
    let Some(p) = p else {
        out.ty = s.info.ty;
        out.rf = s.info.rf.clone();
        return RList {
            info: out,
            last: s,
            conflict,
        };
    };
    if p.info.flag == 0 && s.info.flag == 0 {
        out.ty = s.info.ty;
        out.rf = s.info.rf.clone();
        return RList {
            info: out,
            last: s,
            conflict,
        };
    }
    let (pf, sf) = (p.info.flag, s.info.flag);
    let mut v7 = false;
    if sf != pf {
        if sf != 0 {
            if pf != 0 {
                conflict = true;
            } else {
                out.flag = sf;
                out.ty = s.info.ty;
                out.rf = s.info.rf.clone();
                v7 = true;
            }
        } else {
            out.flag = pf;
            out.ty = p.info.ty;
            out.rf = p.info.rf.clone();
            // The statement itself takes over the previous statement's return information.
            s.info = out.clone();
            v7 = true;
        }
    }
    if p.info.flag != 2 || s.info.flag != 2 {
        if !v7 {
            out.ty = s.info.ty;
            out.rf = s.info.rf.clone();
        }
        return RList {
            info: out,
            last: s,
            conflict,
        };
    }
    let (ts, tp) = (s.info.ty, p.info.ty);
    if ts == T_NULL {
        if tp != T_NULL {
            out.ty = tp;
            out.rf = p.info.rf.clone();
            return RList {
                info: out,
                last: s,
                conflict,
            };
        }
    } else if tp == T_NULL {
        out.ty = ts;
        out.rf = s.info.rf.clone();
        return RList {
            info: out,
            last: s,
            conflict,
        };
    } else if tp != ts {
        if (ts == T_INT && tp == T_REAL) || (ts == T_REAL && tp == T_INT) {
            out.ty = ts;
            out.rf = s.info.rf.clone();
        } else {
            conflict = true;
        }
        return RList {
            info: out,
            last: s,
            conflict,
        };
    }
    if ts == T_HANDLE && s.info.rf != p.info.rf {
        conflict = true;
        return RList {
            info: out,
            last: s,
            conflict,
        };
    }
    out.ty = s.info.ty;
    out.rf = s.info.rf.clone();
    RList {
        info: out,
        last: s,
        conflict,
    }
}

/// The `if` / `elseif` / `else` node (`0x6F4617C0`) plus its validator (`0x6F461B80`). Returns the
/// node's aggregate and whether the validator reports a conflict.
fn if_node(then: Option<Info>, other: Option<Info>, strict: bool) -> (Info, bool) {
    let mut a4 = then;
    let mut a5 = other;
    let mut out = Info::NONE;
    if strict {
        match (&mut a4, &mut a5) {
            (Some(x), Some(y)) => {
                if x.ty == T_NULL && matches!(y.ty, T_HANDLE | T_STRING | T_CODE) {
                    x.ty = y.ty;
                    x.rf = y.rf.clone();
                    x.flag = y.flag;
                }
                if y.ty == T_NULL && matches!(x.ty, T_HANDLE | T_STRING | T_CODE) {
                    y.ty = x.ty;
                    y.rf = x.rf.clone();
                    y.flag = x.flag;
                }
                let (f4, f5) = (x.flag, y.flag);
                if f4 == f5 || f4 != 0 {
                    out.flag = f4;
                } else {
                    out.flag = f5;
                    out.ty = y.ty;
                    out.rf = y.rf.clone();
                    return finish_if(out, a4, a5, true);
                }
                if x.flag == 2 {
                    out.ty = x.ty;
                    out.rf = x.rf.clone();
                }
            }
            (Some(x), None) => {
                out.flag = x.flag;
                if x.flag == 2 {
                    out.ty = x.ty;
                    out.rf = x.rf.clone();
                }
            }
            (None, Some(y)) => {
                out.flag = y.flag;
                if y.flag == 2 {
                    out.ty = y.ty;
                    out.rf = y.rf.clone();
                }
            }
            (None, None) => {}
        }
        return finish_if(out, a4, a5, true);
    }
    // Lenient mode.
    match (&mut a4, &mut a5) {
        (None, Some(y)) => {
            if y.flag == 2 {
                out.ty = y.ty;
                out.rf = y.rf.clone();
            } else {
                y.ty = 0;
                y.rf = None;
            }
        }
        (None, None) => {}
        (Some(x), y) => {
            if let Some(y) = y.as_mut() {
                if x.ty == T_NULL && matches!(y.ty, T_HANDLE | T_STRING | T_CODE) {
                    x.ty = y.ty;
                    x.rf = y.rf.clone();
                }
                if y.ty == T_NULL && matches!(x.ty, T_HANDLE | T_STRING | T_CODE) {
                    y.ty = x.ty;
                    y.rf = x.rf.clone();
                }
            }
            if x.flag == 2 {
                out.ty = x.ty;
                out.rf = x.rf.clone();
            } else {
                x.ty = 0;
                x.rf = None;
            }
            if let Some(y) = y.as_mut() {
                if x.flag == y.flag {
                    out.flag = x.flag;
                }
                if y.flag == 2 {
                    out.ty = y.ty;
                    out.rf = y.rf.clone();
                } else {
                    y.ty = 0;
                    y.rf = None;
                }
            }
        }
    }
    finish_if(out, a4, a5, false)
}

fn finish_if(out: Info, a4: Option<Info>, a5: Option<Info>, strict: bool) -> (Info, bool) {
    // Integer / real branches are reconciled by a conversion node and never conflict.
    if let (Some(x), Some(y)) = (&a4, &a5) {
        if matches!((x.ty, y.ty), (T_INT, T_REAL) | (T_REAL, T_INT)) {
            return (out, false);
        }
    }
    let (t4, f4) = a4.as_ref().map_or((0, 0), |i| (i.ty, i.flag));
    let (t5, f5) = a5.as_ref().map_or((0, 0), |i| (i.ty, i.flag));
    let bad = if strict {
        t4 != 0 && f4 == 2 && t5 != 0 && f5 == 2 && t5 != t4
    } else {
        t4 != 0 && t5 != 0 && t5 != t4
    };
    (out, bad)
}

/// Per-function context for expressions and statements.
struct Ctx {
    locals: BTreeMap<String, VarInfo>,
    /// Declarations (globals, natives, functions) with `order >= limit` are not visible yet. A global
    /// initializer sees what is declared above it; a function body sees itself and what is above it
    /// (a name used before its declaration is an error).
    limit: usize,
    ret: Option<Ty>,
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
    /// The name as written.
    name: String,
}

struct Checker {
    env: TypeEnv,
    globals: BTreeMap<String, VarInfo>,
    funcs: BTreeMap<String, FuncInfo>,
    diags: Vec<Diagnostic>,
    multi: bool,
    prog: usize,
    line: u32,
    strict: bool,
}

/// Checks `programs` (in load order) and returns every problem found. An empty result means the
/// programs are well typed under the rules documented in this module.
pub fn check(programs: &[Program]) -> Vec<Diagnostic> {
    check_with(programs, &CheckOptions::default())
}

/// Like [`check`], with options.
pub fn check_with(programs: &[Program], opts: &CheckOptions) -> Vec<Diagnostic> {
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
        strict: !opts.lenient_returns,
    };

    // Phase 1: declare every global, native and function signature, in program order. Each one takes
    // the next position, and a name is visible only from its position on (see `Ctx::limit`).
    let mut globals = Vec::new();
    let mut functions = Vec::new();
    let mut order = 0usize;
    for (pi, p) in programs.iter().enumerate() {
        c.prog = pi;
        // Number declarations in source order: common.j declares natives above the globals that
        // call them (e.g. `ConvertPlayerColor` before `PLAYER_COLOR_RED`).
        enum Decl<'a> {
            Global(&'a Global),
            Native(&'a Native),
            Function(&'a Function),
        }
        let mut decls: Vec<(u32, Decl)> = Vec::new();
        decls.extend(p.globals.iter().map(|g| (g.line, Decl::Global(g))));
        decls.extend(p.natives.iter().map(|n| (n.line, Decl::Native(n))));
        decls.extend(p.functions.iter().map(|f| (f.line, Decl::Function(f))));
        decls.sort_by_key(|d| d.0); // stable: same-line ties keep the old kind order
        for (_, d) in decls {
            match d {
                Decl::Global(g) => {
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
                    if g.constant && g.array {
                        c.err(format!("constant array '{}'", g.name));
                    }
                    let key = ident_key(&g.name);
                    if c.name_taken(&key) {
                        c.err(format!("duplicate global '{}'", g.name));
                    } else {
                        c.globals.insert(key, info.clone());
                    }
                    globals.push((pi, g, info));
                }
                Decl::Native(n) => {
                    c.line = n.line;
                    let info = FuncInfo {
                        order,
                        ..c.func_info(&n.sig, (pi, n.line))
                    };
                    if let Some(reg) = &opts.natives {
                        c.check_native(reg, &n.sig, &info);
                    }
                    c.declare_func(&n.sig.name, info);
                }
                Decl::Function(f) => {
                    c.line = f.line;
                    let info = FuncInfo {
                        order,
                        ..c.func_info(&f.sig, (pi, f.line))
                    };
                    c.declare_func(&f.sig.name, info.clone());
                    functions.push((pi, f, info));
                }
            }
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
            c.add_local(&mut ctx, &p.name, v, true);
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
            c.add_local(&mut ctx, &l.name, v, false);
        }
        let list = c.stmts(&ctx, &f.body, 0);
        c.line = f.line;
        c.function_returns(info, list.as_ref());
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

    /// Whether `key` is already the name of a type, a global variable, a function or a native.
    /// The original keeps one symbol table for them and rejects a second declaration of any kind.
    fn name_taken(&self, key: &str) -> bool {
        self.env.is_known(key) || self.globals.contains_key(key) || self.funcs.contains_key(key)
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
            name: sig.name.clone(),
        }
    }

    /// A native has to be registered under exactly this name with exactly this signature.
    fn check_native(&mut self, reg: &NativeRegistry, sig: &Signature, info: &FuncInfo) {
        let Some(want) = reg.get(&sig.name) else {
            self.err(format!("native '{}' is not registered", sig.name));
            return;
        };
        let mut got = String::from("(");
        for p in &info.params {
            match p {
                Some(t) => got.push_str(&t.descriptor()),
                None => return,
            }
        }
        got.push(')');
        match &info.ret {
            Some(t) => got.push_str(&t.descriptor()),
            None => return,
        }
        // The comparison stops after the return letter: the game registers four natives with a stray
        // `;` after it (`()R;` for GetTournamentFinishSoonTimeRemaining, `(Hhashtable;II)I;` for LoadInteger,
        // ...), and the retail `common.j` declares them without it.
        if !want.starts_with(&got) {
            self.err(format!(
                "native '{}' is declared as {got} but registered as {want}",
                sig.name
            ));
        }
    }

    /// Functions, natives, globals and types share one namespace; a second declaration of the same
    /// (case-sensitive) name is an error whatever the kinds are.
    fn declare_func(&mut self, name: &str, info: FuncInfo) {
        let key = ident_key(name);
        if self.name_taken(&key) {
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

    /// Declares a parameter or local. Both may shadow a global and may repeat (the last declaration
    /// wins). Neither may take the name of a type or of a function that is already declared; a local
    /// may not take the name of the function being defined either, a parameter may (the function's
    /// own symbol is entered after its parameter list).
    fn add_local(&mut self, ctx: &mut Ctx, name: &str, v: VarInfo, param: bool) {
        let key = ident_key(name);
        let limit = if param { ctx.limit - 1 } else { ctx.limit };
        if self.env.is_known(&key) {
            self.err(format!("'{name}' is the name of a type"));
        } else if self.funcs.get(&key).is_some_and(|f| f.order < limit) {
            self.err(format!("'{name}' is already the name of a function"));
        }
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
            Expr::FuncRef(name) => match self.visible_func(ctx, name) {
                None => {
                    self.err(format!("undefined function '{name}'"));
                    None
                }
                // Any declared function or native may be referenced, whatever its signature (the
                // original only types the reference as `code`).
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
                // `null` compares with anything; numbers with numbers; any two handle types with each
                // other (unrelated ones too); everything else needs the same type.
                let handles = matches!((a, b), (Ty::Named(x), Ty::Named(y))
                    if self.env.is_handle_derived(x) && self.env.is_handle_derived(y));
                if !(*a == Ty::Null || *b == Ty::Null || numeric || handles || a == b) {
                    let msg = format!("cannot compare {} and {}", a.name(), b.name());
                    self.err(msg);
                }
                Some(Ty::Boolean)
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                // Numbers only; `null` is accepted next to a number.
                let with_null =
                    (*a == Ty::Null && b.is_numeric()) || (a.is_numeric() && *b == Ty::Null);
                if !(numeric || with_null) {
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
                    // An argument `null` is accepted for any parameter type (unlike `set`).
                    if *a != Ty::Null && !self.env.assignable(a, p) {
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

    /// The type code and reference of an expression type for the return analysis.
    fn rcode(t: &Option<Ty>) -> Option<(u8, Option<String>)> {
        Some(match t.as_ref()? {
            Ty::Integer => (T_INT, None),
            Ty::Real => (T_REAL, None),
            Ty::String => (T_STRING, None),
            Ty::Boolean => (T_BOOL, None),
            Ty::Code => (T_CODE, None),
            Ty::Null => (T_NULL, None),
            Ty::Nothing => (0, None),
            Ty::Named(n) => (T_HANDLE, Some(n.clone())),
        })
    }

    /// Checks the statements of a block and returns the list node of the return analysis.
    fn stmts(&mut self, ctx: &Ctx, body: &[Stmt], loop_depth: u32) -> Option<RList> {
        let mut list: Option<RList> = None;
        for s in body {
            let node = self.stmt(ctx, s, loop_depth);
            let l = list_push(list.as_ref(), node, self.strict);
            if l.conflict {
                self.err("conflicting return types in one block");
            }
            list = Some(l);
        }
        list
    }

    fn plain() -> RStmt {
        RStmt {
            exit: false,
            info: Info::NONE,
        }
    }

    fn stmt(&mut self, ctx: &Ctx, s: &Stmt, loop_depth: u32) -> RStmt {
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
                let Some(v) = self.var(ctx, name) else {
                    return Self::plain();
                };
                if v.constant {
                    self.err(format!("cannot assign to constant '{name}'"));
                    return Self::plain();
                }
                match (index.is_some(), v.array) {
                    (false, true) => {
                        self.err(format!("array '{name}' assigned without an index"));
                        return Self::plain();
                    }
                    (true, false) => {
                        self.err(format!("'{name}' is not an array"));
                        return Self::plain();
                    }
                    _ => {}
                }
                if let Some(it) = &it {
                    self.expect_index(it);
                }
                self.require_assignable(&vt, &v.ty);
                Self::plain()
            }
            Stmt::Call { name, args, line } => {
                self.line = *line;
                // The result of a call used as a statement is discarded, including for
                // `returns nothing` and for values.
                self.call(ctx, name, args);
                Self::plain()
            }
            Stmt::If {
                branches,
                otherwise,
                line,
            } => {
                self.line = *line;
                let mut lists: Vec<Option<RList>> = Vec::new();
                for (cond, body) in branches {
                    let t = self.expr(ctx, cond);
                    self.expect_bool(&t, "'if'/'elseif' condition needs a boolean");
                    lists.push(self.stmts(ctx, body, loop_depth));
                }
                let else_list = self.stmts(ctx, otherwise, loop_depth);
                // `elseif` chains nest to the right: if c1 then A else (if c2 then B else C).
                let mut tail: Option<Info> = else_list.map(|l| l.info);
                for list in lists.into_iter().rev() {
                    let (info, bad) = if_node(list.map(|l| l.info), tail, self.strict);
                    if bad {
                        self.line = *line;
                        self.err("branches of an 'if' return different types");
                    }
                    tail = Some(info);
                }
                RStmt {
                    exit: false,
                    info: tail.unwrap_or(Info::NONE),
                }
            }
            Stmt::Loop { body, line } => {
                self.line = *line;
                let l = self.stmts(ctx, body, loop_depth + 1);
                RStmt {
                    exit: false,
                    info: l.map(|l| l.info).unwrap_or(Info::NONE),
                }
            }
            Stmt::ExitWhen { cond, line } => {
                self.line = *line;
                if loop_depth == 0 {
                    self.err("'exitwhen' outside of a loop");
                }
                let t = self.expr(ctx, cond);
                self.expect_bool(&t, "'exitwhen' condition needs a boolean");
                RStmt {
                    exit: true,
                    info: Info::NONE,
                }
            }
            Stmt::Return { value, line } => {
                self.line = *line;
                match value {
                    None => RStmt {
                        exit: false,
                        info: Info {
                            flag: 1,
                            ty: 0,
                            rf: None,
                        },
                    },
                    Some(v) => {
                        let t = self.expr(ctx, v);
                        // An expression that already failed is typed like the function's return
                        // type so that it adds nothing to the analysis.
                        let code = Self::rcode(&t)
                            .or_else(|| Self::rcode(&ctx.ret))
                            .unwrap_or((0, None));
                        RStmt {
                            exit: false,
                            info: Info {
                                flag: 2,
                                ty: code.0,
                                rf: code.1,
                            },
                        }
                    }
                }
            }
            // The `debug` prefix does not change the analysis of the statement it prefixes.
            Stmt::Debug(inner) => self.stmt(ctx, inner, loop_depth),
        }
    }

    /// The function-level check (validator `0x6F4611D0` after the setter `0x6F461260`): `return`
    /// values need a `returns` type that accepts them, and a function that returns a value needs a
    /// value `return` somewhere in its body.
    fn function_returns(&mut self, info: &FuncInfo, list: Option<&RList>) {
        let Some(ret) = &info.ret else { return };
        let body = list.map(|l| l.info.clone()).unwrap_or(Info::NONE);
        if *ret == Ty::Nothing {
            if body.flag & 2 != 0 {
                self.err(format!(
                    "'{}' returns nothing but returns a value",
                    info.name
                ));
            }
            return;
        }
        let Some((v5, dref)) = Self::rcode(&Some(ret.clone())) else {
            return;
        };
        let (mut v4, mut aref) = (body.ty, body.rf.clone());
        if list.is_none() {
            v4 = 0;
        }
        // A lone `null` takes the declared type when that type can be null.
        if v4 == T_NULL && matches!(v5, T_HANDLE | T_STRING | T_CODE) {
            v4 = v5;
            aref = dref.clone();
        }
        let mut ok = (v4 == T_INT && v5 == T_REAL) || (v4 == T_REAL && v5 == T_INT) || v4 == v5;
        if ok && self.strict && v5 == T_HANDLE && v4 != 0 {
            let (a, d) = (aref.unwrap_or_default(), dref.unwrap_or_default());
            ok = self.env.is_subtype(&a, &d);
        }
        if !ok {
            self.err(format!(
                "'{}': the returned value does not match the declared return type",
                info.name
            ));
        } else if body.flag & 2 == 0 {
            self.err(format!("'{}': missing return value", info.name));
        }
    }
}
