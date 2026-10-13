//! Behaviour of the Blood Mage spells Banish (`AHbn`) and Summon Phoenix (`AHpx`) through `ScriptHost` (spec
//! `docs/spec/abilities-human-heroes.md` section 17). First-level numbers of the shipped data, not game data files.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::human_heroes::*;
use sim::natives::game::{ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const SRC: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
globals
    unit mage = null
    unit foe = null
    unit foeHero = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set mage = CreateUnit(Player(0), 'Hblm', 0.0, 0.0, 0.0)
    call UnitAddAbility(mage, 'AHbn')
    call UnitAddAbility(mage, 'AHpx')
    call SetUnitState(mage, 2, 500.0)
    set foe = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set foeHero = CreateUnit(Player(1), 'Hamg', 0.0, 300.0, 0.0)
endfunction
function banish takes nothing returns nothing
    set accepted = IssueTargetOrderById(mage, 852486, foe)
endfunction
function banish_hero takes nothing returns nothing
    set accepted = IssueTargetOrderById(mage, 852486, foeHero)
endfunction
function phoenix takes nothing returns nothing
    set accepted = IssueImmediateOrderById(mage, 852489)
endfunction
"#;

const BLOOD_MAGE: u32 = 0x4862_6C6D;
const ARCHMAGE: u32 = 0x4861_6D67;
const FOOT: u32 = 0x6866_6F6F;
const PHX: u32 = 0x6870_6878;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(dur: f32, hero_dur: f32, area: f32, data: [f32; 2], unit_id: u32) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    d[0] = f(data[0]);
    d[1] = f(data[1]);
    SpellLevel {
        cast: F32::ZERO,
        dur: f(dur),
        hero_dur: f(hero_dur),
        cost: 75,
        cool: f(10.0),
        area: f(area),
        rng: f(800.0),
        data: d,
        unit_id,
    }
}

fn started() -> ScriptHost {
    let prog = jass::parse(SRC).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [(BLOOD_MAGE, "Blood Mage", 700.0), (ARCHMAGE, "Archmage", 700.0), (FOOT, "Footman", 500.0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(500.0),
                    move_speed: f(270.0),
                    ..Default::default()
                },
            );
        }
        g.unit_types_mut().insert(
            PHX,
            UnitType {
                name: "Phoenix".to_string(),
                max_life: f(1000.0),
                move_speed: f(300.0),
                ..Default::default()
            },
        );
        for base in [BANISH, PHOENIX] {
            g.ability_types_mut().insert(base, AbilityType { max_level: Some(1) });
        }
        // Banish level 1 of the shipped data: Dur 12, HeroDur 5, DataA 0.5 (movement), Rng 800.
        g.set_spell_levels(BANISH, vec![level(12.0, 5.0, 0.0, [0.5, 0.0], 0)]);
        // Summon Phoenix level 1: DataA 1 unit, UnitID hphx, no duration.
        g.set_spell_levels(PHOENIX, vec![level(0.0, 0.0, 200.0, [1.0, 0.0], PHX)]);
    }
    h.start("setup", &[]).expect("setup");
    h.step();
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
}

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().unit(k).buffs.iter().any(|b| b.base == code)
}

#[test]
fn banish_makes_a_unit_ethereal_and_slows_it_for_dur() {
    let mut h = started();
    let foe = unit(&h, "foe");
    let speed = h.game().unit(foe).move_speed.to_f32();
    h.start("banish", &[]).unwrap();
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(foe).ethereal, 1, "the ethereal counter goes up (0x6F07A340)");
    assert!(has_buff(&h, foe, BUFF_BANISH));
    let slowed = h.game().unit(foe).move_speed.to_f32();
    assert!((slowed - speed * 0.5).abs() < 0.01, "movement factor -DataA: {slowed} from {speed}");
    run(&mut h, 115);
    assert_eq!(h.game().unit(foe).ethereal, 1, "still banished before 12 s");
    run(&mut h, 15);
    assert_eq!(h.game().unit(foe).ethereal, 0, "the removal lowers the counter");
    assert!(!has_buff(&h, foe, BUFF_BANISH));
    assert!((h.game().unit(foe).move_speed.to_f32() - speed).abs() < 0.01);
}

#[test]
fn banish_takes_hero_dur_on_a_hero() {
    let mut h = started();
    let hero = unit(&h, "foeHero");
    h.start("banish_hero", &[]).unwrap();
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(hero).ethereal, 1);
    run(&mut h, 55);
    assert_eq!(h.game().unit(hero).ethereal, 0, "HeroDur 5 s");
}

#[test]
fn phoenix_summons_a_permanent_unit_without_a_timed_life() {
    let mut h = started();
    let mage = unit(&h, "mage");
    h.start("phoenix", &[]).unwrap();
    run(&mut h, 3);
    assert!(accepted(&h));
    let all: Vec<ObjKey> = (0..h.game().object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|k| matches!(h.game().object(*k), sim::natives::game::Object::Unit(u) if u.type_id == PHX))
        .collect();
    assert_eq!(all.len(), 1, "DataA = 1");
    let p = all[0];
    assert!(h.game().unit(p).summoned);
    assert_eq!(h.game().unit(p).owner, h.game().unit(mage).owner);
    run(&mut h, 3000);
    assert!(!h.game().unit(p).dead, "no timed life: the Phoenix stays");
}
