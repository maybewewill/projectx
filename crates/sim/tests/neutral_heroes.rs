//! Behaviour tests of the neutral hero abilities of S44 (`neutral_spells.rs`, spec `docs/spec/abilities-neutral.md`
//! section 7). Level numbers are pinned test values; rolls need the RNG table (`GAME_DLL`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::neutral_spells::*;
use sim::rng::RngTable;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
"#;

const SRC: &str = r#"
globals
    unit beast = null
    unit brew = null
    unit near = null
    unit mid = null
    unit far = null
    unit brawler = null
    unit fire = null
    unit witch = null
    unit f1 = null
    unit f2 = null
    unit f3 = null
    unit f4 = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set beast = CreateUnit(Player(0), 'nbst', 0.0, 0.0, 0.0)
    set brew = CreateUnit(Player(0), 'nbrw', 5000.0, 0.0, 0.0)
    set near = CreateUnit(Player(1), 'ndmy', 5300.0, 0.0, 0.0)
    set mid = CreateUnit(Player(1), 'ndmy', 5380.0, 0.0, 0.0)
    set far = CreateUnit(Player(1), 'ndmy', 5800.0, 0.0, 0.0)
    set brawler = CreateUnit(Player(0), 'nbwl', 9000.0, 0.0, 0.0)
    set fire = CreateUnit(Player(0), 'nfir', 12000.0, 0.0, 0.0)
    set witch = CreateUnit(Player(0), 'nwit', 14000.0, 0.0, 0.0)
    set f1 = CreateUnit(Player(1), 'ndmy', 14300.0, 0.0, 0.0)
    set f2 = CreateUnit(Player(1), 'ndmy', 14350.0, 0.0, 0.0)
    set f3 = CreateUnit(Player(1), 'ndmy', 14300.0, 90.0, 0.0)
    set f4 = CreateUnit(Player(1), 'ndmy', 14300.0, 120.0, 0.0)
endfunction
function summon_bear takes nothing returns nothing
    set accepted = IssueImmediateOrderById(beast, 852594)
endfunction
function summon_eagle takes nothing returns nothing
    set accepted = IssueImmediateOrderById(beast, 852596)
endfunction
function lava takes nothing returns nothing
    set accepted = IssueImmediateOrderById(fire, 852667)
endfunction
function burn takes nothing returns nothing
    set accepted = IssueTargetOrderById(fire, 852668, near)
endfunction
function acid takes nothing returns nothing
    set accepted = IssueTargetOrderById(fire, 852662, far)
endfunction
function fork takes nothing returns nothing
    set accepted = IssueTargetOrderById(witch, 852587, f1)
endfunction
function haze_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(brew, 852585, near)
endfunction
"#;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn data(v: &[f32]) -> [F32; 9] {
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
        ..Default::default()
    }
}

const BEAST: u32 = u32::from_be_bytes(*b"nbst");
const BREW: u32 = u32::from_be_bytes(*b"nbrw");
const DUMMY: u32 = u32::from_be_bytes(*b"ndmy");
const BRAWL: u32 = u32::from_be_bytes(*b"nbwl");
const GRIZZLY: u32 = u32::from_be_bytes(*b"ngz1");
const EAGLE: u32 = u32::from_be_bytes(*b"nwe1");
const FIRE: u32 = u32::from_be_bytes(*b"nfir");
const WITCH: u32 = u32::from_be_bytes(*b"nwit");
const LAVA: u32 = u32::from_be_bytes(*b"nlv1");

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(BEAST, ut(vec![SUMMON_GRIZZLY, SUMMON_EAGLE]));
        g.unit_types_mut().insert(BREW, ut(vec![HAZE]));
        g.unit_types_mut().insert(DUMMY, ut(vec![]));
        g.unit_types_mut().insert(BRAWL, ut(vec![BRAWLER]));
        g.unit_types_mut().insert(GRIZZLY, ut(vec![]));
        g.unit_types_mut().insert(EAGLE, ut(vec![]));
        g.unit_types_mut().insert(FIRE, ut(vec![LAVA_SPAWN, SOUL_BURN, ACID_BOMB]));
        g.unit_types_mut().insert(WITCH, ut(vec![FORKED_LIGHTNING]));
        g.unit_types_mut().insert(LAVA, ut(vec![]));
        g.set_spell_levels(
            LAVA_SPAWN,
            vec![SpellLevel { cost: 150, cool: f(1.0), dur: f(70.0), hero_dur: f(70.0), area: f(200.0), data: data(&[1.0, 5.0, 15.0, 0.5, 12.0, 3.0]), unit_id: LAVA, ..Default::default() }],
        );
        g.set_spell_levels(
            SOUL_BURN,
            vec![SpellLevel { cost: 85, cool: f(1.0), dur: f(14.0), hero_dur: f(6.0), rng: f(700.0), data: data(&[7.0, 1.0, 0.5]), ..Default::default() }],
        );
        g.set_spell_levels(
            ACID_BOMB,
            vec![SpellLevel { cost: 75, cool: f(1.0), dur: f(15.0), hero_dur: f(15.0), area: f(200.0), rng: f(900.0), data: data(&[0.0, 0.0, 3.0, 5.0, 3.0, 1.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            FORKED_LIGHTNING,
            vec![SpellLevel { cost: 110, cool: f(1.0), dur: f(0.7), area: f(125.0), rng: f(600.0), data: data(&[85.0, 3.0, 900.0, 300.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            SUMMON_GRIZZLY,
            vec![SpellLevel { cost: 125, cool: f(1.0), dur: f(70.0), hero_dur: f(70.0), area: f(200.0), data: data(&[1.0]), unit_id: GRIZZLY, ..Default::default() }],
        );
        g.set_spell_levels(
            SUMMON_EAGLE,
            vec![SpellLevel { cost: 50, cool: f(1.0), dur: f(70.0), hero_dur: f(70.0), area: f(200.0), data: data(&[1.0]), unit_id: EAGLE, ..Default::default() }],
        );
        g.set_spell_levels(
            HAZE,
            vec![SpellLevel { cost: 70, cool: f(1.0), dur: f(12.0), hero_dur: f(5.0), area: f(200.0), rng: f(550.0), data: data(&[0.0, 0.65, 0.3]), ..Default::default() }],
        );
        g.set_spell_levels(BRAWLER, vec![SpellLevel { data: data(&[100.0, 3.0, 0.0, 0.14]), ..Default::default() }]);
        for id in [SUMMON_GRIZZLY, SUMMON_EAGLE, HAZE, BRAWLER, LAVA_SPAWN, SOUL_BURN, ACID_BOMB, FORKED_LIGHTNING] {
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

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

#[test]
fn beastmaster_summons_create_the_level_unit_with_a_timed_life() {
    let mut h = started();
    let beast = unit(&h, "beast");
    h.start("summon_bear", &[]).expect("bear");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert_eq!(count_of_type(&h, GRIZZLY), 1);
    let bear = {
        let g = h.game();
        (0..g.object_count())
            .map(|i| ObjKey(i as u32))
            .find(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if u.type_id == GRIZZLY))
            .unwrap()
    };
    assert!(has_buff(&h, bear, BUFF_GRIZZLY));
    assert_eq!(h.game().unit(bear).owner, h.game().unit(beast).owner);
    h.start("summon_eagle", &[]).expect("eagle");
    run(&mut h, 4);
    assert_eq!(count_of_type(&h, EAGLE), 1, "each summon is its own ability");
    // 70 s later both are gone (the timed life).
    run(&mut h, 800);
    assert_eq!(count_of_type(&h, GRIZZLY), 0);
}

#[test]
fn drunken_haze_slows_and_blinds_the_enemies_around_the_target() {
    let mut h = started();
    let (near, mid, far) = (unit(&h, "near"), unit(&h, "mid"), unit(&h, "far"));
    h.start("haze_it", &[]).expect("haze");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert!(has_buff(&h, near, BUFF_HAZE));
    assert!(has_buff(&h, mid, BUFF_HAZE), "within 200 of the target");
    assert!(!has_buff(&h, far, BUFF_HAZE), "outside the area");
    let scores = h.game().attacker_miss_scores(near);
    assert_eq!(scores.len(), 1);
    assert_eq!(scores[0], f(0.65));
    let speed = h.game().effective_move_speed(near).to_f32();
    assert!((speed - 270.0 * 0.7).abs() < 0.5, "slowed by 30 percent, got {speed}");
}

#[test]
fn drunken_brawler_is_a_critical_strike_and_an_evasion() {
    let Ok(path) = std::env::var("GAME_DLL") else { return };
    let bytes = std::fs::read(&path).expect("GAME_DLL");
    let table = RngTable::from_game_dll(&bytes).expect("RNG table");
    let h = started();
    h.set_rng_table(table);
    h.start_game_rng(0x0BAD_5EED);
    let b = unit(&h, "brawler");
    h.game_mut().orc_swing_start(b);
    assert!(h.game().crit_is_armed(b), "a 100 percent chance arms the crit");
    let raw = f(40.0);
    let hit = h.game_mut().orc_crit_raw(b, raw);
    assert_eq!(hit, f(120.0), "DataB = 3 times the damage");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

#[test]
fn lava_spawn_is_a_summon_of_its_own_unit() {
    let mut h = started();
    h.start("lava", &[]).expect("lava");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert_eq!(count_of_type(&h, LAVA), 1);
}

#[test]
fn soul_burn_burns_and_silences_for_the_duration() {
    let mut h = started();
    let near = unit(&h, "near");
    // The firelord stands far from `near`; move the target next to it by casting at a unit that is in range.
    h.game_mut().unit_mut(near).x = f(12400.0);
    h.game_mut().unit_mut(near).y = f(0.0);
    h.start("burn", &[]).expect("burn");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert!(has_buff(&h, near, BUFF_SOUL_BURN));
    assert!(h.game().is_silenced(near));
    let before = life(&h, near);
    run(&mut h, 60);
    assert!(life(&h, near) < before - 6.9, "7 damage per second, got {} -> {}", before, life(&h, near));
}

#[test]
fn acid_bomb_corrodes_and_burns() {
    let mut h = started();
    let far = unit(&h, "far");
    h.game_mut().unit_mut(far).x = f(12400.0);
    h.game_mut().unit_mut(far).y = f(0.0);
    h.start("acid", &[]).expect("acid");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert!(has_buff(&h, far, BUFF_ACID_BOMB));
    let before = life(&h, far);
    run(&mut h, 60);
    assert!(life(&h, far) < before - 4.9);
}

#[test]
fn forked_lightning_hits_the_target_and_its_nearest_neighbours() {
    let mut h = started();
    let (f1, f2, f3, f4) = (unit(&h, "f1"), unit(&h, "f2"), unit(&h, "f3"), unit(&h, "f4"));
    h.start("fork", &[]).expect("fork");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert!(life(&h, f1) < 500.0, "the target");
    assert!(life(&h, f2) < 500.0, "nearest neighbour (50 away)");
    assert!(life(&h, f3) < 500.0, "second neighbour (90 away)");
    assert_eq!(life(&h, f4), 500.0, "only three units are hit");
}
