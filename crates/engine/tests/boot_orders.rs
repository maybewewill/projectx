//! Unit order data at boot and replay orders on a booted map (`engine::data::install`, `Engine::apply_commands`).
//!
//! The synthetic game archive gives one unit type (`hfoo`) its movement type, speed, weapon flag and ability list.
//! The map's `main` creates the footman, so the order data has to be in place before any unit is created
//! (`docs/spec/replay-actions.md` section 9, now closed). Replay orders name the unit by the object pair the model
//! assigns (`sim::commands::model_pair`, UNSURE), so no explicit registration is needed.

mod common;

use common::{mpq, w3i, PlayerRec, BLIZZARD_J, COMMON_J};
use engine::{Engine, Inputs};
use sim::commands::{model_pair, Command, OrderTarget, Outcome, WorldPoint};
use sim::natives::game::ObjKey;
use sim::orders::{AbilityKind, TypeOrderData};
use softfloat::F32;

const FOOTMAN: u32 = 0x6866_6F6F;
const ORDER_MOVE: u32 = 0x000D_0012;
const ORDER_ATTACK: u32 = 0x000D_000F;

const BOUNDS: [f32; 8] = [-1024.0, 1024.0, -896.0, 896.0, 0.0, 0.0, 0.0, 0.0];

const MISC: &str =
    "[Misc]\nDefenseArmor=0.05\nDamageBonusNormal=1,1,1,1,1,1,1,1\nReactionDelay=0.3\n";
const UI: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitUIID\"\nC;X2;Y1;K\"name\"\nC;X1;Y2;K\"hfoo\"\nC;X2;Y2;K\"Footman\"\nE\n";
const BALANCE: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"HP\"\nC;X3;Y1;K\"spd\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K420\nC;X3;Y2;K270\nE\n";
const WEAPONS: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"weapsOn\"\nC;X3;Y1;K\"dice1\"\nC;X4;Y1;K\"sides1\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K1\nC;X3;Y2;K1\nC;X4;Y2;K6\nE\n";
const ABILITIES: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitAbilID\"\nC;X2;Y1;K\"abilList\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K\"Amov,Aatk,ANsg,Amov\"\nE\n";

/// `movetp` of the footman's `UnitData.slk` row. An empty value makes the type immobile.
fn data_table(movetp: &str) -> String {
    format!(
        "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"movetp\"\nC;X3;Y1;K\"canSleep\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K\"{movetp}\"\nC;X3;Y2;K0\nE\n"
    )
}

/// Builds the game archive. `with_abilities` drops the `UnitAbilities.slk` table when false.
fn game_archive(movetp: &str, with_abilities: bool) -> Vec<u8> {
    let mut files = vec![
        ("Units\\MiscData.txt", MISC.as_bytes().to_vec()),
        ("Units\\UnitUI.slk", UI.as_bytes().to_vec()),
        ("Units\\UnitData.slk", data_table(movetp).into_bytes()),
        ("Units\\UnitBalance.slk", BALANCE.as_bytes().to_vec()),
        ("Units\\UnitWeapons.slk", WEAPONS.as_bytes().to_vec()),
    ];
    if with_abilities {
        files.push(("Units\\UnitAbilities.slk", ABILITIES.as_bytes().to_vec()));
    }
    mpq(&files, 0)
}

/// `main` creates one footman at the origin, owned by player 0. The script declares only what it uses.
const WAR3MAP_J: &str = r#"
type unit extends handle
type trigger extends handle
type triggeraction extends handle
type boolexpr extends handle
type event extends handle
type playerunitevent extends handle

native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateTrigger takes nothing returns trigger
native TriggerAddAction takes trigger t, code c returns triggeraction
native ConvertPlayerUnitEvent takes integer i returns playerunitevent
native TriggerRegisterPlayerUnitEvent takes trigger t, player whichPlayer, playerunitevent whichEvent, boolexpr filter returns event
native GetTriggerUnit takes nothing returns unit
native GetIssuedOrderId takes nothing returns integer

globals
    unit gA = null
    trigger gT = null
    integer gFired = 0
    integer gLastOrder = 0
    unit gLastUnit = null
endglobals

function OnIssued takes nothing returns nothing
    set gFired = gFired + 1
    set gLastOrder = GetIssuedOrderId()
    set gLastUnit = GetTriggerUnit()
endfunction

function main takes nothing returns nothing
    set gA = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
endfunction

function config takes nothing returns nothing
    set gT = CreateTrigger()
    call TriggerRegisterPlayerUnitEvent(gT, Player(0), ConvertPlayerUnitEvent(39), null)
    call TriggerAddAction(gT, function OnIssued)
endfunction
"#;

fn map() -> Vec<u8> {
    let info = w3i(
        "Boot Orders",
        BOUNDS,
        &[PlayerRec {
            number: 0,
            player_type: 1,
            race: 4,
            name: "Alice",
            start: (0.0, 0.0),
        }],
        &[(0b1, "Force A")],
    );
    mpq(
        &[
            ("war3map.w3i", info),
            ("war3map.j", WAR3MAP_J.as_bytes().to_vec()),
        ],
        512,
    )
}

fn boot_with(movetp: &str, with_abilities: bool) -> Engine {
    Engine::boot(Inputs {
        map: map(),
        game_dll: None,
        common_j: COMMON_J.to_string(),
        blizzard_j: BLIZZARD_J.to_string(),
        rng_seed: 0,
        ai_common: None,
        ai_scripts: Vec::new(),
        game_archives: vec![game_archive(movetp, with_abilities).into()],
        check_natives: false,
    })
    .expect("boot")
}

/// The unit held by the script global `gA`, after `main` has run.
fn footman(engine: &Engine) -> ObjKey {
    let id = engine
        .host()
        .vm()
        .global("gA")
        .and_then(|v| v.as_handle())
        .expect("gA is a handle");
    engine
        .host()
        .game()
        .unit_of_handle(id)
        .expect("gA is a live unit")
}

/// Runs `main` (queued at boot) without advancing the clock, so the footman exists.
fn started(mut engine: Engine) -> Engine {
    engine.step_ticks(0);
    engine
}

/// One replay slot's worth of commands for player 0: select the footman by its model pair, then a move order.
fn select_then_point_order(engine: &Engine, unit: ObjKey, id: u32) -> Vec<(u8, Command)> {
    let pair = model_pair(&engine.host().game(), unit);
    let point = WorldPoint {
        x: F32::from_bits(64.0f32.to_bits()),
        y: F32::from_bits(0.0f32.to_bits()),
    };
    vec![
        (
            0,
            Command::Select {
                player: 0,
                add: true,
                units: vec![pair],
            },
        ),
        (
            0,
            Command::Order {
                player: 0,
                flags: 0,
                id,
                target: OrderTarget::Point(point),
            },
        ),
    ]
}

#[test]
fn boot_gives_the_footman_its_move_and_attack_orders() {
    let engine = started(boot_with("foot", true));
    let game = engine.host().game();
    let data = game
        .orders()
        .type_data(FOOTMAN)
        .expect("the footman type has order data at boot");
    assert_eq!(
        data,
        &TypeOrderData {
            abilities: vec![AbilityKind::Move, AbilityKind::Attack],
        }
    );
    // The creation attached the fixed slots and the list (`ANsg` from the list; the duplicate `Amov` is skipped).
    drop(game);
    let unit = footman(&engine);
    let ids = engine.host().game().unit_order_ids(unit);
    assert!(ids.is_empty(), "no order yet: {ids:?}");
}

#[test]
fn booted_map_accepts_a_replay_move_order() {
    let mut engine = started(boot_with("foot", true));
    let unit = footman(&engine);
    let cmds = select_then_point_order(&engine, unit, ORDER_MOVE);
    let outcomes = engine.apply_commands(&cmds);
    assert_eq!(
        outcomes[1],
        Outcome::Ordered {
            accepted: 1,
            refused: 0,
            unresolved: 0,
        }
    );
    assert_eq!(engine.host().game().unit_order_ids(unit), vec![ORDER_MOVE]);
}

#[test]
fn a_unit_type_without_movement_type_refuses_the_move_order() {
    let mut engine = started(boot_with("", true));
    let unit = footman(&engine);
    assert!(engine
        .host()
        .game()
        .orders()
        .type_data(FOOTMAN)
        .is_some_and(|d| {
            !d.abilities.contains(&AbilityKind::Move) && d.abilities.contains(&AbilityKind::Attack)
        }));
    let cmds = select_then_point_order(&engine, unit, ORDER_MOVE);
    let outcomes = engine.apply_commands(&cmds);
    assert_eq!(
        outcomes[1],
        Outcome::Ordered {
            accepted: 0,
            refused: 1,
            unresolved: 0,
        }
    );
    assert!(engine.host().game().unit_order_ids(unit).is_empty());
}

#[test]
fn the_attack_ability_comes_from_the_weapon_flag_without_an_ability_list() {
    // No UnitAbilities.slk: the fixed slots still come from the unit record, the list is empty.
    let mut engine = started(boot_with("foot", false));
    let unit = footman(&engine);
    let cmds = select_then_point_order(&engine, unit, ORDER_ATTACK);
    let outcomes = engine.apply_commands(&cmds);
    assert_eq!(
        outcomes[1],
        Outcome::Ordered {
            accepted: 1,
            refused: 0,
            unresolved: 0,
        }
    );
}

#[test]
fn replay_order_fires_the_issued_order_trigger_like_a_native_order() {
    // `config` registered the trigger for the player-unit issued-order event of a point order (39): the replayed
    // move is a point order, so the trigger runs once, with the unit and the order id in its context.
    let mut engine = started(boot_with("foot", true));
    let unit = footman(&engine);
    let cmds = select_then_point_order(&engine, unit, ORDER_MOVE);
    engine.apply_commands(&cmds);
    let vm = engine.host().vm();
    assert_eq!(vm.global("gFired").and_then(|v| v.as_integer()), Some(1));
    assert_eq!(
        vm.global("gLastOrder").and_then(|v| v.as_integer()),
        Some(ORDER_MOVE as i32)
    );
    let handle = engine.host().game().handle_of(unit);
    assert_eq!(
        engine
            .host()
            .vm()
            .global("gLastUnit")
            .and_then(|v| v.as_handle()),
        Some(handle)
    );
}

#[test]
fn replay_immediate_order_does_not_fire_the_point_trigger() {
    // The stop order is immediate (event 38), which the trigger does not listen to: nothing fires.
    let mut engine = started(boot_with("foot", true));
    let unit = footman(&engine);
    let pair = model_pair(&engine.host().game(), unit);
    let cmds = vec![
        (
            0,
            Command::Select {
                player: 0,
                add: true,
                units: vec![pair],
            },
        ),
        (
            0,
            Command::Order {
                player: 0,
                flags: 0,
                id: 0x000D_0004,
                target: OrderTarget::None,
            },
        ),
    ];
    engine.apply_commands(&cmds);
    assert_eq!(
        engine
            .host()
            .vm()
            .global("gFired")
            .and_then(|v| v.as_integer()),
        Some(0)
    );
}
