//! Animation tracks: KGTR/KGRT/KGSC/KATV (nodes) and KMTA/KMTF (material layers).
//!
//! Track layout: tag, u32 key count, u32 interpolation, i32 global sequence id (-1 = none),
//! then the keys. Each key is a u32 time followed by its value, and for Hermite and Bezier
//! tracks also an in-tangent and an out-tangent of the same type as the value.

use crate::reader::Reader;
use crate::{fourcc, Error, Tag};

/// Interpolation mode of a track.
///
/// The raw numbering (0 = none, 1 = linear) is taken from public format notes.
/// The values 2 (Hermite) and 3 (Bezier) are **uncertain**: they may be swapped in the
/// binary format, so check against real files before relying on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// Raw 0: no interpolation, the value steps at each key.
    None,
    /// Raw 1: linear interpolation between keys.
    Linear,
    /// Raw 2: Hermite. Keys carry in/out tangents (confirmed on shipped models: raw-2 tracks
    /// have tangent-sized keys and parse to the end of the record).
    Hermite,
    /// Raw 3 (uncertain): Bezier. Keys carry in/out tangents.
    Bezier,
}

impl Interpolation {
    fn from_raw(raw: u32, offset: usize) -> Result<Self, Error> {
        match raw {
            0 => Ok(Self::None),
            1 => Ok(Self::Linear),
            2 => Ok(Self::Hermite),
            3 => Ok(Self::Bezier),
            _ => Err(Error::InvalidValue {
                what: "interpolation type",
                offset,
                value: i64::from(raw),
            }),
        }
    }

    /// True when keys carry in/out tangents.
    pub fn has_tangents(self) -> bool {
        matches!(self, Self::Hermite | Self::Bezier)
    }
}

/// One keyframe. `time` is in the units of the track's sequence (frames in the format notes).
#[derive(Debug, Clone, PartialEq)]
pub struct Key<T> {
    pub time: u32,
    pub value: T,
    pub in_tangent: Option<T>,
    pub out_tangent: Option<T>,
}

/// An animated value track.
#[derive(Debug, Clone, PartialEq)]
pub struct Track<T> {
    pub interpolation: Interpolation,
    /// Global sequence id that loops this track, or `None` (raw -1) for sequence-local time.
    pub global_sequence: Option<u32>,
    pub keys: Vec<Key<T>>,
}

/// Maps the raw "-1 means none" convention used for ids in MDX.
pub(crate) fn opt_index(raw: i32, what: &'static str, offset: usize) -> Result<Option<u32>, Error> {
    match raw {
        -1 => Ok(None),
        v if v >= 0 => Ok(Some(v as u32)),
        _ => Err(Error::InvalidValue {
            what,
            offset,
            value: i64::from(raw),
        }),
    }
}

/// Reads one track whose tag must equal `tag`. `value_size` is the byte size of one value.
pub(crate) fn read_track<'a, T, F>(
    r: &mut Reader<'a>,
    tag: &Tag,
    value_size: usize,
    mut read: F,
) -> Result<Track<T>, Error>
where
    F: FnMut(&mut Reader<'a>) -> Result<T, Error>,
{
    r.tag(tag)?;
    let count = r.count(4 + value_size, "animation keys")?;
    let offset = r.offset();
    let interpolation = Interpolation::from_raw(r.u32()?, offset)?;
    let offset = r.offset();
    let global_sequence = opt_index(r.i32()?, "global sequence id", offset)?;
    let mut keys = Vec::with_capacity(count);
    for _ in 0..count {
        let time = r.u32()?;
        let value = read(r)?;
        let (in_tangent, out_tangent) = if interpolation.has_tangents() {
            let i = read(r)?;
            let o = read(r)?;
            (Some(i), Some(o))
        } else {
            (None, None)
        };
        keys.push(Key {
            time,
            value,
            in_tangent,
            out_tangent,
        });
    }
    Ok(Track {
        interpolation,
        global_sequence,
        keys,
    })
}

/// Stores a parsed value into an optional slot. A second occurrence is an error.
pub(crate) fn put<T>(slot: &mut Option<T>, tag: &Tag, value: T) -> Result<(), Error> {
    if slot.is_some() {
        return Err(Error::DuplicateChunk { tag: fourcc(*tag) });
    }
    *slot = Some(value);
    Ok(())
}
