//! A crowd of regenerating units must not make the agile tick expensive (the pump looks at every pending timer). Needs `GAME_DLL`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::UnitType;
use sim::ScriptHost;
use softfloat::{Tables, F32};

const SRC: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
globals
    integer i = 0
endglobals
function main takes nothing returns nothing
    loop
        exitwhen i >= 400
        call CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
        set i = i + 1
    endloop
endfunction
"#;

#[test]
fn four_hundred_regenerating_units_tick_in_reasonable_time() {
    let Ok(path) = std::env::var("GAME_DLL") else { return };
    let bytes = std::fs::read(&path).unwrap();
    let t = Tables::from_game_dll(&bytes).unwrap();
    let module = compile(&[jass::parse(SRC).expect("parse")]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), Some(t)).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            u32::from_be_bytes(*b"hfoo"),
            UnitType {
                name: "Footman".to_string(),
                max_life: F32::from_bits(0x42C8_0000),
                max_mana: F32::from_bits(0x4248_0000),
                regen_hp: F32::from_bits(0x4000_0000),
                regen_mana: F32::from_bits(0x3F00_0000),
                regen_type: 1,
                ..Default::default()
            },
        );
    }
    host.start("main", &[]).expect("main");
    let start = std::time::Instant::now();
    host.step_ticks(4000);
    let took = start.elapsed();
    eprintln!("4000 ticks with 400 units: {took:?}");
    assert!(took.as_secs() < 60, "{took:?}");
}
