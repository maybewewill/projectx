//! Earthquake effect numbers against the original (Unicorn vectors from `tools/emu/quake_emu.py`,
//! `tests/data/orc/quake.txt`): the real timer handler `0x6F0C4420` and effect initialiser `0x6F0C4230` on a fake ability.

use sim::orc::*;
use softfloat::F32;

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

fn f(s: &str) -> F32 {
    F32::from_bits(h(s))
}

#[test]
fn effect_cells_and_tick_count_match_the_original() {
    let mut n = 0;
    for line in include_str!("data/orc/quake.txt").lines() {
        let parts: Vec<&str> = line.split('|').collect();
        let w: Vec<&str> = parts[0].split_whitespace().collect();
        // Q DataB DataC DataD Area Dur plus cx cy level
        let (data_b, data_c, area, dur, plus) = (f(w[1]), f(w[2]), f(w[4]), f(w[5]), f(w[6]));
        let (cx, cy) = (h(w[7]), h(w[8]));
        let cells: Vec<&str> = parts[1].split_whitespace().collect();
        let flds: Vec<&str> = parts[2].split_whitespace().collect();
        let plan = quake_plan(dur, plus, data_b, data_c, area);
        assert_eq!((h(cells[0]), h(cells[1])), (cx, cy), "centre: {line}");
        assert_eq!(h(cells[2]), plan.damage.to_bits(), "damage (DataB): {line}");
        assert_eq!(h(cells[3]), plan.radius.to_bits(), "radius (Area): {line}");
        assert_eq!(h(cells[5]), plan.slow.to_bits(), "slow factor (DataC): {line}");
        assert_eq!(h(flds[0]) as i32, plan.ticks, "ticks (F2I(Dur + 0x9C)): {line}");
        n += 1;
    }
    assert!(n >= 300, "{n}");
}
