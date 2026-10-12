//! Unit life and mana regeneration in the game model (`crate::regen`, `unit_stats.rs`; docs/spec/agile.md section 9). The
//! exact arithmetic is pinned by `regen_vectors.rs` and `daynight_vectors.rs`; these tests check the wiring: creation,
//! the damage / heal write paths, death, the day and night regeneration types and the caches. They need `GAME_DLL` (the
//! properties and the soft-float division live on the tables) and return early without it.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native KillUnit takes unit whichUnit returns boolean
"#;

const FOOT: u32 = u32::from_be_bytes(*b"hfoo");
const ELF: u32 = u32::from_be_bytes(*b"ewsp");
const SUN: u32 = u32::from_be_bytes(*b"hsun");
const CASTER: u32 = u32::from_be_bytes(*b"hmpr");

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn ty(name: &str, regen_hp: f32, regen_type: u32, max_mana: f32, regen_mana: f32) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: f(100.0),
        max_mana: f(max_mana),
        start_mana: f(0.0),
        regen_hp: f(regen_hp),
        regen_mana: f(regen_mana),
        regen_type,
        ..Default::default()
    }
}

const SRC: &str = r#"
globals
    unit foot = null
    unit elf = null
    unit sun = null
    unit caster = null
endglobals
function main takes nothing returns nothing
    set foot = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    set elf = CreateUnit(Player(0), 'ewsp', 200.0, 0.0, 0.0)
    set sun = CreateUnit(Player(0), 'hsun', 400.0, 0.0, 0.0)
    set caster = CreateUnit(Player(0), 'hmpr', 600.0, 0.0, 0.0)
endfunction
function kill_foot takes nothing returns nothing
    call KillUnit(foot)
endfunction
"#;

fn rig(day_cycle: bool) -> Option<ScriptHost> {
    let t = tables()?;
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), Some(t)).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(FOOT, ty("Footman", 2.0, 1, 0.0, 0.0));
        g.unit_types_mut().insert(ELF, ty("Wisp", 2.0, 4, 0.0, 0.0));
        g.unit_types_mut().insert(SUN, ty("Sunny", 2.0, 3, 0.0, 0.0));
        g.unit_types_mut().insert(CASTER, ty("Priest", 0.25, 1, 200.0, 0.5));
        if day_cycle {
            g.start_time_of_day(F32::ONE);
        }
    }
    host.start("main", &[]).expect("main");
    Some(host)
}

fn key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host.vm().global(name).and_then(|v| v.as_handle()).unwrap_or_else(|| panic!("{name}"));
    host.game().unit_of_handle(id).expect("unit")
}

fn life(host: &ScriptHost, k: ObjKey) -> f32 {
    host.game().unit(k).life.to_f32()
}

#[test]
fn a_damaged_unit_regenerates_its_rate_per_second() {
    let Some(mut host) = rig(false) else { return };
    let foot = key(&host, "foot");
    host.game_mut().write_life(foot, f(40.0));
    assert_eq!(life(&host, foot), 40.0);
    host.step_ticks(200);
    // One second of the clock (200 ticks of 0.005 s, with the soft-float drift) at 2.0 per second.
    let l = life(&host, foot);
    assert!((l - 42.0).abs() < 0.05, "life {l}");
    host.step_ticks(200);
    let l2 = life(&host, foot);
    assert!((l2 - 44.0).abs() < 0.1, "life {l2}");
}

#[test]
fn regeneration_stops_at_the_maximum_and_a_heal_is_clamped() {
    let Some(mut host) = rig(false) else { return };
    let foot = key(&host, "foot");
    host.game_mut().write_life(foot, f(99.0));
    host.step_ticks(400);
    assert_eq!(life(&host, foot), 100.0, "clamped at the maximum by the property");
    host.game_mut().write_life(foot, f(500.0));
    assert_eq!(life(&host, foot), 100.0, "a set above the maximum is clamped");
}

#[test]
fn mana_regenerates_for_a_unit_with_mana_only() {
    let Some(mut host) = rig(false) else { return };
    let caster = key(&host, "caster");
    let foot = key(&host, "foot");
    host.step_ticks(200);
    let m = host.game().unit(caster).mana.to_f32();
    assert!((m - 0.5).abs() < 0.02, "mana {m}");
    assert_eq!(host.game().unit(foot).mana.to_f32(), 0.0, "no mana, no modifier");
}

#[test]
fn a_dead_unit_does_not_regenerate() {
    let Some(mut host) = rig(false) else { return };
    let foot = key(&host, "foot");
    host.start("kill_foot", &[]).expect("kill");
    host.step_ticks(10);
    let after_release = life(&host, foot);
    assert!(after_release < 0.01, "a corpse keeps only the residue of the release delay: {after_release}");
    host.step_ticks(400);
    assert_eq!(life(&host, foot), after_release, "nothing regenerates after the modifier is gone");
    assert!(host.game().unit(foot).dead);
}

#[test]
fn night_and_day_types_follow_the_day_flag() {
    let Some(mut host) = rig(true) else { return };
    let elf = key(&host, "elf");
    let sun = key(&host, "sun");
    host.game_mut().write_life(elf, f(10.0));
    host.game_mut().write_life(sun, f(10.0));
    // Time of day starts at 0.0 hours: night. The night type (4) regenerates, the day type (3) does not.
    host.step_ticks(1000);
    let (e1, s1) = (life(&host, elf), life(&host, sun));
    assert!(e1 > 10.0 + 8.0, "the night type regenerates at night: {e1}");
    assert_eq!(s1, 10.0, "the day type does not regenerate at night");
    assert!(!host.game().day_flag());
    // The watch wakes at about 6.01 hours (tick ~12023 of the default 240 s / 24 h cycle) and turns the flag on.
    host.step_ticks(10500);
    host.game_mut().write_life(elf, f(10.0));
    host.game_mut().write_life(sun, f(10.0));
    host.step_ticks(1100);
    let (e2, s2) = (life(&host, elf), life(&host, sun));
    assert!(host.game().day_flag(), "the watch turned the day on");
    // The night type ran from tick 11500 to the dawn (~523 ticks, 2.6 s, +5.2), the day type from the dawn on (~577 ticks).
    assert!((e2 - 15.2).abs() < 0.4, "night type until the dawn: {e2}");
    assert!((s2 - 15.8).abs() < 0.4, "day type after the dawn: {s2}");
    let before = life(&host, elf);
    host.step_ticks(400);
    assert_eq!(life(&host, elf), before, "the night type is off by day");
    assert!(life(&host, sun) > s2 + 3.0, "the day type keeps regenerating");
}

#[test]
fn caches_follow_every_tick() {
    let Some(mut host) = rig(false) else { return };
    let foot = key(&host, "foot");
    host.game_mut().write_life(foot, f(10.0));
    let mut last = life(&host, foot);
    for _ in 0..50 {
        host.step_ticks(1);
        let l = life(&host, foot);
        assert!(l >= last, "life never decreases while regenerating");
        last = l;
    }
    assert!(last > 10.0);
}
