//! AI host tests: one JASS VM per computer player, stepped after the main VM; the AI natives of table `ai`; the
//! main-VM AI natives (`StartMeleeAI`, `CommandAI`, `PauseCompAI`). The AI scripts here are small synthetic sources.

use std::cell::RefCell;
use std::rc::Rc;

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::ai::{self, AiArg, AiEvent, AiHost, AiLibrary, SharedAi, StartKind};
use sim::natives::ai::{register_main, AiNative, AI_NATIVES};
use sim::natives::game::{Game, UnitType};
use sim::ScriptHost;
use softfloat::F32;

/// Declarations shared by the AI sources: the types and the AI natives the scripts call.
const AI_COMMON: &str = r#"
type player extends handle
type unit extends handle
native Player takes integer number returns player
native Sleep takes real t returns nothing
native DebugFI takes string s, integer i returns nothing
native GetAiPlayer takes nothing returns integer
native GetUnitCount takes integer t returns integer
native GetUnitCountDone takes integer t returns integer
native GetPlayerUnitTypeCount takes player p, integer t returns integer
native GetGoldOwned takes nothing returns integer
native GetUnitGoldCost takes integer t returns integer
native UnitAlive takes unit u returns boolean
native CommandsWaiting takes nothing returns integer
native GetLastCommand takes nothing returns integer
native GetLastData takes nothing returns integer
native PopLastCommand takes nothing returns nothing
native GetAllianceTarget takes nothing returns unit
native SetAllianceTarget takes unit u returns nothing
native SetHeroesFlee takes boolean b returns nothing
native SetReplacementCount takes integer n returns nothing
"#;

/// Declarations and natives of the main script.
const MAIN_PRELUDE: &str = r#"
type player extends handle
type unit extends handle
native Player takes integer number returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native KillUnit takes unit u returns nothing
native SetPlayerState takes player p, integer s, integer v returns nothing
native StartMeleeAI takes player p, string s returns nothing
native StartCampaignAI takes player p, string s returns nothing
native CommandAI takes player p, integer c, integer d returns nothing
native PauseCompAI takes player p, boolean b returns nothing
"#;

/// The raw code of the unit type the tests create.
const HFOO: u32 = u32::from_be_bytes(*b"hfoo");

/// One AI script: it counts itself, logs every tick and sleeps 0.1 s between ticks.
const TICK_AI: &str = r#"
globals
    integer ticks = 0
endglobals
function main takes nothing returns nothing
    loop
        set ticks = ticks + 1
        call DebugFI("tick", GetAiPlayer())
        call Sleep(0.1)
    endloop
endfunction
"#;

struct Rig {
    host: ScriptHost,
    ai: AiHost,
    shared: SharedAi,
    game: Rc<RefCell<Game>>,
}

fn library() -> AiLibrary {
    let mut lib = AiLibrary::new(AI_COMMON);
    lib.add_script("tick.ai", TICK_AI);
    lib
}

/// Builds the main VM (with the AI natives), the AI host over the same game, and the shared AI state.
fn rig(main_src: &str, lib: AiLibrary) -> Rig {
    let game = Rc::new(RefCell::new(Game::new()));
    let shared = ai::new_shared();
    let mut reg = NativeRegistry::with_builtins();
    register_main(&mut reg, &game, &shared);
    let full = format!("{MAIN_PRELUDE}\n{main_src}");
    let module = compile(&[jass::parse(&full).expect("parse main")]).expect("compile main");
    let mut host = ScriptHost::with_game(module, reg, None, Rc::clone(&game)).expect("main vm");
    host.start("main", &[]).expect("start main");
    let ai = AiHost::new(Rc::clone(&game), Rc::clone(&shared), lib, None);
    Rig {
        host,
        ai,
        shared,
        game,
    }
}

/// One 0.1 s tick: the main VM, then the AI VMs in player-index order.
fn tick(rig: &mut Rig) {
    rig.host.step();
    let now = F32::from_bits(rig.host.clock().now().to_bits());
    rig.ai.step(now);
}

fn ticks(rig: &mut Rig, n: usize) {
    for _ in 0..n {
        tick(rig);
    }
}

fn ai_int(rig: &Rig, player: u32, name: &str) -> i32 {
    rig.ai
        .vm(player)
        .unwrap_or_else(|| panic!("no AI VM for player {player}"))
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} of player {player} is not an integer"))
}

/// The player indices of the `DebugFI("tick", ...)` log entries, in log order.
fn tick_players(shared: &SharedAi) -> Vec<u32> {
    shared
        .borrow()
        .commands
        .iter()
        .filter(|c| c.name == "DebugFI")
        .map(|c| match c.args.get(1) {
            Some(AiArg::Int(p)) => *p as u32,
            other => panic!("unexpected argument {other:?}"),
        })
        .collect()
}

/// Two computer players run the same tick script: their ticks alternate in player-index order every 0.1 s.
#[test]
fn two_ai_players_interleave_in_index_order() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(1), "tick.ai")
    call StartMeleeAI(Player(0), "tick.ai")
endfunction
"#;
    let mut r = rig(main, library());
    ticks(&mut r, 5);
    assert_eq!(r.ai.players(), vec![0, 1]);
    assert_eq!(tick_players(&r.shared), vec![0, 1, 0, 1, 0, 1, 0, 1, 0, 1]);
    assert_eq!(ai_int(&r, 0, "ticks"), 5);
    assert_eq!(ai_int(&r, 1, "ticks"), 5);
}

/// The same inputs give the same command log and the same globals, run twice.
#[test]
fn the_ai_run_is_deterministic() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "tick.ai")
    call StartMeleeAI(Player(1), "tick.ai")
    call CommandAI(Player(1), 3, 9)
endfunction
"#;
    let run = || {
        let mut r = rig(main, library());
        ticks(&mut r, 20);
        let log = r.shared.borrow().commands.clone();
        (log, ai_int(&r, 0, "ticks"), ai_int(&r, 1, "ticks"))
    };
    assert_eq!(run(), run());
}

/// `CommandAI` from the main script reaches the AI's queue, and the AI reads it in order.
#[test]
fn command_ai_fills_the_queue_the_ai_reads() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "queue.ai")
    call CommandAI(Player(0), 7, 42)
    call CommandAI(Player(0), 8, 43)
    call CommandAI(Player(1), 9, 44)
endfunction
"#;
    let queue_ai = r#"
globals
    integer waiting = -1
    integer first = -1
    integer firstData = -1
    integer second = -1
endglobals
function main takes nothing returns nothing
    set waiting = CommandsWaiting()
    set first = GetLastCommand()
    set firstData = GetLastData()
    call PopLastCommand()
    set second = GetLastCommand()
    call PopLastCommand()
endfunction
"#;
    let mut lib = library();
    lib.add_script("queue.ai", queue_ai);
    let mut r = rig(main, lib);
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "waiting"), 2);
    assert_eq!(ai_int(&r, 0, "first"), 7);
    assert_eq!(ai_int(&r, 0, "firstData"), 42);
    assert_eq!(ai_int(&r, 0, "second"), 8);
    // Player 1 has no AI: its command was dropped, and it has no VM.
    assert!(r.ai.vm(1).is_none());
    assert!(r
        .shared
        .borrow()
        .control(1)
        .is_none_or(|c| c.queue.is_empty()));
}

/// A full queue (12) drops further `CommandAI` calls, and the drops are counted.
#[test]
fn command_queue_is_limited_to_twelve() {
    let mut main = String::from("function main takes nothing returns nothing\n");
    main.push_str("    call StartMeleeAI(Player(0), \"tick.ai\")\n");
    for i in 0..14 {
        main.push_str(&format!("    call CommandAI(Player(0), {i}, {i})\n"));
    }
    main.push_str("endfunction\n");
    let mut r = rig(&main, library());
    tick(&mut r);
    let sh = r.shared.borrow();
    let c = sh.control(0).expect("player 0 control");
    assert_eq!(c.queue.len(), ai::COMMAND_QUEUE_LIMIT);
    assert_eq!(c.dropped_commands, 2);
    assert_eq!(c.queue.front().copied(), Some((0, 0)));
    assert_eq!(c.queue.back().copied(), Some((11, 11)));
}

/// Unit counts and gold come from the game, for the AI's own player.
#[test]
fn unit_and_gold_queries_read_the_game() {
    let main = r#"
function main takes nothing returns nothing
    call CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    call CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    call CreateUnit(Player(1), 'hfoo', 0.0, 0.0, 0.0)
    call SetPlayerState(Player(0), 1, 500)
    call StartMeleeAI(Player(0), "count.ai")
endfunction
"#;
    let count_ai = r#"
globals
    integer own = -1
    integer done = -1
    integer other = -1
    integer gold = -1
    integer goldCost = -1
endglobals
function main takes nothing returns nothing
    set own = GetUnitCount('hfoo')
    set done = GetUnitCountDone('hfoo')
    set other = GetPlayerUnitTypeCount(Player(1), 'hfoo')
    set gold = GetGoldOwned()
    set goldCost = GetUnitGoldCost('hfoo')
endfunction
"#;
    let mut lib = library();
    lib.add_script("count.ai", count_ai);
    let mut r = rig(main, lib);
    r.game.borrow_mut().unit_types_mut().insert(
        HFOO,
        UnitType {
            name: "hfoo".to_string(),
            max_life: F32::from_bits(0x42C8_0000),
            ..Default::default()
        },
    );
    // The unit type must exist before the main VM creates units: insert it, then run the first tick.
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "own"), 2);
    assert_eq!(ai_int(&r, 0, "done"), 2);
    assert_eq!(ai_int(&r, 0, "other"), 1);
    assert_eq!(ai_int(&r, 0, "gold"), 500);
    // Not implemented: the call returns 0 and is counted.
    assert_eq!(ai_int(&r, 0, "goldCost"), 0);
    assert_eq!(
        r.shared
            .borrow()
            .unimplemented
            .get("GetUnitGoldCost")
            .copied(),
        Some(1)
    );
}

/// A dead unit is no longer counted or alive for the AI.
#[test]
fn dead_units_are_not_counted_or_alive() {
    let main = r#"
globals
    unit victim = null
endglobals
function main takes nothing returns nothing
    set victim = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    call StartMeleeAI(Player(0), "dead.ai")
    call KillUnit(victim)
endfunction
"#;
    let dead_ai = r#"
globals
    integer count = -1
    boolean alive = true
endglobals
function main takes nothing returns nothing
    set count = GetUnitCount('hfoo')
endfunction
"#;
    let mut lib = library();
    lib.add_script("dead.ai", dead_ai);
    let mut r = rig(main, lib);
    r.game.borrow_mut().unit_types_mut().insert(
        HFOO,
        UnitType {
            name: "hfoo".to_string(),
            max_life: F32::from_bits(0x42C8_0000),
            ..Default::default()
        },
    );
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "count"), 0);
}

/// `PauseCompAI` skips the AI's step while its count is above zero; the other AI keeps running.
#[test]
fn pause_skips_only_the_paused_ai() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "tick.ai")
    call StartMeleeAI(Player(1), "tick.ai")
    call PauseCompAI(Player(0), true)
endfunction
"#;
    let mut r = rig(main, library());
    ticks(&mut r, 4);
    assert_eq!(tick_players(&r.shared), vec![1, 1, 1, 1]);
    assert_eq!(r.shared.borrow().control(0).map(|c| c.pause), Some(1));
}

/// A start for a player that already runs an AI is refused and logged; a missing script is logged as a build failure.
#[test]
fn refused_and_failed_starts_are_logged() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "tick.ai")
    call StartMeleeAI(Player(0), "tick.ai")
    call StartCampaignAI(Player(2), "missing.ai")
endfunction
"#;
    let mut r = rig(main, library());
    tick(&mut r);
    let events = r.shared.borrow().events.clone();
    assert!(events.iter().any(|e| matches!(
        e,
        AiEvent::Refused {
            player: 0,
            reason: "the player already runs an AI",
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        AiEvent::BuildFailed { player: 2, script, .. } if script == "missing.ai"
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        AiEvent::Started {
            player: 0,
            kind: StartKind::Melee,
            ..
        }
    )));
}

/// A campaign AI with the same script runs as its own VM with its own globals.
#[test]
fn each_ai_vm_has_its_own_globals() {
    let main = r#"
function main takes nothing returns nothing
    call StartCampaignAI(Player(3), "tick.ai")
    call StartMeleeAI(Player(5), "tick.ai")
endfunction
"#;
    let mut r = rig(main, library());
    ticks(&mut r, 3);
    assert_eq!(r.ai.players(), vec![3, 5]);
    assert_eq!(ai_int(&r, 3, "ticks"), 3);
    assert_eq!(ai_int(&r, 5, "ticks"), 3);
    assert_eq!(ai_int(&r, 5, "ticks"), ai_int(&r, 3, "ticks"));
    assert!(r.ai.vm(3).and_then(|vm| vm.global("ticks")).is_some());
    assert_eq!(
        r.shared.borrow().control(3).and_then(|c| c.kind),
        Some(StartKind::Campaign)
    );
}

/// The setter natives store their values; the getters return them.
#[test]
fn setters_store_values_for_the_ai() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "set.ai")
endfunction
"#;
    let set_ai = r#"
globals
    integer target = 0
endglobals
function main takes nothing returns nothing
    call SetAllianceTarget(GetAllianceTarget())
    call SetHeroesFlee(true)
    call SetReplacementCount(4)
    set target = GetAiPlayer()
endfunction
"#;
    let mut lib = library();
    lib.add_script("set.ai", set_ai);
    let mut r = rig(main, lib);
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "target"), 0);
    let sh = r.shared.borrow();
    let c = sh.control(0).expect("control");
    assert_eq!(c.settings.get("SetHeroesFlee"), Some(&AiArg::Bool(true)));
    assert_eq!(c.settings.get("SetReplacementCount"), Some(&AiArg::Int(4)));
    // The alliance target is stored and logged; the flag and the count are stored only.
    assert!(sh.commands.iter().any(|c| c.name == "SetAllianceTarget"));
    assert!(!sh.commands.iter().any(|c| c.name == "SetHeroesFlee"));
}

/// Builds the declarations of one table row from its signature (for the bind test).
fn declaration(row: &AiNative) -> String {
    fn types(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            let ty = match c {
                'I' => "integer".to_string(),
                'R' => "real".to_string(),
                'S' => "string".to_string(),
                'B' => "boolean".to_string(),
                'C' => "code".to_string(),
                'H' => chars.by_ref().take_while(|&c| c != ';').collect(),
                'V' => continue,
                other => panic!("unknown signature character {other}"),
            };
            out.push(ty);
        }
        out
    }
    let body = row.sig.strip_prefix('(').expect("signature starts with (");
    let (params, ret) = body.split_once(')').expect("signature has )");
    let params = types(params);
    let ret = types(ret);
    let takes = if params.is_empty() {
        "nothing".to_string()
    } else {
        params
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{t} p{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let returns = ret
        .first()
        .cloned()
        .unwrap_or_else(|| "nothing".to_string());
    format!("native {} takes {takes} returns {returns}", row.name)
}

/// Every one of the 124 AI-table natives is bound in an AI VM (none is left unbound).
#[test]
fn all_ai_table_natives_are_bound_in_an_ai_vm() {
    let mut decls = String::from("type player extends handle\ntype unit extends handle\n");
    for row in AI_NATIVES {
        decls.push_str(&declaration(row));
        decls.push('\n');
    }
    let mut lib = AiLibrary::new(&decls);
    lib.add_script(
        "all.ai",
        "function main takes nothing returns nothing\nendfunction\n",
    );
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "all.ai")
endfunction
"#;
    let mut r = rig(main, lib);
    tick(&mut r);
    let vm = r.ai.vm(0).expect("AI VM built");
    assert!(
        vm.unbound_natives().is_empty(),
        "unbound: {:?}",
        vm.unbound_natives()
    );
    assert_eq!(AI_NATIVES.len(), 124);
}

/// The table's level labels and the not-implemented list are consistent: a not-implemented row always has a reason.
#[test]
fn not_implemented_rows_say_why() {
    let missing: Vec<&AiNative> = AI_NATIVES.iter().filter(|r| !r.implemented).collect();
    assert!(!missing.is_empty());
    for row in missing {
        assert!(row.note.starts_with("NOT IMPLEMENTED: "), "{}", row.name);
    }
}
