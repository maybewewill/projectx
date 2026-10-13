//! Critical Strike (`AOcr`, `crates/sim/src/orc.rs`, spec `docs/spec/abilities-orc-heroes.md`).
//!
//! The PRD table, the effective chance and the compare were checked against the original in Unicorn
//! (`tools/emu/crit_emu.py`, vectors in `tests/data/orc/crit.txt`). The behaviour tests need `GAME_DLL` (RNG table).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::Ability;
use sim::orc::*;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::rng::RngTable;
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
"#;

const SRC: &str = r#"
globals
    unit ua = null
    unit ub = null
endglobals
function setup takes nothing returns nothing
    set ua = CreateUnit(Player(0), 'hfoo', 0, 0, 0)
    set ub = CreateUnit(Player(1), 'hfoo', 100, 0, 180)
endfunction
function attack_one takes nothing returns nothing
    call IssueTargetOrder(ua, "attack", ub)
endfunction
"#;

const FOOTMAN: u32 = 0x6866_6F6F;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn rng_table() -> Option<RngTable> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL"))
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                name: "Footman".to_string(),
                max_life: f(1000.0),
                move_speed: f(270.0),
                turn_speed: f(0.6),
                prop_window: F32::from_bits(0x3F86_0A92),
                ..Default::default()
            },
        );
        g.set_type_orders(
            FOOTMAN,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
        g.set_attack_type(
            FOOTMAN,
            AttackType {
                cooldown: f(1.0),
                damage_point: f(0.5),
                backswing: f(0.5),
                range: f(128.0),
                collision: f(0.5),
                damage: f(10.0),
                missile_speed: None,
                ..Default::default()
            },
        );
    }
    host
}

fn unit_key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host.vm().global(name).and_then(|v| v.as_handle()).expect("handle");
    host.game().unit_of_handle(id).expect("live unit")
}

fn scene(chance: f32) -> Option<(ScriptHost, ObjKey, ObjKey)> {
    let table = rng_table()?;
    let mut host = host();
    host.set_rng_table(table);
    host.start_game_rng(0x0BAD_5EED);
    host.start("setup", &[]).expect("setup");
    host.step();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    {
        let mut g = host.game_mut();
        g.set_spell_levels(
            CRITICAL_STRIKE,
            vec![SpellLevel {
                data: [f(chance), f(3.0), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
                ..Default::default()
            }],
        );
        g.unit_mut(ua).abilities.attach(Ability::new(CRITICAL_STRIKE, None));
    }
    Some((host, ua, ub))
}

fn rows(text: &str) -> Vec<Vec<String>> {
    text.lines().map(|l| l.split_whitespace().map(str::to_string).collect()).collect()
}

fn h(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}

#[test]
fn prd_table_and_effective_chance_match_the_original() {
    let mut cases = 0;
    for r in rows(include_str!("data/orc/crit.txt")) {
        match r[0].as_str() {
            "W" => {
                let i: usize = r[1].parse().unwrap();
                assert_eq!(PRD_TABLE[i], h(&r[2]), "table entry {i}");
            }
            "T" => {
                assert_eq!(prd_value(F32::from_bits(h(&r[1]))).to_bits(), h(&r[2]), "lookup {}", r[1]);
                cases += 1;
            }
            "E" => {
                let count = h(&r[2]) as i32;
                assert_eq!(
                    effective_chance(F32::from_bits(h(&r[1])), count).to_bits(),
                    h(&r[3]),
                    "chance {} count {count}",
                    r[1]
                );
                cases += 1;
            }
            _ => {}
        }
    }
    assert!(cases >= 800, "{cases} cases");
}

#[test]
fn damage_bonus_adds_the_multiple() {
    assert_eq!(crit_damage(f(10.0), f(3.0)).to_f32(), 30.0);
    assert_eq!(crit_damage(f(10.0), F32::ZERO).to_f32(), 10.0);
    assert_eq!(crit_damage(f(10.0), f(1.0)).to_f32(), 10.0);
}

#[test]
fn a_certain_chance_arms_every_swing_and_resets_the_counter() {
    let Some((mut host, ua, _ub)) = scene(100.0) else { return };
    assert_eq!(host.game().crit_counter(ua), 1);
    host.start("attack_one", &[]).expect("attack");
    for _ in 0..30 {
        host.step();
    }
    assert!(host.game().crit_is_armed(ua));
    assert_eq!(host.game().crit_counter(ua), 1);
}

#[test]
fn a_zero_chance_never_arms_and_the_counter_grows() {
    let Some((mut host, ua, _ub)) = scene(0.0) else { return };
    host.start("attack_one", &[]).expect("attack");
    for _ in 0..30 {
        host.step();
    }
    assert!(!host.game().crit_is_armed(ua));
    assert!(host.game().crit_counter(ua) > 1);
}
