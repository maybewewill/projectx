//! The terrain grid built from `war3map.w3e` (`engine::terrain`). The synthetic test always runs; the real-map test
//! needs the user's own `(2)Circumvention.w3x` terrain extracted to a file (`TERRAIN_W3E=path/to/war3map.w3e`) and
//! compares 700 queries with the values the original `Game.dll` code gave on the same file
//! (`tools/emu/terrain_emu.py --w3e`, vectors in `crates/sim/tests/terrain/vectors.rs`, `REAL`).

#[path = "../../sim/tests/terrain/vectors.rs"]
#[allow(dead_code)]
mod vectors;

use engine::terrain::grid_from_w3e;
use sim::combat::CliffLevels;
use w3map::{Terrain, TilePoint};

fn point(height: i16, layer: u8, ramp: bool) -> TilePoint {
    TilePoint {
        ground_height: height,
        water_level: 0x2000,
        map_edge: false,
        ground_texture: 0,
        ramp,
        blight: false,
        water: false,
        boundary: false,
        ground_variation: 0,
        cliff_variation: 0,
        cliff_texture: 0,
        layer_height: layer,
    }
}

#[test]
fn a_synthetic_w3e_becomes_a_grid_with_the_layer_as_level() {
    // 3x3 corners: the left column on layer 2, the others on layer 4.
    let mut points = Vec::new();
    for _y in 0..3 {
        for x in 0..3 {
            points.push(point(0x2000, if x == 0 { 2 } else { 4 }, false));
        }
    }
    let t = Terrain {
        main_tileset: b'L',
        custom_tileset: false,
        ground_tilesets: vec![],
        cliff_tilesets: vec![],
        width: 3,
        height: 3,
        center: [-256.0, -256.0],
        points,
    };
    let g = grid_from_w3e(&t).expect("grid");
    // cell 0 and 1 are nearest corner column 0 ((c + 2) / 4 = 0), cell 2 and later column 1
    assert_eq!(g.cell_level(0, 0), 2);
    assert_eq!(g.cell_level(1, 5), 2);
    assert_eq!(g.cell_level(2, 0), 4);
    assert_eq!(g.cell_level(7, 7), 4);
    // outside the 8x8 cells of a 3x3 corner grid
    assert_eq!(g.cell_level(8, 0), -1);
    assert_eq!(g.cell_level(-1, 0), -1);
    // flat ground at raw 0x2000 plus the layer: (layer - 2) * 128 in world units
    assert_eq!(g.ground_height(-256.0 + 192.0, -256.0 + 64.0), 128.0 * 2.0);
    assert!(CliffLevels::level(&g, 0, 0) < CliffLevels::level(&g, 7, 0));
}

#[test]
fn the_real_map_matches_the_original() {
    let Ok(path) = std::env::var("TERRAIN_W3E") else {
        return;
    };
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("reading TERRAIN_W3E={path}: {e}"));
    let t = Terrain::parse(&data).expect("w3e");
    let mut g = grid_from_w3e(&t).expect("grid");
    g.set_water_offset(Some(0.0));
    let mut differing = 0;
    for &(cx, cy, xb, yb, level, ground, locz) in vectors::REAL {
        assert_eq!(g.cell_level(cx, cy), level, "cell ({cx}, {cy})");
        let (x, y) = (f32::from_bits(xb), f32::from_bits(yb));
        assert_eq!(g.ground_height(x, y).to_bits(), ground, "ground at ({x}, {y})");
        assert_eq!(g.location_z(x, y).to_bits(), locz, "location z at ({x}, {y})");
        if level != vectors::REAL[0].4 {
            differing += 1;
        }
    }
    assert!(vectors::REAL.len() >= 500 && differing > 0);
}
