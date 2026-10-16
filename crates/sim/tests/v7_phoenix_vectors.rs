//! V7: Summon Phoenix (`AHpx`) entered through its own slot 0x3C0 (`0x6F11BA10`), which registers a callback and jumps into
//! the summon loop `0x6F163390` (`tools/emu/v7_phoenix_emu.py`, `tests/data/v7/phoenix.txt`, 300 cases): the model is the
//! shared `orc::feral_plan` arithmetic of `human_heroes::summon`.

use sim::orc::feral_plan;
use softfloat::F32;

#[test]
fn phoenix_effect_matches_the_original() {
    let h = |s: &str| u32::from_str_radix(s, 16).unwrap();
    let f = |s: &str| F32::from_bits(h(s));
    let (mut n, mut flagged, mut timed, mut multi) = (0, 0, 0, 0);
    for line in include_str!("data/v7/phoenix.txt").lines() {
        let (head, rest) = line.split_once('|').unwrap();
        let hw: Vec<&str> = head.split_whitespace().collect();
        let parts: Vec<&str> = rest.split('|').collect();
        let plan = feral_plan(f(hw[2]), f(hw[3]), f(hw[4]));
        let calls: Vec<&str> = parts[0].split_whitespace().collect();
        let count: usize = calls[0].parse().unwrap();
        assert_eq!(count, plan.count.max(0) as usize, "{line}");
        let creators: Vec<&str> = parts[0].split(';').collect();
        if count > 0 {
            for (i, c) in creators.iter().enumerate() {
                let w: Vec<&str> = c.split_whitespace().skip(if i == 0 { 1 } else { 0 }).collect();
                assert_eq!((w[0], w[1], w[2], w[3]), (hw[6], "1", "1", "1"), "{line}");
                assert_eq!((w[4], h(w[5])), ("1", plan.flags), "{line}");
            }
        }
        assert_eq!(parts[1].trim().parse::<usize>().unwrap(), count, "link calls, {line}");
        let slot: Vec<&str> = parts[2].split_whitespace().collect();
        assert_eq!(slot[0].parse::<usize>().unwrap(), count, "{line}");
        for k in 0..count {
            assert_eq!((slot[1 + 2 * k], h(slot[2 + 2 * k])), ("1", plan.duration.to_bits()), "{line}");
        }
        n += 1;
        flagged += (plan.flags == 0x502) as u32;
        timed += (plan.duration.to_f32() != 0.0) as u32;
        multi += (count > 1) as u32;
    }
    assert!(n >= 300 && flagged > 30 && timed > 60 && multi > 60, "{n} {flagged} {timed} {multi}");
}
