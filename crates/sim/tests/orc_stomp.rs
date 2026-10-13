//! War Stomp per-unit body against the original (Unicorn vectors from `tools/emu/stomp_emu.py`,
//! `tests/data/orc/stomp.txt`): the real callback `0x6F1607B0` ran on a fake ability and a fake victim.

use sim::orc::*;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn per_unit_body_matches_the_original() {
    let mut n = 0;
    for line in include_str!("data/orc/stomp.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(w[0], "U");
        let data_a = h(w[1]);
        let inv: u32 = w[2].parse().unwrap();
        let immune: u32 = w[3].parse().unwrap();
        let targ: u32 = w[4].parse().unwrap();
        let resolved: u32 = w[5].parse().unwrap();
        let owner: u32 = w[6].parse().unwrap();
        // | built amount class flags called120 idx stored stun
        let built: u32 = w[8].parse().unwrap();
        let amount = h(w[9]);
        let class = h(w[10]);
        let flags = h(w[11]);
        let called: u32 = w[12].parse().unwrap();
        let idx = w[13].parse::<u32>().unwrap();
        let stored: u32 = w[14].parse().unwrap();
        let stun: u32 = w[15].parse().unwrap();
        let zero = inv != 0 || immune != 0;
        let out = stomp_unit(zero, resolved != 0, targ, owner);
        assert_eq!((built, called, class), (1, 1, 0x8000), "{line}");
        assert_eq!(amount, if zero { 0 } else { data_a }, "{line}");
        assert_eq!(flags, stomp_record_flags(zero), "{line}");
        assert_eq!(stored, u32::from(out.hit_slot.is_some()), "{line}");
        if let Some(i) = out.hit_slot {
            assert_eq!(idx, i, "{line}");
        }
        assert_eq!(stun, u32::from(out.stun), "{line}");
        n += 1;
    }
    assert!(n >= 300, "{n}");
}

#[test]
fn word_clears_self_player_ally_and_sets_enemy_neutral() {
    // ground,organic
    assert_eq!(stomp_word(0x0400_0002), 0x0401_8002);
    // a friend word loses its bits
    assert_eq!(stomp_word(0x6000 | 0x1000 | 0x2), 0x0001_8002);
    assert_eq!(STOMP_TARGS_DEFAULT, 0x0400_0002);
}
