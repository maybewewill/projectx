//! The unit height (`0x6F281F50`) against the original, 1,500 Unicorn vectors (`tools/emu/unit_z_emu.py`).

#[path = "vectors/unit_z_vectors.rs"]
mod unit_z_vectors;

use sim::unitz::unit_height;
use softfloat::F32;

#[test]
fn the_unit_height_matches_the_original() {
    let b = F32::from_bits;
    let (mut raised, mut with_water) = (0, 0);
    for (i, r) in unit_z_vectors::ROWS.iter().enumerate() {
        let water = (r[1] != 0).then(|| b(r[2]));
        let got = unit_height(b(r[0]), water, b(r[3]), r[4], r[5] != 0);
        assert_eq!(got.to_bits(), r[6], "row {i}: {r:08X?}");
        with_water += water.is_some() as usize;
        raised += (water.is_some() && r[4] & 0x38 != 0 && b(r[2]).to_f32() > b(r[0]).to_f32()) as usize;
    }
    assert!(with_water > 500 && raised > 100 && unit_z_vectors::ROWS.len() >= 1000);
}
