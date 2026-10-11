//! Hero natives through `ScriptHost` (`natives/hero.rs`, the model in `natives/game.rs`, [`HeroObj`]).
//!
//! The level-up, experience and stat setters are not implemented yet, so the hero state is set on the model between
//! two script entry points: `main` creates the units, and the other functions read or change them through the
//! natives. The hero gate (hero type and not an illusion) is checked for a hero, a non-hero and an illusion.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ObjKey, UnitType};
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
native SuspendHeroXP takes unit whichHero, boolean flag returns nothing
"#;

const FOOTMAN: u32 = 0x6866_6F6F; // 'hfoo'
const HERO: u32 = 0x4870_616C; // 'Hpal'

const SCRIPT: &str = r#"
globals
unit h1
unit f1
integer x1 = -9
integer x2 = -9
integer x3 = -9
endglobals

function main takes nothing returns nothing
    set h1 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    set f1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
endfunction

function read takes nothing returns nothing
    set x1 = GetHeroLevel(h1)
    set x2 = GetHeroXP(h1)
    set x3 = GetHeroSkillPoints(h1)
endfunction

function read_footman takes nothing returns nothing
    set x1 = GetHeroLevel(f1)
    set x2 = GetHeroXP(f1)
    set x3 = GetHeroSkillPoints(f1)
endfunction

function suspend_on takes nothing returns nothing
    call SuspendHeroXP(h1, true)
endfunction

function suspend_off takes nothing returns nothing
    call SuspendHeroXP(h1, false)
endfunction
"#;

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SCRIPT}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        for (id, name) in [(FOOTMAN, "Footman"), (HERO, "Paladin")] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: F32::from_bits(0x4416_0000),
                    ..Default::default()
                },
            );
        }
    }
    host
}

/// Starts `name` and runs the host until it is done.
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

/// The model key of the unit in global `name`.
fn key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .expect("unit handle");
    host.game().unit_of_handle(id).expect("live unit")
}

#[test]
fn a_new_hero_starts_at_level_one_and_a_footman_has_no_hero_state() {
    let mut host = host();
    call(&mut host, "main");
    call(&mut host, "read");
    assert_eq!(int(&host, "x1"), 1);
    assert_eq!(int(&host, "x2"), 0);
    assert_eq!(int(&host, "x3"), 0);

    call(&mut host, "read_footman");
    assert_eq!(int(&host, "x1"), 0, "the gate fails for a non-hero type");
    assert_eq!(int(&host, "x2"), 0);
    assert_eq!(int(&host, "x3"), 0);
}

#[test]
fn level_experience_and_skill_points_are_read_from_the_model() {
    let mut host = host();
    call(&mut host, "main");
    {
        let k = key(&host, "h1");
        let mut g = host.game_mut();
        let h = g.unit_mut(k).hero.as_mut().expect("hero state");
        h.level = 4;
        h.xp = 350;
        h.skill_points = 2;
    }
    call(&mut host, "read");
    assert_eq!(int(&host, "x1"), 4);
    assert_eq!(int(&host, "x2"), 350);
    assert_eq!(int(&host, "x3"), 2);
}

#[test]
fn suspend_xp_stores_the_flag_and_an_illusion_fails_the_gate() {
    let mut host = host();
    call(&mut host, "main");
    call(&mut host, "suspend_on");
    let k = key(&host, "h1");
    assert!(host
        .game()
        .unit(k)
        .hero
        .as_ref()
        .is_some_and(|h| h.xp_suspended));

    call(&mut host, "suspend_off");
    assert!(host
        .game()
        .unit(k)
        .hero
        .as_ref()
        .is_some_and(|h| !h.xp_suspended));

    // An illusion of a hero fails the gate: the reads return 0 and the suspend flag does not change.
    host.game_mut().unit_mut(k).illusion = true;
    call(&mut host, "suspend_on");
    call(&mut host, "read");
    assert_eq!(int(&host, "x1"), 0);
    assert_eq!(int(&host, "x2"), 0);
    assert!(host
        .game()
        .unit(k)
        .hero
        .as_ref()
        .is_some_and(|h| !h.xp_suspended));
}
