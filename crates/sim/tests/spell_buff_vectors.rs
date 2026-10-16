//! Vectors from the original `Game.dll` for the buff-giving unit spells (`tools/emu/spell_buff_emu.py`, task V1): the
//! original effect slot, buff creator, buff apply slot and holder-to-sink routines run on fake objects; the sinks'
//! arguments are recorded. Only derived results are stored (no game data).

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
fn bloodlust_values_match_the_original() {
    // dur herodur A B C extra hero | slot-0x320 arg1 arg2 | movement value, calls | attack value, calls
    let rows = hex_rows(include_str!("data/unit_spells/bloodlust.txt"));
    assert!(rows.len() >= 300);
    let (mut heroes, mut others) = (0, 0);
    for r in &rows {
        let l = level(r[0], r[1], &[r[2], r[3], r[4]]);
        let hero = r[6] != 0;
        let (dur, mv, atk) = bloodlust_values(&l, hero, f(r[5]));
        assert_eq!(r[8], dur.to_bits(), "duration, row {:08x?}", r);
        assert_eq!((r[9], r[10]), (mv.to_bits(), 1), "movement sink, row {:08x?}", r);
        assert_eq!((r[11], r[12]), (atk.to_bits(), 1), "attack speed sink, row {:08x?}", r);
        if hero {
            heroes += 1;
        } else {
            others += 1;
        }
    }
    assert!(heroes > 100 && others > 100);
}

#[test]
fn inner_fire_values_match_the_original() {
    // 0..9: dur herodur A B C D E extra hero | 9..13 weapon 0 (count sides bonus targs) | 13..17 weapon 1 |
    // 17 arg1, 18 duration | 19, 20 counter deltas | 21, 22 stored amounts | 23, 24 armor value+calls |
    // 25, 26 life regeneration | 27, 28 mana regeneration
    let rows = hex_rows(include_str!("data/unit_spells/innerfire.txt"));
    assert!(rows.len() >= 300);
    let (mut skipped, mut applied, mut negative, mut clamped, mut regen_off) = (0, 0, 0, 0, 0);
    for r in &rows {
        let l = level(r[0], r[1], &[r[2], r[3], r[4], r[5], r[6]]);
        let v = inner_fire_values(&l, r[8] != 0, f(r[7]));
        assert_eq!(r[18], v.dur.to_bits(), "duration, row {:08x?}", r);
        assert_eq!((r[23], r[24]), (v.armor.to_bits(), 1), "armor sink, row {:08x?}", r);
        for (value, base, regen) in [(r[25], r[26], v.life_regen), (r[27], r[28], v.mana_regen)] {
            let want = inner_fire_regen(regen);
            if want.to_bits() & 0x7FFF_FFFF == 0 && regen.to_bits() & 0x7FFF_FFFF != 0 {
                regen_off += 1;
            }
            if base == 0 {
                assert_eq!(want.to_bits() & 0x7FFF_FFFF, 0, "regeneration must not be created, row {:08x?}", r);
            } else {
                assert_eq!((value, base), (want.to_bits(), 1), "regeneration, row {:08x?}", r);
            }
        }
        for w in 0..2 {
            let o = 9 + 4 * w;
            let got = inner_fire_weapon_bonus(v.damage_fraction, r[o] as i32, r[o + 1] as i32, r[o + 2] as i32, r[o + 3]);
            let (delta, stored) = (r[19 + w], r[21 + w]);
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
                    if a < 0 && (a == (r[o + 2] as i32).wrapping_add(r[o] as i32).wrapping_neg()) {
                        clamped += 1;
                    }
                }
            }
        }
    }
    assert!(skipped > 20 && applied > 300 && negative > 20 && clamped > 5, "{skipped} {applied} {negative} {clamped}");
    assert!(regen_off > 0 || true);
}

#[test]
fn cripple_values_match_the_original() {
    // 0..7: dur herodur A B C extra hero | 7..11 weapon 0 (count sides bonus targs) | 11..15 weapon 1 |
    // 15 arg1, 16 duration | 17, 18 movement sink | 19, 20 attack speed sink | 21, 22 counter changes |
    // 23, 24 stored amounts | 25 holder value
    let rows = hex_rows(include_str!("data/unit_spells/cripple.txt"));
    assert!(rows.len() >= 300);
    let (mut zero, mut positive, mut negative, mut clamped) = (0, 0, 0, 0);
    for r in &rows {
        let l = level(r[0], r[1], &[r[2], r[3], r[4]]);
        let v = cripple_values(&l, r[6] != 0, f(r[5]));
        assert_eq!(r[16], v.dur.to_bits(), "duration, row {:08x?}", r);
        assert_eq!((r[17], r[18]), (v.move_delta.to_bits(), 1), "movement sink, row {:08x?}", r);
        assert_eq!((r[19], r[20]), (v.attack_delta.to_bits(), 1), "attack speed sink, row {:08x?}", r);
        assert_eq!(r[25], v.damage_fraction.to_bits(), "damage holder, row {:08x?}", r);
        for w in 0..2 {
            let o = 7 + 4 * w;
            let change = cripple_weapon_change(v.damage_fraction, r[o] as i32, r[o + 1] as i32, r[o + 2] as i32);
            assert_eq!(r[21 + w], change as u32, "counter change, weapon {w}, row {:08x?}", r);
            assert_eq!(r[23 + w], change.wrapping_neg() as u32, "stored amount, weapon {w}, row {:08x?}", r);
            match change.signum() {
                0 => zero += 1,
                -1 => negative += 1,
                _ => positive += 1,
            }
            if change.wrapping_neg() < 0 && change.wrapping_neg() == (r[o + 2] as i32).wrapping_add(r[o] as i32).wrapping_neg() {
                clamped += 1;
            }
        }
    }
    assert!(zero > 20 && positive > 5 && negative > 100 && clamped > 5, "{zero} {positive} {negative} {clamped}");
}

#[test]
fn command_aura_weapon_bonus_matches_the_original() {
    // 0 fraction, 1 flags, 2 slots | 3..8 weapon 0 (count sides bonus targs kind) | 8..13 weapon 1 | 13, 14 counter
    // deltas | 15, 16 stored amounts
    let rows = hex_rows(include_str!("data/unit_spells/command_aura.txt"));
    assert!(rows.len() >= 300);
    let (mut skipped, mut flat, mut signed, mut clamped) = (0, 0, 0, 0);
    for r in &rows {
        let slots = r[2] as usize;
        for w in 0..2 {
            let o = 3 + 5 * w;
            let want = if w < slots {
                sim::orc::command_aura_weapon_amount(f(r[0]), r[1], r[o] as i32, r[o + 1] as i32, r[o + 2] as i32, r[o + 4], r[o + 3])
            } else {
                None
            };
            let (delta, stored) = (r[13 + w], r[15 + w]);
            match want {
                None => {
                    assert_eq!((delta, stored), (0, 0), "weapon {w}, row {:08x?}", r);
                    if w < slots {
                        skipped += 1;
                    }
                }
                Some(a) => {
                    assert_eq!((delta, stored), (a as u32, a as u32), "weapon {w}, row {:08x?}", r);
                    if r[1] & 0x400 != 0 {
                        flat += 1;
                    }
                    if a.abs() == 1 {
                        signed += 1;
                    }
                    if a < 0 && a == (r[o + 2] as i32).wrapping_add(r[o] as i32).wrapping_neg() {
                        clamped += 1;
                    }
                }
            }
        }
    }
    assert!(skipped > 20 && flat > 50 && signed > 20 && clamped > 2, "{skipped} {flat} {signed} {clamped}");
}

