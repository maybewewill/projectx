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
native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean
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
function ward_here takes nothing returns nothing
    set accepted = IssuePointOrderById(shaman, 852109, 5000.0, 300.0)
endfunction
function polymorph_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852074, foe)
endfunction
function polymorph_foe2 takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852074, foe2)
endfunction
function polymorph_hero takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852074, hero_foe)
endfunction
function polymorph_ally takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852074, grunt)
endfunction
function abolish_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852132, grunt)
endfunction
function abolish_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852132, foe)
endfunction
function abolish_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shaman, 852133)
endfunction
function foe_walks takes nothing returns nothing
    call IssuePointOrder(foe, "move", 6500.0, 0.0)
endfunction
function ensnare_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852106, foe)
endfunction
function ensnare_ally takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852106, grunt)
endfunction
function web_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852211, foe)
endfunction
function shield_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852110, grunt)
endfunction
function purge_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852111, foe)
endfunction
function purge_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852111, grunt)
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

// ---- Passive attack modifiers: Frost Attack, Slow Poison, Feedback ----

fn give_passive(h: &ScriptHost, name: &str, base: u32, level: SpellLevel) {
    let k = {
        let id = h.vm().global(name).and_then(|v| v.as_handle()).unwrap();
        h.game().unit_of_handle(id).unwrap()
    };
    let mut g = h.game_mut();
    g.ability_types_mut().insert(base, AbilityType { max_level: Some(1) });
    g.set_spell_levels(base, vec![level]);
    g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(base, None));
}

#[test]
fn frost_attack_slows_the_target_by_the_misc_decreases_when_it_hits() {
    let mut h = started();
    // Frost Attack: Dur 5 / HeroDur 5.
    give_passive(&h, "grunt", FROST_ATTACK, SpellLevel { dur: f(5.0), hero_dur: f(5.0), ..Default::default() });
    let foe = unit(&h, "foe");
    let before = h.game().unit(foe).bonuses.move_mult.to_f32();
    call(&mut h, "grunt_attacks");
    run(&mut h, 40);
    assert!(has_buff(&h, foe, BUFF_FROST), "the hit applies Bfro");
    assert!((before - h.game().unit(foe).bonuses.move_mult.to_f32() - 0.5).abs() < 1e-6, "FrostMoveSpeedDecrease 0.5");
    assert!((1.0 - h.game().unit(foe).bonuses.attack_mult.to_f32() - 0.25).abs() < 1e-6, "FrostAttackSpeedDecrease 0.25");
    // Not magic: Dispel Magic does not remove it.
    assert!(!h.game().buffs(foe).iter().find(|b| b.base == BUFF_FROST).unwrap().predicates.magic);
}

#[test]
fn slow_poison_slows_and_poisons_an_organic_target() {
    let mut h = started();
    // Slow Poison: Dur 5 / HeroDur 1, DataA 4 per second, DataB 0.5, DataC 0.25.
    give_passive(
        &h,
        "grunt",
        SLOW_POISON,
        SpellLevel { dur: f(5.0), hero_dur: f(1.0), data: data(&[(0, 4.0), (1, 0.5), (2, 0.25), (3, 1.0)]), ..Default::default() },
    );
    let foe = unit(&h, "foe");
    call(&mut h, "grunt_attacks");
    run(&mut h, 10);
    assert!(has_buff(&h, foe, BUFF_SLOW_POISON));
    assert!(life(&h, foe) < 500.0);
    let l0 = life(&h, foe);
    // The poison alone (the attack keeps hitting every 1.0 s: compare with a unit that is not poisoned).
    assert!((1.0 - h.game().unit(foe).bonuses.move_mult.to_f32() - 0.5).abs() < 1e-6);
    run(&mut h, 40);
    assert!(life(&h, foe) < l0 - 30.0, "ticks of 4 plus hits of 10 add up: {} -> {}", l0, life(&h, foe));
    assert!(!h.game().buffs(foe).iter().find(|b| b.base == BUFF_SLOW_POISON).unwrap().predicates.magic, "poison is not dispellable magic");
}

#[test]
fn feedback_burns_mana_and_adds_burned_mana_times_data_b_to_the_hit() {
    let mut h = started();
    // Feedback: DataA 20 (mana burned), DataB 1 (damage per mana), DataC 4 and DataD 1 for heroes.
    give_passive(&h, "grunt", FEEDBACK, SpellLevel { data: data(&[(0, 20.0), (1, 1.0), (2, 4.0), (3, 1.0)]), ..Default::default() });
    let foe = unit(&h, "foe");
    h.game_mut().write_mana(foe, f(100.0));
    call(&mut h, "grunt_attacks");
    run(&mut h, 12);
    let mana = h.game().unit(foe).mana.to_f32();
    assert_eq!(mana, 80.0, "one hit burned 20");
    assert_eq!(life(&h, foe), 470.0, "10 attack damage + 20 burned x 1");
    // Burns at most what the target has.
    h.game_mut().write_mana(foe, f(5.0));
    run(&mut h, 10);
    assert_eq!(h.game().unit(foe).mana.to_f32(), 0.0);
    assert_eq!(life(&h, foe), 470.0 - 15.0, "10 + 5 burned");
}

// ---- Wards ----

const WARD_UNIT: u32 = 0x6F68_7764; // 'ohwd'

fn wards_alive(h: &ScriptHost) -> Vec<ObjKey> {
    let g = h.game();
    (0..g.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if u.type_id == WARD_UNIT && !u.dead && !u.removed))
        .collect()
}

#[test]
fn healing_ward_creates_the_ward_unit_at_the_point_with_a_timed_life() {
    let mut h = started();
    {
        let k = unit(&h, "shaman");
        let mut g = h.game_mut();
        g.unit_types_mut().insert(
            WARD_UNIT,
            UnitType {
                name: "Healing Ward".to_string(),
                max_life: f(150.0),
                ..Default::default()
            },
        );
        g.ability_types_mut().insert(HEALING_WARD, AbilityType { max_level: Some(1) });
        // Healing Ward: cost 200, range 500, UnitID ohwd, Dur 30 (shortened to 5).
        g.set_spell_levels(
            HEALING_WARD,
            vec![SpellLevel {
                cost: 200,
                cool: f(1.0),
                dur: f(5.0),
                hero_dur: f(5.0),
                rng: f(500.0),
                unit_id: WARD_UNIT,
                ..Default::default()
            }],
        );
        g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(HEALING_WARD, None));
    }
    call(&mut h, "ward_here");
    run(&mut h, 1);
    assert!(accepted(&h));
    let wards = wards_alive(&h);
    assert_eq!(wards.len(), 1);
    let w = wards[0];
    assert_eq!(h.game().unit(w).owner, 0);
    assert!(h.game().unit(w).summoned);
    assert_eq!(h.game().unit(w).x.to_f32(), 5000.0);
    assert_eq!(h.game().unit(w).y.to_f32(), 300.0);
    assert!(has_buff(&h, w, BUFF_HEALING_WARD));
    assert_eq!(h.game().unit(unit(&h, "shaman")).mana.to_f32(), 100.0);
    run(&mut h, 60);
    assert!(wards_alive(&h).is_empty(), "the ward dies when its timed life ends");
}

// ---- Polymorph ----

const SHEEP_LAND: u32 = 0x6E73_6865; // 'nshe'

fn with_polymorph(h: &ScriptHost, dur: f32) {
    let k = {
        let id = h.vm().global("shaman").and_then(|v| v.as_handle()).unwrap();
        h.game().unit_of_handle(id).unwrap()
    };
    let mut g = h.game_mut();
    g.unit_types_mut().insert(
        SHEEP_LAND,
        UnitType {
            name: "Sheep".to_string(),
            max_life: f(50.0),
            move_speed: f(100.0),
            ..Default::default()
        },
    );
    g.ability_types_mut().insert(POLYMORPH, AbilityType { max_level: Some(1) });
    // Polymorph: cost 220, cooldown 3, range 500, DataA 5 (max creep level), Dur 60 (shortened).
    g.set_spell_levels(
        POLYMORPH,
        vec![SpellLevel {
            cost: 220,
            cool: f(3.0),
            dur: f(dur),
            hero_dur: f(dur),
            rng: f(500.0),
            data: data(&[(0, 5.0)]),
            ..Default::default()
        }],
    );
    g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(POLYMORPH, None));
}

#[test]
fn polymorph_turns_an_enemy_into_a_sheep_that_cannot_fight_and_restores_it_afterwards() {
    let mut h = started();
    with_polymorph(&h, 6.0);
    let (foe, grunt) = (unit(&h, "foe"), unit(&h, "grunt"));
    h.game_mut().write_life(foe, f(250.0));
    call(&mut h, "foe_attacks");
    run(&mut h, 3);
    call(&mut h, "polymorph_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, foe, BUFF_POLYMORPH));
    assert_eq!(h.game().unit(foe).type_id, SHEEP_LAND);
    assert_eq!(h.game().unit(foe).max_life.to_f32(), 50.0);
    assert_eq!(life(&h, foe), 25.0, "the life fraction (0.5) is kept");
    assert!(h.game().is_polymorphed(foe));
    let hp = life(&h, grunt);
    run(&mut h, 30);
    assert_eq!(life(&h, grunt), hp, "a sheep does not attack");
    run(&mut h, 40);
    assert!(!has_buff(&h, foe, BUFF_POLYMORPH));
    assert_eq!(h.game().unit(foe).type_id, FOOT);
    assert_eq!(h.game().unit(foe).max_life.to_f32(), 500.0);
    assert_eq!(life(&h, foe), 250.0, "back to the original form with the same life fraction");
    assert!(!h.game().is_polymorphed(foe));
}

#[test]
fn polymorph_refuses_heroes_summons_and_allies_and_kills_illusions() {
    let mut h = started();
    with_polymorph(&h, 6.0);
    let (foe, foe2, hero) = (unit(&h, "foe"), unit(&h, "foe2"), unit(&h, "hero_foe"));
    h.game_mut().unit_mut(foe).summoned = true;
    call(&mut h, "polymorph_foe");
    run(&mut h, 1);
    assert!(!accepted(&h), "summoned units are refused");
    call(&mut h, "polymorph_hero");
    run(&mut h, 1);
    assert!(!accepted(&h), "nonhero targs");
    call(&mut h, "polymorph_ally");
    run(&mut h, 1);
    assert!(!accepted(&h), "enemies only");
    h.game_mut().unit_mut(foe2).illusion = true;
    call(&mut h, "polymorph_foe2");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(h.game().unit(foe2).dead, "an illusion dies");
    assert!(!h.game().is_polymorphed(foe2));
    assert!(!h.game().is_polymorphed(hero));
}

// ---- Abolish Magic ----

fn with_abolish(h: &ScriptHost) {
    let k = {
        let id = h.vm().global("shaman").and_then(|v| v.as_handle()).unwrap();
        h.game().unit_of_handle(id).unwrap()
    };
    let mut g = h.game_mut();
    g.ability_types_mut().insert(ABOLISH_MAGIC, AbilityType { max_level: Some(1) });
    // Abolish Magic: cost 50, range 500, DataB 300 (summoned damage), DataC 1.
    g.set_spell_levels(
        ABOLISH_MAGIC,
        vec![SpellLevel {
            cost: 50,
            cool: f(1.0),
            rng: f(500.0),
            data: data(&[(1, 300.0), (2, 1.0)]),
            ..Default::default()
        }],
    );
    g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(ABOLISH_MAGIC, None));
}

fn pos() -> sim::abilities::BuffPredicates {
    sim::abilities::BuffPredicates { positive: true, magic: true, dispellable: true, ..Default::default() }
}

fn neg() -> sim::abilities::BuffPredicates {
    sim::abilities::BuffPredicates { negative: true, magic: true, dispellable: true, ..Default::default() }
}

#[test]
fn abolish_magic_removes_negative_buffs_from_friends_and_positive_ones_from_enemies() {
    let mut h = started();
    with_abolish(&h);
    let (grunt, foe) = (unit(&h, "grunt"), unit(&h, "foe"));
    {
        let mut g = h.game_mut();
        g.apply_mods_buff(grunt, BUFF_CRIPPLE, neg(), UnitMods::default(), f(60.0));
        g.apply_mods_buff(grunt, BUFF_BLOODLUST, pos(), UnitMods::default(), f(60.0));
        g.apply_mods_buff(foe, BUFF_CRIPPLE, neg(), UnitMods::default(), f(60.0));
        g.apply_mods_buff(foe, BUFF_BLOODLUST, pos(), UnitMods::default(), f(60.0));
    }
    call(&mut h, "abolish_grunt");
    run(&mut h, 12);
    assert!(accepted(&h));
    assert!(!has_buff(&h, grunt, BUFF_CRIPPLE) && has_buff(&h, grunt, BUFF_BLOODLUST));
    call(&mut h, "abolish_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, foe, BUFF_CRIPPLE) && !has_buff(&h, foe, BUFF_BLOODLUST));
    assert_eq!(h.game().unit(unit(&h, "shaman")).mana.to_f32(), 200.0);
}

#[test]
fn abolish_magic_autocast_picks_a_friend_with_a_negative_buff() {
    let mut h = started();
    with_abolish(&h);
    let (grunt, grunt2) = (unit(&h, "grunt"), unit(&h, "grunt2"));
    {
        let mut g = h.game_mut();
        g.apply_mods_buff(grunt2, BUFF_CRIPPLE, neg(), UnitMods::default(), f(60.0));
        g.apply_mods_buff(grunt, BUFF_BLOODLUST, pos(), UnitMods::default(), f(60.0));
    }
    call(&mut h, "abolish_on");
    run(&mut h, 2);
    assert!(accepted(&h));
    run(&mut h, 30);
    assert!(!has_buff(&h, grunt2, BUFF_CRIPPLE), "the autocast cleansed the crippled friend");
    assert!(has_buff(&h, grunt, BUFF_BLOODLUST), "a friend's positive buff is left alone");
}

// ---- Ensnare / Web ----

fn with_nets(h: &ScriptHost, speed: f32) {
    let k = {
        let id = h.vm().global("shaman").and_then(|v| v.as_handle()).unwrap();
        h.game().unit_of_handle(id).unwrap()
    };
    let mut g = h.game_mut();
    for base in [ENSNARE, WEB] {
        g.ability_types_mut().insert(base, AbilityType { max_level: Some(1) });
        g.set_spell_missile_speed(base, f(speed));
        // Ensnare: range 500, cooldown 16, DataA 0.6, DataB 200, DataC 128; Dur 12 / HeroDur 3 (shortened to 6 / 3).
        g.set_spell_levels(
            base,
            vec![SpellLevel {
                cost: 0,
                cool: f(1.0),
                dur: f(6.0),
                hero_dur: f(3.0),
                rng: f(500.0),
                data: data(&[(0, 0.6), (1, 200.0), (2, 128.0)]),
                ..Default::default()
            }],
        );
        g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(base, None));
    }
}

fn x_of(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).x.to_f32()
}

#[test]
fn ensnare_roots_a_moving_enemy_for_its_duration_and_the_net_takes_time_to_fly() {
    let mut h = started();
    with_nets(&h, 1000.0);
    let foe = unit(&h, "foe");
    let x0 = x_of(&h, foe);
    call(&mut h, "foe_walks");
    run(&mut h, 5);
    assert!(x_of(&h, foe) > x0 + 10.0, "the control unit walks: {} -> {}", x0, x_of(&h, foe));
    call(&mut h, "ensnare_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    // The net flies: distance / 1000 s; the buff is not there yet at once (the foe is ~400 away: 0.4 s).
    assert!(!has_buff(&h, foe, BUFF_ENSNARE_GROUND));
    run(&mut h, 5);
    assert!(has_buff(&h, foe, BUFF_ENSNARE_GROUND));
    assert!(h.game().is_rooted(foe));
    let x1 = x_of(&h, foe);
    run(&mut h, 20);
    let x2 = x_of(&h, foe);
    assert!((x2 - x1).abs() < 1.0, "a rooted unit stays: {x1} -> {x2}");
    // 6 s duration: after it ends the unit walks again.
    run(&mut h, 50);
    assert!(!has_buff(&h, foe, BUFF_ENSNARE_GROUND));
    assert!(!h.game().is_rooted(foe));
    run(&mut h, 10);
    assert!(x_of(&h, foe) > x2 + 10.0, "walks again");
}

#[test]
fn web_takes_air_units_only_and_ensnare_refuses_structures_and_friends() {
    let mut h = started();
    with_nets(&h, 0.0);
    call(&mut h, "web_foe");
    run(&mut h, 1);
    assert!(!accepted(&h), "Web: air units only");
    call(&mut h, "ensnare_ally");
    run(&mut h, 1);
    assert!(!accepted(&h), "Ensnare: enemies only");
}

// ---- Lightning Shield ----

fn with_shield(h: &ScriptHost, dur: f32) {
    let k = {
        let id = h.vm().global("shaman").and_then(|v| v.as_handle()).unwrap();
        h.game().unit_of_handle(id).unwrap()
    };
    let mut g = h.game_mut();
    g.ability_types_mut().insert(LIGHTNING_SHIELD, AbilityType { max_level: Some(1) });
    // Lightning Shield: cost 100, range 600, Area 160, DataA 20 damage per second, Dur 20 (shortened for the test).
    g.set_spell_levels(
        LIGHTNING_SHIELD,
        vec![SpellLevel {
            cost: 100,
            cool: f(3.0),
            dur: f(dur),
            hero_dur: f(dur),
            area: f(160.0),
            rng: f(600.0),
            data: data(&[(0, 20.0)]),
            ..Default::default()
        }],
    );
    g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(LIGHTNING_SHIELD, None));
}

#[test]
fn lightning_shield_hurts_everyone_around_the_shielded_unit_but_not_itself_every_half_second() {
    let mut h = started();
    with_shield(&h, 3.0);
    let (grunt, grunt2, foe, foe2, hero) = (unit(&h, "grunt"), unit(&h, "grunt2"), unit(&h, "foe"), unit(&h, "foe2"), unit(&h, "hero_foe"));
    call(&mut h, "shield_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, grunt, BUFF_LIGHTNING_SHIELD));
    // Ticks at 0.01, 0.51, 1.01 ... s: after 1.1 s three ticks of DataA / 2 = 10.
    run(&mut h, 10);
    assert_eq!(life(&h, foe), 470.0);
    assert_eq!(life(&h, foe2), 470.0);
    assert_eq!(life(&h, grunt2), 470.0, "friends in the area are hit too");
    assert_eq!(life(&h, grunt), 500.0, "the shielded unit itself is skipped");
    assert_eq!(life(&h, hero), 800.0, "out of the 160 radius");
    // The buff lasts 3.0 s (+ 0.01): after that no more ticks.
    run(&mut h, 40);
    let after = life(&h, foe);
    assert!(!has_buff(&h, grunt, BUFF_LIGHTNING_SHIELD));
    run(&mut h, 20);
    assert_eq!(life(&h, foe), after);
    assert!(after <= 440.0, "about six ticks landed: {after}");
}

// ---- Purge ----

fn with_purge(h: &ScriptHost, dur: f32, hero_dur: f32) {
    let k = {
        let id = h.vm().global("shaman").and_then(|v| v.as_handle()).unwrap();
        h.game().unit_of_handle(id).unwrap()
    };
    let mut g = h.game_mut();
    g.ability_types_mut().insert(PURGE, AbilityType { max_level: Some(1) });
    // Purge: cost 75, cooldown 1, range 700, DataA 5 (movement updates), DataC 400 (summoned damage); Dur 15 / HeroDur 5.
    g.set_spell_levels(
        PURGE,
        vec![SpellLevel {
            cost: 75,
            cool: f(1.0),
            dur: f(dur),
            hero_dur: f(hero_dur),
            rng: f(700.0),
            data: data(&[(0, 5.0), (2, 400.0)]),
            ..Default::default()
        }],
    );
    g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(PURGE, None));
}

fn speed_mult(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).bonuses.move_mult.to_f32()
}

#[test]
fn purge_slows_an_enemy_fully_then_decays_linearly_over_its_duration() {
    let mut h = started();
    with_purge(&h, 15.0, 5.0);
    let foe = unit(&h, "foe");
    let before = speed_mult(&h, foe);
    call(&mut h, "purge_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, foe, BUFF_PURGE));
    // A full slow: the movement multiplier fell by 1.0.
    assert!((before - speed_mult(&h, foe) - 1.0).abs() < 1e-6);
    // Dur 15, DataA 5: one update every 3.0 s, each takes 0.2 of the slow off.
    run(&mut h, 28);
    assert!((before - speed_mult(&h, foe) - 1.0).abs() < 1e-6, "no update before 3.0 s");
    run(&mut h, 4);
    assert!((before - speed_mult(&h, foe) - 0.8).abs() < 1e-5, "0.8 left after 3.0 s: {}", speed_mult(&h, foe));
    run(&mut h, 30);
    assert!((before - speed_mult(&h, foe) - 0.6).abs() < 1e-5);
    // After the whole duration the buff ends and nothing of the slow is left.
    run(&mut h, 130);
    assert!(!has_buff(&h, foe, BUFF_PURGE));
    assert!((before - speed_mult(&h, foe)).abs() < 1e-5, "restored: {}", speed_mult(&h, foe));
}

#[test]
fn purge_removes_magic_buffs_and_slows_only_enemies_and_the_summoned_damage_needs_an_enemy() {
    let mut h = started();
    with_purge(&h, 15.0, 5.0);
    let (foe, grunt) = (unit(&h, "foe"), unit(&h, "grunt"));
    {
        let mut g = h.game_mut();
        let preds = sim::abilities::BuffPredicates { positive: true, magic: true, dispellable: true, ..Default::default() };
        g.apply_mods_buff(foe, BUFF_BLOODLUST, preds, UnitMods::default(), f(60.0));
        g.apply_mods_buff(grunt, BUFF_BLOODLUST, preds, UnitMods::default(), f(60.0));
        g.unit_mut(grunt).summoned = true;
    }
    call(&mut h, "purge_foe");
    run(&mut h, 1);
    assert!(!has_buff(&h, foe, BUFF_BLOODLUST), "the magic buff is gone");
    assert!(has_buff(&h, foe, BUFF_PURGE));
    run(&mut h, 12); // the 1 s cooldown
    call(&mut h, "purge_grunt");
    run(&mut h, 30);
    assert!(accepted(&h), "no relation token in targs: allies can be purged");
    assert_eq!(life(&h, grunt), 500.0, "the summoned damage needs rel != 0 (an enemy); an ally takes none");
    assert!(!has_buff(&h, grunt, BUFF_BLOODLUST), "the caller pushes positive = negative = 1: allies lose magic buffs too");
    assert!(!has_buff(&h, grunt, BUFF_PURGE), "no slow for a passively allied unit");
}

#[test]
fn a_second_purge_restarts_the_slow() {
    let mut h = started();
    with_purge(&h, 15.0, 5.0);
    let foe = unit(&h, "foe");
    let before = speed_mult(&h, foe);
    call(&mut h, "purge_foe");
    run(&mut h, 1);
    run(&mut h, 35);
    assert!((before - speed_mult(&h, foe) - 0.8).abs() < 1e-5);
    // The cooldown is 1 s: cast again; the old buff is removed (its remaining slow taken off) and a new one applied.
    call(&mut h, "purge_foe");
    run(&mut h, 1);
    assert!((before - speed_mult(&h, foe) - 1.0).abs() < 1e-5, "a fresh full slow: {}", speed_mult(&h, foe));
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
