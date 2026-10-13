//! Wind Walk first strike and arming against the original (Unicorn vectors from `tools/emu/ww_emu.py`,
//! `tests/data/orc/windwalk.txt`): the real handler `0x6F1637B0` (with the real bonus writer `0x6F255720`) and the real
//! arming slot `0x6F171870` ran on a fake buff.

use sim::orc::*;
use softfloat::F32;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn first_strike_and_arming_match_the_original() {
    let (mut nw, mut nx, mut applied) = (0, 0, 0);
    for line in include_str!("data/orc/windwalk.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w[0] == "W" {
            // W flags A B V recflags | A' B' rec+0x1C recflags' flags' event slot34C
            let flags = h(w[1]);
            let got = ww_first_strike(flags, f(w[2]), f(w[3]), f(w[4]), h(w[5]));
            let armed = flags & 0x200 != 0;
            assert_eq!(got.is_some(), armed, "{line}");
            assert_eq!(w[12] == "1", armed, "{line}");
            assert_eq!(w[13] == "1", armed, "{line}");
            assert_eq!(h(w[11]), flags, "{line}");
            match got {
                Some((a, b, rf)) => {
                    assert_eq!(a.to_bits(), h(w[7]), "{line}");
                    assert_eq!(b.to_bits(), h(w[8]), "{line}");
                    assert_eq!(rf, h(w[10]), "{line}");
                    applied += 1;
                }
                None => {
                    assert_eq!((h(w[7]), h(w[8]), h(w[10])), (h(w[2]), h(w[3]), h(w[5])), "{line}");
                }
            }
            nw += 1;
        } else {
            // X has_target flags answer | flags' removed event
            let (flags, removed, event) = ww_arm(w[1] == "1", h(w[2]), w[3].parse().unwrap());
            assert_eq!(flags, h(w[5]), "{line}");
            assert_eq!(removed, w[6] == "1", "{line}");
            assert_eq!(event, w[7] == "1", "{line}");
            nx += 1;
        }
    }
    assert!(nw >= 300 && nx >= 300 && applied >= 100, "{nw} {nx} {applied}");
}

#[test]
fn a_positive_bonus_blocks_the_second_writer() {
    let one = F32::from_bits(0x3F80_0000);
    let first = record_bonus(F32::ZERO, F32::ZERO, one).unwrap();
    assert!(record_bonus(first.0, first.1, one).is_none());
}
