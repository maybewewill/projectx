//! Behaviour tests of the unit abilities batch A part 2 (`unit_abilities_a2.rs`, spec `docs/spec/abilities-unit-a2.md`).
//! The level data are pinned first-level values of the 1.26a `AbilityData.slk`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{code, AbilityType};
use sim::cast::SpellLevel;
use sim::economy::Res;
use sim::natives::game::{ObjKey, UnitType};
use sim::production::Tree;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
native SetWidgetLife takes widget wid, real newLife returns nothing
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native KillUnit takes unit whichUnit returns boolean
native GetUnitX takes unit whichUnit returns real
native GetUnitY takes unit whichUnit returns real
"#;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(area: f32, data: &[f32]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel { area: f(area), data: d, ..Default::default() }
}

pub const FOOT: u32 = u32::from_be_bytes(*b"hfoo");
const HALL: u32 = u32::from_be_bytes(*b"ohal");
const PEON: u32 = u32::from_be_bytes(*b"opeo");
const GHOUL: u32 = u32::from_be_bytes(*b"ugho");
const ACOLYTE: u32 = u32::from_be_bytes(*b"uaco");
const MINE: u32 = u32::from_be_bytes(*b"ngol");
const PIT: u32 = u32::from_be_bytes(*b"ugol");
const TOL: u32 = u32::from_be_bytes(*b"etol");
const ENTANGLED: u32 = u32::from_be_bytes(*b"egol");

fn ty(name: &str, class: u32) -> UnitType {
    UnitType { name: name.to_string(), max_life: f(1000.0), max_mana: f(200.0), move_speed: f(270.0), classification: class, ..Default::default() }
}

const SRC: &str = r#"
globals
    unit hall = null
    unit mine = null
    unit worker = null
    unit pit = null
    unit tol = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set hall = CreateUnit(Player(1), 'ohal', 0.0, 0.0, 0.0)
    set mine = CreateUnit(Player(15), 'ngol', 0.0, 150.0, 0.0)
    set worker = CreateUnit(Player(1), 'opeo', 0.0, 70.0, 0.0)
    set pit = CreateUnit(Player(1), 'ugol', 0.0, 160.0, 0.0)
    set tol = CreateUnit(Player(1), 'etol', 100.0, -100.0, 0.0)
endfunction
function give_ahar takes nothing returns nothing
    call UnitAddAbility(worker, 'Ahar')
endfunction
function give_ahrl takes nothing returns nothing
    call UnitAddAbility(worker, 'Ahrl')
endfunction
function give_aaha takes nothing returns nothing
    call UnitAddAbility(worker, 'Aaha')
endfunction
function give_abgm takes nothing returns nothing
    call UnitAddAbility(pit, 'Abgm')
endfunction
function give_agld takes nothing returns nothing
    call UnitAddAbility(worker, 'Agld')
endfunction
function take_agld takes nothing returns nothing
    call UnitRemoveAbility(worker, 'Agld')
endfunction
function give_aent takes nothing returns nothing
    call UnitAddAbility(tol, 'Aent')
endfunction
function harvest_mine takes nothing returns nothing
    set accepted = IssueTargetOrderById(worker, 852018, mine)
endfunction
function harvest_pit takes nothing returns nothing
    set accepted = IssueTargetOrderById(worker, 852185, pit)
endfunction
function harvest_tree takes nothing returns nothing
    set accepted = IssuePointOrderById(worker, 852018, 10.0, 100.0)
endfunction
function give_aeat takes nothing returns nothing
    call UnitAddAbility(worker, 'Aeat')
endfunction
function give_agra takes nothing returns nothing
    call UnitAddAbility(worker, 'Agra')
endfunction
function eat_tree takes nothing returns nothing
    set accepted = IssuePointOrderById(worker, 852146, 10.0, 100.0)
endfunction
function grab_tree takes nothing returns nothing
    set accepted = IssuePointOrderById(worker, 852511, 10.0, 100.0)
endfunction
function eat_far_tree takes nothing returns nothing
    set accepted = IssuePointOrderById(worker, 852146, 900.0, 900.0)
endfunction
function low_life takes nothing returns nothing
    call SetWidgetLife(worker, 400.0)
endfunction
function entangle_mine takes nothing returns nothing
    set accepted = IssueTargetOrderById(tol, 852147, mine)
endfunction
"#;

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(HALL, ty("hall", 0x40));
        g.unit_types_mut().set_building(HALL, true);
        for (t, n) in [(PEON, "peon"), (GHOUL, "ghoul"), (ACOLYTE, "acolyte")] {
            g.unit_types_mut().insert(t, ty(n, 0));
        }
        for (t, n) in [(MINE, "mine"), (PIT, "pit"), (TOL, "tol"), (ENTANGLED, "entangled mine")] {
            g.unit_types_mut().insert(t, ty(n, 0));
            g.unit_types_mut().set_building(t, true);
        }
        let l = |c: &[u8; 4], v: SpellLevel| {
            g.ability_types_mut().insert(code(c), AbilityType { max_level: Some(1) });
            g.set_spell_levels(code(c), vec![v]);
        };
        let _ = &l;
        for (c, v) in [
            // Harvest: 1 lumber per hit, 10 lumber, 10 gold.
            (code(b"Ahar"), level(900.0, &[1.0, 10.0, 10.0])),
            // Ghoul harvest: 2 per hit, 20 lumber.
            (code(b"Ahrl"), level(900.0, &[2.0, 20.0])),
            (code(b"Aaha"), level(0.0, &[])),
            // Eat Tree: rip 0.8, eat 2.5, 500 hit points over 30 s. Grab Tree: attach 0.8, remove 2.5, 15 attacks.
            (code(b"Aeat"), SpellLevel { dur: f(30.0), hero_dur: f(30.0), rng: f(32.0), ..level(0.0, &[0.8, 2.5, 500.0]) }),
            (code(b"Agra"), SpellLevel { cool: f(5.0), rng: f(32.0), ..level(0.0, &[0.8, 2.5, 0.0, 1.0, 15.0]) }),
            // Haunted mine: 10 gold per 1 s, 5 miners, radius 200.
            // (a test row: 25 gold so the payments tell the ability record from the 10-gold default)
            (code(b"Abgm"), level(0.0, &[25.0, 1.0, 5.0, 200.0])),
            (code(b"Agld"), level(0.0, &[12500.0, 1.0, 1.0])),
            (code(b"Aent"), SpellLevel { cast: f(3.0), rng: f(500.0), unit_id: ENTANGLED, ..level(0.0, &[]) }),
        ] {
            g.ability_types_mut().insert(c, AbilityType { max_level: Some(1) });
            g.set_spell_levels(c, vec![v]);
        }
    }
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
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

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
}

fn started() -> ScriptHost {
    let mut h = host();
    call(&mut h, "setup");
    run(&mut h, 1);
    {
        let mut g = h.game_mut();
        let mine = {
            let id = h.vm().global("mine").and_then(|v| v.as_handle()).unwrap();
            g.unit_of_handle(id).unwrap()
        };
        g.add_resource_node(mine, 5000);
    }
    h
}

#[test]
fn registry_lists_each_modelled_code_once() {
    let mut seen = std::collections::BTreeSet::new();
    for i in sim::unit_abilities_a2::ABILITIES {
        assert!(seen.insert(i.code), "{} listed twice", i.class);
        assert!(!i.class.is_empty() && !i.note.is_empty());
    }
    for w in ["Agld", "Ahar", "Ahrl", "Aaha", "Abgm", "Aegm", "Aent"] {
        assert!(sim::unit_abilities_a2::info(code(w.as_bytes().try_into().unwrap())).is_some(), "{w}");
    }
}

#[test]
fn harvest_ability_alone_lets_a_peasant_mine_gold_for_the_hall() {
    let mut h = started();
    let worker = unit(&h, "worker");
    assert!(!h.game().can_harvest(worker, Res::Gold), "no ability, no type row");
    call(&mut h, "give_ahar");
    run(&mut h, 1);
    assert!(h.game().can_harvest(worker, Res::Gold) && h.game().can_harvest(worker, Res::Lumber));
    call(&mut h, "harvest_mine");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert!(h.game().is_harvesting(worker));
    run(&mut h, 600);
    let st = h.game().production().stats;
    assert!(st.gold_deposited >= 10 && st.gold_deposited % 10 == 0, "{st:?}");
}

#[test]
fn harvest_order_without_the_ability_is_refused() {
    let mut h = started();
    call(&mut h, "harvest_mine");
    run(&mut h, 2);
    assert!(!accepted(&h));
}

#[test]
fn ghoul_lumber_ability_carries_the_ability_capacity() {
    let mut h = started();
    let worker = unit(&h, "worker");
    h.game_mut().production_mut().trees.push(Tree { x: 10.0, y: 100.0, wood: 50 });
    call(&mut h, "give_ahrl");
    run(&mut h, 1);
    assert!(h.game().can_harvest(worker, Res::Lumber));
    assert!(!h.game().can_harvest(worker, Res::Gold), "Ahrl is lumber only");
    call(&mut h, "harvest_tree");
    run(&mut h, 2);
    assert!(accepted(&h), "the point under the tree is accepted");
    run(&mut h, 800);
    let st = h.game().production().stats;
    // 50 wood in a 20-lumber hold: trips of 20, 20 and 10 (a 10-lumber hold would need five).
    assert_eq!((st.lumber_trips, st.lumber_deposited), (3, 50), "{st:?}");
}

#[test]
fn acolyte_ability_attaches_to_a_haunted_mine_that_pays_the_ability_amounts() {
    let mut h = started();
    let (worker, pit) = (unit(&h, "worker"), unit(&h, "pit"));
    h.game_mut().add_resource_node(pit, 1000);
    call(&mut h, "give_aaha");
    call(&mut h, "give_abgm");
    run(&mut h, 2);
    assert!(h.game().can_harvest(worker, Res::Gold));
    call(&mut h, "harvest_pit");
    run(&mut h, 2);
    assert!(accepted(&h));
    run(&mut h, 400);
    let left = h.game().resource_amount(pit).unwrap();
    // 25 gold per payment (the ability's DataA), one miner: a payment every 1.0 s * 5 slots / 1 = 5 s.
    assert!(left < 1000 && (1000 - left) % 25 == 0, "{left}");
    assert!(h.game().production().gold_by_player.get(&1).copied().unwrap_or(0) >= 25);
}

#[test]
fn gold_mine_ability_makes_a_resource_node_of_data_a_gold() {
    let mut h = started();
    let w = unit(&h, "worker");
    assert!(h.game().resource_amount(w).is_none());
    call(&mut h, "give_agld");
    run(&mut h, 1);
    assert_eq!(h.game().resource_amount(w), Some(12500));
    call(&mut h, "take_agld");
    run(&mut h, 1);
    assert!(h.game().resource_amount(w).is_none());
}

#[test]
fn entangle_ability_turns_a_neutral_mine_into_the_entangled_unit() {
    let mut h = started();
    let mine = unit(&h, "mine");
    call(&mut h, "give_aent");
    run(&mut h, 1);
    call(&mut h, "entangle_mine");
    run(&mut h, 2);
    assert!(accepted(&h));
    run(&mut h, 60);
    let g = h.game();
    let made = g.unit_objects().filter(|(_, u)| u.type_id == ENTANGLED && !u.removed).count();
    assert_eq!(made, 1, "the entangled mine stands");
    assert!(g.unit(mine).removed, "the neutral mine is gone");
}

#[test]
fn entangle_autocast_finds_the_mine_in_range_by_itself() {
    let mut h = started();
    call(&mut h, "give_aent");
    run(&mut h, 80);
    let made = h.game().unit_objects().filter(|(_, u)| u.type_id == ENTANGLED && !u.removed).count();
    assert_eq!(made, 1);
}

fn life_of(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

#[test]
fn eat_tree_fells_the_tree_then_heals_the_data_c_hit_points_over_the_duration() {
    let mut h = started();
    let worker = unit(&h, "worker");
    h.game_mut().production_mut().trees.push(Tree { x: 10.0, y: 100.0, wood: 50 });
    call(&mut h, "give_aeat");
    call(&mut h, "low_life");
    run(&mut h, 2);
    call(&mut h, "eat_tree");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(h.game().production().trees[0].wood, 50, "the rip delay 0.8 s has not run out");
    run(&mut h, 10);
    assert_eq!(h.game().production().trees[0].wood, 0, "the tree is gone after 0.8 s");
    assert_eq!(life_of(&h, worker), 400.0, "the heal waits for the 2.5 s eat delay");
    run(&mut h, 50);
    assert!(h.game().buffs(worker).iter().any(|b| b.base == code(b"Beat")));
    let mid = life_of(&h, worker);
    assert!(mid > 400.0 && mid < 900.0, "{mid}");
    // 500 hit points over 30 s: about 16.7 per second, the buff runs for 30.01 s.
    run(&mut h, 360);
    assert!((life_of(&h, worker) - 900.0).abs() < 1.5, "{}", life_of(&h, worker));
    assert!(!h.game().buffs(worker).iter().any(|b| b.base == code(b"Beat")));
}

#[test]
fn eat_tree_ignores_a_tree_out_of_reach_or_missing() {
    let mut h = started();
    h.game_mut().production_mut().trees.push(Tree { x: 900.0, y: 900.0, wood: 50 });
    call(&mut h, "give_aeat");
    run(&mut h, 2);
    call(&mut h, "eat_far_tree");
    run(&mut h, 60);
    assert_eq!(h.game().production().trees[0].wood, 50, "the caster cannot reach it");
}

#[test]
fn grab_tree_holds_the_tree_for_fifteen_attacks() {
    let mut h = started();
    let worker = unit(&h, "worker");
    h.game_mut().production_mut().trees.push(Tree { x: 10.0, y: 100.0, wood: 50 });
    call(&mut h, "give_agra");
    run(&mut h, 2);
    call(&mut h, "grab_tree");
    run(&mut h, 15);
    assert!(accepted(&h));
    assert_eq!(h.game().production().trees[0].wood, 0);
    assert!(h.game().buffs(worker).iter().any(|b| b.base == code(b"Bgra")));
    // 15 hits through the hook (the attack cycle itself is tested with the strike family).
    for _ in 0..14 {
        let _ = h.game_mut().a2_on_hit(worker, worker);
    }
    run(&mut h, 40);
    assert!(h.game().buffs(worker).iter().any(|b| b.base == code(b"Bgra")), "one attack is left");
    let _ = h.game_mut().a2_on_hit(worker, worker);
    run(&mut h, 10);
    assert!(h.game().buffs(worker).iter().any(|b| b.base == code(b"Bgra")), "dropped only after the 2.5 s remove delay");
    run(&mut h, 30);
    assert!(!h.game().buffs(worker).iter().any(|b| b.base == code(b"Bgra")));
}
