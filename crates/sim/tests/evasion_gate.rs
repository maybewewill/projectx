//! Evasion wired into the attack execute (`combat.rs` `evasion_check`, spec `docs/spec/attack-damage.md` section 12).
//!
//! The selection rule (`sim::damage::evasion_select`) was checked against the original's list walk in Unicorn
//! (`tools/emu/evasion_gate_emu.py`, 700 random lists, 0 mismatches). The behaviour tests below need the real RNG table
//! (`GAME_DLL`) and return early without it: they follow a mirror of combat stream 2 and check that the evasion draws
//! are taken exactly where the original takes them (one draw per execute, only when an entry has a positive chance).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{Ability, EVASION};
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::damage::{evades, evasion_select};
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

/// A started duel setup: `ua` attacks `ub`; `ub` has the evasion ability with `chance` (None = no ability).
fn scene(chance: Option<f32>) -> Option<(ScriptHost, ObjKey, ObjKey, RngTable)> {
    let table = rng_table()?;
    let mut host = host();
    host.set_rng_table(table.clone());
    host.start_game_rng(0x0BAD_5EED);
    host.start("setup", &[]).expect("setup");
    host.step();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    if let Some(c) = chance {
        let mut g = host.game_mut();
        g.set_spell_levels(
            EVASION,
            vec![SpellLevel {
                data: [f(c), F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO, F32::ZERO],
                ..Default::default()
            }],
        );
        g.unit_mut(ub).abilities.attach(Ability::new(EVASION, None));
    }
    Some((host, ua, ub, table))
}

/// Runs the fight for `steps` steps and returns (executes seen on stream 2, hits landed on ub).
fn fight(host: &mut ScriptHost, ub: ObjKey, steps: usize) -> f32 {
    host.start("attack_one", &[]).expect("attack");
    for _ in 0..steps {
        host.step();
    }
    1000.0 - host.game().unit(ub).life.to_f32()
}

#[test]
fn selection_takes_the_first_best_and_ignores_non_positive_scores() {
    assert!(evasion_select([]).is_none());
    assert!(evasion_select([f(0.0), f(-0.5)]).is_none());
    let picked = evasion_select([f(0.25), f(0.5), f(0.5), f(0.1)]).unwrap();
    assert_eq!(picked.to_bits(), f(0.5).to_bits());
    // A NaN score is unordered against the best and selects (x87 compare), then everything after it is unordered too.
    let nan = F32::from_bits(0x7FC0_0000);
    let picked = evasion_select([f(0.25), nan, f(0.9)]).unwrap();
    assert_eq!(picked.to_bits(), f(0.9).to_bits());
}

#[test]
fn a_certain_evasion_stops_every_hit_and_takes_one_stream_two_draw_per_execute() {
    let Some((mut host, _ua, ub, table)) = scene(Some(1.0)) else {
        return;
    };
    let before = host.rng().clone();
    let lost = fight(&mut host, ub, 50);
    assert_eq!(lost, 0.0, "every swing was evaded");
    let after = host.rng().clone();
    // Count the draws: advance a mirror of stream 2 until it equals the host's stream.
    let mut mirror = before.clone();
    let mut executes = 0;
    while mirror.stream(2) != after.stream(2) {
        mirror.stream_mut(2).random_fraction(&table);
        executes += 1;
        assert!(executes < 50, "stream 2 did not match any prefix of the draw sequence");
    }
    assert!(executes >= 3, "several swings happened ({executes})");
    for k in 0..sim::STREAM_COUNT {
        if k != 2 && k != 9 {
            assert_eq!(after.stream(k), before.stream(k), "stream {k} untouched");
        }
    }
    assert_eq!(after.global, before.global);
}

#[test]
fn no_evasion_ability_or_zero_chance_draws_nothing_and_every_hit_lands() {
    for chance in [None, Some(0.0)] {
        let Some((mut host, _ua, ub, _table)) = scene(chance) else {
            return;
        };
        let before = host.rng().clone();
        let lost = fight(&mut host, ub, 50);
        assert!(lost >= 30.0, "hits landed ({lost})");
        assert_eq!(host.rng().stream(2), before.stream(2), "stream 2 untouched for {chance:?}");
    }
}

#[test]
fn half_chance_follows_the_stream_two_draws() {
    let Some((mut host, _ua, ub, table)) = scene(Some(0.5)) else {
        return;
    };
    let before = host.rng().clone();
    let lost = fight(&mut host, ub, 120);
    let after = host.rng().clone();
    let mut mirror = before.clone();
    let mut draws = Vec::new();
    while mirror.stream(2) != after.stream(2) {
        draws.push(mirror.stream_mut(2).random_fraction(&table));
        assert!(draws.len() < 60);
    }
    let expected_hits = draws.iter().filter(|&&d| !evades(f(0.5), d)).count();
    assert!(draws.len() >= 8, "enough swings ({})", draws.len());
    assert_eq!(lost, 10.0 * expected_hits as f32, "hits that landed = draws at or above the chance");
}
