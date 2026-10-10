//! Location and rect natives through `ScriptHost` (`natives/location.rs`).
//!
//! The integer cases need no lookup tables (`I2R` gives exact values). The fractional case compares
//! bits with the same softfloat expression, and it runs with the literals of the user's `Game.dll`
//! when `GAME_DLL` is set (the literal text is converted with the original's tables).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::{Playable, ScriptHost};
use softfloat::{arith, Tables, F32};

const PRELUDE: &str = r#"
type location extends handle
type rect extends handle

native Location takes real x, real y returns location
native RemoveLocation takes location l returns nothing
native MoveLocation takes location l, real x, real y returns nothing
native GetLocationX takes location l returns real
native GetLocationY takes location l returns real
native GetLocationZ takes location l returns real
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native RectFromLoc takes location a, location b returns rect
native RemoveRect takes rect r returns nothing
native SetRect takes rect r, real minx, real miny, real maxx, real maxy returns nothing
native SetRectFromLoc takes rect r, location a, location b returns nothing
native GetRectCenterX takes rect r returns real
native GetRectCenterY takes rect r returns real
native MoveRectTo takes rect r, real x, real y returns nothing
native MoveRectToLoc takes rect r, location l returns nothing
native GetRectMinX takes rect r returns real
native GetRectMinY takes rect r returns real
native GetRectMaxX takes rect r returns real
native GetRectMaxY takes rect r returns real
native I2R takes integer i returns real
"#;

/// The soft-float lookup tables of the user's `Game.dll` (`GAME_DLL`), or `None` when it is unset.
fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn host_with(src: &str, playable: Option<Playable>) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse_with(&full, tables().as_ref()).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    host.set_playable(playable);
    host
}

fn host_for(src: &str) -> ScriptHost {
    host_with(src, None)
}

fn real(host: &ScriptHost, name: &str) -> f32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_real())
        .map(|r| r.to_f32())
        .unwrap_or_else(|| panic!("global {name} is not a real"))
}

fn real_bits(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_real())
        .map(|r| r.to_bits())
        .unwrap_or_else(|| panic!("global {name} is not a real"))
}

/// The value of a real literal as the lexer reads it (`softfloat::text::real_literal`, not `f32` parsing).
fn lit(text: &str, tables: &Tables) -> F32 {
    softfloat::text::real_literal(Some(tables), text.as_bytes()).expect("real literal")
}

const PLAYABLE: Playable = Playable {
    min_x: F32::from_bits(0),
    min_y: F32::from_bits(0),
    max_x: F32::from_bits(0x42C8_0000),
    max_y: F32::from_bits(0x42C8_0000),
};

#[test]
fn locations_store_raw_coordinates_and_move_and_remove() {
    let src = r#"
globals
    location gL = null
    real gX = 0.0
    real gY = 0.0
    real gZ = 1.0
    real gXAfterMove = 0.0
    real gYAfterMove = 0.0
    real gXRemoved = 1.0
endglobals

function Main takes nothing returns nothing
    set gL = Location(I2R(3), I2R(4))
    set gX = GetLocationX(gL)
    set gY = GetLocationY(gL)
    set gZ = GetLocationZ(gL)
    call MoveLocation(gL, I2R(-7), I2R(9))
    set gXAfterMove = GetLocationX(gL)
    set gYAfterMove = GetLocationY(gL)
    call RemoveLocation(gL)
    set gXRemoved = GetLocationX(gL)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(real(&host, "gX"), 3.0);
    assert_eq!(real(&host, "gY"), 4.0);
    assert_eq!(real(&host, "gZ"), 0.0);
    assert_eq!(real(&host, "gXAfterMove"), -7.0);
    assert_eq!(real(&host, "gYAfterMove"), 9.0);
    assert_eq!(real(&host, "gXRemoved"), 0.0);
}

#[test]
fn rect_edges_are_sorted_and_the_center_is_the_softfloat_mean() {
    let src = r#"
globals
    rect gR = null
    real gMinX = 0.0
    real gMinY = 0.0
    real gMaxX = 0.0
    real gMaxY = 0.0
    real gCx = 0.0
    real gCy = 0.0
endglobals

function Main takes nothing returns nothing
    // Given with max before min on both axes: the edges are sorted.
    set gR = Rect(I2R(5), I2R(9), I2R(1), I2R(2))
    set gMinX = GetRectMinX(gR)
    set gMinY = GetRectMinY(gR)
    set gMaxX = GetRectMaxX(gR)
    set gMaxY = GetRectMaxY(gR)
    set gCx = GetRectCenterX(gR)
    set gCy = GetRectCenterY(gR)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(real(&host, "gMinX"), 1.0);
    assert_eq!(real(&host, "gMinY"), 2.0);
    assert_eq!(real(&host, "gMaxX"), 5.0);
    assert_eq!(real(&host, "gMaxY"), 9.0);
    assert_eq!(real(&host, "gCx"), 3.0);
    assert_eq!(real(&host, "gCy"), 5.5);
}

#[test]
fn corners_are_clamped_to_the_playable_area_only_when_one_is_set() {
    let src = r#"
globals
    rect gR = null
    real gMinX = 0.0
    real gMaxX = 0.0
    real gMinY = 0.0
    real gMaxY = 0.0
endglobals

function Main takes nothing returns nothing
    set gR = Rect(I2R(-10), I2R(-10), I2R(200), I2R(50))
    set gMinX = GetRectMinX(gR)
    set gMinY = GetRectMinY(gR)
    set gMaxX = GetRectMaxX(gR)
    set gMaxY = GetRectMaxY(gR)
endfunction
"#;
    let mut clamped = host_with(src, Some(PLAYABLE));
    clamped.start("Main", &[]).unwrap();
    clamped.step();
    assert_eq!(real(&clamped, "gMinX"), 0.0);
    assert_eq!(real(&clamped, "gMinY"), 0.0);
    assert_eq!(real(&clamped, "gMaxX"), 100.0);
    assert_eq!(real(&clamped, "gMaxY"), 50.0);

    // No playable area: the rect keeps the values it was given (no clamp is modelled).
    let mut raw = host_for(src);
    raw.start("Main", &[]).unwrap();
    raw.step();
    assert_eq!(real(&raw, "gMinX"), -10.0);
    assert_eq!(real(&raw, "gMaxX"), 200.0);
}

#[test]
fn move_rect_to_moves_the_center_and_keeps_the_size() {
    let src = r#"
globals
    rect gR = null
    location gL = null
    real gCx = 0.0
    real gCy = 0.0
    real gMinX = 0.0
    real gMinY = 0.0
    real gMaxX = 0.0
    real gMaxY = 0.0
endglobals

function Main takes nothing returns nothing
    set gR = Rect(I2R(-10), I2R(-10), I2R(200), I2R(50))
    // The clamped rect is (0, 0) - (100, 50), its center (50, 25).
    call MoveRectTo(gR, I2R(10), I2R(10))
    set gCx = GetRectCenterX(gR)
    set gCy = GetRectCenterY(gR)
    set gMinX = GetRectMinX(gR)
    set gMinY = GetRectMinY(gR)
    set gMaxX = GetRectMaxX(gR)
    set gMaxY = GetRectMaxY(gR)
    set gL = Location(I2R(-5), I2R(-6))
    call MoveRectToLoc(gR, gL)
    set gCx = GetRectCenterX(gR)
    set gCy = GetRectCenterY(gR)
endfunction
"#;
    let mut host = host_with(src, Some(PLAYABLE));
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(real(&host, "gMinX"), -40.0);
    assert_eq!(real(&host, "gMinY"), -15.0);
    assert_eq!(real(&host, "gMaxX"), 60.0);
    assert_eq!(real(&host, "gMaxY"), 35.0);
    assert_eq!(real(&host, "gCx"), -5.0);
    assert_eq!(real(&host, "gCy"), -6.0);
}

#[test]
fn set_rect_and_rect_from_locations_sort_and_clamp() {
    let src = r#"
globals
    rect gR = null
    rect gQ = null
    location gA = null
    location gB = null
    real gMinX = 0.0
    real gMinY = 0.0
    real gMaxX = 0.0
    real gMaxY = 0.0
    real gQMinX = 0.0
    real gQMaxY = 0.0
endglobals

function Main takes nothing returns nothing
    set gR = Rect(I2R(0), I2R(0), I2R(1), I2R(1))
    call SetRect(gR, I2R(20), I2R(-3), I2R(4), I2R(8))
    set gMinX = GetRectMinX(gR)
    set gMinY = GetRectMinY(gR)
    set gMaxX = GetRectMaxX(gR)
    set gMaxY = GetRectMaxY(gR)
    set gA = Location(I2R(1), I2R(2))
    set gB = Location(I2R(5), I2R(-3))
    call SetRectFromLoc(gR, gA, gB)
    set gMinX = GetRectMinX(gR)
    set gMaxY = GetRectMaxY(gR)
    set gQ = RectFromLoc(gA, gB)
    set gQMinX = GetRectMinX(gQ)
    set gQMaxY = GetRectMaxY(gQ)
endfunction
"#;
    let mut host = host_with(src, Some(PLAYABLE));
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(real(&host, "gMinX"), 1.0);
    assert_eq!(real(&host, "gMaxY"), 2.0);
    assert_eq!(real(&host, "gQMinX"), 1.0);
    assert_eq!(real(&host, "gQMaxY"), 2.0);
}

#[test]
fn removed_rects_and_null_rects_read_as_zero_and_ignore_moves() {
    let src = r#"
globals
    rect gR = null
    real gCx = 9.0
    real gMinX = 9.0
    boolean gRectFromNull = true
endglobals

function Main takes nothing returns nothing
    set gR = Rect(I2R(2), I2R(2), I2R(4), I2R(4))
    call RemoveRect(gR)
    set gCx = GetRectCenterX(gR)
    set gMinX = GetRectMinX(gR)
    call MoveRectTo(gR, I2R(1), I2R(1))
    call SetRect(gR, I2R(1), I2R(1), I2R(2), I2R(2))
    set gRectFromNull = RectFromLoc(null, null) == null
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();
    assert_eq!(real(&host, "gCx"), 0.0);
    assert_eq!(real(&host, "gMinX"), 0.0);
    assert!(host
        .vm()
        .global("gRectFromNull")
        .and_then(|v| v.as_boolean())
        .unwrap());
}

#[test]
fn fractional_rect_math_is_bit_exact_with_softfloat() {
    // Literal text goes through the original's S2R, so this needs the tables (GAME_DLL).
    if std::env::var("GAME_DLL").is_err() {
        eprintln!("skipped: GAME_DLL is not set (literal bits need the lookup tables)");
        return;
    }
    let src = r#"
globals
    rect gR = null
    real gCx = 0.0
    real gCy = 0.0
    real gMinX = 0.0
    real gMinY = 0.0
    real gMaxX = 0.0
    real gMaxY = 0.0
endglobals

function Main takes nothing returns nothing
    set gR = Rect(0.1, 0.2, 0.7, 0.9)
    set gCx = GetRectCenterX(gR)
    set gCy = GetRectCenterY(gR)
    call MoveRectTo(gR, 0.3, 0.4)
    set gMinX = GetRectMinX(gR)
    set gMinY = GetRectMinY(gR)
    set gMaxX = GetRectMaxX(gR)
    set gMaxY = GetRectMaxY(gR)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).unwrap();
    host.step();

    // The same expressions in softfloat: center = (max + min) * 0.5, then the delta moves.
    let t = tables().expect("GAME_DLL");
    let half = F32::from_bits(0x3F00_0000);
    let (min_x, min_y, max_x, max_y) = (
        lit("0.1", &t),
        lit("0.2", &t),
        lit("0.7", &t),
        lit("0.9", &t),
    );
    let cx = arith::mul(arith::add(max_x, min_x), half);
    let cy = arith::mul(arith::add(max_y, min_y), half);
    let dx = arith::sub(lit("0.3", &t), cx);
    let dy = arith::sub(lit("0.4", &t), cy);
    assert_eq!(real_bits(&host, "gCx"), cx.to_bits());
    assert_eq!(real_bits(&host, "gCy"), cy.to_bits());
    assert_eq!(real_bits(&host, "gMinX"), arith::add(min_x, dx).to_bits());
    assert_eq!(real_bits(&host, "gMinY"), arith::add(min_y, dy).to_bits());
    assert_eq!(real_bits(&host, "gMaxX"), arith::add(max_x, dx).to_bits());
    assert_eq!(real_bits(&host, "gMaxY"), arith::add(max_y, dy).to_bits());
}
