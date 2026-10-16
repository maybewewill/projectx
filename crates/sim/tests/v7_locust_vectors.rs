//! V7: Locust Swarm (AUls) against the original (`tools/emu/v7_locust_emu.py`, `tests/data/v7/locust.txt`, 300 cases): the real effect
//! `0x6F134EA0` and spawn step `0x6F12CE70`.

use sim::cast::SpellLevel;
use sim::hero_extra::{locust_continues, locust_plan};
use softfloat::{arith, F32};

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn locust_swarm_matches_the_original() {
    let (mut n_rows, mut multi, mut found_rows, mut long_swarms) = (0, 0, 0, 0);
    for line in include_str!("data/v7/locust.txt").lines() {
        let parts: Vec<&str> = line.split('|').collect();
        let h: Vec<&str> = parts[0].split_whitespace().collect();
        let f = |s: &str| F32::from_bits(hex(s));
        let level = SpellLevel {
            dur: f(h[6]),
            area: f(h[8]),
            data: {
                let mut d = [F32::ZERO; 9];
                for (i, slot) in d.iter_mut().take(5).enumerate() {
                    *slot = f(h[1 + i]);
                }
                d
            },
            ..Default::default()
        };
        let plus = f(h[7]);
        let found = h[10] == "1";
        let plan = locust_plan(&level, plus);
        let a: Vec<&str> = parts[1].split_whitespace().collect();
        assert_eq!(hex(a[0]) as i32, plan.count, "count, {line}");
        assert_eq!(a[1], "1", "the effect makes the first locust, {line}");
        // timers after the effect: an interval timer (message 0xD01C1) when more locusts follow, then the end timer (0xD01C4)
        let t1: Vec<&str> = parts[2].split_whitespace().collect();
        let mut expect_t1 = Vec::new();
        if locust_continues(1, plan.count) {
            expect_t1.push(format!("{:08x}:d01c1", plan.interval.to_bits()));
        }
        expect_t1.push(format!("{:08x}:d01c4", plan.total.to_bits()));
        assert_eq!(t1.join(" "), expect_t1.join(" "), "timers, {line}");
        // the creator: ECX = owner index (5), EDX = UnitID, constants 0.0 / 2 / 0 / -1
        let c: Vec<&str> = parts[3].split_whitespace().collect();
        assert_eq!((c[0], hex(c[1])), ("00000005", hex(h[9])), "creator registers, {line}");
        assert_eq!((c[4], c[5], c[6], c[7]), ("6faae470", "00000002", "00000000", "ffffffff"), "creator constants, {line}");
        // after the second step: one more interval timer while the counter (2) is below the count
        let t2 = parts[4].trim();
        let want_t2 = if locust_continues(2, plan.count) { format!("{:08x}:d01c1", plan.interval.to_bits()) } else { "-".to_string() };
        assert_eq!(t2, want_t2, "second step timer, {line}");
        // the locust ability set-up
        let s = parts[5].trim();
        if found {
            let (area, datac, datad, datae) = plan.aloc;
            assert_eq!(s, format!("00000001 {:08x} {:08x} {:08x} {:08x}", area.to_bits(), datac as u32, datad.to_bits(), datae.to_bits()), "aloc, {line}");
            found_rows += 1;
        } else {
            assert_eq!(s, "-", "aloc, {line}");
        }
        // further steps: every step with counter < count re-arms; steps 3..: count - 3 of them
        let resched: i32 = parts[6].trim().parse().unwrap();
        let want = (3..60 + 3).filter(|&c| locust_continues(c, plan.count)).count() as i32;
        assert_eq!(resched, want, "reschedules, {line}");
        let _ = arith::add(F32::ZERO, F32::ZERO);
        n_rows += 1;
        multi += (plan.count > 2) as u32;
        long_swarms += (plan.count > 20) as u32;
    }
    assert!(n_rows >= 300 && multi > 150 && found_rows > 150 && long_swarms > 20, "{n_rows} {multi} {found_rows} {long_swarms}");
}
