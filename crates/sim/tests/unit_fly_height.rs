//! The fly-height property of a flying unit (`crate::unitz`, `docs/spec/terrain.md` section 6.3): `SetUnitFlyHeight`
//! with a rate climbs linearly to the target and stops there; a zero rate jumps; `GetUnitFlyHeight` answers the target
//! at once. The unit height adds the property to the ground.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ObjKey, UnitType};
use sim::{ScriptHost};
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native SetUnitFlyHeight takes unit whichUnit, real newHeight, real rate returns nothing
native GetUnitFlyHeight takes unit whichUnit returns real

globals
    unit u
    real got
endglobals

function setup takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hbrd', I2R(0), I2R(0), I2R(0))
endfunction

function climb takes nothing returns nothing
    call SetUnitFlyHeight(u, I2R(300), I2R(50))
    set got = GetUnitFlyHeight(u)
endfunction

function descend takes nothing returns nothing
    call SetUnitFlyHeight(u, I2R(100), I2R(100))
endfunction

function jump takes nothing returns nothing
    call SetUnitFlyHeight(u, I2R(250), I2R(0))
endfunction
"#;

const BIRD: u32 = 0x6862_7264; // 'hbrd'

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn host() -> ScriptHost {
    let prog = jass::parse(PRELUDE).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    host.game_mut().unit_types_mut().insert(
        BIRD,
        UnitType {
            name: "Bird".to_string(),
            max_life: f(10.0),
            fly_height: f(100.0),
            movement: 2,
            ..Default::default()
        },
    );
    host
}

fn unit(host: &ScriptHost) -> ObjKey {
    let id = host.vm().global("u").and_then(|v| v.as_handle()).expect("unit global");
    host.game().unit_of_handle(id).expect("live unit")
}

#[test]
fn a_rate_climbs_linearly_and_stops_at_the_target() {
    let mut host = host();
    host.start("setup", &[]).expect("setup");
    host.step();
    let k = unit(&host);
    host.game_mut().set_game_time(f(10.0));
    assert_eq!(host.game().fly_property(k).to_f32(), 100.0, "a new unit hovers at its default height");
    host.start("climb", &[]).expect("climb");
    // GetUnitFlyHeight answers the target field at once.
    let got = host.vm().global("got").and_then(|v| v.as_real()).expect("real");
    assert_eq!(got.to_f32(), 300.0);
    host.game_mut().set_game_time(f(12.0));
    assert_eq!(host.game().fly_property(k).to_f32(), 200.0, "2 s at 50 per second");
    host.game_mut().set_game_time(f(50.0));
    assert_eq!(host.game().fly_property(k).to_f32(), 300.0, "stops at the target");
    // Descending reverses the slope.
    host.start("descend", &[]).expect("descend");
    host.game_mut().set_game_time(f(50.5));
    assert_eq!(host.game().fly_property(k).to_f32(), 250.0);
    host.game_mut().set_game_time(f(60.0));
    assert_eq!(host.game().fly_property(k).to_f32(), 100.0);
    // A zero rate jumps at once.
    host.start("jump", &[]).expect("jump");
    assert_eq!(host.game().fly_property(k).to_f32(), 250.0);
    assert_eq!(host.game().current_fly_height(k).to_f32(), 250.0, "a flying unit's height reads the property");
}
