//! V7: the ring collector `0x6F2C2E80` of the area damage body (`tools/emu/v7_ring_emu.py`, `tests/data/v7/ring.txt`, 600 cases):
//! ring lookup (smallest containing ring / max mode), the amount-zero skip, the immunity gates and the relation bucket.

use sim::unit_abilities_a2::{ring_collect, RingProbe};
use softfloat::F32;

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn ring_collector_matches_the_original() {
    let (mut n, mut stored, mut bucket) = (0, 0, [0u32; 3]);
    let (mut max_rows, mut gated_rows) = (0, 0);
    for line in include_str!("data/v7/ring.txt").lines() {
        let parts: Vec<&str> = line.split('|').collect();
        let h: Vec<&str> = parts[0].split_whitespace().collect();
        let m: Vec<&str> = parts[1].split_whitespace().collect();
        let t: Vec<&str> = parts[2].split_whitespace().collect();
        let count: usize = h[0].parse().unwrap();
        let radii: Vec<F32> = (0..4).map(|i| F32::from_bits(hex(h[1 + i]))).collect();
        let amounts: Vec<F32> = (0..4).map(|i| F32::from_bits(hex(h[5 + i]))).collect();
        let class = hex(h[9]);
        let row = hex(h[10]);
        let maxflag = h[11] == "1";
        let maxtest = h[12] == "1";
        let owner_filter = hex(h[13]) as i32;
        let dist = f32::from_bits(hex(m[0]));
        let misc = m[1] == "1";
        let immune = hex(m[3]) as i32;
        let ethereal = hex(m[4]) as i32;
        let invulnerable = m[5] == "1";
        let unit_null = m[6] == "1";
        let owner_idx = m[7].parse::<u32>().unwrap() & 31;
        let ally = hex(m[8]);
        let enemy = hex(m[9]);
        let rings: Vec<(F32, F32)> = (0..count.min(4)).map(|i| (radii[i], amounts[i])).collect();
        let mut max_radius = 0.0f32;
        for (r, _) in &rings {
            if max_radius < r.to_f32() {
                max_radius = r.to_f32();
            }
        }
        let probe = RingProbe {
            class,
            row,
            misc_resist: misc,
            max_mode: maxflag && maxtest,
            invulnerable,
            magic_immune: immune,
            ethereal,
            unit_known: !unit_null,
            owner_filter,
            rel_a: (ally >> owner_idx) & 1 == 1,
            rel_b: (enemy >> owner_idx) & 1 == 1,
        };
        let got = ring_collect(&rings, F32::from_bits(max_radius.to_bits()), &probe, |r| r.to_f32() >= dist);
        let entries: u32 = t[1].parse().unwrap();
        assert_eq!(got.is_some(), entries == 1, "stored, row {line}");
        if let Some(e) = got {
            assert_eq!(e.amount.to_bits(), hex(t[8]), "amount, row {line}");
            assert_eq!(e.bucket, hex(t[9]), "bucket, row {line}");
            assert_eq!(t[2], "1", "total, row {line}");
            stored += 1;
            bucket[e.bucket as usize] += 1;
        } else {
            assert_eq!(t[2], "0", "total, row {line}");
        }
        n += 1;
        max_rows += probe.max_mode as u32;
        gated_rows += (got.is_none() && rings.iter().any(|r| r.1.to_f32() != 0.0)) as u32;
    }
    assert!(n >= 600 && stored > 100 && bucket.iter().all(|b| *b > 5) && max_rows > 50 && gated_rows > 30, "{n} {stored} {bucket:?} {max_rows} {gated_rows}");
}
