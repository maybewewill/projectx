//! MTLS: materials and their LAYS layers (version 800: no shader field).

use crate::anim::{opt_index, put, read_track, Track};
use crate::reader::Reader;
use crate::Error;

/// A material: a priority plane, flags and a stack of layers.
///
/// `flags` is stored raw. Bit meanings (constant color, sort far-Z, full resolution) come
/// from public notes and are **not verified**.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub priority_plane: i32,
    pub flags: u32,
    pub layers: Vec<Layer>,
}

/// One texture layer of a material.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Raw filter mode: 0 none, 1 transparent, 2 blend, 3 additive, 4 add-alpha, 5 modulate,
    /// 6 modulate 2x (from public notes, **unverified** for the 800 layout).
    pub filter_mode: u32,
    /// Raw shading flags (unshaded, sphere env map, two-sided, unfogged, ...). Stored raw.
    pub shading_flags: u32,
    /// Index into the TEXS list.
    pub texture_id: u32,
    /// Index of a texture animation (TXAN chunk), or `None` for raw -1.
    pub texture_anim_id: Option<u32>,
    pub coord_id: u32,
    /// Static alpha, used when there is no alpha track.
    pub alpha: f32,
    /// KMTA: animated alpha (float values).
    pub alpha_track: Option<Track<f32>>,
    /// KMTF: animated texture id (integer values). The tangent layout is **unverified**.
    pub texture_track: Option<Track<u32>>,
}

pub(crate) fn parse_all(p: &mut Reader) -> Result<Vec<Material>, Error> {
    let mut out = Vec::new();
    while !p.is_empty() {
        let mut rec = p.record("material")?;
        let priority_plane = rec.i32()?;
        let flags = rec.u32()?;
        rec.tag(b"LAYS")?;
        let n = rec.count(4, "layers")?;
        let mut layers = Vec::with_capacity(n);
        for _ in 0..n {
            layers.push(parse_layer(&mut rec)?);
        }
        rec.finish("material")?;
        out.push(Material {
            priority_plane,
            flags,
            layers,
        });
    }
    Ok(out)
}

fn parse_layer(r: &mut Reader) -> Result<Layer, Error> {
    let mut l = r.record("layer")?;
    let filter_mode = l.u32()?;
    let shading_flags = l.u32()?;
    let texture_id = l.u32()?;
    let offset = l.offset();
    let texture_anim_id = opt_index(l.i32()?, "texture animation id", offset)?;
    let coord_id = l.u32()?;
    let alpha = l.f32()?;
    let mut alpha_track = None;
    let mut texture_track = None;
    while let Some(tag) = l.peek_tag() {
        match &tag {
            b"KMTA" => put(
                &mut alpha_track,
                &tag,
                read_track(&mut l, &tag, 4, |r| r.f32())?,
            )?,
            b"KMTF" => put(
                &mut texture_track,
                &tag,
                read_track(&mut l, &tag, 4, |r| r.u32())?,
            )?,
            // Unknown tracks have no size field, so the rest of the layer is skipped.
            _ => break,
        }
    }
    l.finish("layer")?;
    Ok(Layer {
        filter_mode,
        shading_flags,
        texture_id,
        texture_anim_id,
        coord_id,
        alpha,
        alpha_track,
        texture_track,
    })
}
