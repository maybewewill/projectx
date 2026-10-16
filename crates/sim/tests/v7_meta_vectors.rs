//! V7: Metamorphosis (AEme) morph-in hook `0x6F13D5C0` and revert hook `0x6F111D20` against the original
//! (`tools/emu/v7_meta_emu.py`, `tests/data/v7/meta.txt`, 300 cases each).

use sim::hero_extra::{meta_in_plan, meta_out_value};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn metamorphosis_hooks_match_the_original() {
    let (mut me, mut made, mut no_buff_with_bonus, mut mr, mut removed) = (0, 0, 0, 0, 0);
    for line in include_str!("data/v7/meta.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().collect();
        let t: Vec<&str> = tail.split_whitespace().collect();
        let f = |s: &str| F32::from_bits(hex(s));
        if h[0] == "ME" {
            let plan = meta_in_plan(f(h[2]), h[1] == "1", f(h[3]));
            let existing = h[4] == "1";
            assert_eq!(t[0] == "1", plan.buff_duration.is_some(), "buff made, {line}");
            if let Some(d) = plan.buff_duration {
                assert_eq!(hex(t[1]), d.to_bits(), "duration, {line}");
                assert_eq!(t[2], "1", "unit, {line}");
                made += 1;
            } else {
                no_buff_with_bonus += (t[4] == "1") as u32;
            }
            assert_eq!(t[3] == "1", existing, "an earlier buff is removed, {line}");
            assert_eq!((t[4], hex(t[5]), t[6]), ("1", plan.sink_value.to_bits(), "3"), "life sink, {line}");
            assert_eq!(hex(t[7]), plan.sink_value.to_bits(), "holder, {line}");
            me += 1;
            removed += existing as u32;
        } else {
            let stored = f(h[1]);
            assert_eq!(t[0] == "1", h[2] == "1", "removal, {line}");
            assert_eq!((t[1], hex(t[2]), t[3]), ("1", meta_out_value(stored).to_bits(), "3"), "revert sink, {line}");
            assert_eq!(hex(t[4]), 0, "holder cleared, {line}");
            mr += 1;
        }
    }
    assert!(me >= 300 && mr >= 300 && made > 90 && no_buff_with_bonus > 60 && removed > 50, "{me} {mr} {made} {no_buff_with_bonus} {removed}");
}
