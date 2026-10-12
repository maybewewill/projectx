//! Keyframe sampling: time lookup, global sequences, and value blending.
//!
//! Time model: key times and sequence times are absolute milliseconds. A track with a global
//! sequence id loops over `time % GLBS[id]`; other tracks use the time as given. Before the
//! first key the first value holds, after the last key the last value holds.
//!
//! Blending (**uncertain** against real files, the layouts are from public format notes):
//! * Linear: plain interpolation. Rotations use the shortest-arc slerp.
//! * Hermite: `p = h00*p0 + h10*m0 + h01*p1 + h11*m1` where `m0` is the out-tangent of the
//!   first key and `m1` the in-tangent of the second. Tangents are slopes per segment and are
//!   not scaled by the key spacing.
//! * Bezier: the tangents are absolute control values, `(1-s)^3 p0 + 3(1-s)^2 s c0 +
//!   3(1-s) s^2 c1 + s^3 p1`.
//! * Rotations with Hermite or Bezier: squad, with the tangents as the inner control
//!   quaternions. Missing tangents fall back to the key values, which makes squad reduce to
//!   slerp.
//! * Discrete (raw 0): the earlier key's value holds until the next key.
//!
//! Integer tracks (texture ids) never blend: they hold the earlier key's value.

use glam::{Quat, Vec3};

use crate::anim::{Interpolation, Key, Track};

/// Value types a track can hold.
pub trait Keyable: Copy {
    /// Blends the segment `a..b` at parameter `s` in `[0, 1)`.
    fn blend(a: &Key<Self>, b: &Key<Self>, s: f32, interp: Interpolation) -> Self;
}

/// Vector-space operations needed by the cubic blends.
pub(crate) trait Vector: Copy {
    fn zero() -> Self;
    fn add(self, other: Self) -> Self;
    fn scale(self, s: f32) -> Self;
}

impl Vector for f32 {
    fn zero() -> Self {
        0.0
    }
    fn add(self, other: Self) -> Self {
        self + other
    }
    fn scale(self, s: f32) -> Self {
        self * s
    }
}

impl Vector for [f32; 3] {
    fn zero() -> Self {
        [0.0; 3]
    }
    fn add(self, o: Self) -> Self {
        [self[0] + o[0], self[1] + o[1], self[2] + o[2]]
    }
    fn scale(self, s: f32) -> Self {
        [self[0] * s, self[1] * s, self[2] * s]
    }
}

pub(crate) fn lerp_v<V: Vector>(a: V, b: V, s: f32) -> V {
    a.scale(1.0 - s).add(b.scale(s))
}

/// Cubic Hermite with per-segment slopes (no time scaling).
pub(crate) fn hermite<V: Vector>(p0: V, m0: V, p1: V, m1: V, s: f32) -> V {
    let s2 = s * s;
    let s3 = s2 * s;
    let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
    let h10 = s3 - 2.0 * s2 + s;
    let h01 = -2.0 * s3 + 3.0 * s2;
    let h11 = s3 - s2;
    p0.scale(h00)
        .add(m0.scale(h10))
        .add(p1.scale(h01))
        .add(m1.scale(h11))
}

/// Cubic Bezier with absolute control values `c0` and `c1`.
pub(crate) fn bezier<V: Vector>(p0: V, c0: V, c1: V, p1: V, s: f32) -> V {
    let u = 1.0 - s;
    p0.scale(u * u * u)
        .add(c0.scale(3.0 * u * u * s))
        .add(c1.scale(3.0 * u * s * s))
        .add(p1.scale(s * s * s))
}

fn blend_vector<V: Vector>(a: &Key<V>, b: &Key<V>, s: f32, interp: Interpolation) -> V {
    match interp {
        Interpolation::None => a.value,
        Interpolation::Linear => lerp_v(a.value, b.value, s),
        Interpolation::Hermite | Interpolation::Bezier => {
            let m0 = a.out_tangent.unwrap_or_else(V::zero);
            let m1 = b.in_tangent.unwrap_or_else(V::zero);
            if interp == Interpolation::Hermite {
                hermite(a.value, m0, b.value, m1, s)
            } else {
                bezier(a.value, m0, m1, b.value, s)
            }
        }
    }
}

impl Keyable for f32 {
    fn blend(a: &Key<f32>, b: &Key<f32>, s: f32, interp: Interpolation) -> f32 {
        blend_vector(a, b, s, interp)
    }
}

impl Keyable for [f32; 3] {
    fn blend(a: &Key<[f32; 3]>, b: &Key<[f32; 3]>, s: f32, interp: Interpolation) -> [f32; 3] {
        blend_vector(a, b, s, interp)
    }
}

impl Keyable for u32 {
    fn blend(a: &Key<u32>, _b: &Key<u32>, _s: f32, _interp: Interpolation) -> u32 {
        a.value
    }
}

/// Rotations are stored as `[x, y, z, w]`.
impl Keyable for [f32; 4] {
    fn blend(a: &Key<[f32; 4]>, b: &Key<[f32; 4]>, s: f32, interp: Interpolation) -> [f32; 4] {
        let qa = quat(a.value);
        let qb = quat(b.value);
        let q = match interp {
            Interpolation::None => qa,
            Interpolation::Linear => slerp_short(qa, qb, s),
            Interpolation::Hermite | Interpolation::Bezier => {
                let out_a = a.out_tangent.map_or(qa, quat);
                let in_b = b.in_tangent.map_or(qb, quat);
                squad(qa, out_a, in_b, qb, s)
            }
        };
        let q = q.normalize();
        [q.x, q.y, q.z, q.w]
    }
}

pub(crate) fn quat(v: [f32; 4]) -> Quat {
    Quat::from_xyzw(v[0], v[1], v[2], v[3])
}

/// Slerp that takes the shorter arc.
pub(crate) fn slerp_short(a: Quat, b: Quat, s: f32) -> Quat {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    a.slerp(b, s)
}

/// Spherical cubic (squad) with inner control quaternions `a` and `b`.
pub(crate) fn squad(q0: Quat, a: Quat, b: Quat, q1: Quat, s: f32) -> Quat {
    let outer = slerp_short(q0, q1, s);
    let inner = slerp_short(a, b, s);
    slerp_short(outer, inner, 2.0 * s * (1.0 - s))
}

/// Maps a track's time into the range its keys use (global sequence wrap).
pub fn track_time(global_sequence: Option<u32>, time: u32, globals: &[u32]) -> u32 {
    match global_sequence.and_then(|id| globals.get(id as usize).copied()) {
        Some(len) if len > 0 => time % len,
        _ => time,
    }
}

impl<T: Keyable> Track<T> {
    /// Value of the track at `time` (absolute milliseconds). `None` if the track has no keys.
    /// `globals` are the GLBS lengths of the model.
    pub fn sample(&self, time: u32, globals: &[u32]) -> Option<T> {
        let t = track_time(self.global_sequence, time, globals);
        sample_keys(&self.keys, self.interpolation, t)
    }

    /// Value at `time` inside the sequence interval `start..=end` (absolute milliseconds).
    ///
    /// Only the keys within the interval take part: keys of other sequences (a Death
    /// sequence's fade-out, say) must not leak into this one. Before the first in-interval
    /// key the first holds, after the last the last holds. `None` when the track has no key in
    /// the interval, so the caller falls back to the static value. A track bound to a global
    /// sequence ignores the interval.
    pub fn sample_in(&self, time: u32, start: u32, end: u32, globals: &[u32]) -> Option<T> {
        if self.global_sequence.is_some() {
            return self.sample(time, globals);
        }
        let lo = self.keys.partition_point(|k| k.time < start);
        let hi = self.keys.partition_point(|k| k.time <= end);
        sample_keys(self.keys.get(lo..hi)?, self.interpolation, time)
    }
}

fn sample_keys<T: Keyable>(keys: &[Key<T>], interp: Interpolation, t: u32) -> Option<T> {
    let first = keys.first()?;
    // Number of keys at or before `t`. Keys are assumed sorted by time.
    let i = keys.partition_point(|k| k.time <= t);
    if i == 0 {
        return Some(first.value);
    }
    if i == keys.len() {
        return Some(keys[i - 1].value);
    }
    let (a, b) = (&keys[i - 1], &keys[i]);
    let s = (t - a.time) as f32 / (b.time - a.time) as f32;
    Some(T::blend(a, b, s, interp))
}

/// Converts a sampled rotation to a unit quaternion.
pub fn rotation_quat(v: [f32; 4]) -> Quat {
    quat(v).normalize()
}

/// Converts a sampled vector.
pub fn vec3(v: [f32; 3]) -> Vec3 {
    Vec3::from_array(v)
}
