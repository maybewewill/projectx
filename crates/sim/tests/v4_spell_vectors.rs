//! Vectors from the original `Game.dll` for the melee unit spells (`tools/emu/v4_spells_emu.py`, task V4): the original
//! effect slots / target rules / autocast filters run on fake objects; the arguments of the recorded callees are the
//! rows. Only derived results are stored (no game data).

use sim::unit_spells::*;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

fn f(x: u32) -> F32 {
    F32::from_bits(x)
}

/// The maker of a marker from the pair of words the buff stores (all ones = no maker).
fn maker(p0: u32, p1: u32, resolved: u32) -> Option<u32> {
    if p0 & p1 == 0xFFFF_FFFF {
        None
    } else {
        Some(resolved)
    }
}

fn caster_of(ownerp: u32) -> u32 {
    if ownerp == 0 {
        0x1234
    } else {
        ownerp
    }
}

#[test]
fn heal_effect_matches_the_original() {
    // da cool eth bonus existing | amount calls | existing dur calls | created dur calls
    let rows = hex_rows(include_str!("data/unit_spells/heal_effect.txt"));
    assert!(rows.len() >= 300);
    let (mut eth, mut plain, mut ex, mut cr) = (0, 0, 0, 0);
    for r in &rows {
        let (amount, dur) = heal_values(f(r[0]), f(r[1]), r[2] as i32, f(r[3]));
        assert_eq!((r[5], r[6]), (amount.to_bits(), 1), "life adder amount, row {:08x?}", r);
        if r[4] != 0 {
            assert_eq!((r[7], r[8], r[10]), (dur.to_bits(), 1, 0), "existing marker, row {:08x?}", r);
            ex += 1;
        } else {
            assert_eq!((r[9], r[10], r[8]), (dur.to_bits(), 1, 0), "created marker, row {:08x?}", r);
            cr += 1;
        }
        if (r[2] as i32) > 0 {
            eth += 1;
        } else {
            plain += 1;
        }
    }
    assert!(eth > 60 && plain > 60 && ex > 60 && cr > 60);
}

#[test]
fn heal_target_rule_matches_the_original() {
    // can illusion life max marker p0 p1 resolved ownerptr | code
    let rows = hex_rows(include_str!("data/unit_spells/heal_target.txt"));
    assert!(rows.len() >= 300);
    let mut seen = std::collections::BTreeMap::new();
    for r in &rows {
        let marker = if r[4] != 0 { Some(maker(r[5], r[6], r[7])) } else { None };
        let code = heal_target_code(r[0] != 0, r[1] != 0, f(r[2]), f(r[3]), marker, caster_of(r[8]));
        assert_eq!(r[9], code, "row {:08x?}", r);
        *seen.entry(code).or_insert(0) += 1;
    }
    for code in [0u32, 0x1C, 0xA2, 0xAE] {
        assert!(seen.get(&code).copied().unwrap_or(0) > 10, "code {code:#x} {seen:?}");
    }
}

#[test]
fn heal_autocast_filter_matches_the_original() {
    // base getac illusion ally regen marker p0 p1 resolved ownerptr | result
    let rows = hex_rows(include_str!("data/unit_spells/heal_filter.txt"));
    assert!(rows.len() >= 300);
    let (mut yes, mut no) = (0, 0);
    for r in &rows {
        let marker = if r[5] != 0 { Some(maker(r[6], r[7], r[8])) } else { None };
        let ok = heal_filter_ok(r[0] != 0, r[1] != 0, r[2] != 0, r[3] != 0, f(r[4]), marker, caster_of(r[9]));
        assert_eq!(r[10], ok as u32, "row {:08x?}", r);
        if ok {
            yes += 1;
        } else {
            no += 1;
        }
    }
    assert!(yes > 20 && no > 20, "{yes} {no}");
}
