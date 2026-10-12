//! A unit's sight on a map with a cliff: the whole-map fog grid, the elevation layer built from the terrain, and the
//! cliff-aware sight cast (`docs/spec/vision.md` section 14). Needs the soft-float tables (`GAME_DLL`); skipped without.

use std::sync::Arc;

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{Playable, UnitType};
use sim::terrain::{Corner, TerrainGrid};
use sim::vision::FOG_VISIBLE;
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit

"#;

const FOOTMAN: u32 = 0x6866_6F6F;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

/// A 16 by 16 corner map (15 by 15 tiles) with a cliff wall (layer 2) on the corner column 8, and the cliff level 2 only
/// on that column.
fn walled_terrain() -> TerrainGrid {
    let mut corners = Vec::new();
    for _y in 0..16 {
        for x in 0..16 {
            let layer = if x == 8 { 2 } else { 0 };
            corners.push(Corner::from_file(8192, 0, 0, layer));
        }
    }
    TerrainGrid::new(16, 16, [-960.0, -960.0], corners).expect("terrain")
}

fn host(unit_x: f32, unit_y: f32, with_wall: bool) -> Option<ScriptHost> {
    let t = tables()?;
    let src = format!(
        "{PRELUDE}\nfunction make takes nothing returns nothing\n    local unit u\n    set u = CreateUnit(Player(0), 'hfoo', {unit_x:?}, {unit_y:?}, 0.0)\nendfunction\n"
    );
    let prog = jass::parse_with(&src, Some(&t)).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), Some(t)).expect("vm");
    let ty = UnitType {
        name: "Footman".to_string(),
        max_life: F32::from_bits(0x43D2_0000),
        sight: f(1024.0), // 8 cells, day and night alike below
        night_sight: f(1024.0),
        ..Default::default()
    };
    host.game_mut().unit_types_mut().insert(FOOTMAN, ty);
    // the whole map is [-960, 960]; the playable area is smaller (a border of 256 units), and the fog grid follows the
    // whole map, not the playable area
    host.game_mut().set_playable(Some(Playable { min_x: f(-704.0), min_y: f(-704.0), max_x: f(704.0), max_y: f(704.0) }));
    host.game_mut().set_world_bounds(Some(Playable { min_x: f(-960.0), min_y: f(-960.0), max_x: f(960.0), max_y: f(960.0) }));
    if with_wall {
        host.game_mut().set_terrain(Some(Arc::new(walled_terrain())));
    }
    host.start("make", &[]).expect("make starts");
    host.step();
    Some(host)
}

fn seen(host: &ScriptHost, cell: (i32, i32)) -> bool {
    host.game().fog_state_of_cell(0, cell) == FOG_VISIBLE
}

#[test]
fn the_fog_grid_covers_the_whole_map() {
    let Some(host) = host(-256.0, 0.0, false) else {
        eprintln!("skipped: GAME_DLL is not set");
        return;
    };
    // 1920 units of whole map = 15 cells of 128, with the origin at the map corner, not the playable corner
    assert_eq!(host.game().fog_grid().width, 15);
    assert_eq!(host.game().fog_grid().origin.0.to_f32(), -960.0);
}

#[test]
fn flat_ground_shows_the_circle_and_a_cliff_wall_hides_what_is_behind_it() {
    let (Some(flat), Some(walled)) = (host(-256.0, 0.0, false), host(-256.0, 0.0, true)) else {
        eprintln!("skipped: GAME_DLL is not set");
        return;
    };
    flat.game_mut().vision_refresh();
    walled.game_mut().vision_refresh();
    // The footman at x = -256, y = 0 is in fog cell (5, 7): (-256 + 960) / 128 = 5.5, (0 + 960) / 128 = 7.5.
    let row = 7;
    for c in 5..=12 {
        assert!(seen(&flat, (c, row)), "flat ground, cell {c}");
    }
    // The wall is the corner column 8: it raises the fog cells 7 and 8. Cells up to the wall are seen, the first wall
    // cell is seen (the cliff face), the cells behind it are not (the second wall cell inherits the block).
    assert!(seen(&walled, (6, row)), "in front of the wall");
    assert!(seen(&walled, (7, row)), "the first wall cell");
    assert!(!seen(&walled, (8, row)), "the second wall cell is behind the first");
    assert!(!seen(&walled, (9, row)), "behind the wall");
    assert!(!seen(&walled, (11, row)), "far behind the wall");
    // the west side is open
    assert!(seen(&walled, (1, row)), "west of the unit");
}
