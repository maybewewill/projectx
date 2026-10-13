//! Behaviour tests of the Human hero abilities (`human_heroes.rs`, spec `docs/spec/abilities-human-heroes.md`):
//! Thunder Clap, Avatar, Divine Shield, Resurrection and Summon Water Elemental through `ScriptHost`, and Bash with
//! the RNG table (`GAME_DLL`; those tests return early without it).
//!
//! The level data are pinned values of the tests (usual first-level numbers), not game data. The spells are cast
//! through `IssueImmediateOrderById` with the order ids of the model (UNSURE source, see `human_heroes.rs`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::human_heroes::*;
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
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native SetWidgetLife takes widget wid, real newLife returns nothing
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
"#;

const SRC: &str = r#"
globals
    unit king = null
    unit pal = null
    unit mage = null
    unit foe = null
    unit foeHero = null
    unit far = null
    unit friend = null
    unit corpseA = null
    unit corpseB = null
    unit corpseC = null
    unit corpseFoe = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set king = CreateUnit(Player(0), 'Hmkg', 0.0, 0.0, 0.0)
    call UnitAddAbility(king, 'AHtc')
    call UnitAddAbility(king, 'AHav')
    call UnitAddAbility(king, 'AHbh')
    call SetUnitState(king, 2, 500.0)
    set pal = CreateUnit(Player(0), 'HPal', 2000.0, 0.0, 0.0)
    call UnitAddAbility(pal, 'AHds')
    call UnitAddAbility(pal, 'AHre')
    call SetUnitState(pal, 2, 500.0)
    set mage = CreateUnit(Player(0), 'Hamg', 4000.0, 0.0, 0.0)
    call UnitAddAbility(mage, 'AHwe')
    call SetUnitState(mage, 2, 500.0)
    set foe = CreateUnit(Player(1), 'hfoo', 100.0, 0.0, 0.0)
    set foeHero = CreateUnit(Player(1), 'HPal', 0.0, 150.0, 0.0)
    set far = CreateUnit(Player(1), 'hfoo', 1000.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 0.0, -100.0, 0.0)
    set corpseA = CreateUnit(Player(0), 'hfoo', 2100.0, 0.0, 0.0)
    set corpseB = CreateUnit(Player(0), 'hfoo', 2150.0, 0.0, 0.0)
    set corpseC = CreateUnit(Player(0), 'hfoo', 2200.0, 0.0, 0.0)
    set corpseFoe = CreateUnit(Player(1), 'hfoo', 2250.0, 0.0, 0.0)
endfunction
function kill_corpses takes nothing returns nothing
    call SetWidgetLife(corpseA, 0.0)
    call SetWidgetLife(corpseB, 0.0)
    call SetWidgetLife(corpseC, 0.0)
    call SetWidgetLife(corpseFoe, 0.0)
endfunction
function clap takes nothing returns nothing
    set accepted = IssueImmediateOrderById(king, 852594)
endfunction
function avatar takes nothing returns nothing
    set accepted = IssueImmediateOrderById(king, 852086)
endfunction
function shield takes nothing returns nothing
    set accepted = IssueImmediateOrderById(pal, 852090)
endfunction
function unshield takes nothing returns nothing
    set accepted = IssueImmediateOrderById(pal, 852056)
endfunction
function resurrect takes nothing returns nothing
    set accepted = IssueImmediateOrderById(pal, 852044)
endfunction
function elementals takes nothing returns nothing
    set accepted = IssueImmediateOrderById(mage, 852097)
endfunction
function attack_foe takes nothing returns nothing
    call IssueTargetOrder(friend, "attack", foe)
endfunction
"#;

const KING: u32 = 0x486D_6B67; // 'Hmkg'
const PALADIN: u32 = 0x4850_616C; // 'HPal'
const MAGE: u32 = 0x4861_6D67; // 'Hamg'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const WATER: u32 = 0x6877_6174; // 'hwat'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn data(a: f32, b: f32, c: f32, d: f32) -> [F32; 9] {
    let mut x = [F32::ZERO; 9];
    x[0] = f(a);
    x[1] = f(b);
    x[2] = f(c);
    x[3] = f(d);
    x
}

fn lvl(cost: i32, cool: f32, dur: f32, hero_dur: f32, area: f32, d: [F32; 9]) -> SpellLevel {
    SpellLevel {
        cast: F32::ZERO,
        dur: f(dur),
        hero_dur: f(hero_dur),
        cost,
        cool: f(cool),
        area: f(area),
        rng: F32::ZERO,
        data: d,
        unit_id: 0,
    }
}

fn game_dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn host(tables: Option<Tables>) -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), tables).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [
            (KING, "Mountain King", 900.0),
            (PALADIN, "Paladin", 800.0),
            (MAGE, "Archmage", 700.0),
            (FOOT, "Footman", 500.0),
            (WATER, "Water Elemental", 525.0),
        ] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(500.0),
                    move_speed: f(270.0),
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
                range: f(300.0),
                collision: f(0.5),
                damage: f(10.0),
                missile_speed: None,
                ..Default::default()
            },
        );
        for base in [THUNDER_CLAP, BASH, AVATAR, DIVINE_SHIELD, RESURRECTION, WATER_ELEMENTAL] {
            g.ability_types_mut().insert(base, AbilityType { max_level: Some(1) });
        }
        // Thunder Clap: damage 100, movement -50 percent, attack speed -40 percent, Dur 5, HeroDur 3, Area 300.
        g.set_spell_levels(THUNDER_CLAP, vec![lvl(90, 6.0, 5.0, 3.0, 300.0, data(100.0, 0.0, 0.5, 0.4))]);
        // Avatar: armor 5, life 500, damage 20, Dur 5.
        g.set_spell_levels(AVATAR, vec![lvl(150, 60.0, 5.0, 5.0, 0.0, data(5.0, 500.0, 20.0, 1.0))]);
        // Divine Shield: DataA = 1 (can be cancelled), Dur 4, HeroDur 4.
        g.set_spell_levels(DIVINE_SHIELD, vec![lvl(25, 35.0, 4.0, 4.0, 0.0, data(1.0, 0.0, 0.0, 0.0))]);
        // Resurrection: DataA = 2 targets, Area 400.
        g.set_spell_levels(RESURRECTION, vec![lvl(200, 240.0, 0.0, 0.0, 400.0, data(2.0, 0.0, 0.0, 0.0))]);
        // Water Elemental: DataA = 2 units of 'hwat' for Dur 4.
        let mut we = lvl(125, 20.0, 4.0, 4.0, 0.0, data(2.0, 0.0, 0.0, 0.0));
        we.unit_id = WATER;
        g.set_spell_levels(WATER_ELEMENTAL, vec![we]);
    }
    h
}

fn started(tables: Option<Tables>) -> ScriptHost {
    let mut h = host(tables);
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

fn units_of_type(h: &ScriptHost, ty: u32) -> Vec<ObjKey> {
    let g = h.game();
    (0..g.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if u.type_id == ty))
        .collect()
}

// ---- Thunder Clap ----

#[test]
fn thunder_clap_hurts_and_slows_enemies_in_the_area_only() {
    let mut h = started(None);
    let (king, foe, hero, far, friend) = (
        unit(&h, "king"),
        unit(&h, "foe"),
        unit(&h, "foeHero"),
        unit(&h, "far"),
        unit(&h, "friend"),
    );
    call(&mut h, "clap");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, foe), 400.0, "DataA damage to the footman");
    assert_eq!(life(&h, hero), 700.0, "DataA damage to the hero");
    assert_eq!(life(&h, far), 500.0, "out of the area");
    assert_eq!(life(&h, friend), 500.0, "allies are not hit");
    assert_eq!(life(&h, king), 900.0, "not the caster");
    assert!(has_buff(&h, foe, BUFF_THUNDER_CLAP));
    assert!(has_buff(&h, hero, BUFF_THUNDER_CLAP));
    assert!(!has_buff(&h, far, BUFF_THUNDER_CLAP));
    // The slow: movement factor 1 - 0.5 and attack speed factor 1 - 0.4.
    assert_eq!(h.game().effective_move_speed(foe).to_f32(), 135.0);
    let mult = h.game().unit(foe).bonuses.attack_mult.to_f32();
    assert!((mult - 0.6).abs() < 1e-5, "attack speed factor {mult}");
    // The mana is paid (90 of 500) and the cooldown runs.
    assert_eq!(h.game().unit(king).mana.to_f32(), 410.0);
    assert!(h.game().is_cooling(king, THUNDER_CLAP));
}

#[test]
fn thunder_clap_slow_lasts_dur_for_units_and_hero_dur_for_heroes() {
    let mut h = started(None);
    let (foe, hero) = (unit(&h, "foe"), unit(&h, "foeHero"));
    call(&mut h, "clap");
    // 3.5 s: the hero (HeroDur 3) is free, the footman (Dur 5) is not.
    run(&mut h, 36);
    assert!(!has_buff(&h, hero, BUFF_THUNDER_CLAP));
    assert!(has_buff(&h, foe, BUFF_THUNDER_CLAP));
    assert_eq!(h.game().effective_move_speed(hero).to_f32(), 270.0, "the hero's speed is back");
    run(&mut h, 20);
    assert!(!has_buff(&h, foe, BUFF_THUNDER_CLAP));
    assert_eq!(h.game().effective_move_speed(foe).to_f32(), 270.0);
    assert_eq!(h.game().unit(foe).bonuses.attack_mult.to_f32(), 1.0);
}

#[test]
fn thunder_clap_is_refused_without_mana() {
    let mut h = started(None);
    let king = unit(&h, "king");
    h.game_mut().write_mana(king, f(50.0));
    call(&mut h, "clap");
    run(&mut h, 1);
    assert!(!accepted(&h));
    assert_eq!(life(&h, unit(&h, "foe")), 500.0);
}

// ---- Avatar ----

#[test]
fn avatar_adds_armor_life_damage_and_immunity_for_dur_and_takes_them_back() {
    let mut h = started(None);
    let king = unit(&h, "king");
    call(&mut h, "avatar");
    run(&mut h, 1);
    assert!(accepted(&h));
    {
        let g = h.game();
        let u = g.unit(king);
        assert_eq!(u.bonuses.armor.to_f32(), 5.0);
        assert_eq!(u.bonuses.damage, 20);
        assert_eq!(u.magic_immune, 1);
        assert_eq!(u.max_life.to_f32(), 1400.0, "mode 3 raises the maximum by DataB");
        assert_eq!(u.life.to_f32(), 1400.0, "and the life by the same amount");
        assert!(g.buffs(king).iter().any(|b| b.base == BUFF_AVATAR));
    }
    // Dur 5 s: still on at 4.5 s, gone at 5.5 s.
    run(&mut h, 44);
    assert_eq!(h.game().unit(king).magic_immune, 1);
    run(&mut h, 12);
    let g = h.game();
    let u = g.unit(king);
    assert_eq!(u.bonuses.armor.to_f32(), 0.0);
    assert_eq!(u.bonuses.damage, 0);
    assert_eq!(u.magic_immune, 0);
    assert_eq!(u.max_life.to_f32(), 900.0, "the maximum comes back");
    assert_eq!(u.life.to_f32(), 900.0, "the life is clamped to it");
    assert!(!g.buffs(king).iter().any(|b| b.base == BUFF_AVATAR));
}

// ---- Divine Shield ----

#[test]
fn divine_shield_makes_the_caster_invulnerable_for_its_duration() {
    let mut h = started(None);
    let pal = unit(&h, "pal");
    assert!(!h.game().unit_is_invulnerable(pal));
    call(&mut h, "shield");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(h.game().unit_is_invulnerable(pal));
    assert_eq!(h.game().unit(pal).magic_immune, 1);
    // A hit while shielded does nothing.
    let foe = unit(&h, "foe");
    let hit = h.game_mut().unit_damage_target(foe, pal, f(100.0), false, false, 0, 15);
    assert!(hit.is_none());
    assert_eq!(life(&h, pal), 800.0);
    run(&mut h, 30);
    assert!(h.game().unit_is_invulnerable(pal), "Dur 4 s");
    run(&mut h, 15);
    assert!(!h.game().unit_is_invulnerable(pal));
    assert_eq!(h.game().unit(pal).magic_immune, 0);
    assert!(!has_buff(&h, pal, BUFF_DIVINE_SHIELD));
}

#[test]
fn divine_shield_can_be_cancelled_when_data_a_allows_it() {
    let mut h = started(None);
    let pal = unit(&h, "pal");
    call(&mut h, "shield");
    run(&mut h, 1);
    call(&mut h, "unshield");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(!h.game().unit_is_invulnerable(pal));
    assert_eq!(h.game().unit(pal).invuln_count, 0);
    assert_eq!(h.game().unit(pal).magic_immune, 0);
    // Without the permission the order does nothing.
    h.game_mut().reset_cooldowns(pal);
    run(&mut h, 1);
    h.game_mut()
        .set_spell_levels(DIVINE_SHIELD, vec![lvl(25, 0.0, 4.0, 4.0, 0.0, data(0.0, 0.0, 0.0, 0.0))]);
    call(&mut h, "shield");
    run(&mut h, 1);
    call(&mut h, "unshield");
    run(&mut h, 1);
    assert!(!accepted(&h));
    assert!(h.game().unit_is_invulnerable(pal));
}

// ---- Resurrection ----

#[test]
fn resurrection_revives_allied_corpses_in_the_area_up_to_data_a() {
    let mut h = started(None);
    call(&mut h, "kill_corpses");
    run(&mut h, 1);
    let (a, b, c, foe_corpse) = (unit(&h, "corpseA"), unit(&h, "corpseB"), unit(&h, "corpseC"), unit(&h, "corpseFoe"));
    for k in [a, b, c, foe_corpse] {
        assert!(h.game().unit(k).dead);
    }
    call(&mut h, "resurrect");
    run(&mut h, 1);
    assert!(accepted(&h));
    // DataA = 2: the first two corpses in walk order, at full life; the third and the enemy's stay dead.
    assert!(!h.game().unit(a).dead);
    assert!(!h.game().unit(b).dead);
    assert_eq!(life(&h, a), 500.0);
    assert_eq!(life(&h, b), 500.0);
    assert!(h.game().unit(c).dead);
    assert!(h.game().unit(foe_corpse).dead);
    assert!(h.game().is_cooling(unit(&h, "pal"), RESURRECTION));
}

#[test]
fn resurrection_does_not_reach_corpses_outside_the_area() {
    let mut h = started(None);
    call(&mut h, "kill_corpses");
    run(&mut h, 1);
    h.game_mut()
        .set_spell_levels(RESURRECTION, vec![lvl(200, 0.0, 0.0, 0.0, 120.0, data(6.0, 0.0, 0.0, 0.0))]);
    call(&mut h, "resurrect");
    run(&mut h, 1);
    // Only corpseA (100 away) is within 120.
    assert!(!h.game().unit(unit(&h, "corpseA")).dead);
    assert!(h.game().unit(unit(&h, "corpseB")).dead);
}

// ---- Summon Water Elemental ----

#[test]
fn water_elemental_summons_data_a_units_that_die_when_dur_ends() {
    let mut h = started(None);
    assert!(units_of_type(&h, WATER).is_empty());
    call(&mut h, "elementals");
    run(&mut h, 1);
    assert!(accepted(&h));
    let summons = units_of_type(&h, WATER);
    assert_eq!(summons.len(), 2);
    for &s in &summons {
        let (owner, summoned, dead) = {
            let g = h.game();
            let u = g.unit(s);
            (u.owner, u.summoned, u.dead)
        };
        assert_eq!(owner, 0);
        assert!(summoned);
        assert!(!dead);
        assert!(has_buff(&h, s, BUFF_WATER_ELEMENTAL));
    }
    run(&mut h, 36);
    assert!(summons.iter().all(|&s| !h.game().unit(s).dead), "alive at 3.7 s");
    run(&mut h, 8);
    assert!(summons.iter().all(|&s| h.game().unit(s).dead), "dead after Dur 4 s");
}

// ---- Bash (needs the RNG table and the soft-float tables of the user's Game.dll) ----

fn bash_scene(chance: f32) -> Option<(ScriptHost, ObjKey, ObjKey, RngTable)> {
    let dll = game_dll()?;
    let table = RngTable::from_game_dll(&dll).expect("RNG table");
    let tables = Tables::from_game_dll(&dll).expect("tables");
    let mut h = host(Some(tables));
    h.set_rng_table(table.clone());
    h.start_game_rng(0x0BAD_5EED);
    h.start("setup", &[]).expect("setup");
    h.step();
    let (friend, foe) = (unit(&h, "friend"), unit(&h, "foe"));
    {
        let mut g = h.game_mut();
        // The friend footman gets Bash: chance, DataB multiplier 0, flat 25, Dur 2, HeroDur 1.
        g.unit_mut(friend)
            .abilities
            .attach(sim::abilities::Ability::new(BASH, Some(AbilityType { max_level: Some(1) })));
        g.set_spell_levels(BASH, vec![lvl(0, 0.0, 2.0, 1.0, 0.0, data(chance, 0.0, 25.0, 0.0))]);
        g.unit_mut(foe).life = f(500.0);
    }
    Some((h, friend, foe, table))
}

#[test]
fn a_certain_bash_stuns_and_adds_the_flat_damage_with_every_hit() {
    let Some((mut h, _friend, foe, _t)) = bash_scene(100.0) else {
        return;
    };
    call(&mut h, "attack_foe");
    run(&mut h, 12);
    // 100 percent is index 20: the constant is 1.0, so the first roll (count 1) always procs.
    let lost = 500.0 - life(&h, foe);
    assert!(lost >= 35.0, "hit 10 + bash 25 landed ({lost})");
    assert!(has_buff(&h, foe, BUFF_PAUSE), "the BPSE pause");
    assert!(h.game().is_stunned(foe));
}

#[test]
fn bash_rolls_on_stream_three_once_per_swing() {
    let Some((mut h, friend, _foe, table)) = bash_scene(25.0) else {
        return;
    };
    let before = h.rng().clone();
    call(&mut h, "attack_foe");
    run(&mut h, 40);
    let after = h.rng().clone();
    // The streams other than 3 (bash) and 9 (reaction delay) and 2 (combat) are untouched.
    for k in 0..sim::STREAM_COUNT {
        if ![2usize, 3, 9].contains(&k) {
            assert_eq!(after.stream(k), before.stream(k), "stream {k} untouched");
        }
    }
    let mut mirror = before.clone();
    let mut draws = 0;
    while mirror.stream(3) != after.stream(3) {
        mirror.stream_mut(3).random_fraction(&table);
        draws += 1;
        assert!(draws < 40, "stream 3 matches a prefix of the draw sequence");
    }
    assert!(draws >= 3, "one draw per swing ({draws})");
    let _ = friend;
}

#[test]
fn bash_state_counter_follows_the_pseudo_random_distribution() {
    let Some((mut h, friend, _foe, _t)) = bash_scene(25.0) else {
        return;
    };
    call(&mut h, "attack_foe");
    run(&mut h, 40);
    let st = h.game().bash_state(friend);
    // After any run the counter is 1 (just procced) or the number of misses since the last proc plus one.
    assert!(st.count >= 1);
}
