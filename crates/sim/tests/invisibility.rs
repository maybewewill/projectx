//! Behaviour tests of the shared invisibility model (`invisibility.rs`, spec `docs/spec/abilities-invisibility.md`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::invisibility::*;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::combat::AttackType;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
"#;

const SRC: &str = r#"
globals
    unit sorc = null
    unit friend = null
    unit foe = null
    unit seer = null
    unit ghost = null
    unit melder = null
    unit ghost2 = null
    unit visible = null
    unit ultra = null
    unit sentry = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set sorc = CreateUnit(Player(0), 'nsrc', 0.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 3000.0, 0.0, 0.0)
    set ghost = CreateUnit(Player(0), 'ngho', 6000.0, 0.0, 0.0)
    set melder = CreateUnit(Player(0), 'nmel', 8000.0, 0.0, 0.0)
    set ghost2 = CreateUnit(Player(0), 'nghs', 10000.0, 0.0, 0.0)
    set visible = CreateUnit(Player(0), 'neth', 11000.0, 0.0, 0.0)
    set ultra = CreateUnit(Player(0), 'nult', 12000.0, 0.0, 0.0)
endfunction
function setup_sentry takes nothing returns nothing
    set sentry = CreateUnit(Player(1), 'nsen', 10100.0, 0.0, 0.0)
endfunction
function foe_stop takes nothing returns nothing
    call IssueImmediateOrderById(foe, 851972)
endfunction
function foe_attacks takes nothing returns nothing
    call IssueTargetOrder(foe, "attack", friend)
endfunction
function setup_seer takes nothing returns nothing
    set seer = CreateUnit(Player(1), 'nsee', 100.0, 400.0, 0.0)
endfunction
function cast_invis takes nothing returns nothing
    set accepted = IssueTargetOrderById(sorc, 852069, friend)
endfunction
function drop_ghost takes nothing returns nothing
    call UnitRemoveAbility(ghost, 'Apiv')
endfunction
"#;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn ut(life: f32, abilities: Vec<u32>) -> UnitType {
    UnitType {
        name: "unit".to_string(),
        max_life: f(life),
        max_mana: f(300.0),
        start_mana: f(300.0),
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
        let id = |s: &[u8; 4]| u32::from_be_bytes(*s);
        for (t, ab) in [
            (id(b"hfoo"), vec![]),
            (id(b"nsrc"), vec![INVISIBILITY]),
            (id(b"ngho"), vec![PERM_INVIS]),
            (id(b"nmel"), vec![SHADOW_MELD]),
            (id(b"nghs"), vec![GHOST]),
            (id(b"neth"), vec![GHOST_VISIBLE]),
            (id(b"nult"), vec![ULTRAVISION]),
            (id(b"nsen"), vec![sim::invisibility::DETECTORS[0]]),
            (id(b"nsee"), vec![DETECTORS[2]]),
        ] {
            g.unit_types_mut().insert(t, ut(500.0, ab));
        }
        g.set_spell_levels(
            INVISIBILITY,
            vec![SpellLevel { cost: 50, cool: f(1.0), dur: f(120.0), hero_dur: f(120.0), rng: f(300.0), ..Default::default() }],
        );
        g.set_spell_levels(PERM_INVIS, vec![SpellLevel { dur: f(2.0), ..Default::default() }]);
        g.set_spell_levels(SHADOW_MELD, vec![SpellLevel { data: [f(1.5), f(2.5), f(0.5), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO], ..Default::default() }]);
        g.set_spell_levels(GHOST, vec![SpellLevel { data: [f(1.0), f(1.0), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO], ..Default::default() }]);
        g.set_spell_levels(sim::invisibility::DETECTORS[0], vec![SpellLevel { rng: f(1100.0), data: [f(2.0), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO], ..Default::default() }]);
        g.set_type_orders(id(b"hfoo"), TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        g.set_attack_type(
            id(b"hfoo"),
            AttackType { cooldown: f(1.0), damage_point: f(0.3), backswing: f(0.3), range: f(100.0), collision: f(0.5), damage: f(10.0), missile_speed: None, ..Default::default() },
        );
        let mut ty = ut(500.0, vec![ULTRAVISION]);
        ty.sight = f(1800.0);
        ty.night_sight = f(800.0);
        g.unit_types_mut().insert(id(b"nult"), ty);
        g.set_spell_levels(DETECTORS[2], vec![SpellLevel { rng: f(900.0), ..Default::default() }]);
        for a in [INVISIBILITY, PERM_INVIS, SHADOW_MELD, DETECTORS[2], GHOST, DETECTORS[0]] {
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

#[test]
fn sorceress_invisibility_hides_a_friend_from_the_enemy_for_the_duration() {
    let mut h = started();
    let (friend, foe) = (unit(&h, "friend"), unit(&h, "foe"));
    assert!(!h.game().is_invisible(friend));
    h.start("cast_invis", &[]).expect("cast");
    run(&mut h, 40);
    assert!(h.vm().global("accepted").and_then(|v| v.as_boolean()).unwrap());
    let g = h.game();
    assert_eq!(g.invisibility_count(friend), 1);
    assert!(g.is_unit_invisible_for(friend, 1), "the enemy cannot see it");
    assert!(!g.is_unit_invisible_for(friend, 0), "its owner can");
    let _ = foe;
    drop(g);
    // 120 s later the buff has run out (one batch of 20 ticks is 0.1 s).
    run(&mut h, 1300);
    assert!(!h.game().is_invisible(friend), "the buff ended");
}

#[test]
fn a_detector_of_the_enemy_reveals_an_invisible_unit_in_range() {
    let mut h = started();
    let friend = unit(&h, "friend");
    h.start("cast_invis", &[]).expect("cast");
    run(&mut h, 40);
    assert!(h.game().is_unit_invisible_for(friend, 1));
    h.start("setup_seer", &[]).expect("seer");
    run(&mut h, 2);
    let g = h.game();
    assert_eq!(g.detecting_players(friend), 1 << 1);
    assert!(!g.is_unit_invisible_for(friend, 1), "true sight shows it");
}

#[test]
fn a_permanent_invisibility_ability_lasts_until_it_is_removed() {
    let mut h = started();
    let ghost = unit(&h, "ghost");
    assert_eq!(h.game().invisibility_count(ghost), 1);
    assert!(!h.game().is_invisible(ghost), "the fade-in of Permanent Invisibility (Dur 2.0) is still running");
    run(&mut h, 25);
    assert!(h.game().is_unit_invisible_for(ghost, 1));
    h.start("drop_ghost", &[]).expect("drop");
    run(&mut h, 2);
    assert_eq!(h.game().invisibility_count(ghost), 0);
}

#[test]
fn shadow_meld_hides_an_idle_unit_at_night_only() {
    let mut h = started();
    let melder = unit(&h, "melder");
    h.game_mut().set_time_of_day(F32::from_bits(12.0f32.to_bits()));
    assert!(!h.game().is_night());
    run(&mut h, 40);
    assert!(!h.game().is_invisible(melder), "no shadow meld by day");
    h.game_mut().set_time_of_day(F32::ZERO);
    assert!(h.game().is_night());
    run(&mut h, 8);
    assert!(!h.game().is_invisible(melder), "idle for the action duration (0.5 s) and the fade (1.5 s) first");
    run(&mut h, 20);
    assert!(h.game().is_invisible(melder), "idle at night");
    h.game_mut().set_time_of_day(F32::from_bits(12.0f32.to_bits()));
    run(&mut h, 3);
    assert!(!h.game().is_invisible(melder), "the day ends the meld");
}

#[test]
fn a_ghost_is_invisible_after_its_fade_and_the_visible_ghost_form_is_not() {
    let mut h = started();
    let (ghost, visible) = (unit(&h, "ghost2"), unit(&h, "visible"));
    assert_eq!(h.game().invisibility_count(ghost), 1, "Agho counts once");
    assert_eq!(h.game().invisibility_count(visible), 0, "Aeth is the visible ghost: no invisibility");
    run(&mut h, 3);
    assert!(h.game().is_unit_invisible_for(ghost, 1));
    assert!(!h.game().is_unit_invisible_for(visible, 1));
    assert!(!h.game().sees_ghosts(ghost), "an Aeth carrier is no seer of anything");
}

#[test]
fn a_detector_without_the_invisible_bit_does_not_reveal() {
    let mut h = started();
    let ghost = unit(&h, "ghost2");
    run(&mut h, 3);
    h.start("setup_sentry", &[]).expect("sentry");
    run(&mut h, 2);
    assert_eq!(h.game().detecting_players(ghost), 0, "detection type 2 is the burrowed bit only");
    assert!(h.game().is_unit_invisible_for(ghost, 1));
}

#[test]
fn an_enemy_cannot_order_an_attack_on_an_unseen_unit() {
    let mut h = started();
    let (friend, foe) = (unit(&h, "friend"), unit(&h, "foe"));
    h.start("foe_attacks", &[]).expect("attack");
    run(&mut h, 2);
    assert!(!h.game().unit_order_ids(foe).is_empty(), "a visible target is accepted");
    h.game_mut().unit_mut(foe).paused = false;
    h.start("cast_invis", &[]).expect("cast");
    run(&mut h, 40);
    assert!(h.game().is_unit_invisible_for(friend, 1));
    // The order is dropped before it replaces the old one: stop the old one first by clearing the unit's orders.
    h.start("foe_stop", &[]).expect("stop");
    run(&mut h, 2);
    h.start("foe_attacks", &[]).expect("attack");
    run(&mut h, 2);
    assert!(h.game().unit_order_ids(foe).is_empty(), "the order at an invisible unit is dropped");
}

#[test]
fn ultravision_gives_the_day_sight_at_night() {
    let h = started();
    let ultra = unit(&h, "ultra");
    assert_eq!(h.game().unit(ultra).night_sight, f(1800.0));
}
