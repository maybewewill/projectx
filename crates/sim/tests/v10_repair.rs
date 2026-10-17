//! Repair (`Arep`, `Aren`, `Arst`) against the original (`tools/emu/v10_repair_emu.py` checks `rep_start`, `rep_gate`): the unit-target effect
//! `0x6F070AB0` for a finished building and the target gate slot `0x3F0` = `0x6F04BF70`.

use sim::unit_abilities_m::*;
use softfloat::{F32, Tables};

fn hex_rows(text: &str) -> Vec<Vec<u32>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|w| u32::from_str_radix(w, 16).expect("hex")).collect())
        .collect()
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

type Canon = (&'static str, u32, u32);

fn canon_plan(ev: &[RepairEvent]) -> Vec<Canon> {
    ev.iter()
        .map(|e| match *e {
            RepairEvent::CanPay { gold } => ("can", u32::from(gold), 0),
            RepairEvent::Pay { gold } => ("pay", u32::from(gold), 0),
            RepairEvent::Timer { msg, period } => ("timer", msg, period.to_bits()),
            RepairEvent::Modifier { rate } => ("mod", rate.to_bits(), 0),
        })
        .collect()
}

fn canon_log(log: &[u32]) -> Vec<Canon> {
    let mut out = Vec::new();
    for e in log.chunks(5) {
        match e[0] {
            3 => out.push(("can", 1, 0)),
            4 => out.push(("can", 0, 0)),
            5 => {
                assert_eq!(e[2], 0xFFFF_FFFF, "one unit of gold");
                out.push(("pay", 1, 0));
            }
            6 => {
                assert_eq!(e[2], 0xFFFF_FFFF, "one unit of lumber");
                out.push(("pay", 0, 0));
            }
            2 => out.push(("timer", e[3], e[2])),
            16 => {
                assert_eq!(e[3], 0, "the second modifier argument is 0.0");
                out.push(("mod", e[2], 0));
            }
            _ => {}
        }
    }
    out
}

#[test]
fn repair_start_matches_the_original() {
    let Some(t) = tables() else { return };
    let rows = hex_rows(include_str!("data/v10/rep_start.txt"));
    assert!(rows.len() >= 400);
    let (mut refused, mut ticks, mut both, mut none) = (0, 0, 0, 0);
    for (ri, r) in rows.iter().enumerate() {
        let inp = RepairStartIn {
            data_a: F32::from_bits(r[0]),
            data_b: F32::from_bits(r[1]),
            gold_rep: r[2] as i32,
            lumber_rep: r[3] as i32,
            rep_time: r[4] as i32,
            max_life: F32::from_bits(r[5]),
            gold_ok: r[6] != 0,
            lumber_ok: r[7] != 0,
        };
        let cnt = r[8] as usize;
        let log = &r[9..9 + 5 * cnt];
        let plan = repair_start_plan(&inp, &|a, b| softfloat::arith::div(a, b, &t));
        assert_eq!(canon_plan(&plan), canon_log(log), "row {ri}: {r:08x?}");
        if !plan.iter().any(|e| matches!(e, RepairEvent::Modifier { .. })) {
            refused += 1;
        }
        ticks += plan.iter().filter(|e| matches!(e, RepairEvent::Timer { msg: 0xD01C1, .. })).count();
        let pays = plan.iter().filter(|e| matches!(e, RepairEvent::Pay { .. })).count();
        both += usize::from(pays == 2);
        none += usize::from(pays == 0 && plan.iter().any(|e| matches!(e, RepairEvent::Modifier { .. })));
    }
    assert!(refused > 20 && ticks > 20 && both > 20 && none > 20, "{refused} {ticks} {both} {none}");
}

#[test]
fn repair_gate_matches_the_original() {
    let rows = hex_rows(include_str!("data/v10/rep_gate.txt"));
    assert!(rows.len() >= 400);
    for (ri, r) in rows.iter().enumerate() {
        let code = repair_gate_code(r[0], F32::from_bits(r[1]), F32::from_bits(r[2]), r[3] != 0, r[4] as i32, r[5] as i32, r[6] != 0, r[7] != 0);
        assert_eq!(code, r[8], "row {ri}: {r:08x?}");
    }
}
