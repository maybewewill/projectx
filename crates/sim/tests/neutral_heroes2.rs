//! Behaviour tests of the S50 neutral hero abilities (`neutral_heroes2.rs`, spec `docs/spec/abilities-neutral.md`
//! section 8). Level numbers are pinned test values (the user's `AbilityData.slk` rows); rolls need `GAME_DLL`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::neutral_heroes2::*;
use sim::neutral_waves::*;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native KillUnit takes unit whichUnit returns boolean
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
"#;

const SRC: &str = r#"
globals
    unit alch = null
    unit pit = null
    unit foe = null
    unit foe2 = null
    unit hero = null
    unit tink = null
    unit alchh = null
    unit c1 = null
    unit c2 = null
    unit c3 = null
    unit c4 = null
    unit h1 = null
    unit h2 = null
    unit e1 = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set alch = CreateUnit(Player(0), 'nalc', 0.0, 0.0, 0.0)
    set pit = CreateUnit(Player(0), 'npit', 3000.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'ndmy', 100.0, 0.0, 0.0)
    set foe2 = CreateUnit(Player(1), 'ndmy', 3100.0, 0.0, 0.0)
    set hero = CreateUnit(Player(1), 'Hdmy', 200.0, 0.0, 0.0)
    set tink = CreateUnit(Player(0), 'ntin', 7000.0, 0.0, 0.0)
    set c1 = CreateUnit(Player(1), 'ndmy', 7500.0, 0.0, 0.0)
    set c2 = CreateUnit(Player(1), 'ndmy', 7550.0, 0.0, 0.0)
    set c3 = CreateUnit(Player(1), 'ndmy', 7500.0, 150.0, 0.0)
    set c4 = CreateUnit(Player(1), 'ndmy', 7500.0, 900.0, 0.0)
    set alchh = CreateUnit(Player(0), 'nalh', 9000.0, 0.0, 0.0)
    set h1 = CreateUnit(Player(0), 'ndmy', 9400.0, 0.0, 0.0)
    set h2 = CreateUnit(Player(0), 'ndmy', 9450.0, 0.0, 0.0)
    set e1 = CreateUnit(Player(1), 'ndmy', 9420.0, 0.0, 0.0)
endfunction
function rockets takes nothing returns nothing
    set accepted = IssuePointOrderById(tink, 852652, 7500.0, 0.0)
endfunction
function spray takes nothing returns nothing
    set accepted = IssuePointOrderById(alchh, 852664, 9400.0, 0.0)
endfunction
function stop_alchh takes nothing returns nothing
    call IssueImmediateOrderById(alchh, 851972)
endfunction
function transmute_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(alch, 852665, foe)
endfunction
function transmute_hero takes nothing returns nothing
    set accepted = IssueTargetOrderById(alch, 852665, hero)
endfunction
function doom_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(pit, 852583, foe2)
endfunction
function kill_foe2 takes nothing returns nothing
    call KillUnit(foe2)
endfunction
"#;

pub fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

pub fn data(v: &[f32]) -> [F32; 9] {
    let mut d = [F32::ZERO; 9];
    for (i, x) in v.iter().enumerate() {
        d[i] = f(*x);
    }
    d
}

fn ut(abilities: Vec<u32>) -> UnitType {
    UnitType {
        name: "unit".to_string(),
        max_life: f(500.0),
        max_mana: f(500.0),
        start_mana: f(500.0),
        move_speed: f(270.0),
        abilities,
        gold_cost: 100,
        wood_cost: 20,
        ..Default::default()
    }
}

const ALCH: u32 = u32::from_be_bytes(*b"nalc");
const PIT: u32 = u32::from_be_bytes(*b"npit");
const DUMMY: u32 = u32::from_be_bytes(*b"ndmy");
const HERO: u32 = u32::from_be_bytes(*b"Hdmy");
const DOOMGUARD: u32 = u32::from_be_bytes(*b"nba2");
const TINK: u32 = u32::from_be_bytes(*b"ntin");
const ALCH2: u32 = u32::from_be_bytes(*b"nalh");

fn cluster_level() -> SpellLevel {
    SpellLevel { cost: 70, cool: f(6.0), dur: f(1.01), hero_dur: f(1.01), area: f(200.0), rng: f(800.0), data: data(&[8.75, 0.25, 6.0, 105.0, 1.0, 1.01]), ..Default::default() }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(ALCH, ut(vec![TRANSMUTE]));
        g.unit_types_mut().insert(PIT, ut(vec![DOOM]));
        g.unit_types_mut().insert(DUMMY, ut(vec![]));
        g.unit_types_mut().insert(HERO, ut(vec![]));
        g.unit_types_mut().insert(DOOMGUARD, ut(vec![]));
        g.unit_types_mut().insert(TINK, ut(vec![CLUSTER_ROCKETS]));
        g.unit_types_mut().insert(ALCH2, ut(vec![HEALING_SPRAY]));
        g.set_spell_levels(CLUSTER_ROCKETS, vec![cluster_level()]);
        g.set_spell_levels(
            HEALING_SPRAY,
            vec![SpellLevel { cost: 75, area: f(250.0), rng: f(800.0), data: data(&[40.0, 1.0, 6.0, 280.0, 1.0, 3.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            TRANSMUTE,
            vec![SpellLevel { cost: 150, cool: f(45.0), rng: f(650.0), data: data(&[0.8, 0.0, 5.0, 1.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            DOOM,
            vec![SpellLevel { cost: 150, cool: f(120.0), rng: f(650.0), data: data(&[40.0, 1.0, 120.0, 5.0]), unit_id: DOOMGUARD, ..Default::default() }],
        );
        for id in [TRANSMUTE, DOOM, CLUSTER_ROCKETS, HEALING_SPRAY] {
            g.ability_types_mut().insert(id, AbilityType { max_level: Some(1) });
        }
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

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
}

fn count_of_type(h: &ScriptHost, ty: u32) -> usize {
    let g = h.game();
    (0..g.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if !u.dead && !u.removed && u.type_id == ty))
        .count()
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn gold(h: &ScriptHost, p: u32) -> i32 {
    h.game().player_economy(p).expect("slot").stock[0]
}

#[test]
fn transmute_kills_the_unit_and_pays_the_gold_cost_factor() {
    let mut h = started();
    let foe = unit(&h, "foe");
    let before = gold(&h, 0);
    h.start("transmute_foe", &[]).expect("transmute");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert!(h.game().unit(foe).dead, "the target dies at once");
    // 0.8 * 100 gold, rounded: 80 gold (the store keeps tenths).
    assert_eq!(gold(&h, 0) - before, 800);
}

#[test]
fn transmute_refuses_heroes() {
    let mut h = started();
    let hero = unit(&h, "hero");
    h.start("transmute_hero", &[]).expect("transmute");
    run(&mut h, 4);
    assert!(!h.game().unit(hero).dead);
}

#[test]
fn doom_burns_until_death_and_the_death_calls_a_doomguard() {
    let mut h = started();
    let foe2 = unit(&h, "foe2");
    h.start("doom_foe", &[]).expect("doom");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert!(h.game().neutral2_doomed(foe2));
    assert!(h.game().is_silenced(foe2), "a doomed unit cannot cast");
    let start = life(&h, foe2);
    run(&mut h, 50);
    assert!(life(&h, foe2) <= start - 5.0 * 40.0 + 1.0, "40 per second, {} -> {}", start, life(&h, foe2));
    h.start("kill_foe2", &[]).expect("kill");
    run(&mut h, 4);
    assert!(h.game().unit(foe2).dead);
    assert_eq!(count_of_type(&h, DOOMGUARD), 1, "a Doomguard rises from the corpse");
    let guard = {
        let g = h.game();
        (0..g.object_count())
            .map(|i| ObjKey(i as u32))
            .find(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if u.type_id == DOOMGUARD))
            .unwrap()
    };
    assert_eq!(h.game().unit(guard).owner, 0, "owned by the Pit Lord's player");
    assert!(h.game().buffs(guard).iter().any(|b| b.base == BUFF_DOOM_MINION));
    // 120 s later it is gone.
    run(&mut h, 1300);
    assert_eq!(count_of_type(&h, DOOMGUARD), 0);
}

#[test]
fn cluster_rockets_fire_four_waves_that_damage_and_stun() {
    let mut h = started();
    let (c1, c2, c3, c4) = (unit(&h, "c1"), unit(&h, "c2"), unit(&h, "c3"), unit(&h, "c4"));
    assert_eq!(wave_count(&h.game(), CLUSTER_ROCKETS, &cluster_level()), 4);
    h.start("rockets", &[]).expect("rockets");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, c1), 500.0, "no wave before the first interval");
    run(&mut h, 3);
    assert!((life(&h, c1) - (500.0 - 8.75)).abs() < 0.01, "one wave, got {}", life(&h, c1));
    assert!(h.game().is_stunned(c1), "stunned for the wave");
    run(&mut h, 12);
    for c in [c1, c2, c3] {
        assert!((life(&h, c) - (500.0 - 35.0)).abs() < 0.05, "four waves of 8.75, got {}", life(&h, c));
    }
    assert_eq!(life(&h, c4), 500.0, "outside the 200 area");
}

#[test]
fn healing_spray_heals_friends_three_times_and_not_enemies() {
    let mut h = started();
    let (h1, h2, e1) = (unit(&h, "h1"), unit(&h, "h2"), unit(&h, "e1"));
    for k in [h1, h2, e1] {
        h.game_mut().unit_mut(k).life = f(200.0);
    }
    h.start("spray", &[]).expect("spray");
    run(&mut h, 40);
    assert!(accepted(&h));
    assert_eq!(life(&h, h1), 320.0, "3 waves of 40");
    assert_eq!(life(&h, h2), 320.0);
    assert_eq!(life(&h, e1), 200.0, "enemies are not healed");
}

#[test]
fn an_interrupted_healing_spray_stops() {
    let mut h = started();
    let h1 = unit(&h, "h1");
    h.game_mut().unit_mut(h1).life = f(200.0);
    h.start("spray", &[]).expect("spray");
    run(&mut h, 15);
    assert_eq!(life(&h, h1), 240.0, "one wave so far");
    h.start("stop_alchh", &[]).expect("stop");
    run(&mut h, 40);
    assert_eq!(life(&h, h1), 240.0, "no more waves after the interrupt");
}
