//! Replay commands applied to the game (`sim::commands`, model in `docs/spec/replay-actions.md`).
//!
//! The units are created by a JASS script through the `CreateUnit` native, so the handles and the unit
//! records are the real ones. The movement needs no tables here: the orders are checked by their list
//! (`unit_order_ids`), not by where the unit stands.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::commands::{Command, CommandState, ObjectRef, Outcome, OrderTarget, WorldPoint, FLAG_QUEUED};
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type location extends handle
type boolexpr extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
"#;

const FOOTMAN: u32 = 0x6866_6F6F;

const SETUP: &str = r#"
globals
  unit gA = null
  unit gB = null
endglobals

function Main takes nothing returns nothing
  set gA = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
  set gB = CreateUnit(Player(0), 'hfoo', I2R(64), I2R(0), I2R(0))
endfunction
"#;

fn host() -> ScriptHost {
    let src = format!("{PRELUDE}\n{SETUP}");
    let prog = jass::parse(&src).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                name: "Footman".to_string(),
                max_life: F32::from_bits(0x43D2_0000),
                move_speed: F32::from_bits(270.0f32.to_bits()),
                ..Default::default()
            },
        );
        g.set_type_orders(
            FOOTMAN,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
    }
    host
}

/// The unit object behind the unit global `name`.
fn unit(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"));
    host.game()
        .unit_of_handle(id)
        .unwrap_or_else(|| panic!("handle {id} is not a live unit"))
}

/// A host with the two footmen created (`gA` at the origin, `gB` at x = 64), registered as pairs
/// `(1, 1)` and `(1, 2)`, and the command state with those pairs.
fn rig() -> (ScriptHost, CommandState, ObjKey, ObjKey) {
    let mut host = host();
    host.start("Main", &[]).expect("start");
    host.step();
    let a = unit(&host, "gA");
    let b = unit(&host, "gB");
    let mut cs = CommandState::new();
    cs.register_object(ObjectRef { id1: 1, id2: 1 }, a);
    cs.register_object(ObjectRef { id1: 1, id2: 2 }, b);
    (host, cs, a, b)
}

fn obj(id1: u32, id2: u32) -> ObjectRef {
    ObjectRef { id1, id2 }
}

fn point(x: f32, y: f32) -> WorldPoint {
    WorldPoint {
        x: F32::from_bits(x.to_bits()),
        y: F32::from_bits(y.to_bits()),
    }
}

#[test]
fn select_adds_once_and_removes() {
    let (host, mut cs, a, b) = rig();
    let cell = host.game_handle();
    let out = cs.apply(
        &cell,
        &Command::Select {
            player: 1,
            add: true,
            units: vec![obj(1, 1), obj(1, 2), obj(1, 1)],
        },
    );
    assert_eq!(out, Outcome::Selected { now: 2, unresolved: 0 });
    assert_eq!(cs.selection(1), &[a, b]);
    let out = cs.apply(
        &cell,
        &Command::Select {
            player: 1,
            add: false,
            units: vec![obj(1, 1)],
        },
    );
    assert_eq!(out, Outcome::Selected { now: 1, unresolved: 0 });
    assert_eq!(cs.selection(1), &[b]);
    // Another player has its own selection.
    assert!(cs.selection(2).is_empty());
}

#[test]
fn unknown_pairs_are_counted_and_do_nothing() {
    let (host, mut cs, a, _) = rig();
    let cell = host.game_handle();
    let out = cs.apply(
        &cell,
        &Command::Select {
            player: 1,
            add: true,
            units: vec![obj(9, 9), obj(1, 1)],
        },
    );
    assert_eq!(out, Outcome::Selected { now: 1, unresolved: 1 });
    assert_eq!(cs.selection(1), &[a]);
}

#[test]
fn hotkey_groups_assign_and_select() {
    let (host, mut cs, _a, b) = rig();
    let cell = host.game_handle();
    cs.apply(
        &cell,
        &Command::Select {
            player: 1,
            add: true,
            units: vec![obj(1, 1), obj(1, 2)],
        },
    );
    let out = cs.apply(
        &cell,
        &Command::AssignGroup {
            player: 1,
            group: 3,
            units: vec![obj(1, 2)],
        },
    );
    assert_eq!(out, Outcome::Grouped { units: 1, unresolved: 0 });
    assert_eq!(cs.group(1, 3), &[b]);
    cs.apply(
        &cell,
        &Command::SelectGroup {
            player: 1,
            group: 3,
        },
    );
    assert_eq!(cs.selection(1), &[b]);
    cs.apply(
        &cell,
        &Command::SelectGroup {
            player: 1,
            group: 7,
        },
    );
    assert!(cs.selection(1).is_empty(), "an empty group selects nothing");
}

#[test]
fn subgroup_choice_leaves_one_unit_selected() {
    let (host, mut cs, _, b) = rig();
    let cell = host.game_handle();
    cs.apply(
        &cell,
        &Command::Select {
            player: 1,
            add: true,
            units: vec![obj(1, 1), obj(1, 2)],
        },
    );
    cs.apply(
        &cell,
        &Command::SelectSubgroup {
            player: 1,
            item: FOOTMAN,
            unit: obj(1, 2),
        },
    );
    assert_eq!(cs.selection(1), &[b]);
}

#[test]
fn point_order_goes_to_every_selected_unit_and_queued_flag_appends() {
    let (host, mut cs, a, b) = rig();
    let cell = host.game_handle();
    cs.apply(
        &cell,
        &Command::Select {
            player: 1,
            add: true,
            units: vec![obj(1, 1), obj(1, 2)],
        },
    );
    let move_id = 0x000D_0012;
    let out = cs.apply(
        &cell,
        &Command::Order {
            player: 1,
            flags: 0,
            id: move_id,
            target: OrderTarget::Point(point(128.0, 32.0)),
        },
    );
    assert_eq!(
        out,
        Outcome::Ordered {
            accepted: 2,
            refused: 0,
            unresolved: 0
        }
    );
    assert_eq!(cell.borrow().unit_order_ids(a), vec![move_id]);
    assert_eq!(cell.borrow().unit_order_ids(b), vec![move_id]);

    // A queued order joins behind the current move instead of replacing it.
    let smart = 0x000D_0003;
    let out = cs.apply(
        &cell,
        &Command::Order {
            player: 1,
            flags: FLAG_QUEUED,
            id: smart,
            target: OrderTarget::Point(point(0.0, 0.0)),
        },
    );
    assert!(matches!(out, Outcome::Ordered { accepted: 2, .. }));
    assert_eq!(cell.borrow().unit_order_ids(a), vec![move_id, smart]);

    // A replacing order clears the list first.
    cs.apply(
        &cell,
        &Command::Order {
            player: 1,
            flags: 0,
            id: 0x000D_0019,
            target: OrderTarget::None,
        },
    );
    assert_eq!(cell.borrow().unit_order_ids(a), vec![0x000D_0019]);
}

#[test]
fn object_target_needs_a_registered_pair() {
    let (host, mut cs, a, b) = rig();
    let cell = host.game_handle();
    cs.apply(
        &cell,
        &Command::Select {
            player: 1,
            add: true,
            units: vec![obj(1, 1)],
        },
    );
    let attack = 0x000D_000F;
    let out = cs.apply(
        &cell,
        &Command::Order {
            player: 1,
            flags: 0,
            id: attack,
            target: OrderTarget::Object {
                object: obj(5, 5),
                point: None,
            },
        },
    );
    assert_eq!(
        out,
        Outcome::Ordered {
            accepted: 0,
            refused: 0,
            unresolved: 1
        }
    );
    assert!(cell.borrow().unit_order_ids(a).is_empty());
    // With the pair registered to the other footman, the attack is a target order at that unit.
    cs.register_object(obj(5, 5), b);
    let out = cs.apply(
        &cell,
        &Command::Order {
            player: 1,
            flags: 0,
            id: attack,
            target: OrderTarget::Object {
                object: obj(5, 5),
                point: None,
            },
        },
    );
    assert!(matches!(out, Outcome::Ordered { unresolved: 0, .. }));
    assert_eq!(cell.borrow().unit_order_ids(a), vec![attack]);
}

#[test]
fn order_with_no_selection_is_accepted_by_nobody() {
    let (host, mut cs, _, _) = rig();
    let cell = host.game_handle();
    let out = cs.apply(
        &cell,
        &Command::Order {
            player: 1,
            flags: 0,
            id: 0x000D_0012,
            target: OrderTarget::Point(point(1.0, 1.0)),
        },
    );
    assert_eq!(
        out,
        Outcome::Ordered {
            accepted: 0,
            refused: 0,
            unresolved: 0
        }
    );
}

#[test]
fn speed_is_recorded_and_clamped_to_fast() {
    let (host, mut cs, _, _) = rig();
    let cell = host.game_handle();
    assert_eq!(cs.speed_setting(), 1, "normal before any command");
    assert_eq!(
        cs.apply(&cell, &Command::SetSpeed { player: 1, setting: 0 }),
        Outcome::Speed { setting: 0 }
    );
    assert_eq!(cs.speed_setting(), 0);
    assert_eq!(
        cs.apply(&cell, &Command::SetSpeed { player: 1, setting: 9 }),
        Outcome::Speed { setting: 2 }
    );
    assert_eq!(cs.speed_setting(), 2);
}

#[test]
fn unmodelled_actions_are_recorded() {
    let (host, mut cs, _, _) = rig();
    let cell = host.game_handle();
    assert_eq!(
        cs.apply(&cell, &Command::Recorded { player: 1, id: 0x6B }),
        Outcome::Recorded
    );
    assert_eq!(
        cs.apply(&cell, &Command::PreSubselection { player: 1 }),
        Outcome::Recorded
    );
    assert_eq!(cs.applied(), 2);
}

#[test]
fn unregistered_unit_resolves_by_the_model_pair() {
    // No `register_object` for these units: the model pair `(handle id, 0)` finds them (sim::commands, UNSURE).
    let (host, mut cs, a, _b) = rig();
    let cell = host.game_handle();
    let pair = sim::commands::model_pair(&cell.borrow(), a);
    assert_eq!(pair.id2, 0);
    let out = cs.apply(
        &cell,
        &Command::Select {
            player: 3,
            add: true,
            units: vec![pair],
        },
    );
    assert_eq!(out, Outcome::Selected { now: 1, unresolved: 0 });
    assert_eq!(cs.selection(3), &[a]);
    // A non-zero second word is not the model's pair: unresolved, no effect.
    let out = cs.apply(
        &cell,
        &Command::Select {
            player: 4,
            add: true,
            units: vec![ObjectRef {
                id1: pair.id1,
                id2: 7,
            }],
        },
    );
    assert_eq!(out, Outcome::Selected { now: 0, unresolved: 1 });
    // A handle that is no unit (the null handle) is unresolved.
    let out = cs.apply(
        &cell,
        &Command::Select {
            player: 5,
            add: true,
            units: vec![ObjectRef { id1: 0, id2: 0 }],
        },
    );
    assert_eq!(out, Outcome::Selected { now: 0, unresolved: 1 });
}
