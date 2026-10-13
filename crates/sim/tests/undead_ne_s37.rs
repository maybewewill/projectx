//! S37 behaviour tests: Carrion Swarm's marker and cap, Inferno's delayed landing. Level data are pinned values of
//! the tests (the stock rows where noted); one host step = 0.1 s.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::undead_ne::{CARRION_SWARM, DEATH_AND_DECAY, IMPALE, INFERNO};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit near = null
    unit far = null
    unit hero = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'Hdkn', 0.0, 0.0, 0.0)
    set near = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set far = CreateUnit(Player(1), 'hfoo', 300.0, 1500.0, 0.0)
    set hero = CreateUnit(Player(1), 'Hamg', 330.0, 40.0, 0.0)
    call UnitAddAbility(caster, 'AUcs')
    call UnitAddAbility(caster, 'AUin')
    call UnitAddAbility(caster, 'AUdd')
    call UnitAddAbility(caster, 'AUim')
    call SetUnitState(caster, 2, 2000.0)
endfunction
function swarm takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852218, 1000.0, 0.0)
endfunction
function impale takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852555, 700.0, 0.0)
endfunction
function decay takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852221, 300.0, 0.0)
endfunction
function inferno takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852224, 300.0, 0.0)
endfunction
"#;

const HERO: u32 = 0x4864_6B6E; // 'Hdkn'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const AMG: u32 = 0x4861_6D67; // 'Hamg'
const NINF: u32 = 0x6E69_6E66; // 'ninf'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(cost: i32, cool: f32, dur: f32, hero_dur: f32, area: f32, data: &[f32]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel {
        cast: F32::ZERO,
        dur: f(dur),
        hero_dur: f(hero_dur),
        cost,
        cool: f(cool),
        area: f(area),
        rng: f(99999.0),
        data: d,
        unit_id: 0,
    }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [(HERO, "DK", 700.0), (FOOT, "Footman", 500.0), (AMG, "Archmage", 600.0), (NINF, "Infernal", 900.0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(2000.0),
                    move_speed: f(270.0),
                    movement: 1,
                    ..Default::default()
                },
            );
        }
        g.set_type_orders(FOOT, TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        g.set_attack_type(
            FOOT,
            AttackType {
                cooldown: f(1.0),
                damage_point: f(0.5),
                backswing: f(0.5),
                range: f(400.0),
                collision: f(0.5),
                damage: f(0.0),
                missile_speed: None,
                ..Default::default()
            },
        );
        for code in [CARRION_SWARM, INFERNO, DEATH_AND_DECAY, IMPALE] {
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(1) });
        }
        // Carrion Swarm: Area 100 (start width), DataA 75, DataB cap 150, DataC distance 800, DataD final width 200.
        g.set_spell_levels(CARRION_SWARM, vec![level(110, 6.0, 0.0, 0.0, 100.0, &[75.0, 150.0, 800.0, 200.0])]);
        g.set_spell_missile_speed(CARRION_SWARM, f(1100.0));
        // Inferno (stock): Dur 4 / HeroDur 2 (stun), Area 250, DataA 50, DataB 180 (the Infernal's life), DataC 1 (delay).
        let mut inf = level(175, 180.0, 4.0, 2.0, 250.0, &[50.0, 180.0, 1.0]);
        inf.unit_id = NINF;
        g.set_spell_levels(INFERNO, vec![inf]);
        // Impale (stock): Dur 2 / HeroDur 1, Area 250, DataA 600 (line length), DataB 0.3 (time), DataC 50 damage.
        g.set_spell_levels(IMPALE, vec![level(100, 9.0, 2.0, 1.0, 250.0, &[600.0, 0.3, 50.0, 1.0])]);
        // Death and Decay (stock): Dur 35, Area 300, DataA 0.04 of the maximum life per second, DataB 1.
        g.set_spell_levels(DEATH_AND_DECAY, vec![level(250, 150.0, 35.0, 35.0, 300.0, &[0.04, 1.0])]);
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
    assert!(h.misfires().is_empty(), "no thread was refused");
}

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

#[test]
fn carrion_swarm_hits_a_unit_once_while_it_stays_in_the_swath() {
    let mut h = host();
    let near = unit(&h, "near");
    let before = life(&h, near);
    call(&mut h, "swarm");
    run(&mut h, 40);
    // The swath passes the unit at (300, 0) for several ticks, but the marker keeps it from being hit again.
    assert_eq!(life(&h, near), before - 75.0);
    assert_eq!(h.game().swarm_hits().iter().filter(|hit| hit.1 == near).count(), 1);
}

#[test]
fn carrion_swarm_cap_counts_the_life_lost_and_spreads_over_the_units_of_a_tick() {
    let mut h = host();
    let (near, hero) = (unit(&h, "near"), unit(&h, "hero"));
    let (l1, l2) = (life(&h, near), life(&h, hero));
    call(&mut h, "swarm");
    run(&mut h, 40);
    // Both units are in the swath when it reaches them; the cap 150 is used up exactly over the hits.
    let lost = (l1 - life(&h, near)) + (l2 - life(&h, hero));
    assert!(lost <= 150.0 + 0.01, "{lost}");
    assert!(lost >= 75.0);
}

#[test]
fn inferno_lands_after_its_delay_with_damage_and_a_stun() {
    let mut h = host();
    let (near, far, hero) = (unit(&h, "near"), unit(&h, "far"), unit(&h, "hero"));
    let (ln, lf, lh) = (life(&h, near), life(&h, far), life(&h, hero));
    call(&mut h, "inferno");
    run(&mut h, 5);
    assert_eq!(life(&h, near), ln, "nothing before the impact delay (1.0 s)");
    run(&mut h, 8);
    assert_eq!(life(&h, near), ln - 50.0);
    assert_eq!(life(&h, hero), lh - 50.0);
    assert_eq!(life(&h, far), lf, "outside Area 250");
    assert!(h.game().is_stunned(near));
    assert!(h.game().is_stunned(hero));
    // The stun lasts Dur = 4 s (HeroDur = 2 s for the hero).
    run(&mut h, 25);
    assert!(h.game().is_stunned(near));
    assert!(!h.game().is_stunned(hero));
    run(&mut h, 20);
    assert!(!h.game().is_stunned(near));
}

#[test]
fn death_and_decay_burns_a_fraction_of_the_maximum_life_each_second_inside_the_area() {
    let mut h = host();
    let (near, far) = (unit(&h, "near"), unit(&h, "far"));
    let (ln, lf) = (life(&h, near), life(&h, far));
    call(&mut h, "decay");
    run(&mut h, 5);
    assert_eq!(life(&h, near), ln, "no pulse in the first second");
    run(&mut h, 30);
    // 4% of the maximum life 500 = 20 per pulse; three pulses by 3.5 s (first after the delay, then every 1.0 s).
    let lost = ln - life(&h, near);
    assert!(lost == 40.0 || lost == 60.0, "{lost}");
    assert_eq!(life(&h, far), lf, "outside Area 300");
    // The effect ends after Dur = 35 s.
    run(&mut h, 400);
    let after = life(&h, near);
    run(&mut h, 50);
    assert_eq!(life(&h, near), after);
}

#[test]
fn impale_spikes_run_along_the_line_and_hit_each_unit_once() {
    let mut h = host();
    let (near, far, hero) = (unit(&h, "near"), unit(&h, "far"), unit(&h, "hero"));
    let (ln, lf, lh) = (life(&h, near), life(&h, far), life(&h, hero));
    call(&mut h, "impale");
    run(&mut h, 3);
    assert_eq!(life(&h, near), ln, "the first spike comes after DataB + 0.1 = 0.4 s");
    run(&mut h, 10);
    assert_eq!(life(&h, near), ln - 50.0, "one spike of the line passes the unit at 300 (spikes every 120 from 125)");
    assert_eq!(life(&h, hero), lh - 50.0);
    assert_eq!(life(&h, far), lf);
    assert!(h.game().is_stunned(near));
    // Dur 2 s for the footman, HeroDur 1 s for the hero.
    run(&mut h, 8);
    assert!(h.game().is_stunned(near));
    assert!(!h.game().is_stunned(hero));
    run(&mut h, 15);
    assert!(!h.game().is_stunned(near));
    assert_eq!(life(&h, near), ln - 50.0, "never hit twice");
}
