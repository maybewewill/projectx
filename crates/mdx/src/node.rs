//! BONE and HELP nodes: the skeleton and helper objects with their animation tracks.

use crate::anim::{put, read_track, Track};
use crate::reader::Reader;
use crate::Error;

/// Common node data: name, object id, parent, flags and optional animated channels.
///
/// The object id is the index used by PIVT and by geoset matrix lists.
/// `flags` is stored raw. Public notes give 0x100 for bones, but this is **not verified**.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub object_id: i32,
    /// Object id of the parent node, or -1 for a root.
    pub parent_id: i32,
    pub flags: u32,
    /// KGTR: translation (vec3).
    pub translation: Option<Track<[f32; 3]>>,
    /// KGRT: rotation (quaternion, stored as x, y, z, w).
    pub rotation: Option<Track<[f32; 4]>>,
    /// KGSC: scale (vec3).
    pub scaling: Option<Track<[f32; 3]>>,
    /// KATV: visibility (float).
    pub visibility: Option<Track<f32>>,
}

/// A bone: a node plus the geoset links that are only present on bones.
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub node: Node,
    /// Geoset this bone belongs to, or -1. **Semantics unverified.**
    pub geoset_id: i32,
    /// Geoset animation this bone is driven by, or -1. **Semantics unverified.**
    pub geoset_anim_id: i32,
}

pub(crate) fn parse_bones(p: &mut Reader) -> Result<Vec<Bone>, Error> {
    let mut out = Vec::new();
    while !p.is_empty() {
        // A bone is a node record (inclusive size, header, tracks) followed by the two geoset ids OUTSIDE it: the size of
        // the node does not cover them (found on the real models, where the old whole-bone size rule broke every BONE).
        let mut rec = p.record("bone")?;
        let mut node = read_header(&mut rec)?;
        read_tracks(&mut rec, &mut node)?;
        let geoset_id = p.i32()?;
        let geoset_anim_id = p.i32()?;
        out.push(Bone {
            node,
            geoset_id,
            geoset_anim_id,
        });
    }
    Ok(out)
}

pub(crate) fn parse_helpers(p: &mut Reader) -> Result<Vec<Node>, Error> {
    let mut out = Vec::new();
    while !p.is_empty() {
        let mut rec = p.record("helper")?;
        let mut node = read_header(&mut rec)?;
        read_tracks(&mut rec, &mut node)?;
        out.push(node);
    }
    Ok(out)
}

fn read_header(r: &mut Reader) -> Result<Node, Error> {
    Ok(Node {
        name: r.fixed_str(80)?,
        object_id: r.i32()?,
        parent_id: r.i32()?,
        flags: r.u32()?,
        translation: None,
        rotation: None,
        scaling: None,
        visibility: None,
    })
}

/// Reads the known node tracks. The first unknown tag ends the list: the rest of the node
/// is skipped, because track records carry no size.
fn read_tracks(r: &mut Reader, node: &mut Node) -> Result<(), Error> {
    while let Some(tag) = r.peek_tag() {
        match &tag {
            b"KGTR" => put(
                &mut node.translation,
                &tag,
                read_track(r, &tag, 12, |r| r.vec3())?,
            )?,
            b"KGRT" => put(
                &mut node.rotation,
                &tag,
                read_track(r, &tag, 16, |r| r.quat())?,
            )?,
            b"KGSC" => put(
                &mut node.scaling,
                &tag,
                read_track(r, &tag, 12, |r| r.vec3())?,
            )?,
            b"KATV" => put(
                &mut node.visibility,
                &tag,
                read_track(r, &tag, 4, |r| r.f32())?,
            )?,
            _ => break,
        }
    }
    Ok(())
}
