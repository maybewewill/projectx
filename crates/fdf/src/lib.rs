//! Warcraft III FDF (frame definition) files: parser, templates, layout and a
//! GPU-free draw list.
//!
//! Pipeline:
//! 1. [`Library::load`] parses a `.toc` entry and its `IncludeFile`s, using a
//!    host-supplied loader (the crate does no file I/O). Use [`parse_toc`] for
//!    the TOC itself.
//! 2. [`Library::resolve`] applies `INHERITS` (with `WITHCHILDREN`).
//! 3. [`Scene::build`] flattens the chosen roots into an indexed tree.
//! 4. [`layout::solve`] gives every node a rectangle in the WC3 UI space.
//! 5. [`DrawList::build`] emits quads and text runs in paint order.
//!
//! Nothing here reads game data. Files must come from the user's own copy of
//! the game at runtime (AGENTS.md). Test inputs are synthetic.

pub mod ast;
pub mod draw;
pub mod error;
pub mod layout;
pub mod lexer;
pub mod library;
pub mod parser;
pub mod props;
pub mod scene;

pub use ast::{
    Block, BlockKind, Child, Document, Frame, Inherits, Property, StringList, TopItem, Value,
};
pub use draw::{DrawItem, DrawList, Quad, QuadKind, TextRun};
pub use error::{ParseError, Span};
pub use layout::{ExternalFrames, Layout, LayoutError, Rect, Screen, Warning};
pub use library::{Library, LibraryError};
pub use parser::{parse, parse_toc};
pub use scene::{Node, NodeKind, Scene, SceneError};
