//! Behaviour tests of the S50 summons, forms and passives (`neutral_forms.rs`): Storm Earth and Fire, Chemical Rage,
//! Robo-Goblin, Engineering Upgrade, Pocket Factory, Tornado. Level numbers are pinned from the user's `AbilityData.slk`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::neutral_forms::*;
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
"#;

const SRC: &str = r#"
globals
    unit brew = null
    unit alch = null
    unit tink = null
    unit eng = null
    unit fct = null
    unit torn = null
    unit foe = null
    unit foe2 = null
    unit hall = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set brew = CreateUnit(Player(0), 'npb0', 0.0, 0.0, 0.0)
    set alch = CreateUnit(Player(0), 'Nalc', 3000.0, 0.0, 0.0)
    set tink = CreateUnit(Player(0), 'Ntin', 6000.0, 0.0, 0.0)
    set eng = CreateUnit(Player(0), 'Neng', 9000.0, 0.0, 0.0)
    set fct = CreateUnit(Player(0), 'Nfct', 15000.0, 0.0, 0.0)
    set torn = CreateUnit(Player(0), 'ntco', 12000.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'ndmy', 12400.0, 0.0, 0.0)
    set foe2 = CreateUnit(Player(1), 'ndmy', 12400.0, 200.0, 0.0)
    set hall = CreateUnit(Player(1), 'nhll', 12300.0, 500.0, 0.0)
endfunction
function kill_part takes unit u returns nothing
    call KillUnit(u)
endfunction
function split takes nothing returns nothing
    set accepted = IssueImmediateOrderById(brew, 852586)
endfunction
function rage takes nothing returns nothing
    set accepted = IssueImmediateOrderById(alch, 852663)
endfunction
function robo takes nothing returns nothing
    set accepted = IssueImmediateOrderById(tink, 852656)
endfunction
function robo_back takes nothing returns nothing
    set accepted = IssueImmediateOrderById(tink, 852657)
endfunction
function factory takes nothing returns nothing
    set accepted = IssuePointOrderById(eng, 852658, 9300.0, 0.0)
endfunction
function factory2 takes nothing returns nothing
    set accepted = IssuePointOrderById(fct, 852658, 15300.0, 0.0)
endfunction
function tornado takes nothing returns nothing
    set accepted = IssuePointOrderById(torn, 852597, 12400.0, 0.0)
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

fn id(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
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

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        let types = g.unit_types_mut();
        types.insert(id(b"npb0"), ut(vec![ELEMENTAL_FURY]));
        for t in [b"npn1", b"npn2", b"npn3"] {
            types.insert(id(t), ut(vec![]));
        }
        types.insert(id(b"Nalc"), ut(vec![CHEMICAL_RAGE]));
        types.insert(id(b"Nalm"), ut(vec![]));
        types.insert(id(b"Ntin"), ut(vec![ROBO_GOBLIN]));
        types.insert(id(b"Nrob"), ut(vec![]));
        types.insert(id(b"Neng"), ut(vec![ENGINEERING_UPGRADE, POCKET_FACTORY]));
        types.insert(id(b"Nfct"), ut(vec![POCKET_FACTORY]));
        types.insert(id(b"nfac"), ut(vec![]));
        types.insert(id(b"nfa1"), ut(vec![]));
        types.insert(id(b"ncgb"), ut(vec![]));
        types.insert(id(b"ntor"), ut(vec![TORNADO_DAMAGE, TORNADO_SPIN]));
        types.insert(id(b"ntco"), ut(vec![TORNADO]));
        types.insert(id(b"ndmy"), ut(vec![]));
        types.insert(id(b"nhll"), ut(vec![]));
        types.set_building(id(b"nhll"), true);
        g.set_neutral_unit_list(ELEMENTAL_FURY, 0, 0, vec![id(b"npn1"), id(b"npn2"), id(b"npn3")]);
        g.set_spell_levels(
            ELEMENTAL_FURY,
            vec![SpellLevel { cost: 150, cool: f(180.0), dur: f(45.0), hero_dur: f(45.0), area: f(128.0), ..Default::default() }],
        );
        g.set_spell_levels(
            CHEMICAL_RAGE,
            vec![SpellLevel { cost: 25, cool: f(30.0), dur: f(0.35), hero_dur: f(15.0), unit_id: id(b"Nalm"), data: data(&[0.0, 1.0, 0.0, 0.0, 0.5, 0.25]), ..Default::default() }],
        );
        g.set_spell_levels(
            ROBO_GOBLIN,
            vec![SpellLevel { cost: 25, cool: f(1.0), dur: f(1.5), unit_id: id(b"Nrob"), data: data(&[0.0, 1.0, 0.0, 0.0, 5.0, 1.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            ENGINEERING_UPGRADE,
            vec![SpellLevel { data: data(&[0.1, 2.0]), ..Default::default() }],
        );
        g.set_neutral_unit_list(ENGINEERING_UPGRADE, 0, 2, vec![POCKET_FACTORY, id(b"ANs1")]);
        g.set_spell_alias(id(b"ANs1"), POCKET_FACTORY);
        g.set_spell_levels(
            POCKET_FACTORY,
            vec![SpellLevel { cost: 125, cool: f(35.0), dur: f(40.0), hero_dur: f(40.0), rng: f(500.0), unit_id: id(b"nfac"), data: data(&[5.0, 0.0, 12.0, 200.0, 1100.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            id(b"ANs1"),
            vec![SpellLevel { cost: 125, cool: f(35.0), dur: f(40.0), hero_dur: f(40.0), rng: f(500.0), unit_id: id(b"nfa1"), data: data(&[5.0, 0.0, 12.0, 200.0, 1100.0]), ..Default::default() }],
        );
        g.set_neutral_unit_list(POCKET_FACTORY, 0, 1, vec![id(b"ncgb")]);
        g.set_spell_levels(
            TORNADO,
            vec![SpellLevel { cost: 200, cool: f(120.0), dur: f(40.0), hero_dur: f(40.0), rng: f(700.0), unit_id: id(b"ntor"), ..Default::default() }],
        );
        g.set_spell_levels(
            TORNADO_DAMAGE,
            vec![SpellLevel { area: f(650.0), data: data(&[7.0, 125.0, 50.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            TORNADO_SPIN,
            vec![SpellLevel { dur: f(12.0), hero_dur: f(6.0), area: f(275.0), data: data(&[22.0, 3.0]), ..Default::default() }],
        );
        for a in [ELEMENTAL_FURY, CHEMICAL_RAGE, ROBO_GOBLIN, ENGINEERING_UPGRADE, POCKET_FACTORY, TORNADO, TORNADO_DAMAGE, TORNADO_SPIN] {
            g.ability_types_mut().insert(a, AbilityType { max_level: Some(1) });
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

fn units_of_type(h: &ScriptHost, ty: u32) -> Vec<ObjKey> {
    let g = h.game();
    (0..g.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if !u.dead && !u.removed && u.type_id == ty))
        .collect()
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn type_of(h: &ScriptHost, k: ObjKey) -> u32 {
    h.game().unit(k).type_id
}

#[test]
fn storm_earth_and_fire_splits_and_reunites_the_brewmaster() {
    let mut h = started();
    let brew = unit(&h, "brew");
    h.start("split", &[]).expect("split");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert!(h.game().is_split(brew));
    assert!(h.game().unit(brew).hidden, "the Brewmaster is out of play");
    let parts: Vec<ObjKey> = [b"npn1", b"npn2", b"npn3"].iter().flat_map(|t| units_of_type(&h, id(t))).collect();
    assert_eq!(parts.len(), 3);
    // One elemental walks off; after 45 s the survivors vanish and the Brewmaster stands where the first survivor was.
    h.game_mut().store_unit_position(parts[0], f(500.0), f(250.0));
    run(&mut h, 460);
    assert!(!h.game().is_split(brew));
    assert!(!h.game().unit(brew).hidden);
    assert_eq!(h.game().unit(brew).x, f(500.0));
    assert!(units_of_type(&h, id(b"npn1")).is_empty());
}

#[test]
fn the_brewmaster_dies_with_the_last_elemental() {
    let mut h = started();
    let brew = unit(&h, "brew");
    h.start("split", &[]).expect("split");
    run(&mut h, 4);
    let parts: Vec<ObjKey> = [b"npn1", b"npn2", b"npn3"].iter().flat_map(|t| units_of_type(&h, id(t))).collect();
    for p in parts {
        let hid = h.game_mut().handle_or_create(p);
        h.start("kill_part", &[jass::vm::Value::Handle(hid)]).expect("kill");
        run(&mut h, 1);
    }
    run(&mut h, 2);
    assert!(h.game().unit(brew).dead);
}

#[test]
fn chemical_rage_morphs_for_fifteen_seconds_and_keeps_the_abilities() {
    let mut h = started();
    let alch = unit(&h, "alch");
    h.start("rage", &[]).expect("rage");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(type_of(&h, alch), id(b"Nalc"), "the transformation takes 0.35 s");
    run(&mut h, 4);
    assert_eq!(type_of(&h, alch), id(b"Nalm"));
    assert!(h.game().unit(alch).abilities.contains(CHEMICAL_RAGE), "learned skills survive the morph");
    run(&mut h, 150);
    assert_eq!(type_of(&h, alch), id(b"Nalc"), "back to normal after HeroDur");
    assert!(!h.game().in_neutral_form(alch));
}

#[test]
fn robo_goblin_toggles_and_adds_armor() {
    let mut h = started();
    let tink = unit(&h, "tink");
    h.start("robo", &[]).expect("robo");
    run(&mut h, 20);
    assert_eq!(type_of(&h, tink), id(b"Nrob"));
    assert!(h.game().buffs(tink).iter().any(|b| b.base == BUFF_ROBO), "the defense bonus buff");
    run(&mut h, 100);
    assert_eq!(type_of(&h, tink), id(b"Nrob"), "it stays until the way back");
    h.start("robo_back", &[]).expect("back");
    run(&mut h, 20);
    assert_eq!(type_of(&h, tink), id(b"Ntin"));
    assert!(!h.game().buffs(tink).iter().any(|b| b.base == BUFF_ROBO));
}

#[test]
fn engineering_upgrade_adds_speed_and_damage_and_upgrades_the_factory() {
    let mut h = started();
    let eng = unit(&h, "eng");
    let speed = h.game().effective_move_speed(eng).to_f32();
    assert!((speed - 297.0).abs() < 0.5, "10 percent faster, got {speed}");
    assert_eq!(h.game().unit(eng).hero_attack, f(2.0), "DataB attack damage");
    h.start("factory", &[]).expect("factory");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert_eq!(units_of_type(&h, id(b"nfac")).len(), 0, "the upgraded row makes the upgraded factory");
    assert_eq!(units_of_type(&h, id(b"nfa1")).len(), 1);
}

#[test]
fn pocket_factory_builds_a_goblin_every_five_seconds_for_forty() {
    let mut h = started();
    h.start("factory2", &[]).expect("factory");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert_eq!(units_of_type(&h, id(b"nfac")).len(), 1);
    run(&mut h, 110);
    assert_eq!(units_of_type(&h, id(b"ncgb")).len(), 2, "goblins at 5 s and 10 s");
    run(&mut h, 800);
    assert_eq!(units_of_type(&h, id(b"nfac")).len(), 0, "the factory lasts 40 s");
}

#[test]
fn tornado_damages_buildings_and_tosses_nearby_units() {
    let mut h = started();
    let (foe, foe2, hall) = (unit(&h, "foe"), unit(&h, "foe2"), unit(&h, "hall"));
    h.start("tornado", &[]).expect("tornado");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert_eq!(units_of_type(&h, id(b"ntor")).len(), 1);
    run(&mut h, 25);
    // The hall is 500 away from the tornado at (12400, 0): inside 650 but outside 125: 7 per second.
    assert!(life(&h, hall) < 500.0 - 6.0, "building damage, got {}", life(&h, hall));
    assert!(h.game().is_stunned(foe), "the nearest unit is spun");
    assert!(!h.game().is_stunned(foe2) || h.game().is_stunned(foe), "one unit per scan");
}
