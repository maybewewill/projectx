//! V7: the Slow aura buff update (Aasl / Basl) against the original (`tools/emu/v7_slow_emu.py`, `tests/data/v7/slow.txt`):
//! the real slot 0x370 `0x6F1AD2F0` with its undo / apply routines on a fake buff, 600 cases.

use sim::unit_abilities_a::slow_aura_update;
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn slow_aura_update_matches_the_original() {
    let (mut n, mut skipped, mut both, mut one) = (0, 0, 0, 0);
    for line in include_str!("data/v7/slow.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().collect();
        let t: Vec<&str> = tail.split_whitespace().collect();
        let f = |i: usize| F32::from_bits(hex(h[i]));
        let u = slow_aura_update(hex(h[0]), (f(1), f(2)), (f(3), f(4)), true, true);
        assert_eq!(t[0], u.refreshed.to_string().replace("true", "1").replace("false", "0"), "0x324 calls, row {line}");
        assert_eq!(t[0], t[1], "slot calls agree, row {line}");
        assert_eq!(hex(t[2]), u.movement.to_bits(), "movement, row {line}");
        assert_eq!(hex(t[3]), u.attack.to_bits(), "attack, row {line}");
        assert_eq!(hex(t[4]), u.flags, "flags, row {line}");
        let want: Vec<String> = u.sinks.iter().map(|(c, v)| format!("{c}:{:08x}", v.to_bits())).collect();
        assert_eq!(t[5..].join(" "), want.join(" "), "sinks, row {line}");
        n += 1;
        skipped += (!u.refreshed) as u32;
        both += (u.sinks.iter().any(|s| s.0 == 'm') && u.sinks.iter().any(|s| s.0 == 'a')) as u32;
        one += (u.sinks.len() == 1) as u32;
    }
    assert!(n >= 600 && skipped > 50 && both > 30 && one > 30, "{n} {skipped} {both} {one}");
}
