//! Aura emitters, buff refresh and the stat modifiers (`crate::auras`, `docs/spec/buffs.md`) through `ScriptHost`.
//! Timings follow the model: the proximity scan every 0.5 s, the aura buff duration 4.0 s, one host step = 0.1 s.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::auras::{AuraKind, AuraLevel, BuffEffect};
use sim::natives::game::UnitType;
use sim::natives::ObjKey;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
globals
unit hero
unit near
unit far
unit foe
endglobals
function main takes nothing returns nothing
    set hero = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    set near = CreateUnit(Player(0), 'hfoo', I2R(256), I2R(0), I2R(0))
    set far = CreateUnit(Player(0), 'hfoo', I2R(4000), I2R(0), I2R(0))
    set foe = CreateUnit(Player(1), 'hfoo', I2R(128), I2R(0), I2R(0))
endfunction
function learn takes nothing returns nothing
    call UnitAddAbility(hero, 'AHad')
endfunction
function unlearn takes nothing returns nothing
    call UnitRemoveAbility(hero, 'AHad')
endfunction
"#;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn host() -> ScriptHost {
    let prog = jass::parse(PRELUDE).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        for id in [0x4870_616C_u32, 0x6866_6F6F] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: "u".into(),
                    max_life: f(420.0),
                    max_mana: f(100.0),
                    start_mana: f(100.0),
                    move_speed: f(270.0),
                    movement: 1,
                    ..Default::default()
                },
            );
        }
        // AHad: three levels, armor 1.5 / 3 / 4.5, radius 900, friends and self.
        let lvl = |a: f32| AuraLevel {
            area: f(900.0),
            data: [f(a), F32::ZERO, F32::ZERO, F32::ZERO],
            buff: 0,
            targs: "air,ground,friend,self,vuln,invu".into(),
        };
        g.aura_types_mut().insert(
            u32::from_be_bytes(*b"AHad"),
            vec![lvl(1.5), lvl(3.0), lvl(4.5)],
        );
        g.ability_types_mut().insert(
            u32::from_be_bytes(*b"AHad"),
            sim::natives::game::AbilityType { max_level: Some(3) },
        );
    }
    host
}

fn key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .expect("unit handle");
    host.game().unit_of_handle(id).expect("live unit")
}

fn run(host: &mut ScriptHost, name: &str) {
    host.start(name, &[]).expect("starts");
    host.step();
}

fn armor(host: &ScriptHost, k: ObjKey) -> f32 {
    host.game().unit_defense(k).armor.to_f32()
}

fn steps(host: &mut ScriptHost, n: usize) {
    for _ in 0..n {
        host.step();
    }
}

fn buff_count(host: &ScriptHost, k: ObjKey) -> usize {
    host.game().buffs(k).len()
}

#[test]
fn devotion_aura_applies_to_units_in_range_only_and_expires_after_the_source_loses_it() {
    let mut h = host();
    run(&mut h, "main");
    let (hero, near, far, foe) = (key(&h, "hero"), key(&h, "near"), key(&h, "far"), key(&h, "foe"));
    run(&mut h, "learn");
    // the owner is served at once, the others at the first 0.5 s scan
    assert_eq!(armor(&h, hero), 1.5);
    assert_eq!(armor(&h, near), 0.0);
    steps(&mut h, 6);
    assert_eq!(armor(&h, near), 1.5, "a friend inside the radius");
    assert_eq!(armor(&h, far), 0.0, "outside the radius");
    assert_eq!(armor(&h, foe), 0.0, "an enemy is not a friend");
    assert_eq!(buff_count(&h, near), 1);
    // many scans refresh the same buff instead of stacking
    steps(&mut h, 100);
    assert_eq!(buff_count(&h, near), 1);
    assert_eq!(armor(&h, near), 1.5);
    // the source loses the ability: the buffs run out within 4.0 s
    run(&mut h, "unlearn");
    steps(&mut h, 20);
    assert_eq!(armor(&h, near), 1.5, "still buffed 2 s later");
    steps(&mut h, 30);
    assert_eq!(armor(&h, near), 0.0);
    assert_eq!(armor(&h, hero), 0.0);
    assert_eq!(buff_count(&h, near), 0);
}

#[test]
fn the_strongest_aura_of_a_kind_wins_and_a_weaker_one_is_ignored() {
    let mut h = host();
    run(&mut h, "main");
    let (hero, near) = (key(&h, "hero"), key(&h, "near"));
    run(&mut h, "learn");
    steps(&mut h, 6);
    assert_eq!(armor(&h, near), 1.5);
    // level 3 raises the stored strength at the next refresh
    h.game_mut()
        .unit_mut(hero)
        .abilities
        .get_mut(u32::from_be_bytes(*b"AHad"))
        .expect("ability")
        .set_level(3);
    steps(&mut h, 6);
    assert_eq!(armor(&h, near), 4.5);
    // a level 1 application afterwards is weaker than the stored value: ignored, still 4.5
    h.game_mut()
        .unit_mut(hero)
        .abilities
        .get_mut(u32::from_be_bytes(*b"AHad"))
        .expect("ability")
        .set_level(1);
    steps(&mut h, 6);
    assert_eq!(armor(&h, near), 4.5);
    // nothing refreshes the duration any more, so the buff runs out and the weaker value is applied afresh
    steps(&mut h, 45);
    assert_eq!(armor(&h, near), 1.5);
}

#[test]
fn endurance_and_unholy_scale_speed_and_back() {
    let mut h = host();
    run(&mut h, "main");
    let near = key(&h, "near");
    {
        let mut g = h.game_mut();
        g.aura_types_mut().insert(
            u32::from_be_bytes(*b"AOae"),
            vec![AuraLevel {
                area: f(900.0),
                data: [f(0.25), f(0.5), F32::ZERO, F32::ZERO],
                buff: 0,
                targs: "ground,friend,self".into(),
            }],
        );
        let hero = key_of(&g, &h, "hero");
        g.unit_mut(hero)
            .abilities
            .attach(sim::abilities::Ability::new(u32::from_be_bytes(*b"AOae"), None));
        g.sync_auras(hero);
    }
    steps(&mut h, 6);
    let g = h.game();
    assert_eq!(g.unit(near).bonuses.move_mult.to_f32(), 1.25);
    assert_eq!(g.unit(near).bonuses.attack_mult.to_f32(), 1.5);
    assert_eq!(g.effective_move_speed(near).to_f32(), 337.5);
    let BuffEffect::Aura(a) = g.buffs(near)[0].effect.clone().expect("effect") else {
        panic!("aura")
    };
    assert_eq!(a.kind, AuraKind::Endurance);
}

fn key_of(g: &sim::natives::Game, h: &ScriptHost, name: &str) -> ObjKey {
    let id = h
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .expect("handle");
    g.unit_of_handle(id).expect("unit")
}

#[test]
fn stun_counts_disables_and_ends_with_its_duration() {
    let mut h = host();
    run(&mut h, "main");
    let near = key(&h, "near");
    let s = h.game_mut().apply_stun(near, f(2.0));
    assert!(h.game().is_stunned(near));
    assert_eq!(h.game().buffs(near)[0].serial, s);
    // a second application only extends: 1.0 s is not longer than the time left
    h.game_mut().apply_stun(near, f(1.0));
    assert_eq!(h.game().buffs(near).len(), 1);
    steps(&mut h, 15);
    assert!(h.game().is_stunned(near));
    steps(&mut h, 10);
    assert!(!h.game().is_stunned(near));
    assert!(h.game().buffs(near).is_empty());
}

#[test]
fn slow_replaces_its_factors_and_restores_the_unit() {
    let mut h = host();
    run(&mut h, "main");
    let near = key(&h, "near");
    h.game_mut().apply_slow(near, f(5.0), f(-0.6), f(-0.25));
    assert!((h.game().effective_move_speed(near).to_f32() - 108.0).abs() < 0.01);
    assert_eq!(h.game().unit(near).bonuses.attack_mult.to_f32(), 0.75);
    // a later application replaces the factors, it does not add to them
    h.game_mut().apply_slow(near, f(5.0), f(-0.5), f(-0.5));
    assert_eq!(h.game().unit(near).bonuses.move_mult.to_f32(), 0.5);
    assert_eq!(h.game().unit(near).bonuses.attack_mult.to_f32(), 0.5);
    assert_eq!(h.game().buffs(near).len(), 1);
    steps(&mut h, 60);
    assert!(h.game().buffs(near).is_empty());
    assert_eq!(h.game().unit(near).bonuses.move_mult.to_f32(), 1.0);
    assert_eq!(h.game().unit(near).bonuses.attack_mult.to_f32(), 1.0);
}
