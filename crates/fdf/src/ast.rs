//! Syntax tree of an FDF file.
//!
//! Every action is kept as a raw [`Property`] (key plus values), so keys this
//! crate does not interpret are never lost. Typed meaning is added by
//! `props.rs`.

use crate::error::Span;

/// One value after a property key.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f32),
    Str(String),
    Ident(String),
}

impl Value {
    /// Text of a string or bare word.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Ident(s) => Some(s),
            Value::Num(_) => None,
        }
    }
}

/// `Key value, value, ...,`
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: String,
    pub values: Vec<Value>,
    pub span: Span,
}

/// `INHERITS [WITHCHILDREN] "Parent"`
#[derive(Debug, Clone, PartialEq)]
pub struct Inherits {
    pub parent: String,
    pub with_children: bool,
}

/// `Frame "TYPE" "Name" [INHERITS ...] { ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Upper-cased frame type, e.g. `BACKDROP`, `SIMPLEFRAME`.
    pub kind: String,
    pub name: String,
    pub inherits: Option<Inherits>,
    pub props: Vec<Property>,
    pub children: Vec<Child>,
    /// Layer level if this frame sits inside a `Layer "..." { }` block.
    pub layer: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Texture,
    String,
}

/// `Texture [name] { ... }` or `String [name] { ... }`: a property-only element.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub name: Option<String>,
    pub props: Vec<Property>,
    pub layer: Option<String>,
    pub span: Span,
}

/// An element nested in a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Child {
    Frame(Frame),
    Block(Block),
}

impl Child {
    pub fn name(&self) -> Option<&str> {
        match self {
            Child::Frame(f) => Some(&f.name),
            Child::Block(b) => b.name.as_deref(),
        }
    }
    pub fn layer(&self) -> Option<&str> {
        match self {
            Child::Frame(f) => f.layer.as_deref(),
            Child::Block(b) => b.layer.as_deref(),
        }
    }
}

/// One `KEY "value",` line of a `StringList`.
#[derive(Debug, Clone, PartialEq)]
pub struct StringEntry {
    pub key: String,
    pub value: String,
    pub span: Span,
}

/// `StringList { KEY "value", ... }`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StringList {
    pub entries: Vec<StringEntry>,
}

/// A top-level item of an FDF file.
#[derive(Debug, Clone, PartialEq)]
pub enum TopItem {
    Include { path: String, span: Span },
    StringList(StringList),
    Frame(Frame),
}

/// A parsed FDF file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub items: Vec<TopItem>,
}
