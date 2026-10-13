//! Chain Lightning hop arithmetic and gate against the original (Unicorn vectors from `tools/emu/chain_emu.py`,
//! `tests/data/orc/chain.txt`). The real hop body `0x6F1C2F50` and gate `0x6F1C3200` ran on a fake ability.

use sim::orc::*;
use softfloat::F32;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn hop_damage_factor_gate_and_flags_match_the_original() {
    let (mut nh, mut ng) = (0, 0);
    for line in include_str!("data/orc/chain.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w[0] == "H" {
            let (a, c, f) = (F32::from_bits(h(w[1])), F32::from_bits(h(w[2])), F32::from_bits(h(w[3])));
            let n: u32 = w[4].parse().unwrap();
            assert_eq!(h(w[6]), CHAIN_RECORD_FLAGS, "{line}");
            assert_eq!(chain_hop_amount(a, f).to_bits(), h(w[7]), "{line}");
            assert_eq!(chain_next_factor(f, c).to_bits(), h(w[10]), "{line}");
            assert_eq!(h(w[11]), n + 1, "{line}");
            assert_eq!(h(w[12]), CHAIN_HOP_DELAY_BITS, "{line}");
            assert_eq!(h(w[13]), 1.0f32.to_bits(), "{line}");
            nh += 1;
        } else {
            let data_b = F32::from_bits(h(w[1]));
            let n: i32 = w[2].parse().unwrap();
            let (targs, flags) = (h(w[3]), h(w[4]));
            let found: u32 = w[5].parse().unwrap();
            let hops: u32 = w[7].parse().unwrap();
            let word = h(w[8]);
            let ended: u32 = w[9].parse().unwrap();
            if chain_gate_open(n, data_b) {
                assert_eq!(word, chain_next_flags(targs, flags), "{line}");
                assert_eq!((hops, ended), (found, 1 - found), "{line}");
            } else {
                assert_eq!((hops, ended, word), (0, 1, 0xDEAD_BEEF), "{line}");
            }
            ng += 1;
        }
    }
    assert!(nh >= 300 && ng >= 300, "{nh} {ng}");
}
