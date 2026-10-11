//! Unit and item pools (`natives/pool.rs`): the entry rules, the handle rules, and the weighted pick against the
//! generator's own draws.
//!
//! The pick oracle uses `Streams::global` through `random_real_soft(table, 0, 1)`, which is `U` itself (the range is
//! 1, so `0 + 1 * U`), so the expected pick is computed without the pool code. The table is the stand-in permutation
//! unless `GAME_DLL` is set, which makes the test run on the user's table (the checks are the same).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ItemType, UnitType};
use sim::{RngTable, ScriptHost, Streams};
use softfloat::{arith, Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type item extends widget
type location extends handle
type trigger extends handle
type unitpool extends handle
type itempool extends handle
type itemtype extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateUnitPool takes nothing returns unitpool
native DestroyUnitPool takes unitpool p returns nothing
native UnitPoolAddUnitType takes unitpool p, integer unitId, real weight returns nothing
native UnitPoolRemoveUnitType takes unitpool p, integer unitId returns nothing
native PlaceRandomUnit takes unitpool p, player owner, real x, real y, real face returns unit
native CreateItemPool takes nothing returns itempool
native DestroyItemPool takes itempool p returns nothing
native ItemPoolAddItemType takes itempool p, integer itemId, real weight returns nothing
native ItemPoolRemoveItemType takes itempool p, integer itemId returns nothing
native PlaceRandomItem takes itempool p, real x, real y returns item
native GetItemTypeId takes item i returns integer
native GetUnitTypeId takes unit u returns integer
native GetUnitX takes unit u returns real
native SetRandomSeed takes integer seed returns nothing
native GetRandomInt takes integer lo, integer hi returns integer
"#;

const FOOTMAN: u32 = 0x6866_6F6F;
const RANGER: u32 = 0x6872_6E67;
const ITEM_A: u32 = 0x4974_6D41; // 'ItmA'
const ITEM_B: u32 = 0x4974_6D42; // 'ItmB'

/// The stand-in table (identity permutation) or the user's table with `GAME_DLL`.
fn table() -> RngTable {
    match std::env::var("GAME_DLL") {
        Ok(path) => {
            let bytes = std::fs::read(&path).expect("reading GAME_DLL");
            RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL")
        }
        Err(_) => {
            let mut bytes = [0u8; 256];
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = i as u8;
            }
            RngTable::from_bytes(bytes).expect("identity is a permutation")
        }
    }
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).ok()?;
    Some(Tables::from_game_dll(&bytes).expect("lookup tables"))
}

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    host.set_rng_table(table());
    {
        let mut g = host.game_mut();
        let ut = |name: &str| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x43D2_0000),
            ..Default::default()
        };
        g.unit_types_mut().insert(FOOTMAN, ut("Footman"));
        g.unit_types_mut().insert(RANGER, ut("Ranger"));
        let item = |class: u32| ItemType {
            class,
            level_ex: 1,
            level_plain: 1,
            flag_48: 1,
            charges: 0,
            flags: 0,
            powerup: false,
            sellable: false,
            pawnable: false,
        };
        g.item_types_mut().insert(ITEM_A, item(1));
        g.item_types_mut().insert(ITEM_B, item(2));
    }
    host
}

fn run(host: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

/// The pick the pool must make for one seed, computed from the generator alone: `U` is one draw of the global
/// generator (`random_real_soft(0, 1)`), the total of the weights is `total`, and the cumulative walk is written out.
fn expected_pick(table: &RngTable, seed: u32, weights: &[(u32, f32)]) -> u32 {
    let mut streams = Streams::default();
    streams.set_random_seed(table, seed);
    let u = streams.global.random_real_soft(table, F32::ZERO, F32::ONE);
    let total = weights.iter().fold(F32::ZERO, |acc, &(_, w)| {
        arith::add(acc, F32::from_bits(w.to_bits()))
    });
    let r = arith::mul(total, u);
    let mut cum = F32::ZERO;
    for &(id, w) in weights {
        cum = arith::add(cum, F32::from_bits(w.to_bits()));
        if arith::gt(cum, r) {
            return id;
        }
    }
    weights[0].0
}

#[test]
fn add_replaces_a_weight_and_remove_drops_the_first_match() {
    // One pool entry is enough to observe the pool: a draw from a single entry always picks it.
    let src = r#"
globals
  unitpool gP = null
  unit gU = null
endglobals
function Main takes nothing returns nothing
  set gP = CreateUnitPool()
  call UnitPoolAddUnitType(gP, 'hrng', 1.0)
  call UnitPoolAddUnitType(gP, 'hfoo', 1.0)
  call UnitPoolAddUnitType(gP, 'hfoo', 0.0)
  call UnitPoolRemoveUnitType(gP, 'hrng')
  set gU = PlaceRandomUnit(gP, Player(0), I2R(10), I2R(20), I2R(0))
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    let id = host
        .vm()
        .global("gU")
        .and_then(|v| v.as_handle())
        .expect("unit handle");
    let u = host.game().unit_of_handle(id).expect("a live unit");
    assert_eq!(
        host.game().unit(u).type_id,
        FOOTMAN,
        "only the footman is left"
    );
}

#[test]
fn empty_pool_gives_null_and_draws_nothing() {
    let src = r#"
globals
  unitpool gP = null
  itempool gIP = null
  integer gAfterU = 0
  integer gAfterI = 0
  item gI = null
  unit gU = null
endglobals
function Main takes nothing returns nothing
  set gP = CreateUnitPool()
  set gIP = CreateItemPool()
  call SetRandomSeed(42)
  set gU = PlaceRandomUnit(gP, Player(0), I2R(0), I2R(0), I2R(0))
  set gI = PlaceRandomItem(gIP, I2R(0), I2R(0))
  set gAfterU = GetRandomInt(0, 1000000)
  call SetRandomSeed(42)
  set gAfterI = GetRandomInt(0, 1000000)
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert_eq!(
        host.vm().global("gU").and_then(|v| v.as_handle()),
        Some(0),
        "null unit"
    );
    assert_eq!(
        host.vm().global("gI").and_then(|v| v.as_handle()),
        Some(0),
        "null item"
    );
    // Two empty placements drew nothing: the next draw is the first draw of seed 42.
    assert_eq!(int(&host, "gAfterU"), int(&host, "gAfterI"));
}

#[test]
fn item_pick_matches_one_draw_of_the_global_generator() {
    // Weights 1 and 3: the pick is A when U < 1/4, else B. Each placement is one draw; the oracle recomputes it.
    let weights = [(ITEM_A, 1.0f32), (ITEM_B, 3.0f32)];
    let table = table();
    let seeds: Vec<u32> = (1..=40).collect();
    let mut expected_a = 0;
    for &s in &seeds {
        if expected_pick(&table, s, &weights) == ITEM_A {
            expected_a += 1;
        }
    }
    assert!(
        expected_a > 0 && expected_a < seeds.len(),
        "both outcomes occur"
    );

    let src = r#"
globals
  itempool gIP = null
  integer gA = 0
  integer gB = 0
  integer gSeed = 0
endglobals
function Main takes nothing returns nothing
  local item it
  set gIP = CreateItemPool()
  call ItemPoolAddItemType(gIP, 'ItmA', 1.0)
  call ItemPoolAddItemType(gIP, 'ItmB', 3.0)
  loop
    exitwhen gSeed == 40
    set gSeed = gSeed + 1
    call SetRandomSeed(gSeed)
    set it = PlaceRandomItem(gIP, I2R(0), I2R(0))
    if GetItemTypeId(it) == 'ItmA' then
      set gA = gA + 1
    else
      set gB = gB + 1
    endif
  endloop
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 400);
    assert_eq!(int(&host, "gA") + int(&host, "gB"), 40);
    assert_eq!(
        int(&host, "gA"),
        expected_a as i32,
        "A picks match the generator"
    );
}

#[test]
fn unit_pick_consumes_one_draw_and_builds_the_picked_type() {
    let weights = [(FOOTMAN, 2.0f32), (RANGER, 2.0f32)];
    let table = table();
    let s = 9u32;
    let expected = expected_pick(&table, s, &weights);
    let src = format!(
        r#"
globals
  unitpool gP = null
  unit gU = null
  integer gAfter = 0
  integer gDirect = 0
endglobals
function Main takes nothing returns nothing
  set gP = CreateUnitPool()
  call UnitPoolAddUnitType(gP, 'hfoo', 2.0)
  call UnitPoolAddUnitType(gP, 'hrng', 2.0)
  call SetRandomSeed({s})
  set gU = PlaceRandomUnit(gP, Player(0), I2R(5), I2R(6), I2R(90))
  set gAfter = GetRandomInt(0, 1000000)
  call SetRandomSeed({s})
  set gDirect = GetRandomInt(0, 1000000)
endfunction
"#
    );
    let mut host = host_for(&src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    let id = host
        .vm()
        .global("gU")
        .and_then(|v| v.as_handle())
        .expect("handle");
    let u = host.game().unit_of_handle(id).expect("a live unit");
    assert_eq!(host.game().unit(u).type_id, expected);
    assert_eq!(host.game().unit(u).x.to_f32(), 5.0, "built at the position");
    // The placement drew once: the integer after it is the second draw of the seed.
    let mut streams = Streams::default();
    streams.set_random_seed(&table, s);
    let _ = streams.global.random_fraction(&table);
    assert_eq!(
        int(&host, "gAfter"),
        streams.global.random_int(&table, 0, 1_000_000),
        "one draw for the pick"
    );
    assert_ne!(int(&host, "gAfter"), int(&host, "gDirect"));
}

#[test]
fn destroyed_pool_handles_are_invalid() {
    let src = r#"
globals
  unitpool gP = null
  unit gU = null
endglobals
function Main takes nothing returns nothing
  set gP = CreateUnitPool()
  call UnitPoolAddUnitType(gP, 'hfoo', 1.0)
  call DestroyUnitPool(gP)
  call UnitPoolAddUnitType(gP, 'hrng', 1.0)
  set gU = PlaceRandomUnit(gP, Player(0), I2R(0), I2R(0), I2R(0))
endfunction
"#;
    let mut host = host_for(src);
    host.start("Main", &[]).expect("start");
    run(&mut host, 2);
    assert_eq!(
        host.vm().global("gU").and_then(|v| v.as_handle()),
        Some(0),
        "a destroyed pool places nothing"
    );
}
