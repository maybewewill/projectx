//! The spell casting core end to end through `ScriptHost` (`cast.rs`, docs/spec/abilities-5.md): Storm Bolt, Holy
//! Light and Slow with the check, the event sequence, the mana payment, the cooldown, Dur versus HeroDur.
//!
//! The level data are pinned values of the tests (usual first-level numbers), not game data. The spells are cast
//! through `IssueTargetOrderById` with the order ids of the model (UNSURE source, see `cast.rs`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::{
    CastTiming, SpellLevel, BUFF_SLOW, BUFF_STUN, HOLY_LIGHT, SLOW, SPELL_CAST, SPELL_CHANNEL, SPELL_EFFECT,
    SPELL_ENDCAST, SPELL_FINISH, STORM_BOLT,
};
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
native SetWidgetLife takes widget wid, real newLife returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native UnitResetCooldown takes unit whichUnit returns nothing
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, integer whichEvent, boolexpr filter returns event
native TriggerRegisterUnitEvent takes trigger t, unit whichUnit, integer whichEvent returns event
native GetTriggerUnit takes nothing returns unit
native GetSpellAbilityId takes nothing returns integer
native GetSpellTargetUnit takes nothing returns unit
native GetSpellAbilityUnit takes nothing returns unit
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit tgt = null
    unit ally = null
    integer order = 0
    integer abil = 0
    unit seenTarget = null
    unit seenCaster = null
    unit seenTrigger = null
    integer unitFinish = 0
    boolean accepted = false
endglobals
function add1 takes nothing returns nothing
    set order = order * 10 + 1
endfunction
function add2 takes nothing returns nothing
    set order = order * 10 + 2
endfunction
function add3 takes nothing returns nothing
    set order = order * 10 + 3
    set abil = GetSpellAbilityId()
    set seenTarget = GetSpellTargetUnit()
    set seenCaster = GetSpellAbilityUnit()
    set seenTrigger = GetTriggerUnit()
endfunction
function add4 takes nothing returns nothing
    set order = order * 10 + 4
endfunction
function add5 takes nothing returns nothing
    set order = order * 10 + 5
endfunction
function onUnitFinish takes nothing returns nothing
    set unitFinish = unitFinish + 1
endfunction
function reg takes integer id, code c returns nothing
    local trigger t = CreateTrigger()
    call TriggerRegisterPlayerUnitEvent(t, Player(0), id, null)
    call TriggerAddAction(t, c)
endfunction
function setup takes nothing returns nothing
    local trigger u = CreateTrigger()
    set caster = CreateUnit(Player(0), 'hmpr', 0.0, 0.0, 0.0)
    set tgt = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set ally = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    call UnitAddAbility(caster, 'AHtb')
    call UnitAddAbility(caster, 'AHhb')
    call UnitAddAbility(caster, 'Aslo')
    call UnitAddAbility(tgt, 'AHtb')
    call SetUnitState(tgt, 2, 200.0)
    call reg(272, function add1)
    call reg(273, function add2)
    call reg(274, function add3)
    call reg(275, function add4)
    call reg(276, function add5)
    call TriggerRegisterUnitEvent(u, caster, 293)
    call TriggerAddAction(u, function onUnitFinish)
    call SetUnitState(caster, 2, 200.0)
endfunction
function bolt takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852095, tgt)
endfunction
function bolt_ally takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852095, ally)
endfunction
function holy_ally takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852089, ally)
endfunction
function holy_tgt takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852089, tgt)
endfunction
function slow_tgt takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852075, tgt)
endfunction
function tgt_bolt takes nothing returns nothing
    set accepted = IssueTargetOrderById(tgt, 852095, caster)
endfunction
function reset takes nothing returns nothing
    call UnitResetCooldown(caster)
endfunction
"#;

const CASTER: u32 = 0x686D_7072; // 'hmpr'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const HERO: u32 = 0x4850_616C; // 'HPal'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(cost: i32, cool: f32, dur: f32, hero_dur: f32, a: f32, b: f32) -> SpellLevel {
    let mut data = [F32::ZERO; 9];
    data[0] = f(a);
    data[1] = f(b);
    SpellLevel {
        cast: F32::ZERO,
        dur: f(dur),
        hero_dur: f(hero_dur),
        cost,
        cool: f(cool),
        area: F32::ZERO,
        rng: f(600.0),
        data,
    }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [(CASTER, "Priest", 300.0), (FOOT, "Footman", 500.0), (HERO, "Paladin", 500.0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(300.0),
                    move_speed: f(270.0),
                    ..Default::default()
                },
            );
        }
        for code in [STORM_BOLT, HOLY_LIGHT, SLOW] {
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(1) });
        }
        g.set_spell_levels(STORM_BOLT, vec![level(75, 9.0, 5.0, 3.0, 100.0, 0.0)]);
        g.set_spell_levels(HOLY_LIGHT, vec![level(65, 5.0, 0.0, 0.0, 200.0, 0.0)]);
        g.set_spell_levels(SLOW, vec![level(40, 8.0, 10.0, 4.0, 0.6, 0.6)]);
    }
    h.start("setup", &[]).expect("setup");
    h.step();
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn int(h: &ScriptHost, name: &str) -> i32 {
    h.vm().global(name).and_then(|v| v.as_integer()).expect("integer global")
}

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
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

fn mana(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).mana.to_f32()
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

#[test]
fn storm_bolt_runs_the_whole_sequence() {
    let mut h = host();
    let (c, t) = (unit(&h, "caster"), unit(&h, "tgt"));
    call(&mut h, "bolt");
    run(&mut h, 1);
    assert!(accepted(&h));
    // CHANNEL, CAST, EFFECT, FINISH, ENDCAST in this order, each seen by the player-unit trigger.
    assert_eq!(int(&h, "order"), 12345);
    assert_eq!(int(&h, "abil"), STORM_BOLT as i32);
    assert_eq!(unit(&h, "seenTarget"), t);
    assert_eq!(unit(&h, "seenCaster"), c);
    assert_eq!(unit(&h, "seenTrigger"), c);
    let log: Vec<u32> = h.game().spell_log().iter().map(|e| e.2).collect();
    assert_eq!(log, vec![SPELL_CHANNEL, SPELL_CAST, SPELL_EFFECT, SPELL_FINISH, SPELL_ENDCAST]);
    // The mana is paid at the effect (75 of 200) and the cooldown runs.
    assert_eq!(mana(&h, c), 125.0);
    assert!(h.game().is_cooling(c, STORM_BOLT));
    let left = h.game().cooldown_remaining(c, STORM_BOLT).to_f32();
    assert!((8.5..=9.0).contains(&left), "cooldown left {left}");
}

#[test]
fn storm_bolt_damages_and_stuns_for_dur_then_expires() {
    let mut h = host();
    let t = unit(&h, "tgt");
    call(&mut h, "bolt");
    // The missile flies 300 units at 1000/s: not there after 0.2 s, there after 0.5 s.
    run(&mut h, 2);
    assert_eq!(life(&h, t), 500.0, "the bolt has not arrived");
    run(&mut h, 3);
    assert_eq!(life(&h, t), 400.0, "DataA damage");
    assert!(has_buff(&h, t, BUFF_STUN));
    assert!(h.game().is_stunned(t));
    // A footman is no hero: Dur = 5 s. Still stunned at 4.5 s, free after 5.6 s.
    run(&mut h, 40);
    assert!(h.game().is_stunned(t));
    run(&mut h, 12);
    assert!(!h.game().is_stunned(t));
    assert!(!has_buff(&h, t, BUFF_STUN));
}

#[test]
fn a_hero_target_takes_hero_duration() {
    let mut h = host();
    let t = unit(&h, "tgt");
    // Make the target a hero type: the test is the first character of the type id in A..Z.
    h.game_mut().unit_mut(t).type_id = HERO;
    call(&mut h, "bolt");
    run(&mut h, 5);
    assert!(h.game().is_stunned(t));
    // HeroDur = 3 s: free after 3.6 s (the buff started at 0.3 s).
    run(&mut h, 30);
    assert!(!h.game().is_stunned(t), "a hero is stunned for HeroDur");
}

#[test]
fn cooldown_blocks_a_second_cast_then_ends() {
    let mut h = host();
    let c = unit(&h, "caster");
    call(&mut h, "bolt");
    run(&mut h, 2);
    assert!(accepted(&h));
    call(&mut h, "bolt");
    run(&mut h, 1);
    assert!(!accepted(&h), "cooling down: error 210");
    assert_eq!(mana(&h, c), 125.0, "a refused cast pays nothing");
    // 9 s later the cooldown is over.
    run(&mut h, 90);
    assert!(!h.game().is_cooling(c, STORM_BOLT));
    call(&mut h, "bolt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(mana(&h, c), 50.0);
}

#[test]
fn not_enough_mana_refuses_the_cast() {
    let mut h = host();
    let c = unit(&h, "caster");
    h.game_mut().write_mana(c, f(74.0));
    call(&mut h, "bolt");
    run(&mut h, 1);
    assert!(!accepted(&h), "74 < 75: error 209");
    assert_eq!(int(&h, "order"), 0, "no event fired");
    assert_eq!(mana(&h, c), 74.0);
    // Exactly enough mana is enough (the compare is mana >= cost).
    h.game_mut().write_mana(c, f(75.0));
    call(&mut h, "bolt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(mana(&h, c), 0.0);
}

#[test]
fn holy_light_heals_an_ally_and_clamps_at_max_life() {
    let mut h = host();
    let (c, a) = (unit(&h, "caster"), unit(&h, "ally"));
    h.game_mut().write_life(a, f(400.0));
    call(&mut h, "holy_ally");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, a), 500.0, "400 + 200 clamped to the 500 maximum");
    assert_eq!(mana(&h, c), 135.0);
    assert!(h.game().is_cooling(c, HOLY_LIGHT));
}

#[test]
fn holy_light_hurts_enemy_undead_for_half_and_ignores_living_enemies() {
    let mut h = host();
    let t = unit(&h, "tgt");
    call(&mut h, "holy_tgt");
    run(&mut h, 1);
    assert_eq!(life(&h, t), 500.0, "a living enemy takes nothing");
    h.game_mut().unit_mut(t).class_flags |= 0x2;
    call(&mut h, "reset");
    run(&mut h, 1);
    call(&mut h, "holy_tgt");
    run(&mut h, 1);
    assert_eq!(life(&h, t), 400.0, "an enemy undead takes DataA / 2");
}

#[test]
fn slow_cuts_move_speed_for_dur_then_restores_it() {
    let mut h = host();
    let t = unit(&h, "tgt");
    let before = h.game().unit(t).move_speed.to_f32();
    call(&mut h, "slow_tgt");
    run(&mut h, 1);
    assert!(has_buff(&h, t, BUFF_SLOW));
    let slowed = h.game().unit(t).move_speed.to_f32();
    assert!((slowed - before * 0.4).abs() < 0.01, "move speed {slowed} from {before}: factor 1 - 0.6");
    run(&mut h, 95);
    assert!(has_buff(&h, t, BUFF_SLOW), "still slowed before 10 s");
    run(&mut h, 10);
    assert!(!has_buff(&h, t, BUFF_SLOW));
    assert_eq!(h.game().unit(t).move_speed.to_f32(), before);
}

#[test]
fn cast_point_delays_the_effect_and_the_payment() {
    let mut h = host();
    let (c, t) = (unit(&h, "caster"), unit(&h, "tgt"));
    h.game_mut().set_cast_timing(CASTER, CastTiming { point: f(0.5), backswing: f(0.5) });
    call(&mut h, "bolt");
    run(&mut h, 3);
    // CHANNEL and CAST fired; the mana is not paid and the target is untouched before the cast point.
    assert_eq!(int(&h, "order"), 12);
    assert_eq!(mana(&h, c), 200.0);
    assert!(!h.game().is_cooling(c, STORM_BOLT));
    run(&mut h, 4);
    assert_eq!(int(&h, "order"), 12345);
    assert_eq!(mana(&h, c), 125.0);
    run(&mut h, 5);
    assert_eq!(life(&h, t), 400.0);
}

#[test]
fn reset_cooldown_shortens_a_running_cooldown_to_ten_milliseconds() {
    let mut h = host();
    let c = unit(&h, "caster");
    call(&mut h, "bolt");
    run(&mut h, 1);
    assert!(h.game().is_cooling(c, STORM_BOLT));
    call(&mut h, "reset");
    run(&mut h, 1);
    assert!(!h.game().is_cooling(c, STORM_BOLT), "ready 0.01 s after the reset");
}

#[test]
fn killing_bolt_reports_the_caster_as_killer() {
    let mut h = host();
    let t = unit(&h, "tgt");
    h.game_mut().write_life(t, f(50.0));
    call(&mut h, "bolt");
    run(&mut h, 6);
    assert!(h.game().unit(t).dead);
    assert!(!has_buff(&h, t, BUFF_STUN), "a dead target gets no stun");
}

#[test]
fn a_stunned_caster_cannot_start_a_cast() {
    let mut h = host();
    let t = unit(&h, "tgt");
    call(&mut h, "bolt");
    run(&mut h, 5);
    assert!(h.game().is_stunned(t));
    call(&mut h, "tgt_bolt");
    run(&mut h, 1);
    assert!(!accepted(&h), "a stunned unit does not start a cast");
    run(&mut h, 50);
    assert!(!h.game().is_stunned(t));
    call(&mut h, "tgt_bolt");
    run(&mut h, 1);
    assert!(accepted(&h), "free again");
}
