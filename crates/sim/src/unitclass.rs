//! Unit classification: the tests behind `IsUnitType`, `IsUnitIdType`, `UnitAddType` and `UnitRemoveType`
//! (`docs/spec/unit-classification.md`).
//!
//! Every function is a plain model of one routine of Game.dll 1.26a and is pinned against the original by Unicorn
//! vectors (`tools/emu/unit_class_emu.py`, `crates/sim/tests/unit_class_vectors.rs`).
//!
//! The `unittype` argument of the natives is the `ConvertUnitType` index (`common.j`, 0 to 26). Values outside that
//! range make `IsUnitType` and `IsUnitIdType` answer false and `UnitAddType` / `UnitRemoveType` fail.

/// Movement type bits (unit `+0x1FC`; the same values as `gamedata::unitclass::move_bits`).
mod move_bits {
    pub const FOOT: u32 = 0x1;
    pub const FLY: u32 = 0x2;
    pub const HORSE: u32 = 0x4;
    pub const HOVER: u32 = 0x8;
}

/// Target flag bits `IsUnitType` reads (the same values as `gamedata::unitclass::target_bits`).
mod target_bits {
    pub const GROUND: u32 = 0x2;
    pub const AIR: u32 = 0x4;
}

/// The target flag words that make a weapon not count as melee or ranged (`0x6F0C5F80`, `0x6F088FD0`): `none`,
/// `tree`, `wall` and `debris`. CONFIRMED.
fn targs_excluded(flags: u32) -> bool {
    matches!(flags, 0x1 | 0x40 | 0x80 | 0x100)
}

/// The `unittype` values (`common.j` `ConvertUnitType(n)`).
pub mod ty {
    pub const HERO: u32 = 0;
    pub const DEAD: u32 = 1;
    pub const STRUCTURE: u32 = 2;
    pub const FLYING: u32 = 3;
    pub const GROUND: u32 = 4;
    pub const ATTACKS_FLYING: u32 = 5;
    pub const ATTACKS_GROUND: u32 = 6;
    pub const MELEE_ATTACKER: u32 = 7;
    pub const RANGED_ATTACKER: u32 = 8;
    pub const GIANT: u32 = 9;
    pub const SUMMONED: u32 = 10;
    pub const STUNNED: u32 = 11;
    pub const PLAGUED: u32 = 12;
    pub const SNARED: u32 = 13;
    pub const UNDEAD: u32 = 14;
    pub const MECHANICAL: u32 = 15;
    pub const PEON: u32 = 16;
    pub const SAPPER: u32 = 17;
    pub const TOWNHALL: u32 = 18;
    pub const ANCIENT: u32 = 19;
    pub const TAUREN: u32 = 20;
    pub const POISONED: u32 = 21;
    pub const POLYMORPHED: u32 = 22;
    pub const SLEEPING: u32 = 23;
    pub const RESISTANT: u32 = 24;
    pub const ETHEREAL: u32 = 25;
    pub const MAGIC_IMMUNE: u32 = 26;
    /// Number of values.
    pub const COUNT: u32 = 27;
}

/// The classification bit of a `unittype` value (the table at `0x6F942888`: pairs of mask and `unittype`), or `None`
/// for a value without a bit. Only these nine values can be added or removed. CONFIRMED.
pub fn class_bit(unittype: u32) -> Option<u32> {
    Some(match unittype {
        ty::GIANT => 0x1,
        ty::SUMMONED => 0x4,
        ty::UNDEAD => 0x2,
        ty::MECHANICAL => 0x8,
        ty::PEON => 0x10,
        ty::SAPPER => 0x20,
        ty::TOWNHALL => 0x40,
        ty::ANCIENT => 0x80,
        ty::TAUREN => 0x800,
        _ => return None,
    })
}

/// `UnitAddType` on the unit's classification word (unit `+0x248`): ORs the bit. Returns whether the call succeeded
/// (the native returns true exactly for the nine values of [`class_bit`]; the unit must resolve). CONFIRMED
/// (`0x6F3C7BC0`: the value minus 9 is range-checked against 11 and then screened by a byte map that passes 9, 10 and
/// 14 to 20).
pub fn add_type(word: &mut u32, unittype: u32) -> bool {
    match class_bit(unittype) {
        Some(bit) => {
            *word |= bit;
            true
        }
        None => false,
    }
}

/// `UnitRemoveType` (`0x6F3C7C20`): clears the bit. Same success rule as [`add_type`].
pub fn remove_type(word: &mut u32, unittype: u32) -> bool {
    match class_bit(unittype) {
        Some(bit) => {
            *word &= !bit;
            true
        }
        None => false,
    }
}

/// The abilities and buffs a state test walks (`unit +0x1DC` list; buffs are list members).
///
/// Each state test asks every list member's virtual method (slots 98 to 104 of `CAbility`) and answers true when one
/// returns non-zero. The base class answers 0; the overriding classes were found by scanning all 3,675 vtables
/// (`docs/spec/unit-classification.md` section 4). Only the classes that answer an unconditional 1 are modelled:
/// the classes whose answer depends on a field the model does not hold (`CBuffLavaMonster`, `CBuffPurge`,
/// `CBuffCargoHoldDeath` for snared, `CAbilitySleepAlways` / `CAbilityCreepSleep` for sleeping, which read a state
/// flag of the ability, and `CAbilityResistantSkin`) answer false here. UNSURE for those.
pub fn state_codes(unittype: u32) -> &'static [&'static str] {
    match unittype {
        ty::STUNNED => &["BSTN", "Bmlt", "Bfrz", "Bfre", "BUst"],
        ty::SNARED => &["Bens", "Bena", "Beng", "Bweb", "Bwea", "BEer"],
        ty::POISONED => &[
            "Bpoi", "Bpsd", "BIpb", "BIpd", "Bspo", "Bssd", "BNpa", "Bliq", "BNab", "BHca", "Bcsd",
            "BEsh",
        ],
        ty::PLAGUED => &["Bapl"],
        ty::POLYMORPHED => &["Bply", "BOhx"],
        ty::SLEEPING => &["BUsl", "BNdc"],
        _ => &[],
    }
}

/// Whether a list member with base code `base` makes the state `unittype` true.
pub fn code_gives_state(unittype: u32, base: u32) -> bool {
    state_codes(unittype)
        .iter()
        .any(|c| u32::from_be_bytes(c.as_bytes().try_into().expect("4 bytes")) == base)
}

/// The weapon data an attack ability holds (`attack +0xDC[w]` kind, `+0x218[w]` target flags), plus the enable bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeaponView {
    /// `weapTp` enum of each weapon (`0x6F262D30`).
    pub kind: [u32; 2],
    /// Target flag word of each weapon.
    pub targs: [u32; 2],
    /// Weapon enabled (flag `0x80000 << w` of the attack ability and no disabling counter fires), per weapon.
    pub enabled: [bool; 2],
}

/// The melee test of one weapon (`0x6F0C5F80` shape): kind 1 and target flags outside the excluded set.
pub fn weapon_melee(kind: u32, targs: u32) -> bool {
    kind == 1 && !targs_excluded(targs)
}

/// The ranged test of one weapon (`0x6F088FD0`): kind 2 to 8 and target flags outside the excluded set.
pub fn weapon_ranged(kind: u32, targs: u32) -> bool {
    (2..=8).contains(&kind) && !targs_excluded(targs)
}

/// The four attack-ability tests of `IsUnitType` (values 5 to 8) for a unit whose attack ability holds `w`
/// (`None` when the unit has no attack ability: false). CONFIRMED (`0x6F0C9840`, `0x6F0C9880`, `0x6F0C9710`,
/// `0x6F0C9770`: the first of the two weapons that is enabled and passes decides).
pub fn attack_test(unittype: u32, w: Option<&WeaponView>) -> bool {
    let Some(w) = w else { return false };
    (0..2).any(|i| {
        w.enabled[i]
            && match unittype {
                ty::ATTACKS_FLYING => w.targs[i] & target_bits::AIR != 0,
                ty::ATTACKS_GROUND => w.targs[i] & target_bits::GROUND != 0,
                ty::MELEE_ATTACKER => weapon_melee(w.kind[i], w.targs[i]),
                ty::RANGED_ATTACKER => weapon_ranged(w.kind[i], w.targs[i]),
                _ => false,
            }
    })
}

/// The object state `IsUnitType` reads for the non-list values.
#[derive(Debug, Clone, Default)]
pub struct UnitView {
    /// Raw type id (the hero test reads its first character).
    pub type_id: u32,
    /// Illusion flag (`+0x5C` bit 30).
    pub illusion: bool,
    /// Dead flag (`+0x5C` bit 8, the unit's alive test).
    pub dead: bool,
    /// Structure flag (`+0x5C` bit 16, from the type's `IsStructure`).
    pub structure: bool,
    /// Movement type bits (`+0x1FC`).
    pub movement: u32,
    /// Classification word (`+0x248`).
    pub class_flags: u32,
    /// Ethereal counter (`+0x158`) and magic immunity counter (`+0x15C`), signed.
    pub ethereal: i32,
    pub magic_immune: i32,
}

/// Whether the first character of a type id is an upper-case letter (heroes use `A` to `Z`).
pub fn id_is_hero(type_id: u32) -> bool {
    (type_id >> 24).wrapping_sub(0x41) <= 0x19
}

/// `IsUnitType(unit, unittype)` for the values that do not depend on the ability list or the attack ability
/// (everything except 5 to 8, 11 to 13 and 21 to 24, handled by [`attack_test`] and [`code_gives_state`]).
/// Returns `None` for those. CONFIRMED (`0x6F3C7E90`).
pub fn simple_test(v: &UnitView, unittype: u32) -> Option<bool> {
    Some(match unittype {
        // Hero: upper-case first character and not an illusion (`0x6F03FDC0`).
        ty::HERO => id_is_hero(v.type_id) && !v.illusion,
        ty::DEAD => v.dead,
        ty::STRUCTURE => v.structure,
        ty::FLYING => v.movement & move_bits::FLY != 0,
        ty::GROUND => v.movement & (move_bits::FOOT | move_bits::HORSE | move_bits::HOVER) != 0,
        ty::ETHEREAL => v.ethereal > 0,
        ty::MAGIC_IMMUNE => v.magic_immune > 0,
        t => {
            let bit = class_bit(t)?;
            v.class_flags & bit != 0
        }
    })
}

/// The record data `IsUnitIdType` reads.
#[derive(Debug, Clone, Default)]
pub struct IdView {
    pub structure: bool,
    pub movement: u32,
    pub classification: u32,
    pub weapons_on: u32,
    pub kind: [u32; 2],
    pub targs: [u32; 2],
}

/// `IsUnitIdType(id, unittype)` (`0x6F3B7F60`). CONFIRMED structure. Quirks reproduced: GROUND compares the movement
/// word with 0xD for equality (no real type has it), DEAD and the state values are always false, and the
/// weapon values read the weapon fields of the record gated by `weapsOn` bits 1 and 2 only.
pub fn id_type_test(type_id: u32, v: &IdView, unittype: u32) -> bool {
    match unittype {
        ty::HERO => id_is_hero(type_id),
        ty::STRUCTURE => v.structure,
        ty::FLYING => v.movement == move_bits::FLY,
        ty::GROUND => v.movement == 0xD,
        ty::ATTACKS_FLYING | ty::ATTACKS_GROUND => {
            let mut t = 0;
            for w in 0..2 {
                if v.weapons_on >> w & 1 != 0 {
                    t |= v.targs[w];
                }
            }
            let bit = if unittype == ty::ATTACKS_FLYING {
                target_bits::AIR
            } else {
                target_bits::GROUND
            };
            t & bit != 0
        }
        ty::MELEE_ATTACKER | ty::RANGED_ATTACKER => (0..2).any(|w| {
            v.weapons_on >> w & 1 != 0
                && if unittype == ty::MELEE_ATTACKER {
                    weapon_melee(v.kind[w], v.targs[w])
                } else {
                    weapon_ranged(v.kind[w], v.targs[w])
                }
        }),
        t => class_bit(t).is_some_and(|bit| v.classification & bit != 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_only_nine_values() {
        let mut w = 0;
        for t in 0..40 {
            let ok = add_type(&mut w, t);
            assert_eq!(
                ok,
                [9, 10, 14, 15, 16, 17, 18, 19, 20].contains(&t),
                "type {t}"
            );
        }
        assert_eq!(w, 0x1 | 0x4 | 0x2 | 0x8 | 0x10 | 0x20 | 0x40 | 0x80 | 0x800);
        assert!(remove_type(&mut w, ty::SUMMONED));
        assert_eq!(w & 4, 0);
        assert!(!remove_type(&mut w, ty::STUNNED));
    }

    #[test]
    fn hero_and_movement() {
        let hero = UnitView {
            type_id: u32::from_be_bytes(*b"Hpal"),
            ..UnitView::default()
        };
        let foot = UnitView {
            type_id: u32::from_be_bytes(*b"hfoo"),
            movement: 1,
            ..UnitView::default()
        };
        assert_eq!(simple_test(&hero, ty::HERO), Some(true));
        assert_eq!(simple_test(&foot, ty::HERO), Some(false));
        assert_eq!(simple_test(&foot, ty::GROUND), Some(true));
        assert_eq!(simple_test(&foot, ty::FLYING), Some(false));
        assert_eq!(simple_test(&foot, ty::ATTACKS_GROUND), None);
        assert_eq!(simple_test(&foot, 99), None);
    }

    #[test]
    fn id_ground_quirk() {
        let v = IdView {
            movement: 1,
            ..IdView::default()
        };
        assert!(!id_type_test(0, &v, ty::GROUND));
        assert!(id_type_test(
            0,
            &IdView {
                movement: 0xD,
                ..IdView::default()
            },
            ty::GROUND
        ));
    }
}
