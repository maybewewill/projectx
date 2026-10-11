//! `UnitDamageTarget`, `UnitDamagePoint` and `SetUnitRescuable` in the model (`docs/spec/natives-damage-items.md`).
//! The damage amount runs through the installed damage model; the tests use the default placeholder (amount passes
//! through), so they pin the native glue: argument checks, life change, killer, delay and rescue state. The
//! argument -> record mapping is checked against the original in `unit_damage_vectors.rs`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{UnitType, UnitTypes};
use sim::ScriptHost;
use softfloat::F32;

const SRC: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type trigger extends handle
type triggeraction extends handle
type event extends handle
type unitevent extends handle
type attacktype extends handle
type damagetype extends handle
type weapontype extends handle

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetHandleId takes handle h returns integer
native GetKillingUnit takes nothing returns unit
native ConvertUnitEvent takes integer i returns unitevent
native ConvertAttackType takes integer i returns attacktype
native ConvertDamageType takes integer i returns damagetype
native ConvertWeaponType takes integer i returns weapontype
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerRegisterUnitEvent takes trigger t, unit whichUnit, unitevent whichEvent returns event
native UnitDamageTarget takes unit whichUnit, widget target, real amount, boolean attack, boolean ranged, attacktype attackType, damagetype damageType, weapontype weaponType returns boolean
native UnitDamagePoint takes unit whichUnit, real delay, real radius, real x, real y, real amount, boolean attack, boolean ranged, attacktype attackType, damagetype damageType, weapontype weaponType returns boolean
native SetUnitRescuable takes unit whichUnit, player byWhichPlayer, boolean flag returns nothing
native GetUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer

globals
    unit ua = null
    unit ub = null
    unit uc = null
    trigger t = null
    integer killer_id = -1
    boolean r1 = false
    boolean r2 = false
    boolean r3 = false
    integer lvl0 = -1
    integer lvl1 = -1
endglobals
function on_death takes nothing returns nothing
    set killer_id = GetHandleId(GetKillingUnit())
endfunction
function setup takes nothing returns nothing
    set ua = CreateUnit(Player(0), 'hfoo', 0, 0, 0)
    set ub = CreateUnit(Player(1), 'hfoo', 100, 0, 180)
    set uc = CreateUnit(Player(1), 'hfoo', 1000, 0, 180)
    set t = CreateTrigger()
    call TriggerRegisterUnitEvent(t, ub, ConvertUnitEvent(53))
    call TriggerAddAction(t, function on_death)
endfunction
function hit takes nothing returns nothing
    set r1 = UnitDamageTarget(ua, ub, 30.0, false, false, ConvertAttackType(0), ConvertDamageType(4), ConvertWeaponType(0))
    set r2 = UnitDamageTarget(ua, ub, 30.0, false, false, ConvertAttackType(8), ConvertDamageType(4), ConvertWeaponType(0))
    set r3 = UnitDamageTarget(null, ub, 30.0, false, false, ConvertAttackType(0), ConvertDamageType(4), ConvertWeaponType(0))
endfunction
function kill takes nothing returns nothing
    call UnitDamageTarget(ua, ub, 500.0, true, false, ConvertAttackType(1), ConvertDamageType(0), ConvertWeaponType(0))
endfunction
function area_now takes nothing returns nothing
    set r1 = UnitDamagePoint(ua, 0.0, 150.0, 0.0, 0.0, 40.0, false, false, ConvertAttackType(0), ConvertDamageType(4), ConvertWeaponType(0))
endfunction
function area_later takes nothing returns nothing
    set r1 = UnitDamagePoint(ua, 1.0, 150.0, 0.0, 0.0, 40.0, false, false, ConvertAttackType(0), ConvertDamageType(4), ConvertWeaponType(0))
endfunction
function rescue takes nothing returns nothing
    set lvl0 = GetUnitAbilityLevel(uc, 'Arsc')
    call SetUnitRescuable(uc, Player(0), false)
    set lvl1 = GetUnitAbilityLevel(uc, 'Arsc')
endfunction
function rescue_on takes nothing returns nothing
    call SetUnitRescuable(uc, Player(0), true)
    set lvl1 = GetUnitAbilityLevel(uc, 'Arsc')
endfunction
"#;

fn host() -> ScriptHost {
    let prog = jass::parse(SRC).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        let types: &mut UnitTypes = g.unit_types_mut();
        types.insert(
            0x6866_6F6F,
            UnitType {
                name: "Footman".to_string(),
                max_life: F32::from_bits(0x42C8_0000), // 100
                ..Default::default()
            },
        );
    }
    host.start("setup", &[]).expect("setup");
    host.step();
    host
}

fn run(host: &mut ScriptHost, f: &str, steps: usize) {
    host.start(f, &[]).expect("start");
    for _ in 0..steps {
        host.step();
    }
    assert!(host.misfires().is_empty());
}

fn handle(host: &ScriptHost, name: &str) -> u32 {
    host.vm().global(name).and_then(|v| v.as_handle()).expect(name)
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm().global(name).and_then(|v| v.as_integer()).expect(name)
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm().global(name).and_then(|v| v.as_boolean()).expect(name)
}

fn life(host: &ScriptHost, name: &str) -> f32 {
    let k = host.game().unit_of_handle(handle(host, name)).expect("unit");
    host.game().unit(k).life.to_f32()
}

#[test]
fn damage_target_applies_amount_and_rejects_bad_arguments() {
    let mut host = host();
    run(&mut host, "hit", 1);
    assert!(boolean(&host, "r1"));
    assert!(!boolean(&host, "r2"), "attack type above 7 is rejected");
    assert!(!boolean(&host, "r3"), "a null source is rejected");
    assert_eq!(life(&host, "ub"), 70.0, "only the accepted call took life");
}

#[test]
fn damage_target_kill_names_the_source_as_killer() {
    let mut host = host();
    let ua = handle(&host, "ua");
    run(&mut host, "kill", 1);
    let k = host.game().unit_of_handle(handle(&host, "ub")).expect("ub");
    assert!(host.game().unit(k).dead);
    assert_eq!(int(&host, "killer_id") as u32, ua);
}

#[test]
fn damage_point_immediate_hits_every_live_unit_in_the_circle() {
    let mut host = host();
    run(&mut host, "area_now", 1);
    assert!(boolean(&host, "r1"));
    assert_eq!(life(&host, "ua"), 60.0, "the source is not excluded");
    assert_eq!(life(&host, "ub"), 60.0);
    assert_eq!(life(&host, "uc"), 100.0, "outside the radius");
}

#[test]
fn damage_point_with_a_delay_waits_for_its_timer() {
    let mut host = host();
    run(&mut host, "area_later", 1);
    assert!(boolean(&host, "r1"));
    assert_eq!(life(&host, "ub"), 100.0, "nothing before the delay");
    for _ in 0..40 {
        host.step();
    }
    assert_eq!(life(&host, "ub"), 60.0, "applied after the delay");
    assert_eq!(life(&host, "ua"), 60.0);
}

#[test]
fn set_unit_rescuable_keeps_a_per_player_mask_in_the_arsc_ability() {
    let mut host = host();
    run(&mut host, "rescue", 1);
    assert_eq!(int(&host, "lvl0"), 0);
    assert_eq!(int(&host, "lvl1"), 0, "false without the ability creates nothing");
    run(&mut host, "rescue_on", 1);
    assert_eq!(int(&host, "lvl1"), 1, "true creates the ability");
    let k = host.game().unit_of_handle(handle(&host, "uc")).expect("uc");
    let mask = host
        .game()
        .unit(k)
        .abilities
        .get(sim::abilities::RESCUE)
        .map(|a| a.mask);
    assert_eq!(mask, Some(1), "player 0's bit");
    run(&mut host, "rescue", 1);
    let mask = host.game().unit(k).abilities.get(sim::abilities::RESCUE).map(|a| a.mask);
    assert_eq!(mask, Some(0), "false clears the bit and keeps the ability");
}
