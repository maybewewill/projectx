//! Layout: anchors to absolute rectangles in WC3 screen space.
//!
//! # Coordinates
//! The UI space is the 4:3 "safe area" `0..0.8 x 0..0.6`, origin bottom-left,
//! x right, y up. Frames in the Frame group (not `SIMPLE*`) are limited to it.
//!
//! # Aspect ratio (pillarbox rule)
//! The safe area is never stretched. It is centred horizontally in the
//! window. The window in UI units is `0.6 * aspect` wide and 0.6 tall, so it
//! spans `[0.4 - 0.3*aspect, 0.4 + 0.3*aspect] x [0, 0.6]`. For 4:3 that is
//! exactly the safe area. For 16:9 it is `-0.133..0.933`. The extra space
//! outside the safe area is only usable by `SIMPLE*` frames and by frames
//! reparented to a host-supplied external frame (see [`ExternalFrames`]).
//!
//! # Roots
//! A top-level `SIMPLE*` frame is positioned in the whole window. Any other
//! top-level frame is positioned in the safe area.
//!
//! # Solver
//! Each node depends on its parent and on the frames its anchors name.
//! Nodes are solved in dependency order. A cycle is an error. Per axis:
//! - the own point's edge is fixed to `relative edge + offset`,
//! - `Width` / `Height` give the size when only one edge is fixed,
//! - with no size and only a centre, the size is 0.
//!
//! A node with no anchors is unpositioned (`None`, with a warning). A node
//! whose anchor target is unpositioned is also `None`.

use std::collections::BTreeMap;
use std::fmt;

use crate::props::{self, Anchor, Edge, PropError, Relative};
use crate::scene::{Node, Scene};

/// Centre x of the 4:3 safe area.
pub const SAFE_CENTER_X: f32 = 0.4;
pub const SAFE_WIDTH: f32 = 0.8;
pub const SAFE_HEIGHT: f32 = 0.6;

const EPS: f32 = 1e-6;

/// Axis-aligned rectangle in UI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

impl Rect {
    pub fn new(left: f32, bottom: f32, right: f32, top: f32) -> Self {
        Rect {
            left,
            bottom,
            right,
            top,
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.top - self.bottom
    }

    /// True if `self` contains `inner` (with a small tolerance).
    pub fn contains(&self, inner: &Rect) -> bool {
        inner.left >= self.left - EPS
            && inner.right <= self.right + EPS
            && inner.bottom >= self.bottom - EPS
            && inner.top <= self.top + EPS
    }

    fn edge_x(&self, e: Edge) -> f32 {
        match e {
            Edge::Low => self.left,
            Edge::Mid => (self.left + self.right) * 0.5,
            Edge::High => self.right,
        }
    }

    fn edge_y(&self, e: Edge) -> f32 {
        match e {
            Edge::Low => self.bottom,
            Edge::Mid => (self.bottom + self.top) * 0.5,
            Edge::High => self.top,
        }
    }
}

/// The screen: window and safe area, in UI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub window: Rect,
    pub safe: Rect,
}

impl Screen {
    /// The window `width_px x height_px` in UI units (pillarbox rule above).
    /// A zero dimension is treated as 4:3.
    pub fn for_window(width_px: u32, height_px: u32) -> Screen {
        let aspect = if width_px == 0 || height_px == 0 {
            SAFE_WIDTH / SAFE_HEIGHT
        } else {
            width_px as f32 / height_px as f32
        };
        let safe = Rect::new(0.0, 0.0, SAFE_WIDTH, SAFE_HEIGHT);
        // 4:3 is the safe area exactly; snap it so float rounding cannot shift the edges.
        if (aspect - SAFE_WIDTH / SAFE_HEIGHT).abs() < 1e-5 {
            return Screen { window: safe, safe };
        }
        let half = 0.3 * aspect;
        Screen {
            window: Rect::new(SAFE_CENTER_X - half, 0.0, SAFE_CENTER_X + half, SAFE_HEIGHT),
            safe,
        }
    }

    /// A 4:3 window (window = safe area).
    pub fn safe_area() -> Screen {
        Self::for_window(4, 3)
    }

    fn root_rect(&self, node: &Node) -> Rect {
        if node.is_simple() {
            self.window
        } else {
            self.safe
        }
    }
}

/// Host-supplied rectangles for frames that live outside the scene
/// (for example `ConsoleUIBackdrop`). Keys are case-insensitive.
pub type ExternalFrames = BTreeMap<String, Rect>;

#[derive(Debug, Clone, PartialEq)]
pub enum Warning {
    /// A Frame-group frame extends outside the 4:3 safe area.
    OutsideSafeArea { node: String },
    /// A node has no anchors, so it has no position.
    Unpositioned { node: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutError {
    /// Anchors form a loop. The names are in dependency order.
    Cycle(Vec<String>),
    /// A named relative is neither in the scene nor an external frame.
    UnknownRelative { node: String, name: String },
    /// A property on a node has a bad value.
    Property { node: String, error: PropError },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::Cycle(names) => write!(f, "anchor cycle: {}", names.join(" -> ")),
            LayoutError::UnknownRelative { node, name } => {
                write!(f, "\"{node}\" is anchored to unknown frame \"{name}\"")
            }
            LayoutError::Property { node, error } => {
                write!(f, "\"{node}\": {} {}", error.key, error.message)
            }
        }
    }
}

impl std::error::Error for LayoutError {}

/// Solved rectangles, indexed like `Scene::nodes`.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub rects: Vec<Option<Rect>>,
    pub warnings: Vec<Warning>,
    /// Node indices in the order they were solved (dependencies first).
    pub order: Vec<usize>,
}

impl Layout {
    pub fn rect(&self, node: usize) -> Option<Rect> {
        self.rects.get(node).copied().flatten()
    }
}

/// Solves all node rectangles of a scene.
pub fn solve(
    scene: &Scene,
    screen: &Screen,
    externals: &ExternalFrames,
) -> Result<Layout, LayoutError> {
    let ext: BTreeMap<String, Rect> = externals
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), *v))
        .collect();
    let n = scene.nodes.len();

    let mut anchors: Vec<Vec<Anchor>> = Vec::with_capacity(n);
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, node) in scene.nodes.iter().enumerate() {
        let a = props::anchors(&node.props).map_err(|error| LayoutError::Property {
            node: node.name.clone(),
            error,
        })?;
        if let Some(p) = node.parent {
            deps[i].push(p);
        }
        for anc in &a {
            if let Relative::Named(name) = &anc.relative {
                match scene.find(name) {
                    Some(j) => deps[i].push(j),
                    None if ext.contains_key(&name.to_ascii_lowercase()) => {}
                    None => {
                        return Err(LayoutError::UnknownRelative {
                            node: node.name.clone(),
                            name: name.clone(),
                        })
                    }
                }
            }
        }
        anchors.push(a);
    }

    let order = topo_order(&deps, scene)?;

    let mut rects: Vec<Option<Rect>> = vec![None; n];
    let mut warnings = Vec::new();
    for &i in &order {
        let node = &scene.nodes[i];
        let parent_rect = match node.parent {
            Some(p) => rects[p],
            None => Some(screen.root_rect(node)),
        };
        let Some(parent_rect) = parent_rect else {
            continue;
        };
        if anchors[i].is_empty() {
            if node.parent.is_none() {
                // A root with no anchors fills its screen rectangle: the game's
                // root frames (for example ConsoleUI) carry no SetPoint.
                rects[i] = Some(parent_rect);
                continue;
            }
            warnings.push(Warning::Unpositioned {
                node: node.name.clone(),
            });
            continue;
        }
        let Some(rect) = solve_node(node, &anchors[i], parent_rect, &rects, scene, &ext)? else {
            continue;
        };
        if node.kind == crate::scene::NodeKind::Frame
            && !node.is_simple()
            && !screen.safe.contains(&rect)
        {
            warnings.push(Warning::OutsideSafeArea {
                node: node.name.clone(),
            });
        }
        rects[i] = Some(rect);
    }
    Ok(Layout {
        rects,
        warnings,
        order,
    })
}

fn solve_node(
    node: &Node,
    anchors: &[Anchor],
    parent_rect: Rect,
    rects: &[Option<Rect>],
    scene: &Scene,
    ext: &BTreeMap<String, Rect>,
) -> Result<Option<Rect>, LayoutError> {
    let perr = |error: PropError| LayoutError::Property {
        node: node.name.clone(),
        error,
    };
    let width = props::number(&node.props, "Width").map_err(perr)?;
    let height = props::number(&node.props, "Height").map_err(perr)?;

    // Per-edge constraints: index 0 = Low, 1 = Mid, 2 = High. First wins.
    let mut cx: [Option<f32>; 3] = [None; 3];
    let mut cy: [Option<f32>; 3] = [None; 3];
    for a in anchors {
        let rel = match &a.relative {
            Relative::Parent => Some(parent_rect),
            Relative::Named(name) => match scene.find(name) {
                Some(j) => rects[j],
                None => ext.get(&name.to_ascii_lowercase()).copied(),
            },
        };
        let Some(rel) = rel else { return Ok(None) };
        let vx = rel.edge_x(a.rel_point.x_edge()) + a.x;
        let vy = rel.edge_y(a.rel_point.y_edge()) + a.y;
        let ox = slot(a.own.x_edge());
        let oy = slot(a.own.y_edge());
        if cx[ox].is_none() {
            cx[ox] = Some(vx);
        }
        if cy[oy].is_none() {
            cy[oy] = Some(vy);
        }
    }
    let x = solve_axis(cx, width);
    let y = solve_axis(cy, height);
    Ok(match (x, y) {
        (Some((l, r)), Some((b, t))) => Some(Rect::new(l, b, r, t)),
        _ => None,
    })
}

fn slot(e: Edge) -> usize {
    match e {
        Edge::Low => 0,
        Edge::Mid => 1,
        Edge::High => 2,
    }
}

/// Returns (low, high) of one axis from [low, mid, high] constraints and an
/// optional size.
fn solve_axis(c: [Option<f32>; 3], size: Option<f32>) -> Option<(f32, f32)> {
    let (low, mid, high) = (c[0], c[1], c[2]);
    match (low, high, mid, size) {
        (Some(l), Some(h), _, _) => Some((l, h)),
        (Some(l), None, _, Some(w)) => Some((l, l + w)),
        (Some(l), None, Some(m), None) => Some((l, 2.0 * m - l)),
        (Some(l), None, None, None) => Some((l, l)),
        (None, Some(h), _, Some(w)) => Some((h - w, h)),
        (None, Some(h), Some(m), None) => Some((2.0 * m - h, h)),
        (None, Some(h), None, None) => Some((h, h)),
        (None, None, Some(m), Some(w)) => Some((m - w * 0.5, m + w * 0.5)),
        (None, None, Some(m), None) => Some((m, m)),
        (None, None, None, _) => None,
    }
}

/// Dependency order (dependencies first). Reports the first cycle found.
fn topo_order(deps: &[Vec<usize>], scene: &Scene) -> Result<Vec<usize>, LayoutError> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        New,
        Active,
        Done,
    }
    fn visit(
        i: usize,
        deps: &[Vec<usize>],
        state: &mut [State],
        path: &mut Vec<usize>,
        order: &mut Vec<usize>,
    ) -> Result<(), Vec<usize>> {
        state[i] = State::Active;
        path.push(i);
        for &d in &deps[i] {
            match state[d] {
                State::New => visit(d, deps, state, path, order)?,
                State::Active => {
                    let start = path.iter().position(|&p| p == d).unwrap_or(0);
                    let mut cycle: Vec<usize> = path[start..].to_vec();
                    cycle.push(d);
                    return Err(cycle);
                }
                State::Done => {}
            }
        }
        path.pop();
        state[i] = State::Done;
        order.push(i);
        Ok(())
    }

    let mut state = vec![State::New; deps.len()];
    let mut order = Vec::with_capacity(deps.len());
    for i in 0..deps.len() {
        if state[i] == State::New {
            let mut path = Vec::new();
            if let Err(cycle) = visit(i, deps, &mut state, &mut path, &mut order) {
                return Err(LayoutError::Cycle(
                    cycle.iter().map(|&c| scene.nodes[c].name.clone()).collect(),
                ));
            }
        }
    }
    Ok(order)
}
