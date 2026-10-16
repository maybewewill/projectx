//! V5: Frost Armor (`AUfa`) against the original: the cast effect `0x6F0BA520` with the buff creator (`tools/emu/
//! v5_frostarmor_emu.py`, `tests/data/v5/frost_armor_cast.txt`, 400 cases) and the retaliation handler `0x6F0BA3F0`
//! (`tools/emu/v5_frostarmor_h_emu.py`, `tests/data/v5/frost_armor_hit.txt`, 400 cases).

use sim::cast::SpellLevel;
use sim::undead_ne::{frost_armor_cast, frost_armor_hit_runs};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn frost_armor_cast_values_match_the_original() {
    let (mut n, mut heroes) = (0, 0);
    for line in include_str!("data/v5/frost_armor_cast.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().skip(1).collect();
        let (da, db, dur, hdur, extra) = (hex(h[0]), hex(h[1]), hex(h[2]), hex(h[3]), hex(h[4]));
        let hero = h[5] == "1";
        let level = SpellLevel {
            dur: F32::from_bits(dur),
            hero_dur: F32::from_bits(hdur),
            data: [F32::from_bits(da), F32::from_bits(db), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            ..SpellLevel::default()
        };
        let got = frost_armor_cast(&level, hero, F32::from_bits(extra));
        let parts: Vec<&str> = tail.split('|').collect();
        let calls: Vec<&str> = parts[0].split_whitespace().collect();
        assert_eq!(calls[0], "1", "one apply, {line}");
        // deref words: DataA (the buff's own duration), the chill duration, DataB (the armor)
        let d: Vec<u32> = parts[1].split_whitespace().map(hex).collect();
        assert_eq!(
            [got.duration.to_bits(), got.chill_duration.to_bits(), got.armor.to_bits()],
            [d[0], d[1], d[2]],
            "row {line}"
        );
        n += 1;
        heroes += hero as u32;
    }
    assert!(n >= 400 && heroes > 100 && n - heroes > 100, "{n} {heroes}");
}

#[test]
fn frost_armor_retaliation_matches_the_original() {
    let (mut n, mut ran, mut fresh, mut refreshed, mut illusions) = (0, 0, 0, 0, 0);
    for line in include_str!("data/v5/frost_armor_hit.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().skip(1).collect();
        let flags = hex(h[0]);
        let class = hex(h[1]);
        let (invuln, immune, found, illusion) = (h[2] == "1", h[3] == "1", h[4] == "1", h[5] == "1");
        let dur = hex(h[6]);
        // effect, new, refresh, new duration, new flag, refresh duration, refresh flag
        let t: Vec<u32> = tail.split_whitespace().map(|x| hex(x)).collect();
        let runs = frost_armor_hit_runs(flags & 1 != 0, class, invuln, immune);
        assert_eq!(t[0], u32::from(runs), "effect, row {line}");
        assert_eq!(t[1], u32::from(runs && !found), "new chill, row {line}");
        assert_eq!(t[2], u32::from(runs && found), "refreshed chill, row {line}");
        if runs && !found {
            assert_eq!((t[3], t[4]), (dur, u32::from(illusion)), "new args, row {line}");
            fresh += 1;
        }
        if runs && found {
            assert_eq!((t[5], t[6]), (dur, u32::from(illusion)), "refresh args, row {line}");
            refreshed += 1;
        }
        n += 1;
        ran += runs as u32;
        illusions += (runs && illusion) as u32;
    }
    assert!(n >= 400 && ran > 60 && fresh > 20 && refreshed > 10 && illusions > 10, "{n} {ran} {fresh} {refreshed} {illusions}");
}
