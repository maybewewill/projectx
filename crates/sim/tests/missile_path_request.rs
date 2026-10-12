//! The missile path request `0x6F496D10` (flight time and velocity) against the original, 1,500 Unicorn vectors
//! (`tools/emu/missile_cap_emu.py`). Needs `GAME_DLL` for the soft-float tables.

#[path = "missile/cap_vectors.rs"]
mod cap_vectors;

use sim::missiles::path_request;
use softfloat::{Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

#[test]
fn the_path_request_matches_the_original() {
    let Some(t) = tables() else {
        return;
    };
    let mut refused = 0;
    for (i, &(dx, dy, a, time, vx, vy)) in cap_vectors::ROWS.iter().enumerate() {
        let r = path_request(F32::from_bits(dx), F32::from_bits(dy), F32::from_bits(a), &t);
        assert_eq!(
            (r.time.to_bits(), r.vx.to_bits(), r.vy.to_bits()),
            (time, vx, vy),
            "row {i}: dx {} dy {} speed {}",
            f32::from_bits(dx),
            f32::from_bits(dy),
            f32::from_bits(a)
        );
        if time == 0 {
            refused += 1;
        }
    }
    assert!(refused > 100 && cap_vectors::ROWS.len() >= 1000);
}
