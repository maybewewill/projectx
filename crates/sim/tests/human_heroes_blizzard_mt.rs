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
globals
    unit mage = null
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
endfunction
function blizzard takes nothing returns nothing
    set accepted = IssuePointOrderById(mage, 852089, 1050.0, 0.0)
endfunction
function teleport takes nothing returns nothing
    set accepted = IssueTargetOrderById(mage, 852093, dest)
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
    // Cast time 1 s, then the first damage wave 0.9 s after the effect starts.
    run(&mut h, 10);
    assert_eq!(life(&h, a), 500.0, "no damage before the first wave");
    run(&mut h, 10);
    // Two enemies in range: 30 each (30 * 2 = 60 is below the cap of 150).
    assert_eq!(life(&h, a), 470.0);
    assert_eq!(life(&h, b), 470.0);
    assert_eq!(life(&h, far), 500.0, "out of the area");
    // The wave interval is Cast (1 s): after 6 waves the effect is over.
    run(&mut h, 80);
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
