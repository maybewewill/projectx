//! Behaviour tests of the on-hit and on-death abilities of `unit_abilities_a2.rs` (Acor, Afak, Aflk, Afsh, Afrb, Afrz,
//! Aliq, Adda): real attacks through the attack cycle. Level numbers are the first-level rows of the 1.26a
//! `AbilityData.slk` (or marked test rows).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{code, AbilityType};
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
native KillUnit takes unit whichUnit returns boolean
"#;

const SRC: &str = r#"
globals
    unit atk = null
    unit prey = null
    unit full = null
    unit mid = null
    unit small = null
    unit outside = null
    unit grounded = null
endglobals
function setup_air takes nothing returns nothing
    set atk = CreateUnit(Player(0), 'a000', 0.0, 0.0, 0.0)
    set prey = CreateUnit(Player(1), 'air1', 50.0, 0.0, 0.0)
    set full = CreateUnit(Player(1), 'air1', 50.0, 60.0, 0.0)
    set mid = CreateUnit(Player(1), 'air1', 50.0, 120.0, 0.0)
    set small = CreateUnit(Player(1), 'air1', 50.0, 300.0, 0.0)
    set outside = CreateUnit(Player(1), 'air1', 50.0, 400.0, 0.0)
    set grounded = CreateUnit(Player(1), 'gnd1', 50.0, 30.0, 0.0)
endfunction
function setup_ground takes nothing returns nothing
    set atk = CreateUnit(Player(0), 'a000', 0.0, 0.0, 0.0)
    set prey = CreateUnit(Player(1), 'gnd1', 50.0, 0.0, 0.0)
    set full = CreateUnit(Player(1), 'gnd1', 50.0, 60.0, 0.0)
    set mid = CreateUnit(Player(1), 'gnd1', 50.0, 120.0, 0.0)
    set small = CreateUnit(Player(1), 'gnd1', 50.0, 300.0, 0.0)
    set outside = CreateUnit(Player(1), 'gnd1', 50.0, 400.0, 0.0)
    set grounded = CreateUnit(Player(1), 'air1', 50.0, 30.0, 0.0)
endfunction
function setup_struct takes nothing returns nothing
    set atk = CreateUnit(Player(0), 'a000', 0.0, 0.0, 0.0)
    set prey = CreateUnit(Player(1), 'bld1', 50.0, 0.0, 0.0)
    set full = CreateUnit(Player(1), 'bld1', 50.0, 60.0, 0.0)
    set outside = CreateUnit(Player(1), 'bld1', 50.0, 400.0, 0.0)
    set grounded = CreateUnit(Player(1), 'gnd1', 50.0, 30.0, 0.0)
endfunction
function setup_boom takes nothing returns nothing
    set atk = CreateUnit(Player(0), 'a000', 0.0, 0.0, 0.0)
    set prey = CreateUnit(Player(1), 'gnd1', 50.0, 0.0, 0.0)
    set full = CreateUnit(Player(1), 'gnd1', 150.0, 0.0, 0.0)
    set mid = CreateUnit(Player(0), 'gnd1', 20.0, 0.0, 0.0)
    set outside = CreateUnit(Player(1), 'gnd1', 600.0, 0.0, 0.0)
endfunction
function attack takes nothing returns nothing
    call IssueTargetOrder(atk, "attack", prey)
endfunction
function boom takes nothing returns nothing
    call KillUnit(atk)
endfunction
"#;

fn c(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

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

fn host(ability: &[u8; 4], level: SpellLevel, setup: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        let ut = |name: &str, abilities: Vec<u32>, movement: u32| UnitType {
            name: name.to_string(),
            max_life: f(1000.0),
            max_mana: f(300.0),
            start_mana: f(300.0),
            move_speed: f(0.0),
            abilities,
            movement,
            ..Default::default()
        };
        g.unit_types_mut().insert(c(b"a000"), ut("attacker", vec![code(ability)], 0));
        g.unit_types_mut().insert(c(b"air1"), ut("air", vec![], 2));
        g.unit_types_mut().insert(c(b"gnd1"), ut("ground", vec![], 0));
        g.unit_types_mut().insert(c(b"bld1"), ut("building", vec![], 0));
        g.unit_types_mut().set_building(c(b"bld1"), true);
        g.set_type_orders(c(b"a000"), TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        g.set_attack_type(
            c(b"a000"),
            AttackType {
                cooldown: f(1.0),
                damage_point: f(0.5),
                backswing: f(0.5),
                range: f(100.0),
                collision: f(0.5),
                damage: f(10.0),
                missile_speed: None,
                ..Default::default()
            },
        );
        g.ability_types_mut().insert(code(ability), AbilityType { max_level: Some(1) });
        g.set_spell_levels(code(ability), vec![level]);
    }
    h.start(setup, &[]).expect("setup");
    h.step();
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

fn life(h: &ScriptHost, name: &str) -> f32 {
    h.game().unit(unit(h, name)).life.to_f32()
}

fn has_buff(h: &ScriptHost, name: &str, code: u32) -> bool {
    h.game().buffs(unit(h, name)).iter().any(|b| b.base == code)
}

/// Starts the attack and runs until the primary target has taken `hits` attack hits of 10.
fn hit(h: &mut ScriptHost, hits: u32) {
    h.start("attack", &[]).expect("attack");
    for _ in 0..400 {
        h.step();
        let taken = 1000.0 - life(h, "prey");
        if taken >= 10.0 * hits as f32 {
            break;
        }
    }
}

#[test]
fn flak_cannon_splashes_air_units_in_three_tiers_and_spares_ground() {
    // The stock row: Area 75 (full radius), DataA 150, DataB 325 (medium, small radius), DataC/D/E 7/6/5.
    let level = SpellLevel { area: f(75.0), data: data(&[150.0, 325.0, 7.0, 6.0, 5.0]), ..Default::default() };
    let mut h = host(b"Aflk", level, "setup_air");
    hit(&mut h, 1);
    let hits = (1000.0 - life(&h, "prey")) / 10.0;
    assert!(hits >= 1.0);
    // One splash per hit; the primary target only took the attack.
    assert_eq!(1000.0 - life(&h, "full"), 7.0 * hits);
    assert_eq!(1000.0 - life(&h, "mid"), 6.0 * hits);
    assert_eq!(1000.0 - life(&h, "small"), 5.0 * hits);
    assert_eq!(life(&h, "outside"), 1000.0);
    assert_eq!(life(&h, "grounded"), 1000.0, "flak targs air only");
}

#[test]
fn frag_shards_splash_ground_units_and_sort_the_radii() {
    // The stock row lists the medium radius 275 (DataA, amount DataD) and the small radius 250 (DataB, amount DataE). V7: the original
    // collector (0x6F2C2E80) gives a unit the amount of the SMALLEST ring that contains it, so 120 away takes DataE (the 250 ring).
    let level = SpellLevel { area: f(100.0), data: data(&[275.0, 250.0, 25.0, 18.0, 12.0]), ..Default::default() };
    let mut h = host(b"Afsh", level, "setup_ground");
    hit(&mut h, 1);
    let hits = (1000.0 - life(&h, "prey")) / 10.0;
    assert_eq!(1000.0 - life(&h, "full"), 25.0 * hits, "60 away: inside the full radius 100");
    assert_eq!(1000.0 - life(&h, "mid"), 12.0 * hits, "120 away: the smallest containing ring is DataB 250, amount DataE");
    assert_eq!(1000.0 - life(&h, "small"), 0.0, "300 away: outside 275");
    assert_eq!(life(&h, "grounded"), 1000.0, "air units are not ground targets");
}

#[test]
fn flaming_attack_pays_mana_and_burns_the_target_and_its_neighbours() {
    // Test row: DataA 20 fire damage, DataB 0.45 inside DataE 75, DataC 0.25 inside Area 150, DataD 1.
    let level = SpellLevel { cost: 25, area: f(150.0), data: data(&[20.0, 0.45, 0.25, 1.0, 75.0]), ..Default::default() };
    let mut h = host(b"Afak", level, "setup_ground");
    hit(&mut h, 1);
    let hits = (1000.0 - life(&h, "prey")) / 30.0;
    assert!(hits >= 1.0, "10 attack + 20 fire per hit");
    assert_eq!(300.0 - h.game().unit(unit(&h, "atk")).mana.to_f32(), 25.0 * hits);
    // full at 60 from the target is inside 75: x0.45; mid at 120 inside 150: x0.25; small at 300 is outside.
    let near = 1000.0 - life(&h, "full");
    assert!((near - 9.0 * hits).abs() < 0.01, "{near}");
    let far = 1000.0 - life(&h, "mid");
    assert!((far - 5.0 * hits).abs() < 0.01, "{far}");
    assert_eq!(life(&h, "small"), 1000.0);
}

#[test]
fn corrosive_breath_eats_structures_over_time_but_not_units() {
    let level = SpellLevel { dur: f(5.0), hero_dur: f(5.0), data: data(&[50.0]), ..Default::default() };
    let mut h = host(b"Acor", level, "setup_struct");
    hit(&mut h, 1);
    assert!(has_buff(&h, "prey", code(b"Bcor")));
    let l0 = life(&h, "prey");
    run(&mut h, 20);
    // 50 per second on the buff: two more seconds took at least 90 beyond the attack hits.
    assert!(l0 - life(&h, "prey") >= 90.0, "{} -> {}", l0, life(&h, "prey"));
}

#[test]
fn freezing_breath_stuns_the_structure_and_the_structures_in_its_area() {
    let level = SpellLevel { dur: f(5.0), hero_dur: f(5.0), area: f(100.0), ..Default::default() };
    let mut h = host(b"Afrz", level, "setup_struct");
    hit(&mut h, 1);
    for name in ["prey", "full"] {
        assert!(h.game().is_stunned(unit(&h, name)), "{name} stunned");
        assert!(has_buff(&h, name, code(b"Bfrz")));
    }
    assert!(!h.game().is_stunned(unit(&h, "outside")), "400 away");
    assert!(!h.game().is_stunned(unit(&h, "grounded")), "units are not structures");
    h.start("boom", &[]).expect("the attacker stops hitting");
    run(&mut h, 60);
    assert!(!h.game().is_stunned(unit(&h, "full")), "the stun ends after 5 s");
}

#[test]
fn liquid_fire_burns_a_structure_and_slows_its_attacks() {
    let level = SpellLevel { dur: f(3.0), hero_dur: f(3.0), data: data(&[8.0, 0.0, 0.8]), ..Default::default() };
    let mut h = host(b"Aliq", level, "setup_struct");
    hit(&mut h, 1);
    assert!(has_buff(&h, "prey", code(b"Bliq")));
    let prey = unit(&h, "prey");
    assert!((h.game().unit(prey).bonuses.attack_mult.to_f32() - 0.2).abs() < 1e-5, "attack speed -80 %");
    assert!(!has_buff(&h, "grounded", code(b"Bliq")));
    h.start("boom", &[]).expect("the attacker stops hitting");
    run(&mut h, 60);
    assert!(!has_buff(&h, "prey", code(b"Bliq")));
    assert!((h.game().unit(prey).bonuses.attack_mult.to_f32() - 1.0).abs() < 1e-5);
}

#[test]
fn frost_breath_slows_with_the_frost_attack_buff() {
    let level = SpellLevel { dur: f(10.0), hero_dur: f(10.0), ..Default::default() };
    let mut h = host(b"Afrb", level, "setup_ground");
    hit(&mut h, 1);
    assert!(has_buff(&h, "prey", code(b"Bfro")));
    assert!(h.game().unit(unit(&h, "prey")).bonuses.move_mult.to_f32() < 1.0);
}

#[test]
fn death_damage_explodes_after_the_delay_on_enemies_only() {
    // The stock row: Dur 0.3, DataA 100 / DataB 500 full, DataC 250 / DataD 200 partial.
    let level = SpellLevel { dur: f(0.3), hero_dur: f(0.3), data: data(&[100.0, 500.0, 250.0, 200.0]), ..Default::default() };
    let mut h = host(b"Adda", level, "setup_boom");
    h.start("boom", &[]).expect("boom");
    run(&mut h, 1);
    assert_eq!(life(&h, "prey"), 1000.0, "not yet: the explosion waits 0.3 s");
    run(&mut h, 6);
    assert_eq!(1000.0 - life(&h, "prey"), 500.0, "50 away: full ring");
    assert_eq!(1000.0 - life(&h, "full"), 200.0, "150 away: partial ring");
    assert_eq!(life(&h, "mid"), 1000.0, "an ally of the dead unit is spared");
    assert_eq!(life(&h, "outside"), 1000.0);
}
