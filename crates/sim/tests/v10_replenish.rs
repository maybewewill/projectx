//! The Moon Well (`Arpb`, `Arpl`, `Arpm`) against the original (`tools/emu/v10_emu.py` checks `rep_effect` and `rep_precheck`): the effect slot
//! `0x428` = `0x6F1FADC0` with its per-unit callback `0x6F1FAAA0` (group builder replaced by a loop over prepared units) and the precheck
//! slot `0x374` = `0x6F1DBCC0`.

use sim::unit_abilities_m::*;
use softfloat::F32;

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

const ABILITY: u32 = 0x0020_1000;
const CASTER: u32 = 0x0020_2000;

type Canon = (&'static str, i64, u32, u32);

fn canon_plan(ev: &[ReplenishEvent]) -> Vec<Canon> {
    ev.iter()
        .map(|e| match *e {
            ReplenishEvent::Fx(i) => ("fx", i as i64, 0, 0),
            ReplenishEvent::Lookup { unit, id } => ("lookup", unit as i64, id, 0),
            ReplenishEvent::Refresh { unit, dur, .. } => ("refresh", unit as i64, dur.to_bits(), 0),
            ReplenishEvent::Create { unit, id, dur } => ("create", unit as i64, id, dur.to_bits()),
            ReplenishEvent::Life { unit, amount } => ("life", unit as i64, amount.to_bits(), 0),
            ReplenishEvent::Mana { unit, amount } => ("mana", unit as i64, amount.to_bits(), 0),
            ReplenishEvent::CasterFx => ("cfx", 0, 0, 0),
            ReplenishEvent::Pay(c) => ("pay", 0, c.to_bits(), 0),
            ReplenishEvent::Cooldown(c) => ("cool", 0, c.to_bits(), 0),
        })
        .collect()
}

/// Turns the recorded stub calls into the same canonical events (and checks the call shapes on the way).
fn canon_log(log: &[u32], row: &[u32]) -> Vec<Canon> {
    let n = log.len() / 5;
    let e = |i: usize| &log[5 * i..5 * i + 5];
    let (mut out, mut i) = (Vec::new(), 0);
    let (mut cur_unit, mut cur_id) = (0i64, 0u32);
    while i < n {
        let x = e(i);
        match x[0] {
            6 => {
                assert_eq!(x[1], ABILITY, "fx on the ability, row {row:08x?}");
                out.push(("fx", i64::from(x[2]), 0, 0));
            }
            1 => {
                cur_unit = i64::from(x[1]);
                cur_id = x[2];
                assert_eq!((x[3], x[4]), (0, 0), "lookup arguments, row {row:08x?}");
                out.push(("lookup", cur_unit, cur_id, 0));
            }
            12 => {
                assert_eq!(x[2], ABILITY, "refresh takes the ability reference, row {row:08x?}");
                out.push(("refresh", cur_unit, x[3], 0));
            }
            2 | 3 | 4 => {
                let want = [BUFF_REPLENISH, BUFF_REPLENISH_LIFE, BUFF_REPLENISH_MANA][(x[0] - 2) as usize];
                assert_eq!(cur_id, want, "creator matches the looked-up id, row {row:08x?}");
                let (a, b, c) = (e(i + 1), e(i + 2), e(i + 3));
                assert_eq!((a[0], b[0], c[0]), (11, 13, 5), "create sequence, row {row:08x?}");
                assert_eq!(a[2], ABILITY, "ability reference, row {row:08x?}");
                assert_eq!(b[4], CASTER, "caster argument, row {row:08x?}");
                assert_eq!(i64::from(b[2]), cur_unit, "apply on the unit, row {row:08x?}");
                assert_eq!(i64::from(c[1]), cur_unit, "registered on the unit, row {row:08x?}");
                assert_eq!(a[1], b[1], "same buff object, row {row:08x?}");
                assert_eq!(c[2], b[1], "registered buff object, row {row:08x?}");
                out.push(("create", cur_unit, cur_id, b[3]));
                i += 3;
            }
            9 => out.push(("life", i64::from(x[1]), x[2], 0)),
            10 => out.push(("mana", i64::from(x[1]), x[2], 0)),
            7 => {
                assert_eq!((x[1], x[2]), (ABILITY, CASTER), "caster effect, row {row:08x?}");
                out.push(("cfx", 0, 0, 0));
            }
            8 => {
                assert_eq!((x[1], x[3]), (CASTER, 1), "payment arguments, row {row:08x?}");
                out.push(("pay", 0, x[2], 0));
            }
            14 => {
                assert_eq!(x[3], 1, "cooldown flag, row {row:08x?}");
                out.push(("cool", 0, x[2], 0));
            }
            k => panic!("unexpected log kind {k}, row {row:08x?}"),
        }
        i += 1;
    }
    out
}

#[test]
fn replenish_pulse_matches_the_original() {
    let rows = hex_rows(include_str!("data/v10/rep_effect.txt"));
    assert!(rows.len() >= 900);
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    let (mut limited, mut scaled, mut illusion_blocked, mut empty) = (0, 0, 0, 0);
    for (ri, r) in rows.iter().enumerate() {
        let base = [REPLENISH, REPLENISH_LIFE, REPLENISH_MANA][r[0] as usize];
        let f = F32::from_bits;
        let inp = ReplenishIn {
            base,
            cast: f(r[1]),
            dur: f(r[2]),
            hero_dur: f(r[3]),
            extra: f(r[4]),
            cost: r[5] as i32,
            cool: f(r[6]),
            data_a: f(r[8]),
            data_b: f(r[9]),
            data_e: f(r[10]),
            extra_cool: f(r[11]),
            misc_life: r[13] != 0,
            misc_mana: r[14] != 0,
            ethereal_bonus: f(r[15]),
        };
        let n = r[18] as usize;
        let mut units = Vec::new();
        for k in 0..n {
            let u = &r[19 + 9 * k..19 + 9 * k + 9];
            units.push(ReplenishUnit {
                hero: u[0] != 0,
                life: f(u[1]),
                max_life: f(u[2]),
                mana: f(u[3]),
                max_mana: f(u[4]),
                illusion: u[5] != 0,
                ethereal: u[6] as i32,
                existing: [u[7] != 0; 3],
                refused: u[8] != 0,
            });
        }
        let at = 19 + 9 * n;
        let (init, fin, args, cnt) = (&r[at..at + 12], &r[at + 12..at + 24], &r[at + 24..at + 35], r[at + 35] as usize);
        let log = &r[at + 36..at + 36 + 5 * cnt];
        // The group builder is handed: the callback, the record, 0xB, Area, the caster position, 0x1D, the targs, the caster, -1, 0x1F.
        assert_eq!((args[0], args[2], args[6], args[9], args[10]), (0x6F1F_AAA0, 0xB, 0x1D, 0xFFFF_FFFF, 0x1F), "builder arguments {ri}");
        assert_eq!((args[3], args[4], args[5], args[7], args[8]), (r[7], r[16], r[17], r[12], CASTER), "builder data {ri}");
        assert_eq!(init[0], ABILITY, "record owner {ri}");
        let (life0, mana0) = (if base == REPLENISH_MANA { 0 } else { r[8] }, if base == REPLENISH_LIFE { 0 } else { r[9] });
        assert_eq!((init[1], init[2], init[3], init[4], init[5]), (life0, mana0, 0, 0, CASTER), "record amounts {ri}");
        assert_eq!(init[6], softfloat::arith::add(inp.dur, inp.extra).to_bits(), "record duration, row {ri}");
        assert_eq!(init[7], r[3], "record hero duration {ri}");
        assert_eq!((init[9], init[10], init[11]), (0, 0, softfloat::conv::f2i(inp.cast) as u32), "record counters {ri}");
        let plan = replenish_plan(&inp, &units);
        let got = canon_log(log, r);
        let want = canon_plan(&plan);
        assert_eq!(want, got, "row {ri}");
        // the counter of the record equals the number of units that were taken
        let taken = plan.iter().filter(|e| matches!(e, ReplenishEvent::Fx(_))).count() as u32;
        assert_eq!(fin[10], taken, "final counter {ri}");
        for e in &plan {
            let k = match e {
                ReplenishEvent::Fx(_) => "fx",
                ReplenishEvent::Lookup { .. } => "lookup",
                ReplenishEvent::Refresh { .. } => "refresh",
                ReplenishEvent::Create { .. } => "create",
                ReplenishEvent::Life { .. } => "life",
                ReplenishEvent::Mana { .. } => "mana",
                _ => "tail",
            };
            *seen.entry(k).or_default() += 1;
        }
        if softfloat::conv::f2i(inp.cast) != 0 && taken as i32 == softfloat::conv::f2i(inp.cast) {
            limited += 1;
        }
        if units.iter().any(|u| u.ethereal > 0) && plan.iter().any(|e| matches!(e, ReplenishEvent::Life { .. })) {
            scaled += 1;
        }
        if units.iter().any(|u| u.illusion) && !inp.misc_life {
            illusion_blocked += 1;
        }
        if taken == 0 {
            empty += 1;
        }
    }
    for k in ["fx", "lookup", "refresh", "create", "life", "mana"] {
        assert!(seen.get(k).copied().unwrap_or(0) > 100, "coverage of {k}: {seen:?}");
    }
    assert!(limited > 5 && scaled > 20 && illusion_blocked > 20 && empty > 5, "{limited} {scaled} {illusion_blocked} {empty}");
}

#[test]
fn replenish_precheck_matches_the_original() {
    let rows = hex_rows(include_str!("data/v10/rep_precheck.txt"));
    assert!(rows.len() >= 300);
    for (ri, r) in rows.iter().enumerate() {
        let v = replenish_precheck(F32::from_bits(r[1]), r[2] as i32);
        assert_eq!(v.to_bits(), r[3], "precheck value, row {ri}");
    }
}
