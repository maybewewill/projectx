//! Hero experience, level, skill point and attribute natives through `ScriptHost` (`natives/hero.rs`, arithmetic in
//! `heroes.rs`). The thresholds are installed directly (`XpTables::from_need_list`), so the test does not need
//! `GAME_DLL`: NeedHeroXP = 200, 400, 600 gives a maximum level of 3. Expected values are worked out by hand from
//! `docs/spec/heroes.md`, sections 3 and 4.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::heroes::{HeroAttributes, XpTables};
use sim::natives::game::UnitType;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type unit extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetHeroLevel takes unit whichHero returns integer
native GetHeroXP takes unit whichHero returns integer
native GetHeroSkillPoints takes unit whichHero returns integer
native AddHeroXP takes unit whichHero, integer xp, boolean showEyeCandy returns nothing
native SetHeroXP takes unit whichHero, integer xp, boolean showEyeCandy returns nothing
native SetHeroLevel takes unit whichHero, integer level, boolean showEyeCandy returns nothing
native UnitStripHeroLevel takes unit whichHero, integer howMany returns boolean
native UnitModifySkillPoints takes unit whichHero, integer skillPointDelta returns boolean
native GetHeroStr takes unit whichHero, boolean includeBonuses returns integer
native SetHeroStr takes unit whichHero, integer newStr, boolean permanent returns nothing
native GetHeroInt takes unit whichHero, boolean includeBonuses returns integer
native SetHeroInt takes unit whichHero, integer newInt, boolean permanent returns nothing
"#;

const HERO: u32 = 0x4870_616C; // 'Hpal'

const SCRIPT: &str = r#"
globals
unit h1
integer r1 = -9
integer r2 = -9
integer r3 = -9
boolean rb = false
endglobals

function main takes nothing returns nothing
    set h1 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
endfunction

function read takes nothing returns nothing
    set r1 = GetHeroLevel(h1)
    set r2 = GetHeroXP(h1)
    set r3 = GetHeroSkillPoints(h1)
endfunction

function add450 takes nothing returns nothing
    call AddHeroXP(h1, 450, true)
endfunction

function strip1 takes nothing returns nothing
    set rb = UnitStripHeroLevel(h1, 1)
endfunction

function setxp100 takes nothing returns nothing
    call SetHeroXP(h1, 100, true)
endfunction

function setlevel3 takes nothing returns nothing
    call SetHeroLevel(h1, 3, true)
endfunction

function skill_minus5 takes nothing returns nothing
    set rb = UnitModifySkillPoints(h1, -5)
endfunction

function read_str takes nothing returns nothing
    set r1 = GetHeroStr(h1, true)
endfunction

function set_str30 takes nothing returns nothing
    call SetHeroStr(h1, 30, true)
endfunction

function read_int takes nothing returns nothing
    set r1 = GetHeroInt(h1, true)
endfunction

function set_int40 takes nothing returns nothing
    call SetHeroInt(h1, 40, true)
endfunction
"#;

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SCRIPT}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            HERO,
            UnitType {
                name: "Paladin".to_string(),
                max_life: F32::from_bits(0x4416_0000),
                ..Default::default()
            },
        );
        g.set_hero_xp(XpTables::from_need_list(&[200, 400, 600]));
        g.set_hero_attributes(
            HERO,
            HeroAttributes {
                str_base: 20,
                str_plus: F32::from_bits(2.5f32.to_bits()),
                int_base: 12,
                int_plus: F32::from_bits(1.5f32.to_bits()),
                ..HeroAttributes::default()
            },
        );
    }
    host
}

fn call(host: &mut ScriptHost, name: &str) {
    host.start(name, &[]).expect("function starts");
    for _ in 0..4 {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| v.as_boolean())
        .unwrap_or_else(|| panic!("global {name} is not a boolean"))
}

/// (level, xp, skill points) of the hero.
fn state(host: &mut ScriptHost) -> (i32, i32, i32) {
    call(host, "read");
    (int(host, "r1"), int(host, "r2"), int(host, "r3"))
}

#[test]
fn experience_levels_and_skill_points_follow_the_thresholds() {
    let mut host = host();
    call(&mut host, "main");
    assert_eq!(state(&mut host), (1, 0, 0));

    // 450 XP passes the thresholds of levels 2 and 3. Level 3 is the maximum, so the XP is clamped to 600.
    call(&mut host, "add450");
    assert_eq!(state(&mut host), (3, 600, 2));

    // One level comes off: level 2, XP = the threshold of level 2 (200, the first need entry), one skill point is spent.
    call(&mut host, "strip1");
    assert!(boolean(&host, "rb"));
    assert_eq!(state(&mut host), (2, 200, 1));

    // A lower target does nothing.
    call(&mut host, "setxp100");
    assert_eq!(state(&mut host), (2, 200, 1));

    // SetHeroLevel(3): the target is need(2) = 400, the difference 200 is added; the hero reaches level 3, gains a
    // skill point and the XP is clamped at the maximum (600).
    call(&mut host, "setlevel3");
    assert_eq!(state(&mut host), (3, 600, 2));

    // Skill points cannot go below zero.
    call(&mut host, "skill_minus5");
    assert!(boolean(&host, "rb"));
    assert_eq!(state(&mut host), (3, 600, 0));
}

#[test]
fn strength_uses_the_base_and_the_gain_and_the_setter_moves_the_base() {
    let mut host = host();
    call(&mut host, "main");
    call(&mut host, "add450");
    // Level 3, counter 2: base 20 + trunc(2.5 * 2) = 25.
    call(&mut host, "read_str");
    assert_eq!(int(&host, "r1"), 25);
    call(&mut host, "set_str30");
    call(&mut host, "read_str");
    assert_eq!(int(&host, "r1"), 30);
}

#[test]
fn intelligence_uses_the_base_and_the_gain_and_the_setter_moves_the_base() {
    let mut host = host();
    call(&mut host, "main");
    call(&mut host, "add450");
    // Level 3, counter 2: base 12 + trunc(1.5 * 2) = 15.
    call(&mut host, "read_int");
    assert_eq!(int(&host, "r1"), 15);
    call(&mut host, "set_int40");
    call(&mut host, "read_int");
    assert_eq!(int(&host, "r1"), 40);
}
