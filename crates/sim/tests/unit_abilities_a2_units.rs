//! Behaviour tests of the unit-conversion abilities of `unit_abilities_a2.rs`: Couple (Acoa, Acoh, Acoi), Decouple
//! (Adec), Battlestations (Abtl) and Sacrificial Lamb (Alam). The level rows are the 1.26a first-level rows; a rawcode
//! field (`DataA` partner types) is stored as the bits of the `F32`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{code, AbilityType};
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native SetWidgetLife takes widget wid, real newLife returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
"#;

const SRC: &str = r#"
globals
    unit archer = null
    unit hippo = null
    unit far_hippo = null
    unit foreign_hippo = null
    unit rider = null
    unit acolyte = null
    unit pit = null
    unit burrow = null
    unit peon1 = null
    unit peon2 = null
    unit peon3 = null
    unit peon_far = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set archer = CreateUnit(Player(0), 'earc', 0.0, 0.0, 0.0)
    set hippo = CreateUnit(Player(0), 'ehip', 300.0, 0.0, 0.0)
    set far_hippo = CreateUnit(Player(0), 'ehip', 2000.0, 0.0, 0.0)
    set foreign_hippo = CreateUnit(Player(1), 'ehip', 50.0, 0.0, 0.0)
    set rider = CreateUnit(Player(0), 'ehpr', 1000.0, 1000.0, 0.0)
    set acolyte = CreateUnit(Player(0), 'uaco', 0.0, 500.0, 0.0)
    set pit = CreateUnit(Player(0), 'usap', 100.0, 500.0, 0.0)
    set burrow = CreateUnit(Player(0), 'obur', 3000.0, 0.0, 0.0)
    set peon1 = CreateUnit(Player(0), 'opeo', 3100.0, 0.0, 0.0)
    set peon2 = CreateUnit(Player(0), 'opeo', 3200.0, 0.0, 0.0)
    set peon3 = CreateUnit(Player(0), 'opeo', 3300.0, 0.0, 0.0)
    set peon_far = CreateUnit(Player(0), 'opeo', 9000.0, 0.0, 0.0)
    call UnitAddAbility(archer, 'Acoa')
    call UnitAddAbility(hippo, 'Acoh')
    call UnitAddAbility(rider, 'Adec')
    call UnitAddAbility(acolyte, 'Alam')
    call UnitAddAbility(burrow, 'Abtl')
    call UnitAddAbility(burrow, 'Acar')
endfunction
function couple_with_hippo takes nothing returns nothing
    set accepted = IssueTargetOrderById(archer, 852507, hippo)
endfunction
function couple_with_foreign takes nothing returns nothing
    set accepted = IssueTargetOrderById(archer, 852507, foreign_hippo)
endfunction
function couple_instant takes nothing returns nothing
    set accepted = IssueImmediateOrderById(archer, 852508)
endfunction
function hippo_couples takes nothing returns nothing
    set accepted = IssueTargetOrderById(hippo, 852507, archer)
endfunction
function split_rider takes nothing returns nothing
    call SetWidgetLife(rider, 500.0)
    set accepted = IssueImmediateOrderById(rider, 852509)
endfunction
function stations takes nothing returns nothing
    set accepted = IssueImmediateOrderById(burrow, 852099)
endfunction
function sacrifice takes nothing returns nothing
    set accepted = IssueTargetOrderById(acolyte, 852201, pit)
endfunction
"#;

const ARCHER: u32 = u32::from_be_bytes(*b"earc");
const HIPPO: u32 = u32::from_be_bytes(*b"ehip");
const RIDER: u32 = u32::from_be_bytes(*b"ehpr");
const ACOLYTE: u32 = u32::from_be_bytes(*b"uaco");
const PIT: u32 = u32::from_be_bytes(*b"usap");
const SHADE: u32 = u32::from_be_bytes(*b"ushd");
const BURROW: u32 = u32::from_be_bytes(*b"obur");
const PEON: u32 = u32::from_be_bytes(*b"opeo");

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn ut(name: &str, life: f32) -> UnitType {
    UnitType { name: name.to_string(), max_life: f(life), max_mana: f(100.0), move_speed: f(270.0), ..Default::default() }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (t, n, life) in [(ARCHER, "archer", 300.0), (HIPPO, "hippogryph", 400.0), (RIDER, "rider", 1000.0), (ACOLYTE, "acolyte", 220.0), (SHADE, "shade", 200.0), (PEON, "peon", 220.0)] {
            g.unit_types_mut().insert(t, ut(n, life));
        }
        for (t, n) in [(PIT, "pit"), (BURROW, "burrow")] {
            g.unit_types_mut().insert(t, ut(n, 800.0));
            g.unit_types_mut().set_building(t, true);
        }
        let with = |l: &mut SpellLevel, i: usize, v: u32| l.data[i] = F32::from_bits(v);
        let mut acoa = SpellLevel { rng: f(100.0), unit_id: RIDER, ..Default::default() };
        with(&mut acoa, 0, HIPPO);
        let mut acoh = SpellLevel { rng: f(100.0), unit_id: RIDER, ..Default::default() };
        with(&mut acoh, 0, ARCHER);
        let mut adec = SpellLevel { cool: f(30.0), area: f(-1.0), ..Default::default() };
        with(&mut adec, 0, ARCHER);
        with(&mut adec, 1, HIPPO);
        let mut abtl = SpellLevel { area: f(2000.0), unit_id: PEON, ..Default::default() };
        abtl.data[1] = f(1.0);
        let mut acar = SpellLevel::default();
        acar.data[0] = f(2.0);
        for (c, l) in [
            (code(b"Acoa"), acoa),
            (code(b"Acoh"), acoh),
            (code(b"Adec"), adec),
            (code(b"Abtl"), abtl),
            (code(b"Acar"), acar),
            (code(b"Alam"), SpellLevel::default()),
        ] {
            g.ability_types_mut().insert(c, AbilityType { max_level: Some(1) });
            g.set_spell_levels(c, vec![l]);
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

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
    for _ in 0..3 {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

fn count(h: &ScriptHost, ty: u32) -> usize {
    h.game().unit_objects().filter(|(_, u)| u.type_id == ty && !u.removed && !u.dead).count()
}

#[test]
fn archer_couples_with_a_hippogryph_into_a_rider() {
    let mut h = started();
    let (archer, hippo) = (unit(&h, "archer"), unit(&h, "hippo"));
    assert_eq!((count(&h, ARCHER), count(&h, RIDER)), (1, 1));
    call(&mut h, "couple_with_foreign");
    assert!(!accepted(&h), "the partner must be an own unit");
    call(&mut h, "couple_with_hippo");
    assert!(accepted(&h));
    assert!(h.game().unit(archer).removed && h.game().unit(hippo).removed);
    assert_eq!(count(&h, RIDER), 2, "a new rider stands where the archer was");
    let new = h.game().unit_objects().filter(|(_, u)| u.type_id == RIDER && !u.removed).map(|(k, _)| k).find(|&k| k != unit(&h, "rider")).unwrap();
    assert_eq!(h.game().unit(new).owner, 0);
    assert_eq!(h.game().unit(new).x.to_f32(), 0.0);
}

#[test]
fn the_hippogryph_side_couples_too_and_instant_takes_the_closest_partner() {
    let mut h = started();
    call(&mut h, "hippo_couples");
    assert!(accepted(&h));
    assert_eq!(count(&h, HIPPO), 2, "one hippogryph is used");
    // The archer is gone: the instant order of nobody is refused; use a fresh game for the closest rule.
    let mut h = started();
    let (near, far, foreign) = (unit(&h, "hippo"), unit(&h, "far_hippo"), unit(&h, "foreign_hippo"));
    call(&mut h, "couple_instant");
    assert!(accepted(&h));
    assert!(h.game().unit(near).removed, "the closest partner (300) is taken");
    assert!(!h.game().unit(far).removed);
    assert!(!h.game().unit(foreign).removed, "the enemy's hippogryph (closer) is not eligible");
}

#[test]
fn decouple_splits_the_rider_keeping_the_life_fraction() {
    let mut h = started();
    let rider = unit(&h, "rider");
    call(&mut h, "split_rider");
    assert!(accepted(&h));
    assert!(h.game().unit(rider).removed);
    // The rider had 500 / 1000 life: the archer (300) and the hippogryph (400) keep half of it.
    let at_rider: Vec<_> = h
        .game()
        .unit_objects()
        .filter(|(_, u)| !u.removed && (u.x.to_f32() - 1000.0).abs() < 40.0 && (u.y.to_f32() - 1000.0).abs() < 1.0)
        .map(|(_, u)| (u.type_id, u.life.to_f32(), u.x.to_f32()))
        .collect();
    assert_eq!(at_rider.len(), 2, "{at_rider:?}");
    let archer = at_rider.iter().find(|r| r.0 == ARCHER).unwrap();
    let hippo = at_rider.iter().find(|r| r.0 == HIPPO).unwrap();
    assert!((archer.1 - 150.0).abs() < 0.5 && (hippo.1 - 200.0).abs() < 0.5);
    assert!(archer.2 < hippo.2, "either side of the rider");
}

#[test]
fn battlestations_loads_the_allowed_units_within_the_area_up_to_the_capacity() {
    let mut h = started();
    let burrow = unit(&h, "burrow");
    call(&mut h, "stations");
    assert!(accepted(&h));
    assert_eq!(h.game().cargo_of(burrow).len(), 2, "capacity 2 (Acar DataA)");
    assert!(!h.game().cargo_of(burrow).contains(&unit(&h, "peon_far")), "9000 away: outside Area 2000");
    assert!(h.game().cargo_of(burrow).contains(&unit(&h, "peon1")), "the list order loads the first peons");
}

#[test]
fn sacrificial_lamb_turns_the_acolyte_into_a_shade_at_the_pit() {
    let mut h = started();
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(SHADE, ut("shade", 200.0));
    }
    let acolyte = unit(&h, "acolyte");
    call(&mut h, "sacrifice");
    assert!(accepted(&h));
    assert!(h.game().unit(acolyte).removed);
    let shade = h.game().unit_objects().find(|(_, u)| u.type_id == SHADE && !u.removed).map(|(k, _)| k).expect("shade");
    assert_eq!((h.game().unit(shade).x.to_f32(), h.game().unit(shade).y.to_f32()), (100.0, 500.0));
}
