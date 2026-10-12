//! The fog grid of the playable area, the sight circles and the vision refresh (`docs/spec/vision.md`).
//!
//! * The constructor vectors are from the original (`tools/emu/vision_grid_emu.py`, 300 playable rectangles, 0
//!   mismatches on the cell counts and the words). The scale is a soft-float quotient, so these tests need the lookup
//!   tables (`GAME_DLL`); they are skipped without it.
//! * The refresh runs from the 0.4 s timer (message 0x80269). The test checks that the timer, not a manual call,
//!   makes a unit's sight visible.
use jass::compile::compile;
use jass::vm::{NativeRegistry, Value};
use sim::natives::game::UnitType;
use sim::vision::{VisionGrid, FOG_FOGGED, FOG_MASKED, FOG_VISIBLE};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IsUnitVisible takes unit whichUnit, player whichPlayer returns boolean
native IsUnitFogged takes unit whichUnit, player whichPlayer returns boolean
native IsUnitMasked takes unit whichUnit, player whichPlayer returns boolean

"#;

/// The footman's raw type id (`'hfoo'`).
const FOOTMAN: u32 = 0x6866_6F6F;

/// Constructor vectors from the original: (min y, min x, max y, max x, width, height, scale x bits, scale y bits).
/// One constructor vector: the playable rectangle, the cell counts and the scale words.
type CtorVector = (f32, f32, f32, f32, i32, i32, u32, u32);

const CTOR_VECTORS: &[CtorVector] = &[
    (-3529.0, -695.0, -2608.0, 5903.0, 51, 7, 1006454967, 1006177553),
    (2779.0, -4814.0, 8900.0, -3142.0, 13, 47, 1006552687, 1006345165),
    (-5050.0, 3548.0, -1403.0, 11991.0, 65, 28, 1006388549, 1006343145),
    (-4592.0, -5386.0, 2389.0, 1848.0, 56, 54, 1006479893, 1006467136),
    (-2057.0, -4856.0, 5028.0, -3240.0, 12, 55, 1005802404, 1006526402),
    (3264.0, -5032.0, 7051.0, -2874.0, 16, 29, 1005777774, 1006300696),
    (4279.0, 4332.0, 10908.0, 5475.0, 8, 51, 1004886252, 1006377343),
    (-2378.0, -5188.0, -67.0, -4295.0, 6, 18, 1004284526, 1006582146),
    (867.0, -1256.0, 9855.0, 1237.0, 19, 70, 1006222449, 1006580698),
    (3353.0, -4071.0, 6444.0, 1113.0, 40, 24, 1006425835, 1006529835),
    (3528.0, -4312.0, 9759.0, -1104.0, 25, 48, 1006591122, 1006398710),
    (2974.0, -4404.0, 4080.0, -3246.0, 9, 8, 1006546032, 1005389081),
    (-2626.0, 4141.0, 6215.0, 12404.0, 64, 69, 1006488803, 1006615883),
    (-854.0, 1005.0, 6700.0, 8763.0, 60, 59, 1006464282, 1006628520),
    (-1089.0, -76.0, 1986.0, 4124.0, 32, 24, 1006217526, 1006616594),
    (-2001.0, 5452.0, 3048.0, 6923.0, 11, 39, 1005914425, 1006443559),
    (2111.0, 2604.0, 9594.0, 8361.0, 44, 58, 1006268683, 1006500681),
    (3977.0, -1283.0, 6041.0, 46.0, 10, 16, 1006014387, 1006502906),
    (850.0, 2387.0, 6584.0, 5219.0, 22, 44, 1006538174, 1006334518),
    (2011.0, -3510.0, 2783.0, 3529.0, 54, 6, 1006330259, 1006546032),
    (-4729.0, 4948.0, 973.0, 10218.0, 41, 44, 1006562924, 1006426999),
    (-263.0, 5391.0, 7341.0, 13658.0, 64, 59, 1006480758, 1006518229),
    (-4467.0, -4874.0, 3430.0, -322.0, 35, 61, 1006367592, 1006443881),
    (4881.0, 5420.0, 6005.0, 6614.0, 9, 8, 1006042807, 1005140326),
    (5493.0, 5979.0, 12924.0, 11181.0, 40, 58, 1006368501, 1006617156),
    (5741.0, -1338.0, 11556.0, 5112.0, 50, 45, 1006502906, 1006474277),
    (1564.0, -5631.0, 4447.0, 322.0, 46, 22, 1006449773, 1006243065),
    (-4082.0, 4009.0, -2987.0, 12227.0, 64, 8, 1006579885, 1005545123),
    (-1291.0, -2425.0, 2895.0, -176.0, 17, 32, 1006088394, 1006272251),
    (405.0, 519.0, 1855.0, 8783.0, 64, 11, 1006486790, 1006146999),
    (1359.0, -3275.0, 6041.0, 3435.0, 52, 36, 1006497944, 1006367796),
    (1053.0, -3757.0, 7987.0, 934.0, 36, 54, 1006336115, 1006579733),
    (5185.0, -122.0, 9095.0, 6241.0, 49, 30, 1006393023, 1006332600),
    (-4641.0, -3528.0, -2033.0, -511.0, 23, 20, 1006227017, 1006324177),
    (4789.0, -2200.0, 5116.0, 1752.0, 30, 2, 1006157492, 1002990201),
    (3652.0, 1945.0, 8086.0, 5062.0, 24, 34, 1006390751, 1006322695),
    (-5933.0, -1381.0, 1061.0, 1135.0, 19, 54, 1006072831, 1006436260),
    (49.0, 2758.0, 2235.0, 8108.0, 41, 17, 1006313097, 1006556212),
    (2445.0, 5313.0, 10056.0, 6327.0, 7, 59, 1004680582, 1006502905),
    (3163.0, 5150.0, 9814.0, 11708.0, 51, 51, 1006556213, 1006322692),
];

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

/// A footman with a 512-unit day sight and no night sight.
fn footman_type() -> UnitType {
    UnitType {
        name: "Footman".to_string(),
        max_life: F32::from_bits(0x43D2_0000),
        sight: F32::from_bits(0x4400_0000), // 512.0
        ..Default::default()
    }
}

fn host_with_footman() -> ScriptHost {
    let src = format!(
        "{PRELUDE}\nfunction make takes nothing returns nothing\n    local unit u\n    set u = CreateUnit(Player(0), 'hfoo', 1000.0, 1000.0, 0.0)\nendfunction\n"
    );
    let prog = jass::parse(&src).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    host.game_mut().unit_types_mut().insert(FOOTMAN, footman_type());
    host
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| match v {
            Value::Boolean(b) => Some(b),
            _ => None,
        })
        .unwrap_or_else(|| panic!("global {name} is not a boolean"))
}

#[test]
fn constructor_matches_the_original_vectors() {
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL is not set (the constructor needs the soft-float tables)");
        return;
    };
    for &(min_y, min_x, max_y, max_x, w, h, sx, sy) in CTOR_VECTORS {
        let g = VisionGrid::from_playable(f(min_x), f(min_y), f(max_x), f(max_y), &t);
        assert_eq!((g.width, g.height), (w, h), "cells of {min_x},{min_y} .. {max_x},{max_y}");
        assert_eq!(g.scale.0.to_bits(), sx, "scale x of {min_x}..{max_x}");
        assert_eq!(g.scale.1.to_bits(), sy, "scale y of {min_y}..{max_y}");
        assert_eq!(g.origin.0.to_bits(), f(min_x).to_bits());
        assert_eq!(g.origin.1.to_bits(), f(min_y).to_bits());
        assert!(g.pitch >= w && g.pitch / 2 < w.max(2), "pitch is the next power of two");
    }
}

#[test]
fn sight_reveals_the_circle_and_the_refresh_clears_it() {
    let mut host = host_with_footman();
    host.start("make", &[]).expect("make starts");
    host.step();
    // Default grid (no tables): 256 cells of 1/128. The footman at (1000, 1000) has a 512-unit sight, 4 cells.
    host.game_mut().vision_refresh();
    let p0 = 0;
    assert_eq!(host.game().fog_state_at(p0, f(1000.0), f(1000.0)), FOG_VISIBLE);
    assert_eq!(host.game().fog_state_at(p0, f(1700.0), f(1000.0)), FOG_MASKED, "outside the sight, never explored");

    // Without the unit's sight the next refresh clears the visible layer: the explored cell is now fogged.
    let key = host.game().unit_objects().next().map(|(k, _)| k).expect("the footman exists");
    host.game_mut().unit_mut(key).sight = F32::ZERO;
    host.game_mut().vision_refresh();
    assert_eq!(host.game().fog_state_at(p0, f(1000.0), f(1000.0)), FOG_FOGGED);
}

#[test]
fn the_refresh_timer_runs_every_point_four_seconds() {
    let mut host = host_with_footman();
    host.start("make", &[]).expect("make starts");
    // 0.3 s of game time: the first refresh (0.4 s) has not run yet.
    host.step_ticks(60);
    assert_eq!(host.game().fog_state_at(0, f(1000.0), f(1000.0)), FOG_MASKED);
    // 0.45 s: the refresh has run once.
    host.step_ticks(30);
    assert_eq!(host.game().fog_state_at(0, f(1000.0), f(1000.0)), FOG_VISIBLE);
}

#[test]
fn unit_visibility_natives_follow_the_cell_state() {
    // One script: a global holds the footman; the probes ask the natives for player 0 and player 1.
    let src = r#"
globals
    unit gU = null
    boolean gVis0 = false
    boolean gVis1 = false
    boolean gFog0 = false
endglobals
function make takes nothing returns nothing
    set gU = CreateUnit(Player(0), 'hfoo', 1000.0, 1000.0, 0.0)
endfunction
function probe takes nothing returns nothing
    set gVis0 = IsUnitVisible(gU, Player(0))
    set gVis1 = IsUnitVisible(gU, Player(1))
    set gFog0 = IsUnitFogged(gU, Player(0))
endfunction
"#;
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    host.game_mut().unit_types_mut().insert(FOOTMAN, footman_type());

    host.start("make", &[]).expect("make starts");
    host.step();
    host.game_mut().vision_refresh();
    host.start("probe", &[]).expect("probe starts");
    host.step();
    assert!(boolean(&host, "gVis0"), "IsUnitVisible: its cell is visible to player 0");
    assert!(!boolean(&host, "gVis1"), "player 1 does not see it");
    assert!(!boolean(&host, "gFog0"), "a visible cell is not fogged");
}

/// A footman with a 512-unit day sight and a 256-unit night sight.
fn footman_type_with_night() -> UnitType {
    UnitType { night_sight: F32::from_bits(0x4380_0000), ..footman_type() }
}

/// The day/night switch moves the sight value over 3 seconds (`0x6f27a4e0`: a modifier of `delta / 3` for 3 s on the
/// unit's sight property); a unit created at night starts at the night value (`0x6f29f180`).
#[test]
fn the_night_switch_ramps_the_sight_over_three_seconds() {
    let Some(t) = tables() else {
        eprintln!("skipped: GAME_DLL is not set (the sight property needs the soft-float tables)");
        return;
    };
    let src = format!(
        "{PRELUDE}\nfunction make takes nothing returns nothing\n    local unit u\n    set u = CreateUnit(Player(0), 'hfoo', 1000.0, 1000.0, 0.0)\nendfunction\n"
    );
    let prog = jass::parse_with(&src, Some(&t)).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), Some(t)).expect("vm");
    host.game_mut().unit_types_mut().insert(FOOTMAN, footman_type_with_night());
    host.start("make", &[]).expect("make starts");
    host.step();
    let key = host.game().unit_objects().next().map(|(k, _)| k).expect("the footman exists");
    // The default clock starts at time 0.0 of the day: night.
    assert!(host.game().is_night());
    let v = |host: &mut ScriptHost, night: bool| host.game_mut().unit_sight_value(key, night).to_f32();
    assert_eq!(v(&mut host, true), 256.0, "created at night: the night sight at once");
    // Dawn: the day sight is approached over three seconds.
    host.game_mut().set_time_of_day(f(12.0));
    assert!(!host.game().is_night());
    let start = v(&mut host, false);
    assert_eq!(start, 256.0, "the ramp starts from the night value");
    host.step_ticks(150); // 0.75 s
    let mid = v(&mut host, false);
    assert!(mid > 300.0 && mid < 340.0, "a quarter of the way: {mid}");
    host.step_ticks(600); // 3.75 s in total
    let end = v(&mut host, false);
    assert!((end - 512.0).abs() < 0.01, "back at the day sight: {end}");
}
