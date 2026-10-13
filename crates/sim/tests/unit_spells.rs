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
endfunction
function setup_manual takes nothing returns nothing
    call setup()
    call IssueImmediateOrderById(priest, 852065)
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
