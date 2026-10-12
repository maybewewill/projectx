//! The AI build-spot search against the original (`tools/emu/ai_site_emu.py`, Unicorn).
//!
//! `vectors/ai_site_seed1.tsv` holds synthetic maps (hash models of the engine predicates, see the harness header)
//! and the answers of the real `0x6F2E1630` (buffer), `0x6F2DF4A0` (marking), `0x6F2DF6F0` (search) and the
//! wrapper's retry. The replay needs no DLL and holds no game data.

use sim::ai_site::{self, Footprint, MarkUnit, Params, SiteHost, Work};
use sim::natives::game::Playable;
use softfloat::F32;

fn h(s: u32, a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ s.wrapping_mul(0xC2B2_AE3D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    x
}

struct Model {
    s: [u32; 4],
    m: [u32; 4],
}

impl SiteHost for Model {
    fn cell_level(&self, x: i32, y: i32) -> i32 {
        i32::from(h(self.s[0], x as u32, y as u32).is_multiple_of(self.m[0]))
    }
    fn unwalkable(&self, x: F32, y: F32) -> bool {
        h(self.s[1], x.0, y.0).is_multiple_of(self.m[1])
    }
    fn reachable(&self, _from: (F32, F32), to: (F32, F32)) -> bool {
        !h(self.s[2], to.0.0, to.1.0).is_multiple_of(self.m[2])
    }
    fn placement_ok(&self, x: F32, y: F32) -> bool {
        h(self.s[3], x.0, y.0).is_multiple_of(self.m[3])
    }
}

fn fnv(data: impl Iterator<Item = u8>) -> u64 {
    let mut x = 0xCBF2_9CE4_8422_2325u64;
    for b in data {
        x ^= b as u64;
        x = x.wrapping_mul(0x100_0000_01B3);
    }
    x
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn search_matches_the_original_on_the_vectors() {
    let (c1, f1, r1, t1) = replay(include_str!("vectors/ai_site_seed1.tsv"));
    let (c2, f2, r2, t2) = replay(include_str!("vectors/ai_site_hard1.tsv"));
    let (c3, f3, r3, t3) = replay(include_str!("vectors/ai_site_dense1.tsv"));
    eprintln!(
        "{} cases, {} found, {} retried, {} from a tier bucket",
        c1 + c2 + c3,
        f1 + f2 + f3,
        r1 + r2 + r3,
        t1 + t2 + t3
    );
    assert!(c1 + c2 + c3 >= 300);
    assert!(r2 >= 10, "the hard set must reach the retry");
    assert!(c2 - f2 >= 10, "the hard set must reach the failure exit");
    assert!(t3 >= 10, "the dense set must reach the tier buckets");
}

/// Replays one vector file. Returns (cases, found, retried, results from a tier bucket).
fn replay(text: &str) -> (usize, usize, usize, usize) {
    let mut lines = text.lines().peekable();
    let mut cases = 0usize;
    let mut found = 0usize;
    let mut retried = 0usize;
    let mut tier_picks = 0usize;
    while let Some(line) = lines.next() {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f[0], "C", "case header");
        let n: usize = f[1].parse().unwrap();
        let playable = Playable {
            min_x: F32::from_bits(hex(f[7])),
            min_y: F32::from_bits(hex(f[8])),
            max_x: F32::from_bits(hex(f[9])),
            max_y: F32::from_bits(hex(f[10])),
        };
        let params = Params {
            type_id: hex(f[4]),
            player: f[6].parse().unwrap(),
            count_ge_two: f[5] == "1",
            center: (F32::from_bits(hex(f[2])), F32::from_bits(hex(f[3]))),
            playable,
            footprint: Footprint { w: f[12].parse().unwrap(), h: f[13].parse().unwrap() },
            rot: ai_site::rotation_class(F32::from_bits(hex(f[11]))),
        };
        let u32s: Vec<u32> = f[14..22].iter().map(|x| x.parse().unwrap()).collect();
        let model = Model { s: [u32s[0], u32s[2], u32s[4], u32s[6]], m: [u32s[1], u32s[3], u32s[5], u32s[7]] };
        let nunits: usize = f[22].parse().unwrap();
        let mut units = Vec::new();
        for _ in 0..nunits {
            let l = lines.next().unwrap();
            let u: Vec<&str> = l.split('\t').collect();
            assert_eq!(u[0], "U");
            units.push(MarkUnit {
                buff_type: u[1].parse().unwrap(),
                buff_radius: F32::from_bits(hex(u[2])),
                x: F32::from_bits(hex(u[3])),
                y: F32::from_bits(hex(u[4])),
                footprint: Footprint { w: u[5].parse().unwrap(), h: u[6].parse().unwrap() },
            });
        }
        let v: Vec<&str> = lines.next().unwrap().split('\t').collect();
        assert_eq!(v[0], "V");
        let r: Vec<&str> = lines.next().unwrap().split('\t').collect();
        assert_eq!(r[0], "R");

        let mut work = Work::new(&model, &params);
        for u in &units {
            work.mark_unit(u, params.rot);
        }
        let vh = fnv(work.value.iter().flat_map(|r| r.iter().copied()));
        let sh = fnv(work.state.iter().flat_map(|r| r.iter().copied()));
        if format!("{vh:016x}") != v[1] {
            if let Ok(dir) = std::env::var("AI_SITE_DUMP") {
                let rows: Vec<String> = work
                    .value
                    .iter()
                    .map(|r| r.iter().map(|b| format!("{b:02x}")).collect::<String>())
                    .collect();
                std::fs::write(format!("{dir}/case{n}_value_rust.txt"), rows.join("\n")).unwrap();
            }
        }
        assert_eq!(format!("{vh:016x}"), v[1], "case {n}: value grid after marking");
        assert_eq!(format!("{sh:016x}"), v[2], "case {n}: state grid after marking");

        let before = work.clone();
        let got = ai_site::run_search(work, &model, &params);
        let status = hex(r[1]);
        match got {
            Some((x, y)) => {
                let ix = softfloat::conv::f2i(softfloat::arith::mul(ai_site::INV32, softfloat::arith::sub(x, before.origin.0)));
                let iy = softfloat::conv::f2i(softfloat::arith::mul(ai_site::INV32, softfloat::arith::sub(y, before.origin.1)));
                if (0..101).contains(&ix) && (0..101).contains(&iy) && before.value[iy as usize][ix as usize] < 4 {
                    tier_picks += 1;
                }
                assert_eq!(status, 0x3F80_0000, "case {n}: status");
                assert_eq!((x.0, y.0), (hex(r[2]), hex(r[3])), "case {n}: position");
                found += 1;
            }
            None => assert_eq!(status, 0xBF80_0000, "case {n}: status"),
        }
        if r[4] == "1" {
            retried += 1;
        }
        cases += 1;
    }
    (cases, found, retried, tier_picks)
}
