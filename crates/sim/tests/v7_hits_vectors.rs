//! V7: the attack-hook handlers of Flak Cannon (Aflk), Flaming Attack (Afak) and Death Damage (Adda) against the original
//! (`tools/emu/v7_hits_emu.py`, `tests/data/v7/hits.txt`, 300 cases each).

use sim::unit_abilities_a2::{death_damage_rings, death_damage_runs, flaming_splash_fields, tier_hit_runs, tier_rings};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn flak_flaming_and_death_handlers_match_the_original() {
    let (mut fl, mut fl_called, mut af, mut af_ran, mut ad, mut ad_ran, mut fs, mut fs_called) = (0, 0, 0, 0, 0, 0, 0, 0);
    for line in include_str!("data/v7/hits.txt").lines() {
        let (head, tail) = line.split_once('|').unwrap();
        let h: Vec<&str> = head.split_whitespace().collect();
        let t: Vec<&str> = tail.split_whitespace().collect();
        let f = |s: &str| F32::from_bits(hex(s));
        match h[0] {
            "FL" => {
                let pause = hex(h[1]);
                let enabled = h[2] == "1";
                let data = [f(h[7]), f(h[8]), f(h[9]), f(h[10]), f(h[11])];
                let area = f(h[12]);
                let runs = tier_hit_runs(enabled, pause);
                assert_eq!(t[0] == "1", runs, "{line}");
                fl += 1;
                if runs {
                    fl_called += 1;
                    let rings = tier_rings(area, data);
                    // tail: called x y r0 a0 r1 a1 r2 a2 count rec5 rec3 rec8 targs src -1 cb ctx0 ctx1 1 0
                    for (i, (r, a)) in rings.iter().enumerate() {
                        assert_eq!((hex(t[3 + 2 * i]), hex(t[4 + 2 * i])), (r.to_bits(), a.to_bits()), "ring {i}, {line}");
                    }
                    assert_eq!(t[9], "00000003", "{line}");
                    // the class / row / flag words come from the record (+12, +20, +32) and the ability's targs word
                    assert_eq!((t[10], t[11], t[12], t[13]), (h[4], h[3], h[5], h[6]), "{line}");
                    assert_eq!((t[14], t[15], t[16]), ("00405000", "ffffffff", "6f172370"), "{line}");
                    assert_eq!((t[17], t[18], t[19], t[20]), ("1", "1", "00000001", "00000000"), "{line}");
                }
            }
            "FS" => {
                // Frag Shards slot 0x140: the same three rings through a temporary object's slot 212, six pointers in the order
                // Area, DataC, DataA, DataD, DataB, DataE; the record and the targs word go first.
                let pause = hex(h[1]);
                let enabled = h[2] == "1";
                let data = [f(h[5]), f(h[6]), f(h[7]), f(h[8]), f(h[9])];
                let area = f(h[10]);
                let runs = tier_hit_runs(enabled, pause);
                assert_eq!(t[0] == "1", runs, "{line}");
                fs += 1;
                if runs {
                    fs_called += 1;
                    let r = tier_rings(area, data);
                    let want = [r[0].0, r[0].1, r[1].0, r[1].1, r[2].0, r[2].1];
                    assert_eq!(t[1], "1", "record first, {line}");
                    assert_eq!(hex(t[2]), hex(h[4]), "targs, {line}");
                    for (i, v) in want.iter().enumerate() {
                        assert_eq!(hex(t[3 + i]), v.to_bits(), "pointer {i}, {line}");
                    }
                }
            }
            "AF" => {
                let flags = hex(h[1]);
                let data = [f(h[2]), f(h[3]), f(h[4]), f(h[5]), f(h[6])];
                let area = f(h[7]);
                af += 1;
                if flags & 0x40_0000 != 0 {
                    af_ran += 1;
                    assert_eq!(hex(t[0]), 0x1234, "{line}");
                    assert_eq!(hex(t[1]), flags & !0x40_0000, "{line}");
                    for (i, v) in flaming_splash_fields(area, data).iter().enumerate() {
                        assert_eq!(hex(t[2 + i]), v.to_bits(), "field {i}, {line}");
                    }
                } else {
                    assert_eq!(hex(t[0]), 0, "{line}");
                    assert_eq!(hex(t[1]), flags, "{line}");
                    assert!(t[2..].iter().all(|w| *w == "cccccccc"), "{line}");
                }
            }
            _ => {
                let enabled = h[1] == "1";
                let flags = hex(h[2]);
                let uflags = hex(h[3]);
                let ubyte: u8 = h[4].parse().unwrap();
                let data = [f(h[5]), f(h[6]), f(h[7]), f(h[8])];
                let dur = hex(h[9]);
                let runs = death_damage_runs(enabled, flags, uflags, ubyte);
                ad += 1;
                assert_eq!(t[0] == "1", runs, "{line}");
                if runs {
                    ad_ran += 1;
                    // tail: 1 delay x y r0 a0 r1 a1 count class 0 0 targs src -1 0
                    assert_eq!(hex(t[1]), dur, "delay, {line}");
                    let rings = death_damage_rings(data);
                    for (i, (r, a)) in rings.iter().enumerate() {
                        assert_eq!((hex(t[4 + 2 * i]), hex(t[5 + 2 * i])), (r.to_bits(), a.to_bits()), "ring {i}, {line}");
                    }
                    assert_eq!((t[8], t[9]), ("00000002", "00000010"), "{line}");
                    assert_eq!((t[10], t[11], t[12].to_string(), t[13], t[14]), ("00000000", "00000000", h[10].to_string(), "1", "ffffffff"), "{line}");
                }
            }
        }
    }
    assert!(fl >= 300 && fl_called > 150 && af >= 300 && af_ran > 100 && ad >= 300 && ad_ran > 60 && fs >= 300 && fs_called > 150, "{fl} {fl_called} {af} {af_ran} {ad} {ad_ran} {fs} {fs_called}");
}
