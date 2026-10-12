//! The launch point of a ranged attack (`0x6F29E6A0`) against the original, 1,500 Unicorn vectors
//! (`tools/emu/missile_launchpoint_emu.py`). Needs `GAME_DLL` for the sine/cosine tables.

#[path = "missile/launchpoint_vectors.rs"]
mod launchpoint_vectors;

use sim::missiles::launch_point;
use softfloat::{Tables, F32};

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

#[test]
fn the_launch_point_matches_the_original() {
    let Some(t) = tables() else {
        return;
    };
    let b = F32::from_bits;
    let mut flagged = 0;
    for (i, r) in launchpoint_vectors::ROWS.iter().enumerate() {
        let g = (r[7] != 0).then(|| [b(r[8]), b(r[9]), b(r[10])]);
        flagged += g.is_some() as usize;
        let got = launch_point(
            (b(r[3]), b(r[4])),
            b(r[5]),
            b(r[6]),
            [b(r[0]), b(r[1]), b(r[2])],
            g,
            &t,
        );
        assert_eq!(
            [got[0].to_bits(), got[1].to_bits(), got[2].to_bits()],
            [r[11], r[12], r[13]],
            "row {i}: {r:08X?}"
        );
    }
    assert!(flagged > 100 && launchpoint_vectors::ROWS.len() >= 1500);
}
