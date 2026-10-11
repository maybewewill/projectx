//! GEOS: triangle meshes with skinning data and UV sets.

use crate::reader::Reader;
use crate::{Error, Extent};

/// PTYP value for triangle lists.
pub const PRIMITIVE_TRIANGLES: u32 = 4;

/// One primitive group: `kind` is the raw PTYP value and `indices` are the PVTX entries of
/// this group. Every index is validated to be below the geoset's vertex count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Primitive {
    pub kind: u32,
    pub indices: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Geoset {
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub primitives: Vec<Primitive>,
    /// GNDX: per-vertex matrix group index. Stored raw, **mapping unverified**.
    pub vertex_groups: Vec<u8>,
    /// MTGC: number of matrices in each matrix group.
    pub matrix_groups: Vec<u32>,
    /// MATS: flat list of object ids for the matrix groups.
    pub matrices: Vec<u32>,
    /// Index into MTLS.
    pub material_id: u32,
    /// Selection group. Semantics **unverified**.
    pub selection_group: u32,
    /// Selection flags (unselectable bit, etc.). Stored raw, **unverified**.
    pub selection_flags: u32,
    pub extent: Extent,
    /// One extent per sequence, in SEQS order.
    pub sequence_extents: Vec<Extent>,
    /// UVAS/UVBS: texture coordinate sets, one `[u, v]` per vertex each.
    pub uv_sets: Vec<Vec<[f32; 2]>>,
}

impl Geoset {
    /// Vertex index triples of all triangle primitives (PTYP = 4), in file order.
    /// Other primitive kinds are skipped.
    pub fn triangles(&self) -> Vec<[u16; 3]> {
        self.primitives
            .iter()
            .filter(|p| p.kind == PRIMITIVE_TRIANGLES)
            .flat_map(|p| p.indices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]))
            .collect()
    }
}

pub(crate) fn parse_all(p: &mut Reader) -> Result<Vec<Geoset>, Error> {
    let mut out = Vec::new();
    while !p.is_empty() {
        let mut rec = p.record("geoset")?;
        out.push(parse_one(&mut rec)?);
    }
    Ok(out)
}

fn parse_one(r: &mut Reader) -> Result<Geoset, Error> {
    let vertices = r.tagged_array(b"VRTX", 12, "vertices", |r| r.vec3())?;
    let normals = r.tagged_array(b"NRMS", 12, "normals", |r| r.vec3())?;
    let types = r.tagged_array(b"PTYP", 4, "primitive types", |r| r.u32())?;
    let counts = r.tagged_array(b"PCNT", 4, "primitive counts", |r| r.u32())?;
    let indices = r.tagged_array(b"PVTX", 2, "primitive indices", |r| r.u16())?;
    let vertex_groups = r.tagged_array(b"GNDX", 1, "vertex groups", |r| r.u8())?;
    let matrix_groups = r.tagged_array(b"MTGC", 4, "matrix groups", |r| r.u32())?;
    let matrices = r.tagged_array(b"MATS", 4, "matrices", |r| r.u32())?;
    let primitives = split_primitives(r.offset(), &types, &counts, &indices, vertices.len())?;

    let material_id = r.u32()?;
    let selection_group = r.u32()?;
    let selection_flags = r.u32()?;
    let extent = Extent::read(r)?;
    let n = r.count(28, "sequence extents")?;
    let mut sequence_extents = Vec::with_capacity(n);
    for _ in 0..n {
        sequence_extents.push(Extent::read(r)?);
    }

    // The UV block is optional. If present, it is UVAS (set count) followed by UVBS per set.
    let mut uv_sets = Vec::new();
    if r.peek_tag() == Some(*b"UVAS") {
        r.tag(b"UVAS")?;
        let n = r.count(8, "uv sets")?;
        for _ in 0..n {
            uv_sets.push(r.tagged_array(b"UVBS", 8, "uv coordinates", |r| r.vec2())?);
        }
    }

    r.finish("geoset")?;
    Ok(Geoset {
        vertices,
        normals,
        primitives,
        vertex_groups,
        matrix_groups,
        matrices,
        material_id,
        selection_group,
        selection_flags,
        extent,
        sequence_extents,
        uv_sets,
    })
}

/// Splits the flat PVTX list into primitive groups using the PCNT counts.
fn split_primitives(
    offset: usize,
    types: &[u32],
    counts: &[u32],
    indices: &[u16],
    vertex_count: usize,
) -> Result<Vec<Primitive>, Error> {
    let malformed = |reason: &'static str| Error::Malformed {
        what: "geoset primitives",
        offset,
        reason,
    };
    if types.len() != counts.len() {
        return Err(malformed("PTYP and PCNT have different counts"));
    }
    let mut total: usize = 0;
    for &c in counts {
        total = total
            .checked_add(c as usize)
            .ok_or_else(|| malformed("primitive index total overflows"))?;
    }
    if total != indices.len() {
        return Err(malformed("PCNT sum does not match the PVTX count"));
    }
    let mut out = Vec::with_capacity(types.len());
    let mut start = 0;
    for (&kind, &count) in types.iter().zip(counts) {
        let n = count as usize;
        // `start + n <= total == indices.len()` by construction.
        let slice = &indices[start..start + n];
        start += n;
        if let Some(&bad) = slice.iter().find(|&&i| usize::from(i) >= vertex_count) {
            return Err(Error::InvalidValue {
                what: "primitive vertex index",
                offset,
                value: i64::from(bad),
            });
        }
        if kind == PRIMITIVE_TRIANGLES && !n.is_multiple_of(3) {
            return Err(malformed("triangle index count is not a multiple of 3"));
        }
        out.push(Primitive {
            kind,
            indices: slice.to_vec(),
        });
    }
    Ok(out)
}
