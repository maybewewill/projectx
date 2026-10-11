//! A flattened, indexed tree of frames and blocks ready for layout.
//!
//! Build a scene from a [`Library`] with [`Scene::build`]. Every node gets an
//! index. Names are case-insensitive and must be unique within a scene.

use std::collections::BTreeMap;
use std::fmt;

use crate::ast::{BlockKind, Child, Frame, Property};
use crate::error::Span;
use crate::library::{Library, LibraryError};
use crate::props;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Frame,
    Texture,
    String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    /// Upper-case frame type, or `TEXTURE` / `STRING` for blocks.
    pub type_name: String,
    /// Empty for unnamed blocks.
    pub name: String,
    pub parent: Option<usize>,
    /// Child node indices in declaration order (frames and blocks).
    pub children: Vec<usize>,
    pub props: Vec<Property>,
    pub layer: Option<String>,
    pub decorate: bool,
    pub span: Span,
}

impl Node {
    /// `SIMPLE*` frame types are not limited to the 4:3 safe area.
    pub fn is_simple(&self) -> bool {
        self.kind == NodeKind::Frame && self.type_name.starts_with("SIMPLE")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SceneError {
    Library(LibraryError),
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::Library(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<LibraryError> for SceneError {
    fn from(e: LibraryError) -> Self {
        SceneError::Library(e)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub nodes: Vec<Node>,
    /// Top-level frames in the order they were added.
    pub roots: Vec<usize>,
    /// Non-fatal notes. The game's own files reuse names (a template child
    /// inherited by two frames, or a name repeated in one file); the first
    /// node keeps the name and later ones are reported here.
    pub warnings: Vec<String>,
    index: BTreeMap<String, usize>,
    spans: Vec<Span>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves each root name through the library (inheritance applied) and
    /// adds it as a root.
    pub fn build(lib: &Library, roots: &[&str]) -> Result<Scene, SceneError> {
        let mut scene = Scene::new();
        for r in roots {
            let flat = lib.resolve(r)?;
            scene.add_frame(&flat)?;
        }
        Ok(scene)
    }

    /// Adds an already flattened frame (and its tree) as a new root.
    pub fn add_frame(&mut self, f: &Frame) -> Result<usize, SceneError> {
        let idx = self.add_rec(f, None)?;
        self.roots.push(idx);
        Ok(idx)
    }

    /// Node by name (case-insensitive).
    pub fn find(&self, name: &str) -> Option<usize> {
        self.index.get(&name.to_ascii_lowercase()).copied()
    }

    /// Registers a name for the node about to be pushed. A repeated name keeps
    /// the first node and records a warning.
    fn register(&mut self, name: &str, span: Span) {
        if name.is_empty() {
            return;
        }
        let key = name.to_ascii_lowercase();
        if let Some(&first) = self.index.get(&key) {
            let f = self.spans[first];
            self.warnings.push(format!(
                "name \"{name}\" used twice (line {}, column {} and line {}, column {}); first kept",
                f.line, f.col, span.line, span.col
            ));
            return;
        }
        self.index.insert(key, self.nodes.len());
    }

    fn push(&mut self, node: Node) -> usize {
        self.spans.push(node.span);
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn add_rec(&mut self, f: &Frame, parent: Option<usize>) -> Result<usize, SceneError> {
        self.register(&f.name, f.span);
        let idx = self.push(Node {
            kind: NodeKind::Frame,
            type_name: f.kind.clone(),
            name: f.name.clone(),
            parent,
            children: Vec::new(),
            props: f.props.clone(),
            layer: f.layer.clone(),
            decorate: props::has(&f.props, "DecorateFileNames"),
            span: f.span,
        });
        for c in &f.children {
            let ci = match c {
                Child::Frame(cf) => self.add_rec(cf, Some(idx))?,
                Child::Block(b) => {
                    let name = b.name.clone().unwrap_or_default();
                    self.register(&name, b.span);
                    let (kind, type_name) = match b.kind {
                        BlockKind::Texture => (NodeKind::Texture, "TEXTURE"),
                        BlockKind::String => (NodeKind::String, "STRING"),
                    };
                    self.push(Node {
                        kind,
                        type_name: type_name.to_string(),
                        name,
                        parent: Some(idx),
                        children: Vec::new(),
                        props: b.props.clone(),
                        layer: b.layer.clone(),
                        decorate: props::has(&f.props, "DecorateFileNames"),
                        span: b.span,
                    })
                }
            };
            self.nodes[idx].children.push(ci);
        }
        Ok(idx)
    }
}
