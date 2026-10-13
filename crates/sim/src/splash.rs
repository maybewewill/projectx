//! Splash attacks (`weapTp` msplash): the candidate filter and the ring factor of `CMissileSplash`
//! (`docs/spec/weapon-splash.md`).
//!
//! Plain models of routines of Game.dll 1.26a, pinned by Unicorn vectors (`tools/emu/splash_emu.py`,
//! `crates/sim/tests/splash.rs`).
//!
//! The flow of an msplash hit, as read from the machine code:
//!
//! 1. The attack object copies the weapon's splash object (`A+0x238+4w`: splash targets, Farea, Harea, Qarea, Hfact,
//!    Qfact) into the launch record (`0x6F0C5510`), see [`ring_spec`].
//! 2. On arrival (`CMissileSplash` slot 47, `0x6F2C2C80`) the primary target is processed first through slot 51
//!    (`0x6F2BF160`, no filter), then every unit in the circle of the outermost ring (see [`query_radius`]) that passes
//!    [`candidate_filter`] is processed through the same slot 51.
//! 3. Slot 51 ([`ring_factor`]) picks the damage factor of one unit from its ring: inside `Farea` the full damage
//!    (factor 1.0), inside `Harea` the factor `Hfact`, otherwise `Qfact`; outside the outermost ring nothing.

use softfloat::{arith, F32};

use crate::target_accept::{predicate, unit_target_flags};

/// The splash ring spec of a launch record as `0x6F0C5510` writes it (record `+0x7C`..`+0x90`): the splash target
/// flags, `Hfact`, `Qfact`, `Farea`, `Harea`, `Qarea`. CONFIRMED (500 Unicorn vectors). `aux` is the weapon's splash
/// object values at `+0x20`, `+0x28`, `+0x30`, `+0x38`, `+0x40`, `+0x48` (flags, Farea, Harea, Qarea, Hfact, Qfact),
/// or `None` when the weapon has no splash object (all zero).
pub fn ring_spec(aux: Option<[u32; 6]>) -> [u32; 6] {
    match aux {
        Some(a) => [a[0], a[4], a[5], a[1], a[2], a[3]],
        None => [0; 6],
    }
}

/// The radius of the circle the arrival queries (`0x6F2C2C80`): `Qarea` when it is positive, else `Harea` when it is
/// positive, else `Farea`. CONFIRMED structure (the positive tests are `0x6F022270`).
pub fn query_radius(farea: F32, harea: F32, qarea: F32) -> F32 {
    if positive(qarea) {
        qarea
    } else if positive(harea) {
        harea
    } else {
        farea
    }
}

/// `0x6F022270` against 0.0: the property value is greater than zero **by at least 0.001** (an x87 compare of the raw
/// values, then the soft-float difference must not be below the epsilon `0x6FAAE47C`). CONFIRMED (Unicorn).
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn positive(v: F32) -> bool {
    let fv = f32::from_bits(v.to_bits());
    if !(0.0f32 < fv) {
        return false;
    }
    let d = arith::sub(v, F32::ZERO).to_bits() & 0x7FFF_FFFF;
    !(f32::from_bits(d) < f32::from_bits(0x3A83_126F))
}

/// `CMissileSplash` slot 51 (`0x6F2BF160`): the damage factor of one candidate unit, or `None` when it takes nothing.
/// `in_ring(radius)` is the ring membership test (`0x6F476C40`, flag 1) of the unit for a ring of that radius. The rings
/// are tried from the innermost: `Farea` always (factor 1.0, even for a zero radius), then `Harea` when it is positive
/// (factor `Hfact`); a unit that is in neither takes `Qfact` when `Qarea` is positive, **without a distance test** (the
/// query circle already bounds it). CONFIRMED (500 Unicorn vectors).
pub fn ring_factor(
    farea: F32,
    harea: F32,
    qarea: F32,
    hfact: F32,
    qfact: F32,
    mut in_ring: impl FnMut(F32) -> bool,
) -> Option<F32> {
    if in_ring(farea) {
        return Some(F32::ONE);
    }
    if positive(harea) && in_ring(harea) {
        return Some(hfact);
    }
    if positive(qarea) {
        return Some(qfact);
    }
    None
}

/// The facts about the candidate unit that the filter reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct Candidate {
    /// Bit `0x100` of `unit+0x5C`.
    pub dead: bool,
    /// Bit 8 of `unit+0x20` (the invulnerable flag of the target flag word).
    pub invulnerable_flag: bool,
    /// The type's `targType` (`unit+0x24C`).
    pub targ_type: u32,
    /// First character of the type id (hero when `A` to `Z`).
    pub type_char: u8,
    /// The classification word `unit+0x248`.
    pub class_word: u32,
    /// Ethereal counter (`unit+0x158`, signed).
    pub ethereal: i32,
    /// Magic immunity counter (`unit+0x15C`, signed).
    pub magic_immune: i32,
    /// The unit's vtable slot `0x11C` (invulnerable, buffs included).
    pub invulnerable: bool,
    /// The unit's owner index.
    pub owner: u32,
}

/// Where the missile's source unit stands relative to the candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The source unit is gone; only its owner index is known.
    Gone,
    /// Another unit, with this owner.
    Other(u32),
    /// The candidate itself.
    Itself,
}

/// `0x6F25D390` with `W` the splash targets word: the relation part of the predicate. 0 = accepted.
/// `passive(a, b)` is the alliance row test `0x6F3A3400` (the passive row of `a` has the bit of `b`), `shared(a, b)` is
/// `0x6F3A36D0` (the fifth row, "shared spells"). CONFIRMED structure; the row types are LIKELY (docs).
fn relation(
    w: u32,
    expanded: u32,
    target_owner: u32,
    source: Source,
    source_owner: u32,
    passive: &mut impl FnMut(u32, u32) -> bool,
    shared: &mut impl FnMut(u32, u32) -> bool,
) -> u32 {
    if expanded & 0x1F000 == 0x1F000 {
        return 0;
    }
    let owner = match source {
        Source::Gone => {
            if source_owner == u32::MAX {
                return 0;
            }
            source_owner
        }
        Source::Other(o) => o,
        Source::Itself => target_owner,
    };
    if source == Source::Itself {
        return if w & 0x1000 != 0 { 0 } else { 0x85 };
    }
    if target_owner == owner {
        return if w & 0x2000 != 0 { 0 } else { 0x84 };
    }
    if !passive(owner, target_owner) {
        return if w & 0x10000 != 0 { 0 } else { 0x88 };
    }
    if shared(owner, target_owner) {
        if w & 0x4000 != 0 {
            0
        } else {
            0x86
        }
    } else if w & 0x8000 != 0 {
        0
    } else {
        0x87
    }
}

/// `0x6F25D560` as the splash filter calls it (target object present, relation arguments set), for a candidate unit
/// whose `unit+0x20` bit 0 and `unit+0x60` bit 0 are clear and whose ability list is empty. 0 = accepted.
fn full_predicate(
    t: u32,
    w: u32,
    target_owner: u32,
    source: Source,
    source_owner: u32,
    passive: &mut impl FnMut(u32, u32) -> bool,
    shared: &mut impl FnMut(u32, u32) -> bool,
) -> u32 {
    use crate::target_accept::expand;
    if (t | w) & 1 != 0 || t & 0x7FE == 0 {
        return 0xDD;
    }
    let e = expand(w);
    let s = t & e;
    // The stages before the relation check are those of `target_accept::predicate`; the stages after it are
    // evaluated in the original's order, so the first failing stage decides the code.
    if s & 0x7FE == 0 {
        return predicate(t, w);
    }
    if s & 0x0300_0000 == 0 {
        return predicate(t, w);
    }
    if s & 0x0030_0000 == 0 {
        return predicate(t, w);
    }
    if t & 0x1E == 0 {
        return 0;
    }
    if s & 0x0C00_0000 == 0 {
        return predicate(t, w);
    }
    let r = relation(w, e, target_owner, source, source_owner, passive, shared);
    if r != 0 {
        return r;
    }
    predicate(t, w)
}

/// The candidate filter `0x6F2BF020`: whether the missile may hurt `c` (0 from `0x6F25D6F0` and the immunity gate).
/// `magic_blocked` / `physical_blocked` are the Misc `MagicImmunesResistDamage` tests of the missile's attack type
/// (`0x6F255630` / `0x6F255660`). CONFIRMED (500 Unicorn vectors).
#[allow(clippy::too_many_arguments)]
pub fn candidate_filter(
    c: &Candidate,
    splash_targs: u32,
    source: Source,
    source_owner: u32,
    magic_blocked: bool,
    physical_blocked: bool,
    mut passive: impl FnMut(u32, u32) -> bool,
    mut shared: impl FnMut(u32, u32) -> bool,
) -> bool {
    if magic_blocked && (c.invulnerable || c.magic_immune > 0) {
        return false;
    }
    if physical_blocked && c.ethereal > 0 {
        return false;
    }
    let t = unit_target_flags(c.dead, c.invulnerable_flag, c.targ_type, c.type_char, c.class_word);
    full_predicate(t, splash_targs, c.owner, source, source_owner, &mut passive, &mut shared) == 0
}

/// The attack-type tests of `0x6F255630` / `0x6F255660` for the Misc flag `misc`.
pub fn attack_type_blocks(misc: bool, atk: u32) -> (bool, bool) {
    (misc && atk == 4, misc && matches!(atk, 1 | 2 | 3 | 5 | 6))
}
