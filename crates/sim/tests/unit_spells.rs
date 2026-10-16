//! Behaviour tests of the unit spells (`unit_spells.rs`, spec `docs/spec/abilities-unit-spells.md`). The level data are
//! pinned values of the tests (the first-level numbers of the 1.26a `AbilityData.slk`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::unit_spells::*;
use sim::ScriptHost;
use softfloat::F32;

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
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
"#;

const SRC: &str = r#"
globals
    unit priest = null
    unit hurt = null
    unit hurt2 = null
    unit full = null
    unit foe = null
    unit far = null
    unit priest2 = null
    unit shaman = null
    unit grunt = null
    unit grunt2 = null
    unit foe2 = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set priest = CreateUnit(Player(0), 'hprt', 0.0, 0.0, 0.0)
    call UnitAddAbility(priest, 'Ahea')
    call SetUnitState(priest, 2, 100.0)
    set hurt = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    call SetWidgetLife(hurt, 400.0)
    set hurt2 = CreateUnit(Player(0), 'hfoo', 150.0, 0.0, 0.0)
    call SetWidgetLife(hurt2, 200.0)
    set full = CreateUnit(Player(0), 'hfoo', 0.0, 100.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 0.0, -100.0, 0.0)
    call SetWidgetLife(foe, 100.0)
    set far = CreateUnit(Player(0), 'hfoo', 1000.0, 0.0, 0.0)
    call SetWidgetLife(far, 10.0)
    set shaman = CreateUnit(Player(0), 'hprt', 5000.0, 0.0, 0.0)
    call UnitAddAbility(shaman, 'Ablo')
    call UnitAddAbility(shaman, 'Ainf')
    call UnitAddAbility(shaman, 'Acri')
    call UnitAddAbility(shaman, 'Afae')
    call UnitAddAbility(shaman, 'Auhf')
    call UnitAddAbility(shaman, 'Arej')
    call UnitAddAbility(shaman, 'Adis')
    call UnitAddAbility(shaman, 'Arai')
    call UnitAddAbility(shaman, 'Aroa')
    call SetUnitState(shaman, 2, 300.0)
    set grunt = CreateUnit(Player(0), 'hfoo', 5100.0, 0.0, 0.0)
    set grunt2 = CreateUnit(Player(0), 'hfoo', 5100.0, 100.0, 0.0)
    set foe2 = CreateUnit(Player(1), 'hfoo', 5140.0, 0.0, 0.0)
endfunction
function setup_manual takes nothing returns nothing
    call setup()
    call IssueImmediateOrderById(priest, 852065)
endfunction
function cripple_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852189, foe2)
endfunction
function cripple_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852189, grunt)
endfunction
function fae_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852149, foe2)
endfunction
function frenzy_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852209, grunt)
endfunction
function frenzy_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852209, foe2)
endfunction
function rejuv_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852160, grunt)
endfunction
function dispel_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852057, foe2)
endfunction
function raise takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shaman, 852197)
endfunction
function raise_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shaman, 852198)
endfunction
function kill_grunts takes nothing returns nothing
    call SetWidgetLife(grunt, 0.0)
    call SetWidgetLife(grunt2, 0.0)
endfunction
function roar takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shaman, 852164)
endfunction
function lust_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852101, grunt)
endfunction
function fire_grunt takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852066, grunt)
endfunction
function lust_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852101, foe2)
endfunction
function fight takes nothing returns nothing
    call IssueTargetOrder(grunt, "attack", foe2)
endfunction
function fight2 takes nothing returns nothing
    call IssueTargetOrder(grunt2, "attack", foe2)
endfunction
function lust_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shaman, 852102)
endfunction
function fire_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shaman, 852067)
endfunction
function heal_hurt takes nothing returns nothing
    set accepted = IssueTargetOrderById(priest, 852063, hurt)
endfunction
function heal_full takes nothing returns nothing
    set accepted = IssueTargetOrderById(priest, 852063, full)
endfunction
function heal_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(priest, 852063, foe)
endfunction
function autocast_off takes nothing returns nothing
    set accepted = IssueImmediateOrderById(priest, 852065)
endfunction
function autocast_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(priest, 852064)
endfunction
"#;

const PRIEST: u32 = 0x6870_7274; // 'hprt'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const SKELETON: u32 = 0x7573_6B65; // 'uske'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [(PRIEST, "Priest", 350.0), (FOOT, "Footman", 500.0)] {
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
        g.ability_types_mut().insert(HEAL, AbilityType { max_level: Some(1) });
        g.ability_types_mut().insert(BLOODLUST, AbilityType { max_level: Some(1) });
        g.ability_types_mut().insert(CRIPPLE, AbilityType { max_level: Some(1) });
        g.set_spell_levels(
            CRIPPLE,
            vec![SpellLevel {
                cost: 100,
                cool: f(1.0),
                dur: f(5.0),
                hero_dur: f(2.0),
                rng: f(600.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(0.75);
                    d[1] = f(0.5);
                    d[2] = f(0.5);
                    d
                },
                ..Default::default()
            }],
        );
        g.ability_types_mut().insert(FAERIE_FIRE, AbilityType { max_level: Some(1) });
        g.set_spell_levels(
            FAERIE_FIRE,
            vec![SpellLevel {
                cost: 45,
                cool: f(1.0),
                dur: f(6.0),
                hero_dur: f(3.0),
                rng: f(700.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(4.0);
                    d
                },
                ..Default::default()
            }],
        );
        g.ability_types_mut().insert(UNHOLY_FRENZY, AbilityType { max_level: Some(1) });
        g.set_spell_levels(
            UNHOLY_FRENZY,
            vec![SpellLevel {
                cost: 50,
                cool: f(1.0),
                dur: f(5.0),
                hero_dur: f(5.0),
                rng: f(500.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(0.75);
                    d[1] = f(4.0);
                    d
                },
                ..Default::default()
            }],
        );
        g.ability_types_mut().insert(REJUVENATION, AbilityType { max_level: Some(1) });
        g.set_spell_levels(
            REJUVENATION,
            vec![SpellLevel {
                cost: 125,
                cool: f(1.0),
                dur: f(12.0),
                hero_dur: f(12.0),
                rng: f(400.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(240.0);
                    d
                },
                ..Default::default()
            }],
        );
        g.ability_types_mut().insert(DISPEL_MAGIC, AbilityType { max_level: Some(1) });
        g.set_spell_levels(
            DISPEL_MAGIC,
            vec![SpellLevel {
                cost: 75,
                cool: f(0.0),
                rng: f(500.0),
                area: f(200.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[1] = f(200.0);
                    d
                },
                ..Default::default()
            }],
        );
        g.ability_types_mut().insert(RAISE_DEAD, AbilityType { max_level: Some(1) });
        g.unit_types_mut().insert(
            SKELETON,
            UnitType {
                name: "Skeleton".to_string(),
                max_life: f(125.0),
                max_mana: f(0.0),
                move_speed: f(270.0),
                ..Default::default()
            },
        );
        g.set_spell_levels(
            RAISE_DEAD,
            vec![SpellLevel {
                cost: 75,
                cool: f(1.0),
                dur: f(5.0),
                hero_dur: f(5.0),
                area: f(900.0),
                rng: f(600.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(2.0);
                    d
                },
                unit_id: SKELETON,
                ..Default::default()
            }],
        );
        g.ability_types_mut().insert(ROAR, AbilityType { max_level: Some(1) });
        g.set_spell_levels(
            ROAR,
            vec![SpellLevel {
                cost: 100,
                cool: f(1.0),
                dur: f(5.0),
                hero_dur: f(5.0),
                area: f(500.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(0.25);
                    d
                },
                ..Default::default()
            }],
        );
        g.ability_types_mut().insert(INNER_FIRE, AbilityType { max_level: Some(1) });
        g.set_type_orders(
            FOOT,
            sim::orders::TypeOrderData {
                abilities: vec![sim::orders::AbilityKind::Move, sim::orders::AbilityKind::Attack],
            },
        );
        g.set_attack_type(
            FOOT,
            sim::combat::AttackType {
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
        // Bloodlust: cost 40, range 600, attack speed +0.4, movement +0.25, Dur 5 (test value).
        g.set_spell_levels(
            BLOODLUST,
            vec![SpellLevel {
                cost: 40,
                cool: f(1.0),
                dur: f(5.0),
                hero_dur: f(5.0),
                rng: f(600.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(0.4);
                    d[1] = f(0.25);
                    d[2] = f(0.3);
                    d
                },
                ..Default::default()
            }],
        );
        // Inner Fire: cost 35, range 500, damage +0.1, armor +5, autocast range 500, Dur 5 (test value).
        g.set_spell_levels(
            INNER_FIRE,
            vec![SpellLevel {
                cost: 35,
                cool: f(1.0),
                dur: f(5.0),
                hero_dur: f(5.0),
                rng: f(500.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(0.1);
                    d[1] = f(5.0);
                    d[2] = f(500.0);
                    d
                },
                ..Default::default()
            }],
        );
        // Heal: cost 5, cooldown 1, range 250, DataA 25.
        g.set_spell_levels(
            HEAL,
            vec![SpellLevel {
                cost: 5,
                cool: f(1.0),
                rng: f(250.0),
                data: {
                    let mut d = [F32::ZERO; 9];
                    d[0] = f(25.0);
                    d
                },
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

fn started_manual() -> ScriptHost {
    let mut h = host();
    h.start("setup_manual", &[]).expect("setup");
    h.step();
    h
}

/// Turns the default autocast off so the manual tests see only their own cast.
fn manual(_h: &mut ScriptHost) {}

#[test]
fn heal_adds_data_a_pays_mana_starts_cooldown_and_marks_the_target() {
    let mut h = started_manual();
    manual(&mut h);
    let (priest, hurt) = (unit(&h, "priest"), unit(&h, "hurt"));
    call(&mut h, "heal_hurt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, hurt), 425.0);
    assert_eq!(h.game().unit(priest).mana.to_f32(), 95.0);
    assert!(h.game().is_cooling(priest, HEAL));
    assert!(has_buff(&h, hurt, BUFF_HEAL));
    // The marker lasts 1.0 + (Cool + 0.01) = 2.01 s.
    run(&mut h, 19);
    assert!(has_buff(&h, hurt, BUFF_HEAL), "still marked at 1.9 s");
    run(&mut h, 4);
    assert!(!has_buff(&h, hurt, BUFF_HEAL), "gone after 2.01 s");
}

#[test]
fn heal_is_capped_at_the_maximum_life() {
    let mut h = started_manual();
    manual(&mut h);
    let hurt = unit(&h, "hurt");
    h.game_mut().write_life(hurt, f(490.0));
    call(&mut h, "heal_hurt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, hurt), 500.0);
}

#[test]
fn heal_refuses_a_target_missing_less_than_one_life_and_enemies() {
    let mut h = started_manual();
    manual(&mut h);
    let priest = unit(&h, "priest");
    call(&mut h, "heal_full");
    run(&mut h, 1);
    assert!(!accepted(&h), "error 0x1C: nothing to heal");
    call(&mut h, "heal_foe");
    run(&mut h, 1);
    assert!(!accepted(&h), "an enemy is not a legal target");
    assert_eq!(h.game().unit(priest).mana.to_f32(), 100.0, "no mana was spent");
}

#[test]
fn autocast_heals_the_lowest_life_fraction_in_range_after_the_first_scan() {
    let mut h = started();
    let (hurt, hurt2, far, foe) = (unit(&h, "hurt"), unit(&h, "hurt2"), unit(&h, "far"), unit(&h, "foe"));
    assert!(h.game().autocast_on(unit(&h, "priest"), HEAL), "Heal starts with autocast on (UNSURE default)");
    run(&mut h, 2);
    assert_eq!(life(&h, hurt2), 225.0, "the most wounded unit in range is healed");
    assert_eq!(life(&h, hurt), 400.0);
    assert_eq!(life(&h, far), 10.0, "out of range");
    assert_eq!(life(&h, foe), 100.0, "enemies are not healed");
}

#[test]
fn autocast_scans_every_three_seconds_and_skips_a_unit_marked_by_another_healer() {
    let mut h = started();
    let (hurt, hurt2) = (unit(&h, "hurt"), unit(&h, "hurt2"));
    run(&mut h, 2);
    assert_eq!(life(&h, hurt2), 225.0);
    // The cooldown is 1 s and the next scan max(cast time, 1.0) = 1.0 s later: hurt2 is still lowest.
    run(&mut h, 12);
    assert_eq!(life(&h, hurt2), 250.0);
    // Another priest's marker on `hurt2` keeps this priest away from it.
    let healer = unit(&h, "full");
    h.game_mut().heal_mark(hurt2, healer, f(10.0));
    run(&mut h, 12);
    assert_eq!(life(&h, hurt2), 250.0, "the foreign marker blocks the target");
    assert!(life(&h, hurt) > 400.0, "the next unit is healed instead");
}

#[test]
fn autocast_off_stops_and_on_resumes() {
    let mut h = started_manual();
    let hurt2 = unit(&h, "hurt2");
    run(&mut h, 40);
    assert_eq!(life(&h, hurt2), 200.0);
    call(&mut h, "autocast_on");
    run(&mut h, 3);
    assert_eq!(life(&h, hurt2), 225.0);
}

/// 600 cases of the original comparator 0x6F0DADD0 (Unicorn, tools/emu/unit_spells_emu.py): derived values only.
#[test]
fn heal_comparator_matches_the_original_vectors() {
    let text = include_str!("data/unit_spells/comparator.txt");
    let mut n = 0;
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let b = |i: usize| F32::from_bits(u32::from_str_radix(w[i], 16).unwrap());
        let got = heal_prefers(b(0), b(1), b(2), b(3), b(4));
        assert_eq!(got, w[5] == "1", "case {n}: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}

/// 600 cases of the shared comparator 0x6F023400 (Unicorn): derived values only.
#[test]
fn default_comparator_matches_the_original_vectors() {
    let text = include_str!("data/unit_spells/comparator_default.txt");
    let mut n = 0;
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let b = |i: usize| F32::from_bits(u32::from_str_radix(w[i], 16).unwrap());
        let got = default_prefers(b(0), b(2), b(4));
        assert_eq!(got, w[5] == "1", "case {n}: {line}");
        n += 1;
    }
    assert_eq!(n, 600);
}

// ---- Inner Fire and Bloodlust ----

fn buffs_gone(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    !has_buff(h, k, code)
}

#[test]
fn bloodlust_adds_attack_and_move_speed_and_takes_them_back() {
    let mut h = started_manual();
    let (shaman, grunt) = (unit(&h, "shaman"), unit(&h, "grunt"));
    call(&mut h, "lust_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, grunt, BUFF_BLOODLUST));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 260.0);
    assert_eq!(h.game().effective_move_speed(grunt).to_f32(), 337.5, "270 * (1 + 0.25)");
    let mult = h.game().unit(grunt).bonuses.attack_mult.to_f32();
    assert!((mult - 1.4).abs() < 1e-5, "attack speed factor {mult}");
    run(&mut h, 50);
    assert!(buffs_gone(&h, grunt, BUFF_BLOODLUST));
    assert_eq!(h.game().effective_move_speed(grunt).to_f32(), 270.0);
    assert_eq!(h.game().unit(grunt).bonuses.attack_mult.to_f32(), 1.0);
}

#[test]
fn bloodlust_refuses_enemies_and_a_second_cast_refreshes_instead_of_stacking() {
    let mut h = started_manual();
    let grunt = unit(&h, "grunt");
    call(&mut h, "lust_foe");
    run(&mut h, 1);
    assert!(!accepted(&h));
    call(&mut h, "lust_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    run(&mut h, 15); // cooldown over
    call(&mut h, "lust_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().effective_move_speed(grunt).to_f32(), 337.5, "replaced, not stacked");
    assert_eq!(h.game().buffs(grunt).iter().filter(|b| b.base == BUFF_BLOODLUST).count(), 1);
}

#[test]
fn inner_fire_adds_armor_and_a_damage_fraction_and_expires() {
    let mut h = started_manual();
    let (shaman, grunt) = (unit(&h, "shaman"), unit(&h, "grunt"));
    call(&mut h, "fire_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    {
        let g = h.game();
        let u = g.unit(grunt);
        assert_eq!(u.bonuses.armor.to_f32(), 5.0);
        // The test weapon has no dice, so the average damage is 0.0 and the fraction gives the sign rule's one point
        // (`inner_fire_weapon_bonus`, Unicorn-checked); Inner Fire is an integer bonus, not a percentage.
        assert_eq!(u.bonuses.damage, 1);
        assert_eq!(u.bonuses.damage_pct.to_f32(), 0.0);
        assert_eq!(g.unit(shaman).mana.to_f32(), 265.0);
    }
    run(&mut h, 50);
    let g = h.game();
    assert!(!has_buff(&h, grunt, BUFF_INNER_FIRE));
    assert_eq!(g.unit(grunt).bonuses.armor.to_f32(), 0.0);
    assert_eq!(g.unit(grunt).bonuses.damage, 0);
}

#[test]
fn bloodlust_autocast_picks_a_unit_that_is_fighting_an_enemy() {
    let mut h = started_manual();
    let (grunt, grunt2) = (unit(&h, "grunt"), unit(&h, "grunt2"));
    call(&mut h, "fight");
    call(&mut h, "lust_on");
    run(&mut h, 5);
    assert!(has_buff(&h, grunt, BUFF_BLOODLUST), "the fighting unit gets Bloodlust");
    assert!(!has_buff(&h, grunt2, BUFF_BLOODLUST), "an idle unit does not");
    // The buff is already on it: the next scans (every 3 s or after the cooldown) do not cast again.
    run(&mut h, 20);
    assert_eq!(h.game().unit(unit(&h, "shaman")).mana.to_f32(), 260.0);
}

#[test]
fn bloodlust_autocast_needs_full_mana_for_a_wounded_unit_and_refuses_a_nearly_dead_one() {
    let mut h = started_manual();
    let (shaman, grunt, grunt2) = (unit(&h, "shaman"), unit(&h, "grunt"), unit(&h, "grunt2"));
    call(&mut h, "fight");
    call(&mut h, "fight2");
    h.game_mut().write_life(grunt, f(200.0)); // 40 percent: below 0.5
    h.game_mut().write_life(grunt2, f(40.0)); // 8 percent: never
    h.game_mut().write_mana(shaman, f(250.0)); // not full
    call(&mut h, "lust_on");
    run(&mut h, 5);
    assert!(!has_buff(&h, grunt, BUFF_BLOODLUST), "below 50 percent life only at full mana");
    h.game_mut().write_mana(shaman, f(300.0));
    run(&mut h, 35); // the next scan comes at 3.0 s
    assert!(has_buff(&h, grunt, BUFF_BLOODLUST), "full mana: allowed");
    assert!(!has_buff(&h, grunt2, BUFF_BLOODLUST), "at most 10 percent life: refused");
}

#[test]
fn inner_fire_autocast_needs_a_unit_in_reach_of_its_enemy() {
    let mut h = started_manual();
    let (grunt, grunt2) = (unit(&h, "grunt"), unit(&h, "grunt2"));
    // grunt2 stands 100 away from the line to the enemy but is ordered to attack: it walks; grunt is in reach at once.
    call(&mut h, "fight");
    call(&mut h, "fire_on");
    run(&mut h, 5);
    assert!(has_buff(&h, grunt, BUFF_INNER_FIRE));
    assert!(!has_buff(&h, grunt2, BUFF_INNER_FIRE), "no attack target: refused");
}

#[test]
fn cripple_slows_movement_and_attacks_and_cuts_damage_then_wears_off() {
    let mut h = started_manual();
    let (shaman, foe2, grunt) = (unit(&h, "shaman"), unit(&h, "foe2"), unit(&h, "grunt"));
    call(&mut h, "cripple_grunt");
    run(&mut h, 1);
    assert!(!accepted(&h), "an ally is not a legal target");
    call(&mut h, "cripple_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 200.0);
    assert!(has_buff(&h, foe2, BUFF_CRIPPLE));
    {
        let g = h.game();
        let u = g.unit(foe2);
        assert_eq!(g.effective_move_speed(foe2).to_f32(), 270.0 * 0.25);
        assert!((u.bonuses.attack_mult.to_f32() - 0.5).abs() < 1e-6);
        // The damage cut is an integer change of the weapon's damage counter (`cripple_weapon_change`, Unicorn-checked);
        // the test weapon has no dice, so its average damage is 0 and the change is 0 (no sign rule for Cripple).
        assert_eq!(u.bonuses.damage, 0);
        assert_eq!(u.bonuses.damage_pct.to_f32(), 0.0);
    }
    run(&mut h, 52);
    let g = h.game();
    assert!(!g.buffs(foe2).iter().any(|b| b.base == BUFF_CRIPPLE));
    assert_eq!(g.effective_move_speed(foe2).to_f32(), 270.0);
    assert_eq!(g.unit(foe2).bonuses.attack_mult.to_f32(), 1.0);
    assert_eq!(g.unit(foe2).bonuses.damage_pct.to_f32(), 0.0);
}

#[test]
fn faerie_fire_lowers_the_armor_of_an_enemy_for_dur() {
    let mut h = started_manual();
    let (shaman, foe2) = (unit(&h, "shaman"), unit(&h, "foe2"));
    call(&mut h, "fae_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(foe2).bonuses.armor.to_f32(), -4.0);
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 255.0);
    run(&mut h, 62);
    assert!(!has_buff(&h, foe2, BUFF_FAERIE_FIRE));
    assert_eq!(h.game().unit(foe2).bonuses.armor.to_f32(), 0.0);
}

#[test]
fn unholy_frenzy_speeds_attacks_and_drains_four_life_a_second_starting_at_once() {
    let mut h = started_manual();
    let (shaman, grunt) = (unit(&h, "shaman"), unit(&h, "grunt"));
    call(&mut h, "frenzy_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 250.0);
    assert!((h.game().unit(grunt).bonuses.attack_mult.to_f32() - 1.75).abs() < 1e-6);
    // The first tick comes 0.01 s after the apply, the rest every 1.0 s.
    run(&mut h, 1);
    assert_eq!(life(&h, grunt), 496.0);
    run(&mut h, 10);
    assert_eq!(life(&h, grunt), 492.0, "second tick at 1.01 s");
    run(&mut h, 40);
    // Dur 5: ticks at 0.01 .. 4.01 = five ticks.
    assert_eq!(life(&h, grunt), 480.0);
    assert!(!has_buff(&h, grunt, BUFF_UNHOLY_FRENZY));
    assert_eq!(h.game().unit(grunt).bonuses.attack_mult.to_f32(), 1.0);
    run(&mut h, 30);
    assert_eq!(life(&h, grunt), 480.0, "no ticks after the buff ends");
}

#[test]
fn unholy_frenzy_can_kill_a_weak_unit() {
    let mut h = started_manual();
    let grunt = unit(&h, "grunt");
    h.game_mut().write_life(grunt, f(6.0));
    call(&mut h, "frenzy_grunt");
    run(&mut h, 15);
    assert!(h.game().unit(grunt).dead, "6 life: dead on the second tick");
}

#[test]
fn unholy_frenzy_cast_twice_keeps_one_tick_stream() {
    let mut h = started_manual();
    let grunt = unit(&h, "grunt");
    call(&mut h, "frenzy_grunt");
    run(&mut h, 15);
    call(&mut h, "frenzy_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    let before = life(&h, grunt);
    run(&mut h, 20);
    assert_eq!(before - life(&h, grunt), 8.0, "two ticks in two seconds, not four");
}

#[test]
fn rejuvenation_heals_in_twelve_ticks_of_one_second_and_stops() {
    let mut h = started_manual();
    let (shaman, grunt) = (unit(&h, "shaman"), unit(&h, "grunt"));
    h.game_mut().write_life(grunt, f(100.0));
    call(&mut h, "rejuv_grunt");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 175.0);
    assert!(has_buff(&h, grunt, BUFF_REJUVENATION));
    assert_eq!(life(&h, grunt), 100.0, "the first tick comes after 1.0 s");
    run(&mut h, 10);
    assert_eq!(life(&h, grunt), 120.0);
    run(&mut h, 110);
    assert_eq!(life(&h, grunt), 340.0, "12 ticks of 240 / 12 = 20");
    assert!(!has_buff(&h, grunt, BUFF_REJUVENATION));
    run(&mut h, 30);
    assert_eq!(life(&h, grunt), 340.0);
}

#[test]
fn dispel_magic_strips_magic_buffs_of_both_polarities_from_everyone_in_the_area() {
    let mut h = started_manual();
    let (shaman, grunt, grunt2, foe2) = (unit(&h, "shaman"), unit(&h, "grunt"), unit(&h, "grunt2"), unit(&h, "foe2"));
    let positive = sim::abilities::BuffPredicates { positive: true, magic: true, dispellable: true, ..Default::default() };
    let negative = sim::abilities::BuffPredicates { negative: true, magic: true, dispellable: true, ..Default::default() };
    let mods = UnitMods { armor: f(5.0), ..Default::default() };
    {
        let mut g = h.game_mut();
        g.apply_mods_buff(foe2, BUFF_INNER_FIRE, positive, mods, f(30.0));
        g.apply_mods_buff(foe2, BUFF_CRIPPLE, negative, UnitMods { armor: f(-2.0), ..Default::default() }, f(30.0));
        g.apply_mods_buff(grunt, BUFF_CRIPPLE, negative, UnitMods { move_delta: f(-0.5), ..Default::default() }, f(30.0));
        g.apply_mods_buff(grunt2, BUFF_BLOODLUST, positive, UnitMods { move_delta: f(0.25), ..Default::default() }, f(30.0));
    }
    call(&mut h, "dispel_foe");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 225.0);
    assert!(!has_buff(&h, foe2, BUFF_INNER_FIRE), "the enemy's positive buff is gone");
    assert!(!has_buff(&h, foe2, BUFF_CRIPPLE), "S38: the caller pushes positive = negative = 1, so its negative buff goes too");
    assert_eq!(h.game().unit(foe2).bonuses.armor.to_f32(), 0.0, "the effects of the removed buffs are undone");
    assert!(!has_buff(&h, grunt, BUFF_CRIPPLE), "the ally's negative buff is gone");
    assert_eq!(h.game().effective_move_speed(grunt).to_f32(), 270.0);
    assert!(!has_buff(&h, grunt2, BUFF_BLOODLUST), "S38: an ally loses positive magic buffs too (Dispel Magic hits friend and foe)");
}

#[test]
fn dispel_magic_hurts_enemy_summons_for_data_b() {
    let mut h = started_manual();
    let foe2 = unit(&h, "foe2");
    h.game_mut().unit_mut(foe2).summoned = true;
    call(&mut h, "dispel_foe");
    run(&mut h, 1);
    assert_eq!(life(&h, foe2), 300.0, "500 - 200 (placeholder armor-free damage model)");
}

fn count_units(h: &ScriptHost, ty: u32, alive_only: bool) -> usize {
    let g = h.game();
    (0..g.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if u.type_id == ty && !u.removed && (!alive_only || !u.dead)))
        .count()
}

#[test]
fn raise_dead_makes_skeletons_from_corpses_in_reach_and_they_expire() {
    let mut h = started_manual();
    let shaman = unit(&h, "shaman");
    call(&mut h, "raise");
    run(&mut h, 1);
    assert!(!accepted(&h), "no corpse in reach: refused");
    call(&mut h, "kill_grunts");
    run(&mut h, 1);
    call(&mut h, "raise");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 225.0);
    assert_eq!(count_units(&h, SKELETON, true), 2);
    assert_eq!(count_units(&h, FOOT, false), 7, "S38: ONE of the 8 footmen was raised (consumed): both skeletons come from the same corpse");
    run(&mut h, 55);
    assert_eq!(count_units(&h, SKELETON, true), 0, "timed life ended");
}

#[test]
fn raise_dead_autocast_raises_when_corpses_are_in_reach() {
    let mut h = started_manual();
    call(&mut h, "kill_grunts");
    call(&mut h, "raise_on");
    run(&mut h, 5);
    assert_eq!(count_units(&h, SKELETON, true), 2);
}

#[test]
fn roar_buffs_friends_in_the_area_including_the_caster_and_not_enemies() {
    let mut h = started_manual();
    let (shaman, grunt, grunt2, foe2, far) = (unit(&h, "shaman"), unit(&h, "grunt"), unit(&h, "grunt2"), unit(&h, "foe2"), unit(&h, "far"));
    call(&mut h, "roar");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 200.0);
    for k in [shaman, grunt, grunt2] {
        assert!(has_buff(&h, k, BUFF_ROAR));
        assert!((h.game().unit(k).bonuses.damage_pct.to_f32() - 0.25).abs() < 1e-6);
    }
    assert!(!has_buff(&h, foe2, BUFF_ROAR));
    assert!(!has_buff(&h, far, BUFF_ROAR), "out of the area");
    run(&mut h, 55);
    assert_eq!(h.game().unit(grunt).bonuses.damage_pct.to_f32(), 0.0);
}
