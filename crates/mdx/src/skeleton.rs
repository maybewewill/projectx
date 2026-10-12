//! Node hierarchy and pose evaluation.
//!
//! Frame convention (**assumption**, consistent with the pivot semantics of PIVT): a node's
//! pivot `p` is its rest origin in model space. Its local transform is
//! `T(p - p_parent) * T(t) * R(r) * S(s)`, where `t`, `r`, `s` are the animated values
//! (identity without tracks). Its world matrix is `world(parent) * local`. A node's skinning
//! matrix is `world * T(-p)`, which is the identity at rest, so bind-pose vertices stay put.
//!
//! Billboards (flags are from public notes, **unverified**): 0x8 billboarded, 0x10/0x20/0x40
//! lock X/Y/Z. A billboarded node keeps the translation of its parent chain and takes the
//! camera's rotation. With a lock flag the locked world axis stays fixed and the node turns
//! around it to face the camera (priority Z, then X, then Y when several are set).

use std::collections::BTreeMap;

use glam::{Mat3, Mat4, Quat, Vec3};

use crate::eval::{rotation_quat, vec3};
use crate::{Model, Node};

/// Node flag: billboarded.
pub const BILLBOARD: u32 = 0x8;
/// Node flag: lock the X axis (billboard keeps it).
pub const BILLBOARD_LOCK_X: u32 = 0x10;
/// Node flag: lock the Y axis.
pub const BILLBOARD_LOCK_Y: u32 = 0x20;
/// Node flag: lock the Z axis (upright billboard).
pub const BILLBOARD_LOCK_Z: u32 = 0x40;

/// Errors while building the node hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkeletonError {
    #[error("two nodes share object id {0}")]
    DuplicateObject(i32),
    #[error("parent chain of object {0} loops back to itself")]
    Cycle(i32),
}

/// The camera data a billboard needs. `right`, `up` and `to_viewer` are unit vectors in
/// world space and form a right-handed basis (`right x up = to_viewer`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BillboardCamera {
    pub right: Vec3,
    pub up: Vec3,
    pub to_viewer: Vec3,
}

impl BillboardCamera {
    /// Builds the basis from the view direction and a world up vector (usually +Z).
    /// Returns `None` when the direction is parallel to `up` or zero.
    pub fn from_view(forward: Vec3, world_up: Vec3) -> Option<BillboardCamera> {
        let forward = forward.try_normalize()?;
        let right = forward.cross(world_up).try_normalize()?;
        let to_viewer = -forward;
        let up = to_viewer.cross(right);
        Some(BillboardCamera {
            right,
            up,
            to_viewer,
        })
    }
}

/// One node of the hierarchy with its pivot and parent index.
#[derive(Debug, Clone, PartialEq)]
struct SkelNode {
    node: Node,
    pivot: Vec3,
    parent: Option<usize>,
}

/// The node hierarchy of a model: BONE nodes followed by HELP nodes, reordered so that parents
/// come before children. Unknown parent ids make a node a root.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    nodes: Vec<SkelNode>,
    index: BTreeMap<i32, usize>,
}

/// Animated state of one node.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePose {
    pub object_id: i32,
    /// Node-to-model matrix.
    pub world: Mat4,
    /// Skinning matrix: `world * T(-pivot)`. Identity at rest.
    pub skin: Mat4,
    /// KATV value (1.0 without a track).
    pub visibility: f32,
}

/// Animated alpha and texture id of one material layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerPose {
    pub alpha: f32,
    /// TEXS index after KMTF (the static id without a track).
    pub texture_id: u32,
}

/// Animated state of one GEOA entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeosetPose {
    pub geoset_id: i32,
    pub alpha: f32,
    pub color: [f32; 3],
}

/// Everything a renderer needs for one moment of one sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct Pose {
    /// Parallel to [`Skeleton`] node order.
    pub nodes: Vec<NodePose>,
    /// `materials[m][l]` is layer `l` of material `m`.
    pub materials: Vec<Vec<LayerPose>>,
    /// Parallel to `Model::geoset_anims`.
    pub geoset_anims: Vec<GeosetPose>,
    index: BTreeMap<i32, usize>,
}

impl Pose {
    /// Skinning matrix of the node with `object_id`, if the model has such a node.
    pub fn skin_matrix(&self, object_id: i32) -> Option<Mat4> {
        self.index.get(&object_id).map(|&i| self.nodes[i].skin)
    }

    /// Animated alpha and color of geoset `geoset_id`, if a GEOA entry drives it.
    pub fn geoset_state(&self, geoset_id: usize) -> Option<GeosetPose> {
        self.geoset_anims
            .iter()
            .find(|g| g.geoset_id == geoset_id as i32)
            .copied()
    }
}

impl Skeleton {
    /// Builds the hierarchy from the BONE and HELP chunks of `model`.
    pub fn new(model: &Model) -> Result<Skeleton, SkeletonError> {
        let raw: Vec<Node> = model
            .bones
            .iter()
            .map(|b| b.node.clone())
            .chain(model.helpers.iter().cloned())
            .collect();

        let mut by_object = BTreeMap::new();
        for (i, node) in raw.iter().enumerate() {
            if by_object.insert(node.object_id, i).is_some() {
                return Err(SkeletonError::DuplicateObject(node.object_id));
            }
        }
        let parents: Vec<Option<usize>> = raw
            .iter()
            .map(|n| by_object.get(&n.parent_id).copied())
            .collect();

        // Depth-first order: parents first. 0 = unvisited, 1 = on the stack, 2 = done.
        let mut state = vec![0u8; raw.len()];
        let mut order = Vec::with_capacity(raw.len());
        for start in 0..raw.len() {
            visit(start, &parents, &raw, &mut state, &mut order)?;
        }

        let mut new_index = vec![0usize; raw.len()];
        for (pos, &old) in order.iter().enumerate() {
            new_index[old] = pos;
        }
        let mut raw: Vec<Option<Node>> = raw.into_iter().map(Some).collect();
        let nodes: Vec<SkelNode> = order
            .iter()
            .map(|&old| {
                let node = raw[old].take().expect("each node is taken once");
                let pivot = usize::try_from(node.object_id)
                    .ok()
                    .and_then(|i| model.pivots.get(i).copied())
                    .map(vec3)
                    .unwrap_or(Vec3::ZERO);
                SkelNode {
                    node,
                    pivot,
                    parent: parents[old].map(|p| new_index[p]),
                }
            })
            .collect();
        let index = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.node.object_id, i))
            .collect();
        Ok(Skeleton { nodes, index })
    }

    /// Number of nodes (bones plus helpers).
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when the model has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Pivot (model space) of the node with `object_id`.
    pub fn pivot(&self, object_id: i32) -> Option<Vec3> {
        self.index.get(&object_id).map(|&i| self.nodes[i].pivot)
    }

    /// Pose at the bind pose: every track ignored.
    pub fn bind_pose(&self, model: &Model) -> Pose {
        self.evaluate_at(model, None, None)
    }

    /// Pose of `sequence` at `elapsed_ms` since its start (looping or clamped as the sequence
    /// says). `camera` is needed only for billboarded nodes.
    pub fn evaluate_sequence(
        &self,
        model: &Model,
        sequence: usize,
        elapsed_ms: f32,
        camera: Option<&BillboardCamera>,
    ) -> Option<Pose> {
        let seq = model.sequences.get(sequence)?;
        let time = seq.time_at(elapsed_ms);
        let mut pose = self.evaluate_at(model, Some(time), camera);
        // Geoset alpha/color: only keys inside this sequence's interval count.
        let globals = &model.global_sequences;
        for (g, p) in model.geoset_anims.iter().zip(pose.geoset_anims.iter_mut()) {
            p.alpha = g
                .alpha_track
                .as_ref()
                .and_then(|k| k.sample_in(time, seq.start, seq.end, globals))
                .unwrap_or(g.alpha);
            p.color = g
                .color_track
                .as_ref()
                .and_then(|k| k.sample_in(time, seq.start, seq.end, globals))
                .unwrap_or(g.color);
        }
        Some(pose)
    }

    /// Pose at an absolute time in milliseconds. `None` is the bind pose.
    pub fn evaluate_at(
        &self,
        model: &Model,
        time: Option<u32>,
        camera: Option<&BillboardCamera>,
    ) -> Pose {
        let globals = &model.global_sequences;
        let mut world: Vec<Mat4> = Vec::with_capacity(self.nodes.len());
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for sn in &self.nodes {
            let n = &sn.node;
            let t = time.and_then(|t| n.translation.as_ref()?.sample(t, globals));
            let r = time.and_then(|t| n.rotation.as_ref()?.sample(t, globals));
            let s = time.and_then(|t| n.scaling.as_ref()?.sample(t, globals));
            let visibility = time
                .and_then(|t| n.visibility.as_ref()?.sample(t, globals))
                .unwrap_or(1.0);
            let trans = t.map(vec3).unwrap_or(Vec3::ZERO);
            let rot = r.map(rotation_quat).unwrap_or(Quat::IDENTITY);
            let scale = s.map(vec3).unwrap_or(Vec3::ONE);

            let (parent_world, parent_pivot) = match sn.parent {
                Some(p) => (world[p], self.nodes[p].pivot),
                None => (Mat4::IDENTITY, Vec3::ZERO),
            };
            let local = Mat4::from_translation(sn.pivot - parent_pivot)
                * Mat4::from_translation(trans)
                * Mat4::from_quat(rot)
                * Mat4::from_scale(scale);
            let mut w = parent_world * local;

            if let Some(cam) = camera {
                if n.flags & (BILLBOARD | BILLBOARD_LOCK_X | BILLBOARD_LOCK_Y | BILLBOARD_LOCK_Z)
                    != 0
                {
                    let origin = w.w_axis.truncate();
                    let basis = billboard_basis(cam, n.flags);
                    w = Mat4::from_translation(origin)
                        * Mat4::from_mat3(basis)
                        * Mat4::from_scale(scale);
                }
            }
            world.push(w);
            nodes.push(NodePose {
                object_id: n.object_id,
                world: w,
                skin: w * Mat4::from_translation(-sn.pivot),
                visibility,
            });
        }

        let materials = model
            .materials
            .iter()
            .map(|m| {
                m.layers
                    .iter()
                    .map(|l| LayerPose {
                        alpha: match time {
                            Some(t) => l
                                .alpha_track
                                .as_ref()
                                .and_then(|k| k.sample(t, globals))
                                .unwrap_or(l.alpha),
                            None => l.alpha,
                        },
                        texture_id: match time {
                            Some(t) => l
                                .texture_track
                                .as_ref()
                                .and_then(|k| k.sample(t, globals))
                                .unwrap_or(l.texture_id),
                            None => l.texture_id,
                        },
                    })
                    .collect()
            })
            .collect();

        let geoset_anims = model
            .geoset_anims
            .iter()
            .map(|g| GeosetPose {
                geoset_id: g.geoset_id,
                alpha: match time {
                    Some(t) => g
                        .alpha_track
                        .as_ref()
                        .and_then(|k| k.sample(t, globals))
                        .unwrap_or(g.alpha),
                    None => g.alpha,
                },
                color: match time {
                    Some(t) => g
                        .color_track
                        .as_ref()
                        .and_then(|k| k.sample(t, globals))
                        .unwrap_or(g.color),
                    None => g.color,
                },
            })
            .collect();

        Pose {
            nodes,
            materials,
            geoset_anims,
            index: self.index.clone(),
        }
    }
}

fn visit(
    i: usize,
    parents: &[Option<usize>],
    raw: &[Node],
    state: &mut [u8],
    order: &mut Vec<usize>,
) -> Result<(), SkeletonError> {
    match state[i] {
        2 => return Ok(()),
        1 => return Err(SkeletonError::Cycle(raw[i].object_id)),
        _ => {}
    }
    state[i] = 1;
    if let Some(p) = parents[i] {
        visit(p, parents, raw, state, order)?;
    }
    state[i] = 2;
    order.push(i);
    Ok(())
}

/// Rotation (3x3, columns are the node axes) that billboards a node toward the camera.
pub fn billboard_basis(cam: &BillboardCamera, flags: u32) -> Mat3 {
    if flags & BILLBOARD_LOCK_Z != 0 {
        locked_basis(cam, 2)
    } else if flags & BILLBOARD_LOCK_X != 0 {
        locked_basis(cam, 0)
    } else if flags & BILLBOARD_LOCK_Y != 0 {
        locked_basis(cam, 1)
    } else {
        // Node X, Y and Z become camera right, up and back (toward the viewer).
        Mat3::from_cols(cam.right, cam.up, cam.to_viewer)
    }
}

/// Keeps world axis `lock` (0 = X, 1 = Y, 2 = Z) and faces the node toward the camera.
fn locked_basis(cam: &BillboardCamera, lock: usize) -> Mat3 {
    let a = [Vec3::X, Vec3::Y, Vec3::Z][lock];
    let project = |v: Vec3| v - a * v.dot(a);
    let facing = project(cam.to_viewer);
    let facing = if facing.length_squared() > 1e-10 {
        facing
    } else {
        project(cam.up)
    };
    let Some(f) = facing.try_normalize() else {
        return Mat3::IDENTITY;
    };
    match lock {
        // X fixed: Y faces the camera, Z = X x Y.
        0 => Mat3::from_cols(a, f, a.cross(f)),
        // Y fixed: Z faces the camera, X = Y x Z.
        1 => Mat3::from_cols(a.cross(f), a, f),
        // Z fixed: Y faces the camera, X = Y x Z.
        _ => Mat3::from_cols(f.cross(a), f, a),
    }
}
