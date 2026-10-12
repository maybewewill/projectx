//! The terrain queries against the original code (Unicorn, `tools/emu/terrain_emu.py`): the cell level
//! (`0x6F0128C0`) and `GetLocationZ` (`0x6F24F3E0`) on synthetic corner grids with ramps, cliffs and water.

#[path = "terrain/vectors.rs"]
mod vectors;

use sim::terrain::{Corner, RampRect, TerrainGrid};

fn grid(set: &vectors::Set) -> TerrainGrid {
    let corners: Vec<Corner> = set
        .points
        .iter()
        .map(|p| Corner::from_file(p[0], p[1], p[2] as u8, p[4] as u8))
        .collect();
    let mut g = TerrainGrid::new(set.w, set.h, [-6144.0, -6144.0], corners).unwrap();
    g.set_water_offset(Some(set.water_offset));
    for (y0, x0, y1, x1, vals) in set.ramps {
        g.add_ramp_rect(RampRect {
            y0: *y0,
            x0: *x0,
            y1: *y1,
            x1: *x1,
            levels: vals.to_vec(),
        });
    }
    g
}

#[test]
fn cell_level_matches_the_original() {
    let mut n = 0;
    let mut ramp_hits = 0;
    for set in vectors::SETS {
        let g = grid(set);
        for &(cx, cy, _, _, level, ramp_hit, _, _) in set.cases {
            assert_eq!(g.cell_level(cx, cy), level, "cell ({cx}, {cy}) of a {}x{} grid", set.w, set.h);
            n += 1;
            ramp_hits += ramp_hit;
        }
    }
    assert!(n >= 500 && ramp_hits > 20, "{n} cases, {ramp_hits} ramp hits");
}

#[test]
fn ground_height_matches_the_original() {
    let mut n = 0;
    for set in vectors::SETS {
        let g = grid(set);
        for &(_, _, xb, yb, _, _, ground, _) in set.cases {
            let (x, y) = (f32::from_bits(xb), f32::from_bits(yb));
            let z = g.ground_height(x, y);
            assert_eq!(z.to_bits(), ground, "ground at ({x}, {y}): got {z}, want {}", f32::from_bits(ground));
            n += 1;
        }
    }
    assert!(n >= 500);
}

#[test]
fn location_z_matches_the_original() {
    let mut n = 0;
    for set in vectors::SETS {
        let g = grid(set);
        for &(_, _, xb, yb, _, _, _, locz) in set.cases {
            let (x, y) = (f32::from_bits(xb), f32::from_bits(yb));
            let z = g.location_z(x, y);
            assert_eq!(z.to_bits(), locz, "location z at ({x}, {y}): got {z}, want {}", f32::from_bits(locz));
            n += 1;
        }
    }
    assert!(n >= 500);
}
