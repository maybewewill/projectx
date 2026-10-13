//! The numeric cores of the item abilities against `Game.dll` 1.26a (`docs/spec/abilities-items.md`).
//!
//! `item_ability_vectors.txt` is the output of `tools/emu/item_cores_emu.py 350 7`: every row is the behaviour of the
//! ORIGINAL code (Unicorn) on fake objects, with the callees that need the object model stubbed and recorded (see the
//! module notes of the script for the row formats). Each test recomputes the rows with the model of
//! `sim::item_abilities`. The level-mod rows need the softfloat tables of the user's `Game.dll` (`GAME_DLL`), like the
//! other table-dependent vectors.

use sim::item_abilities::*;
use sim::natives::game::item_flags;
use softfloat::{arith, conv, Tables, F32};

const ROWS: &str = include_str!("item_ability_vectors.txt");

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("hex {s}"))
}

fn rows(kind: &str) -> Vec<Vec<&'static str>> {
    ROWS.lines()
        .filter(|l| l.split(' ').next() == Some(kind))
        .map(|l| l.split(' ').collect())
        .collect()
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn f(b: u32) -> F32 {
    F32::from_bits(b)
}

#[test]
fn charge_routine_matches_the_original() {
    let rs = rows("CH");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (flags, charges, key) = (h(r[1]), h(r[2]) as i32, h(r[3]));
        assert_eq!(r[4], "|");
        let new_charges = h(r[5]) as i32;
        let calls = r[6];
        let (want_charges, want_calls) = match charge_outcome(flags, charges, key == 1) {
            None => (charges, "-".to_string()),
            Some((new, notify, destroy)) => {
                let mut c = String::new();
                if notify {
                    c.push('N');
                }
                if flags & item_flags::NO_CHARGES == 0 {
                    if destroy {
                        c.push_str("SD");
                    }
                    c.push('U');
                }
                (new, if c.is_empty() { "-".to_string() } else { c })
            }
        };
        assert_eq!((new_charges, calls), (want_charges, want_calls.as_str()), "row {r:?}");
    }
}

#[test]
fn level_mod_effect_matches_the_original() {
    let rs = rows("LM");
    assert!(rs.len() >= 300);
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: level-mod vectors skipped");
        return;
    };
    for r in &rs {
        let (level, xp, n, m) = (h(r[1]) as i32, h(r[2]) as i32, h(r[3]) as i32, h(r[4]) as usize);
        let table: Vec<i32> = r[5..5 + m].iter().map(|s| h(s) as i32).collect();
        assert_eq!(r[5 + m], "|");
        let want = h(r[6 + m]);
        let need = |l: i32| table[(l.clamp(1, m as i32) - 1) as usize];
        let got = level_mod_value(level, xp, n, &need, &t);
        assert_eq!(got.to_bits(), want, "row {r:?}");
    }
}

#[test]
fn restoration_effects_match_the_original() {
    let rs = rows("FX");
    assert!(rs.len() >= 300);
    for r in &rs {
        let code = h(r[1]);
        let (a, b) = (f(h(r[2])), f(h(r[3])));
        assert_eq!(r[6], "|");
        let fam = [Family::Heal, Family::Mana, Family::Restore][code as usize];
        let mut d = [F32::ZERO; 9];
        d[0] = a;
        d[1] = b;
        let plan = restore_plan(fam, &d);
        let k = h(r[7]) as usize;
        assert_eq!(plan.len(), k, "row {r:?}");
        for (i, (sink, amount)) in plan.iter().enumerate() {
            let want_sink = h(r[8 + 2 * i]);
            let want_amount = h(r[9 + 2 * i]);
            assert_eq!(*sink, if want_sink == 1 { Sink::Life } else { Sink::Mana }, "row {r:?}");
            assert_eq!(amount.to_bits(), want_amount, "row {r:?}");
        }
    }
}

#[test]
fn area_callback_matches_the_original() {
    let rs = rows("CB");
    assert!(rs.len() >= 300);
    for r in &rs {
        let (flags, misc, immune, a) = (h(r[1]), h(r[2]), h(r[3]), f(h(r[4])));
        assert_eq!(r[6], "|");
        let (called, amount, ret) = (h(r[7]), h(r[8]), h(r[9]));
        assert_eq!(ret, 1, "the callback always returns 1: {r:?}");
        let got = area_callback_amount(flags & 0x4000_0000 != 0, misc != 0, immune != 0, a);
        assert_eq!(got.is_some(), called == 1, "row {r:?}");
        if let Some(v) = got {
            assert_eq!(v.to_bits(), amount, "row {r:?}");
        }
    }
}

#[test]
fn attribute_tome_and_bonus_match_the_original() {
    let at = rows("AT");
    let ab = rows("AB");
    assert!(at.len() >= 300 && ab.len() >= 300);
    for r in &at {
        let mut d = [F32::ZERO; 9];
        d[0] = f(h(r[1]));
        d[1] = f(h(r[2]));
        d[2] = f(h(r[3]));
        assert_eq!(r[4], "|");
        let (agi, int, str) = attribute_values(&d);
        // The init stores the three integers, the effect hands them to the agility, intelligence and strength adders.
        assert_eq!((h(r[5]), h(r[6]), h(r[7])), (agi as u32, int as u32, str as u32), "row {r:?}");
        assert_eq!(&r[8..], ["agi", &format!("{:x}", agi as u32), "0", "int", &format!("{:x}", int as u32), "0", "str", &format!("{:x}", str as u32), "0"], "row {r:?}");
    }
    for r in &ab {
        let mut d = [F32::ZERO; 9];
        d[0] = f(h(r[1]));
        d[1] = f(h(r[2]));
        d[2] = f(h(r[3]));
        assert_eq!(r[4], "|");
        let (agi, int, str) = attribute_values(&d);
        assert_eq!(&r[5..], ["agi", &format!("{:x}", agi as u32), "0", "int", &format!("{:x}", int as u32), "0", "str", &format!("{:x}", str as u32), "0"], "row {r:?}");
    }
}

#[test]
fn persistent_bonuses_match_the_original() {
    let rs = rows("PB");
    assert!(rs.len() >= 300);
    for r in &rs {
        let kind = r[1];
        let v = f(h(r[2]));
        assert_eq!(r[3], "|");
        // `PB kind value | applied | removed`, each `x y` or `-`.
        let bar = r.iter().skip(4).position(|s| *s == "|").expect("second bar") + 4;
        let applied = &r[4..bar];
        let removed = &r[bar + 1..];
        match kind {
            "dmg" => {
                let amount = damage_bonus_value(v);
                assert_eq!(applied, [format!("{:x}", amount as u32), "3".to_string()], "row {r:?}");
                assert_eq!(removed, [format!("{:x}", amount.wrapping_neg() as u32), "3".to_string()], "row {r:?}");
            }
            "arm" | "asp" => {
                assert_eq!(applied, [format!("{:x}", v.to_bits()), "0".to_string()], "row {r:?}");
                assert_eq!(removed, [format!("{:x}", arith::neg(v).to_bits()), "0".to_string()], "row {r:?}");
            }
            other => panic!("kind {other}"),
        }
    }
    for r in rows("PL") {
        let v = f(h(r[2]));
        assert_eq!(r[3], "|");
        let bar = r.iter().skip(4).position(|s| *s == "|").expect("second bar") + 4;
        // The life and mana bonuses call their sink with the value and mode 1; the removal negates the value.
        assert_eq!(&r[4..bar], [format!("{:x}", v.to_bits()), "1".to_string()], "row {r:?}");
        assert_eq!(&r[bar + 1..], [format!("{:x}", arith::neg(v).to_bits()), "1".to_string()], "row {r:?}");
    }
}

#[test]
fn item_speed_effect_matches_the_original() {
    let rs = rows("SP");
    assert!(rs.len() >= 300);
    for r in &rs {
        let hero = h(r[1]) != 0;
        let level = sim::cast::SpellLevel {
            dur: f(h(r[2])),
            hero_dur: f(h(r[3])),
            data: {
                let mut d = [F32::ZERO; 9];
                d[0] = f(h(r[5]));
                d[1] = f(h(r[6]));
                d
            },
            ..Default::default()
        };
        assert_eq!(r[7], "|");
        let (dur, a, b) = speed_buff_args(&level, hero, f(h(r[4])));
        assert_eq!((dur.to_bits(), a.to_bits(), b.to_bits()), (h(r[8]), h(r[9]), h(r[10])), "row {r:?}");
    }
}

#[test]
fn tomes_match_the_original() {
    let rs = rows("TM");
    assert!(rs.len() >= 300);
    for r in &rs {
        let kind = r[1];
        let n = h(r[2]);
        assert_eq!(r[3], "|");
        let real = conv::i2r(n as i32).to_bits();
        match kind {
            "life" | "mana" => assert_eq!(&r[4..], [format!("{:x}", real), "2".to_string()], "row {r:?}"),
            "xp" => assert_eq!(
                &r[4..],
                [format!("{:x}", real), "1".to_string(), "0".into(), "0".into(), "0".into(), "1".into()],
                "row {r:?}"
            ),
            "attack" => assert_eq!(&r[4..], [format!("{:x}", n), "ffffffff".to_string()], "row {r:?}"),
            other => panic!("kind {other}"),
        }
    }
}
