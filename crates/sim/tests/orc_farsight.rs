//! Far Sight cast arguments against the original (Unicorn vectors from `tools/emu/farsight_emu.py`,
//! `tests/data/orc/farsight.txt`): the real slot `0x418` (`0x6F06FA50`) on a fake ability, the effect creator recorded.

use sim::orc::*;
use softfloat::F32;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn creator_arguments_match_the_original() {
    let mut n = 0;
    for line in include_str!("data/orc/farsight.txt").lines() {
        let (head, rest) = line.split_once('|').unwrap();
        let w: Vec<&str> = head.split_whitespace().collect();
        // F x y Dur plus DataA Area effid owner
        let (x, y, dur, plus, data_a, area) = (h(w[1]), h(w[2]), f(w[3]), f(w[4]), f(w[5]), f(w[6]));
        let (effid, owner): (u32, u32) = (h(w[7]), w[8].parse().unwrap());
        let parts: Vec<&str> = rest.split('|').collect();
        let a: Vec<&str> = parts[0].split_whitespace().collect();
        let v: Vec<&str> = parts[1].split_whitespace().collect();
        let plan = far_sight_plan(dur, plus, data_a, area);
        assert_eq!(h(a[0]), effid, "{line}");
        assert_eq!((h(v[1]), h(v[2])), (x, y), "the cast point: {line}");
        assert_eq!(h(a[4]), owner, "owner index: {line}");
        assert_eq!(h(v[5]), plan.duration.to_bits(), "duration (Dur + 0x9C, no HeroDur): {line}");
        assert_eq!(h(v[6]), plan.radius.to_bits(), "Area: {line}");
        assert_eq!(h(a[7]) as i32, plan.count, "F2I(DataA): {line}");
        n += 1;
    }
    assert!(n >= 300, "{n}");
}

#[test]
fn the_effect_outlives_the_duration_by_a_tenth_of_a_second() {
    let p = far_sight_plan(F32::from_bits(8.0f32.to_bits()), F32::ZERO, F32::from_bits(3.0f32.to_bits()), F32::ZERO);
    assert!((p.lifetime.to_f32() - 8.1).abs() < 1e-5);
}
