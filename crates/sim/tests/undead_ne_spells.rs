//! Undead / Night Elf hero spells through `ScriptHost` (`undead_ne.rs`, docs/spec/abilities-undead-ne-heroes.md):
//! Death Coil, Frost Nova, Entangling Roots, Force of Nature, plus the Unicorn vectors of the relation selector.
//!
//! The level data are pinned values of the tests (usual first-level numbers), not game data. The order ids are
//! the model's (public order lists, UNSURE source).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::natives::game::{DestructableType, ObjKey, UnitType};
use sim::undead_ne::{
    relation, BUFF_FROST_NOVA, BUFF_ROOTS, BUFF_TREANT_LIFE, DEATH_COIL, DEATH_COIL_TABLE, ENTANGLING_ROOTS,
    FORCE_OF_NATURE, FROST_NOVA, HOLY_LIGHT_TABLE,
};
use sim::cast::SpellLevel;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type destructable extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateDestructable takes integer objectid, real x, real y, real face, real scale, integer variation returns destructable
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native SetWidgetLife takes widget wid, real newLife returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit enemy = null
    unit enemy2 = null
    unit far = null
    unit friend = null
    unit mine = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'Udea', 0.0, 0.0, 0.0)
    set enemy = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set enemy2 = CreateUnit(Player(1), 'hfoo', 400.0, 0.0, 0.0)
    set far = CreateUnit(Player(1), 'hfoo', 3000.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'uskl', 100.0, 0.0, 0.0)
    set mine = CreateUnit(Player(0), 'hfoo', 120.0, 0.0, 0.0)
    call UnitAddAbility(caster, 'AUdc')
    call UnitAddAbility(caster, 'AUfn')
    call UnitAddAbility(caster, 'AEer')
    call UnitAddAbility(caster, 'AEfn')
    call SetUnitState(caster, 2, 1000.0)
endfunction
function coil_friend takes nothing returns nothing
    call SetWidgetLife(friend, 300.0)
    set accepted = IssueTargetOrderById(caster, 852001, friend)
endfunction
function coil_enemy takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852001, enemy)
endfunction
function coil_mine takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852001, mine)
endfunction
function nova takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852226, enemy)
endfunction
function roots takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852171, enemy)
endfunction
function trees takes nothing returns nothing
    call CreateDestructable('LTlt', 0.0, 500.0, 0.0, 1.0, 0)
    call CreateDestructable('LTlt', 100.0, 500.0, 0.0, 1.0, 0)
    call CreateDestructable('LTlt', 150.0, 500.0, 0.0, 1.0, 0)
    call CreateDestructable('LTlt', 5000.0, 5000.0, 0.0, 1.0, 0)
    call CreateDestructable('ABcd', 50.0, 500.0, 0.0, 1.0, 0)
endfunction
function nature takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852176, 0.0, 500.0)
endfunction
"#;

const HERO: u32 = 0x5564_6561; // 'Udea'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const SKEL: u32 = 0x7573_6B6C; // 'uskl'
const TREANT: u32 = 0x6566_6F6E; // 'efon'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(cost: i32, cool: f32, dur: f32, hero_dur: f32, area: f32, data: &[f32]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel {
        cast: F32::ZERO,
        dur: f(dur),
        hero_dur: f(hero_dur),
        cost,
        cool: f(cool),
        area: f(area),
        rng: f(900.0),
        data: d,
        unit_id: 0,
    }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [(HERO, "DK", 700.0), (FOOT, "Footman", 500.0), (SKEL, "Skeleton", 500.0), (TREANT, "Treant", 300.0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(1000.0),
                    move_speed: f(270.0),
                    ..Default::default()
                },
            );
        }
        g.destructable_types_mut().insert(
            u32::from_be_bytes(*b"LTlt"),
            DestructableType { max_life: f(50.0), variations: 1 },
        );
        g.destructable_types_mut().insert(
            u32::from_be_bytes(*b"ABcd"),
            DestructableType { max_life: f(50.0), variations: 1 },
        );
        for code in [DEATH_COIL, FROST_NOVA, ENTANGLING_ROOTS, FORCE_OF_NATURE] {
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(1) });
        }
        // Death Coil: heal / damage amount DataA = 100.
        g.set_spell_levels(DEATH_COIL, vec![level(75, 9.0, 0.0, 0.0, 0.0, &[100.0])]);
        // Frost Nova: DataA area damage 50, DataB primary damage 100, area 200; the slow comes from Misc
        // (FrostMoveSpeedDecrease 0.5, FrostAttackSpeedDecrease 0.25 in the stock MiscGame.txt).
        g.set_spell_levels(FROST_NOVA, vec![level(100, 8.0, 4.0, 2.0, 200.0, &[50.0, 100.0])]);
        g.set_frost_misc(f(0.5), f(0.25));
        // Entangling Roots: DataA = 10 damage per second, Dur 10.
        g.set_spell_levels(ENTANGLING_ROOTS, vec![level(100, 10.0, 10.0, 5.0, 0.0, &[10.0])]);
        // Force of Nature: DataA = 3 trees at most, area 800, Dur 30.
        g.set_spell_levels(FORCE_OF_NATURE, vec![level(125, 20.0, 30.0, 30.0, 800.0, &[3.0])]);
        g.set_summon_unit(FORCE_OF_NATURE, TREANT);
        g.set_spell_order(852001, DEATH_COIL);
        g.set_spell_order(852226, FROST_NOVA);
        g.set_spell_order(852171, ENTANGLING_ROOTS);
        g.set_spell_order(852176, FORCE_OF_NATURE);
    }
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

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty(), "no thread was refused");
}

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

#[test]
fn relation_selector_matches_the_original_vectors() {
    let text = include_str!("data/undead_ne/relation.txt");
    let mut n = 0;
    for line in text.lines() {
        let v: Vec<u32> = line.split_whitespace().map(|x| u32::from_str_radix(x, 16).unwrap()).collect();
        let table = if v[0] == 0x6F88_E1F4 { &DEATH_COIL_TABLE } else { &HOLY_LIGHT_TABLE };
        let undead = v[3] != 0 || v[4] == 5;
        let got = relation(table, v[1] != 0, v[2] != 0, undead);
        assert_eq!(got, v[5] as i32, "row {line}");
        n += 1;
    }
    assert!(n >= 800);
}

#[test]
fn death_coil_heals_own_undead_and_halves_on_living_enemies() {
    let mut h = host();
    let (c, friend, enemy, mine) = (unit(&h, "caster"), unit(&h, "friend"), unit(&h, "enemy"), unit(&h, "mine"));
    h.game_mut().unit_mut(friend).class_flags |= 0x2;
    call(&mut h, "coil_friend");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, friend), 400.0, "undead ally heals DataA");
    assert_eq!(h.game().unit(c).mana.to_f32(), 925.0);
    assert!(h.game().is_cooling(c, DEATH_COIL));
    // Cooling: a second cast is refused.
    call(&mut h, "coil_enemy");
    run(&mut h, 1);
    assert!(!accepted(&h));
    h.game_mut().reset_cooldowns(c);
    run(&mut h, 1);
    call(&mut h, "coil_enemy");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, enemy), 450.0, "living enemy takes DataA / 2");
    // A living own unit is neither healed nor hurt.
    h.game_mut().reset_cooldowns(c);
    run(&mut h, 1);
    call(&mut h, "coil_mine");
    run(&mut h, 1);
    assert_eq!(life(&h, mine), 500.0);
}

#[test]
fn death_coil_does_not_hurt_enemy_undead_and_heal_is_capped_at_max_life() {
    let mut h = host();
    let (friend, enemy) = (unit(&h, "friend"), unit(&h, "enemy"));
    h.game_mut().unit_mut(enemy).class_flags |= 0x2;
    call(&mut h, "coil_enemy");
    run(&mut h, 1);
    assert_eq!(life(&h, enemy), 500.0, "enemy undead takes nothing");
    h.game_mut().unit_mut(friend).class_flags |= 0x2;
    let c = unit(&h, "caster");
    h.game_mut().reset_cooldowns(c);
    run(&mut h, 1);
    call(&mut h, "coil_friend");
    run(&mut h, 1);
    assert_eq!(life(&h, friend), 400.0);
    h.game_mut().reset_cooldowns(c);
    run(&mut h, 1);
    call(&mut h, "coil_friend"); // sets life 300 first
    run(&mut h, 1);
    assert_eq!(life(&h, friend), 400.0);
}

#[test]
fn frost_nova_hits_primary_and_area_enemies_and_slows_them() {
    let mut h = host();
    let (enemy, enemy2, far, mine) = (unit(&h, "enemy"), unit(&h, "enemy2"), unit(&h, "far"), unit(&h, "mine"));
    let before = h.game().unit(enemy).move_speed.to_f32();
    call(&mut h, "nova");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, enemy), 350.0, "primary takes the nova DataA (it is inside Area) and DataB");
    assert_eq!(life(&h, enemy2), 450.0, "area unit takes DataA");
    assert_eq!(life(&h, far), 500.0, "outside Area");
    assert_eq!(life(&h, mine), 500.0, "allies are not hit");
    assert!(has_buff(&h, enemy, BUFF_FROST_NOVA) && has_buff(&h, enemy2, BUFF_FROST_NOVA));
    assert!(!has_buff(&h, far, BUFF_FROST_NOVA));
    let slowed = h.game().unit(enemy).move_speed.to_f32();
    assert!((slowed - before * 0.5).abs() < 0.01);
    // Dur 4 s for non-heroes.
    run(&mut h, 35);
    assert!(has_buff(&h, enemy, BUFF_FROST_NOVA));
    run(&mut h, 8);
    assert!(!has_buff(&h, enemy, BUFF_FROST_NOVA));
    assert_eq!(h.game().unit(enemy).move_speed.to_f32(), before);
}

#[test]
fn entangling_roots_hold_the_target_and_tick_damage() {
    let mut h = host();
    let enemy = unit(&h, "enemy");
    call(&mut h, "roots");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, enemy, BUFF_ROOTS));
    assert_eq!(h.game().unit(enemy).move_speed.to_f32(), 0.0, "rooted: cannot move");
    assert!(!h.game().is_stunned(enemy), "a rooted unit can still attack");
    run(&mut h, 35);
    assert_eq!(life(&h, enemy), 460.0, "ticks at 0.01, 1.01, 2.01 and 3.01 s (first pulse 0.01 s after the cast)");
    run(&mut h, 80);
    assert!(!has_buff(&h, enemy, BUFF_ROOTS));
    assert_eq!(h.game().unit(enemy).move_speed.to_f32(), 270.0);
    let after = life(&h, enemy);
    run(&mut h, 30);
    assert_eq!(life(&h, enemy), after, "no damage after the buff ends");
    assert!(after <= 410.0 && after >= 390.0, "about ten ticks, got {after}");
}

#[test]
fn force_of_nature_turns_trees_into_timed_treants() {
    let mut h = host();
    let c = unit(&h, "caster");
    call(&mut h, "trees");
    run(&mut h, 1);
    call(&mut h, "nature");
    run(&mut h, 1);
    assert!(accepted(&h));
    let treants: Vec<ObjKey> = h.game().treants().iter().map(|t| t.1).collect();
    assert_eq!(treants.len(), 3, "min(DataA = 3, trees in Area = 3)");
    for t in &treants {
        assert_eq!(h.game().unit(*t).owner, 0);
        assert!(has_buff(&h, *t, BUFF_TREANT_LIFE));
        assert_eq!(h.game().unit(*t).type_id, TREANT);
    }
    assert_eq!(h.game().unit(c).mana.to_f32(), 875.0);
    run(&mut h, 295);
    assert!(treants.iter().all(|t| !h.game().unit(*t).dead));
    run(&mut h, 20);
    assert!(treants.iter().all(|t| h.game().unit(*t).dead), "timed life ends after Dur");
}
