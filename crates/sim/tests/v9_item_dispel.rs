//! Item Dispel Aoe (`AIdi`, CAbilityItemDispelAoe < CAbilityDispelMagic, vtable `0x6F8C4F34`): the V4 Dispel Magic callback check re-run on its
//! vtable (`tools/emu/v9_emu.py` check `itemdispel_args`, docs/spec/abilities-v9.md). The model delegates to the Dispel Magic effect.

use sim::unit_spells::*;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

#[test]
fn item_dispel_worker_arguments_match_the_original() {
    // DataA DataB | ecx edx target order ability pos neg a6 a7 damage mana flags
    let rows = hex_rows(include_str!("data/v9/itemdispel_args.txt"));
    assert!(rows.len() >= 300);
    for r in &rows {
        let mut d = [F32::ZERO; 9];
        d[0] = F32::from_bits(r[0]);
        d[1] = F32::from_bits(r[1]);
        let a = dispel_magic_args(&sim::cast::SpellLevel { data: d, ..sim::cast::SpellLevel::default() });
        let w = &r[2..];
        assert_eq!((w[5], w[6], w[7], w[8]), (a.positive, a.negative, a.a6, a.a7), "row {:08x?}", r);
        assert_eq!((w[9], w[10], w[11]), (a.damage.to_bits(), a.mana.to_bits(), a.flags), "row {:08x?}", r);
        assert_eq!(w[3], 0xD012F, "order id of the Item Dispel (852271)");
    }
}
