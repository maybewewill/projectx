//! V5: the stat effects of the melee auras against the original (`tools/emu/v5_aura_emu.py`, `tests/data/v5/aura.txt`):
//! the real apply routines of Devotion, Endurance (attack speed, movement), Unholy (movement, life regeneration) and
//! Brilliance ran on a fake buff and owner, 600 cases.

use sim::auras::{aura_gate_suspends, aura_stat_amount};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn aura_stat_applies_match_the_original() {
    // kind -> (applied bit, gated here, percent bit)
    let spec = |k: &str| match k {
        "DV" => (0x200u32, true, 0x100u32),
        "EA" => (0x100, false, 0),
        "EM" => (0x200, false, 0),
        "UM" => (0x200, true, 0),
        "UL" => (0x400, true, 0x100),
        "BM" => (0x100, true, 0x200),
        other => panic!("kind {other}"),
    };
    let (mut n, mut sunk, mut percent_rows, mut gated_rows) = (0, 0, 0, 0);
    for line in include_str!("data/v5/aura.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().collect();
        let t: Vec<&str> = tail.split_whitespace().collect();
        let (applied_bit, gated, pct_bit) = spec(h[0]);
        let flags = hex(h[1]);
        let (strength, stat) = (F32::from_bits(hex(h[2])), F32::from_bits(hex(h[3])));
        let (illusion, misc) = (h[4] == "1", h[5] == "1");
        let already = flags & applied_bit != 0;
        let enters = !already;
        let suspended = gated && aura_gate_suspends(illusion, misc);
        let sinks = enters && !suspended;
        let percent = pct_bit != 0 && flags & pct_bit != 0;
        let calls: u32 = t[0].parse().unwrap();
        assert_eq!(calls, u32::from(sinks), "calls, row {line}");
        if sinks {
            assert_eq!(hex(t[1]), aura_stat_amount(percent, strength, stat).to_bits(), "amount, row {line}");
            sunk += 1;
            percent_rows += percent as u32;
        }
        assert_eq!(hex(t[2]), flags | applied_bit, "flags after, row {line}");
        gated_rows += (gated && suspended) as u32;
        n += 1;
    }
    assert!(n >= 600 && sunk > 250 && percent_rows > 40 && gated_rows > 30, "{n} {sunk} {percent_rows} {gated_rows}");
}
