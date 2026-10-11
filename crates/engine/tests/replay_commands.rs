//! Replay time slots through the engine: decoded player actions applied in stream order before the ticks
//! (`engine::replay`, `Engine::run_turn`; lockstep.md section 3, replay-actions.md).
//!
//! The map's `config` creates two footmen from the synthetic game archive (unit records for `hfoo`), so
//! the handles are the real ones. The action bytes are built here, in the layouts the decoder reads.

mod common;

use common::{mpq, w3i, PlayerRec, BLIZZARD_J, COMMON_J};
use engine::replay::{push_time_slot, ReplayError};
use engine::{Engine, Inputs};
use sim::commands::{Command, ObjectRef, Outcome};
use sim::lockstep::{GameSpeed, TurnScheduler};
use sim::natives::game::ObjKey;
use w3g::CommandBlock;

const BOUNDS: [f32; 8] = [-1024.0, 1024.0, -896.0, 896.0, 0.0, 0.0, 0.0, 0.0];
const HFOO: u32 = 0x6866_6F6F;

const MISC: &str =
    "[Misc]\nDefenseArmor=0.05\nDamageBonusNormal=1,1,1,1,1,1,1,1\nReactionDelay=0.3\n";
const BALANCE: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"def\"\nC;X3;Y1;K\"defType\"\nC;X4;Y1;K\"HP\"\nC;X5;Y1;K\"spd\"\nC;X6;Y1;K\"goldcost\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K2\nC;X3;Y2;K\"medium\"\nC;X4;Y2;K420\nC;X5;Y2;K\"270.5\"\nC;X6;Y2;K135\nE\n";
const UI: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitUIID\"\nC;X2;Y1;K\"name\"\nC;X1;Y2;K\"hfoo\"\nC;X2;Y2;K\"Footman\"\nE\n";
const DATA: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"canSleep\"\nC;X1;Y2;K\"hfoo\"\nC;X2;Y2;K0\nE\n";
const WEAPONS: &str = "ID;PWXL;N;E\n\
C;X1;Y1;K\"unitID\"\nC;X2;Y1;K\"dice1\"\nC;X3;Y1;K\"sides1\"\nC;X4;Y1;K\"dmgplus1\"\n\
C;X5;Y1;K\"cool1\"\nC;X6;Y1;K\"rangeN1\"\nC;X7;Y1;K\"atkType1\"\n\
C;X1;Y2;K\"hfoo\"\nC;X2;Y2;K2\nC;X3;Y2;K6\nC;X4;Y2;K3\nC;X5;Y2;K1.35\nC;X6;Y2;K60\nC;X7;Y2;K\"Normal\"\n\
C;X1;Y3;K\"hfo\"\nC;X2;Y3;K1\nE\n";

/// Two footmen: `gA` at the origin, `gB` at x = 64, created by `main`. The synthetic `common.j` of this crate
/// declares no unit type, so the map declares what it uses.
const WAR3MAP_J: &str = r#"
type unit extends handle
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit

globals
    unit gA = null
    unit gB = null
endglobals

function main takes nothing returns nothing
    set gA = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    set gB = CreateUnit(Player(0), 'hfoo', 64.0, 0.0, 0.0)
endfunction
"#;

fn game_archive() -> Vec<u8> {
    mpq(
        &[
            ("Units\\MiscData.txt", MISC.as_bytes().to_vec()),
            ("Units\\UnitUI.slk", UI.as_bytes().to_vec()),
            ("Units\\UnitData.slk", DATA.as_bytes().to_vec()),
            ("Units\\UnitBalance.slk", BALANCE.as_bytes().to_vec()),
            ("Units\\UnitWeapons.slk", WEAPONS.as_bytes().to_vec()),
        ],
        0,
    )
}

fn map() -> Vec<u8> {
    let info = w3i(
        "Replay Test",
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

fn boot() -> Engine {
    Engine::boot(Inputs {
        map: map(),
        game_dll: None,
        common_j: COMMON_J.to_string(),
        blizzard_j: BLIZZARD_J.to_string(),
        rng_seed: 0,
        ai_common: None,
        ai_scripts: Vec::new(),
        game_archives: vec![game_archive()],
        check_natives: false,
    })
    .expect("boot")
}

fn unit(engine: &Engine, name: &str) -> ObjKey {
    let id = engine
        .host()
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"));
    engine
        .host()
        .game()
        .unit_of_handle(id)
        .unwrap_or_else(|| panic!("{name} is not a live unit"))
}

/// Booted engine with `gA` = pair (1, 1) and `gB` = pair (1, 2).
fn rig() -> (Engine, ObjKey, ObjKey) {
    let mut engine = boot();
    // The engine does not fill the order data of unit types yet (no `set_type_orders` outside tests), so a
    // footman would refuse every order. The test gives it the move and attack abilities.
    engine.host_mut().game_mut().set_type_orders(
        HFOO,
        sim::orders::TypeOrderData {
            abilities: vec![
                sim::orders::AbilityKind::Move,
                sim::orders::AbilityKind::Attack,
            ],
        },
    );
    // Units take their abilities when they are created, so `main` (which creates them) runs after the order
    // data is set: `step_ticks(0)` starts the queued `main` without advancing the clock.
    engine.step_ticks(0);
    let a = unit(&engine, "gA");
    let b = unit(&engine, "gB");
    engine
        .commands_mut()
        .register_object(ObjectRef { id1: 1, id2: 1 }, a);
    engine
        .commands_mut()
        .register_object(ObjectRef { id1: 1, id2: 2 }, b);
    (engine, a, b)
}

fn le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// Select (mode 1) one unit by its pair.
fn select(pair: (u32, u32)) -> Vec<u8> {
    let mut b = vec![0x16, 0x01, 0x01, 0x00];
    b.extend_from_slice(&le(pair.0));
    b.extend_from_slice(&le(pair.1));
    b
}

/// A move order at a point (0x11: flags, ability, two zero words, x, y).
fn move_to(x: f32, y: f32) -> Vec<u8> {
    let mut b = vec![0x11, 0x00, 0x00];
    b.extend_from_slice(&le(0x000D_0012));
    b.extend_from_slice(&le(0));
    b.extend_from_slice(&le(0));
    b.extend_from_slice(&le(x.to_bits()));
    b.extend_from_slice(&le(y.to_bits()));
    b
}

fn block(player: u8, actions: Vec<u8>) -> CommandBlock {
    CommandBlock {
        player_id: player,
        offset: 0,
        actions,
    }
}

#[test]
fn a_turn_applies_its_commands_in_stream_order_before_its_ticks() {
    let (mut engine, a, b) = rig();
    // Player 1 selects gA then orders it to move; a second block (player 2) in the same slot is not
    // a selection of player 1, so it changes nothing for player 1.
    let mut actions = select((1, 1));
    actions.extend(move_to(128.0, 0.0));
    let blocks = [block(1, actions), block(2, select((1, 2)))];
    let mut sched = TurnScheduler::new(GameSpeed::Normal);
    let report = push_time_slot(&mut sched, 100, &blocks).expect("decode");
    // 100 ms of turn time is four 25 ms units; normal speed runs four game ticks per unit.
    assert_eq!(report.game_ticks, 16);
    engine.run_turn(&report);
    assert_eq!(engine.commands().selection(1), &[a]);
    assert_eq!(engine.commands().selection(2), &[b]);
    // The issue is recorded in the order system's accepted-order log (kept after the order completes).
    let game = engine.host().game();
    let issued: Vec<_> = game
        .orders()
        .issued()
        .iter()
        .map(|r| (r.unit, r.order.id))
        .collect();
    assert_eq!(
        issued,
        vec![(a, 0x000D_0012)],
        "the move went to the selected unit only"
    );
}

#[test]
fn stream_order_decides_whether_an_order_reaches_the_selection() {
    let (mut engine, a, _) = rig();
    // The order comes first, so the selection is still empty when it is given.
    let mut actions = move_to(10.0, 10.0);
    actions.extend(select((1, 1)));
    let report = push_time_slot(
        &mut TurnScheduler::new(GameSpeed::Normal),
        0,
        &[block(1, actions)],
    )
    .expect("decode");
    let outcomes = engine.apply_commands(&report.actions);
    assert_eq!(
        outcomes,
        vec![
            Outcome::Ordered {
                accepted: 0,
                refused: 0,
                unresolved: 0
            },
            Outcome::Selected {
                now: 1,
                unresolved: 0
            },
        ]
    );
    assert!(engine.host().game().unit_order_ids(a).is_empty());
}

#[test]
fn speed_and_recorded_actions_go_through_the_same_path() {
    let (mut engine, _, _) = rig();
    // 0x03 speed slow, 0x6B sync integer ("a", "b", "c", value 5), 0x01 pause.
    let mut actions = vec![0x03, 0x00];
    actions.push(0x6B);
    actions.extend_from_slice(b"a\0b\0c\0");
    actions.extend_from_slice(&le(5));
    actions.push(0x01);
    let report = push_time_slot(
        &mut TurnScheduler::new(GameSpeed::Normal),
        0,
        &[block(1, actions)],
    )
    .expect("decode");
    let outcomes = engine.apply_commands(&report.actions);
    assert_eq!(outcomes.len(), 3);
    assert_eq!(outcomes[0], Outcome::Speed { setting: 0 });
    assert_eq!(outcomes[1], Outcome::Recorded);
    assert_eq!(outcomes[2], Outcome::Recorded);
    assert_eq!(engine.commands().speed_setting(), 0);
    assert!(matches!(
        report.actions[0].1,
        Command::SetSpeed {
            player: 1,
            setting: 0
        }
    ));
}

#[test]
fn an_unknown_action_fails_the_slot_with_its_offset() {
    let blocks = [CommandBlock {
        player_id: 3,
        offset: 500,
        actions: vec![0x01, 0x6F],
    }];
    let err = push_time_slot(&mut TurnScheduler::new(GameSpeed::Normal), 100, &blocks)
        .expect_err("0x6F is not an action");
    match err {
        ReplayError::Decode(msg) => assert!(
            msg.contains("0x6f") && msg.contains("0x1f5"),
            "message names the id and the offset: {msg}"
        ),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_selection_mode_other_than_add_or_remove_fails_the_slot() {
    let blocks = [block(1, vec![0x16, 0x07, 0x00, 0x00])];
    let err =
        push_time_slot(&mut TurnScheduler::new(GameSpeed::Normal), 0, &blocks).expect_err("mode 7");
    assert!(matches!(err, ReplayError::SelectionMode { mode: 7, .. }));
}
