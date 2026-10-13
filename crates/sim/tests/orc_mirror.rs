//! Mirror Image numeric cores against the original (Unicorn vectors from `tools/emu/mirror_emu.py`,
//! `tests/data/orc/mirror.txt`). Needs `GAME_DLL` (RNG and soft-float tables); without it the vector tests skip.

use sim::orc::{mirror_offset, mirror_pick, mirror_rotation, mirror_transform, MIRROR_STREAM};
use sim::rng::{Rng, RngTable};
use softfloat::{Tables, F32};

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn the_pick_stream_is_twelve() {
    assert_eq!(MIRROR_STREAM, 12);
}

#[test]
fn slot_pick_polygon_rotation_and_transform_match_the_original() {
    let Some(dll) = game_dll() else {
        return;
    };
    let table = RngTable::from_game_dll(&dll).expect("rng table");
    let t = Tables::from_game_dll(&dll).expect("softfloat tables");
    let (mut nr, mut np, mut nq, mut nt) = (0, 0, 0, 0);
    for line in include_str!("data/orc/mirror.txt").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w[0] {
            "R" => {
                let mut rng = Rng { a: h(w[1]), b: h(w[2]) };
                let n: i32 = w[3].parse().unwrap();
                let got = mirror_pick(&mut rng, &table, n).expect("pick") + 1;
                assert_eq!(got as u32, h(w[4]), "{line}");
                assert_eq!((rng.a, rng.b), (h(w[5]), h(w[6])), "{line}");
                nr += 1;
            }
            "P" => {
                let k: i32 = w[1].parse().unwrap();
                let n: i32 = w[2].parse().unwrap();
                let (o1, o2) = mirror_offset(k, n, f(w[3]), &t);
                assert_eq!((o1.to_bits(), o2.to_bits()), (h(w[4]), h(w[5])), "{line}");
                np += 1;
            }
            "Q" => {
                let m = mirror_rotation(f(w[1]), &t);
                for i in 0..9 {
                    assert_eq!(m[i].to_bits(), h(w[2 + i]), "{line} element {i}");
                }
                nq += 1;
            }
            "T" => {
                let mut m = [F32::ZERO; 9];
                for i in 0..9 {
                    m[i] = f(w[1 + i]);
                }
                let v = [f(w[10]), f(w[11]), f(w[12])];
                let o = mirror_transform(&m, v);
                for i in 0..3 {
                    assert_eq!(o[i].to_bits(), h(w[13 + i]), "{line} component {i}");
                }
                nt += 1;
            }
            _ => panic!("unknown row {line}"),
        }
    }
    assert!(nr >= 300 && np >= 300 && nq >= 300 && nt >= 300, "{nr} {np} {nq} {nt}");
}
