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

fn level_of(data: &[u32], dur: u32, hdur: u32) -> sim::cast::SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    sim::cast::SpellLevel { dur: f(dur), hero_dur: f(hdur), data: d, ..sim::cast::SpellLevel::default() }
}

#[test]
fn dispel_magic_worker_arguments_match_the_original() {
    // DataA DataB | ecx edx target order ability pos neg a6 a7 damage mana flags
    let rows = hex_rows(include_str!("data/unit_spells/dispel_args.txt"));
    assert!(rows.len() >= 300);
    for r in &rows {
        let a = dispel_magic_args(&level_of(&[r[0], r[1]], 0, 0));
        let w = &r[2..];
        assert_eq!((w[5], w[6], w[7], w[8]), (a.positive, a.negative, a.a6, a.a7), "row {:08x?}", r);
        assert_eq!((w[9], w[10], w[11]), (a.damage.to_bits(), a.mana.to_bits(), a.flags), "row {:08x?}", r);
        assert_eq!(w[3], 0xD0059, "order id of Dispel Magic");
    }
}

#[test]
fn abolish_magic_worker_arguments_match_the_original() {
    // misc flags DataC reqLevel not_ally DataA DataB | ecx edx target order ability pos neg a6 a7 damage mana flags
    let rows = hex_rows(include_str!("data/unit_spells/abolish_args.txt"));
    assert!(rows.len() >= 300);
    let (mut smart, mut plain, mut a7zero) = (0, 0, 0);
    for r in &rows {
        let l = level_of(&[r[5], r[6], r[2]], 0, 0);
        let a = abolish_args(r[0] != 0, r[1], &l, r[4] != 0, r[3]);
        let w = &r[7..];
        assert_eq!((w[5], w[6], w[7], w[8]), (a.positive, a.negative, a.a6, a.a7), "row {:08x?}", r);
        assert_eq!((w[9], w[10], w[11]), (a.damage.to_bits(), a.mana.to_bits(), a.flags), "row {:08x?}", r);
        assert_eq!(w[3], 0xD00A4, "order id of Abolish Magic");
        if a.positive != a.negative {
            smart += 1;
        } else {
            plain += 1;
        }
        a7zero += usize::from(a.a7 == 0);
    }
    assert!(smart > 40 && plain > 40 && a7zero > 5, "{smart} {plain} {a7zero}");
}

fn check_buff_apply(text: &str, curse: bool) {
    // dur herodur DataA extra hero existing | existing calls, new calls | 4 recorded words
    let rows = hex_rows(text);
    assert!(rows.len() >= 300);
    let (mut ex, mut cr, mut heroes) = (0, 0, 0);
    for r in &rows {
        let l = level_of(&[r[2]], r[0], r[1]);
        let hero = r[4] != 0;
        let dur = sim::cast::effect_duration(&l, hero, f(r[3])).to_bits();
        let w = &r[8..];
        if r[5] != 0 {
            ex += 1;
            assert_eq!((r[6], r[7]), (1, 0), "row {:08x?}", r);
            // curse: dur DataA owner; faerie: dur DataA owner (recorded in the same order)
            assert_eq!((w[0], w[1], w[2]), (dur, r[2], 0x77), "existing buff, row {:08x?}", r);
        } else {
            cr += 1;
            assert_eq!((r[6], r[7]), (0, 1), "row {:08x?}", r);
            assert_eq!((w[0], w[1], w[2], w[3]), (0x402000, dur, r[2], 0x77), "created buff, row {:08x?}", r);
        }
        heroes += usize::from(hero);
    }
    let _ = curse;
    assert!(ex > 60 && cr > 100 && heroes > 100);
}

#[test]
fn curse_effect_matches_the_original() {
    check_buff_apply(include_str!("data/unit_spells/curse_apply.txt"), true);
}

#[test]
fn faerie_fire_effect_matches_the_original() {
    check_buff_apply(include_str!("data/unit_spells/faerie_apply.txt"), false);
}

#[test]
fn curse_autocast_filter_matches_the_original() {
    // base has_attack target_present rel frac DataB attacks has_buff | result
    let rows = hex_rows(include_str!("data/unit_spells/curse_filter.txt"));
    assert!(rows.len() >= 300);
    let (mut yes, mut no) = (0, 0);
    for r in &rows {
        let ok = curse_filter_ok(r[0] != 0, r[1] != 0, r[2] != 0, r[3] != 0, r[7] != 0);
        assert_eq!(r[8], ok as u32, "row {:08x?}", r);
        if ok {
            yes += 1;
        } else {
            no += 1;
        }
    }
    assert!(yes > 15 && no > 100, "{yes} {no}");
}

#[test]
fn faerie_fire_autocast_filter_matches_the_original() {
    let rows = hex_rows(include_str!("data/unit_spells/faerie_filter.txt"));
    assert!(rows.len() >= 300);
    let (mut yes, mut no) = (0, 0);
    for r in &rows {
        let attacks = r[6] != 0 && r[1] != 0;
        // the pair of the attack object is only resolved when the candidate attacks
        let ok = faerie_filter_ok(r[0] != 0, f(r[4]), f(r[5]), attacks && r[2] != 0, r[3] != 0, r[7] != 0);
        assert_eq!(r[8], ok as u32, "row {:08x?}", r);
        if ok {
            yes += 1;
        } else {
            no += 1;
        }
    }
    assert!(yes > 15 && no > 100, "{yes} {no}");
}

fn game_tables() -> Option<softfloat::Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(softfloat::Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

#[test]
fn taunt_comparators_match_the_original() {
    use sim::unit_abilities_m::{taunt_compare, TauntRec};
    // mode | A: f1 f2 f3 f4 id | B: f1 f2 f3 f4 id | result
    let rows = hex_rows(include_str!("data/unit_spells/taunt_compare.txt"));
    assert!(rows.len() >= 300);
    let mut seen = std::collections::BTreeSet::new();
    for r in &rows {
        let a = TauntRec { f1: f(r[1]), f2: f(r[2]), f3: r[3] as i32, f4: r[4] as i32, id: r[5] as i32 };
        let b = TauntRec { f1: f(r[6]), f2: f(r[7]), f3: r[8] as i32, f4: r[9] as i32, id: r[10] as i32 };
        let got = taunt_compare(r[0], &a, &b);
        assert_eq!(r[11], got as u32, "row {:08x?}", r);
        seen.insert((r[0], got.signum()));
    }
    assert!(seen.len() >= 8, "{seen:?}");
}

#[test]
fn taunt_selection_and_application_match_the_original() {
    use sim::unit_abilities_m::{taunt_plan, TauntRec};
    // n DataA DataB DataC dur herodur extra | 10 x (f1 f2 f3 f4 id hero) | applied | 10 x (index, dur)
    let rows = hex_rows(include_str!("data/unit_spells/taunt_effect.txt"));
    assert!(rows.len() >= 300);
    let (mut limited, mut unlimited, mut big) = (0, 0, 0);
    for r in &rows {
        let n = r[0] as usize;
        let l = level_of(&[r[1], r[2], r[3]], r[4], r[5]);
        let recs: Vec<(TauntRec, bool)> = (0..n)
            .map(|k| {
                let o = 7 + 6 * k;
                (TauntRec { f1: f(r[o]), f2: f(r[o + 1]), f3: r[o + 2] as i32, f4: r[o + 3] as i32, id: r[o + 4] as i32 }, r[o + 5] != 0)
            })
            .collect();
        let plan = taunt_plan(&l, f(r[6]), &recs);
        assert_eq!(r[67] as usize, plan.len(), "applied count, row {:08x?}", r);
        for (i, (idx, dur)) in plan.iter().enumerate() {
            assert_eq!((r[68 + 2 * i], r[69 + 2 * i]), (*idx as u32, dur.to_bits()), "application {i}, row {:08x?}", r);
        }
        if softfloat::conv::f2i(f(r[1])) != 0 && plan.len() < n {
            limited += 1;
        } else {
            unlimited += 1;
        }
        big += usize::from(n > 8);
    }
    assert!(limited > 40 && unlimited > 40 && big > 40, "{limited} {unlimited} {big}");
}

#[test]
fn unholy_frenzy_effect_matches_the_original() {
    // dur herodur DataA DataB extra hero existing | calls e n | existing: dur owner B A / new: target dur owner B A
    let rows = hex_rows(include_str!("data/unit_spells/unholy_apply.txt"));
    assert!(rows.len() >= 300);
    let (mut ex, mut cr) = (0, 0);
    for r in &rows {
        let l = level_of(&[r[2], r[3]], r[0], r[1]);
        let (dur, a, b) = unholy_frenzy_values(&l, r[5] != 0, f(r[4]));
        let w = &r[9..];
        if r[6] != 0 {
            ex += 1;
            assert_eq!((r[7], r[8]), (1, 0), "row {:08x?}", r);
            assert_eq!((w[0], w[1], w[2], w[3]), (dur.to_bits(), 0x202000, b.to_bits(), a.to_bits()), "existing, row {:08x?}", r);
        } else {
            cr += 1;
            assert_eq!((r[7], r[8]), (0, 1), "row {:08x?}", r);
            assert_eq!((w[0], w[1], w[2], w[3], w[4]), (0x402000, dur.to_bits(), 0x202000, b.to_bits(), a.to_bits()), "created, row {:08x?}", r);
        }
    }
    assert!(ex > 60 && cr > 100);
}

#[test]
fn rejuvenation_effect_matches_the_original() {
    // dur herodur A B C extra hero existing | calls e n | existing: dur A B f1 f2 / new: target dur A B f1 f2
    let rows = hex_rows(include_str!("data/unit_spells/rejuv_effect.txt"));
    assert!(rows.len() >= 300);
    let (mut ex, mut cr, mut bits) = (0, 0, [0u32; 4]);
    for r in &rows {
        let l = level_of(&[r[2], r[3], r[4]], r[0], r[1]);
        let (dur, a, b, f1, f2) = rejuvenation_effect_args(&l, r[6] != 0, f(r[5]));
        let w = &r[10..];
        if r[7] != 0 {
            ex += 1;
            assert_eq!((r[8], r[9]), (1, 0), "row {:08x?}", r);
            assert_eq!((w[0], w[1], w[2], w[3], w[4]), (dur.to_bits(), a.to_bits(), b.to_bits(), f1, f2), "existing, row {:08x?}", r);
        } else {
            cr += 1;
            assert_eq!((r[8], r[9]), (0, 1), "row {:08x?}", r);
            assert_eq!((w[0], w[1], w[2], w[3], w[4], w[5]), (0x402000, dur.to_bits(), a.to_bits(), b.to_bits(), f1, f2), "created, row {:08x?}", r);
        }
        bits[(f1 + f2 / 2 * 2) as usize] += 1;
    }
    assert!(ex > 60 && cr > 100 && bits.iter().all(|n| *n > 5), "{bits:?}");
}

#[test]
fn rejuvenation_buff_apply_matches_the_original() {
    // T A B f1 f2 flags0 | set-duration value | flags1 | life per tick | mana per tick
    let Some(t) = game_tables() else {
        return;
    };
    let div = |a: F32, b: F32| softfloat::arith::div(a, b, &t);
    let rows = hex_rows(include_str!("data/unit_spells/rejuv_buff.txt"));
    assert!(rows.len() >= 300);
    for r in &rows {
        let (dur, life, mana) = rejuv_buff_values(f(r[0]), f(r[1]), f(r[2]), &div);
        assert_eq!(r[6], dur.to_bits(), "duration, row {:08x?}", r);
        assert_eq!((r[8], r[9]), (life.to_bits(), mana.to_bits()), "per tick, row {:08x?}", r);
        let want = (r[5] & !0x300) | if r[3] != 0 { 0x100 } else { 0 } | if r[4] != 0 { 0x200 } else { 0 };
        assert_eq!(r[7], want, "flags, row {:08x?}", r);
    }
}

#[test]
fn polymorph_target_rule_matches_the_original() {
    // DataA creep_over summoned | code
    let rows = hex_rows(include_str!("data/unit_spells/polymorph_target.txt"));
    assert!(rows.len() >= 300);
    let mut seen = std::collections::BTreeMap::new();
    for r in &rows {
        let code = polymorph_target_code(f(r[0]), r[1] != 0, r[2] != 0);
        assert_eq!(r[3], code, "row {:08x?}", r);
        *seen.entry(code).or_insert(0) += 1;
    }
    for c in [0u32, 0x33, 0x9F] {
        assert!(seen.get(&c).copied().unwrap_or(0) > 20, "{seen:?}");
    }
}

#[test]
fn polymorph_effect_matches_the_original() {
    // dur herodur illusion hero movement | slot124 calls, amount | pick calls, level, movement | slot43C calls, target, dur, type
    let rows = hex_rows(include_str!("data/unit_spells/polymorph_effect.txt"));
    assert!(rows.len() >= 300);
    let (mut ill, mut morph) = (0, 0);
    for r in &rows {
        let l = level_of(&[], r[0], r[1]);
        match polymorph_effect_duration(&l, r[3] != 0, r[2] != 0) {
            None => {
                ill += 1;
                assert_eq!((r[5], r[6], r[7], r[10]), (1, 0, 0, 0), "illusion killed only, row {:08x?}", r);
            }
            Some(d) => {
                morph += 1;
                assert_eq!((r[5], r[7], r[9], r[10]), (0, 1, r[4], 1), "row {:08x?}", r);
                assert_eq!(r[12], d.to_bits(), "duration, row {:08x?}", r);
            }
        }
    }
    assert!(ill > 60 && morph > 200);
}

#[test]
fn abolish_magic_autocast_filter_matches_the_original() {
    // base ownerflags cand248 ally req | result calls | ecx edx a2..a10
    let rows = hex_rows(include_str!("data/unit_spells/abolish_filter.txt"));
    assert!(rows.len() >= 300);
    let (mut scanned, mut rejected) = (0, 0);
    for r in &rows {
        let got = abolish_filter_scan(r[0] != 0, r[1], r[2], r[3] != 0, r[4], 0x202000);
        match got {
            None => {
                rejected += 1;
                assert_eq!((r[5], r[6]), (0, 0), "row {:08x?}", r);
            }
            Some(a) => {
                scanned += 1;
                assert_eq!((r[5], r[6]), (1, 1), "row {:08x?}", r);
                assert_eq!(r[9..18], a[..], "scan arguments, row {:08x?}", r);
            }
        }
    }
    assert!(scanned > 100 && rejected > 100, "{scanned} {rejected}");
}

#[test]
fn purge_effect_arguments_match_the_original() {
    // 0..6 DataA..F | 6 dur 7 herodur 8 dead 9 allied 10 hero 11 existing | 12.. worker (ecx edx target order ability pos neg
    // a6 a7 damage mana flags) | 24..31 apply (target dur owner A B D E) | 31 call count | 32 removal calls
    let rows = hex_rows(include_str!("data/unit_spells/purge_args.txt"));
    assert!(rows.len() >= 300);
    let (mut applied, mut gated, mut removed) = (0, 0, 0);
    for r in &rows {
        let l = level_of(&r[0..6], r[6], r[7]);
        let a = purge_args(&l);
        let w = &r[12..24];
        assert_eq!((w[5], w[6], w[7], w[8]), (a.positive, a.negative, a.a6, a.a7), "row {:08x?}", r);
        assert_eq!((w[9], w[10], w[11]), (a.damage.to_bits(), a.mana.to_bits(), a.flags), "row {:08x?}", r);
        assert_eq!(w[3], 0xD008F, "order id of Purge");
        if purge_slows(r[8] != 0, r[9] != 0) {
            applied += 1;
            let dur = sim::cast::effect_duration(&l, r[10] != 0, F32::ZERO).to_bits();
            // target dur owner A B D E
            assert_eq!((r[25], r[27], r[28], r[29], r[30]), (dur, r[0], r[1], r[3], r[4]), "apply arguments, row {:08x?}", r);
            assert_eq!(r[31], 2, "worker and apply called, row {:08x?}", r);
            assert_eq!(r[32], u32::from(r[11] != 0), "existing Bprg removed, row {:08x?}", r);
            removed += usize::from(r[11] != 0);
        } else {
            gated += 1;
            assert_eq!((r[31], r[32]), (1, 0), "only the worker runs, row {:08x?}", r);
        }
    }
    assert!(applied > 150 && gated > 40 && removed > 40, "{applied} {gated} {removed}");
}
