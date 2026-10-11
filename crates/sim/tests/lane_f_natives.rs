//! Lane F natives through `ScriptHost` (`natives/lane_f.rs`, `docs/spec/natives-lane-f.md`): unit food use, corpses and
//! decay, build orders, the hotkey lookup, the sound wait, and the rescue and sell event getters. Synthetic data only.
//! Tests that need the soft-float tables return early without `GAME_DLL`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{UnitEvent, UnitType};
use sim::natives::lane_f::{c_strtol, parse_hotkey_text, sound_wait_seconds, LaneFEvent};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const FOOTMAN: u32 = 0x6866_6F6F; // 'hfoo'
const PEASANT: u32 = 0x6865_7065; // 'hepe'
const BARRACKS: u32 = 0x6862_6172; // 'hbar'
const AHBU: u32 = 0x4148_6275; // 'AHbu'

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type sound extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateCorpse takes player whichPlayer, integer unitid, real x, real y, real face returns unit
native RemoveUnit takes unit whichUnit returns nothing
native SetUnitOwner takes unit whichUnit, player whichPlayer, boolean changeColor returns nothing
native SetUnitUseFood takes unit whichUnit, boolean useFood returns nothing
native UnitSuspendDecay takes unit whichUnit, boolean suspend returns nothing
native GetUnitState takes unit whichUnit, integer whichUnitState returns real
native IsUnitType takes unit whichUnit, integer whichUnitType returns boolean
native IssueBuildOrderById takes unit whichPeon, integer unitId, real x, real y returns boolean
native IssueBuildOrder takes unit whichPeon, string unitToBuild, real x, real y returns boolean
native GetLocalizedHotkey takes string source returns integer
native GetHandleId takes handle h returns integer
native CreateSound takes string fileName, boolean looping, boolean is3D, boolean stopWhenOutOfRange, integer fadeInRate, integer fadeOutRate, string eaxSetting returns sound
native SetSoundDuration takes sound soundHandle, integer duration returns nothing
native TriggerWaitForSound takes sound soundHandle, real offset returns nothing
native TriggerSleepAction takes real timeout returns nothing
"#;

const GLOBALS: &str = r#"
globals
unit u1
unit u2
sound snd
integer i1 = -1
integer i2 = -1
integer i3 = -1
integer i4 = -1
boolean b1 = false
boolean b2 = false
boolean b3 = false
real r1 = 0.0
endglobals
"#;

fn host_for(body: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{GLOBALS}\n{body}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        let ty = |name: &str, used: i32, made: i32, abilities: Vec<u32>| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x4416_0000),
            food_used: used,
            food_made: made,
            abilities,
            ..Default::default()
        };
        g.unit_types_mut().insert(FOOTMAN, ty("Footman", 2, 0, vec![]));
        g.unit_types_mut().insert(PEASANT, ty("Peasant", 1, 0, vec![AHBU]));
        g.unit_types_mut().insert(BARRACKS, ty("Barracks", 0, 0, vec![]));
    }
    host
}

fn run(host: &mut ScriptHost) {
    host.start("main", &[]).expect("main starts");
    for _ in 0..6 {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm().global(name).and_then(|v| v.as_integer()).unwrap_or_else(|| panic!("{name} is not an integer"))
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm().global(name).and_then(|v| v.as_boolean()).unwrap_or_else(|| panic!("{name} is not a boolean"))
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables"))
}

#[test]
fn strtol_follows_the_c_rules() {
    assert_eq!(c_strtol(b"65"), 65);
    assert_eq!(c_strtol(b"  -12x"), -12);
    assert_eq!(c_strtol(b"0x41"), 0x41);
    assert_eq!(c_strtol(b"0X7f"), 0x7F);
    assert_eq!(c_strtol(b"010"), 8, "a leading zero is octal");
    assert_eq!(c_strtol(b"089"), 0, "8 is not an octal digit");
    assert_eq!(c_strtol(b"0x"), 0, "no hex digits: just the zero");
    assert_eq!(c_strtol(b""), 0);
    assert_eq!(c_strtol(b"abc"), 0);
    assert_eq!(c_strtol(b"99999999999"), i32::MAX, "saturates");
    assert_eq!(c_strtol(b"-99999999999"), i32::MIN);
    assert_eq!(c_strtol(b"2147483647"), i32::MAX);
    assert_eq!(c_strtol(b"-2147483648"), i32::MIN);
}

#[test]
fn hotkey_text_is_a_quoted_code_or_a_number() {
    assert_eq!(parse_hotkey_text("'Q'"), 0x51);
    assert_eq!(parse_hotkey_text("'hfoo'"), 0x6866_6F6F);
    assert_eq!(parse_hotkey_text("'ab"), 0x6162, "stops at the end of the text");
    assert_eq!(parse_hotkey_text("'abcdef'"), 0x6162_6364, "four characters at most");
    assert_eq!(parse_hotkey_text("81"), 81);
    assert_eq!(parse_hotkey_text("0x51"), 0x51);
}

/// The quoted branch of `0x6F4CE050` against 300 Unicorn runs of the original (`tools/emu/hotkey_text_emu.py`).
#[test]
fn quoted_hotkey_text_matches_the_original_instructions() {
    let vectors = include_str!("hotkey_text_vectors.txt");
    let mut n = 0;
    for line in vectors.lines().filter(|l| !l.trim().is_empty()) {
        let (hex, want) = line.split_once(' ').expect("two fields");
        let bytes: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        let text = String::from_utf8(bytes).unwrap();
        let want = u32::from_str_radix(want, 16).unwrap() as i32;
        assert_eq!(parse_hotkey_text(&text), want, "text {text:?}");
        n += 1;
    }
    assert_eq!(n, 300);
}

#[test]
fn hotkey_native_reads_the_first_value_of_the_key() {
    let body = r#"
function main takes nothing returns nothing
    set i1 = GetLocalizedHotkey("CMDKEY_ATTACK")
    set i2 = GetLocalizedHotkey("cmdkey_move")
    set i3 = GetLocalizedHotkey("")
    set i4 = GetLocalizedHotkey("MISSING")
endfunction
"#;
    let mut host = host_for(body);
    host.game_mut().set_hotkey("CMDKEY_ATTACK", &["'A'", "'B'"]);
    host.game_mut().set_hotkey("cmdkey_Move", &["0x4D"]);
    run(&mut host);
    assert_eq!(int(&host, "i1"), 0x41, "first value only");
    assert_eq!(int(&host, "i2"), 0x4D, "keys match without case");
    assert_eq!(int(&host, "i3"), 0, "an empty string gives 0");
    assert_eq!(int(&host, "i4"), 0, "a missing key gives 0");
}

/// Runs `calls` (JASS statements) after a footman (food 2) is created for player 0, and returns the food used of
/// player 0 and player 1 afterwards.
fn food_after(calls: &str) -> (i32, i32) {
    let script = format!(
        "function main takes nothing returns nothing\n    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))\n{calls}\nendfunction\n"
    );
    let mut host = host_for(&script);
    run(&mut host);
    let g = host.game();
    (g.food_used_of(0), g.food_used_of(1))
}

#[test]
fn creation_applies_the_food_and_set_unit_use_food_toggles_it() {
    assert_eq!(food_after("").0, 2, "CreateUnit applies the food used (flags 0x502)");
    assert_eq!(food_after("call SetUnitUseFood(u1, false)").0, 0, "false takes it back");
    assert_eq!(
        food_after("call SetUnitUseFood(u1, false)\ncall SetUnitUseFood(u1, false)").0,
        0,
        "a second false takes nothing more"
    );
    assert_eq!(
        food_after("call SetUnitUseFood(u1, true)").0,
        2,
        "true on an applied unit adds nothing"
    );
    assert_eq!(
        food_after("call SetUnitUseFood(u1, false)\ncall SetUnitUseFood(u1, true)\ncall SetUnitUseFood(u1, true)").0,
        2,
        "false then true twice leaves it applied once"
    );
    assert_eq!(food_after("call RemoveUnit(u1)").0, 0, "RemoveUnit releases it");
    assert_eq!(
        food_after("call SetUnitUseFood(u1, false)\ncall RemoveUnit(u1)").0,
        0,
        "a unit whose food was taken back releases nothing more"
    );
}

#[test]
fn owner_change_moves_the_applied_food() {
    let script = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitOwner(u1, Player(1), true)
endfunction
"#;
    let mut host = host_for(script);
    run(&mut host);
    assert_eq!(host.game().food_used_of(0), 0);
    assert_eq!(host.game().food_used_of(1), 2);
    let peak = host.game().lane.stats.get(&(1, sim::natives::lane_e::stat::FOOD_MAX_USED)).copied();
    assert_eq!(peak, Some(2), "the peak word follows the counter");
}

#[test]
fn corpse_is_a_stopped_dead_unit_with_the_confirmed_notices() {
    let script = r#"
function main takes nothing returns nothing
    set u1 = CreateCorpse(Player(0), 'hfoo', I2R(5), I2R(6), I2R(90))
    set i1 = GetHandleId(u1)
    set r1 = GetUnitState(u1, 0)
    call UnitSuspendDecay(u1, true)
    call UnitSuspendDecay(u1, false)
    set u2 = CreateCorpse(Player(0), 'zzzz', I2R(5), I2R(6), I2R(90))
    set b1 = u2 == null
    set u2 = CreateCorpse(null, 'hfoo', I2R(5), I2R(6), I2R(90))
    set b2 = u2 == null
endfunction
"#;
    let mut host = host_for(script);
    run(&mut host);
    assert!(int(&host, "i1") >= 0x10_0001, "the handle is allocated");
    assert!(boolean(&host, "b1"), "an unknown type gives null");
    assert!(boolean(&host, "b2"), "a null player gives null");
    let g = host.game();
    let ids: Vec<u32> = g
        .unit_events()
        .iter()
        .filter_map(|e| match e {
            UnitEvent::Notify { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    // Teardown notices, 0xD015A, then the resume pair of UnitSuspendDecay(false).
    assert_eq!(ids, vec![0xD_0164, 0xD_0178, 0xD_0144, 0xD_015A, 0xD_015A, 0xD_0159]);
    let corpse = g.lane_f.corpses.values().next().copied().expect("one corpse");
    assert_eq!(corpse.pathing_off, 1);
    assert!(!corpse.decay_suspended, "the last argument was false");
    let stops = g
        .lane_f
        .events
        .iter()
        .filter(|e| matches!(e, LaneFEvent::DecayStop { .. }))
        .count();
    assert_eq!(stops, 2);
}

#[test]
fn suspend_decay_ignores_a_living_unit() {
    let script = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitSuspendDecay(u1, false)
endfunction
"#;
    let mut host = host_for(script);
    run(&mut host);
    assert!(host.game().lane_f.events.is_empty());
}

#[test]
fn build_order_needs_a_build_ability_and_a_placement() {
    let script = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hepe', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set b1 = IssueBuildOrderById(u1, 'hbar', I2R(256), I2R(256))
    set b2 = IssueBuildOrderById(u2, 'hbar', I2R(256), I2R(256))
    set b3 = IssueBuildOrderById(u1, 'zzzz', I2R(256), I2R(256))
    set i1 = 0
    if IssueBuildOrder(u1, "Barracks", I2R(10), I2R(20)) then
        set i1 = 1
    endif
    if IssueBuildOrder(u1, "Nothing", I2R(10), I2R(20)) then
        set i2 = 1
    else
        set i2 = 0
    endif
endfunction
"#;
    let mut host = host_for(script);
    run(&mut host);
    assert!(boolean(&host, "b1"), "a peasant with a build ability accepts the order");
    assert!(!boolean(&host, "b2"), "a footman has no build ability: the acceptance refuses");
    assert!(!boolean(&host, "b3"), "an unknown type fails the placement");
    assert_eq!(int(&host, "i1"), 1, "IssueBuildOrder resolves the name");
    assert_eq!(int(&host, "i2"), 0, "an unknown name gives false");
    let g = host.game();
    let built: Vec<u32> = g
        .lane_f
        .events
        .iter()
        .filter_map(|e| match e {
            LaneFEvent::BuildOrder { type_id, .. } => Some(*type_id),
            _ => None,
        })
        .collect();
    assert_eq!(built, vec![BARRACKS, BARRACKS]);
    assert_eq!(g.orders().issued().len(), 2);
    assert_eq!(g.orders().issued()[0].order.id, BARRACKS, "the order id is the unit type id");
}

#[test]
fn placement_hook_decides_with_the_confirmed_accept_codes() {
    let script = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hepe', I2R(0), I2R(0), I2R(0))
    set b1 = IssueBuildOrderById(u1, 'hbar', I2R(1), I2R(1))
    set b2 = IssueBuildOrderById(u1, 'hbar', I2R(2), I2R(2))
    set b3 = IssueBuildOrderById(u1, 'hbar', I2R(3), I2R(3))
endfunction
"#;
    let mut host = host_for(script);
    host.game_mut().set_placement_validator(Some(|_, _, _, x, _| {
        // x = 1 -> 0x45 (blockers will move), x = 2 -> 68 (refused), anything else -> 0.
        match x.to_bits() {
            0x3F80_0000 => 0x45,
            0x4000_0000 => 68,
            _ => 0,
        }
    }));
    run(&mut host);
    assert!(boolean(&host, "b1"), "0x45 lets the order go on");
    assert!(!boolean(&host, "b2"), "any other non-zero code refuses it");
    assert!(boolean(&host, "b3"));
}

const SOUND_SCRIPT: &str = r#"
function main takes nothing returns nothing
    set snd = CreateSound("x.wav", false, false, false, 0, 0, "")
    call SetSoundDuration(snd, 5000)
    set i1 = GetHandleId(snd)
endfunction
"#;

#[test]
fn sound_wait_is_total_minus_offset_minus_elapsed() {
    let Some(t) = tables() else { return };
    let mut host = host_for(SOUND_SCRIPT);
    host.game_mut().set_tables(Some(t));
    run(&mut host);
    let h = int(&host, "i1") as u32;
    let mut g = host.game_mut();
    // A sound that is not playing counts as fully played: elapsed = T, so the wait is -offset, floored at 0.
    let w = sound_wait_seconds(&mut g, h, F32::ZERO);
    assert_eq!(w.to_bits(), 0, "T - 0 - T is zero");
    let w = sound_wait_seconds(&mut g, h, F32::from_bits(0x3F80_0000)); // offset 1.0
    assert_eq!(w.to_bits(), 0, "elapsed T is above T - 1.0: no wait");
    let w = sound_wait_seconds(&mut g, h, F32::from_bits(0xBF80_0000)); // offset -1.0
    assert_eq!(w.to_f32(), 1.0, "a negative offset waits for -offset");
    // A bad handle sleeps the offset (the TriggerSleepAction path).
    let w = sound_wait_seconds(&mut g, 0xDEAD, F32::from_bits(0x4000_0000));
    assert_eq!(w.to_f32(), 2.0);
    let w = sound_wait_seconds(&mut g, 0xDEAD, F32::from_bits(0xC000_0000));
    assert_eq!(w.to_bits(), 0, "a negative sleep is clamped");
}

#[test]
fn sound_wait_without_tables_is_the_minimum() {
    let mut host = host_for(SOUND_SCRIPT);
    run(&mut host);
    let h = int(&host, "i1") as u32;
    let mut g = host.game_mut();
    assert_eq!(sound_wait_seconds(&mut g, h, F32::from_bits(0x3F80_0000)).to_bits(), 0);
}

#[test]
fn trigger_wait_for_sound_sleeps_the_calling_thread() {
    let script = r#"
function main takes nothing returns nothing
    set snd = CreateSound("x.wav", false, false, false, 0, 0, "")
    call SetSoundDuration(snd, 5000)
    call TriggerWaitForSound(snd, 0.5)
    set i1 = 7
endfunction
"#;
    let mut host = host_for(script);
    host.start("main", &[]).expect("main starts");
    for _ in 0..4 {
        host.step();
    }
    assert!(host.misfires().is_empty());
    // Without tables the wait is the 1e-4 minimum, so the thread resumes after a few steps.
    assert_eq!(int(&host, "i1"), 7);
}
