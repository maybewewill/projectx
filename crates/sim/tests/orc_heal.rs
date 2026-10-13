//! Healing Wave and the shared chain candidate collector against the original (Unicorn vectors from
//! `tools/emu/heal_emu.py`, `tests/data/orc/heal.txt`): the real hop body, gate and collector ran on fake objects.

use sim::orc::*;
use softfloat::F32;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn healing_wave_hop_gate_and_collector_match_the_original() {
    let (mut nh, mut ng, mut nc, mut picks) = (0, 0, 0, 0);
    for line in include_str!("data/orc/heal.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w[0] {
            "HH" => {
                let n: u32 = w[4].parse().unwrap();
                assert_eq!(h(w[6]), CHAIN_RECORD_FLAGS, "{line}");
                assert_eq!(chain_hop_record_amount(true, f(w[1]), f(w[3])).to_bits(), h(w[7]), "{line}");
                assert_eq!(h(w[8]), CHAIN_HOP_DELAY_BITS, "{line}");
                assert_eq!(h(w[9]), chain_destroy_delay_bits(true), "{line}");
                assert_eq!(chain_next_factor(f(w[3]), f(w[2])).to_bits(), h(w[10]), "{line}");
                assert_eq!(h(w[11]), n + 1, "{line}");
                nh += 1;
            }
            "HG" => {
                let n: i32 = w[2].parse().unwrap();
                let (targs, flags) = (h(w[3]), h(w[4]));
                let found: u32 = w[5].parse().unwrap();
                let hops: u32 = w[7].parse().unwrap();
                let word = h(w[8]);
                if chain_gate_open(n, f(w[1])) {
                    assert_eq!(word, chain_next_flags_for(true, targs, flags), "{line}");
                    assert_eq!(hops, found, "{line}");
                } else {
                    assert_eq!((hops, word), (0, 0xDEAD_BEEF), "{line}");
                }
                ng += 1;
            }
            "CB" => {
                let heal = w[1] == "1";
                let class = ChainClass { heals: heal, score_mode: w[2] == "1", dist_mode: w[3] == "1" };
                let flags = h(w[4]);
                let cand = ChainCandidate {
                    life: f(w[5]),
                    max_life: f(w[6]),
                    dist: f(w[7]),
                    flags_5c: h(w[8]),
                    field_ec: h(w[9]) as i32,
                    visible: w[12] == "1",
                    immune: w[13] == "1",
                    already_hit: w[14] == "1",
                    illusions_restore: w[15] == "1",
                };
                let mut best = ChainBest { dist: f(w[10]), score: f(w[11]) };
                let picked = chain_candidate_update(class, flags, &cand, &mut best);
                assert_eq!(picked, w[17] == "1", "{line}");
                assert_eq!(best.dist.to_bits(), h(w[18]), "{line}");
                assert_eq!(best.score.to_bits(), h(w[19]), "{line}");
                nc += 1;
                picks += picked as u32;
            }
            _ => panic!("{line}"),
        }
    }
    assert!(nh >= 300 && ng >= 300 && nc >= 300 && picks >= 100, "{nh} {ng} {nc} {picks}");
}
