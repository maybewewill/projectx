//! Parser for Warcraft III binary MDX models (format version 800, patch 1.26).
//!
//! An MDX file starts with the magic `MDLX` and continues with chunks laid out as
//! `[4-byte tag][u32 size][payload]`. Records inside a chunk that carry an inclusive size
//! (the size counts its own four bytes) are checked against the bytes left in the chunk.
//!
//! Layouts follow public community format notes (mdx-m3-viewer, Magos and similar tools).
//! Fields that are not confirmed against real 1.26 files are marked **uncertain** or
//! **unverified** in their doc comments. Parsing is bounds-checked: malformed input returns
//! an [`Error`], never a panic.
//!
//! Chunks that are not parsed (ATCH, PREM, PRE2, RIBB, EVTS, CAMS, CLID, LITE, GEOA, TXAN,
//! and anything unknown) are kept as [`RawChunk`] with their tag.

mod anim;
mod eval;
mod geoanim;
mod geoset;
mod material;
mod node;
mod reader;
mod skeleton;
pub mod skin;

pub use anim::{Interpolation, Key, Track};
pub use eval::{track_time, Keyable};
pub use geoanim::GeosetAnim;
pub use geoset::{Geoset, Primitive, PRIMITIVE_TRIANGLES};
pub use material::{Layer, Material};
pub use node::{Bone, Node};
pub use skeleton::{
    billboard_basis, BillboardCamera, GeosetPose, LayerPose, NodePose, Pose, Skeleton,
    SkeletonError, BILLBOARD, BILLBOARD_LOCK_X, BILLBOARD_LOCK_Y, BILLBOARD_LOCK_Z,
};

use anim::put;
use reader::Reader;
pub use glam;

/// A four-character chunk tag.
pub type Tag = [u8; 4];

/// The only format version accepted (patch 1.26).
pub const VERSION: u32 = 800;

const MAGIC: &[u8; 4] = b"MDLX";

/// Errors returned by [`Model::parse`]. Offsets are absolute byte offsets in the file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("bad magic {found:02x?}, expected \"MDLX\"")]
    BadMagic { found: [u8; 4] },
    #[error("unexpected end of data at offset {offset:#x} (wanted {wanted} bytes)")]
    UnexpectedEof { offset: usize, wanted: usize },
    #[error("chunk {tag} at offset {offset:#x} claims {size} bytes but only {available} remain")]
    ChunkOverrun {
        tag: String,
        offset: usize,
        size: usize,
        available: usize,
    },
    #[error("{what} record at offset {offset:#x} has invalid inclusive size {size}")]
    RecordSize {
        what: &'static str,
        offset: usize,
        size: usize,
    },
    #[error("expected tag {expected} at offset {offset:#x}, found {found}")]
    BadTag {
        expected: String,
        found: String,
        offset: usize,
    },
    #[error("{what} count {count} at offset {offset:#x} does not fit in the remaining data")]
    CountTooLarge {
        what: &'static str,
        count: usize,
        offset: usize,
    },
    #[error("{what} at offset {offset:#x} has invalid value {value}")]
    InvalidValue {
        what: &'static str,
        offset: usize,
        value: i64,
    },
    #[error("{what} at offset {offset:#x} is malformed: {reason}")]
    Malformed {
        what: &'static str,
        offset: usize,
        reason: &'static str,
    },
    #[error("chunk {tag}: size {size} is not a multiple of the record size {elem}")]
    SizeMismatch {
        tag: String,
        size: usize,
        elem: usize,
    },
    #[error("unsupported MDX version {found} (expected {VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("missing required chunk {tag}")]
    MissingChunk { tag: String },
    #[error("duplicate chunk {tag}")]
    DuplicateChunk { tag: String },
}

/// Bounding volume: radius plus an axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    pub radius: f32,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// MODL: model name, global bounds and blend time.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    pub name: String,
    pub extent: Extent,
    /// Blend time between animations, in milliseconds.
    pub blend_time: u32,
}

/// One animation sequence from SEQS.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    /// Start and end time, in milliseconds.
    pub start: u32,
    pub end: u32,
    pub move_speed: f32,
    /// Raw flags. Bit 0 set means non-looping; see [`Sequence::is_looping`]. Only the values 0
    /// and 1 occur in the shipped 1.26 models, and the names agree (Death, Attack, Birth and
    /// Decay are set; Stand, Walk and Portrait are clear).
    pub flags: u32,
    /// Selection weight when several sequences share a name (**semantics uncertain**).
    pub rarity: f32,
    pub sync_point: u32,
    pub extent: Extent,
}

impl Sequence {
    /// True unless flag bit 0 is set.
    pub fn is_looping(&self) -> bool {
        self.flags & 1 == 0
    }

    /// Length of the sequence in milliseconds.
    pub fn duration(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Absolute time (ms, the same clock as the keys) reached `elapsed_ms` after the start.
    /// Looping sequences wrap; others stop at the end.
    pub fn time_at(&self, elapsed_ms: f32) -> u32 {
        let len = self.duration();
        if len == 0 {
            return self.start;
        }
        let elapsed = elapsed_ms.max(0.0);
        let offset = if self.is_looping() {
            elapsed % len as f32
        } else {
            elapsed.min(len as f32)
        };
        self.start + offset.floor() as u32
    }
}

/// One entry of TEXS. Records are 268 bytes: replaceable id, a 260-byte path, flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// 0 for a file path. Non-zero values are replaceable textures (team color, glow, ...).
    pub replaceable_id: u32,
    /// Path of the BLP file (260-byte field, NUL-terminated).
    pub path: String,
    /// Wrap flags: bit 0 = wrap width, bit 1 = wrap height. Shipped models use 0 to 3; the
    /// bit meaning is taken from public notes and is not checked against the renderer.
    pub flags: u32,
}

/// A chunk this crate does not parse, kept byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawChunk {
    pub tag: Tag,
    pub data: Vec<u8>,
}

/// A parsed MDX model.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub version: u32,
    pub info: ModelInfo,
    pub sequences: Vec<Sequence>,
    /// GLBS: global sequence lengths, in milliseconds.
    pub global_sequences: Vec<u32>,
    pub textures: Vec<Texture>,
    pub materials: Vec<Material>,
    pub geosets: Vec<Geoset>,
    /// BONE chunk: skeleton nodes.
    pub bones: Vec<Bone>,
    /// HELP chunk: helper nodes (no geoset links).
    pub helpers: Vec<Node>,
    /// PIVT: pivot point per object id.
    pub pivots: Vec<[f32; 3]>,
    /// GEOA: geoset animations (alpha and color per geoset), in file order.
    pub geoset_anims: Vec<GeosetAnim>,
    /// Chunks that are not parsed, in file order.
    pub unknown_chunks: Vec<RawChunk>,
}

impl Model {
    /// Parses a complete MDX file.
    ///
    /// Required chunks are `VERS` (must be 800) and `MODL`. Missing optional chunks give
    /// empty collections. Duplicate chunks are an error.
    pub fn parse(data: &[u8]) -> Result<Model, Error> {
        let mut r = Reader::new(data, 0);
        let magic = r.array::<4>()?;
        if &magic != MAGIC {
            return Err(Error::BadMagic { found: magic });
        }

        let mut version = None;
        let mut info = None;
        let mut sequences = None;
        let mut global_sequences = None;
        let mut textures = None;
        let mut materials = None;
        let mut geosets = None;
        let mut bones = None;
        let mut helpers = None;
        let mut pivots = None;
        let mut geoset_anims = None;
        let mut unknown_chunks = Vec::new();

        while !r.is_empty() {
            let offset = r.offset();
            let tag = r.array::<4>()?;
            let size = r.u32()? as usize;
            if size > r.remaining() {
                return Err(Error::ChunkOverrun {
                    tag: fourcc(tag),
                    offset,
                    size,
                    available: r.remaining(),
                });
            }
            let payload = r.bytes(size)?;
            let mut p = Reader::new(payload, offset + 8);
            match &tag {
                b"VERS" => put(&mut version, &tag, p.u32()?)?,
                b"MODL" => put(&mut info, &tag, parse_modl(&mut p)?)?,
                b"SEQS" => put(&mut sequences, &tag, parse_seqs(&mut p)?)?,
                b"GLBS" => put(&mut global_sequences, &tag, parse_glbs(&mut p)?)?,
                b"TEXS" => put(&mut textures, &tag, parse_texs(&mut p)?)?,
                b"MTLS" => put(&mut materials, &tag, material::parse_all(&mut p)?)?,
                b"GEOS" => put(&mut geosets, &tag, geoset::parse_all(&mut p)?)?,
                b"BONE" => put(&mut bones, &tag, node::parse_bones(&mut p)?)?,
                b"HELP" => put(&mut helpers, &tag, node::parse_helpers(&mut p)?)?,
                b"PIVT" => put(&mut pivots, &tag, parse_pivt(&mut p)?)?,
                b"GEOA" => put(
                    &mut geoset_anims,
                    &tag,
                    geoanim::parse_all(&mut p)?,
                )?,
                _ => unknown_chunks.push(RawChunk {
                    tag,
                    data: payload.to_vec(),
                }),
            }
        }

        let version = version.ok_or_else(|| missing("VERS"))?;
        if version != VERSION {
            return Err(Error::UnsupportedVersion { found: version });
        }
        Ok(Model {
            version,
            info: info.ok_or_else(|| missing("MODL"))?,
            sequences: sequences.unwrap_or_default(),
            global_sequences: global_sequences.unwrap_or_default(),
            textures: textures.unwrap_or_default(),
            materials: materials.unwrap_or_default(),
            geosets: geosets.unwrap_or_default(),
            bones: bones.unwrap_or_default(),
            helpers: helpers.unwrap_or_default(),
            pivots: pivots.unwrap_or_default(),
            geoset_anims: geoset_anims.unwrap_or_default(),
            unknown_chunks,
        })
    }
}

impl Model {
    /// Index of a sequence by name. An exact match (ignoring case) wins; otherwise the first
    /// sequence whose name starts with `query` (so "stand" finds "Stand - 1").
    pub fn find_sequence(&self, query: &str) -> Option<usize> {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return None;
        }
        let names = self.sequences.iter().map(|s| s.name.to_ascii_lowercase());
        let exact = self.sequences.iter().position(|s| s.name.eq_ignore_ascii_case(&q));
        exact.or_else(|| names.into_iter().position(|n| n.starts_with(&q)))
    }
}

impl Extent {
    pub(crate) fn read(r: &mut Reader) -> Result<Extent, Error> {
        Ok(Extent {
            radius: r.f32()?,
            min: r.vec3()?,
            max: r.vec3()?,
        })
    }
}

fn parse_modl(p: &mut Reader) -> Result<ModelInfo, Error> {
    let name = p.fixed_str(336)?;
    // Unknown u32 between the name and the bounds (**uncertain**, always 0 in known files).
    let _unknown = p.u32()?;
    let extent = Extent::read(p)?;
    let blend_time = p.u32()?;
    Ok(ModelInfo {
        name,
        extent,
        blend_time,
    })
}

fn parse_seqs(p: &mut Reader) -> Result<Vec<Sequence>, Error> {
    check_multiple(p, 132, "SEQS")?;
    let mut out = Vec::new();
    while !p.is_empty() {
        out.push(Sequence {
            name: p.fixed_str(80)?,
            start: p.u32()?,
            end: p.u32()?,
            move_speed: p.f32()?,
            flags: p.u32()?,
            rarity: p.f32()?,
            sync_point: p.u32()?,
            extent: Extent::read(p)?,
        });
    }
    Ok(out)
}

fn parse_glbs(p: &mut Reader) -> Result<Vec<u32>, Error> {
    check_multiple(p, 4, "GLBS")?;
    let mut out = Vec::new();
    while !p.is_empty() {
        out.push(p.u32()?);
    }
    Ok(out)
}

fn parse_texs(p: &mut Reader) -> Result<Vec<Texture>, Error> {
    check_multiple(p, 268, "TEXS")?;
    let mut out = Vec::new();
    while !p.is_empty() {
        out.push(Texture {
            replaceable_id: p.u32()?,
            path: p.fixed_str(260)?,
            flags: p.u32()?,
        });
    }
    Ok(out)
}

fn parse_pivt(p: &mut Reader) -> Result<Vec<[f32; 3]>, Error> {
    check_multiple(p, 12, "PIVT")?;
    let mut out = Vec::new();
    while !p.is_empty() {
        out.push(p.vec3()?);
    }
    Ok(out)
}

/// Fixed-size record chunks must divide evenly into records.
fn check_multiple(p: &Reader, elem: usize, tag: &str) -> Result<(), Error> {
    if !p.remaining().is_multiple_of(elem) {
        return Err(Error::SizeMismatch {
            tag: tag.to_string(),
            size: p.remaining(),
            elem,
        });
    }
    Ok(())
}

fn missing(tag: &str) -> Error {
    Error::MissingChunk {
        tag: tag.to_string(),
    }
}

/// Renders a tag for messages. Non-printable bytes become `?`.
pub(crate) fn fourcc(tag: Tag) -> String {
    tag.iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}
