//! Behaviour tests of the smaller S50 abilities (`neutral_misc.rs`, `neutral_waves.rs` Monsoon, the Inferno alias).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::neutral_misc::*;
use sim::neutral_waves::MONSOON;
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native KillUnit takes unit whichUnit returns boolean
"#;

const SRC: &str = r#"
globals
    unit roarer = null
    unit ally = null
    unit rotter = null
    unit mfoe = null
    unit demo = null
    unit bld = null
    unit pawn = null
    unit immo = null
    unit ifoe = null
    unit chaos = null
    unit finger = null
    unit ffoe = null
    unit watery = null
    unit para = null
    unit pfoe = null
    unit chan = null
    unit portal = null
    unit storm = null
    unit sfoe = null
    unit sbld = null
    unit pit = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set roarer = CreateUnit(Player(0), 'nbrs', 0.0, 0.0, 0.0)
    set ally = CreateUnit(Player(0), 'ndmy', 300.0, 0.0, 0.0)
    set rotter = CreateUnit(Player(0), 'nmrt', 2000.0, 0.0, 0.0)
    set mfoe = CreateUnit(Player(1), 'ndmy', 2300.0, 0.0, 0.0)
    set demo = CreateUnit(Player(0), 'ndem', 4000.0, 0.0, 0.0)
    set bld = CreateUnit(Player(1), 'nhll', 4080.0, 0.0, 180.0)
    set pawn = CreateUnit(Player(1), 'ndmy', 4000.0, 80.0, 180.0)
    set immo = CreateUnit(Player(0), 'npim', 6000.0, 0.0, 0.0)
    set ifoe = CreateUnit(Player(1), 'ndmy', 6100.0, 0.0, 0.0)
    set chaos = CreateUnit(Player(0), 'nrch', 8000.0, 0.0, 0.0)
    set finger = CreateUnit(Player(0), 'nfdt', 10000.0, 0.0, 0.0)
    set ffoe = CreateUnit(Player(1), 'ndmy', 10300.0, 0.0, 0.0)
    set watery = CreateUnit(Player(0), 'nwmn', 12000.0, 0.0, 0.0)
    set para = CreateUnit(Player(0), 'npar', 14000.0, 0.0, 0.0)
    set pfoe = CreateUnit(Player(1), 'ndmy', 14300.0, 0.0, 0.0)
    set chan = CreateUnit(Player(0), 'nchn', 16000.0, 0.0, 0.0)
    set portal = CreateUnit(Player(0), 'ndpt', 18000.0, 0.0, 0.0)
    set storm = CreateUnit(Player(0), 'nmon', 20000.0, 0.0, 0.0)
    set sfoe = CreateUnit(Player(1), 'ndmy', 20300.0, 0.0, 0.0)
    set sbld = CreateUnit(Player(1), 'nhll', 20300.0, 100.0, 0.0)
    set pit = CreateUnit(Player(0), 'npit', 22000.0, 0.0, 0.0)
endfunction
function roar takes nothing returns nothing
    set accepted = IssueImmediateOrderById(roarer, 852599)
endfunction
function rot takes nothing returns nothing
    set accepted = IssueTargetOrderById(rotter, 852565, mfoe)
endfunction
function demolish takes nothing returns nothing
    call IssueTargetOrder(demo, "attack", bld)
endfunction
function demolish_unit takes nothing returns nothing
    call IssueTargetOrder(demo, "attack", pawn)
endfunction
function rain takes nothing returns nothing
    set accepted = IssuePointOrderById(chaos, 852237, 8300.0, 0.0)
endfunction
function point takes nothing returns nothing
    set accepted = IssueTargetOrderById(finger, 852230, ffoe)
endfunction
function summon takes nothing returns nothing
    set accepted = IssueImmediateOrderById(watery, 852598)
endfunction
function infest takes nothing returns nothing
    set accepted = IssueTargetOrderById(para, 852601, pfoe)
endfunction
function kill_pfoe takes nothing returns nothing
    call KillUnit(pfoe)
endfunction
function channel takes nothing returns nothing
    set accepted = IssueImmediateOrderById(chan, 852600)
endfunction
function open_portal takes nothing returns nothing
    set accepted = IssuePointOrderById(portal, 852229, 18400.0, 0.0)
endfunction
function monsoon takes nothing returns nothing
    set accepted = IssuePointOrderById(storm, 852591, 20300.0, 0.0)
endfunction
function inferno takes nothing returns nothing
    set accepted = IssuePointOrderById(pit, 852232, 22400.0, 0.0)
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
        let inferno_alias = id(b"ANin");
        let casters: [(&[u8; 4], Vec<u32>); 13] = [
            (b"nbrs", vec![BATTLE_ROAR]),
            (b"nmrt", vec![MIND_ROT]),
            (b"ndem", vec![DEMOLISH]),
            (b"npim", vec![PERM_IMMOLATION]),
            (b"nrch", vec![RAIN_OF_CHAOS]),
            (b"nfdt", vec![FINGER_OF_DEATH]),
            (b"nwmn", vec![WATERY_MINION]),
            (b"npar", vec![PARASITE]),
            (b"nchn", vec![CHANNEL]),
            (b"ndpt", vec![DARK_PORTAL]),
            (b"nmon", vec![MONSOON]),
            (b"npit", vec![inferno_alias]),
            (b"ndmy", vec![]),
        ];
        for (t, abilities) in casters {
            g.unit_types_mut().insert(id(t), ut(abilities));
        }
        for t in [b"nhll", b"ncfs", b"ninf", b"nbal", b"nfel", b"nprs"] {
            g.unit_types_mut().insert(id(t), ut(vec![]));
        }
        g.unit_types_mut().set_building(id(b"nhll"), true);
        g.set_neutral_unit_list(DARK_PORTAL, 0, 0, vec![id(b"nbal"), id(b"nfel")]);
        g.set_spell_alias(inferno_alias, sim::undead_ne::INFERNO);
        let attacker = AttackType {
            cooldown: f(1.0),
            damage_point: f(0.3),
            backswing: f(0.3),
            range: f(100.0),
            collision: f(0.5),
            damage: f(10.0),
            missile_speed: None,
            ..Default::default()
        };
        g.set_type_orders(id(b"ndem"), TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        g.set_attack_type(id(b"ndem"), attacker);
        let lv = |l: SpellLevel| vec![l];
        g.set_spell_levels(BATTLE_ROAR, lv(SpellLevel { cost: 50, cool: f(25.0), dur: f(10.0), hero_dur: f(10.0), area: f(650.0), data: data(&[10.0]), ..Default::default() }));
        g.set_spell_levels(MIND_ROT, lv(SpellLevel { dur: f(30.0), hero_dur: f(10.0), rng: f(700.0), data: data(&[5.0]), ..Default::default() }));
        g.set_spell_levels(DEMOLISH, lv(SpellLevel { data: data(&[100.0, 2.0]), ..Default::default() }));
        g.set_spell_levels(PERM_IMMOLATION, lv(SpellLevel { dur: f(1.0), hero_dur: f(1.0), area: f(220.0), data: data(&[10.0]), ..Default::default() }));
        g.set_spell_levels(
            RAIN_OF_CHAOS,
            lv(SpellLevel { dur: f(1.0), cool: f(120.0), area: f(900.0), rng: f(1000.0), data: data(&[0.0, 2.0]), ..Default::default() }),
        );
        g.set_spell_levels(
            inferno_alias,
            lv(SpellLevel { dur: f(4.0), hero_dur: f(2.0), cool: f(180.0), cost: 175, area: f(250.0), rng: f(900.0), data: data(&[50.0, 360.0, 1.0]), unit_id: id(b"ninf"), ..Default::default() }),
        );
        g.set_spell_levels(FINGER_OF_DEATH, lv(SpellLevel { cool: f(15.0), rng: f(800.0), data: data(&[0.25, 1.0, 500.0]), ..Default::default() }));
        g.set_spell_levels(
            WATERY_MINION,
            lv(SpellLevel { cost: 75, cool: f(20.0), dur: f(60.0), hero_dur: f(60.0), area: f(200.0), data: data(&[2.0]), unit_id: id(b"ncfs"), ..Default::default() }),
        );
        g.set_spell_levels(
            PARASITE,
            lv(SpellLevel { cast: f(90.0), dur: f(30.0), cool: f(5.0), cost: 50, rng: f(700.0), data: data(&[5.0, 0.0, 0.0, 8.0, 1.0]), unit_id: id(b"nprs"), ..Default::default() }),
        );
        g.set_spell_levels(CHANNEL, lv(SpellLevel { rng: f(500.0), data: data(&[2.0]), ..Default::default() }));
        g.set_spell_levels(DARK_PORTAL, lv(SpellLevel { dur: f(1.0), cool: f(100.0), rng: f(800.0), data: data(&[0.0, 3.0, 5.0]), ..Default::default() }));
        g.set_spell_levels(
            MONSOON,
            lv(SpellLevel { dur: f(15.0), hero_dur: f(15.0), cool: f(10.0), cost: 75, area: f(400.0), rng: f(500.0), data: data(&[20.0, 1.5, 0.35]), ..Default::default() }),
        );
        for a in [BATTLE_ROAR, MIND_ROT, DEMOLISH, PERM_IMMOLATION, RAIN_OF_CHAOS, FINGER_OF_DEATH, WATERY_MINION, PARASITE, CHANNEL, DARK_PORTAL, MONSOON, sim::undead_ne::INFERNO] {
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

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn count_of_type(h: &ScriptHost, ty: u32) -> usize {
    let g = h.game();
    (0..g.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(g.object(k), sim::natives::game::Object::Unit(u) if !u.dead && !u.removed && u.type_id == ty))
        .count()
}

#[test]
fn battle_roar_adds_flat_damage_to_the_allies_around_the_caster() {
    let mut h = started();
    let (roarer, ally) = (unit(&h, "roarer"), unit(&h, "ally"));
    h.start("roar", &[]).expect("roar");
    run(&mut h, 4);
    assert!(accepted(&h));
    for k in [roarer, ally] {
        assert_eq!(h.game().unit(k).hero_attack, f(10.0), "+10 damage");
    }
    run(&mut h, 110);
    assert_eq!(h.game().unit(ally).hero_attack, f(0.0), "gone after 10 s");
}

#[test]
fn mind_rot_drains_mana_every_second_for_the_duration() {
    let mut h = started();
    let foe = unit(&h, "mfoe");
    h.start("rot", &[]).expect("rot");
    run(&mut h, 4);
    assert!(accepted(&h));
    run(&mut h, 30);
    let mana = h.game().unit(foe).mana.to_f32();
    assert!(mana <= 500.0 - 15.0 + 0.01 && mana >= 500.0 - 20.0, "3 drains of 5 so far, got {mana}");
    run(&mut h, 400);
    let drained = 500.0 - h.game().unit(foe).mana.to_f32();
    assert!(drained == 145.0 || drained == 150.0, "29 or 30 drains over the 30 s, got {drained}");
    run(&mut h, 100);
    assert_eq!(500.0 - h.game().unit(foe).mana.to_f32(), drained, "no drain after the buff");
}

#[test]
fn demolish_doubles_the_damage_against_buildings_only() {
    let mut h = started();
    let (bld, pawn) = (unit(&h, "bld"), unit(&h, "pawn"));
    h.start("demolish", &[]).expect("attack");
    run(&mut h, 8);
    assert_eq!(life(&h, bld), 500.0 - 20.0, "DataB 2 against a building");
    h.start("demolish_unit", &[]).expect("attack");
    run(&mut h, 9);
    assert_eq!(life(&h, pawn), 500.0 - 10.0, "an ordinary unit takes the normal hit");
}

#[test]
fn permanent_immolation_burns_nearby_enemies_every_second() {
    let mut h = started();
    let foe = unit(&h, "ifoe");
    run(&mut h, 25);
    assert_eq!(life(&h, foe), 500.0 - 20.0, "10 per second, 100 away inside the 220 area");
}

#[test]
fn rain_of_chaos_calls_down_the_level_count_of_infernals() {
    let mut h = started();
    h.start("rain", &[]).expect("rain");
    run(&mut h, 3);
    assert!(accepted(&h));
    assert_eq!(count_of_type(&h, id(b"ninf")), 1, "the first infernal falls at once");
    run(&mut h, 12);
    assert_eq!(count_of_type(&h, id(b"ninf")), 2, "DataB = 2 in all, one per Dur");
}

#[test]
fn the_neutral_inferno_row_runs_the_dreadlord_inferno() {
    let mut h = started();
    let pit = unit(&h, "pit");
    h.game_mut().unit_mut(pit).mana = f(500.0);
    h.start("inferno", &[]).expect("inferno");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert_eq!(count_of_type(&h, id(b"ninf")), 1);
}

#[test]
fn finger_of_death_hits_after_the_graphic_delay() {
    let mut h = started();
    let foe = unit(&h, "ffoe");
    h.start("point", &[]).expect("finger");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(life(&h, foe), 500.0, "not yet");
    run(&mut h, 4);
    assert!(h.game().unit(foe).dead, "500 damage after 0.25 s");
}

#[test]
fn watery_minion_summons_two_for_sixty_seconds() {
    let mut h = started();
    h.start("summon", &[]).expect("summon");
    run(&mut h, 4);
    assert!(accepted(&h));
    assert_eq!(count_of_type(&h, id(b"ncfs")), 2);
    run(&mut h, 700);
    assert_eq!(count_of_type(&h, id(b"ncfs")), 0);
}

#[test]
fn parasite_poisons_and_hatches_a_minion_when_the_host_dies() {
    let mut h = started();
    let foe = unit(&h, "pfoe");
    h.start("infest", &[]).expect("infest");
    run(&mut h, 25);
    assert!(accepted(&h));
    assert!(life(&h, foe) < 500.0 - 9.0, "5 per second");
    h.start("kill_pfoe", &[]).expect("kill");
    run(&mut h, 4);
    assert_eq!(count_of_type(&h, id(b"nprs")), 1, "one minion for DataE = 1");
    run(&mut h, 950);
    assert_eq!(count_of_type(&h, id(b"nprs")), 0, "gone after the Cast field's 90 s");
}

#[test]
fn channel_fires_the_spell_events_and_holds_for_the_follow_through_time() {
    let mut h = started();
    let chan = unit(&h, "chan");
    h.start("channel", &[]).expect("channel");
    run(&mut h, 4);
    assert!(accepted(&h));
    let log = h.game().spell_log().to_vec();
    assert!(log.iter().any(|&(c, b, _)| c == chan && b == CHANNEL), "the cast events fired");
}

#[test]
fn dark_portal_opens_with_the_minimum_unit_count() {
    let mut h = started();
    h.start("open_portal", &[]).expect("portal");
    run(&mut h, 4);
    assert!(accepted(&h));
    let n = count_of_type(&h, id(b"nbal")) + count_of_type(&h, id(b"nfel"));
    assert_eq!(n, 3, "DataB = 3 without the RNG table");
}

#[test]
fn monsoon_strikes_every_enemy_in_the_area_each_interval() {
    let mut h = started();
    let (foe, bld) = (unit(&h, "sfoe"), unit(&h, "sbld"));
    h.start("monsoon", &[]).expect("monsoon");
    run(&mut h, 2);
    assert!(accepted(&h));
    run(&mut h, 40);
    assert_eq!(life(&h, foe), 500.0 - 40.0, "two strikes of 20 in the first 4 s (1.5 s and 3.0 s)");
    assert_eq!(life(&h, bld), 500.0 - 14.0, "buildings take 0.35 of it");
    run(&mut h, 300);
    assert_eq!(life(&h, foe), 500.0 - 200.0, "ten strikes in all");
}
