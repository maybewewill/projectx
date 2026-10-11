//! Vertex skinning, the way WC3 does it: each vertex names a matrix group (GNDX), a matrix
//! group is a list of bones (MTGC counts, MATS object ids), and the vertex is transformed by
//! the average of the bone matrices of its group. Averaging the matrices is the same as
//! averaging the transformed points, because the transform is linear in the matrix.
//!
//! This is the GPU-free reference. The viewer uses the same rule on the GPU.

use glam::{Mat3, Mat4, Vec3};

use crate::Geoset;

/// Object ids of the bones of each matrix group, from MTGC and MATS.
///
/// Group `g` takes the next `MTGC[g]` entries of MATS. A truncated MATS list gives shorter
/// groups instead of an error.
pub fn matrix_groups(geoset: &Geoset) -> Vec<Vec<u32>> {
    let mut out = Vec::with_capacity(geoset.matrix_groups.len());
    let mut next = 0usize;
    for &count in &geoset.matrix_groups {
        let end = next
            .saturating_add(count as usize)
            .min(geoset.matrices.len());
        out.push(geoset.matrices[next.min(end)..end].to_vec());
        next = end;
    }
    out
}

/// Element-wise average of matrices. Empty input gives the identity.
pub fn average(matrices: &[Mat4]) -> Mat4 {
    if matrices.is_empty() {
        return Mat4::IDENTITY;
    }
    let sum = matrices.iter().fold(Mat4::ZERO, |acc, m| acc + *m);
    sum * (1.0 / matrices.len() as f32)
}

/// Transforms one vertex. The normal uses the upper 3x3 of the matrix and is renormalized.
pub fn skin_vertex(m: Mat4, position: [f32; 3], normal: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let p = m.transform_point3(Vec3::from_array(position));
    let n = (Mat3::from_mat4(m) * Vec3::from_array(normal)).normalize_or_zero();
    (p.to_array(), n.to_array())
}

/// Skins every vertex of `geoset`. `group_matrices[g]` is the averaged matrix of matrix group
/// `g`. A vertex whose GNDX is out of range keeps the identity. A missing normal becomes +Z.
pub fn skin_geoset(geoset: &Geoset, group_matrices: &[Mat4]) -> Vec<([f32; 3], [f32; 3])> {
    geoset
        .vertices
        .iter()
        .enumerate()
        .map(|(i, &position)| {
            let normal = geoset.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
            let m = geoset
                .vertex_groups
                .get(i)
                .and_then(|&g| group_matrices.get(usize::from(g)).copied())
                .unwrap_or(Mat4::IDENTITY);
            skin_vertex(m, position, normal)
        })
        .collect()
}
