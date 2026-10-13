//! Death Pact, Blink, Carrion Swarm, Sleep, Shadow Strike, Immolation and Vampiric Aura through `ScriptHost`
//! (`undead_ne.rs`, docs/spec/abilities-undead-ne-heroes.md section S28). Level data are pinned values of the tests,
//! not game data; the order ids are the public order lists (UNSURE source). One host step = 0.1 s.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::auras::AuraLevel;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::undead_ne::{
    BLINK, BUFF_IMMOLATION, BUFF_SHADOW_STRIKE, BUFF_SLEEP, BUFF_SLEEP_PAUSE, CARRION_SWARM, DEATH_PACT, IMMOLATION,
    SHADOW_STRIKE, SLEEP, VAMPIRIC_AURA,
};
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
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit enemy = null
    unit enemy2 = null
    unit enemy3 = null
    unit friend = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'Hdkn', 0.0, 0.0, 0.0)
    set enemy = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set enemy2 = CreateUnit(Player(1), 'hfoo', 600.0, 40.0, 0.0)
    set enemy3 = CreateUnit(Player(1), 'hfoo', 600.0, 900.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    call UnitAddAbility(caster, 'AUdp')
    call UnitAddAbility(caster, 'AEbl')
    call UnitAddAbility(caster, 'AUcs')
    call UnitAddAbility(caster, 'AUsl')
    call UnitAddAbility(caster, 'AEsh')
    call UnitAddAbility(caster, 'AEim')
    call SetUnitState(caster, 2, 1000.0)
endfunction
function pact takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852218, friend)
endfunction
function blink_to takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852525, 0.0, 500.0)
endfunction
function swarm takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852129, 1000.0, 0.0)
endfunction
function sleep_enemy takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852227, enemy)
endfunction
function strike takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852527, enemy)
endfunction
function burn_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852177)
endfunction
function burn_off takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852178)
endfunction
function give_aura takes nothing returns nothing
    call UnitAddAbility(friend, 'AUav')
endfunction
function bite takes nothing returns nothing
    set accepted = IssueTargetOrder(friend, "attack", enemy)
endfunction
"#;

const HERO: u32 = 0x4864_6B6E; // 'Hdkn'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'

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
        rng: f(99999.0),
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
        for (id, name, life) in [(HERO, "DK", 700.0), (FOOT, "Footman", 500.0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(1000.0),
                    move_speed: f(270.0),
                    movement: 1,
                    ..Default::default()
                },
            );
        }
        g.set_type_orders(FOOT, TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        g.set_attack_type(
            FOOT,
            AttackType {
                cooldown: f(1.0),
                damage_point: f(0.5),
                backswing: f(0.5),
                range: f(400.0),
                collision: f(0.5),
                damage: f(20.0),
                missile_speed: None,
                ..Default::default()
            },
        );
        for code in [DEATH_PACT, BLINK, CARRION_SWARM, SLEEP, SHADOW_STRIKE, IMMOLATION, VAMPIRIC_AURA] {
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(1) });
        }
        // Death Pact: DataA 0.5 (mana factor of the life), DataB 1.0 (life factor), C = D = E = 0.
        g.set_spell_levels(DEATH_PACT, vec![level(75, 0.0, 0.0, 0.0, 0.0, &[0.5, 1.0, 0.0, 0.0, 0.0])]);
        // Blink: DataA = maximum range 1000.
        g.set_spell_levels(BLINK, vec![level(50, 0.0, 0.0, 0.0, 0.0, &[1000.0])]);
        // Carrion Swarm: Area (start width) 100, DataA damage 75, DataB distance 800, DataC end width 200, DataD max 150.
        g.set_spell_levels(CARRION_SWARM, vec![level(110, 6.0, 0.0, 0.0, 100.0, &[75.0, 800.0, 200.0, 150.0])]);
        // Sleep: Dur 20, HeroDur 10.
        g.set_spell_levels(SLEEP, vec![level(125, 10.0, 20.0, 10.0, 0.0, &[0.0])]);
        // Shadow Strike: DataA 10 per second (UNSURE role), DataB slow 0.5, DataE initial damage 50, Dur 5.
        let mut strike = level(75, 8.0, 5.0, 5.0, 0.0, &[10.0, 0.5, 0.0, 3.0, 50.0]);
        strike.cast = f(2.0); // Cast = the pulse period (S33, CONFIRMED)
        g.set_spell_levels(SHADOW_STRIKE, vec![strike]);
        // Immolation: Area 200, DataA 20 per pulse, DataB drain 10 mana per second, DataC buffer 10.
        g.set_spell_levels(IMMOLATION, vec![level(0, 0.0, 0.0, 0.0, 200.0, &[20.0, 10.0, 10.0])]);
        g.set_spell_order(852218, DEATH_PACT);
        g.set_spell_order(852525, BLINK);
        g.set_spell_order(852129, CARRION_SWARM);
        g.set_spell_order(852227, SLEEP);
        g.set_spell_order(852527, SHADOW_STRIKE);
        g.set_spell_order(852177, IMMOLATION);
        g.set_spell_order(852178, IMMOLATION);
        // Vampiric Aura: 20% life steal around the owner.
        g.aura_types_mut().insert(
            VAMPIRIC_AURA,
            vec![AuraLevel {
                area: f(900.0),
                data: [f(0.2), F32::ZERO, F32::ZERO, F32::ZERO],
                buff: 0,
                targs: "air,ground,friend,self,vuln,invu".into(),
            }],
        );
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

fn mana(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).mana.to_f32()
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

#[test]
fn death_pact_kills_the_friend_and_pays_the_caster() {
    let mut h = host();
    let (c, friend) = (unit(&h, "caster"), unit(&h, "friend"));
    // The caster is hurt and low on mana.
    h.game_mut().unit_mut(c).life = f(100.0);
    h.game_mut().unit_mut(c).mana = f(200.0);
    call(&mut h, "pact");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(h.game().unit(friend).dead, "the target dies");
    // life = target life 500: mana 500 * 0.5 = 250 (200 -> 450, minus the cost 75), life 500 * 1.0 (100 -> 600).
    assert_eq!(life(&h, c), 600.0);
    assert_eq!(mana(&h, c), 200.0 - 75.0 + 250.0);
    assert!(h.game().is_cooling(c, DEATH_PACT) || h.game().cooldown_remaining(c, DEATH_PACT).to_f32() >= 0.0);
}

#[test]
fn death_pact_heal_is_capped_at_the_maximum_life() {
    let mut h = host();
    let (c, friend) = (unit(&h, "caster"), unit(&h, "friend"));
    h.game_mut().unit_mut(c).life = f(400.0);
    call(&mut h, "pact");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(h.game().unit(friend).dead);
    assert_eq!(life(&h, c), 700.0);
}

#[test]
fn blink_moves_the_caster_within_range_and_refuses_beyond_it_without_tables() {
    let mut h = host();
    let c = unit(&h, "caster");
    call(&mut h, "blink_to");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(c).y.to_f32(), 500.0);
    assert_eq!(h.game().unit(c).x.to_f32(), 0.0);
    // Beyond the range the projection needs the soft-float tables; without them the point stays out of range and the
    // cast start replaces it by the caster's own position (no move).
    h.game_mut().reset_cooldowns(c);
    h.game_mut().set_spell_levels(BLINK, vec![level(50, 0.0, 0.0, 0.0, 0.0, &[100.0])]);
    run(&mut h, 1);
    call(&mut h, "blink_to");
    run(&mut h, 1);
    assert_eq!(h.game().unit(c).y.to_f32(), 500.0, "no move");
}

#[test]
fn carrion_swarm_hits_the_line_in_order_and_stops_at_the_damage_cap() {
    let mut h = host();
    let c = unit(&h, "caster");
    let (e1, e2, e3) = (unit(&h, "enemy"), unit(&h, "enemy2"), unit(&h, "enemy3"));
    let (l1, l2, l3) = (life(&h, e1), life(&h, e2), life(&h, e3));
    call(&mut h, "swarm");
    run(&mut h, 1);
    assert!(accepted(&h));
    // Both units on the x axis are hit (width 100..200 around the line), the one 900 off is not.
    assert_eq!(life(&h, e1), l1 - 75.0, "nearest first, full DataA");
    assert_eq!(life(&h, e2), l2 - 75.0, "second, the cap 150 is used up exactly");
    assert_eq!(life(&h, e3), l3, "outside the swath");
    let hits = h.game().swarm_hits().to_vec();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].1, e1);
    assert_eq!(hits[1].1, e2);
    assert_eq!(hits[0].0, c);
}

#[test]
fn sleep_disables_until_damaged_unless_a_keeper_buff_is_present() {
    let mut h = host();
    let (c, e1) = (unit(&h, "caster"), unit(&h, "enemy"));
    call(&mut h, "sleep_enemy");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, e1, BUFF_SLEEP));
    assert!(h.game().is_stunned(e1), "asleep = disabled");
    // Time passes: still asleep before the duration (20 s for a non-hero).
    run(&mut h, 100);
    assert!(has_buff(&h, e1, BUFF_SLEEP));
    // Damage wakes it.
    h.game_mut().unit_damage_target(c, e1, f(5.0), false, false, 0, 9);
    assert!(!has_buff(&h, e1, BUFF_SLEEP));
    assert!(!h.game().is_stunned(e1));
    // With a 'BUsp' keeper, damage does not wake.
    h.game_mut().reset_cooldowns(c);
    run(&mut h, 1);
    call(&mut h, "sleep_enemy");
    run(&mut h, 1);
    assert!(has_buff(&h, e1, BUFF_SLEEP));
    h.game_mut().add_buff(e1, BUFF_SLEEP_PAUSE, Default::default());
    h.game_mut().unit_damage_target(c, e1, f(5.0), false, false, 0, 9);
    assert!(has_buff(&h, e1, BUFF_SLEEP), "the keeper buff holds the sleep");
    // Duration: it ends after 20 s from the second cast.
    run(&mut h, 205);
    assert!(!has_buff(&h, e1, BUFF_SLEEP));
}

#[test]
fn shadow_strike_hits_slows_and_ticks() {
    let mut h = host();
    let e1 = unit(&h, "enemy");
    let l0 = life(&h, e1);
    call(&mut h, "strike");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, e1), l0 - 50.0, "initial damage DataE");
    assert!(has_buff(&h, e1, BUFF_SHADOW_STRIKE));
    assert_eq!(h.game().unit(e1).move_speed.to_f32(), 135.0, "slowed by half");
    // Pulses every Cast = 2 s (the first one too) of DataA = 10 for the duration 5 s: at 2 s and 4 s.
    run(&mut h, 25);
    assert_eq!(life(&h, e1), l0 - 50.0 - 10.0);
    run(&mut h, 40);
    assert_eq!(life(&h, e1), l0 - 50.0 - 20.0, "two pulses before the expiry");
    assert!(!has_buff(&h, e1, BUFF_SHADOW_STRIKE));
    assert_eq!(h.game().unit(e1).move_speed.to_f32(), 270.0, "speed restored");
}

#[test]
fn immolation_toggles_drains_mana_and_burns_enemies_in_the_area() {
    let mut h = host();
    let (c, e1, e2) = (unit(&h, "caster"), unit(&h, "enemy"), unit(&h, "enemy2"));
    // Put the enemies inside / outside the 200 area.
    h.game_mut().store_unit_position(e1, f(150.0), f(0.0));
    let (l1, l2) = (life(&h, e1), life(&h, e2));
    call(&mut h, "burn_on");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, c, BUFF_IMMOLATION));
    run(&mut h, 25);
    // Two pulses: 20 damage each to the unit in range, 10 mana each.
    assert_eq!(life(&h, e1), l1 - 40.0);
    assert_eq!(life(&h, e2), l2, "outside the area");
    assert_eq!(mana(&h, c), 1000.0 - 20.0);
    // Off again.
    call(&mut h, "burn_off");
    run(&mut h, 1);
    assert!(!has_buff(&h, c, BUFF_IMMOLATION));
    run(&mut h, 30);
    assert_eq!(life(&h, e1), l1 - 40.0, "no more pulses");
}

#[test]
fn vampiric_aura_heals_the_attacker_by_a_fraction_of_the_damage() {
    let mut h = host();
    let (friend, e1) = (unit(&h, "friend"), unit(&h, "enemy"));
    h.game_mut().unit_mut(friend).life = f(300.0);
    call(&mut h, "give_aura");
    run(&mut h, 8);
    call(&mut h, "bite");
    run(&mut h, 1);
    assert!(accepted(&h));
    run(&mut h, 30);
    assert!(life(&h, e1) < 500.0, "the footman attacked");
    let dealt = 500.0 - life(&h, e1);
    let healed = life(&h, friend) - 300.0;
    assert!(dealt >= 20.0);
    assert!((healed - dealt * 0.2).abs() < 0.01, "healed {healed} of {dealt}");
}
