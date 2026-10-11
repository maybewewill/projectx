//! Pathing at boot: `war3map.wpm` is read, built into the pyramid and installed, so a move order
//! routes around a wall in the grid. The walk needs the soft-float tables (`GAME_DLL`); without
//! them the boot and the pathing report are still checked.

mod common;

use common::{mpq, w3i, PlayerRec, BLIZZARD_J, COMMON_J};
use engine::{Engine, Inputs, PathingSummary};
use sim::natives::game::UnitType;
use sim::orders::{AbilityKind, TypeOrderData};
use softfloat::F32;

/// Camera bounds (left, right, bottom, top): a 512 by 256 world, that is 16 by 8 cells.
const BOUNDS: [f32; 8] = [-256.0, 256.0, -128.0, 128.0, 0.0, 0.0, 0.0, 0.0];
const GRID_W: u32 = 16;
const GRID_H: u32 = 8;
/// Raw type id of the test footman ('hfoo').
const FOOTMAN: u32 = 0x6866_6F6F;

/// A map script that declares the natives it uses, creates a footman at cell (2, 2) and orders
/// it to cell (13, 2).
const MAIN_J: &str = r#"
type unit extends handle

native I2R takes integer i returns real
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean

globals
    unit gU = null
endglobals

function config takes nothing returns nothing
endfunction

function main takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', I2R(-176), I2R(-48), I2R(0))
    call IssuePointOrder(gU, "move", I2R(176), I2R(-48))
endfunction
"#;

/// A `war3map.wpm` with `cells` (row-major file bytes) for a `GRID_W` by `GRID_H` grid.
fn wpm(cells: &[u8]) -> Vec<u8> {
    let mut f = b"MP3W".to_vec();
    f.extend(0u32.to_le_bytes());
    f.extend(GRID_W.to_le_bytes());
    f.extend(GRID_H.to_le_bytes());
    f.extend(cells);
    f
}

/// Wall: column 8 blocked (unwalkable, file bit 0x02) for rows 0 to 6; row 7 is the gap.
fn wall_cells() -> Vec<u8> {
    let mut cells = vec![0u8; (GRID_W * GRID_H) as usize];
    for y in 0..7 {
        cells[(y * GRID_W + 8) as usize] = 0x02;
    }
    cells
}

/// A map with one human player, the test script and optionally a `war3map.wpm`.
fn map(wpm_bytes: Option<Vec<u8>>) -> Vec<u8> {
    let players = [PlayerRec {
        number: 0,
        player_type: 1,
        race: 4,
        name: "Alice",
        start: (-176.0, -48.0),
    }];
    let info = w3i("Pathing Test", BOUNDS, &players, &[(0b1, "Force A")]);
    let mut files = vec![
        ("war3map.w3i", info),
        ("war3map.j", MAIN_J.as_bytes().to_vec()),
    ];
    if let Some(bytes) = wpm_bytes {
        files.push(("war3map.wpm", bytes));
    }
    mpq(&files, 512)
}

/// The game's tables, if `GAME_DLL` names a 1.26a `Game.dll`.
fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn boot(map_bytes: Vec<u8>, dll: Option<Vec<u8>>) -> Engine {
    let inputs = Inputs {
        map: map_bytes,
        game_dll: dll,
        common_j: COMMON_J.to_string(),
        blizzard_j: BLIZZARD_J.to_string(),
        rng_seed: 0,
        ai_common: None,
        ai_scripts: Vec::new(),
        game_archives: Vec::new(),
        check_natives: false,
    };
    match Engine::boot(inputs) {
        Ok(e) => e,
        Err(e) => panic!("boot failed: {e}"),
    }
}

#[test]
fn wpm_dimensions_and_origin_are_reported() {
    let engine = boot(map(Some(wpm(&wall_cells()))), None);
    let report = engine.report();
    assert_eq!(
        report.pathing,
        Some(PathingSummary {
            width: GRID_W,
            height: GRID_H,
            origin: (-256.0, -128.0),
        })
    );
    let text = report.to_string();
    assert!(
        text.contains("pathing: 16 x 8 cells (32 units each), origin (-256.0, -128.0)"),
        "{text}"
    );
    assert!(
        !text.contains("cells but the w3i playable size"),
        "the grid matches the playable size, so there is no note: {text}"
    );
}

#[test]
fn map_without_wpm_has_no_pathing_and_says_so() {
    let engine = boot(map(None), None);
    let report = engine.report();
    assert_eq!(report.pathing, None);
    assert!(report.to_string().contains("pathing: none"));
    assert!(report.notes.iter().any(|n| n.contains("no war3map.wpm")));
}

#[test]
fn bad_wpm_is_a_note_and_the_boot_continues() {
    let engine = boot(
        map(Some(b"MP3W\0\0\0\0\x10\0\0\0\x08\0\0\0\x01".to_vec())),
        None,
    );
    let report = engine.report();
    assert_eq!(report.pathing, None);
    assert!(report
        .notes
        .iter()
        .any(|n| n.starts_with("war3map.wpm not used")));
}

/// Boots the map with the footman order, registers the footman type (the engine has no unit
/// data yet), runs the steps and returns the largest y the unit reached and its final position.
fn walk(wpm_bytes: Option<Vec<u8>>, dll: Vec<u8>) -> (f32, f32, f32, bool) {
    let mut engine = boot(map(wpm_bytes), Some(dll));
    {
        let host = engine.host_mut();
        let mut game = host.game_mut();
        game.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                name: "Footman".to_string(),
                max_life: F32::from_bits(0x43D2_0000),
                move_speed: F32::from_bits(270.0f32.to_bits()),
                // The walk only accelerates inside the propulsion window (movement-6.md): real footman values.
                turn_speed: F32::from_bits(0x3F19_999A),
                prop_window: F32::from_bits(0x3F86_0A92),
                ..Default::default()
            },
        );
        game.set_type_orders(
            FOOTMAN,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
    }
    let mut max_y = f32::MIN;
    let mut done = false;
    let mut key = None;
    for _ in 0..400 {
        engine.step();
        if key.is_none() {
            key = engine
                .host()
                .vm()
                .global("gU")
                .and_then(|v| v.as_handle())
                .and_then(|id| engine.host().game().unit_of_handle(id));
        }
        let Some(u) = key else { continue };
        let game = engine.host().game();
        max_y = max_y.max(game.unit(u).y.to_f32());
        if game.unit_order_ids(u).is_empty() && max_y > f32::MIN {
            done = true;
            break;
        }
    }
    let u = key.expect("the footman was created");
    let game = engine.host().game();
    let end = game.unit(u);
    (max_y, end.x.to_f32(), end.y.to_f32(), done)
}

#[test]
fn wall_in_the_wpm_makes_the_move_route_around_it() {
    let Some(dll) = game_dll() else {
        eprintln!("GAME_DLL not set: skipping the walk (boot and report are tested above)");
        return;
    };
    // The straight line from (-176, -48) to (176, -48) crosses the wall at x in [0, 32) below
    // y = 96. The only gap is row 7 (y in [96, 128)), so the route must reach y >= 96.
    let (max_y, x, y, done) = walk(Some(wpm(&wall_cells())), dll.clone());
    assert!(done, "the move completed");
    assert!(
        max_y >= 96.0,
        "the route went through the gap (max y {max_y})"
    );
    assert!((x - 176.0).abs() <= 16.0, "end x {x}");
    assert!((y + 48.0).abs() <= 16.0, "end y {y}");

    // The same order on an open grid is a straight line.
    let open = vec![0u8; (GRID_W * GRID_H) as usize];
    let (max_y, x, y, done) = walk(Some(wpm(&open)), dll);
    assert!(done, "the move completed on the open grid");
    assert!(
        max_y < 0.0,
        "a straight move stays at y = -48 (max y {max_y})"
    );
    assert!((x - 176.0).abs() <= 16.0, "end x {x}");
    assert!((y + 48.0).abs() <= 16.0, "end y {y}");
}
