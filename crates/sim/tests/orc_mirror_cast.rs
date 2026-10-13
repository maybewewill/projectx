//! Mirror Image end to end with the original's tables (`GAME_DLL`; the tests return early without it): one stream-12 draw
//! per cast, the polygon around the caster, the hero on the drawn slot, the images as illusions
//! (`crates/sim/src/orc.rs`, `docs/spec/abilities-orc-heroes.md` section 7).

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
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
"#;

const SRC: &str = r#"
globals
    unit caster = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'Obla', 1000.0, 2000.0, 90.0)
    call UnitAddAbility(caster, 'AOmi')
    call SetUnitState(caster, 2, 1000.0)
endfunction
function mirror takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852093)
endfunction
"#;

const HERO: u32 = 0x4F62_6C61; // 'Obla'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}")))
}

fn host(data_a: f32) -> Option<ScriptHost> {
    let dll = dll()?;
    let tables = Tables::from_game_dll(&dll).expect("tables");
    let table = RngTable::from_game_dll(&dll).expect("rng");
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), Some(tables)).expect("vm");
    h.set_rng_table(table);
    h.start_game_rng(0x0BAD_5EED);
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(
            HERO,
            UnitType {
                name: "Blademaster".to_string(),
                max_life: f(600.0),
                max_mana: f(1000.0),
                move_speed: f(300.0),
                movement: 1,
                ..Default::default()
            },
        );
        g.ability_types_mut().insert(MIRROR_IMAGE, AbilityType { max_level: Some(1) });
        let mut data = [F32::ZERO; 9];
        data[0] = f(data_a);
        data[2] = f(2.0);
        data[3] = f(0.5);
        g.set_spell_levels(
            MIRROR_IMAGE,
            vec![SpellLevel { dur: f(60.0), hero_dur: f(60.0), cost: 150, cool: f(3.0), area: f(1000.0), rng: f(128.0), data, ..Default::default() }],
        );
    }
    h.start("setup", &[]).expect("setup");
    h.step();
    Some(h)
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn pos(h: &ScriptHost, k: ObjKey) -> (f32, f32) {
    let g = h.game();
    (g.unit(k).x.to_f32(), g.unit(k).y.to_f32())
}

#[test]
fn one_stream_twelve_draw_and_a_triangle_of_units() {
    let Some(mut h) = host(2.0) else {
        return;
    };
    let table = RngTable::from_game_dll(&dll().unwrap()).unwrap();
    let c = unit(&h, "caster");
    let before = h.rng().clone();
    h.start("mirror", &[]).expect("start");
    h.step();
    assert!(h.vm().global("accepted").and_then(|v| v.as_boolean()).unwrap());
    // Nothing happens before DataD.
    assert_eq!(h.game().unit_objects().count(), 1);
    assert_eq!(h.rng().clone(), before, "no draw before the delay");
    for _ in 0..6 {
        h.step();
    }
    // Exactly one draw of stream 12 (one die) and no other stream moved.
    let after = h.rng().clone();
    let mut expect = before.clone();
    let chosen = mirror_pick(expect.stream_mut(MIRROR_STREAM), &table, 2).unwrap();
    assert_eq!(after, expect, "one stream 12 step");
    assert!((0..3).contains(&chosen));
    // The caster plus two illusions (the placeholder of the hero slot is gone).
    let units: Vec<(ObjKey, bool)> = h.game().unit_objects().filter(|(_, u)| !u.removed).map(|(k, u)| (k, u.illusion)).collect();
    assert_eq!(units.len(), 3);
    assert_eq!(units.iter().filter(|u| u.1).count(), 2);
    // The three units stand on an equilateral triangle of circumradius 128 around (1000, 2000).
    let pts: Vec<(f32, f32)> = units.iter().map(|&(k, _)| pos(&h, k)).collect();
    let centre = ((pts[0].0 + pts[1].0 + pts[2].0) / 3.0, (pts[0].1 + pts[1].1 + pts[2].1) / 3.0);
    assert!(dist(centre, (1000.0, 2000.0)) < 0.5, "centre {centre:?}");
    for i in 0..3 {
        for j in (i + 1)..3 {
            let d = dist(pts[i], pts[j]);
            assert!((d - 221.7).abs() < 0.5, "chord {d}");
        }
    }
    // The hero moved onto its slot: it is no longer at the centre (the polygon does not contain the centre).
    assert!(dist(pos(&h, c), (1000.0, 2000.0)) > 100.0);
}

#[test]
fn a_recast_removes_the_old_images() {
    let Some(mut h) = host(1.0) else {
        return;
    };
    h.start("mirror", &[]).expect("start");
    for _ in 0..8 {
        h.step();
    }
    let n1 = h.game().unit_objects().filter(|(_, u)| u.illusion && !u.dead).count();
    assert_eq!(n1, 1);
    // Cooldown 3 s: wait, then cast again.
    for _ in 0..35 {
        h.step();
    }
    h.start("mirror", &[]).expect("start");
    h.step();
    let alive = h.game().unit_objects().filter(|(_, u)| u.illusion && !u.dead).count();
    assert_eq!(alive, 0, "the old image died with the new cast");
    for _ in 0..8 {
        h.step();
    }
    let alive = h.game().unit_objects().filter(|(_, u)| u.illusion && !u.dead).count();
    assert_eq!(alive, 1);
}
