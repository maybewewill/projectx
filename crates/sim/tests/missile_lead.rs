//! The target point of a missile launch (`0x6F2C3F70`, the branch that does not miss) against the original, 1,800
//! Unicorn vectors (`tools/emu/missile_launch_emu.py`). Needs `GAME_DLL` for the soft-float tables.

#[path = "missile/lead_vectors.rs"]
mod lead_vectors;

use sim::missiles::launch_target;
use softfloat::{Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

#[test]
fn the_aimed_point_matches_the_original() {
    let Some(t) = tables() else {
        return;
    };
    let b = F32::from_bits;
    let mut led = 0;
    for (i, r) in lead_vectors::ROWS.iter().enumerate() {
        let got = launch_target(
            (b(r[0]), b(r[1])),
            (b(r[2]), b(r[3])),
            (b(r[4]), b(r[5])),
            b(r[6]),
            b(r[7]),
            b(r[8]),
            [b(r[9]), b(r[10]), b(r[11]), b(r[12])],
            &t,
        );
        assert_eq!(
            (got.x.to_bits(), got.y.to_bits(), got.z.to_bits(), got.intercepted as u32),
            (r[13], r[14], r[15], r[16]),
            "row {i}: {r:08X?}"
        );
        led += got.intercepted as usize;
    }
    assert!(led > 500 && lead_vectors::ROWS.len() >= 1500);
}
