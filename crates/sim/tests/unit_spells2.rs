//! Behaviour tests of the S38 unit spells (`unit_spells.rs`, spec `docs/spec/abilities-unit-spells.md` section 12):
//! Curse and the others added in this lane. The level data are pinned first-level values of the 1.26a `AbilityData.slk`
//! (some durations shortened for the tests, marked in the comments).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::rng::RngTable;
use sim::unit_spells::*;
use sim::ScriptHost;
use softfloat::{Tables, F32};

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
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
"#;

const SRC: &str = r#"
globals
    unit shaman = null
    unit grunt = null
    unit grunt2 = null
    unit foe = null
    unit foe2 = null
    unit hero_foe = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set shaman = CreateUnit(Player(0), 'hprt', 5000.0, 0.0, 0.0)
    call UnitAddAbility(shaman, 'Acrs')
    call SetUnitState(shaman, 2, 300.0)
    set grunt = CreateUnit(Player(0), 'hfoo', 5100.0, 0.0, 0.0)
    set grunt2 = CreateUnit(Player(0), 'hfoo', 5100.0, 100.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 5140.0, 0.0, 180.0)
    set foe2 = CreateUnit(Player(1), 'hfoo', 5140.0, 60.0, 180.0)
    set hero_foe = CreateUnit(Player(1), 'Hpal', 5300.0, 300.0, 180.0)
endfunction
function curse_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852190, foe)
endfunction
function curse_hero takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852190, hero_foe)
endfunction
function curse_ally takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852190, grunt)
endfunction
function curse_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shaman, 852191)
endfunction
function dispel_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852057, foe)
endfunction
function foe_attacks takes nothing returns nothing
    call IssueTargetOrder(foe, "attack", grunt)
endfunction
function grunt_attacks takes nothing returns nothing
    call IssueTargetOrder(grunt, "attack", foe)
endfunction
"#;

const PRIEST: u32 = 0x6870_7274; // 'hprt'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const PALADIN: u32 = 0x4870_616C; // 'Hpal'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn data(d: &[(usize, f32)]) -> [F32; 9] {
    let mut out = [F32::ZERO; 9];
    for &(i, v) in d {
        out[i] = f(v);
    }
    out
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn rng_table() -> Option<RngTable> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL"))
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [(PRIEST, "Priest", 350.0), (FOOT, "Footman", 500.0), (PALADIN, "Paladin", 800.0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(300.0),
                    move_speed: f(270.0),
                    turn_speed: f(0.6),
                    prop_window: F32::from_bits(0x3F86_0A92),
                    ..Default::default()
                },
            );
        }
        g.set_type_orders(
            FOOT,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
        g.set_attack_type(
            FOOT,
            AttackType {
                cooldown: f(1.0),
                damage_point: f(0.5),
                backswing: f(0.5),
                range: f(100.0),
                collision: f(0.5),
                damage: f(10.0),
                missile_speed: None,
                ..Default::default()
            },
        );
        // Curse: cost 40, range 700, chance to miss 0.33, Dur 120 / HeroDur 60 (shortened to 6 / 3 for the tests).
        g.ability_types_mut().insert(CURSE, AbilityType { max_level: Some(1) });
        g.set_spell_levels(
            CURSE,
            vec![SpellLevel {
                cost: 40,
                cool: f(1.0),
                dur: f(6.0),
                hero_dur: f(3.0),
                rng: f(700.0),
                data: data(&[(0, 0.33)]),
                ..Default::default()
            }],
        );
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

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
}

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty(), "no thread was refused");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

// ---- Curse ----

#[test]
fn curse_puts_the_buff_on_an_enemy_pays_mana_and_refuses_friends() {
    let mut h = started();
    let (shaman, foe, grunt) = (unit(&h, "shaman"), unit(&h, "foe"), unit(&h, "grunt"));
    call(&mut h, "curse_ally");
    run(&mut h, 1);
    assert!(!accepted(&h), "enemy-only targs");
    assert!(!has_buff(&h, grunt, BUFF_CURSE));
    call(&mut h, "curse_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, foe, BUFF_CURSE));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 260.0);
    assert_eq!(h.game().attacker_miss_scores(foe), vec![f(0.33)]);
    // Dur 6 for a normal unit: gone after 6.01 s.
    run(&mut h, 55);
    assert!(has_buff(&h, foe, BUFF_CURSE), "still cursed at 5.6 s");
    run(&mut h, 8);
    assert!(!has_buff(&h, foe, BUFF_CURSE));
    assert!(h.game().attacker_miss_scores(foe).is_empty());
}

#[test]
fn curse_on_a_hero_uses_hero_duration() {
    let mut h = started();
    let hero = unit(&h, "hero_foe");
    call(&mut h, "curse_hero");
    run(&mut h, 1);
    assert!(accepted(&h));
    run(&mut h, 25);
    assert!(has_buff(&h, hero, BUFF_CURSE));
    run(&mut h, 8);
    assert!(!has_buff(&h, hero, BUFF_CURSE), "HeroDur 3 s");
}

#[test]
fn a_cursed_attacker_misses_by_the_original_rule_with_one_stream_two_draw_per_swing() {
    let (Some(table), Some(_)) = (rng_table(), tables()) else {
        return;
    };
    let mut h = host();
    h.set_rng_table(table.clone());
    h.start_game_rng(0x0BAD_5EED);
    h.start("setup", &[]).expect("setup");
    h.step();
    let (foe, grunt) = (unit(&h, "foe"), unit(&h, "grunt"));
    // A certain curse (the test overrides the chance): every attack of the cursed foe misses unless the draw is
    // within 0.001 of 1.0.
    {
        let mut g = h.game_mut();
        let mods = UnitMods {
            miss_chance: f(1.0),
            ..UnitMods::default()
        };
        let preds = sim::abilities::BuffPredicates {
            negative: true,
            magic: true,
            dispellable: true,
            ..Default::default()
        };
        g.apply_mods_buff(foe, BUFF_CURSE, preds, mods, f(60.0));
    }
    call(&mut h, "foe_attacks");
    run(&mut h, 80);
    assert!(life(&h, grunt) >= 499.0, "cursed attacker landed {} damage", 500.0 - life(&h, grunt));
    // Without the buff the same fight lands hits.
    let mut h2 = host();
    h2.set_rng_table(table);
    h2.start_game_rng(0x0BAD_5EED);
    h2.start("setup", &[]).expect("setup");
    h2.step();
    let grunt2 = unit(&h2, "grunt");
    call(&mut h2, "foe_attacks");
    run(&mut h2, 80);
    assert!(life(&h2, grunt2) < 500.0, "an uncursed attacker deals damage");
}

// ---- Dispel Magic through the worker `0x6F0EEF00` ----

fn with_dispel(h: &ScriptHost, mana_loss: f32) {
    let k = {
        let id = h.vm().global("shaman").and_then(|v| v.as_handle()).unwrap();
        h.game().unit_of_handle(id).unwrap()
    };
    let mut g = h.game_mut();
    g.ability_types_mut().insert(DISPEL_MAGIC, AbilityType { max_level: Some(1) });
    g.set_spell_levels(
        DISPEL_MAGIC,
        vec![SpellLevel {
            cost: 75,
            rng: f(500.0),
            area: f(200.0),
            data: data(&[(0, mana_loss), (1, 200.0)]),
            ..Default::default()
        }],
    );
    g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(DISPEL_MAGIC, None));
}

fn dispel_at_foe(h: &mut ScriptHost) {
    call(h, "dispel_foe");
    run(h, 1);
}

#[test]
fn dispel_damages_an_enemy_summoned_unit_unless_it_carries_anti_magic_shell() {
    let mut h = started();
    with_dispel(&h, 0.0);
    let (foe, foe2) = (unit(&h, "foe"), unit(&h, "foe2"));
    {
        let mut g = h.game_mut();
        g.unit_mut(foe).summoned = true;
        g.unit_mut(foe2).summoned = true;
        let preds = sim::abilities::BuffPredicates { positive: true, magic: true, dispellable: true, ..Default::default() };
        g.apply_mods_buff(foe2, BUFF_ANTI_MAGIC_SHELL, preds, UnitMods::default(), f(30.0));
    }
    dispel_at_foe(&mut h);
    assert!(accepted(&h));
    assert_eq!(life(&h, foe), 300.0, "DataB = 200 magic damage to an unprotected summoned enemy");
    assert_eq!(life(&h, foe2), 500.0, "Bams: the worker takes the plain dispel record instead");
}

#[test]
fn dispel_spares_a_magic_immune_summoned_unit_and_skips_mana_loss_for_invulnerable_ones() {
    let mut h = started();
    with_dispel(&h, 50.0);
    let (foe, foe2, grunt) = (unit(&h, "foe"), unit(&h, "foe2"), unit(&h, "grunt"));
    {
        let mut g = h.game_mut();
        g.unit_mut(foe).summoned = true;
        g.unit_mut(foe).magic_immune = 1;
        g.write_mana(foe, f(100.0));
        g.write_mana(foe2, f(100.0));
        g.write_mana(grunt, f(100.0));
        g.set_unit_invulnerable(grunt, true);
    }
    dispel_at_foe(&mut h);
    assert_eq!(life(&h, foe), 500.0, "slot 0x11C protects the unit from the summoned damage");
    assert_eq!(h.game().unit(foe).mana.to_f32(), 100.0, "a protected unit returns before the mana loss");
    assert_eq!(h.game().unit(foe2).mana.to_f32(), 50.0, "DataA mana loss");
    assert_eq!(h.game().unit(grunt).mana.to_f32(), 100.0, "invulnerable (+0x20 & 8): no mana loss");
}
