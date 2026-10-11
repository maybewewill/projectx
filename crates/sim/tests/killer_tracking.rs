//! The killer of a unit death (`combat.rs` hands the attacker to `natives/unit_event.rs`, `GetKillingUnit` reads it).
//!
//! The original takes the killer of a death from its damage path (`docs/spec/heroes.md`, section 8.3: the death
//! handler `0x6F2A4880` reads the killer through `0x6F4786B0`). The model records the attacker of the killing blow of a
//! combat swing or missile, and `KillUnit` (no damage source) keeps the killer null. The model values are not run
//! against the original, so these tests pin the model's own rule.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::combat::AttackType;
use sim::natives::game::{UnitType, UnitTypes};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type trigger extends handle
type triggeraction extends handle
type event extends handle
type unitevent extends handle

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
native KillUnit takes unit whichUnit returns nothing
native GetHandleId takes handle h returns integer
native GetKillingUnit takes nothing returns unit
native ConvertUnitEvent takes integer i returns unitevent
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerRegisterUnitEvent takes trigger t, unit whichUnit, unitevent whichEvent returns event
"#;

/// The footman type: 'hfoo'. Two footmen: `ua` (player 0) and `ub` (player 1), 100 world units apart. `on_death` is
/// the action of a unit-death trigger on `ub` (event id 53, `EVENT_UNIT_DEATH`); it stores the killer's handle id.
const SRC: &str = r#"
globals
    unit ua = null
    unit ub = null
    trigger t = null
    integer killer_id = -1
endglobals
function on_death takes nothing returns nothing
    set killer_id = GetHandleId(GetKillingUnit())
endfunction
function setup takes nothing returns nothing
    set ua = CreateUnit(Player(0), 'hfoo', 0, 0, 0)
    set ub = CreateUnit(Player(1), 'hfoo', 100, 0, 180)
    set t = CreateTrigger()
    call TriggerRegisterUnitEvent(t, ub, ConvertUnitEvent(53))
    call TriggerAddAction(t, function on_death)
endfunction
function duel takes nothing returns nothing
    call IssueTargetOrder(ua, "attack", ub)
endfunction
function plain_kill takes nothing returns nothing
    call KillUnit(ub)
endfunction
"#;

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
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
        g.set_type_orders(
            0x6866_6F6F,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
        // Cooldown 1.0 s, damage point and backswing 0.5 s, range 128 world units, collision 0.5 cells, damage 10.
        g.set_attack_type(
            0x6866_6F6F,
            AttackType {
                cooldown: F32::from_bits(0x3F80_0000),
                damage_point: F32::from_bits(0x3F00_0000),
                backswing: F32::from_bits(0x3F00_0000),
                range: F32::from_bits(0x4300_0000),
                collision: F32::from_bits(0x3F00_0000),
                damage: F32::from_bits(0x4120_0000),
                missile_speed: None,
                ..Default::default()
            },
        );
    }
    host
}

fn run(host: &mut ScriptHost, n: usize) {
    for _ in 0..n {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn global_handle(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"))
}

fn global_int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

/// Sets up the duel with `ub` one hit from death (the footman's swing is 10 damage in the combat fixtures; here the
/// life is set so that one swing kills it).
fn armed() -> ScriptHost {
    let mut host = host();
    host.start("setup", &[]).expect("setup");
    host.step();
    let ub_key = host
        .game()
        .unit_of_handle(global_handle(&host, "ub"))
        .expect("ub is a live unit");
    host.game_mut().unit_mut(ub_key).life = F32::from_bits(0x4120_0000); // 10.0
    host
}

#[test]
fn combat_killing_blow_names_the_attacker_as_killer() {
    let mut host = armed();
    let (ua, ub) = (global_handle(&host, "ua"), global_handle(&host, "ub"));
    host.start("duel", &[]).expect("duel");
    run(&mut host, 60);
    let ub_key = host.game().unit_of_handle(ub).expect("ub");
    assert!(host.game().unit(ub_key).dead, "ub died from the swing of ua");
    assert_eq!(
        global_int(&host, "killer_id") as u32,
        ua,
        "GetKillingUnit in the death event is the attacker"
    );
}

#[test]
fn kill_unit_has_no_killer() {
    let mut host = armed();
    host.start("plain_kill", &[]).expect("plain_kill");
    run(&mut host, 5);
    assert_eq!(
        global_int(&host, "killer_id"),
        0,
        "KillUnit has no damage source, so GetKillingUnit is null"
    );
}
