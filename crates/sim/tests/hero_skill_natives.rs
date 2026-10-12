//! `SelectHeroSkill` and the hero skill slots through `ScriptHost` (`natives/hero.rs`, arithmetic in `heroes.rs`).
//!
//! The learn gate and the learn are Unicorn-checked against the original (`tests/hero_learn_vectors.rs`); these tests pin
//! the native: a new hero's skill point, the level requirement, the skill point spend, the hero skill event, and the cap
//! the slots put on the points a level-up grants. Expected values are worked out by hand from `docs/spec/heroes.md`
//! section 12: a skill's next level needs `reqLevel + index * levelSkip` (`HeroAbilityLevelSkip` when the definition has
//! none).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::heroes::XpTables;
use sim::natives::game::{HeroAbilityDef, KillXpConfig, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const HERO: u32 = 0x4870_616C; // 'Hpal'
const A1: u32 = 0x4141_4131; // 'AAA1'
const A2: u32 = 0x4141_4132; // 'AAA2'
const A3: u32 = 0x4141_4133; // 'AAA3'

const PRELUDE: &str = r#"
type player extends handle
type unit extends handle
type trigger extends handle
type triggeraction extends handle
type event extends handle
type boolexpr extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetHandleId takes handle h returns integer
native GetHeroLevel takes unit whichHero returns integer
native GetHeroSkillPoints takes unit whichHero returns integer
native AddHeroXP takes unit whichHero, integer xp, boolean showEyeCandy returns nothing
native UnitModifySkillPoints takes unit whichHero, integer skillPointDelta returns boolean
native SelectHeroSkill takes unit whichHero, integer abilcode returns nothing
native GetUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer
native GetLearnedSkill takes nothing returns integer
native GetLearnedSkillLevel takes nothing returns integer
native GetLearningUnit takes nothing returns unit
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, integer whichEvent, boolexpr filter returns event
"#;

const SCRIPT: &str = r#"
globals
unit h
trigger t
integer ev_skill = 0
integer ev_level = 0
integer ev_unit = 0
integer ev_count = 0
integer r_points = -9
integer r_level = -9
integer r_l1 = -9
integer r_l2 = -9
integer r_l3 = -9
endglobals
function on_learn takes nothing returns nothing
    set ev_skill = GetLearnedSkill()
    set ev_level = GetLearnedSkillLevel()
    set ev_unit = GetHandleId(GetLearningUnit())
    set ev_count = ev_count + 1
endfunction
function setup takes nothing returns nothing
    set h = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    set t = CreateTrigger()
    call TriggerAddAction(t, function on_learn)
    call TriggerRegisterPlayerUnitEvent(t, Player(0), 42, null)
endfunction
function read takes nothing returns nothing
    set r_points = GetHeroSkillPoints(h)
    set r_level = GetHeroLevel(h)
    set r_l1 = GetUnitAbilityLevel(h, 'AAA1')
    set r_l2 = GetUnitAbilityLevel(h, 'AAA2')
    set r_l3 = GetUnitAbilityLevel(h, 'AAA3')
endfunction
function learn1 takes nothing returns nothing
    call SelectHeroSkill(h, 'AAA1')
endfunction
function learn2 takes nothing returns nothing
    call SelectHeroSkill(h, 'AAA2')
endfunction
function learn3 takes nothing returns nothing
    call SelectHeroSkill(h, 'AAA3')
endfunction
function learn_unknown takes nothing returns nothing
    call SelectHeroSkill(h, 'ZZZZ')
endfunction
function xp100 takes nothing returns nothing
    call AddHeroXP(h, 100, false)
endfunction
function plus_point takes nothing returns nothing
    call UnitModifySkillPoints(h, 1)
endfunction
"#;

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SCRIPT}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            HERO,
            UnitType {
                name: "Paladin".to_string(),
                max_life: F32::from_bits(100.0f32.to_bits()),
                hero_abilities: vec![A1, A2, A3],
                ..Default::default()
            },
        );
        for (code, def) in [
            (A1, HeroAbilityDef { levels: 3, req_level: 1, level_skip: 0 }),
            (A2, HeroAbilityDef { levels: 3, req_level: 1, level_skip: 2 }),
            (A3, HeroAbilityDef { levels: 3, req_level: 6, level_skip: 6 }),
        ] {
            g.set_hero_ability_def(code, def);
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(def.levels) });
        }
        g.set_kill_xp_config(KillXpConfig { hero_ability_level_skip: 2, ..KillXpConfig::default() });
        g.set_hero_xp(XpTables::from_need_list(&[100, 200, 300, 400, 500]));
    }
    host
}

fn call(host: &mut ScriptHost, name: &str) {
    host.start(name, &[]).expect("function starts");
    for _ in 0..4 {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

/// (points, level, level of A1, A2, A3).
fn state(host: &mut ScriptHost) -> (i32, i32, i32, i32, i32) {
    call(host, "read");
    (int(host, "r_points"), int(host, "r_level"), int(host, "r_l1"), int(host, "r_l2"), int(host, "r_l3"))
}

#[test]
fn a_new_hero_has_one_skill_point_and_learns_a_first_level() {
    let mut host = host();
    call(&mut host, "setup");
    assert_eq!(state(&mut host), (1, 1, 0, 0, 0), "one point at creation, capped at the 9 learnable levels");
    call(&mut host, "learn1");
    assert_eq!(state(&mut host), (0, 1, 1, 0, 0));
    assert_eq!(int(&host, "ev_count"), 1);
    assert_eq!((int(&host, "ev_skill"), int(&host, "ev_level")), (A1 as i32, 1));
    assert_ne!(int(&host, "ev_unit"), 0, "GetLearningUnit is the hero");
    // No points: the learn is refused (the map designer error), nothing changes.
    call(&mut host, "learn1");
    assert_eq!(state(&mut host), (0, 1, 1, 0, 0));
    assert_eq!(int(&host, "ev_count"), 1);
}

#[test]
fn the_next_level_needs_the_required_hero_level() {
    let mut host = host();
    call(&mut host, "setup");
    call(&mut host, "learn1");
    call(&mut host, "xp100");
    // Level 2, one point. A1 level 2 needs reqLevel 1 + 1 * 2 (Misc skip, the definition has none) = 3: refused.
    assert_eq!(state(&mut host), (1, 2, 1, 0, 0));
    call(&mut host, "learn1");
    assert_eq!(state(&mut host), (1, 2, 1, 0, 0), "hero level 2 is below the required level 3");
    // A2's first level needs 1.
    call(&mut host, "learn2");
    assert_eq!(state(&mut host), (0, 2, 1, 1, 0));
    // Level 3: A1 level 2 is allowed now.
    call(&mut host, "xp100");
    assert_eq!(state(&mut host), (1, 3, 1, 1, 0));
    call(&mut host, "learn1");
    assert_eq!(state(&mut host), (0, 3, 2, 1, 0));
    assert_eq!((int(&host, "ev_skill"), int(&host, "ev_level")), (A1 as i32, 2), "the event carries the new level");
    assert_eq!(int(&host, "ev_count"), 3);
}

#[test]
fn an_ultimate_waits_for_its_level_and_unknown_codes_do_nothing() {
    let mut host = host();
    call(&mut host, "setup");
    call(&mut host, "learn3");
    assert_eq!(state(&mut host), (1, 1, 0, 0, 0), "A3 needs hero level 6");
    call(&mut host, "learn_unknown");
    assert_eq!(state(&mut host), (1, 1, 0, 0, 0));
    assert_eq!(int(&host, "ev_count"), 0);
}

#[test]
fn the_skill_slots_cap_the_points_a_level_up_grants() {
    let mut host = host();
    call(&mut host, "setup");
    // Learn A1, A2 to the level reached and spend: 9 learnable levels in total. Raise the points by hand and level up:
    // the cap is the learnable levels left (9 - learned), so a hero with 8 learned levels left holds at most 1.
    {
        let mut g = host.game_mut();
        let k = g.unit_of_handle(host.vm().global("h").and_then(|v| v.as_handle()).unwrap()).unwrap();
        for code in [A1, A2, A3] {
            g.unit_mut(k).abilities.attach(sim::abilities::Ability::new(code, Some(AbilityType { max_level: Some(3) })));
        }
        // 3 learned (index 2 = level 3) for A1 and A2: 6 levels; A3 at level 1: 1 level -> 2 left of 9.
        for code in [A1, A2] {
            g.unit_mut(k).abilities.get_mut(code).unwrap().level_index = 2;
        }
        g.unit_mut(k).hero.as_mut().unwrap().skill_points = 0;
    }
    call(&mut host, "xp100");
    call(&mut host, "xp100");
    call(&mut host, "xp100");
    // Three level-ups would grant 3 points; only 2 levels are learnable (A3 level 2 and 3).
    assert_eq!(state(&mut host).0, 2, "capped at the learnable levels left");
}
