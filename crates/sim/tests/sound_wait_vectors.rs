//! `TriggerWaitForSound` wait arithmetic against vectors from the original's machine code
//! (`tools/emu/sound_wait_emu.py`: the instructions `0x6F4475F8..0x6F4476A0` run in Unicorn). Needs `GAME_DLL` for the
//! soft-float tables; without it the test returns early.

use sim::natives::lane_f::wait_from_timing;
use softfloat::{Tables, F32};

const VECTORS: &str = include_str!("sound_wait_vectors.txt");

#[test]
fn wait_matches_the_original_instructions() {
    let Ok(path) = std::env::var("GAME_DLL") else { return };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    let tables = Tables::from_game_dll(&bytes).expect("tables");
    let mut n = 0;
    let mut nonzero = 0;
    for line in VECTORS.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let dur: i32 = f[0].parse().unwrap();
        let playing = f[1] == "1";
        let pos: i32 = f[2].parse().unwrap();
        let off = F32::from_bits(u32::from_str_radix(f[3], 16).unwrap());
        let want = u32::from_str_radix(f[4], 16).unwrap();
        let got = wait_from_timing(dur, playing.then_some(pos), off, &tables).to_bits();
        assert_eq!(got, want, "dur {dur} playing {playing} pos {pos} offset {:08x}", off.to_bits());
        n += 1;
        nonzero += usize::from(want != 0);
    }
    assert_eq!(n, 400);
    assert!(nonzero > 100, "the vectors reach the non-zero branch ({nonzero})");
}
