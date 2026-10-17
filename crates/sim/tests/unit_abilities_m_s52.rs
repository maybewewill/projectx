//! Behaviour tests of the S52 additions to `unit_abilities_m.rs` (spec `docs/spec/abilities-unit-m.md`, section S52).
//! Level numbers are the editor rows of `AbilityData.slk` (1.26a patch); nothing here needs the game files.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::economy::Res;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData, ORDER_ATTACK};
use sim::unit_abilities_m::*;
use sim::ScriptHost;
use softfloat::F32;

const SRC: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native SetWidgetLife takes widget w, real l returns nothing
native SetUnitState takes unit u, integer s, real v returns nothing
native SetPlayerState takes player p, integer s, integer v returns nothing
globals
    unit repairer = null
    unit bld = null
    unit foe_bld = null
    unit well = null
    unit hurt = null
    unit taunter = null
    unit foe_near = null
    unit foe_far = null
    unit unsummoner = null
    unit own_bld = null
    unit aura = null
    unit mate = null
    unit raven = null
    unit gargoyle = null
    unit rider = null
    unit phoenix = null
    unit hall = null
    unit panda = null
    unit p_foe = null
    unit p_far = null
    unit p_ally = null
    unit worker = null
    boolean accepted = false
endglobals
function setup_worker takes nothing returns nothing
    set hall = CreateUnit(Player(0), 'nthl', 21000.0, 0.0, 0.0)
    set worker = CreateUnit(Player(0), 'nwrk', 21060.0, 0.0, 0.0)
endfunction
function setup_panda takes nothing returns nothing
    set panda = CreateUnit(Player(0), 'npnd', 24000.0, 0.0, 0.0)
    set p_foe = CreateUnit(Player(1), 'nfoe', 24100.0, 0.0, 0.0)
    set p_far = CreateUnit(Player(1), 'nfoe', 24400.0, 0.0, 0.0)
    set p_ally = CreateUnit(Player(0), 'nhrt', 24050.0, 0.0, 0.0)
endfunction
function return_it takes nothing returns nothing
    set accepted = IssueImmediateOrderById(worker, 852020)
endfunction
function setup takes nothing returns nothing
    call SetPlayerState(Player(0), 1, 1000)
    set repairer = CreateUnit(Player(0), 'nrpr', 0.0, 0.0, 0.0)
    set bld = CreateUnit(Player(0), 'nbld', 10.0, 0.0, 0.0)
    set foe_bld = CreateUnit(Player(1), 'nbld', 20.0, 0.0, 0.0)
    set well = CreateUnit(Player(0), 'nwel', 3000.0, 0.0, 0.0)
    set hurt = CreateUnit(Player(0), 'nhrt', 3050.0, 0.0, 0.0)
    set taunter = CreateUnit(Player(0), 'ntau', 6000.0, 0.0, 0.0)
    set foe_near = CreateUnit(Player(1), 'nfoe', 6100.0, 0.0, 0.0)
    set foe_far = CreateUnit(Player(1), 'nfoe', 7000.0, 0.0, 0.0)
    set unsummoner = CreateUnit(Player(0), 'nuns', 9000.0, 0.0, 0.0)
    set own_bld = CreateUnit(Player(0), 'nbl2', 9050.0, 0.0, 0.0)
    set aura = CreateUnit(Player(0), 'naur', 12000.0, 0.0, 0.0)
    set mate = CreateUnit(Player(0), 'nhrt', 12100.0, 0.0, 0.0)
    set raven = CreateUnit(Player(0), 'nrav', 15000.0, 0.0, 0.0)
    set gargoyle = CreateUnit(Player(0), 'ngar', 15100.0, 0.0, 0.0)
    set rider = CreateUnit(Player(0), 'nrid', 15200.0, 0.0, 0.0)
    set phoenix = CreateUnit(Player(0), 'nphx', 15300.0, 0.0, 0.0)
endfunction
function hurt_things takes nothing returns nothing
    call SetWidgetLife(bld, 500.0)
    call SetWidgetLife(foe_bld, 500.0)
    call SetWidgetLife(hurt, 500.0)
    call SetUnitState(hurt, 2, 0.0)
    call SetWidgetLife(mate, 500.0)
endfunction
function repair_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(repairer, 852024, bld)
endfunction
function repair_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(repairer, 852024, foe_bld)
endfunction
function replenish_it takes nothing returns nothing
    set accepted = IssueImmediateOrderById(well, 852542)
endfunction
function taunt_it takes nothing returns nothing
    set accepted = IssueImmediateOrderById(taunter, 852520)
endfunction
function unsummon_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(unsummoner, 852210, own_bld)
endfunction
function raven_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(raven, 852155)
endfunction
function raven_off takes nothing returns nothing
    set accepted = IssueImmediateOrderById(raven, 852156)
endfunction
function stone_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(gargoyle, 852206)
endfunction
function mount_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(rider, 852469)
endfunction
function mount_off takes nothing returns nothing
    set accepted = IssueImmediateOrderById(rider, 852470)
endfunction
function phoenix_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(phoenix, 852482)
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

fn ut(life: f32, abilities: Vec<u32>) -> UnitType {
    UnitType {
        name: "unit".to_string(),
        max_life: f(life),
        max_mana: f(300.0),
        start_mana: f(300.0),
        move_speed: f(270.0),
        abilities,
        ..Default::default()
    }
}

const RAVEN_TYPE: u32 = u32::from_be_bytes(*b"ntrv");
const STONE_TYPE: u32 = u32::from_be_bytes(*b"ngst");
const MOUNT_TYPE: u32 = u32::from_be_bytes(*b"nmnt");
const PHX_TYPE: u32 = u32::from_be_bytes(*b"negg");

fn host() -> ScriptHost {
    let prog = jass::parse(SRC).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let tables = std::env::var("GAME_DLL")
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| softfloat::Tables::from_game_dll(&b).ok());
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), tables).expect("vm");
    {
        let mut g = h.game_mut();
        let b = |cost: (i32, i32)| UnitType { gold_cost: cost.0, wood_cost: cost.1, gold_rep: cost.0, lumber_rep: cost.1, repair_time: 60, classification: 0x8, ..ut(1000.0, vec![]) };
        g.unit_types_mut().insert(c(b"nrpr"), ut(300.0, vec![REPAIR]));
        g.unit_types_mut().insert(c(b"nbld"), b((100, 50)));
        g.unit_types_mut().set_building(c(b"nbld"), true);
        g.production_mut().build_seconds.insert(c(b"nbld"), 60);
        g.unit_types_mut().insert(c(b"nwel"), UnitType { max_mana: f(600.0), start_mana: f(600.0), ..ut(300.0, vec![REPLENISH]) });
        g.unit_types_mut().insert(c(b"nhrt"), ut(1000.0, vec![]));
        g.unit_types_mut().insert(c(b"ntau"), ut(800.0, vec![TAUNT]));
        g.unit_types_mut().insert(c(b"nfoe"), ut(400.0, vec![]));
        g.set_type_orders(c(b"nfoe"), TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        g.unit_types_mut().insert(c(b"nuns"), ut(300.0, vec![UNSUMMON]));
        g.unit_types_mut().insert(c(b"nbl2"), b((200, 100)));
        g.unit_types_mut().set_building(c(b"nbl2"), true);
        g.unit_types_mut().insert(c(b"naur"), ut(300.0, vec![AURA_REGEN_LIFE]));
        g.unit_types_mut().insert(c(b"nrav"), ut(500.0, vec![RAVEN_FORM]));
        g.unit_types_mut().insert(RAVEN_TYPE, ut(600.0, vec![RAVEN_FORM]));
        g.unit_types_mut().insert(c(b"ngar"), ut(500.0, vec![STONE_FORM]));
        g.unit_types_mut().insert(STONE_TYPE, ut(900.0, vec![STONE_FORM]));
        g.unit_types_mut().insert(c(b"nrid"), ut(500.0, vec![MOUNT]));
        g.unit_types_mut().insert(MOUNT_TYPE, ut(700.0, vec![MOUNT]));
        g.unit_types_mut().insert(c(b"nphx"), ut(500.0, vec![PHOENIX_MORPH]));
        g.unit_types_mut().insert(PHX_TYPE, ut(100.0, vec![]));
        let lv = |cast: f32, dur: f32, cost: i32, rng: f32, area: f32, d: &[f32], unit: u32| SpellLevel {
            cast: f(cast),
            dur: f(dur),
            hero_dur: f(dur),
            cost,
            rng: f(rng),
            area: f(area),
            data: data(d),
            unit_id: unit,
            ..Default::default()
        };
        // AbilityData.slk rows (patch 1.26a).
        g.set_spell_levels(REPAIR, vec![lv(0.0, 0.0, 0, 50.0, 0.0, &[0.35, 1.5, 0.0, 0.0, 75.0], 0)]);
        g.set_spell_levels(REPLENISH, vec![lv(0.0, 1.0, 6, 250.0, 700.0, &[25.0, 25.0, 0.0, 0.0, 5.0], 0)]);
        g.set_spell_levels(TAUNT, vec![lv(0.0, 0.0, 0, 0.0, 450.0, &[10.0], 0)]);
        g.set_spell_levels(UNSUMMON, vec![lv(0.0, 0.0, 0, 0.0, 0.0, &[0.5, 50.0], 0)]);
        g.set_spell_levels(AURA_REGEN_LIFE, vec![lv(0.0, 0.0, 0, 0.0, 500.0, &[0.02, 1.0], 0)]);
        g.set_spell_levels(RAVEN_FORM, vec![lv(1.05, 0.6, 50, 0.0, 0.0, &[], RAVEN_TYPE)]);
        g.set_spell_levels(STONE_FORM, vec![lv(0.7, 0.5, 0, 0.0, 0.0, &[], STONE_TYPE)]);
        g.set_spell_levels(MOUNT, vec![lv(0.0, 0.5, 0, 0.0, 0.0, &[], MOUNT_TYPE)]);
        g.set_spell_levels(PHOENIX_MORPH, vec![lv(0.7, 0.5, 0, 0.0, 0.0, &[], PHX_TYPE)]);
        for id in [REPAIR, REPLENISH, TAUNT, UNSUMMON, AURA_REGEN_LIFE, RAVEN_FORM, STONE_FORM, MOUNT, PHOENIX_MORPH] {
            g.ability_types_mut().insert(id, AbilityType { max_level: Some(1) });
        }
    }
    h
}

fn started() -> ScriptHost {
    let mut h = host();
    h.start("setup", &[]).expect("setup");
    h.step();
    h.start("hurt_things", &[]).expect("hurt");
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
    assert!(h.misfires().is_empty());
}

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("call");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn mana(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).mana.to_f32()
}

fn type_of(h: &ScriptHost, k: ObjKey) -> u32 {
    h.game().unit(k).type_id
}

#[test]
fn the_s52_codes_are_modelled_in_the_table() {
    for code in ["Arep", "Aren", "Arst", "Artn", "Arpb", "Arpl", "Arpm", "Auns", "Atau", "Aoar", "Apig", "Arav", "Astn", "Amou", "Aphx"] {
        let row = CODES.iter().find(|r| r.0 == code.as_bytes()).expect("row");
        assert_eq!(row.3, Status::Modelled, "{code}");
    }
}

#[test]
fn repair_restores_life_at_the_build_time_rate_and_pays_a_share_of_the_cost() {
    if std::env::var("GAME_DLL").is_err() {
        return; // the regeneration modifier needs the soft-float tables
    }
    let mut h = started();
    let (bld, foe_bld) = (unit(&h, "bld"), unit(&h, "foe_bld"));
    call(&mut h, "repair_foe");
    run(&mut h, 3);
    assert!(!accepted(&h), "an enemy building cannot be repaired");
    assert_eq!(life(&h, foe_bld), 500.0);
    h.game_mut().credit_resource(0, Res::Lumber, 1000, sim::economy::Flags::NONE);
    let gold0 = h.game().player_economy(0).unwrap().stored(Res::Gold);
    call(&mut h, "repair_it");
    run(&mut h, 3);
    assert!(accepted(&h));
    run(&mut h, 100); // 10 s
    // 1000 life / (60 s * 1.5) = 11.11 life per second.
    let l = life(&h, bld);
    assert!(l > 600.0 && l < 615.0, "life {l}");
    // V10 (CONFIRMED against `0x6F070AB0`): the repair takes 1.5 * reptm = 90 s and costs 0.35 * goldRep = 35 gold over it, paid as
    // single units: one at the start, then one every 90 / 35 = 2.57 s (4 or 5 units after 10 s).
    let paid = gold0 - h.game().player_economy(0).unwrap().stored(Res::Gold);
    assert!(paid == 40 || paid == 50, "paid {paid}");
    // Lumber: 0.35 * 50 = 17.5 over 90 s, one unit every 5.14 s (3 units after 10.3 s).
    let wood = 10_000 - h.game().player_economy(0).unwrap().stored(Res::Lumber);
    assert!(wood == 20 || wood == 30, "wood {wood}");
}

#[test]
fn repair_stops_when_the_owner_cannot_pay() {
    let mut h = started();
    let bld = unit(&h, "bld");
    // No lumber: the target gate answers 9 (V10, CONFIRMED against `0x6F04BF70`) and the order is refused.
    call(&mut h, "repair_it");
    run(&mut h, 3);
    assert!(!accepted(&h));
    run(&mut h, 200);
    assert!(h.game().m_repairing(bld).is_none());
    assert!(life(&h, bld) < 520.0, "life {}", life(&h, bld));
}

#[test]
fn repair_runs_to_the_end_when_it_can_pay_and_stops_whole() {
    if std::env::var("GAME_DLL").is_err() {
        return;
    }
    let mut h = started();
    let bld = unit(&h, "bld");
    h.game_mut().credit_resource(0, Res::Lumber, 1000, sim::economy::Flags::NONE);
    call(&mut h, "repair_it");
    run(&mut h, 3);
    assert!(accepted(&h));
    run(&mut h, 480); // 48 s: 500 life at 11.11 per second
    // The gate answers 30 once less than 1.0 life is missing, so the repair ends there (V10).
    assert!(life(&h, bld) > 999.0, "life {}", life(&h, bld));
    assert!(h.game().m_repairing(bld).is_none());
    // Half the building was missing: about half of 0.35 * 100 gold and of 0.35 * 50 lumber, paid in single units on a timer.
    let g = h.game();
    let e = g.player_economy(0).unwrap();
    let gold = (10_000 - e.stored(Res::Gold)) as f32 / 10.0;
    let wood = (10_000 - e.stored(Res::Lumber)) as f32 / 10.0;
    assert!((gold - 17.5).abs() < 2.0, "gold {gold}");
    assert!((wood - 8.75).abs() < 2.0, "lumber {wood}");
}

#[test]
fn replenish_pulses_once_into_every_unit_that_needs_it() {
    let mut h = started();
    let (well, hurt) = (unit(&h, "well"), unit(&h, "hurt"));
    call(&mut h, "replenish_it");
    run(&mut h, 3);
    assert!(accepted(&h));
    // One pulse (V10, CONFIRMED against `0x6F1FADC0`): +25 life and +25 mana for the one unit that needs them, the caster pays
    // `max(1, min(units, F2I(DataE))) * Cost` = 6 mana; there is no channel.
    assert_eq!(life(&h, hurt), 525.0);
    assert_eq!(mana(&h, hurt), 25.0);
    assert_eq!(mana(&h, well), 594.0);
    run(&mut h, 80);
    assert_eq!(life(&h, hurt), 525.0);
    assert_eq!(mana(&h, hurt), 25.0);
}

#[test]
fn taunt_orders_the_enemies_in_the_area_to_attack_the_caster() {
    let mut h = started();
    let (near, far) = (unit(&h, "foe_near"), unit(&h, "foe_far"));
    call(&mut h, "taunt_it");
    run(&mut h, 3);
    assert!(accepted(&h));
    assert_eq!(h.game().unit_order_ids(near).first().copied(), Some(ORDER_ATTACK));
    assert!(h.game().unit_order_ids(far).is_empty());
}

#[test]
fn unsummon_removes_an_own_building_and_refunds_half_its_cost() {
    let mut h = started();
    let own = unit(&h, "own_bld");
    h.game_mut().player_economy_mut(0).unwrap().stock = [0, 0];
    call(&mut h, "unsummon_it");
    run(&mut h, 3);
    assert!(accepted(&h));
    assert!(h.game().unit(own).removed);
    let g = h.game();
    let e = g.player_economy(0).unwrap();
    assert_eq!(e.stored(Res::Gold), 1000, "100 gold x10");
    assert_eq!(e.stored(Res::Lumber), 500, "50 lumber x10");
}

#[test]
fn the_life_regeneration_aura_reaches_friends_only() {
    let mut h = started();
    let (aura, mate) = (unit(&h, "aura"), unit(&h, "mate"));
    run(&mut h, 12);
    let has = |k: ObjKey| h.game().buffs(k).iter().any(|b| b.base == BUFF_REGEN_LIFE);
    assert!(has(mate));
    assert!(!has(aura), "the row's targs name friend, not self");
}

#[test]
fn raven_stone_mount_and_phoenix_transform_after_their_duration() {
    let mut h = started();
    let (raven, garg, rider, phx) = (unit(&h, "raven"), unit(&h, "gargoyle"), unit(&h, "rider"), unit(&h, "phoenix"));
    call(&mut h, "raven_on");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(type_of(&h, raven), c(b"nrav"), "still the old form during the transformation time");
    for name in ["stone_on", "mount_on", "phoenix_on"] {
        call(&mut h, name);
        run(&mut h, 2);
        assert!(accepted(&h), "{name}");
    }
    run(&mut h, 25);
    assert_eq!(type_of(&h, raven), RAVEN_TYPE);
    assert_eq!(type_of(&h, garg), STONE_TYPE);
    assert_eq!(type_of(&h, rider), MOUNT_TYPE);
    assert_eq!(type_of(&h, phx), PHX_TYPE);
    // The alternate orders return the raven and the rider.
    call(&mut h, "raven_off");
    run(&mut h, 2);
    assert!(accepted(&h));
    call(&mut h, "mount_off");
    run(&mut h, 2);
    assert!(accepted(&h));
    run(&mut h, 25);
    assert_eq!(type_of(&h, raven), c(b"nrav"));
    assert_eq!(type_of(&h, rider), c(b"nrid"));
}

#[test]
fn return_resources_brings_the_hits_made_so_far_to_the_hall() {
    use sim::production::{HarvestSpec, ProdEvent, Tree, TypeTech};
    let mut h = host();
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(c(b"nthl"), UnitType { classification: 0x40, ..ut(1000.0, vec![]) });
        g.unit_types_mut().set_building(c(b"nthl"), true);
        g.unit_types_mut().insert(c(b"nwrk"), ut(200.0, vec![RETURN]));
        let spec = HarvestSpec { wood_cap: 50, per_hit: 10, hit_seconds: 1.0, ..Default::default() };
        g.production_mut().tech.insert(c(b"nthl"), TypeTech { lumber_dropoff: true, ..Default::default() });
        g.production_mut()
            .tech
            .insert(c(b"nwrk"), TypeTech { harvest_wood: true, harvest: spec, ..Default::default() });
        g.production_mut().trees.push(Tree { x: 21150.0, y: 0.0, wood: 100 });
        g.ability_types_mut().insert(RETURN, AbilityType { max_level: Some(1) });
    }
    h.start("setup_worker", &[]).expect("setup");
    h.step();
    let worker = unit(&h, "worker");
    h.game_mut().assign_harvest(worker, Res::Lumber, (21000.0, 0.0), None);
    // The full trip is 5 hits (5 s). Return after about 3 s of chopping.
    run(&mut h, 40);
    call(&mut h, "return_it");
    run(&mut h, 3);
    assert!(accepted(&h));
    run(&mut h, 60);
    let g = h.game();
    let first = g
        .production()
        .events
        .iter()
        .find_map(|e| match e {
            ProdEvent::Deposit { amount, .. } => Some(*amount),
            _ => None,
        })
        .expect("a deposit");
    assert!((10..50).contains(&first), "a partial load of whole hits, got {first}");
}

#[test]
fn panda_immolation_burns_enemies_in_the_area_every_second() {
    let mut h = host();
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(c(b"npnd"), ut(800.0, vec![PANDA_IMMOLATION]));
        let l = SpellLevel { dur: f(1.0), hero_dur: f(1.0), area: f(220.0), data: data(&[10.0]), ..Default::default() };
        g.set_spell_levels(PANDA_IMMOLATION, vec![l]);
        g.ability_types_mut().insert(PANDA_IMMOLATION, AbilityType { max_level: Some(1) });
    }
    h.start("setup_panda", &[]).expect("setup");
    h.step();
    let (foe, far, ally) = (unit(&h, "p_foe"), unit(&h, "p_far"), unit(&h, "p_ally"));
    let before = life(&h, foe);
    run(&mut h, 35); // 3.5 s: three pulses
    assert_eq!(before - life(&h, foe), 30.0);
    assert_eq!(life(&h, far), 400.0, "outside the area");
    assert_eq!(life(&h, ally), 1000.0, "allies are not burned");
}
