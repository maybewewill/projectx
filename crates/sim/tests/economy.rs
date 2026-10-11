//! Economy arithmetic checked against the original Game.dll (1.26a), replayed from the vectors that
//! `tools/emu/economy_vectors.py` recorded in Unicorn. The vectors are embedded, so this test needs no
//! `GAME_DLL`. Spec: docs/spec/economy-3.md.

#[path = "economy/vectors.rs"]
mod vectors;

use sim::economy::{self, refund_units, UpkeepTable};
use softfloat::F32;

#[test]
fn constants_match_the_original() {
    assert_eq!(economy::PERCENT_CONST_BITS, vectors::PERCENT_CONST_BITS);
    let u = UpkeepTable::default();
    let bits: Vec<u32> = u.gold_tax.iter().map(|t| t.0).collect();
    assert_eq!(bits, vectors::GOLD_TAX_DEFAULT_BITS.to_vec());
    assert!(u.lumber_tax.iter().all(|t| t.0 == 0));
}

#[test]
fn refund_matches_the_original_truncated_product() {
    let mut bad = Vec::new();
    for &(cost, rate, expected) in vectors::REFUND_VECTORS {
        let got = refund_units(cost, F32(rate));
        if got != expected {
            bad.push((cost, rate, expected, got));
        }
    }
    assert!(bad.is_empty(), "mismatches (cost, rate, original, ours): {bad:?}");
    assert!(vectors::REFUND_VECTORS.len() > 2000);
}

#[test]
fn upkeep_rate_matches_the_original_chain() {
    let mut bad = Vec::new();
    for &(tax, expected) in vectors::UPKEEP_RATE_VECTORS {
        let got = UpkeepTable::tax_percent(F32(tax));
        if got != expected {
            bad.push((tax, expected, got));
        }
    }
    assert!(bad.is_empty(), "mismatches (tax bits, original, ours): {bad:?}");
    assert!(vectors::UPKEEP_RATE_VECTORS.len() > 100);
}
