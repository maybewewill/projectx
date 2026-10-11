//! The argument -> attack record mapping of `UnitDamageTarget` (`0x6F3C8850`) and the argument checks and the
//! immediate/delayed branch of `UnitDamagePoint` (`0x6F3C8770`) against the original, run in Unicorn by
//! `tools/emu/unit_damage_target_emu.py` (600 cases per native, edge values for the three range checks). The file
//! holds synthetic arguments and the record words the original produced; it contains no game tables.

use std::collections::HashMap;

use sim::damage::native_record;
use softfloat::{arith, F32};

const VECTORS: &str = include_str!("unit_damage_vectors.txt");

fn fields(line: &str) -> HashMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|t| t.split_once('='))
        .collect()
}

fn num(f: &HashMap<&str, &str>, k: &str) -> i64 {
    f[k].parse().unwrap_or_else(|_| panic!("field {k}"))
}

#[test]
fn damage_target_record_matches_the_original() {
    let mut cases = 0;
    let mut accepted = 0;
    for line in VECTORS.lines().filter(|l| l.starts_with("T ")) {
        let f = fields(line);
        let (unit, widget) = (num(&f, "unit") != 0, num(&f, "widget") != 0);
        let record = native_record(
            num(&f, "attack") != 0,
            num(&f, "ranged") != 0,
            num(&f, "atk") as u32,
            num(&f, "dmg") as u32,
            num(&f, "wpn") as u32,
        );
        let ret = num(&f, "ret");
        match (unit && widget, record) {
            (true, Some(r)) => {
                accepted += 1;
                assert_eq!(ret, 1, "{line}");
                let w: Vec<u32> = f["rec"].split(',').map(|x| x.parse().unwrap()).collect();
                // +0x04 weapon, +0x0C flags, +0x14 class, +0x18 and +0x1C zero, +0x20 row.
                assert_eq!(w[1], r.weapon, "{line}");
                assert_eq!(w[3], r.flags, "{line}");
                assert_eq!(w[5], r.class, "{line}");
                assert_eq!((w[6], w[7]), (0, 0), "{line}");
                assert_eq!(w[8], r.row, "{line}");
            }
            _ => {
                assert_eq!(ret, 0, "{line}");
                assert_eq!(f["rec"], "-", "{line}");
            }
        }
        cases += 1;
    }
    assert!(cases >= 500 && accepted >= 100, "{cases} cases, {accepted} accepted");
}

#[test]
fn damage_point_checks_and_branch_match_the_original() {
    let mut cases = 0;
    let (mut immediate, mut delayed) = (0, 0);
    for line in VECTORS.lines().filter(|l| l.starts_with("P ")) {
        let f = fields(line);
        let unit = num(&f, "unit") != 0;
        let record = native_record(
            num(&f, "attack") != 0,
            num(&f, "ranged") != 0,
            num(&f, "atk") as u32,
            num(&f, "dmg") as u32,
            num(&f, "wpn") as u32,
        );
        let ret = num(&f, "ret");
        match (unit, record) {
            (true, Some(r)) => {
                assert_eq!(ret, 1, "{line}");
                let delay = F32::from_bits(u32::from_str_radix(f["delay"], 16).unwrap());
                let is_immediate = arith::le(delay, F32::ZERO);
                let args: Vec<i32> = f["args"].split(',').map(|x| x.parse().unwrap()).collect();
                // Immediate: class, flags, attack type at 2, 3, 4. Delayed: at 3, 4, 5.
                let base = if is_immediate { 2 } else { 3 };
                assert_eq!(f["area"], if is_immediate { "immediate" } else { "delayed" }, "{line}");
                assert_eq!(args[base] as u32, r.class, "{line}");
                assert_eq!(args[base + 1] as u32, r.flags, "{line}");
                assert_eq!(args[base + 2] as u32, r.row, "{line}");
                if is_immediate {
                    immediate += 1;
                } else {
                    delayed += 1;
                }
            }
            _ => {
                assert_eq!(ret, 0, "{line}");
                assert_eq!(f["area"], "-", "{line}");
            }
        }
        cases += 1;
    }
    assert!(cases >= 500 && immediate >= 20 && delayed >= 20, "{cases} {immediate} {delayed}");
}
