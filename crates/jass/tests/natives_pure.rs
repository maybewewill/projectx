//! Tests for the natives in `jass::natives::pure`, run through the VM with JASS programs.
//!
//! Each check compiles `set gV = <expr>` inside a function and reads `gV` back. The declarations
//! use the signatures of docs/spec/natives-1.26a.tsv.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
use std::sync::OnceLock;

use jass::compile::compile;
use jass::natives::register_pure;
use jass::vm::{NativeRegistry, RunStatus, Value, Vm};
use softfloat::{arith, conv, math, Tables, F32};

/// The soft-float lookup tables read from the user's own `Game.dll` (`GAME_DLL`), once per run.
/// `None` when `GAME_DLL` is not set.
fn tables() -> Option<Tables> {
    static TABLES: OnceLock<Option<Tables>> = OnceLock::new();
    TABLES
        .get_or_init(|| {
            let path = std::env::var("GAME_DLL").ok()?;
            let bytes =
                std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
            Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
        })
        .clone()
}

/// The tables for a test that compares against them, or `None` (with a note) if `GAME_DLL` is unset.
fn need_tables() -> Option<Tables> {
    let t = tables();
    if t.is_none() {
        eprintln!("GAME_DLL not set: skipping test that needs the soft-float lookup tables");
    }
    t
}

/// The soft-float value of an `f32` literal (its bit pattern).
fn sf(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// `R2I` of an `f32`, through the softfloat helper the native uses.
fn r2i_f(x: f32) -> i32 {
    conv::r2i(sf(x))
}

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
native StringHash takes string s returns integer
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
    let mut vm = Vm::new(module, registry(), tables()).unwrap_or_else(|e| panic!("vm: {e}"));
    vm.call("Run", &[])
        .unwrap_or_else(|e| panic!("run failed: {e:?} for {expr}"));
    vm
}

fn real(expr: &str) -> f32 {
    match eval("real", expr).global("gV") {
        Some(Value::Real(f)) => f.to_f32(),
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

/// The table-based helpers give approximations, so the expected bits come from the softfloat crate
/// (the same helper the native calls). The loose checks against the libm values only guard against
/// gross errors.
#[test]
fn trig_and_roots_match_softfloat() {
    let Some(t) = need_tables() else { return };
    let same = |expr: &str, expected: F32| {
        assert_eq!(real(expr).to_bits(), expected.to_bits(), "{expr}");
    };
    same("Sin(0.0)", math::sin(sf(0.0), &t));
    same("Cos(0.0)", math::cos(sf(0.0), &t));
    same("Tan(0.0)", math::tan(sf(0.0), &t));
    same("SquareRoot(16.0)", math::sqrt(sf(16.0), &t));
    same("Pow(2.0, 10.0)", math::pow(sf(2.0), sf(10.0), &t));
    same("Atan2(1.0, 1.0)", math::atan2(sf(1.0), sf(1.0), &t));
    same("Atan(1.0)", math::atan(sf(1.0), &t));
    same("Asin(1.0)", math::asin(sf(1.0), &t));
    same("Acos(1.0)", math::acos(sf(1.0), &t));
    assert!(close(real("Sin(1.0)"), 1.0f32.sin(), 1e-5));
    assert!(close(real("Cos(1.0)"), 1.0f32.cos(), 1e-5));
    assert!(close(real("SquareRoot(16.0)"), 4.0, 1e-6));
    assert!(close(real("Atan2(1.0, 1.0)"), FRAC_PI_4, 1e-5));
    assert!(close(real("Atan(1.0) * 4.0"), PI, 1e-4));
    assert!(close(real("Asin(1.0)"), FRAC_PI_2, 1e-5));
}

#[test]
fn inverse_trig_out_of_domain_follows_the_helpers() {
    // The wrappers' argument checks are not ported (spec softfloat, open items), so the native
    // returns the helper's value for |x| > 1. This pins that choice, not the original's wrapper.
    let Some(t) = need_tables() else { return };
    assert_eq!(
        real("Asin(2.0)").to_bits(),
        math::asin(sf(2.0), &t).to_bits()
    );
    assert_eq!(
        real("Acos(-2.0)").to_bits(),
        math::acos(sf(-2.0), &t).to_bits()
    );
}

#[test]
fn atan2_takes_y_first() {
    let Some(t) = need_tables() else { return };
    // Atan2(y, x): y = 1, x = 0 is pi/2; the swapped order would give 0.
    assert_eq!(
        real("Atan2(1.0, 0.0)").to_bits(),
        math::atan2(sf(1.0), sf(0.0), &t).to_bits()
    );
    assert!(close(real("Atan2(1.0, 0.0)"), FRAC_PI_2, 1e-5));
}

#[test]
fn table_natives_stop_the_thread_without_tables() {
    // Runs without GAME_DLL: no table, no IEEE fallback, an explicit status.
    for expr in [
        "Sin(1.0)",
        "SquareRoot(4.0)",
        "Pow(2.0, 3.0)",
        "Atan2(1.0, 1.0)",
    ] {
        let src = format!(
            "{NATIVES}\nglobals\n    real gV\nendglobals\n\n\
             function Run takes nothing returns nothing\n    set gV = {expr}\nendfunction\n"
        );
        let prog = jass::parse(&src).unwrap_or_else(|e| panic!("parse failed: {e}"));
        let module = compile(&[prog]).unwrap_or_else(|e| panic!("compile failed: {e:?}"));
        let mut vm = Vm::new(module, registry(), None).expect("vm");
        assert_eq!(vm.call("Run", &[]), Err(RunStatus::MissingTables), "{expr}");
    }
}

#[test]
fn degree_radian_conversion() {
    // Multiplies by the stored constants (spec softfloat 6) with the soft-float multiply.
    assert_eq!(
        real("Deg2Rad(180.0)").to_bits(),
        arith::mul(sf(180.0), F32::from_bits(0x3C8E_FA35)).to_bits()
    );
    assert!(close(real("Deg2Rad(180.0)"), PI, 1e-6));
    assert_eq!(
        real("Rad2Deg(3.14159274)").to_bits(),
        arith::mul(sf(PI), F32::from_bits(0x4265_2EE1)).to_bits()
    );
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
    // atoi skips leading white space (CONFIRMED that the native calls atoi; LIKELY for the rule).
    assert_eq!(int("S2I(\" 12\")"), 12);
    assert_eq!(int("S2I(\"\")"), 0);
    assert_eq!(int("S2I(\"+7\")"), 7);
    assert_eq!(int("S2I(\"-7x\")"), -7);
    assert_eq!(int("S2I(\"1.9\")"), 1);
}

/// S2R results are the original's bits (recorded in the emulator): "1.5" is one ulp above 1.5.
#[test]
fn s2r_parses_leading_number() {
    let Some(_) = need_tables() else { return };
    assert_eq!(real("S2R(\"1.5\")").to_bits(), 0x3FC0_0001);
    assert_eq!(real("S2R(\"2.5abc\")").to_bits(), 0x4020_0000);
    assert_eq!(real("S2R(\"-2.25\")").to_bits(), 0xC010_0000);
    assert_eq!(real("S2R(\"\")").to_bits(), 0);
    assert_eq!(real("S2R(\" 3\")").to_bits(), 0);
    assert_eq!(real("S2R(\"1.2.3\")").to_bits(), 0x3F99_999A);
}

/// R2S prints three decimals (the helper is called with precision 3, see docs/spec/softfloat.md).
#[test]
fn r2s_has_three_decimals() {
    assert_eq!(string("R2S(3.14159)"), "3.142");
    assert_eq!(string("R2S(-0.5)"), "-0.500");
    assert_eq!(string("R2S(10.0)"), "10.000");
}

#[test]
fn r2sw_width_and_precision() {
    assert_eq!(string("R2SW(3.14159, 8, 3)"), "    3.142");
    // The integer part is padded to `width - precision` columns, at least one. A negative width
    // gives less than one column, so no padding.
    assert_eq!(string("R2SW(1.5, -6, 1)"), "1.5");
    // A negative precision means 6 decimals with trailing zeros removed (one kept).
    assert_eq!(string("R2SW(1.5, 0, -1)"), "1.5");
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
    let mut vm = Vm::new(module, registry(), tables()).expect("vm");
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

// ------------------------------------------------------------------------------------------
// Checked against Game.dll 1.26a in Unicorn (tools/emu/verify_strings.py)
// ------------------------------------------------------------------------------------------

/// (f32 bits, R2I result) observed from the original's float-to-int helper 0x6f6eec70.
const R2I_PAIRS: [(u32, i32); 33] = [
    (0x3f800000, 1),
    (0xbf800000, -1),
    (0x3f7fffff, 0),
    (0x3fc00000, 1),
    (0xbfc00000, -1),
    (0x3f000000, 0),
    (0x40000000, 2),
    (0x41200000, 10),
    (0xc2c80000, -100),
    (0x42c80000, 100),
    (0x4affffff, 8388607),
    (0x4b000000, 8388608),
    (0x4effffff, 2147483520),
    (0x4f000000, 2147483647),
    (0xceffffff, -2147483520),
    (0xcf000000, -2147483648),
    (0x7f800000, 2147483647),
    (0xff800000, -2147483648),
    (0x7fc00000, 2147483647),
    (0xffc00000, -2147483648),
    (0x00000001, 0),
    (0x80000000, 0),
    (0x007fffff, 0),
    (0x7f7fffff, 2147483647),
    (0xff7fffff, -2147483648),
    (0x46fffe00, 32767),
    (0x4e7fffff, 1073741760),
    (0xc6fffe00, -32767),
    (0x3effffff, 0),
    (0xbeffffff, 0),
    (0x4000b333, 2),
    (0xc0490fdb, -3),
    (0x5d5e0b6b, 2147483647),
];

#[test]
fn r2i_matches_original_pairs() {
    for (bits, expected) in R2I_PAIRS {
        let got = r2i_f(f32::from_bits(bits));
        assert_eq!(got, expected, "R2I of {bits:#010x}");
    }
}

#[test]
fn r2i_truncates_and_saturates() {
    assert_eq!(r2i_f(-0.5), 0);
    assert_eq!(r2i_f(1.999), 1);
    // Above 2^31 saturates by sign, and NaN follows its sign bit (not 0 as with Rust's `as`).
    assert_eq!(r2i_f(3.0e9), i32::MAX);
    assert_eq!(r2i_f(-3.0e9), i32::MIN);
    assert_eq!(r2i_f(f32::NAN), i32::MAX);
    assert_eq!(r2i_f(-f32::NAN), i32::MIN);
}

/// (name, hash type, hash) observed from Game.dll's MPQ-style hash at 0x6f68cc10 (crypt table
/// from 0x6f68cb00). Names are ASCII; the native applies it with type 0.
const HASH_PAIRS: [(&str, u32, u32); 30] = [
    ("a", 0, 0x224408eb),
    ("A", 0, 0x224408eb),
    ("abc", 0, 0xf434f751),
    ("abc", 1, 0xaab2c1a5),
    ("abc", 2, 0x34300855),
    ("abc", 3, 0x4e203978),
    ("Abc", 0, 0xf434f751),
    ("ABC", 0, 0xf434f751),
    ("units\\human", 0, 0x175717f6),
    ("units\\human", 1, 0x536846a8),
    ("units\\human", 2, 0x7d70ac67),
    ("units\\human", 3, 0x5abbeefe),
    ("units/human", 0, 0x175717f6),
    ("Units\\Human", 0, 0x175717f6),
    ("Hello World", 0, 0xfeb41edd),
    ("Hello World", 1, 0xef7b1bdb),
    ("Hello World", 2, 0x4ba6f4b8),
    ("Hello World", 3, 0x59c90699),
    ("TRIGSTR_001", 0, 0x1fa3f8c7),
    ("CAbilityBlizzard", 0, 0xc7bedb16),
    ("war3map.j", 0, 0x0cca3be6),
    ("ab/cd", 0, 0x77e9543e),
    ("ab\\cd", 0, 0x77e9543e),
    ("0123456789", 0, 0x78408e49),
    ("!@#$%^&*()", 0, 0xd42a0799),
    ("_", 0, 0xe225441e),
    ("z", 0, 0x6d747097),
    ("Z", 0, 0x6d747097),
    ("{}[]", 0, 0xf9d8fb15),
    ("AbAbAbAbAbAbAbAbAbAbAbAbAbAbAbAbAb", 0, 0xfd8009fe),
];

#[test]
fn mpq_hash_matches_game_dll_pairs() {
    for (name, ty, expected) in HASH_PAIRS {
        assert_eq!(
            jass::natives::pure::mpq_hash(name.as_bytes(), ty),
            expected,
            "hash of {name:?} type {ty}"
        );
    }
}

#[test]
fn string_hash_native_rules() {
    // Empty string is 0 by the wrapper, not the hash of "".
    assert_eq!(int("StringHash(\"\")"), 0);
    // Case-insensitive, and the hash is the signed 32-bit value of seed1 (0xf434f751 is negative).
    assert_eq!(int("StringHash(\"abc\")"), 0xf434_f751_u32 as i32);
    assert_eq!(int("StringHash(\"ABC\")"), int("StringHash(\"abc\")"));
}

#[test]
fn string_case_keeps_at_most_1023_bytes() {
    let long = "a".repeat(2000);
    let out = jass::natives::pure::string_case(&long, true);
    assert_eq!(out.len(), 1023);
    assert!(out.bytes().all(|b| b == b'A'));
    assert_eq!(
        jass::natives::pure::string_case("ab\u{e9}", true),
        "AB\u{e9}"
    );
}

#[test]
fn atoi_rules() {
    use jass::natives::pure::atoi;
    assert_eq!(atoi("  \t-42x"), -42);
    assert_eq!(atoi("+"), 0);
    assert_eq!(atoi("2147483647"), i32::MAX);
    assert_eq!(atoi("-2147483648"), i32::MIN);
    assert_eq!(atoi("0x1F"), 0);
}

// ------------------------------------------------------------------------------------------
// Text conversions: R2S, R2SW and S2R (softfloat::text, checked against Game.dll in Unicorn)
// ------------------------------------------------------------------------------------------

/// Values the original prints (recorded with tools/emu/text_vectors.py). They need no table.
#[test]
fn r2s_r2sw_match_the_original() {
    assert_eq!(string("R2S(1.5)"), "1.500");
    assert_eq!(string("R2S(-3.25)"), "-3.250");
    assert_eq!(string("R2S(0.0)"), "0.000");
    assert_eq!(string("R2S(-0.0)"), "0.000");
    assert_eq!(string("R2S(0.0049999)"), "0.005");
    assert_eq!(string("R2SW(1.0, 10, 2)"), "       1.00");
    assert_eq!(string("R2SW(-1.5, 10, 2)"), "-       1.50");
    assert_eq!(string("R2SW(1.5, 0, 0)"), "2.0");
    assert_eq!(string("R2SW(2.5, 0, -1)"), "2.5");
    assert_eq!(string("R2SW(1.5, -6, -1)"), "1.5");
}

#[test]
fn s2r_matches_the_original() {
    let Some(_) = need_tables() else { return };
    // "1.5" is one ulp above 1.5: the original divides 15 by 10 with the soft DIV.
    assert_eq!(real("S2R(\"1.5\")").to_bits(), 0x3FC0_0001);
    assert_eq!(real("S2R(\"-3.25\")").to_bits(), 0xC050_0000);
    assert_eq!(real("S2R(\"1000000\")").to_bits(), 0x4974_2400);
    assert_eq!(real("S2R(\"0.1\")").to_bits(), 0x3DCC_CCCE);
    assert_eq!(real("S2R(\"12abc\")").to_bits(), 0x4140_0000);
    // Empty text gives 0.0 without the parser.
    assert_eq!(real("S2R(\"\")").to_bits(), 0);
}
