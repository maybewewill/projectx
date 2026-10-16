//! Behaviour tests of the area and hero passives of `unit_abilities_a2.rs`: Flare (Afla), Blight Placement / Growth
//! (Ablp, Abli) and Attribute Bonus (Aamk). Level rows are the 1.26a first-level rows.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{code, AbilityType};
use sim::cast::SpellLevel;
use sim::natives::fog::FOG_VISIBLE;
use sim::natives::game::{ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
"#;

const SRC: &str = r#"
globals
    unit mortar = null
    unit necro = null
    unit hero = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set mortar = CreateUnit(Player(0), 'hmtm', 0.0, 0.0, 0.0)
    set necro = CreateUnit(Player(0), 'unpl', 4000.0, 0.0, 0.0)
    set hero = CreateUnit(Player(0), 'Hpal', -4000.0, 0.0, 0.0)
endfunction
function give_flare takes nothing returns nothing
    call UnitAddAbility(mortar, 'Afla')
endfunction
function fire_flare takes nothing returns nothing
    set accepted = IssuePointOrderById(mortar, 852060, 2000.0, 2000.0)
endfunction
function give_blight takes nothing returns nothing
    call UnitAddAbility(necro, 'Abli')
endfunction
function give_aamk takes nothing returns nothing
    call UnitAddAbility(hero, 'Aamk')
endfunction
function take_aamk takes nothing returns nothing
    call UnitRemoveAbility(hero, 'Aamk')
endfunction
"#;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn data(v: &[f32]) -> [F32; 9] {
    let mut d = [F32::ZERO; 9];
    for (i, x) in v.iter().enumerate() {
        d[i] = f(*x);
    }
    d
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (name, t) in [("mortar", b"hmtm"), ("necropolis", b"unpl"), ("paladin", b"Hpal")] {
            g.unit_types_mut().insert(
                u32::from_be_bytes(*t),
                UnitType { name: name.to_string(), max_life: f(1000.0), max_mana: f(300.0), move_speed: f(270.0), ..Default::default() },
            );
        }
        g.unit_types_mut().set_building(u32::from_be_bytes(*b"unpl"), true);
        let rows = [
            // Flare: Dur 15, Area 1800, Rng 99999, DataA 3, DataB 0.8 s delay.
            (code(b"Afla"), SpellLevel { dur: f(15.0), hero_dur: f(15.0), cool: f(120.0), area: f(1800.0), rng: f(99999.0), data: data(&[3.0, 0.8]), ..Default::default() }),
            // The Necropolis blight row (alias Abgs): Dur 0.08, Area 768, DataA 64, DataB 1.
            (code(b"Abli"), SpellLevel { dur: f(0.08), hero_dur: f(0.08), area: f(768.0), data: data(&[64.0, 1.0]), ..Default::default() }),
            (code(b"Aamk"), SpellLevel { data: data(&[3.0, 3.0, 3.0]), ..Default::default() }),
        ];
        for (c, l) in rows {
            g.ability_types_mut().insert(c, AbilityType { max_level: Some(1) });
            g.set_spell_levels(c, vec![l]);
        }
    }
    h.start("setup", &[]).expect("setup");
    h.step();
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn call(h: &mut ScriptHost, name: &str, steps: usize) {
    h.start(name, &[]).expect("start");
    run(h, steps);
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

#[test]
fn flare_opens_the_area_after_its_delay_and_closes_it_after_its_duration() {
    let mut h = host();
    call(&mut h, "give_flare", 1);
    let state = |h: &ScriptHost| h.game().fog_state_at(0, f(2000.0), f(2000.0));
    assert_ne!(state(&h), FOG_VISIBLE);
    call(&mut h, "fire_flare", 2);
    assert!(h.vm().global("accepted").and_then(|v| v.as_boolean()).unwrap());
    assert_ne!(state(&h), FOG_VISIBLE, "the 0.8 s effect delay has not run out");
    run(&mut h, 12);
    assert_eq!(state(&h), FOG_VISIBLE, "open for the caster's player");
    assert_ne!(h.game().fog_state_at(1, f(2000.0), f(2000.0)), FOG_VISIBLE, "not for another player");
    run(&mut h, 160);
    assert_ne!(state(&h), FOG_VISIBLE, "closed after 15 s");
}

#[test]
fn blight_grows_in_64_unit_steps_up_to_the_area() {
    let mut h = host();
    call(&mut h, "give_blight", 1);
    let blighted = |h: &ScriptHost, d: f32| h.game().is_blighted(f(4000.0 + d), f(0.0));
    assert!(!blighted(&h, 700.0));
    run(&mut h, 3);
    assert!(blighted(&h, 40.0) && !blighted(&h, 700.0), "first steps");
    run(&mut h, 40);
    assert!(blighted(&h, 700.0), "radius 768 reached");
    assert!(!blighted(&h, 900.0), "no further");
}

#[test]
fn attribute_bonus_adds_to_the_hero_base_attributes_and_leaves_with_the_ability() {
    let mut h = host();
    let hero = unit(&h, "hero");
    let base = {
        let g = h.game();
        let o = g.unit(hero).hero.as_ref().expect("hero state");
        (o.agi_base, o.int_base, o.str_base)
    };
    call(&mut h, "give_aamk", 1);
    let after = {
        let g = h.game();
        let o = g.unit(hero).hero.as_ref().unwrap();
        (o.agi_base, o.int_base, o.str_base)
    };
    assert_eq!(after, (base.0 + 3, base.1 + 3, base.2 + 3));
    call(&mut h, "take_aamk", 1);
    let o = h.game().unit(hero).hero.clone().unwrap();
    assert_eq!((o.agi_base, o.int_base, o.str_base), base);
}
