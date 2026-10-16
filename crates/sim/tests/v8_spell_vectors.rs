//! Vectors from the original `Game.dll` for the V8 verification wave (`tools/emu/v8_spells_emu.py`, docs/spec/abilities-unit-spells.md
//! section 14). The original per-unit effect and buff routines run on fake objects; only derived results are stored.

use sim::cast::SpellLevel;
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

fn level(dur: u32, hdur: u32, data: &[u32]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel { dur: f(dur), hero_dur: f(hdur), data: d, ..SpellLevel::default() }
}

#[test]
fn roar_unit_matches_the_original() {
    // 0..4: A B C D | 4 duration argument | 5..9 weapon 0 (count sides bonus targs) | 9..13 weapon 1 |
    // 13 arg1, 14 duration | 15, 16 counter deltas | 17, 18 stored amounts | 19, 20 armor value+calls |
    // 21, 22 life regeneration | 23, 24 mana regeneration
    let rows = hex_rows(include_str!("data/unit_spells/roar_unit.txt"));
    assert!(rows.len() >= 300);
    let (mut skipped, mut applied, mut negative) = (0, 0, 0);
    for r in &rows {
        let l = level(0, 0, &[r[0], r[1], r[2], r[3]]);
        let (fraction, armor, life, mana) = roar_unit_values(&l);
        assert_eq!(r[14], r[4], "duration reaches the buff unchanged, row {:08x?}", r);
        assert_eq!((r[19], r[20]), (armor.to_bits(), 1), "armor sink, row {:08x?}", r);
        for (value, base, regen) in [(r[21], r[22], life), (r[23], r[24], mana)] {
            let want = inner_fire_regen(regen);
            if base == 0 {
                assert_eq!(want.to_bits() & 0x7FFF_FFFF, 0, "regeneration must not be created, row {:08x?}", r);
            } else {
                assert_eq!((value, base), (want.to_bits(), 1), "regeneration, row {:08x?}", r);
            }
        }
        for w in 0..2 {
            let o = 5 + 4 * w;
            let got = inner_fire_weapon_bonus(fraction, r[o] as i32, r[o + 1] as i32, r[o + 2] as i32, r[o + 3]);
            let (delta, stored) = (r[15 + w], r[17 + w]);
            match got {
                None => {
                    assert_eq!((delta, stored), (0, 0), "skipped weapon {w}, row {:08x?}", r);
                    skipped += 1;
                }
                Some(a) => {
                    assert_eq!((delta, stored), (a as u32, a as u32), "weapon {w}, row {:08x?}", r);
                    applied += 1;
                    if a < 0 {
                        negative += 1;
                    }
                }
            }
        }
    }
    assert!(skipped > 20 && applied > 300 && negative > 20, "{skipped} {applied} {negative}");
}

#[test]
fn unholy_frenzy_buff_apply_matches_the_original() {
    // 0..8: dur DataA DataB player illusion misc_flag has_attack pre_flags | 8 stored amount, 9 stored attack speed, 10 player,
    // 11 flags after | 12, 13 slot 0x320 arg1 arg2 | 14, 15 timer delay, calls | 16, 17, 18 sink value, flag, calls
    let rows = hex_rows(include_str!("data/unit_spells/unholy_buff.txt"));
    assert!(rows.len() >= 300);
    let (mut sunk, mut illusion_skipped, mut no_attack) = (0, 0, 0);
    for r in &rows {
        let (flags, sink) = unholy_apply_plan(r[7], r[4] != 0, r[5] != 0, r[6] != 0, f(r[1]));
        assert_eq!((r[8], r[9], r[10]), (r[2], r[1], r[3]), "stored values, row {:08x?}", r);
        assert_eq!(r[13], r[0], "duration, row {:08x?}", r);
        assert_eq!((r[14], r[15]), (0x3C23_D70A, 1), "first tick delay, row {:08x?}", r);
        assert_eq!(r[11], flags, "buff flags, row {:08x?}", r);
        match sink {
            Some(v) => {
                assert_eq!((r[16], r[18]), (v.to_bits(), 1), "sink, row {:08x?}", r);
                sunk += 1;
            }
            None => {
                assert_eq!(r[18], 0, "no sink call, row {:08x?}", r);
                if r[6] == 0 {
                    no_attack += 1;
                } else if r[4] != 0 {
                    illusion_skipped += 1;
                }
            }
        }
    }
    assert!(sunk > 100 && illusion_skipped > 5 && no_attack > 20, "{sunk} {illusion_skipped} {no_attack}");
}

#[test]
fn lightning_shield_victim_matches_the_original() {
    // 0..4: marker victim_ec is_self blocked | 4 return | 5 marker calls, 6 marker arg is the victim, 7 marker duration word, 8 third |
    // 9 register calls | 10 blocked calls | 11 damage calls, 12 damage ecx is the buff, 13 damage arg is the victim
    let rows = hex_rows(include_str!("data/unit_spells/shield_victim.txt"));
    assert!(rows.len() >= 300);
    let (mut skip, mut marker_only, mut damage) = (0, 0, 0);
    for r in &rows {
        let plan = shield_victim_plan(r[0] != 0, r[1] as i32, r[2] != 0, r[3] != 0);
        match plan {
            ShieldVictim::Skip => {
                assert_eq!((r[5], r[9], r[10], r[11]), (0, 0, 0, 0), "skipped victim, row {:08x?}", r);
                skip += 1;
            }
            ShieldVictim::MarkerOnly => {
                assert_eq!((r[5], r[6], r[8], r[9], r[10], r[11]), (1, 1, 0, 1, 1, 0), "marker only, row {:08x?}", r);
                marker_only += 1;
            }
            ShieldVictim::MarkerAndDamage => {
                assert_eq!((r[5], r[6], r[8], r[9], r[10], r[11], r[12], r[13]), (1, 1, 0, 1, 1, 1, 1, 1), "damage, row {:08x?}", r);
                damage += 1;
            }
        }
        assert_eq!(r[4], u32::from(plan != ShieldVictim::Skip), "return value, row {:08x?}", r);
    }
    assert!(skip > 100 && marker_only > 20 && damage > 50, "{skip} {marker_only} {damage}");
}

#[test]
fn polymorph_morph_matches_the_original() {
    // 0..9: dur herodur DataA illusion hero movement existing type unit_flags | 9 slot-0x124 calls | 11 refresh calls |
    // 14 registrations | 15 stored type, 16 buff flags | 20 duration setter calls, 22 duration | 23 timer calls, 24 delay |
    // 25 morph request calls | 29 Cast
    let rows = hex_rows(include_str!("data/unit_spells/polymorph_morph.txt"));
    assert!(rows.len() >= 300);
    let (mut illusions, mut refreshed, mut morphed, mut zero_cast) = (0, 0, 0, 0);
    for r in &rows {
        let l = level(r[0], r[1], &[r[2]]);
        match polymorph_effect_duration(&l, r[4] != 0, r[3] != 0) {
            None => {
                illusions += 1;
                assert_eq!((r[9], r[11], r[14], r[20], r[25]), (1, 0, 0, 0, 0), "illusion, row {:08x?}", r);
            }
            Some(d) if r[6] != 0 => {
                refreshed += 1;
                assert_eq!((r[9], r[11], r[14], r[20], r[25]), (0, 1, 0, 0, 0), "refresh, row {:08x?}", r);
                let _ = d;
            }
            Some(d) => {
                morphed += 1;
                let (total, delay, flags) = polymorph_apply_plan(d, f(r[29]), r[8]);
                assert_eq!((r[9], r[11], r[14], r[20], r[23], r[25]), (0, 0, 1, 1, 1, 1), "new buff, row {:08x?}", r);
                assert_eq!(r[15], r[7], "stored type, row {:08x?}", r);
                assert_eq!(r[22], total.to_bits(), "duration, row {:08x?}", r);
                assert_eq!(r[24], delay.to_bits(), "morph delay, row {:08x?}", r);
                assert_eq!(r[16], flags, "buff flags, row {:08x?}", r);
                if r[29] & 0x7FFF_FFFF == 0 {
                    zero_cast += 1;
                }
            }
        }
    }
    assert!(illusions > 30 && refreshed > 30 && morphed > 150 && zero_cast > 0, "{illusions} {refreshed} {morphed} {zero_cast}");
}

#[test]
fn dot_tick_matches_the_original() {
    // 0..13: amount class misc imm_magic imm_phys busl s368 bonus buff_flags f4 f0 life0 life1 | 13 damage calls (slot 0x120),
    // 14 builder calls | 15, 16 builder ecx edx | 17 amount, 18 class, 19 source, 20 flag bits, 21 zero | 22 buff flags at the damage
    // call | 23, 24 slot 0x120 args | 25 heal calls, 26 heal amount | 27 post calls, 28 post delta
    let rows = hex_rows(include_str!("data/unit_spells/dot_tick.txt"));
    assert!(rows.len() >= 300);
    let (mut skip, mut damage, mut heal, mut sleeping, mut immune) = (0, 0, 0, 0, 0);
    for r in &rows {
        let has_source = r[9] & r[10] != 0xFFFF_FFFF;
        let plan = dot_tick_plan(f(r[0]), r[1], r[2] != 0, r[3] as i32, r[4] as i32, r[5] != 0, r[6] != 0, f(r[7]), r[8], has_source);
        match plan {
            DotPlan::Skip => {
                assert_eq!((r[13], r[14], r[25], r[27]), (0, 0, 0, 0), "skipped tick, row {:08x?}", r);
                skip += 1;
                if r[5] != 0 {
                    sleeping += 1;
                } else {
                    immune += 1;
                }
            }
            DotPlan::Damage { amount, class, has_source, flag_bits } => {
                assert_eq!((r[13], r[14], r[25]), (1, 1, 0), "damage tick, row {:08x?}", r);
                assert_eq!((r[17], r[18], r[20], r[21]), (amount.to_bits(), class, flag_bits, 0), "damage record, row {:08x?}", r);
                assert_eq!(r[19] != 0, has_source, "source, row {:08x?}", r);
                assert_ne!(r[22] & 0x0200_0000, 0, "flag during the damage call, row {:08x?}", r);
                assert_eq!(r[27], 1, "post hook, row {:08x?}", r);
                assert_eq!(r[28], softfloat::arith::sub(f(r[11]), f(r[12])).to_bits(), "post delta, row {:08x?}", r);
                damage += 1;
            }
            DotPlan::Heal { amount } => {
                assert_eq!((r[13], r[25], r[26]), (0, 1, amount.to_bits()), "heal tick, row {:08x?}", r);
                assert_eq!(r[27], 1, "post hook, row {:08x?}", r);
                assert_eq!(r[28], softfloat::arith::sub(f(r[11]), f(r[11])).to_bits(), "post delta, row {:08x?}", r);
                heal += 1;
            }
        }
    }
    assert!(skip > 20 && damage > 100 && heal > 20 && sleeping > 10 && immune > 10, "{skip} {damage} {heal} {sleeping} {immune}");
}
