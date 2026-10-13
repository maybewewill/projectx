//! The numeric cores of the remaining item abilities against `Game.dll` 1.26a (`docs/spec/abilities-items.md`, S41).
//!
//! `item_ability_vectors2.txt` is the output of `tools/emu/item_cores2_emu.py 350 7`: every row is the behaviour of the
//! ORIGINAL code (Unicorn) on fake objects, with the callees that need the object model stubbed and recorded (the row
//! formats are in the module notes of the script). Each test recomputes the rows with `sim::item_abilities`.

use sim::cast::SpellLevel;
use sim::item_abilities::*;
use softfloat::F32;

const ROWS: &str = include_str!("item_ability_vectors2.txt");

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

#[test]
fn group_cooldown_start_matches_the_original() {
    let rs = rows("GR");
    assert!(rs.len() >= 300);
    let mut started = 0;
    for r in &rs {
        let (cool, ignore) = (h(r[1]), h(r[2]) != 0);
        assert_eq!(r[3], "|");
        let want = if r[4] == "1" { Some(h(r[5])) } else { None };
        let got = group_cooldown_start(f(cool), ignore).map(|c| c.to_bits());
        assert_eq!(got, want, "row {r:?}");
        started += usize::from(want.is_some());
    }
    assert!(started > 20, "the rows reach the start path");
}

#[test]
fn potion_buff_duration_matches_the_original() {
    let rs = rows("VI");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (hero, dur, herodur, extra) = (h(r[2]) != 0, h(r[3]), h(r[4]), h(r[5]));
        assert_eq!(r[6], "|");
        let level = SpellLevel {
            dur: f(dur),
            hero_dur: f(herodur),
            ..Default::default()
        };
        let got = potion_buff_duration(&level, hero, f(extra)).to_bits();
        assert_eq!(got, h(r[7]), "row {r:?}");
    }
}

#[test]
fn life_regen_item_gate_matches_the_original() {
    let rs = rows("RL");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (v88, illusion, misc) = (h(r[1]), h(r[2]) != 0, h(r[3]) != 0);
        assert_eq!(r[4], "|");
        let called = regen_item_gate(illusion, misc);
        assert_eq!(called, r[5] == "1", "row {r:?}");
        if called {
            assert_eq!(v88, h(r[6]), "the modifier rate is the ability value, row {r:?}");
        }
    }
}

#[test]
fn mana_regen_item_rate_matches_the_original() {
    let rs = rows("RM");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (type_regen, x, v88) = (h(r[1]), h(r[2]), h(r[3]));
        assert_eq!(r[4], "|");
        let got = mana_regen_item_rate(f(type_regen), f(x), f(v88)).to_bits();
        assert_eq!(got, h(r[5]), "row {r:?}");
    }
}

#[test]
fn hero_int_regen_term_matches_the_original() {
    let rs = rows("RH");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (misc, int) = (h(r[1]), h(r[2]) as i32);
        assert_eq!(r[3], "|");
        let got = hero_int_regen_term(f(misc), int).to_bits();
        assert_eq!(got, h(r[4]), "row {r:?}");
    }
}

#[test]
fn spell_reduction_handler_matches_the_original() {
    let rs = rows("SR");
    assert!(rs.len() >= 300);
    let mut hit = 0;
    for r in &rs {
        let (flags, row, illusion, data_b, null) = (h(r[1]), h(r[2]), h(r[3]) != 0, h(r[4]), h(r[5]) != 0);
        assert_eq!(r[6], "|");
        let got = spell_reduction_factor(!null, illusion, flags, row, f(data_b));
        let want = (r[7] == "1").then(|| h(r[8]));
        assert_eq!(got.map(|x| x.to_bits()), want, "row {r:?}");
        hit += usize::from(want.is_some());
    }
    assert!(hit > 30, "the rows reach the scale call");
}

#[test]
fn record_scale_matches_the_original() {
    let rs = rows("RS");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (a, b, factor) = (h(r[1]), h(r[2]), h(r[3]));
        assert_eq!(r[4], "|");
        let (na, nb) = scale_record(f(a), f(b), f(factor));
        assert_eq!((na.to_bits(), nb.to_bits()), (h(r[5]), h(r[6])), "row {r:?}");
    }
}

#[test]
fn life_steal_handler_matches_the_original() {
    let rs = rows("VA");
    assert!(rs.len() >= 300);
    let (mut healed, mut shown) = (0, 0);
    for r in &rs {
        let b = |i: usize| h(r[i]) != 0;
        let input = LifeStealInput {
            armed: b(1),
            filter_refused: b(2),
            immune: b(3),
            victim: b(4),
            owner_illusion: b(5),
            victim_illusion: b(6),
            misc: b(7),
        };
        let (amount, v88) = (h(r[8]), h(r[9]));
        assert_eq!(r[10], "|");
        let (fx, heal) = life_steal_outcome(&input, f(amount), f(v88));
        assert_eq!(u32::from(fx), h(r[13]), "effect, row {r:?}");
        assert_eq!(heal.is_some(), r[11] == "1", "heal, row {r:?}");
        if let Some(x) = heal {
            assert_eq!(x.to_bits(), h(r[12]), "heal amount, row {r:?}");
            healed += 1;
        }
        shown += usize::from(fx);
    }
    assert!(healed > 20 && shown > healed, "the rows reach the heal and the blocked-heal paths");
}

#[test]
fn orb_of_frost_duration_matches_the_original() {
    let rs = rows("FR");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (hero, dur, herodur, ill) = (h(r[1]) != 0, h(r[2]), h(r[3]), h(r[4]));
        assert_eq!(r[5], "|");
        let level = SpellLevel {
            dur: f(dur),
            hero_dur: f(herodur),
            ..Default::default()
        };
        assert_eq!(orb_frost_duration(&level, hero).to_bits(), h(r[6]), "row {r:?}");
        assert_eq!(ill, h(r[7]), "the illusion bit passed to the buff apply, row {r:?}");
    }
}

#[test]
fn figurine_summon_batches_match_the_original() {
    let rs = rows("FG");
    assert!(rs.len() >= 300);
    let mut batches = 0;
    for r in &rs {
        let (dur, extra, area, a, b, c, d) = (h(r[1]), h(r[2]), h(r[3]), h(r[4]), h(r[5]), h(r[6]), h(r[7]));
        assert_eq!(r[8], "|");
        let mut data = [F32::ZERO; 9];
        data[0] = f(a);
        data[1] = f(b);
        let level = SpellLevel {
            dur: f(dur),
            area: f(area),
            data,
            ..Default::default()
        };
        let got = figurine_batches(&level, [c, d], f(extra));
        let n: usize = r[9].parse().unwrap();
        assert_eq!(got.len(), n, "batch count, row {r:?}");
        for (i, (code, count, dur, area)) in got.iter().enumerate() {
            let at = 10 + 4 * i;
            assert_eq!(*code, h(r[at]), "row {r:?}");
            assert_eq!(*count as u32, h(r[at + 1]), "row {r:?}");
            assert_eq!(dur.to_bits(), h(r[at + 2]), "row {r:?}");
            assert_eq!(area.to_bits(), h(r[at + 3]), "row {r:?}");
        }
        batches += n;
    }
    assert!(batches > 100, "the rows reach the summon loop");
}
