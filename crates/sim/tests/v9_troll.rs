//! Spirit Troll (`AIsh`, CAbilitySpiritTroll < CAbilitySpiritWolf, vtable `0x6F92828C`): the Feral Spirit cast effect from the Troll class's own
//! vtable slot `0x3C0` (`tools/emu/v9_troll_emu.py`, 400 cases). The model runs `orc::feral_plan` for the borrowed class.

use sim::orc::feral_plan;
use softfloat::F32;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn spirit_troll_effect_matches_the_original() {
    let mut n = 0;
    for line in include_str!("data/v9/troll_effect.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(w[0], "E");
        let (dur, plus, data_b) = (f(w[1]), f(w[2]), f(w[3]));
        let unit_id = h(w[5]);
        let calls: usize = w[8].parse().unwrap();
        let out = feral_plan(dur, plus, data_b);
        assert_eq!(out.count as usize, calls, "{line}");
        if calls > 0 {
            let rec: Vec<&str> = line.split('|').nth(1).unwrap().split(';').next().unwrap().split_whitespace().skip(1).collect();
            assert_eq!(h(rec[0]), unit_id, "{line}");
            assert_eq!((rec[1], rec[2], rec[3], rec[4]), ("1", "1", "1", "1"), "{line}");
            assert_eq!(h(rec[5]), out.flags, "{line}");
        }
        let n3e4: usize = line.split('|').nth(2).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(n3e4, calls, "{line}");
        n += 1;
    }
    assert!(n >= 300, "{n}");
}
