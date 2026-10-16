//! V7: the regeneration auras (Aabr, Aoar, Aarm) against the original (`tools/emu/v7_aura_emu.py`,
//! `tests/data/v7/aura_regen.txt`): the buff apply routines on a fake buff and owner, and the ability apply that hands the
//! buff its "absolute" flag.

use sim::auras::{aura_gate_suspends, aura_stat_amount};
use sim::unit_abilities_a::{regen_aura_absolute_flag, regen_aura_amount};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn regen_aura_buff_and_ability_apply_match_the_original() {
    let (mut ap, mut ab, mut sunk, mut percent_rows, mut flag_true) = (0, 0, 0, 0, 0);
    for line in include_str!("data/v7/aura_regen.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().collect();
        let t: Vec<&str> = tail.split_whitespace().collect();
        if h[0] == "AP" {
            let flags = hex(h[2]);
            let (strength, stat) = (F32::from_bits(hex(h[3])), F32::from_bits(hex(h[4])));
            let (illusion, misc) = (h[5] == "1", h[6] == "1");
            let enters = flags & 0x200 == 0;
            let sinks = enters && !aura_gate_suspends(illusion, misc);
            let percent = flags & 0x100 == 0;
            assert_eq!(t[0].parse::<u32>().unwrap(), u32::from(sinks), "calls, row {line}");
            if sinks {
                assert_eq!(hex(t[1]), aura_stat_amount(percent, strength, stat).to_bits(), "amount, row {line}");
                sunk += 1;
                percent_rows += percent as u32;
            }
            assert_eq!(hex(t[2]), flags | 0x200, "flags after, row {line}");
            ap += 1;
        } else {
            let data_b = F32::from_bits(hex(h[2]));
            assert_eq!(t[1], "1", "the slot 0x36C call count, row {line}");
            let want = t[0] == "1";
            assert_eq!(regen_aura_absolute_flag(data_b), want, "flag, row {line}");
            // the composed rate: a clear flag means a fraction of the maximum
            let amount = regen_aura_amount(F32::from_bits(0x4000_0000), data_b, F32::from_bits(0x42C8_0000));
            let expected = if want { 0x4000_0000 } else { aura_stat_amount(true, F32::from_bits(0x4000_0000), F32::from_bits(0x42C8_0000)).to_bits() };
            assert_eq!(amount.to_bits(), expected, "composed, row {line}");
            flag_true += want as u32;
            ab += 1;
        }
    }
    assert!(ap >= 600 && ab >= 300 && sunk > 250 && percent_rows > 100 && flag_true > 30, "{ap} {ab} {sunk} {percent_rows} {flag_true}");
}
