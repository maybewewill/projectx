//! `IsUnitType`, `UnitAddType`, `UnitRemoveType` and `IsUnitIdType` through `ScriptHost`
//! (docs/spec/unit-classification.md). The per-value rules are pinned by Unicorn vectors in `unit_class_vectors.rs`;
//! this file checks the native wiring (handles, unit state, buffs).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{Buff, BuffPredicates};
use sim::natives::game::UnitType;
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type unittype extends handle
native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native ConvertUnitType takes integer i returns unittype
native IsUnitType takes unit whichUnit, unittype whichUnitType returns boolean
native IsUnitIdType takes integer id, unittype whichUnitType returns boolean
native UnitAddType takes unit whichUnit, unittype whichUnitType returns boolean
native UnitRemoveType takes unit whichUnit, unittype whichUnitType returns boolean
"#;

const FOOTMAN: u32 = 0x6866_6F6F; // 'hfoo'
const PALADIN: u32 = 0x4870_616C; // 'Hpal'

fn host_for(src: &str) -> ScriptHost {
    let prog = jass::parse(&format!("{PRELUDE}\n{src}")).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                name: "Footman".into(),
                classification: 0x10, // peon
                movement: 1,
                weapons_on: 1,
                weapon_kind: [1, 0],
                weapon_targs: [2, 0],
                ..Default::default()
            },
        );
        g.unit_types_mut().insert(
            PALADIN,
            UnitType {
                name: "Paladin".into(),
                movement: 1,
                ..Default::default()
            },
        );
        g.set_type_orders(
            FOOTMAN,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
    }
    host
}

fn run(host: &mut ScriptHost) {
    host.start("main", &[]).expect("main starts");
    for _ in 0..4 {
        host.step();
    }
}

fn b(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| v.as_boolean())
        .unwrap_or_else(|| panic!("{name}"))
}

#[test]
fn classification_through_the_natives() {
    let mut host = host_for(
        r#"
globals
unit u
unit h
unit nul
boolean peon = false
boolean ground = false
boolean flying = false
boolean hero = false
boolean hero2 = false
boolean melee = false
boolean ranged = false
boolean atkground = false
boolean atkair = false
boolean undead0 = true
boolean add_ok = false
boolean undead1 = false
boolean rem_ok = false
boolean undead2 = true
boolean add_bad = true
boolean stunned = true
boolean idpeon = false
boolean idhero = false
boolean nullunit = true
boolean badtype = true
endglobals
function main takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set h = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    set peon = IsUnitType(u, ConvertUnitType(16))
    set ground = IsUnitType(u, ConvertUnitType(4))
    set flying = IsUnitType(u, ConvertUnitType(3))
    set hero = IsUnitType(h, ConvertUnitType(0))
    set hero2 = IsUnitType(u, ConvertUnitType(0))
    set melee = IsUnitType(u, ConvertUnitType(7))
    set ranged = IsUnitType(u, ConvertUnitType(8))
    set atkground = IsUnitType(u, ConvertUnitType(6))
    set atkair = IsUnitType(u, ConvertUnitType(5))
    set undead0 = IsUnitType(u, ConvertUnitType(14))
    set add_ok = UnitAddType(u, ConvertUnitType(14))
    set undead1 = IsUnitType(u, ConvertUnitType(14))
    set rem_ok = UnitRemoveType(u, ConvertUnitType(14))
    set undead2 = IsUnitType(u, ConvertUnitType(14))
    set add_bad = UnitAddType(u, ConvertUnitType(2))
    set stunned = IsUnitType(u, ConvertUnitType(11))
    set idpeon = IsUnitIdType('hfoo', ConvertUnitType(16))
    set idhero = IsUnitIdType('Hpal', ConvertUnitType(0))
    set nullunit = IsUnitType(nul, ConvertUnitType(16))
    set badtype = IsUnitType(u, ConvertUnitType(30))
endfunction
"#,
    );
    run(&mut host);
    assert!(b(&host, "peon") && b(&host, "ground") && !b(&host, "flying"));
    assert!(b(&host, "hero") && !b(&host, "hero2"));
    assert!(b(&host, "melee") && !b(&host, "ranged"));
    assert!(b(&host, "atkground") && !b(&host, "atkair"));
    assert!(!b(&host, "undead0") && b(&host, "add_ok") && b(&host, "undead1"));
    assert!(b(&host, "rem_ok") && !b(&host, "undead2"));
    assert!(!b(&host, "add_bad"), "structure has no settable bit");
    assert!(!b(&host, "stunned"));
    assert!(b(&host, "idpeon") && b(&host, "idhero"));
    assert!(!b(&host, "nullunit") && !b(&host, "badtype"));
}

#[test]
fn a_stun_buff_makes_the_unit_stunned() {
    let mut host = host_for(
        r#"
globals
unit u
boolean stunned = true
endglobals
function main takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
endfunction
"#,
    );
    run(&mut host);
    let id = host
        .vm()
        .global("u")
        .and_then(|v| v.as_handle())
        .expect("unit");
    {
        let mut g = host.game_mut();
        let k = g.unit_of_handle(id).expect("live");
        g.unit_mut(k).buffs.push(Buff {
            serial: 1,
            base: u32::from_be_bytes(*b"BSTN"),
            predicates: BuffPredicates::default(),
            timed_life: None,
            ..Default::default()
        });
        let u = g.unit(k).clone();
        assert!(sim::natives::unit_attr::is_unit_type(&g, &u, 11));
        assert!(!sim::natives::unit_attr::is_unit_type(&g, &u, 13));
    }
}
