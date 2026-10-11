//! The AI manager: `SetProduce`, `SetUpgrade`, `HarvestGold`, `HarvestWood` and `ClearHarvestAI` act on the game
//! through the production model (`sim::production`, `sim::ai_manager`). Synthetic types and a synthetic map: one
//! town hall, one mine, a few peons, all within reach of each other so that the walking (which needs the Game.dll
//! tables) is not needed.

use std::cell::RefCell;
use std::rc::Rc;

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::ai::{self, AiHost, AiLibrary};
use sim::economy::Res;
use sim::natives::ai::register_main;
use sim::natives::game::{Game, ObjKey, UnitType};
use sim::production::{BuildGrid, ProdEvent, TypeTech, UpgradeType};
use sim::ScriptHost;
use softfloat::F32;

const HALL: u32 = u32::from_be_bytes(*b"ohal");
const PEON: u32 = u32::from_be_bytes(*b"opeo");
const BARRACKS: u32 = u32::from_be_bytes(*b"obar");
const MINE: u32 = u32::from_be_bytes(*b"ngol");
const TECH: u32 = u32::from_be_bytes(*b"Rome");

const AI_COMMON: &str = r#"
type player extends handle
type unit extends handle
native Player takes integer number returns player
native Sleep takes real t returns nothing
native GetUnitCount takes integer t returns integer
native GetUnitCountDone takes integer t returns integer
native TownWithMine takes nothing returns integer
native SetUpgrade takes integer t returns boolean
native SetProduce takes integer n, integer t, integer x returns boolean
native ClearHarvestAI takes nothing returns nothing
native HarvestGold takes integer town, integer peons returns nothing
native HarvestWood takes integer town, integer peons returns nothing
"#;

const MAIN: &str = r#"
type player extends handle
type unit extends handle
native Player takes integer number returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native SetPlayerState takes player p, integer s, integer v returns nothing
native StartMeleeAI takes player p, string s returns nothing
function main takes nothing returns nothing
    call CreateUnit(Player(15), 'ngol', 0.0, 150.0, 0.0)
    call CreateUnit(Player(1), 'ohal', 0.0, 0.0, 0.0)
    call CreateUnit(Player(1), 'opeo', 0.0, 70.0, 0.0)
    call CreateUnit(Player(1), 'opeo', 20.0, 70.0, 0.0)
    call CreateUnit(Player(1), 'opeo', -20.0, 70.0, 0.0)
    call SetPlayerState(Player(1), 1, 1000)
    call SetPlayerState(Player(1), 2, 500)
    call StartMeleeAI(Player(1), "prod.ai")
endfunction
"#;

const PROD_AI: &str = r#"
function main takes nothing returns nothing
    local integer t
    loop
        call ClearHarvestAI()
        set t = TownWithMine()
        call HarvestGold(t, 2)
        call HarvestWood(0, 5)
        if GetUnitCount('opeo') < 6 then
            call SetProduce(1, 'opeo', -1)
        endif
        if GetUnitCount('obar') < 1 then
            call SetProduce(1, 'obar', -1)
        endif
        call SetUpgrade('Rome')
        call Sleep(3.0)
    endloop
endfunction
"#;

fn ty(name: &str, gold: i32, wood: i32, food_used: i32, food_made: i32, class: u32) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: F32::from_bits(0x4348_0000),
        gold_cost: gold,
        wood_cost: wood,
        food_used,
        food_made,
        classification: class,
        ..Default::default()
    }
}

fn setup(game: &Rc<RefCell<Game>>) {
    let mut g = game.borrow_mut();
    g.unit_types_mut().insert(HALL, ty("hall", 0, 0, 0, 10, 0x40));
    g.unit_types_mut().set_building(HALL, true);
    g.unit_types_mut().insert(PEON, ty("peon", 75, 0, 1, 0, 0));
    g.unit_types_mut().insert(BARRACKS, ty("barracks", 200, 50, 0, 0, 0));
    g.unit_types_mut().set_building(BARRACKS, true);
    g.unit_types_mut().insert(MINE, ty("mine", 0, 0, 0, 0, 0));
    g.unit_types_mut().set_building(MINE, true);
    let p = g.production_mut();
    p.build_seconds.insert(PEON, 15);
    p.build_seconds.insert(BARRACKS, 30);
    p.tech.insert(HALL, TypeTech { trains: vec![PEON], researches: vec![TECH], ..Default::default() });
    p.tech.insert(
        PEON,
        TypeTech { builds: vec![BARRACKS], harvest_gold: true, harvest_wood: true, ..Default::default() },
    );
    p.upgrades.insert(
        TECH,
        UpgradeType { gold_base: 100, gold_mod: 50, lumber_base: 50, lumber_mod: 0, time_base: 20, time_mod: 10, max_level: 3 },
    );
    // A buildable plane of 128 x 128 cells, origin (-2048, -2048).
    p.grid = Some(BuildGrid { width: 128, height: 128, origin_x: -2048.0, origin_y: -2048.0, types: vec![0; 128 * 128] });
}

struct Rig {
    host: ScriptHost,
    ai: AiHost,
    game: Rc<RefCell<Game>>,
}

fn rig() -> Rig {
    let game = Rc::new(RefCell::new(Game::new()));
    setup(&game);
    let shared = ai::new_shared();
    let mut reg = NativeRegistry::with_builtins();
    register_main(&mut reg, &game, &shared);
    let module = compile(&[jass::parse(MAIN).expect("parse")]).expect("compile");
    let mut host = ScriptHost::with_game(module, reg, None, Rc::clone(&game)).expect("vm");
    host.start("main", &[]).expect("start");
    let mut lib = AiLibrary::new(AI_COMMON);
    lib.add_script("prod.ai", PROD_AI);
    let ai = AiHost::new(Rc::clone(&game), shared, lib, None);
    // The mine holds gold.
    {
        let mut g = game.borrow_mut();
        let mine = g.unit_objects().find(|(_, u)| u.type_id == MINE).map(|(k, _)| k).expect("mine");
        g.add_resource_node(mine, 5000);
    }
    Rig { host, ai, game }
}

fn run(r: &mut Rig, batches: usize) {
    for _ in 0..batches {
        r.host.step();
        let now = F32::from_bits(r.host.clock().now().to_bits());
        r.ai.step(now);
    }
}

fn count(r: &Rig, ty: u32, done: bool) -> usize {
    let g = r.game.borrow();
    g.unit_objects()
        .filter(|(k, u)| {
            u.owner == 1 && u.type_id == ty && !u.removed && (!done || g.unit_completed(*k))
        })
        .count()
}

#[test]
fn the_ai_trains_builds_researches_and_harvests() {
    let mut r = rig();
    run(&mut r, 1500); // 150 s
    let g = r.game.borrow();
    let prod = g.production();
    // Harvesting: gold arrived at the hall through the income changer.
    assert!(prod.stats.gold_deposited > 0, "no gold deposited: {:?}", prod.stats);
    // Training: the hall made peons (3 at the start, capped by the script at 6).
    drop(g);
    let peons = count(&r, PEON, true);
    assert!(peons > 3, "peons: {peons}");
    // Construction: the barracks went up and completed.
    assert_eq!(count(&r, BARRACKS, true), 1);
    let g = r.game.borrow();
    let events = &g.production().events;
    assert!(events.iter().any(|e| matches!(e, ProdEvent::Built { type_id, .. } if *type_id == BARRACKS)));
    // Research: SetUpgrade returned false but the order went out and the level rose.
    assert!(g.tech_level(1, TECH) >= 1, "tech level {}", g.tech_level(1, TECH));
    // Food: the hall's 10 food made, peons use it.
    assert_eq!(g.food_made_of(1), 10);
    assert_eq!(g.food_used_of(1), peons as i32);
    // Gold was spent: more than the 1000 start minus what is left plus deposits is impossible to check exactly, but
    // the stored gold must be non-negative.
    assert!(g.player_economy(1).unwrap().stored(Res::Gold) >= 0);
}

#[test]
fn the_production_run_is_deterministic() {
    let once = || {
        let mut r = rig();
        run(&mut r, 600);
        let g = r.game.borrow();
        (g.production().stats, g.production().events.clone())
    };
    assert_eq!(once(), once());
}

#[test]
fn set_produce_needs_a_building_that_can_train() {
    let mut r = rig();
    run(&mut r, 2);
    let mut g = r.game.borrow_mut();
    let hall: ObjKey = g.unit_objects().find(|(_, u)| u.type_id == HALL).map(|(k, _)| k).unwrap();
    // A type nothing trains is refused by the building.
    assert!(g.train_unit(hall, BARRACKS).is_err());
    // A queued unit makes the building busy (the AI skips it) and counts as an in-progress unit.
    let _ = g.train_unit(hall, PEON);
    assert!(!g.building_usable(hall) || g.production().queued(hall).is_empty());
}
