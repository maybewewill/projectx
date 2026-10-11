//! The attribute setters apply the derived life change (`natives/hero.rs`, `with_attribute_change`, `docs/spec/heroes.md`
//! section 9). The rates are installed directly (`BonusRates`), so the test does not need `GAME_DLL`. Expected values
//! are worked out by hand: StrHitPointBonus = 0.5, base strength 20 with gain 2.5 at level 1 (strength 20), then
//! SetHeroStr(30) moves the base to 30: the hit-point bonus goes from 20 * 0.5 = 10 to 30 * 0.5 = 15, so the maximum
//! life (600 from the unit type) rises by 5.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::heroes::{BonusRates, HeroAttributes};
use sim::natives::game::UnitType;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type unit extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetHeroStr takes unit whichHero, boolean includeBonuses returns integer
native SetHeroStr takes unit whichHero, integer newStr, boolean permanent returns nothing
native GetUnitState takes unit whichUnit, integer whichUnitState returns real
"#;

const HERO: u32 = 0x4870_616C; // 'Hpal'

const SCRIPT: &str = r#"
globals
unit h1
integer r1 = -9
real r2 = 0.0
real r3 = 0.0
endglobals

function main takes nothing returns nothing
    set h1 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
endfunction

function read takes nothing returns nothing
    set r1 = GetHeroStr(h1, true)
    set r2 = GetUnitState(h1, 0)
    set r3 = GetUnitState(h1, 1)
endfunction

function set_str30 takes nothing returns nothing
    call SetHeroStr(h1, 30, true)
endfunction
"#;

fn host(rates: BonusRates) -> ScriptHost {
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
                max_life: F32::from_bits(600.0f32.to_bits()),
                ..Default::default()
            },
        );
        g.set_hero_attributes(
            HERO,
            HeroAttributes {
                str_base: 20,
                str_plus: F32::from_bits(2.5f32.to_bits()),
                ..HeroAttributes::default()
            },
        );
        g.set_hero_rates(rates);
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

fn real(host: &ScriptHost, name: &str) -> f32 {
    let v = host
        .vm()
        .global(name)
        .and_then(|v| v.as_real())
        .unwrap_or_else(|| panic!("global {name} is not a real"));
    v.to_f32()
}

#[test]
fn strength_setter_moves_the_maximum_life_by_the_hit_point_bonus_change() {
    let rates = BonusRates {
        str_hit_point: F32::from_bits(0.5f32.to_bits()),
        ..BonusRates::default()
    };
    let mut host = host(rates);
    call(&mut host, "main");
    call(&mut host, "read");
    assert_eq!(int(&host, "r1"), 20);
    assert_eq!(real(&host, "r3"), 600.0);
    call(&mut host, "set_str30");
    call(&mut host, "read");
    assert_eq!(int(&host, "r1"), 30);
    // 0.5 * 30 - 0.5 * 20 = 5: maximum life 605, and the life (600, full) moves with it.
    assert_eq!(real(&host, "r3"), 605.0);
    assert_eq!(real(&host, "r2"), 605.0);
}

#[test]
fn no_rates_means_no_derived_change() {
    let mut host = host(BonusRates::default());
    call(&mut host, "main");
    call(&mut host, "set_str30");
    call(&mut host, "read");
    assert_eq!(int(&host, "r1"), 30);
    assert_eq!(real(&host, "r3"), 600.0);
}
