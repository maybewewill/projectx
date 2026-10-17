//! Behaviour of Blizzard (`AHbz`) and Mass Teleport (`AHmt`) through `ScriptHost` (spec
//! `docs/spec/abilities-human-heroes.md` sections 11 and 12). Pinned first-level numbers, not game data.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::human_heroes::*;
use sim::natives::game::{ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const SRC: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native KillUnit takes unit whichUnit returns nothing
globals
    unit mage = null
    unit mage2 = null
    unit near = null
    unit nearB = null
    unit far = null
    unit ally = null
    unit allyFar = null
    unit dest = null
    unit foeA = null
    unit foeB = null
    unit foeFar = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set mage = CreateUnit(Player(0), 'Hamg', 0.0, 0.0, 0.0)
    call UnitAddAbility(mage, 'AHbz')
    call UnitAddAbility(mage, 'AHmt')
    call SetUnitState(mage, 2, 500.0)
    set foeA = CreateUnit(Player(1), 'hfoo', 1000.0, 0.0, 0.0)
    set foeB = CreateUnit(Player(1), 'hfoo', 1100.0, 50.0, 0.0)
    set foeFar = CreateUnit(Player(1), 'hfoo', 1900.0, 0.0, 0.0)
    set ally = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    set near = CreateUnit(Player(0), 'hfoo', 0.0, 200.0, 0.0)
    set nearB = CreateUnit(Player(0), 'hfoo', 0.0, 300.0, 0.0)
    set allyFar = CreateUnit(Player(0), 'hfoo', 3000.0, 0.0, 0.0)
    set dest = CreateUnit(Player(0), 'hfoo', 5000.0, 5000.0, 0.0)
    set mage2 = CreateUnit(Player(0), 'Hamg', 0.0, -3000.0, 0.0)
    call UnitAddAbility(mage2, 'AHbz')
    call SetUnitState(mage2, 2, 500.0)
endfunction
function blizzard2 takes nothing returns nothing
    set accepted = IssuePointOrderById(mage2, 852089, 1050.0, 0.0)
endfunction
function blizzard takes nothing returns nothing
    set accepted = IssuePointOrderById(mage, 852089, 1050.0, 0.0)
endfunction
function teleport takes nothing returns nothing
    set accepted = IssueTargetOrderById(mage, 852093, dest)
endfunction
function walk takes nothing returns nothing
    set accepted = IssuePointOrderById(mage, 851986, 0.0, 500.0)
endfunction
function halt takes nothing returns nothing
    set accepted = IssueImmediateOrderById(mage, 851972)
endfunction
function slay takes nothing returns nothing
    call KillUnit(mage)
endfunction
"#;

const MAGE: u32 = 0x4861_6D67;
const FOOT: u32 = 0x6866_6F6F;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(cast: f32, area: f32, data: [f32; 6]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel {
        cast: f(cast),
        dur: F32::ZERO,
        hero_dur: F32::ZERO,
        cost: 75,
        cool: f(6.0),
        area: f(area),
        rng: f(99999.0),
        data: d,
        unit_id: 0,
    }
}

fn started() -> ScriptHost {
    let prog = jass::parse(SRC).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life) in [(MAGE, "Archmage", 700.0), (FOOT, "Footman", 500.0)] {
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
            MAGE,
            sim::orders::TypeOrderData {
                abilities: vec![sim::orders::AbilityKind::Move],
            },
        );
        for base in [BLIZZARD, MASS_TELEPORT] {
            g.ability_types_mut().insert(base, AbilityType { max_level: Some(1) });
        }
        // Blizzard level 1 (usual numbers): Cast 1, Area 200, 6 waves, 30 damage, 6 shards, building factor 0.5,
        // cap 150.
        g.set_spell_levels(BLIZZARD, vec![level(1.0, 200.0, [6.0, 30.0, 6.0, 0.5, 0.0, 150.0])]);
        // Mass Teleport: Area 700, 24 units, delay 3, clustering on.
        g.set_spell_levels(MASS_TELEPORT, vec![level(0.0, 700.0, [24.0, 3.0, 1.0, 0.0, 0.0, 0.0])]);
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
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

#[test]
fn blizzard_hurts_enemies_in_the_area_once_per_wave() {
    let mut h = started();
    let (a, b, far) = (unit(&h, "foeA"), unit(&h, "foeB"), unit(&h, "foeFar"));
    h.start("blizzard", &[]).unwrap();
    run(&mut h, 1);
    assert!(accepted(&h));
    // Blizzard's level init sets the stage-1 delay to 0 (`Cast` is only the wave interval): the effect starts at once
    // and the first damage wave comes 0.9 s later (t = 0.1 s now).
    run(&mut h, 7);
    assert_eq!(life(&h, a), 500.0, "no damage before the first wave");
    run(&mut h, 3);
    // Two enemies in range: 30 each (30 * 2 = 60 is below the cap of 150).
    assert_eq!(life(&h, a), 470.0);
    assert_eq!(life(&h, b), 470.0);
    assert_eq!(life(&h, far), 500.0, "out of the area");
    // The wave interval is Cast (1 s): after 6 waves (the last at 5.9 s) the effect is over.
    run(&mut h, 51);
    assert_eq!(life(&h, a), 500.0 - 30.0 * 6.0);
    run(&mut h, 40);
    assert_eq!(life(&h, a), 500.0 - 30.0 * 6.0, "exactly DataA waves");
    assert_eq!(life(&h, unit(&h, "ally")), 500.0, "allies are not hurt");
}

#[test]
fn blizzard_wave_cap_divides_the_damage() {
    let mut h = started();
    h.game_mut().set_spell_levels(BLIZZARD, vec![level(1.0, 200.0, [1.0, 100.0, 6.0, 0.5, 0.0, 150.0])]);
    let (a, b) = (unit(&h, "foeA"), unit(&h, "foeB"));
    h.start("blizzard", &[]).unwrap();
    run(&mut h, 40);
    // 100 * 2 = 200 exceeds the cap 150, which needs the soft-float tables; without them the damage stays 100.
    assert!(life(&h, a) <= 400.0 && life(&h, b) <= 400.0);
}

#[test]
fn mass_teleport_moves_the_caster_and_nearby_allies_after_the_delay() {
    let mut h = started();
    let (mage, ally, far, dest, foe) = (
        unit(&h, "mage"),
        unit(&h, "ally"),
        unit(&h, "allyFar"),
        unit(&h, "dest"),
        unit(&h, "foeA"),
    );
    h.start("teleport", &[]).unwrap();
    run(&mut h, 1);
    assert!(accepted(&h));
    run(&mut h, 20);
    assert!(h.game().unit(mage).x.to_f32() < 10.0, "still channelling");
    run(&mut h, 30);
    let dx = h.game().unit(dest).x.to_f32();
    assert!((h.game().unit(mage).x.to_f32() - dx).abs() < 1.0, "the caster arrived");
    // Clustering keeps the ally's offset (+100, 0) from the caster.
    assert!((h.game().unit(ally).x.to_f32() - (dx + 100.0)).abs() < 1.0);
    assert!(h.game().unit(far).x.to_f32() > 2500.0, "out of the 700 area");
    assert!(h.game().unit(foe).x.to_f32() < 2000.0, "enemies stay");
}

#[test]
fn mass_teleport_clustering_needs_the_misc_key_and_data_c() {
    // Without Misc `MassTeleportCluster` (the stock game) units keep their offset; with it they all arrive on the
    // destination position (Unicorn: `0x6F056980`).
    let mut h = started();
    h.game_mut().set_mass_teleport_cluster(true);
    let (mage, ally, dest) = (unit(&h, "mage"), unit(&h, "ally"), unit(&h, "dest"));
    h.start("teleport", &[]).unwrap();
    run(&mut h, 40);
    let (dx, dy) = (h.game().unit(dest).x.to_f32(), h.game().unit(dest).y.to_f32());
    for k in [mage, ally] {
        assert!((h.game().unit(k).x.to_f32() - dx).abs() < 1.0 && (h.game().unit(k).y.to_f32() - dy).abs() < 1.0);
    }
}

#[test]
fn mass_teleport_count_limit_keeps_the_nearest() {
    let mut h = started();
    // DataA = 3: the caster plus 2 others (limit = 3 - 1); the two allies nearest to the caster stay in.
    h.game_mut().set_spell_levels(MASS_TELEPORT, vec![level(0.0, 700.0, [3.0, 1.0, 0.0, 0.0, 0.0, 0.0])]);
    let (ally, near, near_b) = (unit(&h, "ally"), unit(&h, "near"), unit(&h, "nearB"));
    h.start("teleport", &[]).unwrap();
    run(&mut h, 40);
    let moved = |k: ObjKey| h.game().unit(k).x.to_f32() > 1000.0;
    // distances: ally 100, near 200, nearB 300 -> ally and near go.
    assert!(moved(ally) && moved(near) && !moved(near_b));
}

// ---- the channel lifecycle (cast.rs, spec section 14) ----

/// The spell events of `base` fired by `caster`, as event indexes in order (0 CHANNEL .. 4 ENDCAST).
fn events(h: &ScriptHost, caster: ObjKey, base: u32) -> Vec<u32> {
    h.game()
        .spell_log()
        .iter()
        .filter(|(c, b, _)| *c == caster && *b == base)
        .map(|(_, _, e)| *e)
        .collect()
}

#[test]
fn blizzard_finishes_after_waves_times_interval() {
    let mut h = started();
    let mage = unit(&h, "mage");
    h.start("blizzard", &[]).unwrap();
    run(&mut h, 1);
    assert_eq!(events(&h, mage, BLIZZARD), vec![0, 1, 2], "CHANNEL, CAST, EFFECT; the cast is still open");
    run(&mut h, 50);
    assert_eq!(events(&h, mage, BLIZZARD), vec![0, 1, 2], "channel time 6 x 1 s: not finished at 5.2 s");
    run(&mut h, 10);
    assert_eq!(events(&h, mage, BLIZZARD), vec![0, 1, 2, 3, 4], "FINISH then ENDCAST at 6.0 s");
}

#[test]
fn a_replacing_order_ends_blizzard_without_finish_and_stops_the_waves() {
    let mut h = started();
    let (mage, a) = (unit(&h, "mage"), unit(&h, "foeA"));
    h.start("blizzard", &[]).unwrap();
    run(&mut h, 26); // t = 2.7 s: waves at 0.9 and 1.9 s
    assert_eq!(life(&h, a), 440.0);
    h.start("walk", &[]).unwrap();
    run(&mut h, 60);
    assert_eq!(life(&h, a), 440.0, "no further wave after the caster's order");
    assert_eq!(events(&h, mage, BLIZZARD), vec![0, 1, 2, 4], "ENDCAST without FINISH");
}

#[test]
fn the_stop_order_ends_a_channel_and_death_ends_it_too() {
    let mut h = started();
    let (mage, a) = (unit(&h, "mage"), unit(&h, "foeA"));
    h.start("blizzard", &[]).unwrap();
    run(&mut h, 12);
    h.start("halt", &[]).unwrap();
    run(&mut h, 30);
    assert_eq!(events(&h, mage, BLIZZARD), vec![0, 1, 2, 4]);
    assert_eq!(life(&h, a), 470.0, "one wave (0.9 s) before the stop at 1.3 s");
    // Death: a new channel, the caster dies after 2 s.
    h.game_mut().write_mana(mage, f(500.0));
    h.game_mut().reset_cooldowns(mage);
    run(&mut h, 2);
    h.start("blizzard", &[]).unwrap();
    run(&mut h, 20);
    h.start("slay", &[]).unwrap();
    run(&mut h, 40);
    let ev = events(&h, mage, BLIZZARD);
    assert_eq!(&ev[4..], &[0, 1, 2, 4], "the second cast ended by death");
    assert!(life(&h, a) > 400.0, "waves stop with the caster");
}

#[test]
fn two_blizzards_do_not_stack_and_invulnerable_units_are_skipped() {
    let mut h = started();
    let (a, b) = (unit(&h, "foeA"), unit(&h, "foeB"));
    h.game_mut().ability_types_mut().insert(BLIZZARD, AbilityType { max_level: Some(1) });
    // The shield of foeB: an invulnerability count, like Divine Shield / 'Avul' (the enumeration word has no `invu`).
    h.game_mut().invuln_add(b);
    h.start("blizzard", &[]).unwrap();
    h.start("blizzard2", &[]).unwrap();
    run(&mut h, 12); // first wave at 0.9 s from both casters
    // One marker per unit (`BHbz`): the second Blizzard's wave in the same window deals nothing.
    assert_eq!(life(&h, a), 470.0, "30, not 60");
    assert_eq!(life(&h, b), 500.0, "an invulnerable unit is not enumerated");
    run(&mut h, 10); // the next waves come after the marker (interval - 0.01) has run out
    assert_eq!(life(&h, a), 440.0);
}

#[test]
fn a_stun_ends_a_channel() {
    let mut h = started();
    let (mage, a) = (unit(&h, "mage"), unit(&h, "foeA"));
    h.start("blizzard", &[]).unwrap();
    run(&mut h, 12);
    h.game_mut().add_spell_buff(mage, sim::cast::BUFF_STUN, sim::cast::BuffEffect::Stun, f(5.0));
    run(&mut h, 40);
    assert_eq!(events(&h, mage, BLIZZARD), vec![0, 1, 2, 4]);
    assert_eq!(life(&h, a), 470.0);
}

#[test]
fn mass_teleport_teleports_at_the_channel_end_and_a_stop_cancels_it() {
    let mut h = started();
    let mage = unit(&h, "mage");
    h.start("teleport", &[]).unwrap();
    run(&mut h, 20);
    assert_eq!(events(&h, mage, MASS_TELEPORT), vec![0, 1, 2], "channelling: no FINISH yet");
    run(&mut h, 15);
    assert_eq!(events(&h, mage, MASS_TELEPORT), vec![0, 1, 2, 3, 4]);
    assert!(h.game().unit(mage).x.to_f32() > 1000.0, "arrived");
    // A second cast, stopped half way: the caster stays where it is.
    h.game_mut().write_mana(mage, f(500.0));
    h.game_mut().reset_cooldowns(mage);
    run(&mut h, 2);
    let (x0, y0) = (h.game().unit(mage).x.to_f32(), h.game().unit(mage).y.to_f32());
    h.start("teleport", &[]).unwrap();
    run(&mut h, 10);
    h.start("halt", &[]).unwrap();
    run(&mut h, 40);
    let ev = events(&h, mage, MASS_TELEPORT);
    assert_eq!(&ev[5..], &[0, 1, 2, 4], "ENDCAST without FINISH");
    assert_eq!((h.game().unit(mage).x.to_f32(), h.game().unit(mage).y.to_f32()), (x0, y0));
}
