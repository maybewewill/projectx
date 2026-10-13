//! The orc hero spells end to end through `ScriptHost` (`crates/sim/src/orc.rs`, `docs/spec/abilities-orc-heroes.md`):
//! War Stomp, Chain Lightning, Shockwave, Wind Walk, Mirror Image, Feral Spirit and Far Sight.
//!
//! The level data are pinned values of the tests (the 1.26a first-level numbers), not game data. The spells are cast
//! through the by-id order natives with the order ids of the model (UNSURE source, see `orc::register_orders`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::{SpellLevel, BUFF_STUN};
use sim::natives::game::{ObjKey, UnitType};
use sim::orc::*;
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
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit e1 = null
    unit e2 = null
    unit e3 = null
    unit e4 = null
    unit decoy = null
    unit ally = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'Obla', 0.0, 0.0, 0.0)
    set e1 = CreateUnit(Player(1), 'hfoo', 300.0, 0.0, 0.0)
    set e2 = CreateUnit(Player(1), 'hfoo', 400.0, 0.0, 0.0)
    set e3 = CreateUnit(Player(1), 'hfoo', 600.0, 0.0, 0.0)
    set e4 = CreateUnit(Player(1), 'hfoo', 1500.0, 0.0, 0.0)
    set decoy = CreateUnit(Player(1), 'hfoo', 300.0, 150.0, 0.0)
    set ally = CreateUnit(Player(0), 'hfoo', 50.0, 0.0, 0.0)
    call UnitAddAbility(caster, 'AOws')
    call UnitAddAbility(caster, 'AOcl')
    call UnitAddAbility(caster, 'AOsh')
    call UnitAddAbility(caster, 'AOwk')
    call UnitAddAbility(caster, 'AOmi')
    call UnitAddAbility(caster, 'AOsf')
    call UnitAddAbility(caster, 'AOfs')
    call SetUnitState(caster, 2, 1000.0)
endfunction
function stomp takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852127)
endfunction
function chain takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852119, e1)
endfunction
function shock takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852125, 700.0, 0.0)
endfunction
function walk takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852129)
endfunction
function mirror takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852093)
endfunction
function wolves takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852120)
endfunction
function sight takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852122, 2000.0, 0.0)
endfunction
"#;

const HERO: u32 = 0x4F62_6C61; // 'Obla'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const WOLF: u32 = 0x6F73_7731; // 'osw1'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

#[allow(clippy::too_many_arguments)]
fn level(cost: i32, cool: f32, dur: f32, hero_dur: f32, area: f32, rng: f32, data: [f32; 5]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (slot, v) in d.iter_mut().zip(data) {
        *slot = f(v);
    }
    SpellLevel {
        cast: F32::ZERO,
        dur: f(dur),
        hero_dur: f(hero_dur),
        cost,
        cool: f(cool),
        area: f(area),
        rng: f(rng),
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
        for (id, name, life, mv) in [(HERO, "Blademaster", 600.0, 1), (FOOT, "Footman", 500.0, 1), (WOLF, "Wolf", 300.0, 1)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(1000.0),
                    move_speed: f(300.0),
                    movement: mv,
                    ..Default::default()
                },
            );
        }
        for code in [WAR_STOMP, CHAIN_LIGHTNING, SHOCKWAVE, WIND_WALK, MIRROR_IMAGE, FERAL_SPIRIT, FAR_SIGHT] {
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(1) });
        }
        // Stomp: Dur 3 / HeroDur 2, Area 250, DataA = damage 25.
        g.set_spell_levels(WAR_STOMP, vec![level(90, 6.0, 3.0, 2.0, 250.0, 0.0, [25.0, 0.0, 0.0, 0.0, 0.0])]);
        // Chain Lightning: damage 100, 3 targets, reduction 0.15, bounce radius 500.
        g.set_spell_levels(CHAIN_LIGHTNING, vec![level(120, 9.0, 0.0, 0.0, 500.0, 700.0, [100.0, 3.0, 0.15, 0.0, 0.0])]);
        // Shockwave: damage 75, cap 99999, distance 800, final width 150, width 150.
        g.set_spell_levels(SHOCKWAVE, vec![level(100, 8.0, 0.0, 0.0, 150.0, 900.0, [75.0, 99999.0, 800.0, 150.0, 0.0])]);
        // Wind Walk: fade 0.6, speed +0.7, first strike +100; HeroDur 20.
        g.set_spell_levels(WIND_WALK, vec![level(75, 5.0, 20.0, 20.0, 0.0, 0.0, [0.6, 0.7, 100.0, 0.0, 0.0])]);
        // Mirror Image: 2 images, deal 0, take 2, 60 s.
        g.set_spell_levels(MIRROR_IMAGE, vec![level(150, 3.0, 60.0, 60.0, 1000.0, 0.0, [2.0, 0.0, 2.0, 0.5, 0.0])]);
        // Feral Spirit: 2 wolves, 60 s.
        g.set_spell_levels(FERAL_SPIRIT, vec![level(100, 15.0, 60.0, 60.0, 200.0, 0.0, [0.0, 2.0, 0.0, 0.0, 0.0])]);
        g.set_summon_units(FERAL_SPIRIT, vec![WOLF]);
        // Far Sight: 8 s, area 600.
        g.set_spell_levels(FAR_SIGHT, vec![level(75, 0.0, 8.0, 8.0, 600.0, 99999.0, [3.0, 0.0, 0.0, 0.0, 0.0])]);
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

fn cast(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
    run(h, 1);
    assert!(accepted(h), "{name} was refused");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

fn unit_count(h: &ScriptHost) -> usize {
    h.game().unit_objects().count()
}

#[test]
fn war_stomp_damages_and_stuns_enemies_in_the_area_only() {
    let mut h = host();
    let (c, e1, ally) = (unit(&h, "caster"), unit(&h, "e1"), unit(&h, "ally"));
    // Move an enemy into the 250 radius and keep the others out.
    {
        let mut g = h.game_mut();
        let near = g.unit(e1).clone();
        let _ = near;
        g.unit_mut(e1).x = f(100.0);
        g.unit_mut(e1).y = f(100.0);
    }
    cast(&mut h, "stomp");
    assert_eq!(life(&h, e1), 475.0, "DataA damage");
    assert!(has_buff(&h, e1, BUFF_STUN), "stunned");
    assert!(h.game().is_spell_stunned(e1));
    assert_eq!(life(&h, ally), 500.0, "an ally is untouched");
    assert_eq!(life(&h, unit(&h, "e2")), 500.0, "out of the area");
    assert_eq!(h.game().unit(c).mana.to_f32(), 910.0, "mana paid");
    // A footman is no hero: Dur 3 s.
    run(&mut h, 25);
    assert!(h.game().is_spell_stunned(e1));
    run(&mut h, 8);
    assert!(!h.game().is_spell_stunned(e1));
}

#[test]
fn chain_lightning_hops_to_the_nearest_unit_with_falling_damage() {
    let mut h = host();
    let (e1, e2, e3, e4, decoy) = (unit(&h, "e1"), unit(&h, "e2"), unit(&h, "e3"), unit(&h, "e4"), unit(&h, "decoy"));
    cast(&mut h, "chain");
    // First hop: the target, full damage. Second: the nearest unit to it (e2 at 100, the decoy at 150), 85 % of it.
    assert_eq!(life(&h, e1), 400.0);
    let second = 500.0 - life(&h, e2);
    assert!((84.9..=85.0).contains(&second), "second hop {second}");
    // Third: the nearest unhit unit to e2 at (400, 0): the decoy at (300, 150) is 180 away, e3 at (600, 0) 200 away.
    let third = 500.0 - life(&h, decoy);
    assert!((72.2..=72.3).contains(&third), "third hop {third}");
    assert_eq!(life(&h, e3), 500.0, "only three units are hit");
    assert_eq!(life(&h, e4), 500.0, "out of every bounce radius");
}

#[test]
fn shockwave_hits_units_on_the_line_in_range_only() {
    let mut h = host();
    let (e1, e2, e3, decoy) = (unit(&h, "e1"), unit(&h, "e2"), unit(&h, "e3"), unit(&h, "decoy"));
    {
        let mut g = h.game_mut();
        g.unit_mut(e3).x = f(900.0); // beyond 800
        g.unit_mut(decoy).y = f(200.0); // beside the line, outside the width
    }
    cast(&mut h, "shock");
    assert_eq!(life(&h, e1), 425.0, "on the line");
    assert_eq!(life(&h, e2), 425.0, "on the line");
    assert_eq!(life(&h, decoy), 500.0, "beside the line");
    assert_eq!(life(&h, e3), 500.0, "beyond the distance");
}

#[test]
fn wind_walk_speeds_up_and_the_first_strike_ends_it() {
    let mut h = host();
    let c = unit(&h, "caster");
    cast(&mut h, "walk");
    assert!(has_buff(&h, c, BUFF_WIND_WALK));
    let speed = h.game().unit(c).move_speed.to_f32();
    assert!((509.0..=510.1).contains(&speed), "speed {speed}");
    let raw = h.game_mut().orc_crit_raw(c, f(50.0));
    assert_eq!(raw.to_f32(), 150.0, "DataC added to the first strike");
    assert!(!has_buff(&h, c, BUFF_WIND_WALK));
    assert_eq!(h.game().unit(c).move_speed.to_f32(), 300.0, "speed restored");
    assert_eq!(h.game_mut().orc_crit_raw(c, f(50.0)).to_f32(), 50.0, "only the first strike");
}

#[test]
fn mirror_image_makes_illusions_that_expire() {
    let mut h = host();
    let before = unit_count(&h);
    cast(&mut h, "mirror");
    assert_eq!(unit_count(&h), before + 2);
    let images: Vec<ObjKey> = h.game().unit_objects().filter(|(_, u)| u.illusion).map(|(k, _)| k).collect();
    assert_eq!(images.len(), 2);
    // Zero damage dealt (DataB = 0).
    assert_eq!(h.game_mut().orc_crit_raw(images[0], f(40.0)).to_f32(), 0.0);
    assert_eq!(h.game().illusion_taken_factor(images[0]).to_f32(), 2.0);
}

#[test]
fn feral_spirit_summons_timed_wolves() {
    let mut h = host();
    let before = unit_count(&h);
    cast(&mut h, "wolves");
    assert_eq!(unit_count(&h), before + 2);
    let wolves: Vec<ObjKey> = h.game().unit_objects().filter(|(_, u)| u.type_id == WOLF).map(|(k, _)| k).collect();
    assert_eq!(wolves.len(), 2);
    assert!(wolves.iter().all(|&w| h.game().unit(w).summoned));
}

#[test]
fn far_sight_marks_the_caster_for_its_duration() {
    let mut h = host();
    let c = unit(&h, "caster");
    cast(&mut h, "sight");
    assert!(has_buff(&h, c, BUFF_FAR_SIGHT));
    run(&mut h, 90);
    assert!(!has_buff(&h, c, BUFF_FAR_SIGHT));
}
