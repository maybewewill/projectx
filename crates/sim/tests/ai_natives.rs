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
native GetMinesOwned takes nothing returns integer
native TownWithMine takes nothing returns integer
native TownHasMine takes integer slot returns boolean
native TownHasHall takes integer slot returns boolean
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
native SetCaptainChanges takes boolean b returns nothing
native GroupTimedLife takes boolean b returns nothing
native SetMeleeAI takes nothing returns nothing
native SetCampaignAI takes nothing returns nothing
native DisablePathing takes nothing returns nothing
native SetAmphibious takes nothing returns nothing
native CaptainIsFull takes nothing returns boolean
native CaptainRetreating takes nothing returns boolean
native SuicidePlayer takes player p, boolean b returns boolean
native SetUpgrade takes integer t returns boolean
native ConvertUnits takes integer a, integer b returns boolean
native SetProduce takes integer n, integer t, integer x returns boolean
native MeleeDifficulty takes nothing returns integer
native GetEnemyPower takes nothing returns integer
native GetUnitWoodCost takes integer t returns integer
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
    rig_with(main_src, lib, |_| {})
}

/// Like [`rig`], but `setup` runs on the game before the main script starts. The main script starts
/// at once (lockstep.md section 12.1), so state it reads at its start must be set here.
fn rig_with(main_src: &str, lib: AiLibrary, setup: impl FnOnce(&Rc<RefCell<Game>>)) -> Rig {
    let game = Rc::new(RefCell::new(Game::new()));
    setup(&game);
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

/// Runs batches until every listed AI VM has run `n` times (its `ticks` global). A computer player
/// sleeps 0.1 s and wakes at the first batch boundary whose clock reaches its wake time, so the number of
/// batches per run is 1 or 2 (the clock is a sum of 0.005 s ticks, and it drifts below 0.1 per batch).
fn run_until(rig: &mut Rig, players: &[u32], n: i32) {
    for _ in 0..200 {
        let runs = |p: u32| {
            rig.ai
                .vm(p)
                .and_then(|vm| vm.global("ticks"))
                .and_then(|v| v.as_integer())
                .unwrap_or(0)
        };
        if players.iter().all(|&p| runs(p) >= n) {
            return;
        }
        tick(rig);
    }
    panic!("AI runs did not reach {n}");
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

/// Two computer players run the same tick script: their ticks alternate in player-index order.
#[test]
fn two_ai_players_interleave_in_index_order() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(1), "tick.ai")
    call StartMeleeAI(Player(0), "tick.ai")
endfunction
"#;
    let mut r = rig(main, library());
    run_until(&mut r, &[0, 1], 5);
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

/// `CommandAI` from the main script reaches the AI's command stack. The AI reads the newest pair first (CONFIRMED:
/// the stack is read at its count), and an empty stack reads -1.
#[test]
fn command_ai_fills_the_stack_the_ai_reads() {
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
    integer third = 0
endglobals
function main takes nothing returns nothing
    set waiting = CommandsWaiting()
    set first = GetLastCommand()
    set firstData = GetLastData()
    call PopLastCommand()
    set second = GetLastCommand()
    call PopLastCommand()
    set third = GetLastCommand()
endfunction
"#;
    let mut lib = library();
    lib.add_script("queue.ai", queue_ai);
    let mut r = rig(main, lib);
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "waiting"), 2);
    assert_eq!(ai_int(&r, 0, "first"), 8);
    assert_eq!(ai_int(&r, 0, "firstData"), 43);
    assert_eq!(ai_int(&r, 0, "second"), 7);
    assert_eq!(ai_int(&r, 0, "third"), -1);
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
    // The stack keeps the oldest twelve: the first pushed is at the bottom, the last one on top.
    assert_eq!(c.queue.first().copied(), Some((0, 0)));
    assert_eq!(c.queue.last().copied(), Some((11, 11)));
}

/// Unit counts come from the game, for the AI's own player. Unit costs come from the type registry. The gold query is
/// not implemented: its body sums the AI's town slots, not the player's gold, so it returns 0 and is counted.
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
    integer woodCost = -1
    integer unknownCost = -1
endglobals
function main takes nothing returns nothing
    set own = GetUnitCount('hfoo')
    set done = GetUnitCountDone('hfoo')
    set other = GetPlayerUnitTypeCount(Player(1), 'hfoo')
    set gold = GetGoldOwned()
    set goldCost = GetUnitGoldCost('hfoo')
    set woodCost = GetUnitWoodCost('hfoo')
    set unknownCost = GetUnitGoldCost('hbar')
endfunction
"#;
    let mut lib = library();
    lib.add_script("count.ai", count_ai);
    // The unit type must exist before the main script starts (it starts at once, in `rig_with`).
    let mut r = rig_with(main, lib, |game| {
        game.borrow_mut().unit_types_mut().insert(
            HFOO,
            UnitType {
                name: "hfoo".to_string(),
                max_life: F32::from_bits(0x42C8_0000),
                gold_cost: 150,
                wood_cost: 50,
                ..Default::default()
            },
        );
    });
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "own"), 2);
    assert_eq!(ai_int(&r, 0, "done"), 2);
    assert_eq!(ai_int(&r, 0, "other"), 1);
    assert_eq!(ai_int(&r, 0, "goldCost"), 150);
    assert_eq!(ai_int(&r, 0, "woodCost"), 50);
    // An id that is not in the registry costs 0.
    assert_eq!(ai_int(&r, 0, "unknownCost"), 0);
    // GetGoldOwned sums the gold of the mines of the AI's town slots (not the player's gold): no town, no gold.
    assert_eq!(ai_int(&r, 0, "gold"), 0);
    assert_eq!(r.shared.borrow().unimplemented.get("GetGoldOwned"), None);
}

const HTOW: u32 = u32::from_be_bytes(*b"htow");
const NGOL: u32 = u32::from_be_bytes(*b"ngol");
const UGOL: u32 = u32::from_be_bytes(*b"ugol");
/// `UNIT_TYPE_TOWNHALL` in the classification word.
const CLASS_TOWNHALL: u32 = 0x40;

const TOWN_AI: &str = r#"
globals
    integer mines = -9
    integer gold = -9
    integer town = -9
    integer hall0 = -9
    integer hall1 = -9
    integer hall4 = -9
    integer mine0 = -9
    integer mine1 = -9
endglobals
function b2i takes boolean b returns integer
    if b then
        return 1
    endif
    return 0
endfunction
function main takes nothing returns nothing
    set mines = GetMinesOwned()
    set gold = GetGoldOwned()
    set town = TownWithMine()
    set hall0 = b2i(TownHasHall(0))
    set hall1 = b2i(TownHasHall(1))
    set hall4 = b2i(TownHasHall(4))
    set mine0 = b2i(TownHasMine(0))
    set mine1 = b2i(TownHasMine(1))
endfunction
"#;

fn town_rig(race: u32, extra_main: &str) -> Rig {
    let main = format!(
        r#"
native SetResourceAmount takes unit u, integer a returns nothing
function main takes nothing returns nothing
    local unit m = CreateUnit(Player(15), 'ngol', 0.0, 0.0, 0.0)
    call SetResourceAmount(m, 12500)
    call CreateUnit(Player(0), 'htow', 300.0, 0.0, 0.0)
    {extra_main}
    call StartMeleeAI(Player(0), "town.ai")
endfunction
"#
    );
    let mut lib = library();
    lib.add_script("town.ai", TOWN_AI);
    rig_with(&main, lib, |game| {
        let mut gm = game.borrow_mut();
        gm.player_mut(0).unwrap().race = race;
        for (id, name, class) in [(HTOW, "htow", CLASS_TOWNHALL), (NGOL, "ngol", 0), (UGOL, "ugol", 0)] {
            gm.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: F32::from_bits(0x42C8_0000),
                    classification: class,
                    ..Default::default()
                },
            );
        }
    })
}

/// A hall and a gold mine near each other form one town of the AI: the model of `ai_towns.rs` (the five natives'
/// bodies are Unicorn-checked in `ai_towns_vectors.rs`; the registration of units is the model's).
#[test]
fn town_natives_see_the_hall_and_the_mine() {
    let mut r = town_rig(1, "");
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "mines"), 1);
    assert_eq!(ai_int(&r, 0, "gold"), 12500);
    assert_eq!(ai_int(&r, 0, "town"), 0);
    assert_eq!(ai_int(&r, 0, "hall0"), 1);
    assert_eq!(ai_int(&r, 0, "hall1"), 0, "an empty slot has no hall");
    assert_eq!(ai_int(&r, 0, "hall4"), 0, "slot 4 is out of range");
    assert_eq!(ai_int(&r, 0, "mine0"), 1);
    assert_eq!(ai_int(&r, 0, "mine1"), 0);
    assert!(r.shared.borrow().unimplemented.is_empty());
}

/// Without a hall a human or orc AI has no town with a mine; an undead AI needs only the mine; a night elf needs an
/// entangled mine (`egol`), which a plain gold mine is not.
#[test]
fn town_with_mine_depends_on_the_race() {
    // A mine alone, no AI hall: the hall natives say no, TownWithMine depends on the race.
    for (race, want) in [(1u32, -1), (2, -1), (3, -1), (4, -1)] {
        let main = r#"
native SetResourceAmount takes unit u, integer a returns nothing
function main takes nothing returns nothing
    local unit m = CreateUnit(Player(15), 'ngol', 0.0, 0.0, 0.0)
    call SetResourceAmount(m, 100)
    call StartMeleeAI(Player(0), "town.ai")
endfunction
"#;
        let mut lib = library();
        lib.add_script("town.ai", TOWN_AI);
        let mut r = rig_with(main, lib, |game| {
            let mut gm = game.borrow_mut();
            gm.player_mut(0).unwrap().race = race;
            gm.unit_types_mut().insert(
                NGOL,
                UnitType {
                    name: "ngol".to_string(),
                    ..Default::default()
                },
            );
        });
        tick(&mut r);
        // No hall, so the AI has no town slot at all: every race answers -1 and 0 mines.
        assert_eq!(ai_int(&r, 0, "town"), want, "race {race}");
        assert_eq!(ai_int(&r, 0, "mines"), 0);
    }
    // With the hall, a night elf still needs the entangled mine.
    let mut r = town_rig(4, "");
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "town"), -1);
    let mut r = town_rig(3, "");
    tick(&mut r);
    assert_eq!(ai_int(&r, 0, "town"), 0);
}

/// The captain and threat records start idle: the captain group is empty, nothing is in combat or threatened.
#[test]
fn captain_and_threat_records_start_idle() {
    let main = "function main takes nothing returns nothing\n    call StartMeleeAI(Player(0), \"idle.ai\")\nendfunction\n";
    let idle = r#"
native CaptainIsEmpty takes nothing returns boolean
native CaptainGroupSize takes nothing returns integer
native CaptainInCombat takes boolean attack returns boolean
native TownThreatened takes nothing returns boolean
globals
    integer empty = -9
    integer size = -9
    integer atk = -9
    integer def = -9
    integer thr = -9
endglobals
function main takes nothing returns nothing
    if CaptainIsEmpty() then
        set empty = 1
    else
        set empty = 0
    endif
    set size = CaptainGroupSize()
    if CaptainInCombat(true) then
        set atk = 1
    else
        set atk = 0
    endif
    if CaptainInCombat(false) then
        set def = 1
    else
        set def = 0
    endif
    if TownThreatened() then
        set thr = 1
    else
        set thr = 0
    endif
endfunction
"#;
    let mut lib = library();
    lib.add_script("idle.ai", idle);
    let mut r = rig(main, lib);
    tick(&mut r);
    let got = ["empty", "size", "atk", "def", "thr"].map(|n| ai_int(&r, 0, n));
    assert_eq!(got, [1, 0, 0, 0, 0]);
}

/// The boolean natives whose bodies fix their value: `SetUpgrade` and `ConvertUnits` return false, `SetProduce` with
/// a count of 0 returns true and with a positive count false, and `GetEnemyPower` is 0.
#[test]
fn fixed_return_values_follow_the_bodies() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "fixed.ai")
endfunction
"#;
    let fixed_ai = r#"
globals
    boolean upgrade = true
    boolean convert = true
    boolean produce0 = false
    boolean produce2 = true
    integer power = -1
endglobals
function main takes nothing returns nothing
    set upgrade = SetUpgrade(3)
    set convert = ConvertUnits(1, 2)
    set produce0 = SetProduce(0, 'hfoo', 0)
    set produce2 = SetProduce(2, 'hfoo', 0)
    set power = GetEnemyPower()
endfunction
"#;
    let mut lib = library();
    lib.add_script("fixed.ai", fixed_ai);
    let mut r = rig(main, lib);
    tick(&mut r);
    let vm = r.ai.vm(0).expect("AI VM");
    assert_eq!(
        vm.global("upgrade").and_then(|v| v.as_boolean()),
        Some(false)
    );
    assert_eq!(
        vm.global("convert").and_then(|v| v.as_boolean()),
        Some(false)
    );
    assert_eq!(
        vm.global("produce0").and_then(|v| v.as_boolean()),
        Some(true)
    );
    assert_eq!(
        vm.global("produce2").and_then(|v| v.as_boolean()),
        Some(false)
    );
    assert_eq!(ai_int(&r, 0, "power"), 0);
    // The requests are recorded: they have no other effect in the model.
    let sh = r.shared.borrow();
    assert!(sh.commands.iter().any(|c| c.name == "SetUpgrade"));
    assert!(sh.commands.iter().any(|c| c.name == "SetProduce"));
    assert!(sh.unimplemented.is_empty());
}

/// The flag setters change their bits of the AI flag word, and `SetMeleeAI` / `SetCampaignAI` move the melee bit.
#[test]
fn flag_setters_change_their_bits() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "flags.ai")
endfunction
"#;
    let flags_ai = r#"
function main takes nothing returns nothing
    call SetCaptainChanges(true)
    call GroupTimedLife(true)
    call SetHeroesFlee(true)
    call SetHeroesFlee(false)
    call SetCampaignAI()
    call SetMeleeAI()
endfunction
"#;
    let mut lib = library();
    lib.add_script("flags.ai", flags_ai);
    let mut r = rig(main, lib);
    tick(&mut r);
    let sh = r.shared.borrow();
    let c = sh.control(0).expect("control");
    // Captain changes 0x80, group timed life 0x100, melee 0x4000 (set last); heroes flee 0x8 was cleared again.
    assert_eq!(c.flags, 0x80 | 0x100 | 0x4000);
}

/// `MeleeDifficulty` reads the difficulty bits of the flag word: 1 when `0x8000` is set (even with `0x10000`), 3 for
/// `0x10000` alone, 2 without them.
#[test]
fn melee_difficulty_follows_the_difficulty_bits() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "diff.ai")
endfunction
"#;
    let diff_ai = r#"
globals
    integer level = -1
endglobals
function main takes nothing returns nothing
    set level = MeleeDifficulty()
endfunction
"#;
    for (bits, expected) in [(0, 2), (0x8000, 1), (0x1_8000, 1), (0x1_0000, 3)] {
        let mut lib = library();
        lib.add_script("diff.ai", diff_ai);
        let mut r = rig(main, lib);
        r.shared.borrow_mut().control_mut(0).flags = bits;
        tick(&mut r);
        assert_eq!(ai_int(&r, 0, "level"), expected, "bits {bits:#x}");
    }
}

/// The captain record bits: `DisablePathing` and `SetAmphibious` set theirs. `CaptainIsFull` and `CaptainRetreating`
/// read the full and retreating bits, which the model's natives never set. `SuicidePlayer` with `true` needs the full
/// bit (so it is false here), and with `false` it passes the gate (true).
#[test]
fn captain_record_bits_and_the_suicide_gate() {
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "captain.ai")
endfunction
"#;
    let captain_ai = r#"
globals
    boolean full = true
    boolean retreat = true
    boolean plain = false
    boolean gated = true
endglobals
function main takes nothing returns nothing
    call DisablePathing()
    call SetAmphibious()
    set full = CaptainIsFull()
    set retreat = CaptainRetreating()
    set plain = SuicidePlayer(Player(1), false)
    set gated = SuicidePlayer(Player(1), true)
endfunction
"#;
    let mut lib = library();
    lib.add_script("captain.ai", captain_ai);
    let mut r = rig(main, lib);
    tick(&mut r);
    let vm = r.ai.vm(0).expect("AI VM");
    assert_eq!(vm.global("full").and_then(|v| v.as_boolean()), Some(false));
    assert_eq!(
        vm.global("retreat").and_then(|v| v.as_boolean()),
        Some(false)
    );
    assert_eq!(vm.global("plain").and_then(|v| v.as_boolean()), Some(true));
    assert_eq!(vm.global("gated").and_then(|v| v.as_boolean()), Some(false));
    assert_eq!(
        r.shared.borrow().control(0).map(|c| c.captain_flags),
        Some(0x4000 | 0x8000)
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
    run_until(&mut r, &[1], 4);
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
    run_until(&mut r, &[3, 5], 3);
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
    // The boolean setter is a bit of the flag word (0x8); the count is an integer setting.
    assert_eq!(c.flags & 0x8, 0x8);
    assert_eq!(c.settings.get("SetReplacementCount"), Some(&AiArg::Int(4)));
    assert!(!c.settings.contains_key("SetHeroesFlee"));
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

/// The argument text of a call to a row: one value of each parameter type.
fn arg_values(row: &AiNative) -> Vec<String> {
    let body = row.sig.strip_prefix('(').expect("signature starts with (");
    let (params, _) = body.split_once(')').expect("signature has )");
    let mut out = Vec::new();
    let mut chars = params.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            'I' => "0".to_string(),
            'R' => "0.0".to_string(),
            'S' => "\"\"".to_string(),
            'B' => "false".to_string(),
            'C' => "function dummy".to_string(),
            'H' => {
                let ty: String = chars.by_ref().take_while(|&c| c != ';').collect();
                if ty == "player" {
                    "Player(0)".to_string()
                } else {
                    "null".to_string()
                }
            }
            other => panic!("unknown signature character {other}"),
        });
    }
    out
}

/// The global that receives a row's result, by its return type (`None` for a row without a result).
fn result_slot(row: &AiNative) -> Option<&'static str> {
    let ret = row.sig.rsplit_once(')').map_or("V", |(_, r)| r);
    match ret.as_bytes().first() {
        Some(b'I') => Some("rInt"),
        Some(b'R') => Some("rReal"),
        Some(b'B') => Some("rBool"),
        Some(b'S') => Some("rStr"),
        Some(b'H') if ret.starts_with("Hplayer") => Some("rPlayer"),
        Some(b'H') => Some("rUnit"),
        _ => None,
    }
}

/// Every row is dispatched: a script that calls each of the 124 natives once reaches the not-implemented fallback for
/// exactly the rows marked not implemented, and no other row.
#[test]
fn every_row_is_dispatched_and_only_the_missing_rows_fall_back() {
    let mut decls = String::from("type player extends handle\ntype unit extends handle\n");
    decls.push_str("native Player takes integer number returns player\n");
    for row in AI_NATIVES {
        decls.push_str(&declaration(row));
        decls.push('\n');
    }
    let mut script = String::from(
        "globals\n    integer rInt = 0\n    real rReal = 0.0\n    boolean rBool = false\n    string rStr = \"\"\n    unit rUnit = null\n    player rPlayer = null\nendglobals\n",
    );
    script.push_str("function dummy takes nothing returns nothing\nendfunction\n");
    script.push_str("function main takes nothing returns nothing\n");
    for row in AI_NATIVES {
        let args = arg_values(row).join(", ");
        match result_slot(row) {
            None => script.push_str(&format!("    call {}({args})\n", row.name)),
            Some(slot) => script.push_str(&format!("    set {slot} = {}({args})\n", row.name)),
        }
    }
    script.push_str("endfunction\n");
    let mut lib = AiLibrary::new(&decls);
    lib.add_script("call.ai", &script);
    let main = r#"
function main takes nothing returns nothing
    call StartMeleeAI(Player(0), "call.ai")
endfunction
"#;
    let mut r = rig(main, lib);
    ticks(&mut r, 5);
    let vm = r.ai.vm(0).expect("AI VM built");
    assert!(
        vm.unbound_natives().is_empty(),
        "{:?}",
        vm.unbound_natives()
    );
    let sh = r.shared.borrow();
    let mut missing: Vec<&str> = AI_NATIVES
        .iter()
        .filter(|row| !row.implemented)
        .map(|row| row.name)
        .collect();
    let mut fell_back: Vec<&str> = sh.unimplemented.keys().copied().collect();
    missing.sort_unstable();
    fell_back.sort_unstable();
    assert_eq!(fell_back, missing);
    assert!(sh.unimplemented.values().all(|&n| n == 1));
}

/// `IgnoredUnits` counts the AI's unassigned units of a type (those in no captain), `GetHeroId` / `GetHeroLevelAI`
/// are 0 without a hero, `GetNextExpansion` is -1 without a mine, and `GetMegaTarget` is null without the watch flag.
#[test]
fn s19_natives_answer_on_a_small_world() {
    let main = r#"
function main takes nothing returns nothing
    call CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    call CreateUnit(Player(0), 'hfoo', 64.0, 0.0, 0.0)
    call CreateUnit(Player(0), 'hfoo', 128.0, 0.0, 0.0)
    call StartMeleeAI(Player(0), "cap.ai")
endfunction
"#;
    let ai_src = r#"
native CreateCaptains takes nothing returns nothing
native InitAssault takes nothing returns nothing
native AddAssault takes integer qty, integer id returns boolean
native IgnoredUnits takes integer id returns integer
native GetHeroId takes nothing returns integer
native GetHeroLevelAI takes nothing returns integer
native GetNextExpansion takes nothing returns integer
native GetMegaTarget takes nothing returns unit
native GetBuilding takes player p returns unit
globals
    integer free_before = -9
    integer free_after = -9
    integer hero = -9
    integer level = -9
    integer next_exp = -9
    integer mega_null = -9
    integer bldg_null = -9
endglobals
function main takes nothing returns nothing
    call CreateCaptains()
    set free_before = IgnoredUnits('hfoo')
    call InitAssault()
    call AddAssault(2, 'hfoo')
    set free_after = IgnoredUnits('hfoo')
    set hero = GetHeroId()
    set level = GetHeroLevelAI()
    set next_exp = GetNextExpansion()
    if GetMegaTarget() == null then
        set mega_null = 1
    endif
    if GetBuilding(null) == null then
        set bldg_null = 1
    endif
endfunction
"#;
    let mut r = captain_rig(main, ai_src);
    tick(&mut r);
    let got = ["free_before", "free_after", "hero", "level", "next_exp", "mega_null", "bldg_null"].map(|n| ai_int(&r, 0, n));
    assert_eq!(got, [3, 1, 0, 0, -1, 1, 1]);
}

/// The table's level labels and the not-implemented list are consistent: a not-implemented row always has a reason.
#[test]
fn not_implemented_rows_say_why() {
    let missing: Vec<&AiNative> = AI_NATIVES.iter().filter(|r| !r.implemented).collect();
    for row in missing {
        assert!(row.note.starts_with("NOT IMPLEMENTED: "), "{}", row.name);
    }
}

// ---- captains (docs/spec/ai-natives.md section 8) ----

fn hfoo_type() -> UnitType {
    UnitType {
        name: "hfoo".to_string(),
        max_life: F32::from_bits(0x42C8_0000),
        ..Default::default()
    }
}

fn captain_rig(main: &str, script: &str) -> Rig {
    let mut lib = library();
    lib.add_script("cap.ai", script);
    rig_with(main, lib, |game| {
        game.borrow_mut().unit_types_mut().insert(HFOO, hfoo_type());
    })
}

/// `AddAssault` supplies units: members already in the group count first, idle units are recruited for the rest, the
/// request returns true when the quantity is met and the full bit stays; a shortage clears the bit and returns false.
#[test]
fn assault_requests_fill_the_attack_group() {
    let main = r#"
function main takes nothing returns nothing
    call CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    call CreateUnit(Player(0), 'hfoo', 64.0, 0.0, 0.0)
    call CreateUnit(Player(0), 'hfoo', 128.0, 0.0, 0.0)
    call StartMeleeAI(Player(0), "cap.ai")
endfunction
"#;
    let ai_src = r#"
native CreateCaptains takes nothing returns nothing
native InitAssault takes nothing returns nothing
native AddAssault takes integer qty, integer id returns boolean
native CaptainGroupSize takes nothing returns integer
native CaptainIsEmpty takes nothing returns boolean
native CaptainReadinessHP takes nothing returns integer
globals
    integer ok1 = -9
    integer size1 = -9
    integer full1 = -9
    integer ok2 = -9
    integer size2 = -9
    integer full2 = -9
    integer hp = -9
    integer empty0 = -9
endglobals
function b2i takes boolean b returns integer
    if b then
        return 1
    endif
    return 0
endfunction
function main takes nothing returns nothing
    call CreateCaptains()
    set empty0 = b2i(CaptainIsEmpty())
    call InitAssault()
    set ok1 = b2i(AddAssault(2, 'hfoo'))
    set size1 = CaptainGroupSize()
    set full1 = b2i(CaptainIsFull())
    call InitAssault()
    set ok2 = b2i(AddAssault(5, 'hfoo'))
    set size2 = CaptainGroupSize()
    set full2 = b2i(CaptainIsFull())
    set hp = CaptainReadinessHP()
endfunction
"#;
    let mut r = captain_rig(main, ai_src);
    tick(&mut r);
    let got = ["empty0", "ok1", "size1", "full1", "ok2", "size2", "full2", "hp"].map(|n| ai_int(&r, 0, n));
    // 2 requested, 2 recruited, full; then 5 requested: the 2 count, 1 more is recruited, 2 are missing: not full.
    assert_eq!(got, [1, 1, 2, 1, 0, 3, 0, 100]);
}

/// An attack-move records a departure with the group, and `CaptainAtGoal` / `CaptainIsHome` follow the group centre.
#[test]
fn attack_move_gives_the_group_a_goal() {
    let main = r#"
function main takes nothing returns nothing
    call CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    call CreateUnit(Player(0), 'hfoo', 64.0, 0.0, 0.0)
    call StartMeleeAI(Player(0), "cap.ai")
endfunction
"#;
    let ai_src = r#"
native CreateCaptains takes nothing returns nothing
native InitAssault takes nothing returns nothing
native AddAssault takes integer qty, integer id returns boolean
native AttackMoveXY takes integer x, integer y returns nothing
native CaptainAtGoal takes nothing returns boolean
native CaptainIsHome takes nothing returns boolean
globals
    integer home = -9
    integer atgoal0 = -9
    integer atgoal1 = -9
endglobals
function b2i takes boolean b returns integer
    if b then
        return 1
    endif
    return 0
endfunction
function main takes nothing returns nothing
    call CreateCaptains()
    call InitAssault()
    call AddAssault(2, 'hfoo')
    set atgoal0 = b2i(CaptainAtGoal())
    set home = b2i(CaptainIsHome())
    call AttackMoveXY(30, 0)
    set atgoal1 = b2i(CaptainAtGoal())
endfunction
"#;
    let mut r = captain_rig(main, ai_src);
    tick(&mut r);
    // No goal yet: not at goal. The group stands at home. Goal (30, 0) is within the goal radius of the centre.
    assert_eq!([ai_int(&r, 0, "atgoal0"), ai_int(&r, 0, "home"), ai_int(&r, 0, "atgoal1")], [0, 1, 1]);
    let sh = r.shared.borrow();
    let cap = &sh.control(0).expect("control").captains[0];
    assert_eq!(cap.departures.len(), 1);
    assert_eq!(cap.departures[0].size, 2);
    assert_eq!(cap.mode, sim::captains::Mode::AttackMove);
}

/// Creeps group into camps and `GetCreepCamp` returns the first living member of the nearest camp whose level fits.
#[test]
fn creep_camps_are_found_by_distance_and_level() {
    let main = r#"
function main takes nothing returns nothing
    call CreateUnit(Player(0), 'hfoo', 4000.0, 0.0, 0.0)
    call CreateUnit(Player(12), 'hfoo', 5000.0, 0.0, 0.0)
    call CreateUnit(Player(12), 'hfoo', 5050.0, 0.0, 0.0)
    call CreateUnit(Player(12), 'hfoo', 9000.0, 0.0, 0.0)
    call StartMeleeAI(Player(0), "cap.ai")
endfunction
"#;
    let ai_src = r#"
native CreateCaptains takes nothing returns nothing
native GetCreepCamp takes integer lo, integer hi, boolean fly returns unit
native CreepsOnMap takes nothing returns boolean
native KillUnit takes unit u returns nothing
globals
    integer near = -9
    integer none = -9
    integer far = -9
    integer anyc = -9
endglobals
function b2i takes boolean b returns integer
    if b then
        return 1
    endif
    return 0
endfunction
function main takes nothing returns nothing
    local unit u
    call CreateCaptains()
    set anyc = b2i(CreepsOnMap())
    set u = GetCreepCamp(0, 100, true)
    set near = b2i(u != null)
    call KillUnit(u)
    set u = GetCreepCamp(0, 100, true)
    set far = b2i(u != null)
    set none = b2i(GetCreepCamp(5, 100, true) == null)
endfunction
"#;
    let mut r = captain_rig(main, ai_src);
    tick(&mut r);
    let got = ["anyc", "near", "far", "none"].map(|n| ai_int(&r, 0, n));
    assert_eq!(got[0], 1);
    assert_eq!(got[1], 1);
    assert_eq!(got[2], 1);
    // Both camps are below the minimum level 5: the nearest one is returned as the fallback.
    assert_eq!(got[3], 0);
    let sh = r.shared.borrow();
    assert_eq!(sh.camps.camps.len(), 2);
    assert_eq!(sh.camps.camps[0].len(), 2);
}

