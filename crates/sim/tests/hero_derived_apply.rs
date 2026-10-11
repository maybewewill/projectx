//! The derived attribute bonuses applied to the unit (`docs/spec/heroes.md`, section 10). Armor enters the armor of
//! the unit (`combat::unit_defense`), the move bonus the move speed, the attack bonus the attacker's dice bonus, and
//! the regeneration and attack speed bonuses are stored. The rates are installed directly, so no `GAME_DLL` is needed.
//!
//! Hand-computed values (all exact in binary): AgiDefenseBase 2, AgiDefenseBonus 0.25, agility 20 (base 20, gain 0),
//! so the armor bonus is 2 + 20 * 0.25 = 7; AgiMoveBonus 0.5 gives +10 move speed for agility 20 (the move speed of
//! the test unit starts at 0; the hero starts at agility 0, so the setter moves it to 20). StrAttackBonus 0.25 with the default primary (agility) gives 5.

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
native SetHeroAgi takes unit whichHero, integer newAgi, boolean permanent returns nothing
"#;

const HERO: u32 = 0x4870_616C; // 'Hpal'

const SCRIPT: &str = r#"
globals
unit h1
endglobals

function main takes nothing returns nothing
    set h1 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
endfunction

function set_agi20 takes nothing returns nothing
    call SetHeroAgi(h1, 20, true)
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
                agi_base: 0,
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

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

#[test]
fn agility_setter_applies_armor_and_move_bonuses_to_the_unit() {
    let rates = BonusRates {
        agi_defense_base: f(2.0),
        agi_defense: f(0.25),
        agi_move: f(0.5),
        ..BonusRates::default()
    };
    let mut host = host(rates);
    call(&mut host, "main");
    let handle = host.vm().global("h1").and_then(|v| v.as_handle()).expect("the hero handle");
    let key = host.game_mut().unit_of_handle(handle).expect("the hero unit");
    assert_eq!(host.game_mut().unit_defense(key).armor, F32::ZERO, "no armor before the setter");
    assert_eq!(host.game_mut().unit(key).move_speed.to_f32(), 0.0);
    call(&mut host, "set_agi20");
    let g = host.game_mut();
    assert_eq!(g.unit_defense(key).armor.to_f32(), 7.0, "armor = 2 + 20 * 0.25");
    assert_eq!(g.unit(key).hero_armor.to_f32(), 7.0);
    assert_eq!(g.unit(key).move_speed.to_f32(), 10.0, "move = 20 * 0.5, the setter moves agility 0 to 20");
}

#[test]
fn stored_bonuses_are_totals_and_regeneration_and_attack_speed_are_stored() {
    let rates = BonusRates {
        str_regen: f(0.5),
        agi_attack_speed: f(0.125),
        ..BonusRates::default()
    };
    let mut host = host(rates);
    call(&mut host, "main");
    call(&mut host, "set_agi20");
    let handle = host.vm().global("h1").and_then(|v| v.as_handle()).expect("the hero handle");
    let key = host.game_mut().unit_of_handle(handle).expect("the hero unit");
    let g = host.game_mut();
    // The strength part of the regeneration is zero (strength 0), the attack speed is 20 * 0.125 = 2.5.
    assert_eq!(g.unit(key).hero_regen.to_f32(), 0.0);
    assert_eq!(g.unit(key).hero_attack_speed.to_f32(), 2.5);
    // A second call with the same attributes changes nothing: the totals are recomputed, not accumulated.
    drop(g);
    call(&mut host, "set_agi20");
    let g = host.game_mut();
    assert_eq!(g.unit(key).hero_attack_speed.to_f32(), 2.5);
    assert_eq!(g.unit(key).move_speed.to_f32(), 0.0);
}
