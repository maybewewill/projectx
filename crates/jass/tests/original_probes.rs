//! Replays the probe corpus recorded from the original compiler of Game.dll 1.26a (the compile path
//! the game uses for `common.j` + `Blizzard.j` + `war3map.j`, run in Unicorn with the natives
//! registered) against `jass::parse` + `jass::check::check_with`: every program the original accepted must be
//! accepted, every program it rejected must be rejected here (at parse or check time).
//!
//! `tests/data/original_probes.txt` holds hand-written programs (declaration order, namespaces, return
//! statements, lexical forms) and random `return` structures; `tests/data/original_matrix.txt` holds
//! the outcome of every operator / assignment / argument / return combination of the types
//! integer, real, string, boolean, code, handle, widget, unit, item, player and null; the sources
//! of the matrix are generated below. All programs are our own text. See docs/spec/jass-frontend.md
//! ("Real compile path") for the rules this pins down.

use jass::check::{check_with, CheckOptions, NativeRegistry};

const PROBES: &str = include_str!("data/original_probes.txt");
const MATRIX: &str = include_str!("data/original_matrix.txt");
const NATIVES: &str = include_str!("../../../docs/spec/natives-1.26a.tsv");

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn registry() -> NativeRegistry {
    NativeRegistry::from_tsv(NATIVES, &["common"])
}

/// Whether our pipeline accepts the program set.
fn accepts(segs: &[String], lenient: bool) -> Result<(), String> {
    let mut programs = Vec::new();
    for s in segs {
        match jass::parse(s) {
            Ok(p) => programs.push(p),
            Err(e) => return Err(format!("parse: {e}")),
        }
    }
    let opts = CheckOptions {
        lenient_returns: lenient,
        natives: Some(registry()),
    };
    let d = check_with(&programs, &opts);
    if d.is_empty() {
        Ok(())
    } else {
        Err(format!("check: line {} {}", d[0].line, d[0].message))
    }
}

struct Probe {
    name: String,
    lenient: bool,
    ok: bool,
    segs: Vec<String>,
}

fn probes() -> Vec<Probe> {
    let mut out: Vec<Probe> = Vec::new();
    for line in PROBES.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix('@') {
            let cols: Vec<&str> = h.split('\t').collect();
            out.push(Probe {
                name: cols[0].to_string(),
                lenient: cols[1] == "lenient",
                ok: cols[2] == "1",
                segs: Vec::new(),
            });
        } else if let Some(s) = line.strip_prefix("seg\t") {
            out.last_mut().unwrap().segs.push(unescape(s));
        }
    }
    out
}

#[test]
fn hand_written_and_random_programs_match_the_original() {
    let all = probes();
    assert!(all.len() > 1000, "corpus missing: {}", all.len());
    let mut bad = Vec::new();
    for p in &all {
        let ours = accepts(&p.segs, p.lenient);
        if ours.is_ok() != p.ok {
            bad.push(format!(
                "{}: original {} ours {:?}\n{}",
                p.name,
                if p.ok { "accepts" } else { "rejects" },
                ours,
                p.segs.join("\n--- next program ---\n")
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} probes differ:\n{}",
        bad.len(),
        all.len(),
        bad.iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n=====\n")
    );
}

// ---- the operator / assignment matrix --------------------------------------------------------

const PRE: &str = "type widget extends handle\ntype unit extends widget\ntype item extends widget\ntype player extends handle\n";

fn f(body: &str, ret: &str, params: &str, name: &str) -> String {
    format!("function {name} takes {params} returns {ret}\n{body}\nendfunction\n")
}

fn decl(t: &str, v: &str) -> String {
    if t == "null" {
        String::new()
    } else {
        format!("local {t} {v}\n")
    }
}

fn ex(t: &str, v: &str) -> String {
    if t == "null" {
        "null".into()
    } else {
        v.into()
    }
}

const TYPES: [&str; 11] = [
    "integer", "real", "string", "boolean", "code", "handle", "widget", "unit", "item", "player",
    "null",
];

/// Source of a matrix key (mirror of the generator that recorded the outcomes).
fn matrix_source(key: &str) -> Option<String> {
    let ops: [(&str, &str); 10] = [
        ("eq", "=="),
        ("ne", "!="),
        ("lt", "<"),
        ("ge", ">="),
        ("add", "+"),
        ("sub", "-"),
        ("mul", "*"),
        ("div", "/"),
        ("and", "and"),
        ("or", "or"),
    ];
    let t = |s: &str| TYPES.iter().find(|x| **x == s).copied();
    if let Some(rest) = key.strip_prefix("bin_") {
        let (name, rest) = rest.split_once('_')?;
        let op = ops.iter().find(|o| o.0 == name)?.1;
        let (rest, tgt) = match rest.split_once("_to_") {
            Some((r, g)) => (r, Some(g)),
            None => (rest, None),
        };
        let (a, b) = rest.split_once('_')?;
        let (a, b) = (t(a)?, t(b)?);
        let body = format!("{}{}", decl(a, "a"), decl(b, "b"));
        let stmt = match tgt {
            Some(g) => format!("local {g} r = {} {op} {}", ex(a, "a"), ex(b, "b")),
            None => format!("local boolean r = {} {op} {}", ex(a, "a"), ex(b, "b")),
        };
        return Some(format!(
            "{PRE}{}",
            f(&format!("{body}{stmt}"), "nothing", "nothing", "f")
        ));
    }
    for (prefix, kind) in [
        ("asg_", 0),
        ("init_", 1),
        ("arg_", 2),
        ("ret_", 3),
        ("glob_", 4),
    ] {
        if let Some(rest) = key.strip_prefix(prefix) {
            let (a, b) = rest.split_once("_from_")?;
            let (a, b) = (t(a)?, t(b)?);
            let body = decl(b, "b");
            let src = match kind {
                0 => f(
                    &format!("{}{body}set a = {}", decl(a, "a"), ex(b, "b")),
                    "nothing",
                    "nothing",
                    "f",
                ),
                1 => f(
                    &format!("{body}local {a} a = {}", ex(b, "b")),
                    "nothing",
                    "nothing",
                    "f",
                ),
                2 => format!(
                    "{}{}",
                    f("", "nothing", &format!("{a} p"), "g"),
                    f(
                        &format!("{body}call g({})", ex(b, "b")),
                        "nothing",
                        "nothing",
                        "f"
                    )
                ),
                3 => f(&format!("{body}return {}", ex(b, "b")), a, "nothing", "f"),
                _ => format!(
                    "globals\n{a} G\nendglobals\n{}",
                    f(
                        &format!("{}set G = {}", decl(b, "b"), ex(b, "b")),
                        "nothing",
                        "nothing",
                        "f"
                    )
                ),
            };
            return Some(format!("{PRE}{src}"));
        }
    }
    let unary: [(&str, &str); 8] = [
        ("neg_", "local real r = -a"),
        ("not_", "local boolean r = not a"),
        ("if_", "if a then\nendif"),
        ("exitwhen_", "loop\nexitwhen a\nendloop"),
        ("indexr_", "local integer r = A[a]"),
        ("index_", "set A[a] = 1"),
        ("elseif_", "if true then\nelseif a then\nendif"),
        ("negint_", "local integer r = -a"),
    ];
    let arr = "globals\ninteger array A\nendglobals\n";
    // Special names first.
    let special = |k: &str| -> Option<String> {
        match k {
            "if_null" => Some(f("if null then\nendif", "nothing", "nothing", "f")),
            "neg_null" => Some(f("local integer r = -null", "nothing", "nothing", "f")),
            "not_null" => Some(f("local boolean r = not null", "nothing", "nothing", "f")),
            "index_null" => Some(format!(
                "{arr}{}",
                f("set A[null] = 1", "nothing", "nothing", "f")
            )),
            _ => None,
        }
    };
    if let Some(s) = special(key) {
        return Some(format!("{PRE}{s}"));
    }
    // neg_<type>_int has its own suffix.
    if let Some(rest) = key.strip_prefix("neg_") {
        if let Some(a) = rest.strip_suffix("_int") {
            let a = t(a)?;
            return Some(format!(
                "{PRE}{}",
                f(
                    &format!("{}local integer r = -a", decl(a, "a")),
                    "nothing",
                    "nothing",
                    "f"
                )
            ));
        }
    }
    for (prefix, stmt) in unary {
        if prefix == "negint_" {
            continue;
        }
        if let Some(a) = key.strip_prefix(prefix) {
            let a = t(a)?;
            let pre_arr = if prefix.starts_with("index") { arr } else { "" };
            return Some(format!(
                "{PRE}{pre_arr}{}",
                f(
                    &format!("{}{stmt}", decl(a, "a")),
                    "nothing",
                    "nothing",
                    "f"
                )
            ));
        }
    }
    None
}

#[test]
fn operator_and_assignment_matrix_matches_the_original() {
    let mut bad = Vec::new();
    let mut n = 0;
    for line in MATRIX.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let (key, ok) = line.split_once('\t').unwrap();
        let src = matrix_source(key).unwrap_or_else(|| panic!("no generator for {key}"));
        let ours = accepts(std::slice::from_ref(&src), false);
        n += 1;
        if ours.is_ok() != (ok == "1") {
            bad.push(format!("{key}: original ok={ok}, ours {ours:?}"));
        }
    }
    assert!(n > 2500, "matrix missing: {n}");
    assert!(
        bad.is_empty(),
        "{} of {n} matrix entries differ:\n{}",
        bad.len(),
        bad.iter().take(60).cloned().collect::<Vec<_>>().join("\n")
    );
}
