//! Behaviour tests of the shared invisibility model (`invisibility.rs`, spec `docs/spec/abilities-invisibility.md`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::invisibility::*;
use sim::natives::game::{ObjKey, UnitType};
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
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set sorc = CreateUnit(Player(0), 'nsrc', 0.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 3000.0, 0.0, 0.0)
    set ghost = CreateUnit(Player(0), 'ngho', 6000.0, 0.0, 0.0)
    set melder = CreateUnit(Player(0), 'nmel', 8000.0, 0.0, 0.0)
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
            (id(b"nsee"), vec![DETECTORS[2]]),
        ] {
            g.unit_types_mut().insert(t, ut(500.0, ab));
        }
        g.set_spell_levels(
            INVISIBILITY,
            vec![SpellLevel { cost: 50, cool: f(1.0), dur: f(120.0), hero_dur: f(120.0), rng: f(300.0), ..Default::default() }],
        );
        g.set_spell_levels(PERM_INVIS, vec![SpellLevel { dur: f(2.0), ..Default::default() }]);
        g.set_spell_levels(SHADOW_MELD, vec![SpellLevel::default()]);
        g.set_spell_levels(DETECTORS[2], vec![SpellLevel { rng: f(900.0), ..Default::default() }]);
        for a in [INVISIBILITY, PERM_INVIS, SHADOW_MELD, DETECTORS[2]] {
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
    assert!(h.game().is_unit_invisible_for(ghost, 1));
    h.start("drop_ghost", &[]).expect("drop");
    run(&mut h, 2);
    assert_eq!(h.game().invisibility_count(ghost), 0);
}

#[test]
fn shadow_meld_hides_an_idle_unit_at_night_only() {
    let h = started();
    let melder = unit(&h, "melder");
    h.game_mut().set_time_of_day(F32::from_bits(12.0f32.to_bits()));
    assert!(!h.game().is_night());
    assert!(!h.game().is_invisible(melder), "no shadow meld by day");
    h.game_mut().set_time_of_day(F32::ZERO);
    assert!(h.game().is_night());
    assert!(h.game().is_invisible(melder), "idle at night");
}
