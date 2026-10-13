//! Spell buff integration: Storm Bolt stun, Slow movement reduction, through the game loop.
//! Adapted from spell_cast.rs with focus on buff application and expiry behavior.

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
native GetWidgetLife takes widget wid returns real
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit tgt = null
    unit hero_tgt = null
    boolean accepted = false
endglobals

function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'hmpr', 0.0, 0.0, 0.0)
    set tgt = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set hero_tgt = CreateUnit(Player(1), 'HPal', 400.0, 0.0, 0.0)
    call UnitAddAbility(caster, 'AHtb')
    call UnitAddAbility(caster, 'Aslo')
    call SetUnitState(caster, 2, 300.0)
    call SetUnitState(tgt, 2, 500.0)
    call SetUnitState(hero_tgt, 2, 500.0)
endfunction

function bolt takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852095, tgt)
endfunction

function slow takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852075, tgt)
endfunction

function bolt_hero takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852095, hero_tgt)
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
                unit_id: 0,
            }],
        );
        // Set Slow levels: (dur=10.0, hero_dur=4.0, move_delta=0.6, attack_delta=0.6)
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
                unit_id: 0,
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

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().unit(k).buffs.iter().any(|b| b.base == code)
}

#[test]
fn storm_bolt_applies_stun_buff_with_dur_for_non_hero() {
    let mut h = host();
    let t = unit(&h, "tgt");

    call(&mut h, "bolt");
    // Missile flies 300 units at 1000 speed: ~5 ticks to impact
    run(&mut h, 5);

    // Verify stun buff applied
    assert!(has_buff(&h, t, BUFF_STUN), "target should have stun buff after Storm Bolt impact");
    assert_eq!(h.game().unit(t).bonuses.stun, 1, "stun counter incremented");
    assert!(h.game().is_stunned(t), "is_stunned should return true");
}

#[test]
fn stun_buff_expires_after_dur_for_non_hero() {
    let mut h = host();
    let t = unit(&h, "tgt");

    call(&mut h, "bolt");
    run(&mut h, 5); // Impact and apply stun

    assert_eq!(h.game().unit(t).bonuses.stun, 1, "stun applied");
    assert!(h.game().is_stunned(t));

    // Dur=5.0s = 50 ticks; still stunned at 4.5s (45 ticks)
    run(&mut h, 40);
    assert!(h.game().is_stunned(t), "stun active before expiry");

    // Expire after 5.6s (56 ticks total); 5 + 40 + 12 = 57
    run(&mut h, 12);
    assert_eq!(h.game().unit(t).bonuses.stun, 0, "stun expired after Dur");
    assert!(!has_buff(&h, t, BUFF_STUN), "buff removed from list");
}

#[test]
fn hero_target_uses_hero_dur_for_stun() {
    let mut h = host();
    let t = unit(&h, "hero_tgt");

    // Verify it's a hero type
    h.game_mut().unit_mut(t).type_id = HERO;

    call(&mut h, "bolt_hero");
    run(&mut h, 5); // Impact

    assert_eq!(h.game().unit(t).bonuses.stun, 1, "hero stunned");

    // HeroDur=3.0s = 30 ticks; expire after 3.6s (36 ticks total)
    run(&mut h, 30);
    assert_eq!(h.game().unit(t).bonuses.stun, 0, "stun expired after HeroDur");
}

#[test]
fn slow_applies_speed_reduction() {
    let mut h = host();
    let t = unit(&h, "tgt");

    let base_speed = h.game().unit(t).move_speed.to_f32();
    call(&mut h, "slow");
    run(&mut h, 5); // Impact and apply slow

    assert!(has_buff(&h, t, BUFF_SLOW), "slow buff applied");
    let slowed_speed = h.game().unit(t).move_speed.to_f32();
    assert!(slowed_speed < base_speed, "speed reduced (base={}, slowed={})", base_speed, slowed_speed);
}

#[test]
fn slow_restores_speed_after_expiry() {
    let mut h = host();
    let t = unit(&h, "tgt");

    let base_speed = h.game().unit(t).move_speed.to_f32();
    call(&mut h, "slow");
    run(&mut h, 5); // Impact at tick 5
    let slowed_speed = h.game().unit(t).move_speed.to_f32();

    // Dur=10.0s = 100 ticks; buff applied at tick 5, expires at tick 105
    // Run for 100 more ticks to reach tick 105 (past expiry)
    run(&mut h, 100);

    let restored_speed = h.game().unit(t).move_speed.to_f32();
    assert!(
        (restored_speed - base_speed).abs() < 0.1,
        "speed restored (base={}, restored={}, diff={})",
        base_speed,
        restored_speed,
        (restored_speed - base_speed).abs()
    );
    assert!(!has_buff(&h, t, BUFF_SLOW), "slow buff removed");
}
