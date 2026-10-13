//! Serpent Ward point cast against the original (Unicorn vectors from `tools/emu/ward_emu.py`,
//! `tests/data/orc/ward.txt`): the real slot `0x418` (`0x6F162DC0`) on a fake ability. Needs `GAME_DLL` for the facing
//! (soft-float multiply is table-free, but the check of `building_angle_radians` only needs the arithmetic).

use sim::orc::*;
use softfloat::F32;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn count_flag_duration_position_and_facing_match_the_original() {
    let mut n = 0;
    for line in include_str!("data/orc/ward.txt").lines() {
        let (head, rest) = line.split_once('|').unwrap();
        let w: Vec<&str> = head.split_whitespace().collect();
        // W x y dur plus data_a unit_id angle owner
        let (x, y, dur, plus, data_a) = (f(w[1]), f(w[2]), f(w[3]), f(w[4]), f(w[5]));
        let unit_id = h(w[6]);
        let angle = f(w[7]);
        let owner: u32 = w[8].parse().unwrap();
        let parts: Vec<&str> = rest.split('|').collect();
        // parts[0] = " <ncalls> <call> ; <call> ... " (each call: owner type flags), parts[1] = pointer words,
        // parts[2] = values; the call lists are joined by " ; " so re-split on ';'.
        let calls: Vec<&str> = rest.split(';').collect();
        let first_tokens: Vec<&str> = calls[0].split_whitespace().collect();
        let ncalls: usize = first_tokens[0].parse().unwrap();
        let plan = ward_plan(dur, plus, data_a);
        assert_eq!(plan.count.max(0) as usize, ncalls, "{line}");
        if ncalls > 0 {
            // first call: <owner> <type> <flags> | a1..a6 | v1 v2 v3
            assert_eq!(first_tokens[1].parse::<u32>().unwrap(), owner, "{line}");
            assert_eq!(h(first_tokens[2]), unit_id, "{line}");
            assert_eq!(h(first_tokens[3]), plan.flags, "{line}");
            // values: after the second '|' of the call: x y facing
            let vals: Vec<&str> = parts[2].split_whitespace().collect();
            assert_eq!(h(vals[0]), x.to_bits(), "{line}");
            assert_eq!(h(vals[1]), y.to_bits(), "{line}");
            assert_eq!(h(vals[2]), building_angle_radians(angle).to_bits(), "{line}");
        }
        n += 1;
    }
    assert!(n >= 300, "{n}");
}

#[test]
fn building_angle_rounds_to_multiples_of_ninety() {
    let rad = |d: f32| building_angle_radians(F32::from_bits(d.to_bits())).to_bits();
    assert_eq!(rad(270.0), building_angle_radians(F32::from_bits(270.0f32.to_bits())).to_bits());
    assert_eq!(rad(100.0), rad(90.0));
    assert_eq!(rad(-100.0), rad(270.0));
    assert_eq!(rad(-50.0), rad(0.0));
    assert_eq!(rad(450.0), rad(90.0));
}
