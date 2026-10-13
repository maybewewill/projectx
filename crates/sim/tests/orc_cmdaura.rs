//! The base damage of the Command Aura against the original (Unicorn vectors from `tools/emu/cmdaura_emu.py`,
//! `tests/data/orc/cmdaura.txt`): the real `0x6F0C6D00` on a fake attack ability and the real `0x6F26D5F0` on a fake unit.

use sim::orc::command_base_damage;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn average_weapon_damage_matches_the_original() {
    let (mut nb, mut nu) = (0, 0);
    for line in include_str!("data/orc/cmdaura.txt").lines() {
        let (head, res) = line.split_once('|').unwrap();
        let w: Vec<&str> = head.split_whitespace().collect();
        let want = h(res.trim());
        match w[0] {
            "B" => {
                let (n, s, b): (i32, i32, i32) = (w[1].parse().unwrap(), w[2].parse().unwrap(), w[3].parse().unwrap());
                assert_eq!(command_base_damage(n, s, b).to_bits(), want, "{line}");
                nb += 1;
            }
            _ => {
                let has: u32 = w[1].parse().unwrap();
                let (n, s, b): (i32, i32, i32) = (w[2].parse().unwrap(), w[3].parse().unwrap(), w[4].parse().unwrap());
                let got = if has == 0 { 0 } else { command_base_damage(n, s, b).to_bits() };
                assert_eq!(got, want, "{line}");
                nu += 1;
            }
        }
    }
    assert!(nb >= 200 && nu >= 200, "{nb} {nu}");
}
