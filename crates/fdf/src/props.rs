//! Typed reading of the raw properties of a frame or block.
//!
//! Only the keys that layout and drawing need are typed here. Everything
//! else stays available as a raw [`Property`].

use crate::ast::{Property, Value};

/// A property value that does not match what the key expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropError {
    pub key: String,
    pub message: String,
}

impl PropError {
    fn new(key: &str, message: impl Into<String>) -> Self {
        PropError {
            key: key.to_string(),
            message: message.into(),
        }
    }
}

/// The last property with this key (case-insensitive). Later lines win.
pub fn last<'a>(props: &'a [Property], key: &str) -> Option<&'a Property> {
    props.iter().rev().find(|p| p.key.eq_ignore_ascii_case(key))
}

pub fn has(props: &[Property], key: &str) -> bool {
    last(props, key).is_some()
}

/// First value of a property as text (string or bare word).
pub fn text(props: &[Property], key: &str) -> Option<String> {
    last(props, key)?
        .values
        .first()?
        .as_text()
        .map(str::to_string)
}

/// The single number of a property, if present.
pub fn number(props: &[Property], key: &str) -> Result<Option<f32>, PropError> {
    let Some(p) = last(props, key) else {
        return Ok(None);
    };
    match p.values.as_slice() {
        [Value::Num(n)] => Ok(Some(*n)),
        _ => Err(PropError::new(key, "expected one number")),
    }
}

/// Exactly `n` numbers, if the property is present.
pub fn numbers(props: &[Property], key: &str, n: usize) -> Result<Option<Vec<f32>>, PropError> {
    let Some(p) = last(props, key) else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(n);
    for v in &p.values {
        match v {
            Value::Num(x) => out.push(*x),
            _ => return Err(PropError::new(key, "expected numbers only")),
        }
    }
    if out.len() != n {
        return Err(PropError::new(
            key,
            format!("expected {n} numbers, got {}", out.len()),
        ));
    }
    Ok(Some(out))
}

/// The 9 frame points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// Position of a point or of a rectangle edge along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// Left (x) or bottom (y).
    Low,
    /// Centre.
    Mid,
    /// Right (x) or top (y).
    High,
}

impl Point {
    /// Parses `TOPLEFT` or `FRAMEPOINT_TOPLEFT` (any case).
    pub fn parse(s: &str) -> Option<Point> {
        let up = s.to_ascii_uppercase();
        let up = up.strip_prefix("FRAMEPOINT_").unwrap_or(&up);
        Some(match up {
            "TOPLEFT" => Point::TopLeft,
            "TOP" => Point::Top,
            "TOPRIGHT" => Point::TopRight,
            "LEFT" => Point::Left,
            "CENTER" => Point::Center,
            "RIGHT" => Point::Right,
            "BOTTOMLEFT" => Point::BottomLeft,
            "BOTTOM" => Point::Bottom,
            "BOTTOMRIGHT" => Point::BottomRight,
            _ => return None,
        })
    }

    /// Horizontal edge: LEFT-ish = Low, RIGHT-ish = High, otherwise centre.
    pub fn x_edge(self) -> Edge {
        match self {
            Point::TopLeft | Point::Left | Point::BottomLeft => Edge::Low,
            Point::TopRight | Point::Right | Point::BottomRight => Edge::High,
            Point::Top | Point::Center | Point::Bottom => Edge::Mid,
        }
    }

    /// Vertical edge: BOTTOM-ish = Low, TOP-ish = High, otherwise centre.
    pub fn y_edge(self) -> Edge {
        match self {
            Point::BottomLeft | Point::Bottom | Point::BottomRight => Edge::Low,
            Point::TopLeft | Point::Top | Point::TopRight => Edge::High,
            Point::Left | Point::Center | Point::Right => Edge::Mid,
        }
    }
}

/// What a point is attached to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relative {
    /// The enclosing parent frame (no name given).
    Parent,
    /// A named frame, block or host-supplied external frame.
    Named(String),
}

/// One `SetPoint` (or `Anchor`, or one corner from `SetAllPoints`).
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    /// Point on this frame.
    pub own: Point,
    pub relative: Relative,
    /// Point on the relative frame.
    pub rel_point: Point,
    pub x: f32,
    pub y: f32,
}

/// Reads all anchors in property order.
///
/// `SetAllPoints` adds TOPLEFT and BOTTOMRIGHT to the parent. A later
/// `SetPoint` (or `Anchor`) for the same own point replaces the earlier one.
pub fn anchors(props: &[Property]) -> Result<Vec<Anchor>, PropError> {
    let mut out: Vec<Anchor> = Vec::new();
    for p in props {
        match p.key.to_ascii_uppercase().as_str() {
            "SETALLPOINTS" => {
                upsert(
                    &mut out,
                    Anchor {
                        own: Point::TopLeft,
                        relative: Relative::Parent,
                        rel_point: Point::TopLeft,
                        x: 0.0,
                        y: 0.0,
                    },
                );
                upsert(
                    &mut out,
                    Anchor {
                        own: Point::BottomRight,
                        relative: Relative::Parent,
                        rel_point: Point::BottomRight,
                        x: 0.0,
                        y: 0.0,
                    },
                );
            }
            "SETPOINT" | "ANCHOR" => upsert(&mut out, parse_anchor(p)?),
            _ => {}
        }
    }
    Ok(out)
}

fn upsert(out: &mut Vec<Anchor>, a: Anchor) {
    match out.iter().position(|x| x.own == a.own) {
        Some(i) => out[i] = a,
        None => out.push(a),
    }
}

/// `SetPoint own, ["Name" | Parent], [relPoint], [x], [y]`
/// and `Anchor own, [x], [y]` (relative to the parent, same point).
fn parse_anchor(p: &Property) -> Result<Anchor, PropError> {
    let key = p.key.as_str();
    let v = &p.values;
    let own = v
        .first()
        .and_then(Value::as_text)
        .and_then(Point::parse)
        .ok_or_else(|| PropError::new(key, "first value must be a frame point"))?;
    let mut i = 1;
    let relative = match v.get(i) {
        Some(Value::Str(s)) => {
            i += 1;
            Relative::Named(s.clone())
        }
        Some(Value::Ident(s)) if Point::parse(s).is_none() => {
            i += 1;
            Relative::Named(s.clone())
        }
        _ => Relative::Parent,
    };
    let mut rel_point = own;
    if let Some(Value::Ident(s) | Value::Str(s)) = v.get(i) {
        if let Some(pt) = Point::parse(s) {
            rel_point = pt;
            i += 1;
        }
    }
    let mut nums = Vec::new();
    for val in &v[i..] {
        match val {
            Value::Num(n) => nums.push(*n),
            _ => return Err(PropError::new(key, "unexpected value after the offsets")),
        }
    }
    if nums.len() > 2 {
        return Err(PropError::new(key, "at most two offsets (x, y)"));
    }
    Ok(Anchor {
        own,
        relative,
        rel_point,
        x: nums.first().copied().unwrap_or(0.0),
        y: nums.get(1).copied().unwrap_or(0.0),
    })
}

/// A font as given by `FrameFont` or `Font`.
#[derive(Debug, Clone, PartialEq)]
pub struct FontSpec {
    pub name: String,
    pub size: f32,
}

/// `FrameFont "name", size, "flags"` or `Font "name", size, ...`.
pub fn font(props: &[Property], key: &str) -> Result<Option<FontSpec>, PropError> {
    let Some(p) = last(props, key) else {
        return Ok(None);
    };
    let name = match p.values.first() {
        Some(Value::Str(s) | Value::Ident(s)) => s.clone(),
        _ => return Err(PropError::new(key, "expected a font name first")),
    };
    let size = match p.values.get(1) {
        Some(Value::Num(n)) => *n,
        None => 0.0,
        _ => return Err(PropError::new(key, "expected a size second")),
    };
    Ok(Some(FontSpec { name, size }))
}

/// `FontColor r g b [a]`, RGBA in 0..1. Missing alpha is 1.
pub fn color(props: &[Property], key: &str) -> Result<Option<[f32; 4]>, PropError> {
    let Some(p) = last(props, key) else {
        return Ok(None);
    };
    let mut c = [1.0f32; 4];
    if !(3..=4).contains(&p.values.len()) {
        return Err(PropError::new(key, "expected 3 or 4 numbers"));
    }
    for (slot, v) in c.iter_mut().zip(&p.values) {
        match v {
            Value::Num(n) => *slot = *n,
            _ => return Err(PropError::new(key, "expected numbers")),
        }
    }
    Ok(Some(c))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JustifyH {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JustifyV {
    Top,
    Middle,
    Bottom,
}

/// `FontJustificationH`, default `JUSTIFYLEFT`.
pub fn justify_h(props: &[Property]) -> Result<JustifyH, PropError> {
    let key = "FontJustificationH";
    match text(props, key).map(|s| s.to_ascii_uppercase()).as_deref() {
        None | Some("JUSTIFYLEFT") => Ok(JustifyH::Left),
        Some("JUSTIFYCENTER") => Ok(JustifyH::Center),
        Some("JUSTIFYRIGHT") => Ok(JustifyH::Right),
        Some(other) => Err(PropError::new(
            key,
            format!("unknown justification '{other}'"),
        )),
    }
}

/// `FontJustificationV`, default `JUSTIFYMIDDLE`.
pub fn justify_v(props: &[Property]) -> Result<JustifyV, PropError> {
    let key = "FontJustificationV";
    match text(props, key).map(|s| s.to_ascii_uppercase()).as_deref() {
        None | Some("JUSTIFYMIDDLE") => Ok(JustifyV::Middle),
        Some("JUSTIFYTOP") => Ok(JustifyV::Top),
        Some("JUSTIFYBOTTOM") => Ok(JustifyV::Bottom),
        Some(other) => Err(PropError::new(
            key,
            format!("unknown justification '{other}'"),
        )),
    }
}

/// Texture blend mode (`AlphaMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    Blend,
    Add,
    AlphaKey,
    Disable,
    Mod,
    Mod2x,
    /// Backdrop without `BackdropBlendAll`: texture alpha is ignored.
    Opaque,
}

/// `AlphaMode`, default `BLEND`.
pub fn alpha_mode(props: &[Property]) -> Result<AlphaMode, PropError> {
    let key = "AlphaMode";
    match text(props, key).map(|s| s.to_ascii_uppercase()).as_deref() {
        None | Some("BLEND") => Ok(AlphaMode::Blend),
        Some("ADD") => Ok(AlphaMode::Add),
        Some("ALPHAKEY") => Ok(AlphaMode::AlphaKey),
        Some("DISABLE") => Ok(AlphaMode::Disable),
        Some("MOD") => Ok(AlphaMode::Mod),
        Some("MOD2X") => Ok(AlphaMode::Mod2x),
        Some(other) => Err(PropError::new(key, format!("unknown alpha mode '{other}'"))),
    }
}

/// `TexCoord left, right, top, bottom`, default the whole image.
pub fn tex_coord(props: &[Property]) -> Result<[f32; 4], PropError> {
    match numbers(props, "TexCoord", 4)? {
        Some(v) => Ok([v[0], v[1], v[2], v[3]]),
        None => Ok([0.0, 1.0, 0.0, 1.0]),
    }
}
