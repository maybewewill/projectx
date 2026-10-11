//! A set of loaded FDF files: templates, StringLists and inheritance.
//!
//! The host supplies file contents through a loader closure, so the crate
//! never touches the file system. For the game, the loader reads the path
//! from the user's own MPQ archives.
//!
//! Lookups (frame names, StringList keys, INHERITS parents) are
//! case-insensitive. Only top-level frames can be inherited, as in the game.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ast::{Block, Child, Document, Frame, Property, StringList, TopItem};
use crate::error::ParseError;
use crate::parser::parse;

const MAX_INCLUDE_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryError {
    /// `IncludeFile` target the loader did not return.
    MissingInclude {
        path: String,
        from: String,
    },
    /// Syntax error in a loaded file.
    Parse {
        path: String,
        error: ParseError,
    },
    UnknownFrame(String),
    /// `INHERITS` names a frame that is not defined as a top-level frame.
    UnknownParent {
        frame: String,
        parent: String,
    },
    /// Inheritance (or a nested frame inheriting an ancestor) loops.
    InheritCycle(Vec<String>),
    IncludeTooDeep(String),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryError::MissingInclude { path, from } => {
                write!(f, "IncludeFile \"{path}\" in {from} was not found")
            }
            LibraryError::Parse { path, error } => write!(f, "{path}: {error}"),
            LibraryError::UnknownFrame(n) => write!(f, "no frame named \"{n}\""),
            LibraryError::UnknownParent { frame, parent } => {
                write!(f, "frame \"{frame}\" inherits from unknown \"{parent}\"")
            }
            LibraryError::InheritCycle(path) => {
                write!(f, "inheritance cycle: {}", path.join(" -> "))
            }
            LibraryError::IncludeTooDeep(p) => write!(f, "IncludeFile nesting too deep at \"{p}\""),
        }
    }
}

impl std::error::Error for LibraryError {}

/// Loaded templates and strings.
#[derive(Debug, Default, Clone)]
pub struct Library {
    /// Top-level frames by lower-case name.
    frames: BTreeMap<String, Frame>,
    /// StringList entries by lower-case key. Later definitions win.
    strings: BTreeMap<String, String>,
    /// Lower-case paths already loaded (each file loads once).
    loaded: BTreeSet<String>,
    /// Non-fatal notes, e.g. a frame or string defined twice.
    pub warnings: Vec<String>,
}

/// Lower-case, `/` and `\` treated alike, for file and name comparisons.
pub fn normalize_path(p: &str) -> String {
    p.replace('/', "\\").to_ascii_lowercase()
}

impl Library {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads one FDF file by path, following its `IncludeFile`s depth-first.
    /// A path that is already loaded is skipped.
    pub fn load(
        &mut self,
        path: &str,
        loader: &mut dyn FnMut(&str) -> Option<String>,
    ) -> Result<(), LibraryError> {
        self.load_at(path, "<root>", loader, 0)
    }

    /// Loads FDF text that has the given origin name (used in errors).
    pub fn load_source(
        &mut self,
        origin: &str,
        src: &str,
        loader: &mut dyn FnMut(&str) -> Option<String>,
    ) -> Result<(), LibraryError> {
        let doc = parse(src).map_err(|error| LibraryError::Parse {
            path: origin.to_string(),
            error,
        })?;
        self.add_document(origin, doc, loader, 0)
    }

    fn load_at(
        &mut self,
        path: &str,
        from: &str,
        loader: &mut dyn FnMut(&str) -> Option<String>,
        depth: usize,
    ) -> Result<(), LibraryError> {
        if depth > MAX_INCLUDE_DEPTH {
            return Err(LibraryError::IncludeTooDeep(path.to_string()));
        }
        let key = normalize_path(path);
        if self.loaded.contains(&key) {
            return Ok(());
        }
        self.loaded.insert(key);
        let src = loader(path).ok_or_else(|| LibraryError::MissingInclude {
            path: path.to_string(),
            from: from.to_string(),
        })?;
        let doc = parse(&src).map_err(|error| LibraryError::Parse {
            path: path.to_string(),
            error,
        })?;
        self.add_document(path, doc, loader, depth)
    }

    fn add_document(
        &mut self,
        origin: &str,
        doc: Document,
        loader: &mut dyn FnMut(&str) -> Option<String>,
        depth: usize,
    ) -> Result<(), LibraryError> {
        for item in doc.items {
            match item {
                TopItem::Include { path, .. } => self.load_at(&path, origin, loader, depth + 1)?,
                TopItem::StringList(list) => self.add_strings(list),
                TopItem::Frame(f) => {
                    let key = f.name.to_ascii_lowercase();
                    if self.frames.insert(key, f.clone()).is_some() {
                        self.warnings.push(format!(
                            "frame \"{}\" defined twice; last definition wins",
                            f.name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn add_strings(&mut self, list: StringList) {
        for e in list.entries {
            let key = e.key.to_ascii_lowercase();
            if self.strings.insert(key, e.value).is_some() {
                self.warnings.push(format!(
                    "string \"{}\" defined twice; last definition wins",
                    e.key
                ));
            }
        }
    }

    /// The raw (unflattened) top-level frame with this name.
    pub fn frame(&self, name: &str) -> Option<&Frame> {
        self.frames.get(&name.to_ascii_lowercase())
    }

    /// Names of all top-level frames, lower-case, in sorted order.
    pub fn frame_names(&self) -> impl Iterator<Item = &str> {
        self.frames.keys().map(String::as_str)
    }

    /// StringList value for a key (case-insensitive).
    pub fn string(&self, key: &str) -> Option<&str> {
        self.strings
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// With `DecorateFileNames`, a value that is a StringList key is replaced
    /// by its string. Otherwise, or if the key is unknown, the value is kept.
    pub fn decorate(&self, decorated: bool, value: &str) -> String {
        if decorated {
            if let Some(s) = self.string(value) {
                return s.to_string();
            }
        }
        value.to_string()
    }

    /// The top-level frame with inheritance applied to it and to every
    /// nested frame.
    pub fn resolve(&self, name: &str) -> Result<Frame, LibraryError> {
        let f = self
            .frame(name)
            .ok_or_else(|| LibraryError::UnknownFrame(name.to_string()))?;
        let mut stack = vec![f.name.to_ascii_lowercase()];
        self.flatten(f, &mut stack)
    }

    fn flatten(&self, f: &Frame, stack: &mut Vec<String>) -> Result<Frame, LibraryError> {
        let mut children = Vec::with_capacity(f.children.len());
        for c in &f.children {
            children.push(match c {
                Child::Frame(cf) => {
                    stack.push(cf.name.to_ascii_lowercase());
                    let flat = self.flatten(cf, stack);
                    stack.pop();
                    Child::Frame(flat?)
                }
                Child::Block(b) => Child::Block(b.clone()),
            });
        }
        let mut out = Frame {
            children,
            ..f.clone()
        };
        let Some(inh) = &f.inherits else {
            return Ok(out);
        };

        let key = inh.parent.to_ascii_lowercase();
        if stack.contains(&key) {
            let mut path = stack.clone();
            path.push(key);
            return Err(LibraryError::InheritCycle(path));
        }
        let parent_raw = self
            .frames
            .get(&key)
            .ok_or_else(|| LibraryError::UnknownParent {
                frame: f.name.clone(),
                parent: inh.parent.clone(),
            })?;
        stack.push(key);
        let parent = self.flatten(parent_raw, stack);
        stack.pop();
        let parent = parent?;

        out.props = merge_props(&parent.props, &f.props);
        out.children = merge_children(&parent.children, &out.children, inh.with_children);
        out.inherits = None;
        Ok(out)
    }
}

/// Identity of an anchor property: its own point, or `None` for SetAllPoints.
fn anchor_identity(p: &Property) -> Option<String> {
    match p.key.to_ascii_uppercase().as_str() {
        "SETPOINT" | "ANCHOR" => Some(
            p.values
                .first()
                .and_then(|v| v.as_text())
                .map(|s| s.to_ascii_uppercase())
                .unwrap_or_default(),
        ),
        "SETALLPOINTS" => Some("*".to_string()),
        _ => None,
    }
}

/// Child properties replace inherited ones: an anchor replaces the inherited
/// anchor for the same own point, and any other key replaces every inherited
/// property with that key. Order is inherited first, then the child's.
fn merge_props(parent: &[Property], child: &[Property]) -> Vec<Property> {
    let mut child_anchor_ids = BTreeSet::new();
    let mut child_keys = BTreeSet::new();
    for p in child {
        match anchor_identity(p) {
            Some(id) => {
                child_anchor_ids.insert(id);
            }
            None => {
                child_keys.insert(p.key.to_ascii_uppercase());
            }
        }
    }
    let mut out: Vec<Property> = parent
        .iter()
        .filter(|p| match anchor_identity(p) {
            Some(id) => !child_anchor_ids.contains(&id),
            None => !child_keys.contains(&p.key.to_ascii_uppercase()),
        })
        .cloned()
        .collect();
    out.extend(child.iter().cloned());
    out
}

/// Without `WITHCHILDREN` the inherited children are dropped. A child with the
/// same name as an inherited child replaces it in place (no deep merge).
fn merge_children(parent: &[Child], own: &[Child], with_children: bool) -> Vec<Child> {
    let mut out: Vec<Child> = if with_children {
        parent.to_vec()
    } else {
        Vec::new()
    };
    for c in own {
        if let Some(name) = c.name() {
            if let Some(i) = out
                .iter()
                .position(|x| x.name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
            {
                out[i] = c.clone();
                continue;
            }
        }
        out.push(c.clone());
    }
    out
}

/// Helper for tests and tools: all blocks directly under a frame.
pub fn blocks(f: &Frame) -> impl Iterator<Item = &Block> {
    f.children.iter().filter_map(|c| match c {
        Child::Block(b) => Some(b),
        Child::Frame(_) => None,
    })
}
