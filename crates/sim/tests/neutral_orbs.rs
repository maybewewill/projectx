//! Behaviour tests of Black Arrow and Incinerate (`neutral_orbs.rs`, spec `docs/spec/abilities-neutral.md` section 8).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::neutral_orbs::*;
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
"#;

const SRC: &str = r#"
globals
    unit ranger = null
    unit burner = null
    unit victim = null
    unit near = null
    unit mid = null
    unit far = null
    unit v2 = null
    unit iarrow = null
    unit iv = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set ranger = CreateUnit(Player(0), 'nrng', 0.0, 0.0, 0.0)
    set victim = CreateUnit(Player(1), 'ndmy', 80.0, 0.0, 180.0)
    set burner = CreateUnit(Player(0), 'nfl0', 5000.0, 0.0, 0.0)
    set v2 = CreateUnit(Player(1), 'ndmy', 5080.0, 0.0, 180.0)
    set near = CreateUnit(Player(1), 'ndmy', 5130.0, 0.0, 0.0)
    set mid = CreateUnit(Player(1), 'ndmy', 5230.0, 0.0, 0.0)
    set far = CreateUnit(Player(1), 'ndmy', 5500.0, 0.0, 0.0)
    set iarrow = CreateUnit(Player(0), 'nia0', 8000.0, 0.0, 0.0)
    set iv = CreateUnit(Player(1), 'ndmy', 8080.0, 0.0, 180.0)
endfunction
function arrows_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(ranger, 852578)
endfunction
function arrows_off takes nothing returns nothing
    set accepted = IssueImmediateOrderById(ranger, 852579)
endfunction
function shoot takes nothing returns nothing
    call IssueTargetOrder(ranger, "attack", victim)
endfunction
function iarrows_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(iarrow, 852671)
endfunction
function ishoot takes nothing returns nothing
    call IssueTargetOrder(iarrow, "attack", iv)
endfunction
function burn takes nothing returns nothing
    call IssueTargetOrder(burner, "attack", v2)
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

fn id(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

fn ut(abilities: Vec<u32>) -> UnitType {
    UnitType {
        name: "unit".to_string(),
        max_life: f(500.0),
        max_mana: f(500.0),
        start_mana: f(500.0),
        move_speed: f(270.0),
        abilities,
        ..Default::default()
    }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(id(b"nrng"), ut(vec![BLACK_ARROW]));
        g.unit_types_mut().insert(id(b"nfl0"), ut(vec![INCINERATE]));
        g.unit_types_mut().insert(id(b"nia0"), ut(vec![INCINERATE_ARROW]));
        g.unit_types_mut().insert(id(b"ndmy"), ut(vec![]));
        g.unit_types_mut().insert(id(b"ndr1"), ut(vec![]));
        for t in [id(b"nrng"), id(b"nfl0"), id(b"nia0")] {
            g.set_type_orders(t, TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
            g.set_attack_type(
                t,
                AttackType {
                    cooldown: f(1.0),
                    damage_point: f(0.3),
                    backswing: f(0.3),
                    range: f(100.0),
                    collision: f(0.5),
                    damage: f(10.0),
                    missile_speed: None,
                    ..Default::default()
                },
            );
        }
        g.set_spell_levels(
            BLACK_ARROW,
            vec![SpellLevel { cost: 6, dur: f(2.0), hero_dur: f(2.0), rng: f(600.0), unit_id: id(b"ndr1"), data: data(&[2.0, 1.0, 80.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            INCINERATE,
            vec![SpellLevel { dur: f(2.0), hero_dur: f(2.0), data: data(&[1.0, 30.0, 90.0, 15.0, 180.0, 0.2]), ..Default::default() }],
        );
        g.set_spell_levels(
            INCINERATE_ARROW,
            vec![SpellLevel { cost: 6, dur: f(2.0), hero_dur: f(2.0), rng: f(550.0), data: data(&[1.0, 30.0, 90.0, 15.0, 180.0, 0.2]), ..Default::default() }],
        );
        for a in [BLACK_ARROW, INCINERATE, INCINERATE_ARROW] {
            g.ability_types_mut().insert(a, AbilityType { max_level: Some(1) });
        }
    }
    h
}

fn started() -> ScriptHost {
    let mut h = host();
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

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn count_of_type(h: &ScriptHost, ty: u32) -> usize {
    let g = h.game();
    (0..g.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if !u.dead && !u.removed && u.type_id == ty))
        .count()
}

#[test]
fn black_arrow_is_off_until_the_autocast_is_switched_on() {
    let mut h = started();
    let (ranger, victim) = (unit(&h, "ranger"), unit(&h, "victim"));
    h.start("shoot", &[]).expect("shoot");
    run(&mut h, 8);
    assert!(life(&h, victim) < 500.0, "a plain hit");
    assert!(!h.game().black_arrow_marked(victim));
    assert_eq!(h.game().unit(ranger).mana.to_f32(), 500.0);
    h.start("arrows_on", &[]).expect("on");
    run(&mut h, 1);
    assert!(h.game().black_arrow_autocast(ranger));
}

#[test]
fn a_black_arrow_costs_mana_adds_damage_and_marks_the_target() {
    let mut h = started();
    let (ranger, victim) = (unit(&h, "ranger"), unit(&h, "victim"));
    h.start("arrows_on", &[]).expect("on");
    run(&mut h, 1);
    h.start("shoot", &[]).expect("shoot");
    run(&mut h, 8);
    assert!(h.game().black_arrow_marked(victim), "marked for Dur");
    assert_eq!(h.game().unit(ranger).mana.to_f32(), 494.0, "6 mana per arrow");
    assert_eq!(life(&h, victim), 500.0 - 10.0 - 2.0, "the attack plus DataA");
}

#[test]
fn a_unit_that_dies_marked_becomes_a_dark_minion_for_the_archer() {
    let mut h = started();
    let victim = unit(&h, "victim");
    h.game_mut().unit_mut(victim).life = f(22.0);
    h.start("arrows_on", &[]).expect("on");
    run(&mut h, 1);
    h.start("shoot", &[]).expect("shoot");
    run(&mut h, 20);
    assert!(h.game().unit(victim).dead);
    assert_eq!(count_of_type(&h, id(b"ndr1")), 1, "one skeleton rises");
    run(&mut h, 900);
    assert_eq!(count_of_type(&h, id(b"ndr1")), 0, "gone after DataC = 80 s");
}

#[test]
fn incinerate_stacks_the_burn_and_the_death_blast_has_two_rings() {
    let mut h = started();
    let (v2, near, mid, far) = (unit(&h, "v2"), unit(&h, "near"), unit(&h, "mid"), unit(&h, "far"));
    h.start("burn", &[]).expect("burn");
    run(&mut h, 8);
    assert_eq!(h.game().incinerate_stacks(v2), 1);
    assert_eq!(life(&h, v2), 500.0 - 10.0 - 1.0, "attack plus 1 x DataA");
    run(&mut h, 10);
    assert_eq!(h.game().incinerate_stacks(v2), 2, "the second attack stacks");
    assert_eq!(life(&h, v2), 500.0 - 10.0 - 1.0 - 10.0 - 2.0);
    // Kill it while it burns: the blast follows after 0.2 s.
    h.game_mut().unit_mut(v2).life = f(5.0);
    run(&mut h, 10);
    assert!(h.game().unit(v2).dead);
    run(&mut h, 4);
    assert_eq!(life(&h, near), 500.0 - 30.0, "within 90 of the corpse: the full amount");
    assert_eq!(life(&h, mid), 500.0 - 15.0, "within 180: the half amount");
    assert_eq!(life(&h, far), 500.0, "outside");
}

#[test]
fn incinerate_arrow_needs_the_autocast_and_pays_mana_per_arrow() {
    let mut h = started();
    let (iarrow, iv) = (unit(&h, "iarrow"), unit(&h, "iv"));
    h.start("ishoot", &[]).expect("shoot");
    run(&mut h, 8);
    assert_eq!(life(&h, iv), 500.0 - 10.0, "off: a plain hit");
    assert_eq!(h.game().incinerate_stacks(iv), 0);
    h.start("iarrows_on", &[]).expect("on");
    run(&mut h, 12);
    assert_eq!(h.game().incinerate_stacks(iv), 1, "on: the second arrow burns");
    assert_eq!(h.game().unit(iarrow).mana.to_f32(), 494.0, "6 mana per arrow");
}
