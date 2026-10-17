//! Lightning Purge (`AIlp`, CAbilityLightningPurge < CAbilityPurge, vtable `0x6F8CB464`): the V4 Purge effect check re-run through the LP class's
//! own unit-effect slot `0x424` (`0x6F10C070`, `jmp 0x6F107E10`) with the LP vtable on the fake ability (`tools/emu/v9_emu.py` check
//! `lpurge_args`, docs/spec/abilities-v9.md). The model runs the Purge effect for the item ability.

use sim::unit_spells::*;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

fn level_of(data: &[u32], dur: u32, hdur: u32) -> sim::cast::SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = F32::from_bits(*v);
    }
    sim::cast::SpellLevel { dur: F32::from_bits(dur), hero_dur: F32::from_bits(hdur), data: d, ..sim::cast::SpellLevel::default() }
}

#[test]
fn lightning_purge_effect_matches_the_original() {
    // 0..6 DataA..F | 6 dur 7 herodur 8 dead 9 allied 10 hero 11 existing | 12.. worker (ecx edx target order ability pos neg
    // a6 a7 damage mana flags) | 24..31 apply (target dur owner A B D E) | 31 call count | 32 removal calls
    let rows = hex_rows(include_str!("data/v9/lpurge_args.txt"));
    assert!(rows.len() >= 300);
    let (mut applied, mut gated, mut removed) = (0, 0, 0);
    for r in &rows {
        let l = level_of(&r[0..6], r[6], r[7]);
        let a = purge_args(&l);
        let w = &r[12..24];
        assert_eq!((w[5], w[6], w[7], w[8]), (a.positive, a.negative, a.a6, a.a7), "row {:08x?}", r);
        assert_eq!((w[9], w[10], w[11]), (a.damage.to_bits(), a.mana.to_bits(), a.flags), "row {:08x?}", r);
        assert_eq!(w[3], 0xD0133, "order id of Lightning Purge (852275)");
        if purge_slows(r[8] != 0, r[9] != 0) {
            applied += 1;
            let dur = sim::cast::effect_duration(&l, r[10] != 0, F32::ZERO).to_bits();
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
