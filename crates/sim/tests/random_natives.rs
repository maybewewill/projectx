//! `GetRandomInt`, `GetRandomReal` and `SetRandomSeed` through a JASS script, compared with direct
//! calls on `sim::Streams` (the same generator and streams the natives use).
//!
//! The logic is checked with a stand-in permutation (identity) so it runs without the game. The
//! `GAME_DLL` tests repeat the checks with the permutation table of the user's `Game.dll`.

use jass::compile::compile;
use jass::vm::{NativeRegistry, RunStatus, Value};
use sim::{RngTable, ScriptHost, Streams};

const PRELUDE: &str = r#"
native GetRandomInt takes integer lo, integer hi returns integer
native GetRandomReal takes real lo, real hi returns real
native SetRandomSeed takes integer seed returns nothing

globals
    integer array RI
    real array RR
endglobals

function Draw takes nothing returns nothing
    local integer i = 0
    loop
        exitwhen i == 20
        set RI[i] = GetRandomInt(1, 6)
        set RR[i] = GetRandomReal(0.0, 10.0)
        set i = i + 1
    endloop
endfunction

function Reseed takes integer s returns nothing
    call SetRandomSeed(s)
endfunction
"#;

/// Number of draws of each kind in `Draw`.
const DRAWS: usize = 20;

/// The identity permutation: a valid table that is not the game's data.
fn stand_in_table() -> RngTable {
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    RngTable::from_bytes(bytes).expect("identity is a permutation")
}

/// The permutation table of the user's `Game.dll`, or `None` (with a note) if `GAME_DLL` is unset.
fn game_table() -> Option<RngTable> {
    let Ok(path) = std::env::var("GAME_DLL") else {
        eprintln!("GAME_DLL not set: skipping test that needs the Game.dll RNG table");
        return None;
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL"))
}

fn host_with(table: Option<RngTable>) -> ScriptHost {
    let prog = jass::parse(PRELUDE).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    if let Some(t) = table {
        host.set_rng_table(t);
    }
    host
}

/// The integers and the reals (as bits) that `Draw` stored.
fn read_draws(host: &ScriptHost) -> (Vec<i32>, Vec<u32>) {
    let ints = (0..DRAWS)
        .map(|i| {
            host.vm()
                .global_element("RI", i)
                .and_then(|v| v.as_integer())
                .expect("RI element")
        })
        .collect();
    let reals = (0..DRAWS)
        .map(|i| match host.vm().global_element("RR", i) {
            Some(Value::Real(r)) => r.to_bits(),
            other => panic!("RR[{i}] is {other:?}"),
        })
        .collect();
    (ints, reals)
}

/// The same draws as `Draw`, made directly on the generator.
fn direct_draws(t: &RngTable, s: &mut Streams) -> (Vec<i32>, Vec<u32>) {
    let mut ints = Vec::new();
    let mut reals = Vec::new();
    for _ in 0..DRAWS {
        ints.push(s.global.random_int(t, 1, 6));
        reals.push(s.global.random_real(t, 0.0, 10.0).to_bits());
    }
    (ints, reals)
}

fn script_draws_match_direct(t: RngTable) {
    let mut host = host_with(Some(t.clone()));
    host.start_game_rng(0x1234_5678);
    host.vm_mut().call("Draw", &[]).expect("Draw runs");

    let mut direct = Streams::default();
    direct.start_game(&t, 0x1234_5678);
    let (ints, reals) = direct_draws(&t, &mut direct);

    assert_eq!(read_draws(&host), (ints, reals));
    assert_eq!(
        *host.rng(),
        direct,
        "generator and stream state after the draws"
    );
}

fn set_random_seed_reseeds_everything(t: RngTable) {
    let mut host = host_with(Some(t.clone()));
    host.start_game_rng(99);
    host.vm_mut()
        .call("Reseed", &[Value::Integer(42)])
        .expect("Reseed runs");

    let mut direct = Streams::default();
    direct.start_game(&t, 99);
    direct.set_random_seed(&t, 42);
    assert_eq!(
        *host.rng(),
        direct,
        "SetRandomSeed reseeds global and streams"
    );

    host.vm_mut().call("Draw", &[]).expect("Draw runs");
    let (ints, reals) = direct_draws(&t, &mut direct);
    assert_eq!(read_draws(&host), (ints, reals));
}

#[test]
fn script_draws_match_direct_draws_stand_in_table() {
    script_draws_match_direct(stand_in_table());
}

#[test]
fn set_random_seed_reseeds_stand_in_table() {
    set_random_seed_reseeds_everything(stand_in_table());
}

#[test]
fn reseeding_to_the_same_value_repeats_the_sequence() {
    let t = stand_in_table();
    let mut host = host_with(Some(t));
    host.vm_mut()
        .call("Reseed", &[Value::Integer(7)])
        .expect("Reseed runs");
    host.vm_mut().call("Draw", &[]).expect("Draw runs");
    let first = read_draws(&host);

    host.vm_mut()
        .call("Reseed", &[Value::Integer(7)])
        .expect("Reseed runs");
    host.vm_mut().call("Draw", &[]).expect("Draw runs");
    assert_eq!(read_draws(&host), first);
}

#[test]
fn random_natives_stop_the_thread_without_table() {
    let mut host = host_with(None);
    assert_eq!(
        host.vm_mut().call("Draw", &[]),
        Err(RunStatus::MissingTables)
    );
    assert_eq!(
        host.vm_mut().call("Reseed", &[Value::Integer(5)]),
        Err(RunStatus::MissingTables)
    );
    assert_eq!(
        *host.rng(),
        Streams::default(),
        "no draw and no reseed happened"
    );
    assert!(!host.start_game_rng(5), "game start needs the table");
}

#[test]
fn script_draws_match_original_game_table() {
    let Some(t) = game_table() else { return };
    script_draws_match_direct(t);
}

#[test]
fn set_random_seed_reseeds_original_game_table() {
    let Some(t) = game_table() else { return };
    set_random_seed_reseeds_everything(t);
}
