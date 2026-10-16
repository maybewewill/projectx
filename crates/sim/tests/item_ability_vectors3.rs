//! The numeric cores of the S53 item abilities against `Game.dll` 1.26a (`docs/spec/abilities-items.md` section 17).
//!
//! `item_ability_vectors3.txt` is the output of `tools/emu/item_cores3_emu.py OUT 350 7`: every row is the behaviour of the
//! ORIGINAL code (Unicorn) on fake objects, with the callees that need the object model stubbed and recorded (the row
//! formats are in the module notes of the script). The Moonstone rows need the softfloat tables of the user's `Game.dll`
//! (`GAME_DLL`, like the other soft divide tests).

use sim::cast::SpellLevel;
use sim::item_abilities::*;
use softfloat::{arith, Tables, F32};

const ROWS: &str = include_str!("item_ability_vectors3.txt");

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("hex {s}"))
}

fn rows(kind: &str) -> Vec<Vec<&'static str>> {
    ROWS.lines()
        .filter(|l| l.split(' ').next() == Some(kind))
        .map(|l| l.split(' ').collect())
        .collect()
}

fn f(b: u32) -> F32 {
    F32::from_bits(b)
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn level(dur: u32, herodur: u32, data: &[(usize, u32)]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for &(i, v) in data {
        d[i] = f(v);
    }
    SpellLevel {
        dur: f(dur),
        hero_dur: f(herodur),
        data: d,
        ..Default::default()
    }
}

#[test]
fn moonstone_arguments_match_the_original() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: moonstone vectors skipped");
        return;
    };
    let rs = rows("CT");
    assert!(rs.len() >= 300);
    let mut called = 0;
    for r in &rs {
        let (a, b, dur, extra) = (h(r[1]), h(r[2]), h(r[3]), h(r[4]));
        assert_eq!(r[5], "|");
        let lv = level(dur, 0, &[(0, a), (1, b)]);
        let got = moonstone_args(&lv, f(extra), &|x, y| arith::div(x, y, &t));
        if r[6] == "1" {
            let (tod, d) = got.unwrap_or_else(|| panic!("row {r:?} must start"));
            assert_eq!((tod.to_bits(), d.to_bits()), (h(r[7]), h(r[8])), "row {r:?}");
            called += 1;
        } else {
            assert!(got.is_none(), "row {r:?}");
        }
    }
    assert!(called > 100 && called < rs.len() - 20, "the rows reach both paths");
}

#[test]
fn vampire_potion_arguments_match_the_original() {
    let rs = rows("PV");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (hero, dur, herodur, extra, a, b) = (h(r[1]) != 0, h(r[2]), h(r[3]), h(r[4]), h(r[5]), h(r[6]));
        assert_eq!(r[7], "|");
        let lv = level(dur, herodur, &[(0, a), (1, b)]);
        let (d, p2, p3) = vampire_potion_args(&lv, hero, f(extra));
        assert_eq!((d.to_bits(), p2.to_bits(), p3.to_bits()), (h(r[8]), h(r[9]), h(r[10])), "row {r:?}");
    }
}

#[test]
fn vampire_potion_heal_matches_the_original() {
    let rs = rows("VP");
    assert!(rs.len() >= 300);
    let (mut healed, mut shown) = (0, 0);
    for r in &rs {
        let b = |i: usize| h(r[i]) != 0;
        let input = VampireHitInput {
            victim_mechanical: b(1),
            relation: b(2),
            misc_immunity_gate: b(3),
            victim_slot_11c: b(4),
            victim_immune: b(5),
            misc_illusions_heal: b(6),
            owner_gate: b(7),
            victim_illusion: b(8),
        };
        let (ratio, amount) = (h(r[9]), h(r[10]));
        assert_eq!(r[11], "|");
        let (fx, heal) = vampire_potion_heal(&input, f(ratio), f(amount));
        assert_eq!(u32::from(fx), h(r[14]), "effect, row {r:?}");
        assert_eq!(heal.is_some(), r[12] == "1", "heal, row {r:?}");
        if let Some(x) = heal {
            assert_eq!(x.to_bits(), h(r[13]), "heal amount, row {r:?}");
            healed += 1;
        }
        shown += usize::from(fx);
    }
    assert!(healed > 20 && shown > healed, "the rows reach the heal and the blocked-heal paths");
}

#[test]
fn orb_of_fire_area_plan_matches_the_original() {
    let rs = rows("FB");
    assert!(rs.len() >= 300);
    let mut armed_rows = 0;
    for r in &rs {
        let (targs, area, v88, armed, recflags) = (h(r[1]), h(r[2]), h(r[3]), h(r[4]) != 0, h(r[5]));
        assert_eq!(r[6], "|");
        let plan = orb_fire_plan(armed, f(area), f(v88), targs, recflags);
        if r[7] == "1" {
            let p = plan.unwrap_or_else(|| panic!("row {r:?}"));
            // shape area, shape value, count, flags, class, unit is the carrier, -1, callback ok, context is the victim
            assert_eq!(p.area.to_bits(), h(r[8]), "row {r:?}");
            assert_eq!(p.value.to_bits(), h(r[9]), "row {r:?}");
            assert_eq!(h(r[10]), 1, "one shape, row {r:?}");
            assert_eq!(p.flags, h(r[11]), "row {r:?}");
            assert_eq!(p.class, h(r[12]), "row {r:?}");
            assert_eq!(h(r[13]), 1, "the carrier is the source, row {r:?}");
            assert_eq!(h(r[14]), 0xFFFF_FFFF, "row {r:?}");
            assert_eq!((h(r[15]), h(r[16])), (1, 1), "callback skips the victim, context is the victim, row {r:?}");
            assert_eq!(p.deliver_flag, h(r[19]), "delivery flag, row {r:?}");
            assert_eq!(h(r[20]), 1, "row {r:?}");
            armed_rows += 1;
        } else {
            assert!(plan.is_none(), "row {r:?}");
            assert_eq!(r[19], "-");
        }
    }
    assert!(armed_rows > 150, "the rows reach the area body");
}

#[test]
fn orb_of_freezing_apply_matches_the_original() {
    let rs = rows("ZB");
    assert!(rs.len() >= 300);
    let mut applied = 0;
    for r in &rs {
        let (hero, dur, herodur, ill, armed) = (h(r[1]) != 0, h(r[2]), h(r[3]), h(r[4]) != 0, h(r[5]) != 0);
        assert_eq!(r[6], "|");
        let lv = level(dur, herodur, &[]);
        let plan = orb_freeze_plan(&lv, armed, hero, ill);
        if r[7] == "1" {
            let (d, i) = plan.unwrap_or_else(|| panic!("row {r:?}"));
            assert_eq!(d.to_bits(), h(r[8]), "row {r:?}");
            assert_eq!(u32::from(i), h(r[10]), "the illusion bit, row {r:?}");
            assert_eq!(h(r[9]), 1, "row {r:?}");
            applied += 1;
        } else {
            assert!(plan.is_none(), "row {r:?}");
        }
    }
    assert!(applied > 150, "the rows reach the apply");
}

#[test]
fn orb_of_corruption_apply_matches_the_original() {
    let rs = rows("CR");
    assert!(rs.len() >= 300);
    let mut applied = 0;
    for r in &rs {
        let (hero, dur, herodur, ill, armed, a, b) =
            (h(r[1]) != 0, h(r[2]), h(r[3]), h(r[4]) != 0, h(r[5]) != 0, h(r[6]), h(r[7]));
        assert_eq!(r[8], "|");
        let lv = level(dur, herodur, &[(0, a), (1, b)]);
        let plan = orb_corruption_plan(&lv, armed, hero, ill);
        if r[9] == "1" {
            let (d, amount) = plan.unwrap_or_else(|| panic!("row {r:?}"));
            assert_eq!((d.to_bits(), amount.to_bits()), (h(r[10]), h(r[11])), "row {r:?}");
            applied += 1;
        } else {
            assert!(plan.is_none(), "row {r:?}");
        }
    }
    assert!(applied > 150, "the rows reach the apply");
}

#[test]
fn orb_of_lightning_firing_matches_the_original() {
    let rs = rows("LB");
    assert!(rs.len() >= 300);
    let mut fired = 0;
    for r in &rs {
        let b = |i: usize| h(r[i]) != 0;
        assert_eq!(r[6], "|");
        let got = orb_lightning_fires(b(1), b(2), b(3), b(4), b(5));
        assert_eq!(got, r[7] == "1", "row {r:?}");
        assert_eq!(r[7], r[8], "the victim delivery follows the effect, row {r:?}");
        fired += usize::from(got);
    }
    assert!(fired > 30, "the rows reach the effect");
}

#[test]
fn move_speed_bonus_is_the_maximum_like_the_original() {
    let rs = rows("MS");
    assert!(rs.len() >= 300);
    let mut stacked_rows = 0;
    for r in &rs {
        let n: usize = r[1].parse().unwrap();
        let vals: Vec<F32> = (0..n).map(|i| f(h(r[2 + i]))).collect();
        assert_eq!(r[2 + n], "|");
        assert_eq!(speed_flat_bonus(&vals).to_bits(), h(r[3 + n]), "row {r:?}");
        stacked_rows += usize::from(n >= 2);
    }
    assert!(stacked_rows > 100, "the rows reach several abilities");
}

#[test]
fn passive_defense_factor_matches_the_original() {
    let rs = rows("DD");
    assert!(rs.len() >= 300);
    let mut scaled = 0;
    for r in &rs {
        let (row, armed, a, e, g, hh, a1, a2) = (h(r[1]), h(r[2]) != 0, h(r[3]), h(r[4]), h(r[5]), h(r[6]), h(r[7]), h(r[8]));
        assert_eq!(r[9], "|");
        let factor = passive_defense_factor(row, armed, f(a), f(e), f(g), f(hh));
        let (n1, n2) = match factor {
            Some(v) => (arith::mul(f(a1), v), arith::mul(f(a2), v)),
            None => (f(a1), f(a2)),
        };
        assert_eq!((n1.to_bits(), n2.to_bits()), (h(r[10]), h(r[11])), "row {r:?}");
        assert_eq!(u32::from(factor.is_some()), h(r[12]), "effect call, row {r:?}");
        assert_eq!(h(r[13]), 0, "the armed flag is cleared, row {r:?}");
        scaled += usize::from(factor.is_some());
    }
    assert!(scaled > 100, "the rows reach the multiply");
}
