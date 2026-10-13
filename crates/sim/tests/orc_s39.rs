//! S39 orc spells end to end (`crates/sim/src/orc.rs`, `docs/spec/abilities-orc-heroes.md` sections 13 to 16): War Stomp's
//! area filter, Serpent Ward's point and facing, Hex's creature pick. The tables of the DLL (`GAME_DLL`) are needed for
//! Hex and the ward timing; those tests return early without it.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::orc::*;
use sim::rng::RngTable;
use sim::ScriptHost;
use softfloat::{Tables, F32};

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
    unit ground = null
    unit flier = null
    unit tower = null
    unit ally = null
    unit far = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'Obla', 0.0, 0.0, 0.0)
    set ground = CreateUnit(Player(1), 'hfoo', 100.0, 0.0, 0.0)
    set flier = CreateUnit(Player(1), 'hgry', -100.0, 0.0, 0.0)
    set tower = CreateUnit(Player(1), 'hbar', 0.0, 100.0, 0.0)
    set ally = CreateUnit(Player(0), 'hfoo', 0.0, -100.0, 0.0)
    set far = CreateUnit(Player(1), 'hfoo', 900.0, 0.0, 0.0)
    call UnitAddAbility(caster, 'AOws')
    call UnitAddAbility(caster, 'AOsw')
    call UnitAddAbility(caster, 'AOhx')
    call UnitAddAbility(caster, 'AOvd')
    call UnitAddAbility(caster, 'AOww')
    call SetUnitState(caster, 2, 1000.0)
endfunction
function stomp takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852127)
endfunction
function ward takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852504, 300.0, 50.0)
endfunction
function storm takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852128)
endfunction
function voodoo takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852503)
endfunction
function hex takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852502, ground)
endfunction
"#;

const HERO: u32 = 0x4F62_6C61; // 'Obla'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const GRYPHON: u32 = 0x6867_7279; // 'hgry'
const BARRACKS: u32 = 0x6862_6172; // 'hbar'
const WARD: u32 = 0x6F73_7031; // 'osp1'
const RAT: u32 = 0x6E72_6174; // 'nrat'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn lvl(cost: i32, cool: f32, dur: f32, hero_dur: f32, area: f32, rng: f32, data: [f32; 3]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (slot, v) in d.iter_mut().zip(data) {
        *slot = f(v);
    }
    SpellLevel { dur: f(dur), hero_dur: f(hero_dur), cost, cool: f(cool), area: f(area), rng: f(rng), data: d, ..Default::default() }
}

fn host(with_tables: bool) -> Option<ScriptHost> {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let (tables, rng) = if with_tables {
        let dll = dll()?;
        (Some(Tables::from_game_dll(&dll).expect("tables")), Some(RngTable::from_game_dll(&dll).expect("rng")))
    } else {
        (None, None)
    };
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), tables).expect("vm");
    if let Some(t) = rng {
        h.set_rng_table(t);
        h.start_game_rng(0x0BAD_5EED);
    }
    {
        let mut g = h.game_mut();
        for (id, name, life, mv, targ) in [
            (HERO, "Blademaster", 600.0, 1, 2),
            (FOOT, "Footman", 500.0, 1, 2),
            (GRYPHON, "Gryphon", 500.0, 2, 4),
            (BARRACKS, "Barracks", 500.0, 0, 8),
            (WARD, "Ward", 200.0, 0, 0x10),
            (RAT, "Rat", 50.0, 1, 2),
        ] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(1000.0),
                    move_speed: f(300.0),
                    movement: mv,
                    targ_type: targ,
                    ..Default::default()
                },
            );
        }
        for code in [WAR_STOMP, SERPENT_WARD, HEX, VOODOO, BLADESTORM] {
            g.ability_types_mut().insert(code, AbilityType { max_level: Some(1) });
        }
        g.set_spell_levels(WAR_STOMP, vec![lvl(90, 6.0, 3.0, 2.0, 250.0, 0.0, [25.0, 0.0, 0.0])]);
        g.set_spell_levels(SERPENT_WARD, vec![lvl(30, 6.5, 40.0, 40.0, 0.0, 500.0, [1.0, 0.0, 0.0])]);
        g.set_summon_units(SERPENT_WARD, vec![WARD]);
        g.set_spell_levels(HEX, vec![lvl(70, 7.0, 15.0, 4.0, 0.0, 800.0, [99.0, 0.0, 0.0])]);
        g.set_spell_levels(VOODOO, vec![lvl(200, 180.0, 3.0, 3.0, 800.0, 0.0, [0.0, 0.0, 0.0])]);
        g.set_spell_levels(BLADESTORM, vec![lvl(130, 0.0, 5.0, 5.0, 200.0, 0.0, [110.0, 0.0, 0.0])]);
        g.set_orc_lists(HEX, vec![["nrat".into(), "nalb".into(), "nsha".into(), "nshw".into()]]);
        g.production_mut().building_angle = f(270.0);
    }
    h.start("setup", &[]).expect("setup");
    h.step();
    Some(h)
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
}

fn cast(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
    run(h, 1);
    assert!(h.vm().global("accepted").and_then(|v| v.as_boolean()).unwrap(), "{name} was refused");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

#[test]
fn stomp_hits_enemy_ground_units_only() {
    let mut h = host(false).unwrap();
    let (ground, flier, tower, ally, far) =
        (unit(&h, "ground"), unit(&h, "flier"), unit(&h, "tower"), unit(&h, "ally"), unit(&h, "far"));
    cast(&mut h, "stomp");
    assert_eq!(life(&h, ground), 475.0, "an enemy ground unit takes DataA");
    assert!(h.game().is_spell_stunned(ground), "and is stunned");
    assert_eq!(life(&h, flier), 500.0, "the targs word `ground,organic` refuses air units");
    assert_eq!(life(&h, tower), 500.0, "and structures that are not ground targets");
    assert_eq!(life(&h, ally), 500.0, "the relation bits are enemy and neutral only");
    assert_eq!(life(&h, far), 500.0, "outside the area");
}

#[test]
fn stomp_words_follow_the_host_targs() {
    let mut h = host(false).unwrap();
    // `air,ground,organic`: the gryphon is hit too.
    h.game_mut().set_orc_targs(WAR_STOMP, vec![0x0400_0006]);
    let flier = unit(&h, "flier");
    cast(&mut h, "stomp");
    assert_eq!(life(&h, flier), 475.0);
}

#[test]
fn serpent_ward_stands_on_the_target_point_facing_the_building_angle() {
    let Some(mut h) = host(true) else {
        return;
    };
    cast(&mut h, "ward");
    run(&mut h, 40);
    let wards: Vec<ObjKey> = h.game().unit_objects().filter(|(_, u)| u.type_id == WARD).map(|(k, _)| k).collect();
    assert_eq!(wards.len(), 1, "DataA = 1 ward");
    let w = wards[0];
    let g = h.game();
    let u = g.unit(w);
    assert_eq!((u.x.to_f32(), u.y.to_f32()), (300.0, 50.0), "at the ordered point");
    assert!(u.summoned);
    assert!((u.facing.to_f32() - 4.712389).abs() < 1e-4, "BuildingAngle 270 degrees: {}", u.facing.to_f32());
}

#[test]
fn hex_picks_a_creature_with_one_stream_24_draw() {
    let Some(mut h) = host(true) else {
        return;
    };
    // Only 'nrat' of the default list exists as a unit type, so the pick is forced but still consumes one draw.
    let ground = unit(&h, "ground");
    let before = h.rng().clone();
    cast(&mut h, "hex");
    run(&mut h, 20);
    assert_eq!(h.game().hex_form(ground), Some(RAT));
    let after = h.rng().clone();
    let table = RngTable::from_game_dll(&dll().unwrap()).unwrap();
    let mut expect = before.clone();
    let got = sim::damage::roll_total(expect.stream_mut(HEX_STREAM), &table, 1, 1, -1).unwrap();
    assert_eq!(got, 0);
    assert_eq!(after, expect, "exactly one stream-24 step");
}

#[test]
fn voodoo_makes_friends_in_the_area_invulnerable_while_it_pulses() {
    let mut h = host(false).unwrap();
    let (c, ally, ground, far) = (unit(&h, "caster"), unit(&h, "ally"), unit(&h, "ground"), unit(&h, "far"));
    cast(&mut h, "voodoo");
    assert!(h.game().unit_is_invulnerable(ally), "a friend in the area");
    assert!(!h.game().unit_is_invulnerable(c), "no `self` in the targs: the caster is not an area target");
    assert!(!h.game().unit_is_invulnerable(ground), "an enemy");
    let _ = far;
    run(&mut h, 20);
    assert!(h.game().unit_is_invulnerable(ally), "refreshed by the pulses");
    run(&mut h, 80);
    assert!(!h.game().unit_is_invulnerable(ally), "the voodoo ended after Dur and the last buff ran out");
    assert_eq!(voodoo_buff_duration().to_f32(), 1.0);
}

#[test]
fn bladestorm_hurts_ground_units_and_structures_but_not_air() {
    let mut h = host(false).unwrap();
    let (ground, flier, tower, ally) = (unit(&h, "ground"), unit(&h, "flier"), unit(&h, "tower"), unit(&h, "ally"));
    cast(&mut h, "storm");
    run(&mut h, 60);
    assert!(life(&h, ground) < 500.0, "ground enemy");
    assert!(life(&h, tower) < 500.0, "the shipped targs name `structure`, so buildings are hit too");
    assert_eq!(life(&h, flier), 500.0, "air is not in the targs");
    assert_eq!(life(&h, ally), 500.0, "an ally");
}
