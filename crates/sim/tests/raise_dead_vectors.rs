//! Raise Dead's autocast comparator `0x6F0F5950` against the original (`tools/emu/raise_dead_cmp_emu.py`: the real function and
//! the shared comparator it falls back to, 1,200 random cases over three seeds with NaN/infinity/denormal values and
//! dead/living candidates). Numbers only; no game data.

use sim::unit_spells::raise_dead_prefers;
use softfloat::F32;

#[test]
fn raise_dead_comparator_matches_the_original_on_1200_vectors() {
    let text = include_str!("vectors/raise_dead_cmp.txt");
    let mut n = 0;
    let mut wins = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let p: Vec<&str> = line.split_whitespace().collect();
        let dc = p[0] == "1";
        let db = p[1] == "1";
        let h = |s: &str| F32::from_bits(u32::from_str_radix(s, 16).unwrap());
        let want = p[5] == "1";
        let got = raise_dead_prefers(dc, db, h(p[2]), h(p[3]), h(p[4]));
        assert_eq!(got, want, "{line}");
        n += 1;
        wins += usize::from(want);
    }
    assert_eq!(n, 1200);
    assert!(wins > 400 && wins < 900, "both outcomes covered: {wins}");
}
