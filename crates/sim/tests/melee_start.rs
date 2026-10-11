//! Melee start natives through `ScriptHost` (`natives/melee_start.rs`, spec `docs/spec/melee-start-natives.md`).
//!
//! The random picks use the identity RNG table unless `GAME_DLL` is set (as in `item_natives.rs`); the expected indexes
//! are computed from a fresh `Streams` with the same table, so the test does not depend on the table contents.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{AbilityType, UnitType};
use sim::natives::melee_start::{
    building_candidate, creep_candidate, tileset_accepts, ALARM_ABILITY, SLEEP_ABILITY,
    SLEEP_PERM_ABILITY,
};
use sim::natives::{PickRecord, Playable};
use sim::{RngTable, ScriptHost, Streams};
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type rect extends handle

native I2R takes integer i returns real
native R2I takes real r returns integer
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetHandleId takes handle h returns integer
native GetUnitTypeId takes unit u returns integer
native GetOwningPlayer takes unit u returns player
native GetPlayerId takes player p returns integer
native IsUnitHidden takes unit u returns boolean
native GetUnitX takes unit u returns real
native GetUnitY takes unit u returns real
native CreateBlightedGoldmine takes player id, real x, real y, real face returns unit
native SetResourceAmount takes unit u, integer a returns nothing
native GetResourceAmount takes unit u returns integer
native GetWorldBounds takes nothing returns rect
native GetRectMinX takes rect r returns real
native GetRectMinY takes rect r returns real
native GetRectMaxX takes rect r returns real
native GetRectMaxY takes rect r returns real
native ChooseRandomCreep takes integer level returns integer
native ChooseRandomNPBuilding takes nothing returns integer
native UnitAddSleep takes unit u, boolean b returns nothing
native UnitAddSleepPerm takes unit u, boolean b returns nothing
native UnitCanSleep takes unit u returns boolean
native UnitCanSleepPerm takes unit u returns boolean
native UnitIsSleeping takes unit u returns boolean
native UnitWakeUp takes unit u returns nothing
native UnitIgnoreAlarm takes unit u, boolean b returns boolean
native UnitIgnoreAlarmToggled takes unit u returns boolean
native RecycleGuardPosition takes unit u returns nothing
native RemoveGuardPosition takes unit u returns nothing
native RemoveAllGuardPositions takes player p returns nothing
"#;

const GLOBALS: &str = r#"
globals
unit u1
unit u2
unit u3
unit nul
rect r1
integer i1 = -1
integer i2 = -1
integer i3 = -1
integer i4 = -1
integer i5 = -1
boolean b1 = false
boolean b2 = false
boolean b3 = false
boolean b4 = false
boolean b5 = false
boolean b6 = false
real x1 = 0.0
real x2 = 0.0
real x3 = 0.0
real x4 = 0.0
endglobals
"#;

const NGOL: u32 = 0x6E67_6F6C;
const UGOL: u32 = 0x7567_6F6C;
const FOOT: u32 = 0x6866_6F6F;

fn rng_table() -> RngTable {
    if let Ok(path) = std::env::var("GAME_DLL") {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
        return RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL");
    }
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    RngTable::from_bytes(bytes).expect("identity is a permutation")
}

fn ut(name: &str) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: F32::from_bits(0x43D2_0000),
        ..Default::default()
    }
}

fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{GLOBALS}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    host.set_rng_table(rng_table());
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(NGOL, ut("Gold Mine"));
        g.unit_types_mut().insert(UGOL, ut("Haunted Gold Mine"));
        g.unit_types_mut().insert(FOOT, ut("Footman"));
        for code in [SLEEP_ABILITY, SLEEP_PERM_ABILITY, ALARM_ABILITY] {
            g.ability_types_mut().insert(code, AbilityType { max_level: None });
        }
    }
    host
}

fn run(host: &mut ScriptHost) {
    host.start("main", &[]).expect("main starts");
    for _ in 0..4 {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm().global(name).and_then(|v| v.as_integer()).expect(name)
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm().global(name).and_then(|v| v.as_boolean()).expect(name)
}

fn real(host: &ScriptHost, name: &str) -> f32 {
    host.vm().global(name).and_then(|v| v.as_real()).expect(name).to_f32()
}

#[test]
fn blighted_goldmine_builds_a_hidden_neutral_mine_and_a_haunted_mine() {
    let mut host = host_for(
        "function main takes nothing returns nothing
    set u1 = CreateBlightedGoldmine(Player(0), I2R(256), I2R(-128), I2R(90))
    set i1 = GetUnitTypeId(u1)
    set i2 = GetPlayerId(GetOwningPlayer(u1))
    set b1 = IsUnitHidden(u1)
    call SetResourceAmount(u1, 750)
    set i3 = GetResourceAmount(u1)
    set x1 = GetUnitX(u1)
    set x2 = GetUnitY(u1)
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set i4 = GetHandleId(u2) - GetHandleId(u1)
    set u3 = CreateBlightedGoldmine(null, I2R(0), I2R(0), I2R(0))
    set b2 = u3 == null
endfunction",
    );
    run(&mut host);
    assert_eq!(int(&host, "i1") as u32, UGOL);
    assert_eq!(int(&host, "i2"), 0, "owned by the player");
    assert!(!boolean(&host, "b1"), "the haunted mine is visible");
    assert_eq!(int(&host, "i3"), 750, "SetResourceAmount works on the new mine");
    assert_eq!((real(&host, "x1"), real(&host, "x2")), (256.0, -128.0));
    assert_eq!(
        int(&host, "i4"),
        1,
        "the hidden neutral mine takes no handle id: the next unit's id follows the mine's"
    );
    assert!(boolean(&host, "b2"), "an invalid player gives null");
    let g = host.game();
    let hidden: Vec<_> = g
        .unit_objects()
        .filter(|(_, u)| u.type_id == NGOL)
        .map(|(_, u)| (u.owner, u.hidden))
        .collect();
    assert_eq!(hidden, vec![(15, true)], "one hidden gold mine for player 15");
    assert_eq!(
        g.unit_objects().filter(|(_, u)| u.type_id == UGOL).count(),
        1
    );
}

#[test]
fn world_bounds_is_a_copy_of_the_host_rect_without_clamp() {
    let mut host = host_for(
        "function main takes nothing returns nothing
    set r1 = GetWorldBounds()
    set x1 = GetRectMinX(r1)
    set x2 = GetRectMinY(r1)
    set x3 = GetRectMaxX(r1)
    set x4 = GetRectMaxY(r1)
endfunction",
    );
    {
        let mut g = host.game_mut();
        // Circumvention: w3e offset (-6144, -6144), 96 x 96 tiles.
        let f = |v: f32| F32::from_bits(v.to_bits());
        g.set_playable(Some(Playable {
            min_x: f(-5248.0),
            min_y: f(-5504.0),
            max_x: f(5248.0),
            max_y: f(4992.0),
        }));
        g.set_world_bounds(Some(Playable {
            min_x: f(-6144.0),
            min_y: f(-6144.0),
            max_x: f(6144.0),
            max_y: f(6144.0),
        }));
    }
    run(&mut host);
    assert_eq!(real(&host, "x1"), -6144.0);
    assert_eq!(real(&host, "x2"), -6144.0);
    assert_eq!(real(&host, "x3"), 6144.0);
    assert_eq!(real(&host, "x4"), 6144.0);
}

fn rec(race: i32, level: i32, building: bool) -> PickRecord {
    PickRecord {
        race,
        level,
        is_building: building,
        tilesets: "*".to_string(),
        nb_random: 1,
        ..Default::default()
    }
}

#[test]
fn pick_filters_follow_the_walks() {
    let ok = rec(8, 3, false);
    assert!(creep_candidate(&ok, 3, 0));
    assert!(creep_candidate(&ok, -1, b'L'));
    assert!(!creep_candidate(&ok, 4, 0), "level mismatch");
    assert!(!creep_candidate(&rec(7, 3, false), 3, 0), "race");
    assert!(!creep_candidate(&rec(8, 3, true), 3, 0), "building");
    assert!(!creep_candidate(&PickRecord { special: 1, ..ok.clone() }, 3, 0));
    assert!(!creep_candidate(&PickRecord { code_upper_first: true, ..ok.clone() }, 3, 0));
    assert!(!creep_candidate(&PickRecord { tilesets: "A".into(), ..ok.clone() }, 3, b'L'));
    assert!(creep_candidate(&PickRecord { tilesets: "AL".into(), ..ok.clone() }, 3, b'L'));
    assert!(creep_candidate(&PickRecord { tilesets: "A".into(), ..ok.clone() }, 3, 0));
    let nb = rec(7, 0, true);
    assert!(building_candidate(&nb, b'L'));
    assert!(!building_candidate(&PickRecord { nb_random: 0, ..nb.clone() }, 0));
    assert!(!building_candidate(&PickRecord { hostile_pal: 1, ..nb.clone() }, 0));
    assert!(!building_candidate(&rec(7, 0, false), 0));
    assert!(tileset_accepts("", 0));
    assert!(!tileset_accepts("", b'L'));
}

#[test]
fn random_creep_draws_stream_32_over_the_row_order() {
    let mut host = host_for(
        "function main takes nothing returns nothing
    set i1 = ChooseRandomCreep(3)
    set i2 = ChooseRandomCreep(3)
    set i3 = ChooseRandomCreep(99)
    set i4 = ChooseRandomNPBuilding()
    set i5 = ChooseRandomNPBuilding()
endfunction",
    );
    // Insertion order is NOT code order: 'zomb' first, then 'abcd', 'nwlt'.
    let ids = [0x7a6f_6d62u32, 0x6162_6364, 0x6e77_6c74, 0x6e62_6b31, 0x6e62_6b32];
    {
        let mut g = host.game_mut();
        g.unit_types_mut().set_pick_record(ids[0], rec(8, 3, false));
        g.unit_types_mut().set_pick_record(ids[1], rec(8, 3, false));
        g.unit_types_mut().set_pick_record(ids[2], rec(8, 3, false));
        g.unit_types_mut().set_pick_record(ids[3], rec(7, 0, true));
        g.unit_types_mut().set_pick_record(ids[4], rec(7, 0, true));
    }
    run(&mut host);
    let table = rng_table();
    let mut creep = Streams::default();
    let mut bld = Streams::default();
    let idx = |streams: &mut Streams, stream: usize, n: u64| {
        let r = streams.stream_mut(stream).next_u32(&table);
        ((u64::from(r) * n) >> 32) as usize
    };
    assert_eq!(int(&host, "i1") as u32, ids[idx(&mut creep, 32, 3)]);
    assert_eq!(int(&host, "i2") as u32, ids[idx(&mut creep, 32, 3)]);
    assert_eq!(int(&host, "i3"), 0, "no candidate of level 99, and no draw");
    assert_eq!(int(&host, "i4") as u32, ids[3 + idx(&mut bld, 34, 2)]);
    assert_eq!(int(&host, "i5") as u32, ids[3 + idx(&mut bld, 34, 2)]);
}

#[test]
fn sleep_and_alarm_state() {
    let mut host = host_for(
        "function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set b1 = UnitCanSleep(u1)
    call UnitAddSleep(u1, true)
    set b2 = UnitCanSleep(u1)
    call UnitAddSleep(u1, true)
    call UnitAddSleep(u1, false)
    set b3 = UnitCanSleep(u1)
    set b4 = UnitCanSleepPerm(u1)
    set b5 = UnitIsSleeping(u1)
    call UnitAddSleepPerm(u1, true)
    set b6 = UnitCanSleepPerm(u1)
    set i1 = 0
    if UnitIsSleeping(u1) then
        set i1 = 1
    endif
    call UnitWakeUp(u1)
    if UnitIsSleeping(u1) then
        set i1 = i1 + 10
    endif
    set i2 = 0
    if UnitIgnoreAlarm(u1, true) then
        set i2 = 1
    endif
    if UnitIgnoreAlarmToggled(u1) then
        set i2 = i2 + 10
    endif
    call RecycleGuardPosition(u1)
    call RemoveGuardPosition(u1)
    call RemoveGuardPosition(nul)
    call RemoveAllGuardPositions(Player(0))
    call UnitWakeUp(nul)
    set i3 = 0
    if UnitCanSleep(nul) then
        set i3 = 1
    endif
endfunction",
    );
    run(&mut host);
    assert!(!boolean(&host, "b1"));
    assert!(boolean(&host, "b2"));
    assert!(!boolean(&host, "b3"), "added then removed");
    assert!(!boolean(&host, "b4"));
    assert!(!boolean(&host, "b5"), "no Asla, not sleeping");
    assert!(boolean(&host, "b6"));
    assert_eq!(int(&host, "i1"), 1, "asleep until woken up");
    assert_eq!(int(&host, "i2"), 0, "the unit has no Arla ability: nothing to toggle");
    assert_eq!(int(&host, "i3"), 0);
}

#[test]
fn alarm_toggle_with_the_ability() {
    let mut host = host_for(
        "function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set b1 = UnitIgnoreAlarmToggled(u1)
    set b2 = UnitIgnoreAlarm(u1, true)
    set b3 = UnitIgnoreAlarmToggled(u1)
    set b4 = UnitIgnoreAlarm(u1, false)
    set b5 = UnitIgnoreAlarmToggled(u1)
endfunction",
    );
    {
        let mut g = host.game_mut();
        let mut t = ut("Footman");
        t.abilities = vec![ALARM_ABILITY];
        g.unit_types_mut().insert(FOOT, t);
    }
    run(&mut host);
    assert!(!boolean(&host, "b1"));
    assert!(boolean(&host, "b2"), "the call reports that the ability exists");
    assert!(boolean(&host, "b3"));
    assert!(boolean(&host, "b4"));
    assert!(!boolean(&host, "b5"));
}
