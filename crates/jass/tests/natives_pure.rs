//! Tests for the natives in `jass::natives::pure`, run through the VM with JASS programs.
//!
//! Each check compiles `set gV = <expr>` inside a function and reads `gV` back. The declarations
//! use the signatures of docs/spec/natives-1.26a.tsv.

use jass::compile::compile;
use jass::natives::register_pure;
use jass::vm::{NativeRegistry, Value, Vm};

const NATIVES: &str = r#"
native Deg2Rad takes real a returns real
native Rad2Deg takes real a returns real
native Sin takes real a returns real
native Cos takes real a returns real
native Tan takes real a returns real
native Asin takes real a returns real
native Acos takes real a returns real
native Atan takes real a returns real
native Atan2 takes real y, real x returns real
native SquareRoot takes real a returns real
native Pow takes real x, real y returns real
native I2R takes integer i returns real
native R2I takes real r returns integer
native I2S takes integer i returns string
native R2S takes real r returns string
native R2SW takes real r, integer width, integer precision returns string
native S2I takes string s returns integer
native S2R takes string s returns real
native SubString takes string s, integer start, integer end returns string
native StringLength takes string s returns integer
native StringCase takes string s, boolean upper returns string
"#;

fn registry() -> NativeRegistry {
    let mut r = NativeRegistry::new();
    register_pure(&mut r);
    r
}

/// Compiles `set gV = expr` in a function of type `ty` and runs it. Returns the VM so the caller
/// can read `gV` (and the string table).
fn eval(ty: &str, expr: &str) -> Vm {
    eval_with(ty, expr, "")
}

fn eval_with(ty: &str, expr: &str, extra: &str) -> Vm {
    let src = format!(
        "{NATIVES}\n{extra}\nglobals\n    {ty} gV\nendglobals\n\n\
         function Run takes nothing returns nothing\n    set gV = {expr}\nendfunction\n"
    );
    let prog = jass::parse(&src).unwrap_or_else(|e| panic!("parse failed: {e}\n{src}"));
    let module = compile(&[prog]).unwrap_or_else(|e| panic!("compile failed: {e:?}\n{src}"));
    let mut vm = Vm::new(module, registry()).unwrap_or_else(|e| panic!("vm: {e}"));
    vm.call("Run", &[])
        .unwrap_or_else(|e| panic!("run failed: {e:?} for {expr}"));
    vm
}

fn real(expr: &str) -> f32 {
    match eval("real", expr).global("gV") {
        Some(Value::Real(f)) => f,
        other => panic!("expected real for {expr}, got {other:?}"),
    }
}

fn int(expr: &str) -> i32 {
    match eval("integer", expr).global("gV") {
        Some(Value::Integer(i)) => i,
        other => panic!("expected integer for {expr}, got {other:?}"),
    }
}

fn string(expr: &str) -> String {
    let vm = eval("string", expr);
    match vm.global("gV") {
        Some(Value::String(id)) => vm.strings().get(id).to_string(),
        other => panic!("expected string for {expr}, got {other:?}"),
    }
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

// ------------------------------------------------------------------------------------------
// Math
// ------------------------------------------------------------------------------------------

#[test]
fn trig_and_roots_match_reference_values() {
    assert_eq!(real("Sin(0.0)"), 0.0);
    assert_eq!(real("Cos(0.0)"), 1.0);
    assert_eq!(real("Tan(0.0)"), 0.0);
    assert_eq!(real("SquareRoot(16.0)"), 4.0);
    assert_eq!(real("Pow(2.0, 10.0)"), 1024.0);
    assert!(close(
        real("Atan2(1.0, 1.0)"),
        std::f32::consts::FRAC_PI_4,
        1e-6
    ));
    assert!(close(real("Atan(1.0) * 4.0"), std::f32::consts::PI, 1e-5));
    assert!(close(real("Asin(1.0)"), std::f32::consts::FRAC_PI_2, 1e-6));
    assert_eq!(real("Acos(1.0)"), 0.0);
}

#[test]
fn inverse_trig_out_of_domain_is_nan() {
    assert!(real("Asin(2.0)").is_nan());
    assert!(real("Acos(-2.0)").is_nan());
}

#[test]
fn atan2_takes_y_first() {
    // Atan2(y, x): y = 1, x = 0 is pi/2; the swapped order would give 0.
    assert!(close(
        real("Atan2(1.0, 0.0)"),
        std::f32::consts::FRAC_PI_2,
        1e-6
    ));
}

#[test]
fn degree_radian_conversion() {
    assert!(close(real("Deg2Rad(180.0)"), std::f32::consts::PI, 1e-6));
    assert!(close(real("Rad2Deg(3.14159274)"), 180.0, 1e-4));
}

// ------------------------------------------------------------------------------------------
// Number conversion
// ------------------------------------------------------------------------------------------

#[test]
fn i2r_truncates_large_integers_instead_of_rounding() {
    // 16777219 lies between 16777218 and 16777220 (both exact f32); nearest-even picks ...220,
    // truncation picks ...218. The original's integer-to-float helper truncates.
    assert_eq!(real("I2R(16777219)"), 16_777_218.0);
    assert_eq!(real("I2R(-16777219)"), -16_777_218.0);
    assert_eq!(real("I2R(-7)"), -7.0);
    assert_eq!(real("I2R(0)"), 0.0);
}

#[test]
fn r2i_truncates_toward_zero() {
    assert_eq!(int("R2I(2.9)"), 2);
    assert_eq!(int("R2I(-2.9)"), -2);
    assert_eq!(int("R2I(0.99)"), 0);
}

#[test]
fn i2s_and_s2i() {
    assert_eq!(string("I2S(-42)"), "-42");
    assert_eq!(string("I2S(0)"), "0");
    assert_eq!(int("S2I(\"12abc\")"), 12);
    // Leading space gives 0 (the parser stops at the first character it does not accept).
    assert_eq!(int("S2I(\" 12\")"), 0);
    assert_eq!(int("S2I(\"\")"), 0);
}

#[test]
fn s2r_parses_leading_number() {
    assert_eq!(real("S2R(\"1.5\")"), 1.5);
    assert_eq!(real("S2R(\"2.5abc\")"), 2.5);
    assert_eq!(real("S2R(\"-2.25\")"), -2.25);
    assert_eq!(real("S2R(\"\")"), 0.0);
    assert_eq!(real("S2R(\" 3\")"), 0.0);
    assert_eq!(real("S2R(\"1.2.3\")"), 1.2);
}

#[test]
fn r2s_has_two_decimals() {
    assert_eq!(string("R2S(3.14159)"), "3.14");
    assert_eq!(string("R2S(-0.5)"), "-0.50");
    assert_eq!(string("R2S(10.0)"), "10.00");
}

#[test]
fn r2sw_width_and_precision() {
    assert_eq!(string("R2SW(3.14159, 8, 3)"), "   3.142");
    // A negative width left-justifies.
    assert_eq!(string("R2SW(1.5, -6, 1)"), "1.5   ");
    // A negative precision means 6 decimals.
    assert_eq!(string("R2SW(1.5, 0, -1)"), "1.500000");
}

// ------------------------------------------------------------------------------------------
// Strings
// ------------------------------------------------------------------------------------------

#[test]
fn substring_clamps_end_to_length() {
    assert_eq!(string("SubString(\"hello\", 1, 3)"), "el");
    assert_eq!(string("SubString(\"hello\", 2, 99)"), "llo");
    assert_eq!(string("SubString(\"hello\", 0, 5)"), "hello");
    assert_eq!(string("SubString(\"hello\", 5, 5)"), "");
}

#[test]
fn substring_end_before_start_gives_the_tail() {
    // Edge case from the decompile of the helper (end written as a terminator before the start).
    assert_eq!(string("SubString(\"hello\", 3, 2)"), "lo");
}

#[test]
fn substring_indices_are_16_bit() {
    // 65537 truncated to 16 bits is 1 and 65539 is 3.
    assert_eq!(string("SubString(\"hello\", 65537, 65539)"), "el");
}

#[test]
fn substring_past_the_end_is_empty() {
    assert_eq!(string("SubString(\"hello\", 6, 6)"), "");
    assert_eq!(string("SubString(\"\", 0, 0)"), "");
}

#[test]
fn string_length_counts_bytes() {
    assert_eq!(int("StringLength(\"\")"), 0);
    assert_eq!(int("StringLength(\"abc\")"), 3);
}

#[test]
fn string_case_converts_ascii_only() {
    assert_eq!(string("StringCase(\"MixEd 1\", true)"), "MIXED 1");
    assert_eq!(string("StringCase(\"MixEd 1\", false)"), "mixed 1");
}

// ------------------------------------------------------------------------------------------
// Convert* (integer to handle)
// ------------------------------------------------------------------------------------------

/// (native name, handle type) for the 44 `(I)H` Convert natives.
const CONVERTS: [(&str, &str); 44] = [
    ("ConvertRace", "race"),
    ("ConvertAllianceType", "alliancetype"),
    ("ConvertRacePref", "racepreference"),
    ("ConvertIGameState", "igamestate"),
    ("ConvertFGameState", "fgamestate"),
    ("ConvertPlayerState", "playerstate"),
    ("ConvertPlayerScore", "playerscore"),
    ("ConvertPlayerGameResult", "playergameresult"),
    ("ConvertUnitState", "unitstate"),
    ("ConvertAIDifficulty", "aidifficulty"),
    ("ConvertGameEvent", "gameevent"),
    ("ConvertPlayerEvent", "playerevent"),
    ("ConvertPlayerUnitEvent", "playerunitevent"),
    ("ConvertUnitEvent", "unitevent"),
    ("ConvertWidgetEvent", "widgetevent"),
    ("ConvertDialogEvent", "dialogevent"),
    ("ConvertLimitOp", "limitop"),
    ("ConvertUnitType", "unittype"),
    ("ConvertGameSpeed", "gamespeed"),
    ("ConvertPlacement", "placement"),
    ("ConvertStartLocPrio", "startlocprio"),
    ("ConvertGameDifficulty", "gamedifficulty"),
    ("ConvertGameType", "gametype"),
    ("ConvertMapFlag", "mapflag"),
    ("ConvertMapVisibility", "mapvisibility"),
    ("ConvertMapSetting", "mapsetting"),
    ("ConvertMapDensity", "mapdensity"),
    ("ConvertMapControl", "mapcontrol"),
    ("ConvertPlayerColor", "playercolor"),
    ("ConvertPlayerSlotState", "playerslotstate"),
    ("ConvertVolumeGroup", "volumegroup"),
    ("ConvertCameraField", "camerafield"),
    ("ConvertBlendMode", "blendmode"),
    ("ConvertRarityControl", "raritycontrol"),
    ("ConvertTexMapFlags", "texmapflags"),
    ("ConvertFogState", "fogstate"),
    ("ConvertEffectType", "effecttype"),
    ("ConvertVersion", "version"),
    ("ConvertItemType", "itemtype"),
    ("ConvertAttackType", "attacktype"),
    ("ConvertDamageType", "damagetype"),
    ("ConvertWeaponType", "weapontype"),
    ("ConvertSoundType", "soundtype"),
    ("ConvertPathingType", "pathingtype"),
];

#[test]
fn convert_natives_tag_the_integer_as_a_handle() {
    let mut decls = String::new();
    let mut body = String::new();
    for (i, (name, ty)) in CONVERTS.iter().enumerate() {
        decls.push_str(&format!("type {ty} extends handle\n"));
        decls.push_str(&format!("native {name} takes integer i returns {ty}\n"));
        body.push_str(&format!(
            "    set g{i} = {name}(7)\n    set z{i} = {name}(0)\n"
        ));
    }
    let mut globals = String::from("globals\n");
    for (i, (_, ty)) in CONVERTS.iter().enumerate() {
        globals.push_str(&format!("    {ty} g{i}\n    {ty} z{i}\n"));
    }
    globals.push_str("endglobals\n");
    let src = format!(
        "{decls}\n{globals}\nfunction Run takes nothing returns nothing\n{body}endfunction\n"
    );
    let prog = jass::parse(&src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let module = compile(&[prog]).unwrap_or_else(|e| panic!("compile failed: {e:?}"));
    let mut vm = Vm::new(module, registry()).expect("vm");
    vm.call("Run", &[]).expect("run");
    for (i, (name, _)) in CONVERTS.iter().enumerate() {
        assert_eq!(
            vm.global(&format!("g{i}")),
            Some(Value::Handle(7)),
            "{name}"
        );
        assert_eq!(
            vm.global(&format!("z{i}")),
            Some(Value::Handle(0)),
            "{name} of 0 is null"
        );
    }
}
