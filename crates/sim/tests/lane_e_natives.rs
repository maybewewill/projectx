//! Lane E natives through `ScriptHost` (`natives/lane_e.rs`): selection, player removal and crippling, hero cache,
//! player score, timed turns, shared vision, strings and names, presentation-only unit natives, level/save/load.
//! Synthetic data only. Tests that need the soft-float division tables return early without `GAME_DLL`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{HeroObj, UnitType};
use sim::natives::lane_e::{parse_wts, player_score, stat, HeroCacheEntry, HostRequest, LaneEvent};
use sim::natives::presentation::PresentationEvent;
use sim::ScriptHost;
use softfloat::{Tables, F32};

const FOOTMAN: u32 = 0x6866_6F6F; // 'hfoo'
const HPAL: u32 = 0x4870_616C; // 'Hpal'
const HAMG: u32 = 0x4861_6D67; // 'Hamg'

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type force extends handle
type group extends handle
type boolexpr extends handle
type playerscore extends handle
type playergameresult extends handle
type effecttype extends handle
type soundtype extends handle
type playercolor extends handle
type raritycontrol extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitFacing takes unit whichUnit returns real
native CreateForce takes nothing returns force
native ForceAddPlayer takes force whichForce, player whichPlayer returns nothing
native CreateGroup takes nothing returns group
native FirstOfGroup takes group g returns unit
native GroupRemoveUnit takes group g, unit u returns nothing
native IsUnitInGroup takes unit u, group g returns boolean
native GroupEnumUnitsSelected takes group whichGroup, player whichPlayer, boolexpr filter returns nothing
native SelectUnit takes unit whichUnit, boolean flag returns nothing
native ClearSelection takes nothing returns nothing
native SyncSelections takes nothing returns nothing
native SetPlayerUnitsOwner takes player whichPlayer, integer newOwner returns nothing
native CripplePlayer takes player whichPlayer, force toWhichPlayers, boolean flag returns nothing
native RemovePlayer takes player whichPlayer, playergameresult gameResult returns nothing
native ConvertPlayerGameResult takes integer i returns playergameresult
native ConvertPlayerScore takes integer i returns playerscore
native GetPlayerScore takes player whichPlayer, playerscore whichPlayerScore returns integer
native CachePlayerHeroData takes player whichPlayer returns nothing
native SetUnitFacingTimed takes unit whichUnit, real facingAngle, real duration returns nothing
native UnitShareVision takes unit whichUnit, player who, boolean share returns nothing
native StringLength takes string s returns integer
native StringCase takes string source, boolean upper returns string
native GetLocalizedString takes string source returns string
native GetObjectName takes integer objectId returns string
native ConvertEffectType takes integer i returns effecttype
native ConvertSoundType takes integer i returns soundtype
native GetAbilityEffectById takes integer abilityId, effecttype t, integer index returns string
native GetAbilitySoundById takes integer abilityId, soundtype t returns string
native SetUnitAnimation takes unit whichUnit, string whichAnimation returns nothing
native SetUnitAnimationByIndex takes unit whichUnit, integer whichAnimation returns nothing
native SetUnitAnimationWithRarity takes unit whichUnit, string whichAnimation, raritycontrol rarity returns nothing
native ConvertRarityControl takes integer i returns raritycontrol
native SetUnitBlendTime takes unit whichUnit, real blendTime returns nothing
native SetUnitColor takes unit whichUnit, playercolor whichColor returns nothing
native ConvertPlayerColor takes integer i returns playercolor
native UnitAddIndicator takes unit whichUnit, integer red, integer green, integer blue, integer alpha returns nothing
native ChangeLevel takes string newLevel, boolean doScoreScreen returns nothing
native LoadGame takes string saveFileName, boolean doScoreScreen returns nothing
native SaveGame takes string saveFileName returns nothing
"#;

const GLOBALS: &str = r#"
globals
unit u1
unit u2
unit u3
group g
force f
integer i1 = -1
integer i2 = -1
integer i3 = -1
integer i4 = -1
boolean b1 = false
boolean b2 = false
string s1 = null
string s2 = null
string s3 = null
endglobals
"#;

fn host_for(body: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{GLOBALS}\n{body}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        let ty = |name: &str| UnitType {
            name: name.to_string(),
            max_life: F32::from_bits(0x4416_0000),
            ..Default::default()
        };
        g.unit_types_mut().insert(FOOTMAN, ty("Footman"));
        g.unit_types_mut().insert(HPAL, ty("Paladin"));
        g.unit_types_mut().insert(HAMG, ty("Archmage"));
    }
    host
}

fn run(host: &mut ScriptHost) {
    host.start("main", &[]).expect("main starts");
    for _ in 0..6 {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm().global(name).and_then(|v| v.as_integer()).unwrap_or_else(|| panic!("{name} is not an integer"))
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm().global(name).and_then(|v| v.as_boolean()).unwrap_or_else(|| panic!("{name} is not a boolean"))
}

fn string(host: &ScriptHost, name: &str) -> String {
    let id = match host.vm().global(name) {
        Some(jass::vm::Value::String(id)) => id,
        other => panic!("{name} is not a string: {other:?}"),
    };
    host.vm().strings().get(id).to_string()
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables"))
}

const SELECT_SCRIPT: &str = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(64), I2R(0), I2R(0))
    set g = CreateGroup()
    call SelectUnit(u2, true)
    call SelectUnit(u1, true)
    call SelectUnit(u1, true)
    call GroupEnumUnitsSelected(g, Player(0), null)
    set i1 = 0
    if FirstOfGroup(g) == u2 then
        set i1 = 1
    endif
    call GroupRemoveUnit(g, u2)
    if FirstOfGroup(g) == u1 then
        set i2 = 1
    endif
    call SelectUnit(u2, false)
    call GroupEnumUnitsSelected(g, Player(0), null)
    set b1 = IsUnitInGroup(u2, g)
    set b2 = IsUnitInGroup(u1, g)
    call ClearSelection()
    call GroupEnumUnitsSelected(g, Player(0), null)
    set i3 = 0
    if FirstOfGroup(g) == null then
        set i3 = 1
    endif
endfunction
"#;

#[test]
fn local_selection_keeps_order_and_enumerates_in_it() {
    let mut host = host_for(SELECT_SCRIPT);
    run(&mut host);
    assert_eq!(int(&host, "i1"), 1, "first selected unit is first in the group");
    assert_eq!(int(&host, "i2"), 1, "a duplicate select adds nothing");
    assert!(!boolean(&host, "b1"), "deselected unit is not enumerated");
    assert!(boolean(&host, "b1") == false && boolean(&host, "b2"), "the remaining unit is");
    assert_eq!(int(&host, "i3"), 1, "ClearSelection empties it");
    assert!(host.game().lane.selections.get(&0).is_none_or(Vec::is_empty));
}

#[test]
fn selection_is_limited_to_twelve_and_belongs_to_the_local_player() {
    let body = r#"
function main takes nothing returns nothing
    local integer n = 0
    loop
        exitwhen n == 14
        set u1 = CreateUnit(Player(2), 'hfoo', I2R(n), I2R(0), I2R(0))
        call SelectUnit(u1, true)
        set n = n + 1
    endloop
    set g = CreateGroup()
    call GroupEnumUnitsSelected(g, Player(3), null)
    call GroupEnumUnitsSelected(g, Player(2), null)
endfunction
"#;
    let mut host = host_for(body);
    host.game_mut().set_local_player(2);
    run(&mut host);
    let g = host.game();
    assert_eq!(g.lane.selections[&2].len(), 12);
    assert!(!g.lane.selections.contains_key(&3));
}

#[test]
fn replay_selection_commands_and_natives_share_one_set() {
    use sim::commands::{Command, CommandState, ObjectRef};
    let mut host = host_for(
        "function main takes nothing returns nothing\n set u1 = CreateUnit(Player(1), 'hfoo', I2R(0), I2R(0), I2R(0))\n set g = CreateGroup()\n endfunction\n",
    );
    run(&mut host);
    let k = host.game().unit_of_handle(host.vm().global("u1").and_then(|v| v.as_handle()).unwrap()).unwrap();
    let pair = sim::commands::model_pair(&host.game(), k);
    let mut cs = CommandState::new();
    let cell = host.game_handle();
    let out = cs.apply(&cell, &Command::Select { player: 1, add: true, units: vec![pair] });
    assert!(matches!(out, sim::commands::Outcome::Selected { now: 1, .. }));
    assert_eq!(host.game().lane.selections[&1], vec![k]);
    // A native edit of the same player shows in the next command.
    host.game_mut().set_local_player(1);
    host.game_mut().lane.selections.get_mut(&1).unwrap().clear();
    let out = cs.apply(&cell, &Command::Select { player: 1, add: true, units: vec![ObjectRef { id1: pair.id1, id2: 0 }] });
    assert!(matches!(out, sim::commands::Outcome::Selected { now: 1, .. }));
}

#[test]
fn sync_selections_sleeps_like_a_zero_wait() {
    let mut host = host_for(
        "function main takes nothing returns nothing\n set i1 = 1\n call SyncSelections()\n set i1 = 2\n endfunction\n",
    );
    host.start("main", &[]).expect("start");
    assert_eq!(int(&host, "i1"), 1, "the thread yields at the call");
    for _ in 0..4 {
        host.step();
    }
    assert_eq!(int(&host, "i1"), 2, "and resumes");
}

#[test]
fn remove_player_stores_the_result_once_and_clears_the_selection() {
    let body = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(1), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SelectUnit(u1, true)
    call RemovePlayer(Player(1), ConvertPlayerGameResult(1))
    call RemovePlayer(Player(1), ConvertPlayerGameResult(0))
    call RemovePlayer(Player(2), ConvertPlayerGameResult(0))
endfunction
"#;
    let mut host = host_for(body);
    {
        let mut g = host.game_mut();
        g.set_local_player(1);
        g.player_mut(1).unwrap().slot_state = 1;
        g.player_mut(2).unwrap().slot_state = 1;
    }
    run(&mut host);
    let g = host.game();
    assert_eq!(g.lane.result_of(1), 1, "the first result sticks");
    assert_eq!(g.player(1).slot_state, 2, "defeat leaves the game");
    assert!(g.lane.selections.get(&1).is_none_or(Vec::is_empty));
    assert_eq!(g.lane.result_of(2), 0);
    assert_eq!(g.player(2).slot_state, 1, "victory keeps the slot");
    assert!(g.lane.events.iter().any(|e| matches!(e, LaneEvent::PlayerRemoved { player: 1, result: 1, slot_state: 2 })));
}

#[test]
fn cripple_player_stores_the_force_mask_and_clears_it() {
    let body = r#"
function main takes nothing returns nothing
    set f = CreateForce()
    call ForceAddPlayer(f, Player(0))
    call ForceAddPlayer(f, Player(3))
    call CripplePlayer(Player(5), f, true)
    call CripplePlayer(Player(6), f, true)
    call CripplePlayer(Player(6), f, false)
endfunction
"#;
    let mut host = host_for(body);
    run(&mut host);
    let g = host.game();
    assert_eq!(g.lane.crippled[&5], 0b1001);
    assert_eq!(g.lane.crippled[&6], 0);
}

#[test]
fn set_player_units_owner_changes_nothing() {
    let mut host = host_for(
        "function main takes nothing returns nothing\n set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))\n call SetPlayerUnitsOwner(Player(0), 1)\n endfunction\n",
    );
    run(&mut host);
    let g = host.game();
    let k = g.unit_of_handle(host.vm().global("u1").and_then(|v| v.as_handle()).unwrap()).unwrap();
    assert_eq!(g.unit(k).owner, 0);
}

#[test]
fn hero_cache_sorts_by_level_then_xp_then_handle() {
    let body = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'Hamg', I2R(0), I2R(0), I2R(0))
    set u3 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
endfunction
function cache takes nothing returns nothing
    call CachePlayerHeroData(Player(0))
endfunction
"#;
    let mut host = host_for(body);
    run(&mut host);
    let key = |n: &str, h: &ScriptHost| h.game().unit_of_handle(h.vm().global(n).and_then(|v| v.as_handle()).unwrap()).unwrap();
    let (k1, k2, k3) = (key("u1", &host), key("u2", &host), key("u3", &host));
    {
        let mut g = host.game_mut();
        g.unit_mut(k1).hero = Some(HeroObj { level: 2, xp: 100, ..Default::default() });
        g.unit_mut(k2).hero = Some(HeroObj { level: 3, xp: 10, ..Default::default() });
        // A non-hero id (lowercase first letter) is not cached even with a hero record.
        g.unit_mut(k3).hero = Some(HeroObj { level: 9, xp: 9, ..Default::default() });
    }
    host.start("cache", &[]).expect("cache");
    let g = host.game();
    assert_eq!(
        g.lane.hero_cache[&0],
        vec![
            HeroCacheEntry { type_id: HAMG, level: 3, xp: 10 },
            HeroCacheEntry { type_id: HPAL, level: 2, xp: 100 },
        ]
    );
}

#[test]
fn player_score_reads_the_counters() {
    let body = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u3 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
endfunction
"#;
    let mut host = host_for(body);
    run(&mut host);
    let hero = host.game().unit_of_handle(host.vm().global("u3").and_then(|v| v.as_handle()).unwrap()).unwrap();
    {
        let mut g = host.game_mut();
        g.unit_mut(hero).hero = Some(HeroObj { level: 1, xp: 250, ..Default::default() });
        g.lane.add_stat(0, stat::GOLD_MINED, 1234);
        g.lane.add_stat(0, stat::LUMBER_MINED, 56);
        g.lane.add_stat(0, stat::GOLD_UPKEEP_LOST, 70);
        g.lane.add_stat(0, stat::GOLD_GIVEN, 9);
        g.lane.add_stat(0, stat::FOOD_MAX_USED, 12);
        g.lane.type_kills.insert((0, FOOTMAN), 4);
    }
    let score = |n: u32, h: &mut ScriptHost| -> i32 { player_score(&h.game(), 0, n) };
    assert_eq!(score(11, &mut host), 123); // gold mined / 10
    assert_eq!(score(12, &mut host), 7);
    assert_eq!(score(10, &mut host), 130);
    assert_eq!(score(14, &mut host), 9, "given is not divided");
    assert_eq!(score(6, &mut host), 12);
    assert_eq!(score(16, &mut host), 5);
    assert_eq!(score(23, &mut host), 128, "resource total = gold + lumber mined / 10");
    assert_eq!(score(22, &mut host), 2500, "hero total = 10 * xp");
    let units = score(0, &mut host);
    assert!(units >= 3, "units trained counts the created units, got {units}");
    assert_eq!(score(1, &mut host), 4);
    assert_eq!(score(99, &mut host), 0, "an unknown score is 0");
}

#[test]
fn timed_turn_without_tables_does_nothing_and_short_duration_is_instant() {
    let body = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitFacingTimed(u1, I2R(90), I2R(2))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitFacingTimed(u2, I2R(90), I2R(0))
endfunction
"#;
    let mut host = host_for(body);
    run(&mut host);
    let g = host.game();
    let get = |n: &str| g.unit_of_handle(host.vm().global(n).and_then(|v| v.as_handle()).unwrap()).unwrap();
    assert!(g.lane.turns.is_empty());
    assert_eq!(g.unit(get("u1")).facing, F32::ZERO);
    assert_eq!(g.unit(get("u2")).facing.to_bits(), 0x3FC9_0FDA, "duration 0 turns at once (90 deg in radians)");
}

#[test]
fn timed_turn_advances_by_a_fixed_rate_per_movement_tick() {
    let Some(t) = tables() else { return };
    let body = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitFacingTimed(u1, I2R(90), I2R(1))
endfunction
"#;
    let mut host = host_for(body);
    host.game_mut().set_tables(Some(t));
    run(&mut host);
    let k = host.game().unit_of_handle(host.vm().global("u1").and_then(|v| v.as_handle()).unwrap()).unwrap();
    let ts = host.game().lane.turns[&k];
    // 1 s at 0.03 s per tick is 33.3 ticks: the rate is (pi/2) / 33.3.
    let rate = f32::from_bits(ts.rate.to_bits());
    assert!((rate - 0.047124).abs() < 1e-4, "rate {rate}");
    let before = f32::from_bits(host.game().unit(k).facing.to_bits());
    host.game_mut().tick_movement();
    let after = f32::from_bits(host.game().unit(k).facing.to_bits());
    assert!((after - before - rate).abs() < 1e-5);
    for _ in 0..40 {
        host.game_mut().tick_movement();
    }
    assert_eq!(host.game().unit(k).facing, ts.target, "the turn ends exactly on the target");
    assert!(host.game().lane.turns.is_empty());
}

#[test]
fn share_vision_is_recorded_per_unit_and_player() {
    let mut host = host_for(
        "function main takes nothing returns nothing\n set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))\n call UnitShareVision(u1, Player(2), true)\n call UnitShareVision(u1, Player(3), true)\n call UnitShareVision(u1, Player(3), false)\n endfunction\n",
    );
    run(&mut host);
    let g = host.game();
    let k = g.unit_of_handle(host.vm().global("u1").and_then(|v| v.as_handle()).unwrap()).unwrap();
    assert_eq!(g.lane.shared_vision[&(k, 2)], true);
    assert_eq!(g.lane.shared_vision[&(k, 3)], false);
}

#[test]
fn wts_parser_reads_blocks_comments_and_multiline_text() {
    let wts = "\u{feff}STRING 1\r\n{\r\nMy Map\r\n}\r\n\r\nSTRING 2\r\n// comment\r\n{\r\nline one\r\nline two\r\n}\r\n\r\nSTRING 7\r\n{\r\n}\r\n";
    let m = parse_wts(wts);
    assert_eq!(m[&1], "My Map");
    assert_eq!(m[&2], "line one\nline two");
    assert_eq!(m[&7], "");
    assert_eq!(m.len(), 3);
}

#[test]
fn trigstr_is_substituted_by_string_length_and_case() {
    let body = r#"
function main takes nothing returns nothing
    set i1 = StringLength("TRIGSTR_002")
    set s1 = StringCase("TRIGSTR_002", true)
    set i2 = StringLength("TRIGSTR_099")
    set i3 = StringLength("plain")
    set i4 = StringLength("")
endfunction
"#;
    let mut host = host_for(body);
    host.game_mut().lane.map_strings = parse_wts("STRING 2\n{\nhello there\n}\n");
    run(&mut host);
    assert_eq!(int(&host, "i1"), 11);
    assert_eq!(string(&host, "s1"), "HELLO THERE");
    assert_eq!(int(&host, "i2"), 0, "an unknown number gives the empty string");
    assert_eq!(int(&host, "i3"), 5);
    assert_eq!(int(&host, "i4"), 0);
}

#[test]
fn localized_string_substitutes_then_looks_up_the_global_table() {
    let body = r#"
function main takes nothing returns nothing
    set s1 = GetLocalizedString("WESTRING_OK")
    set s2 = GetLocalizedString("TRIGSTR_001")
    set s3 = GetLocalizedString("unknown key")
    set b1 = GetLocalizedString("") == null
endfunction
"#;
    let mut host = host_for(body);
    {
        let mut g = host.game_mut();
        g.lane.localized.insert("westring_ok".into(), "Okay".into());
        g.lane.map_strings = parse_wts("STRING 1\n{\nWESTRING_OK\n}\n");
    }
    run(&mut host);
    assert_eq!(string(&host, "s1"), "Okay");
    assert_eq!(string(&host, "s2"), "Okay", "the substituted text is looked up again");
    assert_eq!(string(&host, "s3"), "unknown key", "a miss returns the input");
    assert!(boolean(&host, "b1"), "empty input gives the null string");
}

#[test]
fn object_names_and_ability_art_come_from_the_host_tables() {
    let body = r#"
function main takes nothing returns nothing
    set s1 = GetObjectName('hfoo')
    set s2 = GetObjectName('xxxx')
    set s3 = GetAbilityEffectById('AHhb', ConvertEffectType(1), 5)
    set b1 = GetAbilityEffectById('AHhb', ConvertEffectType(3), 0) == null
    set b2 = GetAbilitySoundById('AHhb', ConvertSoundType(1)) == "loop.wav"
endfunction
"#;
    let mut host = host_for(body);
    {
        let mut g = host.game_mut();
        g.lane.object_names.insert(FOOTMAN, "Footman".into());
        let ahhb = u32::from_be_bytes(*b"AHhb");
        g.lane.ability_art.insert((ahhb, 1), vec!["a.mdl".into(), "b.mdl".into()]);
        g.lane.ability_sound.insert((ahhb, 1), vec!["loop.wav".into()]);
    }
    run(&mut host);
    assert_eq!(string(&host, "s1"), "Footman");
    assert!(host.vm().global("s2").is_some_and(|v| matches!(v, jass::vm::Value::String(0))), "missing name is null");
    assert_eq!(string(&host, "s3"), "b.mdl", "index is clamped to the last entry");
    assert!(boolean(&host, "b1"), "empty list gives null");
    assert!(boolean(&host, "b2"));
}

#[test]
fn presentation_natives_are_logged_for_live_units_only() {
    let body = r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitAnimation(u1, "attack")
    call SetUnitAnimationByIndex(u1, 3)
    call SetUnitAnimationWithRarity(u1, "stand", ConvertRarityControl(1))
    call SetUnitBlendTime(u1, I2R(2))
    call SetUnitColor(u1, ConvertPlayerColor(4))
    call UnitAddIndicator(u1, 255, 0, 0, 128)
    call SetUnitAnimation(u2, "attack")
endfunction
"#;
    let mut host = host_for(body);
    run(&mut host);
    let events = host.game_mut().presentation_mut().log.drain();
    let names: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            PresentationEvent::Command { name, .. } => Some(*name),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        ["SetUnitAnimation", "SetUnitAnimationByIndex", "SetUnitAnimation", "SetUnitBlendTime", "SetUnitColor", "UnitAddIndicator"]
    );
}

#[test]
fn level_save_and_load_requests_follow_the_mode_gates() {
    let body = r#"
function main takes nothing returns nothing
    call ChangeLevel("Campaign\\Next.w3m", true)
    call LoadGame("slot1", false)
    call SaveGame("slot1")
    call SaveGame("slot2")
    call SaveGame("bad..name")
endfunction
"#;
    let mut replay = host_for(body);
    run(&mut replay);
    assert_eq!(
        replay.game().lane.requests,
        vec![HostRequest::ChangeLevel { name: "Campaign\\Next.w3m".into(), score_screen: true }],
        "a replay or network game ignores save and load"
    );
    let mut single = host_for(body);
    single.game_mut().lane.single_player = true;
    run(&mut single);
    assert_eq!(
        single.game().lane.requests,
        vec![
            HostRequest::ChangeLevel { name: "Campaign\\Next.w3m".into(), score_screen: true },
            HostRequest::LoadGame { name: "slot1".into(), score_screen: false },
            HostRequest::SaveGame { name: "slot1".into() },
        ],
        "one save at a time; names with .. are refused"
    );
}
