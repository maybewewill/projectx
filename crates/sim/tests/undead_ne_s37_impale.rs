//! Unicorn vectors of the Impale spike line plan (`tools/emu/undead_ne4_emu.py`, CHECK=impale).

use sim::undead_ne::{halve, impale_plan};
use softfloat::{Tables, F32};

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex")
}

fn f(s: &str) -> F32 {
    F32::from_bits(hex(s))
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

#[test]
fn impale_plan_matches_the_original() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: impale vectors skipped");
        return;
    };
    let text = include_str!("data/undead_ne/impale_plan.txt");
    let (mut facing_rows, mut dir_rows, mut bad) = (0, 0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once(" | ").expect("separator");
        let h: Vec<&str> = head.split_whitespace().collect();
        let w: Vec<&str> = tail.split_whitespace().collect();
        let (area, a, b, cx, cy, tx, ty, facing) = (f(h[0]), f(h[1]), f(h[2]), f(h[3]), f(h[4]), f(h[5]), f(h[6]), f(h[7]));
        let plan = impale_plan((cx, cy), (tx, ty), facing, area, a, b, &t);
        let got = [
            plan.first.0.to_bits(),
            plan.first.1.to_bits(),
            plan.step.0.to_bits(),
            plan.step.1.to_bits(),
            plan.interval.to_bits(),
            plan.first_delay.to_bits(),
        ];
        let want = [hex(w[0]), hex(w[1]), hex(w[2]), hex(w[3]), hex(w[4]), hex(w[5])];
        let near = {
            let (dx, dy) = (tx.to_f32() - cx.to_f32(), ty.to_f32() - cy.to_f32());
            dx * dx + dy * dy < 400.0
        };
        if near {
            facing_rows += 1;
        } else {
            dir_rows += 1;
        }
        if got != want || plan.count != w[6].parse::<i32>().unwrap() {
            bad += 1;
            eprintln!("row {line}\n got {got:08x?} n={}\nwant {want:08x?} n={}", plan.count, w[6]);
        }
    }
    assert_eq!(bad, 0, "{bad} mismatching rows (facing rows {facing_rows}, direct rows {dir_rows})");
    assert!(dir_rows >= 150 && facing_rows >= 40, "{dir_rows} {facing_rows}");
}

#[test]
fn halving_is_an_exponent_decrement() {
    assert_eq!(halve(F32::from_bits(0x4396_0000)).to_bits(), 0x4316_0000); // 300 -> 150
    assert_eq!(halve(F32::ZERO).to_bits(), 0);
}
