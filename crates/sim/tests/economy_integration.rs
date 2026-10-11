//! Gold and lumber through the integrated paths (`natives/economy.rs`, `natives/player.rs`, `economy.rs`): one store
//! shared by `SetPlayerState` and the changer, the upkeep rates from the constants, cancel refunds by kind, and gold
//! mines with the harvest. Synthetic setups only: no game files are read. Spec: `docs/spec/economy-3.md`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::economy::{EconomyConstants, Flags, MineNotice, RefundKind, Res, UpkeepTable, Worker};
use sim::natives::game::ObjKey;
use sim::{Game, ScriptHost};
use softfloat::F32;

const MINE: ObjKey = ObjKey(7);

fn rate(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// Defaults of the upkeep table (thresholds 40, 70, then 10000; gold tax 0, 0.3, then 0.6) with refund rates that
/// are exact in binary, and a low-gold threshold of 10.
fn constants() -> EconomyConstants {
    EconomyConstants {
        upkeep: UpkeepTable::default(),
        train_refund_rate: rate(0.5),
        research_refund_rate: rate(0.25),
        revive_refund_rate: rate(0.125),
        low_gold_amount: 10,
    }
}

fn game() -> Game {
    let mut g = Game::new();
    g.set_economy_constants(constants());
    g
}

const PRELUDE: &str = r#"
type player extends handle
type playerstate extends handle
native Player takes integer i returns player
native SetPlayerState takes player p, playerstate s, integer v returns nothing
native GetPlayerState takes player p, playerstate s returns integer
native ConvertPlayerState takes integer i returns playerstate
"#;

#[test]
fn tax_follows_the_food_used_through_the_constants() {
    let mut g = game();
    // Food 0 is tier 0 (tax 0): 10 gold credited in full, stored x10.
    assert_eq!(g.credit_resource(0, Res::Gold, 10, Flags::INCOME), Some(10));
    // Food 50 is tier 1 (30 percent): 3 of 10 goes to the tax, 7 is credited.
    g.refresh_upkeep(0, 50);
    assert_eq!(g.credit_resource(0, Res::Gold, 10, Flags::INCOME), Some(7));
    // Food 200 is past every threshold: tier 2 (60 percent): 4 is credited.
    g.refresh_upkeep(0, 200);
    assert_eq!(g.credit_resource(0, Res::Gold, 10, Flags::INCOME), Some(4));
    // Stored: 100 + 70 + 40.
    assert_eq!(g.player_economy(0).unwrap().stored(Res::Gold), 210);
}

#[test]
fn set_economy_constants_refreshes_every_slot_for_food_zero() {
    let mut c = constants();
    c.upkeep.gold_tax[0] = rate(0.5);
    let mut g = Game::new();
    g.set_economy_constants(c);
    for p in 0..16 {
        assert_eq!(g.player_economy(p).unwrap().tax_percent[0], 50, "slot {p}");
    }
    assert!(g.player_economy(16).is_none());
}

#[test]
fn ally_share_is_paid_first_and_the_tax_comes_from_what_is_left() {
    let mut g = game();
    g.player_economy_mut(0).unwrap().ally_share[0][1] = 50;
    g.refresh_upkeep(0, 50); // 30 percent tax
    // 10 gold in: 50 percent (5 gold) to slot 1 with no tax of its own; tax 3 from the 5 left; 2 credited.
    assert_eq!(g.credit_resource(0, Res::Gold, 10, Flags::INCOME), Some(2));
    assert_eq!(g.player_economy(1).unwrap().stored(Res::Gold), 50);
    assert_eq!(g.player_economy(0).unwrap().stored(Res::Gold), 20);
}

#[test]
fn player_state_and_the_changer_share_one_store() {
    let src = r#"
globals
    integer gBefore = -1
    integer gAfter = -1
endglobals
function Main takes nothing returns nothing
    call SetPlayerState(Player(0), ConvertPlayerState(1), 500)
endfunction
function Later takes nothing returns nothing
    set gBefore = GetPlayerState(Player(0), ConvertPlayerState(1))
    set gAfter = GetPlayerState(Player(0), ConvertPlayerState(1))
endfunction
"#;
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    host.start("Main", &[]).expect("start");
    host.step();
    // The changer credits 10 gold (no tax at food 0): the same field the script reads.
    assert_eq!(
        host.game_mut()
            .credit_resource(0, Res::Gold, 10, Flags::NONE),
        Some(10)
    );
    host.start("Later", &[]).expect("start");
    host.step();
    let got = host.vm().global("gBefore").and_then(|v| v.as_integer());
    assert_eq!(got, Some(510));
    // SetPlayerState writes the stored value directly: the next read is the same.
    let again = host.vm().global("gAfter").and_then(|v| v.as_integer());
    assert_eq!(again, Some(510));
}

#[test]
fn cancel_refunds_use_the_rate_of_their_kind_and_no_tax() {
    let mut g = game();
    g.refresh_upkeep(0, 200); // 60 percent gold tax: a refund must not pay it
    let (gold, lumber) = g.refund_cancelled(0, RefundKind::Train, 400, 200, true);
    assert_eq!((gold, lumber), (200, 100));
    let (gold, lumber) = g.refund_cancelled(0, RefundKind::Research, 400, 0, true);
    assert_eq!((gold, lumber), (100, 0));
    let (gold, lumber) = g.refund_cancelled(0, RefundKind::Revive, 100, 100, true);
    assert_eq!((gold, lumber), (12, 12));
    let e = g.player_economy(0).unwrap();
    assert_eq!(e.stored(Res::Gold), 2000 + 1000 + 120);
    assert_eq!(e.stored(Res::Lumber), 1000 + 120);
    // A slot whose bit in the mask is set refunds nothing (the counter is not modelled).
    assert_eq!(
        g.refund_cancelled(0, RefundKind::Train, 400, 200, false),
        (0, 0)
    );
    assert_eq!(g.player_economy(0).unwrap().stored(Res::Gold), 3120);
}

#[test]
fn gold_mine_feeds_the_worker_and_the_mine_runs_out() {
    let mut g = game();
    g.add_resource_node(MINE, 25);
    let mut w = Worker::new(0, 10, 20, 5);
    w.mining = true;

    let trip = g.gold_trip(&mut w, MINE, true).expect("a mine");
    assert_eq!(trip, (10, MineNotice::None));
    assert_eq!(g.resource_amount(MINE), Some(15));
    assert_eq!(g.deposit_worker(&mut w), 10);
    assert_eq!(g.player_economy(0).unwrap().stored(Res::Gold), 100);

    // 15 left, above the threshold of 10, takes 10: 5 left, falling to the low-gold notice.
    assert_eq!(
        g.gold_trip(&mut w, MINE, true),
        Some((10, MineNotice::Low))
    );
    g.deposit_worker(&mut w);

    // 5 left takes 5: empty, the depleted notice.
    assert_eq!(
        g.gold_trip(&mut w, MINE, true),
        Some((5, MineNotice::Depleted))
    );
    assert_eq!(g.resource_amount(MINE), Some(0));
    assert_eq!(g.deposit_worker(&mut w), 5);
    assert_eq!(g.player_economy(0).unwrap().stored(Res::Gold), 100 + 100 + 50);
}

#[test]
fn a_hidden_mine_gives_no_notice_but_still_pays() {
    let mut g = game();
    g.add_resource_node(MINE, 5);
    let mut w = Worker::new(0, 10, 20, 5);
    w.mining = true;
    assert_eq!(g.gold_trip(&mut w, MINE, false), Some((5, MineNotice::None)));
    assert_eq!(w.carry, 5);
}

#[test]
fn a_gold_trip_needs_a_mine_and_a_valid_owner() {
    let mut g = game();
    let mut w = Worker::new(0, 10, 20, 5);
    w.mining = true;
    assert_eq!(g.gold_trip(&mut w, MINE, true), None, "no mine registered");
    g.add_resource_node(MINE, 25);
    let mut stray = Worker::new(20, 10, 20, 5);
    stray.mining = true;
    assert_eq!(g.gold_trip(&mut stray, MINE, true), None, "owner 20 is no slot");
    assert_eq!(g.deposit_worker(&mut stray), 0);
    assert_eq!(g.credit_resource(16, Res::Gold, 10, Flags::NONE), None);
    assert_eq!(g.refund_cancelled(16, RefundKind::Train, 400, 200, true), (0, 0));
}
