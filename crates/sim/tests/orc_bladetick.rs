//! One Bladestorm tick against the original (Unicorn vectors from `tools/emu/bladetick_emu.py`,
//! `tests/data/orc/bladetick.txt`): the real `0x6F1912A0` on a fake ability.

use sim::orc::blade_tick_step;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn counter_end_and_damage_call_match_the_original() {
    let (mut n, mut calls, mut ends) = (0, 0, 0);
    for line in include_str!("data/orc/bladetick.txt").lines() {
        let parts: Vec<&str> = line.split('|').collect();
        let w: Vec<&str> = parts[0].split_whitespace().collect();
        let (left, pause): (i32, i32) = (w[1].parse().unwrap(), w[2].parse().unwrap());
        let (radius, damage, word) = (h(w[3]), h(w[4]), h(w[5]));
        let r: Vec<&str> = parts[1].split_whitespace().collect();
        let (after, end_calls, body_calls): (i32, u32, u32) =
            (r[0].parse().unwrap(), r[1].parse().unwrap(), r[2].parse().unwrap());
        let (a, e, d) = blade_tick_step(left, pause);
        assert_eq!((a, u32::from(e), u32::from(d)), (after, end_calls, body_calls), "{line}");
        if body_calls == 1 {
            // | the 12 stack words | ring radius, amount | ...
            let s: Vec<&str> = parts[2].split_whitespace().collect();
            assert_eq!(h(s[1]), 1, "one ring: {line}");
            assert_eq!(h(s[2]), 0x20, "damage class: {line}");
            assert_eq!(h(s[5]), word, "the ability word, unchanged: {line}");
            assert_eq!(h(s[7]), 0xFFFF_FFFF, "owner -1: {line}");
            let ring: Vec<&str> = parts[3].split_whitespace().collect();
            assert_eq!((h(ring[0]), h(ring[1])), (radius, damage), "ring (radius, amount): {line}");
            calls += 1;
        }
        ends += end_calls;
        n += 1;
    }
    assert!(n >= 300 && calls > 100 && ends > 5, "{n} {calls} {ends}");
}
