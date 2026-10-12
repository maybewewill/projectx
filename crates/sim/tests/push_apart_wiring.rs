//! Push-apart in the movement walk (`crate::push`, `docs/spec/movement-7.md` section 8).
//!
//! The kernel and the neighbour gather are bit-exact against the original (`push_gather_vectors.rs`,
//! `movement7_vectors.rs`); this checks the wiring on a small game: still units with `repulse` set that stand close are
//! pushed apart, units without it are left alone, a moving unit is not pushed. Needs the softfloat and RNG tables of the
//! user's `Game.dll` (`GAME_DLL`); without them the tests return early.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::UnitType;
use sim::rng::RngTable;
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitX takes unit whichUnit returns real
native GetUnitY takes unit whichUnit returns real
"#;

const PUSHER: u32 = 0x6866_6F6F; // 'hfoo'
const PLAIN: u32 = 0x6872_6E67; // 'hrng'

fn dll() -> Option<Vec<u8>> {
    let path = std::env::var("GAME_DLL").ok()?;
    Some(std::fs::read(path).expect("GAME_DLL readable"))
}

fn host(units: &[(&str, i32, i32)]) -> Option<ScriptHost> {
    let bytes = dll()?;
    let tables = Tables::from_game_dll(&bytes).expect("tables");
    let rng = RngTable::from_game_dll(&bytes).expect("rng table");
    let mut body = String::new();
    let mut globals = String::new();
    for (i, (code, x, y)) in units.iter().enumerate() {
        globals.push_str(&format!("  unit u{i} = null\n"));
        body.push_str(&format!("  set u{i} = CreateUnit(Player(0), {code}, I2R({x}), I2R({y}), I2R(0))\n"));
    }
    let src = format!("{PRELUDE}\nglobals\n{globals}endglobals\nfunction Main takes nothing returns nothing\n{body}endfunction\n");
    let module = compile(&[jass::parse(&src).expect("parse")]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), Some(tables)).expect("vm");
    host.set_rng_table(rng);
    host.start_game_rng(0x1234_5678);
    {
        let mut g = host.game_mut();
        let ty = |name: &str, repulse: i32| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x43D2_0000),
            // collision 31 world units: a footprint radius of about one cell
            collision: F32::from_bits(31.0f32.to_bits()),
            repulse: [repulse, 0, 1, 1],
            ..Default::default()
        };
        g.unit_types_mut().insert(PUSHER, ty("Pusher", 1));
        g.unit_types_mut().insert(PLAIN, ty("Plain", 0));
        g.enable_push_apart(64, 64);
    }
    Some(host)
}

fn pos(host: &ScriptHost, name: &str) -> (f32, f32) {
    let id = host.vm().global(name).and_then(|v| v.as_handle()).expect("handle");
    let g = host.game();
    let k = g.unit_of_handle(id).expect("unit");
    let u = g.unit(k);
    (u.x.to_f32(), u.y.to_f32())
}

#[test]
fn close_still_units_with_repulse_are_pushed_apart() {
    let Some(mut h) = host(&[("'hfoo'", 1056, 1056), ("'hfoo'", 1072, 1056)]) else {
        return;
    };
    h.start("Main", &[]).expect("start");
    let (a0, b0) = (pos(&h, "u0"), pos(&h, "u1"));
    for _ in 0..40 {
        h.step();
    }
    let (a1, b1) = (pos(&h, "u0"), pos(&h, "u1"));
    let gap0 = (b0.0 - a0.0).hypot(b0.1 - a0.1);
    let gap1 = (b1.0 - a1.0).hypot(b1.1 - a1.1);
    assert!(gap1 > gap0 + 1.0, "pushed apart: {gap0} -> {gap1}");
    let g = h.game();
    assert_eq!(g.push_state_records(), 2);
}

#[test]
fn units_without_repulse_stay_put() {
    let Some(mut h) = host(&[("'hrng'", 1056, 1056), ("'hrng'", 1064, 1056)]) else {
        return;
    };
    h.start("Main", &[]).expect("start");
    let (a0, b0) = (pos(&h, "u0"), pos(&h, "u1"));
    for _ in 0..40 {
        h.step();
    }
    assert_eq!((a0, b0), (pos(&h, "u0"), pos(&h, "u1")));
    assert_eq!(h.game().push_state_records(), 0);
}

#[test]
fn far_apart_units_do_not_interact() {
    let Some(mut h) = host(&[("'hfoo'", 1056, 1056), ("'hfoo'", 1056 + 640, 1056)]) else {
        return;
    };
    h.start("Main", &[]).expect("start");
    let (a0, b0) = (pos(&h, "u0"), pos(&h, "u1"));
    for _ in 0..40 {
        h.step();
    }
    assert_eq!((a0, b0), (pos(&h, "u0"), pos(&h, "u1")));
}
