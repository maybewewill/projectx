//! S48 (`hero_extra.rs`, docs/spec/abilities-hero-extra.md): Fan of Knives, Mana Burn, Metamorphosis, Scout, Starfall,
//! Tranquility, Animate Dead, Carrion Scarabs and the ranger arrows. The level data are 1.26a AbilityData.slk values;
//! the models are LIKELY/UNSURE.

use jass::compile::compile;
use jass::vm::{NativeRegistry, Value};
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::hero_extra::*;
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
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native SetWidgetLife takes widget wid, real newLife returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit e1 = null
    unit e2 = null
    unit e3 = null
    unit far = null
    unit friend = null
    unit corpse1 = null
    unit corpse2 = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'Udea', 0.0, 0.0, 0.0)
    set e1 = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set e2 = CreateUnit(Player(1), 'hfoo', 0.0, 300.0, 0.0)
    set e3 = CreateUnit(Player(1), 'hfoo', -300.0, 0.0, 0.0)
    set far = CreateUnit(Player(1), 'hfoo', 3000.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    call SetWidgetLife(friend, 100.0)
    call SetUnitState(caster, 2, 1000.0)
endfunction
function add takes integer a returns nothing
    call UnitAddAbility(caster, a)
endfunction
function immediate takes integer id returns nothing
    set accepted = IssueImmediateOrderById(caster, id)
endfunction
function target_e1 takes integer id returns nothing
    set accepted = IssueTargetOrderById(caster, id, e1)
endfunction
function target_corpse takes integer id returns nothing
    set accepted = IssueTargetOrderById(caster, id, corpse1)
endfunction
function make_corpses takes nothing returns nothing
    set corpse1 = CreateUnit(Player(1), 'hfoo', 200.0, 200.0, 0.0)
    set corpse2 = CreateUnit(Player(1), 'hfoo', 250.0, 250.0, 0.0)
    call SetWidgetLife(corpse1, 0.0)
    call SetWidgetLife(corpse2, 0.0)
endfunction
"#;

const HERO: u32 = 0x5564_6561; // 'Udea'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const OWL: u32 = 0x6E6F_776C; // 'nowl'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(cost: i32, dur: f32, hero_dur: f32, area: f32, data: &[f32]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel {
        cast: F32::ZERO,
        dur: f(dur),
        hero_dur: f(hero_dur),
        cost,
        cool: f(1.0),
        area: f(area),
        rng: f(900.0),
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
        for (id, name, life) in [(HERO, "DK", 700.0), (FOOT, "Footman", 500.0), (OWL, "Owl", 50.0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(1000.0),
                    move_speed: f(270.0),
                    ..Default::default()
                },
            );
        }
        for code in [
            FAN_OF_KNIVES,
            MANA_BURN,
            METAMORPHOSIS,
            SCOUT,
            ANIMATE_DEAD,
            CARRION_SCARABS,
            STARFALL,
            TRANQUILITY,
            COLD_ARROWS,
            SEARING_ARROWS,
            POISON_ARROWS,
        ] {
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(1) });
        }
        g.set_spell_levels(FAN_OF_KNIVES, vec![level(100, 0.0, 0.0, 400.0, &[75.0, 200.0, 0.0, 100.0])]);
        g.set_spell_levels(MANA_BURN, vec![level(50, 0.0, 0.0, 0.0, &[50.0, 0.25, 1.0])]);
        g.set_spell_levels(METAMORPHOSIS, vec![level(150, 1.5, 60.0, 0.0, &[0.0, 1.0, 0.0, 0.0, 500.0])]);
        let mut scout = level(100, 60.0, 60.0, 200.0, &[1.0]);
        scout.unit_id = OWL;
        g.set_spell_levels(SCOUT, vec![scout]);
        g.set_spell_levels(STARFALL, vec![level(200, 45.0, 30.0, 1000.0, &[50.0, 1.5, 0.35])]);
        g.set_spell_levels(TRANQUILITY, vec![level(125, 30.0, 30.0, 900.0, &[20.0, 1.0])]);
        g.set_spell_levels(ANIMATE_DEAD, vec![level(175, 40.0, 40.0, 900.0, &[1.0, 1.0])]);
        let mut sc = level(30, 0.0, 0.0, 900.0, &[2.0, 0.0, 0.0, 0.0, 5.0]);
        sc.unit_id = OWL;
        g.set_spell_levels(CARRION_SCARABS, vec![sc]);
        g.set_spell_levels(COLD_ARROWS, vec![level(8, 6.0, 1.0, 0.0, &[5.0, 0.3, 0.3, 7.0])]);
        g.set_spell_levels(SEARING_ARROWS, vec![level(8, 0.0, 0.0, 0.0, &[10.0])]);
        g.set_spell_levels(POISON_ARROWS, vec![level(0, 10.0, 10.0, 0.0, &[10.0, 4.0])]);
        for &(id, base) in ORDERS {
            g.set_spell_order(id, base);
        }
    }
    h.start("setup", &[]).expect("setup");
    h.step();
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("unit")
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty(), "no thread was refused");
}

fn give(h: &mut ScriptHost, code: u32) {
    h.start("add", &[Value::Integer(code as i32)]).expect("add");
    run(h, 1);
}

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).unwrap()
}

fn cast_immediate(h: &mut ScriptHost, id: i32) -> bool {
    h.start("immediate", &[Value::Integer(id)]).expect("start");
    run(h, 1);
    accepted(h)
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

#[test]
fn fan_of_knives_damage_is_capped_in_total() {
    let mut h = host();
    give(&mut h, FAN_OF_KNIVES);
    let (e1, e2, e3, far) = (unit(&h, "e1"), unit(&h, "e2"), unit(&h, "e3"), unit(&h, "far"));
    assert!(cast_immediate(&mut h, 852_526));
    // 3 victims x 75 = 225 > cap 200 -> 66.67 each.
    for e in [e1, e2, e3] {
        let taken = 500.0 - life(&h, e);
        assert!((taken - 200.0 / 3.0).abs() < 0.05, "got {taken}");
    }
    assert_eq!(life(&h, far), 500.0);
    assert_eq!(life(&h, unit(&h, "friend")), 100.0);
}

#[test]
fn fan_damage_each_uses_the_cap_only_when_exceeded() {
    let h = host();
    let g = h.game();
    assert_eq!(fan_damage_each(&g, f(75.0), f(300.0), 3).to_f32(), 75.0);
    assert_eq!(fan_damage_each(&g, f(75.0), f(0.0), 9).to_f32(), 75.0);
    assert!((fan_damage_each(&g, f(75.0), f(300.0), 8).to_f32() - 37.5).abs() < 0.01);
}

#[test]
fn mana_burn_takes_mana_and_deals_that_much_damage() {
    let mut h = host();
    give(&mut h, MANA_BURN);
    let e1 = unit(&h, "e1");
    h.game_mut().write_mana(e1, f(30.0));
    h.start("target_e1", &[Value::Integer(852_179)]).unwrap();
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(e1).mana.to_f32(), 0.0, "burned min(30, 50)");
    assert_eq!(life(&h, e1), 470.0, "damage = burned * DataC");
    let (b, d) = mana_burn_amounts(f(200.0), f(50.0), f(1.0));
    assert_eq!((b.to_f32(), d.to_f32()), (50.0, 50.0));
}

#[test]
fn metamorphosis_adds_hit_points_after_the_delay_and_loses_them_again() {
    let mut h = host();
    give(&mut h, METAMORPHOSIS);
    let c = unit(&h, "caster");
    assert!(cast_immediate(&mut h, 852_180));
    assert_eq!(h.game().unit(c).max_life.to_f32(), 700.0, "not yet transformed");
    run(&mut h, 20);
    assert_eq!(h.game().unit(c).max_life.to_f32(), 1200.0);
    assert_eq!(h.game().unit(c).life.to_f32(), 1200.0);
    run(&mut h, 640);
    assert_eq!(h.game().unit(c).max_life.to_f32(), 700.0);
    assert_eq!(h.game().unit(c).life.to_f32(), 700.0);
}

#[test]
fn scout_summons_an_owl_that_expires() {
    let mut h = host();
    give(&mut h, SCOUT);
    let before = h.game().object_count();
    assert!(cast_immediate(&mut h, 852_181));
    assert_eq!(h.game().object_count(), before + 1);
    let owl = ObjKey(before as u32);
    assert_eq!(h.game().unit(owl).type_id, OWL);
    assert!(h.game().unit(owl).summoned);
    run(&mut h, 640);
    assert!(h.game().unit(owl).dead, "Dur 60 s");
}

#[test]
fn starfall_pulses_until_the_channel_ends() {
    let mut h = host();
    give(&mut h, STARFALL);
    let (e1, far) = (unit(&h, "e1"), unit(&h, "far"));
    assert!(cast_immediate(&mut h, 852_183));
    run(&mut h, 35); // 3.5 s: pulses at 1.5 and 3.0
    assert_eq!(500.0 - life(&h, e1), 100.0);
    assert_eq!(life(&h, far), 500.0, "outside Area");
    run(&mut h, 320); // the 30 s channel has ended
    let n = h.game().channel_pulses().len();
    assert!(n >= 19 && n <= 21, "about 20 pulses, got {n}");
    run(&mut h, 100);
    assert_eq!(h.game().channel_pulses().len(), n, "no pulses after the channel");
}

#[test]
fn tranquility_heals_friends_each_second() {
    let mut h = host();
    give(&mut h, TRANQUILITY);
    let friend = unit(&h, "friend");
    assert!(cast_immediate(&mut h, 852_184));
    run(&mut h, 25);
    assert_eq!(life(&h, friend), 140.0, "two pulses of DataA = 20");
    assert_eq!(life(&h, unit(&h, "e1")), 500.0, "enemies are not healed");
}

#[test]
fn animate_dead_raises_the_nearest_corpse_as_a_timed_unit() {
    let mut h = host();
    give(&mut h, ANIMATE_DEAD);
    h.start("make_corpses", &[]).unwrap();
    run(&mut h, 1);
    let before = h.game().object_count();
    assert!(cast_immediate(&mut h, 852_217));
    assert_eq!(h.game().object_count(), before + 1, "DataA = 1");
    let raised = ObjKey(before as u32);
    assert_eq!(h.game().unit(raised).owner, 0);
    assert!(h.game().unit(raised).summoned);
    run(&mut h, 420);
    assert!(h.game().unit(raised).dead, "Dur 40 s");
}

#[test]
fn carrion_scarabs_consume_the_corpse_and_make_units() {
    let mut h = host();
    give(&mut h, CARRION_SCARABS);
    h.start("make_corpses", &[]).unwrap();
    run(&mut h, 1);
    let before = h.game().object_count();
    h.start("target_corpse", &[Value::Integer(852_551)]).unwrap();
    run(&mut h, 1);
    assert!(accepted(&h), "a corpse is a legal target");
    assert_eq!(h.game().object_count(), before + 2, "DataA = 2 units");
    assert_eq!(h.game().unit(ObjKey(before as u32)).type_id, OWL);
}

#[test]
fn ranger_arrows_add_damage_pay_mana_and_apply_their_buffs() {
    let mut h = host();
    give(&mut h, SEARING_ARROWS);
    give(&mut h, COLD_ARROWS);
    give(&mut h, POISON_ARROWS);
    let (c, e1) = (unit(&h, "caster"), unit(&h, "e1"));
    let mut g = h.game_mut();
    let before = g.unit(c).mana.to_f32();
    let died = g.hero_orb_on_hit(c, e1);
    assert!(died.is_empty());
    assert_eq!(g.unit(e1).life.to_f32(), 500.0 - 25.0, "10 + 5 + 10 bonus damage");
    assert_eq!(g.unit(c).mana.to_f32(), before - 16.0, "two orbs cost 8 each");
    assert!(g.buffs(e1).iter().any(|b| b.base == BUFF_COLD_ARROWS));
    assert!(g.buffs(e1).iter().any(|b| b.base == BUFF_POISON_ARROWS));
    let cold = g.buffs(e1).iter().find(|b| b.base == BUFF_COLD_ARROWS).cloned().unwrap();
    match cold.effect {
        Some(sim::auras::BuffEffect::Mods(m)) => {
            assert!((m.move_delta.to_f32() + 0.3).abs() < 1e-6 && (m.attack_delta.to_f32() + 0.3).abs() < 1e-6)
        }
        other => panic!("expected stat mods, got {other:?}"),
    }
}

#[test]
fn ranger_arrows_need_the_mana() {
    let mut h = host();
    give(&mut h, SEARING_ARROWS);
    let (c, e1) = (unit(&h, "caster"), unit(&h, "e1"));
    let mut g = h.game_mut();
    g.write_mana(c, f(5.0));
    assert!(g.hero_orb_on_hit(c, e1).is_empty());
    assert_eq!(g.unit(e1).life.to_f32(), 500.0);
}

#[test]
fn rally_point_is_stored() {
    let mut h = host();
    let c = unit(&h, "caster");
    assert!(h.game().rally_point(c).is_none());
    h.game_mut().set_rally_point(c, f(10.0), f(20.0));
    assert_eq!(h.game().rally_point(c).map(|p| (p.0.to_f32(), p.1.to_f32())), Some((10.0, 20.0)));
    assert_ne!(RALLY, NEUTRAL_SPELL);
}
