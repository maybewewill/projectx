//! Creep Thunder Clap (`ACtc`, CAbilityCreepThunderClap < CAbilityThunderClap, vtable `0x6F8F7044`): the Thunder Clap per-unit body
//! `0x6F1607B0` and the slow slot `0x430` (`0x6F18AA90`) re-run on its vtable (`tools/emu/v9_creep_clap_unit.py`,
//! `tools/emu/v9_emu.py` check `creep_clap_slow`, docs/spec/abilities-v9.md).

use sim::cast::SpellLevel;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

#[test]
fn creep_thunder_clap_slow_matches_the_original() {
    let data = hex_rows(include_str!("data/v9/creep_clap_slow.txt"));
    assert!(data.len() >= 300);
    let (mut applied, mut refused) = (0, 0);
    for r in &data {
        let level = SpellLevel {
            dur: F32::from_bits(r[0]),
            hero_dur: F32::from_bits(r[1]),
            data: [F32::ZERO, F32::ZERO, F32::from_bits(r[2]), F32::from_bits(r[3]), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let got = sim::human_heroes::thunder_clap_slow(&level, r[5] != 0, F32::from_bits(r[4]));
        assert_eq!(u32::from(got.is_some()), r[6], "applied, row {:08x?}", r);
        if let Some((d, m, a)) = got {
            assert_eq!([d.to_bits(), m.to_bits(), a.to_bits()], [r[7], r[8], r[9]], "row {:08x?}", r);
            applied += 1;
        } else {
            refused += 1;
        }
    }
    assert!(applied > 200 && refused > 20);
}

#[test]
fn creep_thunder_clap_per_unit_body_matches_the_original() {
    use sim::orc::{stomp_record_flags, stomp_unit};
    let h = |s: &str| u32::from_str_radix(s, 16).unwrap();
    let (mut n, mut zero_rows, mut stuns) = (0, 0, 0);
    for line in include_str!("data/v9/creep_clap_unit.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(w[0], "U");
        let data_a = h(w[1]);
        let zero = w[2] != "0" || w[3] != "0";
        let (targ, resolved, owner): (u32, u32, u32) = (w[4].parse().unwrap(), w[5].parse().unwrap(), w[6].parse().unwrap());
        let out = stomp_unit(zero, resolved != 0, targ, owner);
        assert_eq!((w[8], w[12]), ("1", "1"), "{line}");
        assert_eq!(h(w[9]), if zero { 0 } else { data_a }, "{line}");
        assert_eq!(h(w[10]), 0x8000, "{line}");
        assert_eq!(h(w[11]), stomp_record_flags(zero), "{line}");
        assert_eq!(w[14] == "1", out.hit_slot.is_some(), "{line}");
        assert_eq!(w[15] == "1", out.stun, "{line}");
        n += 1;
        zero_rows += zero as u32;
        stuns += out.stun as u32;
    }
    assert!(n >= 400 && zero_rows > 50 && stuns > 50, "{n} {zero_rows} {stuns}");
}
