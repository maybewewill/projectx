//! GEOA: geoset animations (per-geoset alpha and color, optionally keyframed).
//!
//! Record layout (inclusive size first), from public format notes and **unverified** against
//! real 1.26 files: f32 static alpha, u32 flags (stored raw), vec3 static color, i32 geoset id,
//! then optional KGAO (float) and KGAC (vec3) tracks.

use crate::anim::{put, read_track, Track};
use crate::reader::Reader;
use crate::Error;

/// One GEOA entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GeosetAnim {
    /// Static alpha, used when there is no KGAO track.
    pub alpha: f32,
    /// Raw flags. Meanings **unverified**.
    pub flags: u32,
    /// Static color, used when there is no KGAC track. Default white in most models.
    pub color: [f32; 3],
    /// Index of the geoset this entry drives, or -1.
    pub geoset_id: i32,
    /// KGAO: animated alpha.
    pub alpha_track: Option<Track<f32>>,
    /// KGAC: animated color.
    pub color_track: Option<Track<[f32; 3]>>,
}

pub(crate) fn parse_all(p: &mut Reader) -> Result<Vec<GeosetAnim>, Error> {
    let mut out = Vec::new();
    while !p.is_empty() {
        let mut rec = p.record("geoset animation")?;
        let alpha = rec.f32()?;
        let flags = rec.u32()?;
        let color = rec.vec3()?;
        let geoset_id = rec.i32()?;
        let mut alpha_track = None;
        let mut color_track = None;
        while let Some(tag) = rec.peek_tag() {
            match &tag {
                b"KGAO" => put(
                    &mut alpha_track,
                    &tag,
                    read_track(&mut rec, &tag, 4, |r| r.f32())?,
                )?,
                b"KGAC" => put(
                    &mut color_track,
                    &tag,
                    read_track(&mut rec, &tag, 12, |r| r.vec3())?,
                )?,
                // Unknown tracks have no size field, so the rest of the record is skipped.
                _ => break,
            }
        }
        out.push(GeosetAnim {
            alpha,
            flags,
            color,
            geoset_id,
            alpha_track,
            color_track,
        });
    }
    Ok(out)
}
