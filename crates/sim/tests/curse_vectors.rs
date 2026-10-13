//! Curse's attacker-side miss decision against vectors from the original (`tools/emu/curse_emu.py`: the real
//! `CBuffCurse::EvadeAttack` `0x6F07F850` with its helper `0x6F07C4C0` and the score slot `0x6F07F8A0`, 1,800 cases over
//! three seeds, the stream draw and the assert helper stubbed). Numbers only; no game data.

use sim::unit_spells::curse_evades;
use softfloat::F32;

#[test]
fn curse_evade_matches_the_original_on_1800_vectors() {
    let text = include_str!("vectors/curse_evade.txt");
    let mut n = 0;
    let mut evaded = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut it = line.split_whitespace();
        let chance = u32::from_str_radix(it.next().unwrap(), 16).unwrap();
        let draw = u32::from_str_radix(it.next().unwrap(), 16).unwrap();
        let want: u32 = it.next().unwrap().parse().unwrap();
        let got = curse_evades(F32::from_bits(chance), F32::from_bits(draw));
        assert_eq!(u32::from(got), want, "chance {chance:08x} draw {draw:08x}");
        n += 1;
        evaded += want;
    }
    assert_eq!(n, 1800);
    assert!(evaded > 300 && evaded < 1500, "vectors must cover both outcomes, {evaded} evasions");
}
