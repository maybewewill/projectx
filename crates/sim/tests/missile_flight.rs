//! The flight setup of a missile (cells, path request, `min(T, horizon - 1, 60)`, the `1e-4` floor, the expiry) against
//! the original: 1,500 Unicorn vectors of the real chain `0x6F47A650 -> 0x6F497080 -> 0x6F4AA6A0`, one third of them the
//! homing branch `0x6F497260` (`tools/emu/missile_flight_emu.py`). Needs `GAME_DLL` for the soft-float tables.

#[path = "missile/flight_vectors.rs"]
mod flight_vectors;

use sim::missiles::{cell_position, flight_duration, flight_duration_cells};
use softfloat::{arith, Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

#[test]
fn the_flight_duration_and_expiry_match_the_original() {
    let Some(t) = tables() else {
        return;
    };
    let b = F32::from_bits;
    let (mut capped, mut floored, mut long, mut homing) = (0, 0, 0, 0);
    for (i, r) in flight_vectors::ROWS.iter().enumerate() {
        let d = if r[12] == 0 {
            flight_duration(
                (b(r[0]), b(r[1])),
                (b(r[3]), b(r[4])),
                (b(r[8]), b(r[9])),
                b(r[6]),
                b(r[11]),
                &t,
            )
        } else {
            homing += 1;
            use sim::pathing::scale_by_1_32 as cells;
            let start = (
                cells(arith::sub(b(r[0]), b(r[8]))),
                cells(arith::sub(b(r[1]), b(r[9]))),
            );
            let target = (
                cell_position(b(r[13]), b(r[15]), F32::ZERO),
                cell_position(b(r[14]), b(r[16]), F32::ZERO),
            );
            flight_duration_cells(start, target, b(r[6]), b(r[11]), &t)
        };
        let expiry = arith::add(b(r[10]), d);
        assert_eq!(
            (expiry.to_bits(), d.to_bits()),
            (r[17], r[18]),
            "row {i}: {r:08X?}"
        );
        floored += (d.to_bits() == 0x38D1_B717) as usize;
        capped += (d.to_f32() == 60.0) as usize;
        long += (d.to_f32() > 1.0) as usize;
    }
    assert!(floored > 100 && capped > 10 && long > 200 && homing > 400 && flight_vectors::ROWS.len() >= 1000);
}
