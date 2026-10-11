//! Set-then-get round trips of unit positions against the original (`docs/spec/unit-position.md`).
//!
//! The vectors come from `tools/emu/unit_position_emu.py` (Unicorn runs of the real cell setter `0x6F474250` and the
//! real normaliser `0x6F4743A0`, grid A, random origins and grid values). They are not committed (they are outputs
//! of the user's own `Game.dll`). Run:
//!
//!   GAME_DLL=/path/Game.dll python3 tools/emu/unit_position_emu.py /tmp/up.txt 2000 1
//!   UNIT_POSITION_VECTORS=/tmp/up.txt cargo test -p sim --test unit_position_vectors -- --nocapture
//!
//! Without the variable the test is skipped (and says so).

use softfloat::F32;
use sim::unit_position::{cell_to_world, stored_cell};

fn f(bits: u32) -> F32 {
    F32(bits)
}

#[test]
fn set_then_get_matches_the_original() {
    let Ok(path) = std::env::var("UNIT_POSITION_VECTORS") else {
        eprintln!("skipped: UNIT_POSITION_VECTORS not set");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("vector file");
    let (mut cases, mut mismatches, mut other_moved, mut cell_mismatches) = (0usize, 0usize, 0usize, 0usize);
    let mut per_mode = [0usize; 3];
    for line in text.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        if t.len() != 12 || t[0] != "P" {
            continue;
        }
        let mode: usize = t[1].parse().unwrap();
        let hex = |i: usize| u32::from_str_radix(t[i], 16).unwrap();
        let (ox, oy) = (f(hex(2)), f(hex(3)));
        let (cx0, cy0) = (f(hex(4)), f(hex(5)));
        let (xin, yin) = (f(hex(6)), f(hex(7)));
        let (nx, ny, cx1, cy1) = (hex(8), hex(9), hex(10), hex(11));

        // The other coordinate as the normaliser reads it before the set (the original writes it back).
        let x_pre = cell_to_world(cx0, ox);
        let y_pre = cell_to_world(cy0, oy);
        let (cx, cy) = match mode {
            0 => (stored_cell(cx0, xin, ox), stored_cell(cy0, y_pre, oy)),
            1 => (stored_cell(cx0, x_pre, ox), stored_cell(cy0, yin, oy)),
            _ => (stored_cell(cx0, xin, ox), stored_cell(cy0, yin, oy)),
        };
        let (mx, my) = (cell_to_world(cx, ox), cell_to_world(cy, oy));
        cases += 1;
        per_mode[mode] += 1;
        if (cx.0, cy.0) != (cx1, cy1) {
            cell_mismatches += 1;
            if cell_mismatches <= 5 {
                eprintln!("cell mismatch mode {mode}: got ({:08x},{:08x}) want ({cx1:08x},{cy1:08x}) pre ({:08x},{:08x}) org ({:08x},{:08x}) in ({:08x},{:08x})", cx.0, cy.0, cx0.0, cy0.0, ox.0, oy.0, xin.0, yin.0);
            }
        }
        if (mx.0, my.0, cx.0, cy.0) != (nx, ny, cx1, cy1) {
            mismatches += 1;
            if mismatches <= 5 {
                eprintln!("mismatch mode {mode}: got ({:08x},{:08x}) want ({nx:08x},{ny:08x})", mx.0, my.0);
            }
        }
        if (mode == 0 && ny != y_pre.0) || (mode == 1 && nx != x_pre.0) {
            other_moved += 1;
        }
    }
    eprintln!("cell mismatches {cell_mismatches}");
    eprintln!(
        "unit position vectors: {cases} cases (setX {}, setY {}, pair {}), mismatches {mismatches}, \
         other coordinate re-rounded {other_moved}",
        per_mode[0], per_mode[1], per_mode[2]
    );
    assert!(cases > 0, "no vectors in {path}");
    assert_eq!(mismatches, 0);
}
