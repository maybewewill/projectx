//! V9: the Water Elemental family summon loop `0x6F163390` run on five more classes that share it (`tools/emu/v9_summon_emu.py`:
//! ANsg Summon Grizzly, ANsq Quilbeast, ANsw War Eagle, ANlm Lava Monster, ANwm Watery Minion; 150 cases each). The model runs the same
//! `feral_plan` arithmetic for all of them (docs/spec/abilities-v9.md).

use sim::orc::feral_plan;
use softfloat::F32;
use std::collections::BTreeMap;

#[test]
fn summon_loop_matches_the_original_on_the_creep_summon_classes() {
    let h = |s: &str| u32::from_str_radix(s, 16).unwrap();
    let f = |s: &str| F32::from_bits(h(s));
    let mut per_class: BTreeMap<u32, u32> = BTreeMap::new();
    let (mut flagged, mut timed, mut multi) = (0, 0, 0);
    for line in include_str!("data/v9/summon_family.txt").lines() {
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
        *per_class.entry(h(hw[1])).or_default() += 1;
        flagged += (plan.flags == 0x502) as u32;
        timed += (plan.duration.to_f32() != 0.0) as u32;
        multi += (count > 1) as u32;
    }
    assert_eq!(per_class.len(), 5, "{per_class:?}");
    assert!(per_class.values().all(|&n| n >= 150), "{per_class:?}");
    assert!(flagged > 100 && timed > 200 && multi > 200, "{flagged} {timed} {multi}");
}
