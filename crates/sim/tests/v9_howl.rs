//! Howl of Terror (`ANht`, CAbilityHowlOfTerror < CAbilityRoar) against the original (`tools/emu/v9_emu.py` check `howl_unit`): the per-unit slot
//! `0x3EC` -> creator -> the Inner Fire buff apply, the V8 Roar check on the Howl classes.

use sim::cast::SpellLevel;
use sim::unit_spells::*;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

#[test]
fn howl_unit_matches_the_original() {
    // 0..4: A B C D | 4 duration argument | 5..9 weapon 0 (count sides bonus targs) | 9..13 weapon 1 | 13 arg1, 14 duration |
    // 15, 16 counter deltas | 17, 18 stored amounts | 19, 20 armor value+calls | 21, 22 life regen | 23, 24 mana regen
    let rows = hex_rows(include_str!("data/v9/howl_unit.txt"));
    assert!(rows.len() >= 300);
    let (mut skipped, mut applied, mut negative) = (0, 0, 0);
    for r in &rows {
        let mut d = [F32::ZERO; 9];
        for i in 0..4 {
            d[i] = F32::from_bits(r[i]);
        }
        let l = SpellLevel { data: d, ..SpellLevel::default() };
        let (fraction, armor, life, mana) = howl_unit_values(&l);
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
            match inner_fire_weapon_bonus(fraction, r[o] as i32, r[o + 1] as i32, r[o + 2] as i32, r[o + 3]) {
                None => {
                    assert_eq!((r[15 + w], r[17 + w]), (0, 0), "skipped weapon {w}, row {:08x?}", r);
                    skipped += 1;
                }
                Some(a) => {
                    assert_eq!((r[15 + w], r[17 + w]), (a as u32, a as u32), "weapon {w}, row {:08x?}", r);
                    applied += 1;
                    negative += usize::from(a < 0);
                }
            }
        }
    }
    assert!(skipped > 20 && applied > 300 && negative > 100, "{skipped} {applied} {negative}");
}
