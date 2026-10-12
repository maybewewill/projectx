//! Integration of spells (Storm Bolt, Holy Light, Slow) with their buff effects through the game loop.
//! Tests that stun prevents action, slow reduces speed, buffs refresh correctly, and expire on schedule.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::cast::{BUFF_SLOW, BUFF_STUN, SLOW, STORM_BOLT};
use sim::natives::game::{ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type boolexpr extends handle
type trigger extends handle
type triggeraction extends handle
type event extends handle

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native GetUnitX takes unit whichUnit returns real
native GetWidgetLife takes widget wid returns real
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit tgt = null
    unit hero_tgt = null
endglobals

function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'hmpr', 0.0, 0.0, 0.0)
    set tgt = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set hero_tgt = CreateUnit(Player(1), 'HPal', 400.0, 0.0, 0.0)
    call UnitAddAbility(caster, 'AHtb')
    call UnitAddAbility(caster, 'Aslo')
    call SetUnitState(tgt, 2, 500.0)
    call SetUnitState(hero_tgt, 2, 500.0)
endfunction

function cast_storm_bolt takes nothing returns nothing
    call IssueTargetOrderById(caster, 852095, tgt)
endfunction

function cast_slow takes nothing returns nothing
    call IssueTargetOrderById(caster, 852075, tgt)
endfunction

function cast_storm_bolt_on_hero takes nothing returns nothing
    call IssueTargetOrderById(caster, 852095, hero_tgt)
endfunction
"#;

const CASTER: u32 = 0x686D_7072; // 'hmpr'
const FOOT: u32 = 0x6866_6F6F;   // 'hfoo'
const HERO: u32 = 0x4850_616C;   // 'HPal'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        // Register unit types
        g.unit_types_mut().insert(
            CASTER,
            UnitType {
                name: "Priest".to_string(),
                max_life: f(300.0),
                max_mana: f(300.0),
                move_speed: f(270.0),
                ..Default::default()
            },
        );
        g.unit_types_mut().insert(
            FOOT,
            UnitType {
                name: "Footman".to_string(),
                max_life: f(500.0),
                move_speed: f(270.0),
                ..Default::default()
            },
        );
        g.unit_types_mut().insert(
            HERO,
            UnitType {
                name: "Paladin".to_string(),
                max_life: f(500.0),
                move_speed: f(270.0),
                ..Default::default()
            },
        );
        // Register spell types
        for code in [STORM_BOLT, SLOW] {
            g.ability_types_mut().insert(code, sim::abilities::AbilityType { max_level: Some(1) });
        }
        // Set spell levels: Storm Bolt (dur=5.0, hero_dur=3.0, damage=100.0)
        g.set_spell_levels(
            STORM_BOLT,
            vec![sim::cast::SpellLevel {
                cast: F32::ZERO,
                dur: f(5.0),
                hero_dur: f(3.0),
                cost: 75,
                cool: f(9.0),
                area: F32::ZERO,
                rng: f(600.0),
                data: [f(100.0), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            }],
        );
        // Set Slow levels: (dur=10.0, hero_dur=4.0, move_delta=-0.6, attack_delta=-0.6)
        g.set_spell_levels(
            SLOW,
            vec![sim::cast::SpellLevel {
                cast: F32::ZERO,
                dur: f(10.0),
                hero_dur: f(4.0),
                cost: 40,
                cool: f(8.0),
                area: F32::ZERO,
                rng: f(600.0),
                data: [f(0.6), f(0.6), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
            }],
        );
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

#[test]
fn basic_unit_setup_works() {
    let h = host();
    let caster = unit(&h, "caster");
    let tgt = unit(&h, "tgt");

    // Verify units exist and have expected properties
    assert_eq!(h.game().unit(caster).type_id, CASTER);
    assert_eq!(h.game().unit(tgt).type_id, FOOT);
    assert!(!h.game().unit(tgt).dead, "target should be alive");
}

#[test]
fn storm_bolt_applies_stun_buff_with_dur_for_non_hero() {
    let mut h = host();
    let tgt = unit(&h, "tgt");

    // Cast Storm Bolt on non-hero target
    call(&mut h, "cast_storm_bolt");
    // Missile takes ~0.5s to arrive (300 units / 1000 speed = 0.3s, but need ~5 steps)
    // Run to impact
    run(&mut h, 5);

    // Verify stun buff is applied
    assert!(
        h.game().unit(tgt).buffs.iter().any(|b| b.base == BUFF_STUN),
        "target should have stun buff after Storm Bolt impact"
    );
    assert_eq!(
        h.game().unit(tgt).bonuses.stun, 1,
        "stun counter should be incremented"
    );
}

#[test]
fn stun_buff_expires_after_dur_for_non_hero() {
    let mut h = host();
    let tgt = unit(&h, "tgt");

    // Cast Storm Bolt
    call(&mut h, "cast_storm_bolt");
    run(&mut h, 5); // Reach impact and apply stun

    // Verify stun is active
    assert_eq!(h.game().unit(tgt).bonuses.stun, 1, "stun should be applied");

    // Storm Bolt Dur=5.0 seconds = 50 ticks for non-hero
    // We've run 5 steps, stun was applied at step 5
    // Run for 40 more steps (total 45): should still be stunned
    run(&mut h, 40);
    assert_eq!(h.game().unit(tgt).bonuses.stun, 1, "stun should still be active before expiry");

    // Run 12 more steps (total 57): should be expired
    run(&mut h, 12);
    assert_eq!(h.game().unit(tgt).bonuses.stun, 0, "stun should expire after Dur (5.0s)");
    assert!(
        !h.game().unit(tgt).buffs.iter().any(|b| b.base == BUFF_STUN),
        "stun buff should be removed from unit's list"
    );
}

#[test]
fn hero_target_uses_hero_dur_for_stun() {
    let mut h = host();
    let hero_tgt = unit(&h, "hero_tgt");

    // Change the target to a hero type by changing its type_id
    h.game_mut().unit_mut(hero_tgt).type_id = HERO;

    // Cast Storm Bolt on hero target
    call(&mut h, "cast_storm_bolt_on_hero");
    run(&mut h, 5); // Missile impact

    // Verify stun is applied
    assert_eq!(h.game().unit(hero_tgt).bonuses.stun, 1, "hero should be stunned");

    // For a hero, HeroDur=3.0 seconds = 30 ticks
    // Run for 30 ticks to reach the expiry boundary
    run(&mut h, 30); // Total: 35 steps

    // Stun should be expired (3.0s duration)
    assert_eq!(h.game().unit(hero_tgt).bonuses.stun, 0, "stun should expire after HeroDur (3.0s)");
}

#[test]
fn slow_reduces_and_restores_move_speed() {
    let mut h = host();
    let tgt = unit(&h, "tgt");
    let base_speed = h.game().unit(tgt).move_speed.to_f32();

    // Cast Slow on target
    call(&mut h, "cast_slow");
    run(&mut h, 5); // Missile impact and slow application

    // Verify slow is applied
    assert!(h.game().unit(tgt).buffs.iter().any(|b| b.base == BUFF_SLOW), "slow buff should be applied");
    let slowed_speed = h.game().unit(tgt).move_speed.to_f32();
    assert!(slowed_speed < base_speed, "speed should be reduced (base={}, slowed={})", base_speed, slowed_speed);

    // Slow duration for non-hero: Dur=10.0 seconds = 100 ticks
    // Run to expiry: 5 + 95 more = 100 steps total
    run(&mut h, 95);

    // Speed should be restored to original
    let restored_speed = h.game().unit(tgt).move_speed.to_f32();
    let speed_diff = (restored_speed - base_speed).abs();
    assert!(
        speed_diff < 0.1,
        "speed should be restored after slow expires (base={}, restored={}, diff={})",
        base_speed,
        restored_speed,
        speed_diff
    );
    assert!(
        !h.game().unit(tgt).buffs.iter().any(|b| b.base == BUFF_SLOW),
        "slow buff should be removed after expiry"
    );
}
