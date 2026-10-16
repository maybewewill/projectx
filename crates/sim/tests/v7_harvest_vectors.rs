//! V7: the init slots of Gold Mine (Agld), Harvest Lumber (Ahrl) and Harvest (Ahar) against the original
//! (`tools/emu/v7_harvest_emu.py`, `tests/data/v7/harvest.txt`, 300 cases each).

use sim::unit_abilities_a2::{gold_mine_init, harvest_init};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn harvest_and_mine_inits_match_the_original() {
    let (mut gm, mut hl, mut hv, mut slots_cap, mut rate_default, mut attackless) = (0, 0, 0, 0, 0, 0);
    for line in include_str!("data/v7/harvest.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().collect();
        let t: Vec<&str> = tail.split_whitespace().collect();
        let f = |s: &str| F32::from_bits(hex(s));
        match h[0] {
            "GM" => {
                let m = gold_mine_init(f(h[1]), f(h[2]), f(h[3]));
                assert_eq!(hex(t[0]), m.amount as u32, "amount, {line}");
                assert_eq!(hex(t[1]), m.rate.to_bits(), "rate, {line}");
                assert_eq!(hex(t[2]), m.slots as u32, "slots, {line}");
                assert_eq!(hex(t[3]), 15, "+0xAC, {line}");
                gm += 1;
                slots_cap += (m.slots == 32) as u32;
                rate_default += (m.rate == F32::ONE) as u32;
            }
            "HL" => {
                let i = harvest_init(f(h[1]), f(h[2]), f(h[3]), F32::ZERO, false);
                assert_eq!(hex(t[0]), i.area.to_bits(), "area, {line}");
                assert_eq!(hex(t[1]), i.lumber_per_hit as u32, "per hit, {line}");
                assert_eq!(hex(t[2]), i.lumber_capacity as u32, "capacity, {line}");
                let has_attack = h[4] == "1";
                let (p, q) = if has_attack { (hex(h[5]), hex(h[6])) } else { (0x3F00_0000, 0x3F00_0000) };
                assert_eq!((hex(t[3]), hex(t[4])), (p, q), "attack properties, {line}");
                assert_eq!(hex(t[5]), 0x8004_0000, "flags, {line}");
                hl += 1;
                attackless += (!has_attack) as u32;
            }
            _ => {
                let i = harvest_init(f(h[1]), f(h[2]), f(h[3]), f(h[4]), true);
                assert_eq!(hex(t[0]), i.area.to_bits(), "area, {line}");
                assert_eq!(hex(t[1]), i.lumber_per_hit as u32, "per hit, {line}");
                assert_eq!(hex(t[2]), i.lumber_capacity as u32, "capacity, {line}");
                assert_eq!(hex(t[3]), i.gold_capacity.unwrap() as u32, "gold capacity, {line}");
                assert_eq!(hex(t[4]), 0x8004_8000, "flags, {line}");
                hv += 1;
            }
        }
    }
    assert!(gm >= 300 && hl >= 300 && hv >= 300 && slots_cap > 20 && rate_default > 5 && attackless > 80, "{gm} {hl} {hv} {slots_cap} {rate_default} {attackless}");
}
