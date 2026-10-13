//! Weapon target acceptance: the target flag word of a unit, the predicate that matches it against a weapon's `targs`
//! word, and the two weapon choosers (`docs/spec/weapon-target.md`).
//!
//! Plain models of routines of Game.dll 1.26a, pinned against the original by Unicorn vectors
//! (`tools/emu/target_accept_emu.py`, `crates/sim/tests/target_accept.rs`).
//!
//! The target flag word `T` of a unit (`0x6F25D280`) carries: alive `0x1000000` / dead `0x2000000`, vulnerable
//! `0x100000` / invulnerable `0x200000`, the unit's own `targType` bits (`0x2` ground, `0x4` air, `0x8` structure,
//! `0x10` ward, masked to `0x1E` at load), hero `0x400000` / nonhero `0x800000`, sapper `0x20000000` / nonsapper
//! `0x10000000`, mechanical `0x8000000` / organic `0x4000000`, ancient `0x80000000` / nonancient `0x40000000`.
//! The relation bits (`0x1000` self ... `0x10000` enemy) are never part of `T`: the weapon choosers call the predicate
//! with the "no relation" arguments, so a weapon's `enemy`/`ally`/`self` words do not take part in the choice.

/// The error code of "cannot attack" (also the code of a missing target kind).
pub const CANNOT: u32 = 0xDD;

/// `0x6F2552F0`: a weapon `targs` word with every unnamed group widened to "any". The groups are
/// `0x1F000` (relation), `0xC0000000` (ancient), `0xC000000` (organic/mechanical), `0x30000000` (sapper),
/// `0x7FE` (kind: widened to ground, air and structure `0xE` only), `0x3000000` (alive: widened to alive only),
/// `0xC00000` (hero), `0x300000` (vulnerable: widened to vulnerable only). CONFIRMED.
pub fn expand(w: u32) -> u32 {
    let mut e = 0;
    if w & 0x1F000 == 0 {
        e |= 0x1F000;
    }
    if w & 0xC000_0000 == 0 {
        e |= 0xC000_0000;
    }
    if w & 0x0C00_0000 == 0 {
        e |= 0x0C00_0000;
    }
    if w & 0x3000_0000 == 0 {
        e |= 0x3000_0000;
    }
    if w & 0x7FE == 0 {
        e |= 0xE;
    }
    if w & 0x0300_0000 == 0 {
        e |= 0x0100_0000;
    }
    if w & 0x00C0_0000 == 0 {
        e |= 0x00C0_0000;
    }
    if w & 0x0030_0000 == 0 {
        e |= 0x0010_0000;
    }
    e | w
}

/// `0x6F2553C0`: the error code when no kind bit of the target is accepted (`t` the target flags, `e` the expanded
/// weapon word). CONFIRMED (vectors compare the codes).
pub fn kind_error(t: u32, e: u32) -> u32 {
    let k = e & 0x7FE;
    match k {
        2 => {
            if t & 8 != 0 {
                0x8C
            } else if t & 0x10 != 0 {
                0x8D
            } else {
                0x5E
            }
        }
        4 => 0x5F,
        8 => 0x60,
        0x10 => 0x61,
        0x20 => 0x62,
        0x40 => 0x63,
        0x80 => 0x64,
        0x100 => 0x65,
        0x200 => 0x66,
        0x400 => 0x67,
        0x48 | 0xC8 => 0x78,
        _ => {
            let k6 = k & 6;
            if t & 2 != 0 {
                0x89 + u32::from(k6 != 0)
            } else if t & 4 != 0 {
                0x89 + 2 * u32::from(k6 != 0)
            } else if t & 8 != 0 {
                0x8C
            } else if t & 0x10 != 0 {
                0x8D
            } else if t & 0x20 != 0 {
                0x8E
            } else if t & 0x40 != 0 {
                0x8F
            } else if t & 0x80 != 0 {
                0x90
            } else if t & 0x100 != 0 {
                0x91
            } else if t & 0x200 != 0 {
                0x92
            } else if t & 0x400 != 0 {
                0x93
            } else {
                0
            }
        }
    }
}

/// `0x6F25D560` with the arguments the choosers push (`-1, 0, 0, -1`): 0 = the weapon word `w` accepts the target
/// flags `t`, otherwise an error code. The sub-calls `0x6F25D390` (relations) and `0x6F0766A0` are never reached with
/// these arguments. CONFIRMED.
pub fn predicate(t: u32, w: u32) -> u32 {
    if (t | w) & 1 != 0 || t & 0x7FE == 0 {
        return CANNOT;
    }
    let e = expand(w);
    let s = t & e;
    if s & 0x7FE == 0 {
        return kind_error(t, e);
    }
    if s & 0x0300_0000 == 0 {
        return if t & 0x0100_0000 != 0 { 0x74 } else { 0x98 };
    }
    if s & 0x0030_0000 == 0 {
        return if t & 0x0020_0000 != 0 { 0x96 } else { 0x5C };
    }
    if t & 0x1E == 0 {
        return 0;
    }
    if s & 0x0C00_0000 == 0 {
        return if t & 0x0800_0000 != 0 { 0x9B } else { 0x9C };
    }
    if s & 0x00C0_0000 == 0 {
        return if t & 0x0040_0000 != 0 { 0x97 } else { 0x72 };
    }
    if s & 0x3000_0000 == 0 {
        return if t & 0x2000_0000 != 0 { 0x9D } else { 0x5C };
    }
    if s & 0xC000_0000 == 0 {
        return if t & 0x8000_0000 != 0 { 0x9E } else { 0x5C };
    }
    0
}

/// `0x6F25D280` for a unit: the target flag word. `dead` is bit `0x100` of `unit+0x5C`, `invulnerable` bit 8 of
/// `unit+0x20`, `targ_type` the record's `targType` (`unit+0x24C`, masked to `0x1E` at load), `type_char` the first
/// character of the type id (`unit+0x33`, a hero when `A` to `Z`), `class_word` the classification word `unit+0x248`
/// (sapper `0x20`, mechanical `0x8`, ancient `0x80`). CONFIRMED (600 Unicorn vectors).
pub fn unit_target_flags(dead: bool, invulnerable: bool, targ_type: u32, type_char: u8, class_word: u32) -> u32 {
    let mut t = if dead { 0x0200_0000 } else { 0x0100_0000 };
    t |= if invulnerable { 0x0020_0000 } else { 0x0010_0000 };
    t |= targ_type;
    t |= if type_char.wrapping_sub(0x41) <= 0x19 { 0x0040_0000 } else { 0x0080_0000 };
    t |= if class_word & 0x20 != 0 { 0x2000_0000 } else { 0x1000_0000 };
    t |= if class_word & 0x08 != 0 { 0x0800_0000 } else { 0x0400_0000 };
    t |= if class_word & 0x80 != 0 { 0x8000_0000 } else { 0x4000_0000 };
    t
}

/// One weapon slot as the choosers read it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeaponSlot {
    /// The weapon is enabled (`0x6F0C6F90`: the `0x80000 << w` bit of the attack ability; the three disable counters
    /// have no writer in the model).
    pub enabled: bool,
    /// The weapon's `targs` word (`A+0x218+4w`).
    pub targs: u32,
    /// The attack type row of the weapon (`A+0xF4+4w`, 0 Spells .. 6 Hero).
    pub atk_type: u32,
}

/// The result of a chooser: `code` 0 means a weapon fits and `index` is its slot; otherwise `index` is -1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub code: u32,
    pub index: i32,
}

/// `0x6F0C95D0`: the weapon chooser for a target-info object whose flag word is `t`. Walks the enabled weapons in
/// order; a weapon whose `targs` is exactly 1 (`none`) is skipped; the first weapon the predicate accepts wins. When
/// none fits, the code is the lowest error of the weapons tried (one usable weapon), `0xDD` (none usable), or, with
/// two usable weapons, the predicate over the union of both expanded words (its code when non-zero, else the lowest
/// error). The "restriction list" at `A+0x2BC` (weapons skipped when it is non-empty) has no writer in the model and
/// is not applied. CONFIRMED (600 Unicorn vectors).
pub fn choose_for_info(t: u32, w: [WeaponSlot; 2]) -> Choice {
    let mut best = CANNOT;
    let mut count = 0;
    let mut union = 0;
    for (i, slot) in w.iter().enumerate() {
        if !slot.enabled || slot.targs == 1 {
            continue;
        }
        count += 1;
        union |= expand(slot.targs);
        let r = predicate(t, slot.targs);
        if r == 0 {
            return Choice { code: 0, index: i as i32 };
        }
        if (r as i32) <= (best as i32) {
            best = r;
        }
    }
    let code = match count {
        1 => best,
        0 => CANNOT,
        _ => {
            let r = predicate(t, union);
            if r != 0 {
                r
            } else {
                best
            }
        }
    };
    Choice { code, index: -1 }
}

/// The immunity inputs of the unit chooser `0x6F0C93E0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitChecks {
    /// The target is magic immune (`unit+0x15C > 0`, signed).
    pub magic_immune: bool,
    /// The target is ethereal (`unit+0x158 > 0`, signed).
    pub ethereal: bool,
    /// Misc `MagicImmunesResistDamage`.
    pub misc_flag: bool,
}

/// `0x6F255630`: with the Misc flag, a Magic attack (row 4) cannot hit a magic immune target.
fn magic_blocked(misc: bool, atk: u32) -> bool {
    misc && atk == 4
}

/// `0x6F255660`: with the Misc flag, a physical attack (rows 1, 2, 3, 5, 6) cannot hit an ethereal target.
fn physical_blocked(misc: bool, atk: u32) -> bool {
    misc && matches!(atk, 1 | 2 | 3 | 5 | 6)
}

/// `0x6F0C93E0`: the weapon chooser for a target unit with flag word `t`. Like [`choose_for_info`], but a weapon the
/// predicate accepts is still refused (code `0x36`) when the target is magic immune and the attack type is Magic, or
/// (code `0xBC`) when it is ethereal and the attack type is physical; a refusal code is returned in preference to the
/// predicate's lowest error. `checks` are zero when the global container pointer `0x6FA4F6E8` is null (never in a
/// running game). CONFIRMED (600 Unicorn vectors).
pub fn choose_for_unit(t: u32, w: [WeaponSlot; 2], checks: UnitChecks) -> Choice {
    let mut best = CANNOT;
    let mut final_err = CANNOT;
    let mut count = 0;
    let mut union = 0;
    for (i, slot) in w.iter().enumerate() {
        if !slot.enabled || slot.targs == 1 {
            continue;
        }
        count += 1;
        union |= expand(slot.targs);
        let r = predicate(t, slot.targs);
        if r != 0 {
            if (r as i32) <= (best as i32) {
                best = r;
            }
            continue;
        }
        if checks.magic_immune && magic_blocked(checks.misc_flag, slot.atk_type) {
            final_err = 0x36;
            continue;
        }
        if checks.ethereal && physical_blocked(checks.misc_flag, slot.atk_type) {
            final_err = 0xBC;
            continue;
        }
        return Choice { code: 0, index: i as i32 };
    }
    let code = if final_err != CANNOT {
        final_err
    } else {
        match count {
            1 => best,
            0 => CANNOT,
            _ => {
                let r = predicate(t, union);
                if r != 0 {
                    r
                } else {
                    best
                }
            }
        }
    };
    Choice { code, index: -1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(targ_type: u32) -> u32 {
        unit_target_flags(false, false, targ_type, b'h', 0)
    }

    #[test]
    fn ground_unit_is_hit_by_the_default_weapon() {
        // `targs` empty (0) widens to ground/air/structure.
        assert_eq!(predicate(flags(2), 0), 0);
        assert_eq!(predicate(flags(4), 0), 0);
        assert_eq!(predicate(flags(8), 0), 0);
    }

    #[test]
    fn air_only_weapon_refuses_ground() {
        assert_ne!(predicate(flags(2), 4), 0);
        assert_eq!(predicate(flags(4), 4), 0);
        assert_ne!(predicate(flags(4), 2), 0);
    }

    #[test]
    fn dead_and_invulnerable_targets_are_refused() {
        let dead = unit_target_flags(true, false, 2, b'h', 0);
        assert_ne!(predicate(dead, 2), 0);
        let inv = unit_target_flags(false, true, 2, b'h', 0);
        assert_ne!(predicate(inv, 2), 0);
        // a weapon naming `invulnerable` hits it
        assert_eq!(predicate(inv, 2 | 0x20_0000), 0);
    }

    #[test]
    fn two_weapons_pick_by_target() {
        let ground = WeaponSlot { enabled: true, targs: 2, atk_type: 1 };
        let air = WeaponSlot { enabled: true, targs: 4, atk_type: 2 };
        assert_eq!(choose_for_info(flags(2), [ground, air]).index, 0);
        assert_eq!(choose_for_info(flags(4), [ground, air]).index, 1);
        assert_eq!(choose_for_info(flags(8), [ground, air]).index, -1);
    }
}
