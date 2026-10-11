//! The attack cycle through `ScriptHost` (`combat.rs`, spec `docs/spec/attack-cycle.md` and `attack-cycle-2.md`).
//!
//! Tests that need the trigonometry or division tables (walking to a target, the missile flight, the attack-move walk)
//! return early without `GAME_DLL`. The reaction-delay test compares the stream-9 draw with the real RNG table, so it
//! needs `GAME_DLL` too. Every other test runs with the model's own timings: the expected steps are pinned values of
//! the model (UNVERIFIED against the original), so a change to the cycle shows up here.

use std::cell::RefCell;
use std::rc::Rc;

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::combat::{AttackType, CombatMisc, DamageModel};
use sim::natives::game::{Game, ObjKey, UnitEvent, UnitType};
use sim::orders::{AbilityKind, TypeOrderData, ORDER_ATTACK};
use sim::rng::RngTable;
use sim::ScriptHost;
use softfloat::{arith, Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean
"#;

const FOOTMAN: u32 = 0x6866_6F6F;

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn rng_table() -> Option<RngTable> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL"))
}

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

/// The weapon of the tests: cooldown 1.0 s, damage point 0.5 s, backswing 0.5 s, range 128 world units, collision
/// 0.5 cells, placeholder damage 10.
fn weapon() -> AttackType {
    AttackType {
        cooldown: f(1.0),
        damage_point: f(0.5),
        backswing: f(0.5),
        range: f(128.0),
        collision: f(0.5),
        damage: f(10.0),
        missile_speed: None,
    }
}

/// A host with the footman type: move and attack abilities, speed 270, 100 life, the test weapon.
fn host_for(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), tables()).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                name: "Footman".to_string(),
                max_life: f(100.0),
                move_speed: f(270.0),
                ..Default::default()
            },
        );
        g.set_type_orders(
            FOOTMAN,
            TypeOrderData {
                abilities: vec![AbilityKind::Move, AbilityKind::Attack],
            },
        );
        g.set_attack_type(FOOTMAN, weapon());
    }
    host
}

fn run(host: &mut ScriptHost, n: usize) {
    for _ in 0..n {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

/// The unit key of the JASS global `name`.
fn unit_key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"));
    host.game()
        .unit_of_handle(id)
        .unwrap_or_else(|| panic!("global {name} is not a live unit"))
}

fn life(host: &ScriptHost, k: ObjKey) -> f32 {
    host.game().unit(k).life.to_f32()
}

fn order_ids(host: &ScriptHost, k: ObjKey) -> Vec<u32> {
    host.game().unit_order_ids(k)
}

/// Two footmen 100 world units apart: `ua` (player 0) at the origin, `ub` (player 1) at x = 100. `duel` gives both a
/// target order on the other; `amove_point` gives `ua` an attack-move to x = 400. Setup runs in the first step.
const DUEL: &str = r#"
globals
    unit ua = null
    unit ub = null
endglobals
function setup takes nothing returns nothing
    set ua = CreateUnit(Player(0), 'hfoo', 0, 0, 0)
    set ub = CreateUnit(Player(1), 'hfoo', 100, 0, 180)
endfunction
function duel takes nothing returns nothing
    call IssueTargetOrder(ua, "attack", ub)
    call IssueTargetOrder(ub, "attack", ua)
endfunction
function attack_one takes nothing returns nothing
    call IssueTargetOrder(ua, "attack", ub)
endfunction
function amove_point takes nothing returns nothing
    call IssuePointOrder(ua, "attack", 400, 0)
endfunction
"#;

fn duel_host() -> ScriptHost {
    let mut host = host_for(DUEL);
    host.start("setup", &[]).expect("setup");
    host.step();
    host
}

/// The steps of the hits in the duel (model values, pinned): the swing starts on the first step, the execute point
/// is 0.5 s later, and each swing repeats one cooldown (1.0 s) after the last.
const HIT_STEPS: [usize; 4] = [7, 17, 27, 37];

#[test]
fn mutual_attack_hits_at_pinned_steps() {
    let mut host = duel_host();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    host.start("duel", &[]).expect("duel");
    let (mut hits_on_a, mut hits_on_b) = (Vec::new(), Vec::new());
    let (mut la, mut lb) = (life(&host, ua), life(&host, ub));
    for step in 1..=40 {
        host.step();
        let (na, nb) = (life(&host, ua), life(&host, ub));
        if nb < lb {
            hits_on_b.push(step);
        }
        if na < la {
            hits_on_a.push(step);
        }
        la = na;
        lb = nb;
    }
    assert_eq!(hits_on_b, HIT_STEPS, "hits on ub");
    assert_eq!(hits_on_a, HIT_STEPS, "hits on ua, the same steps");
    assert_eq!(life(&host, ub), 100.0 - 10.0 * HIT_STEPS.len() as f32);
}

#[test]
fn cooldown_spaces_the_swings_by_the_cooldown() {
    let mut host = duel_host();
    let ub = unit_key(&host, "ub");
    host.start("duel", &[]).expect("duel");
    let mut hit_times = Vec::new();
    let mut last = life(&host, ub);
    for _ in 0..40 {
        host.step();
        let now = life(&host, ub);
        if now < last {
            hit_times.push(host.timers().now());
        }
        last = now;
    }
    assert_eq!(hit_times.len(), HIT_STEPS.len());
    for pair in hit_times.windows(2) {
        // Consecutive hits are one cooldown (1.0 s) apart, up to the agile clock's drift.
        let gap = pair[1] - pair[0];
        assert!((gap - 1.0).abs() < 0.11, "gap {gap} is the cooldown");
    }
}

#[test]
fn death_stops_the_cycle() {
    let mut host = duel_host();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    // One hit kills ub.
    host.game_mut().unit_mut(ub).life = f(10.0);
    host.start("duel", &[]).expect("duel");
    run(&mut host, 60);
    assert!(host.game().unit(ub).dead, "ub died from the first hit");
    assert_eq!(life(&host, ub), 0.0);
    // ub's first swing is the only one that can land before it dies.
    let hits_on_a = (100.0 - life(&host, ua)) / 10.0;
    assert!(hits_on_a <= 1.0, "ub made at most one hit: {hits_on_a}");
    // ua's order is finished and nothing is scheduled for it any more.
    assert!(order_ids(&host, ua).is_empty(), "ua stopped attacking");
    let view = host
        .game()
        .attack_view(ua)
        .expect("ua has an attack object");
    assert!(view.execute_expiry.is_none() && view.reaction_expiry.is_none());
    assert!(host
        .game_mut()
        .take_unit_events()
        .contains(&UnitEvent::Death(ub)));
}

/// The step of the first acquisition in [`idle_unit_acquires_an_enemy_that_enters_range_at_the_next_scan`] (model).
const FIRST_ACQUIRE_STEP: usize = 5;

/// An idle unit with an acquire range scans every 0.5 s and takes an enemy that moved into range.
#[test]
fn idle_unit_acquires_an_enemy_that_enters_range_at_the_next_scan() {
    let mut host = host_for(DUEL);
    host.start("setup", &[]).expect("setup");
    host.step();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    // ua is idle (no order); ub is outside ua's acquire range of 300 at x = 400.
    host.game_mut().unit_mut(ua).acquire_range = f(300.0);
    host.game_mut().unit_mut(ub).x = f(400.0);
    let mut first_order_at = None;
    for step in 1..=12 {
        if step == 4 {
            // Between the scans, ub walks into acquire range (x = 100).
            host.game_mut().unit_mut(ub).x = f(100.0);
        }
        host.step();
        if first_order_at.is_none() && !order_ids(&host, ua).is_empty() {
            first_order_at = Some(step);
        }
    }
    assert_eq!(
        first_order_at,
        Some(FIRST_ACQUIRE_STEP),
        "the scan after the move acquires"
    );
    assert_eq!(order_ids(&host, ua)[0], ORDER_ATTACK);
    assert_eq!(host.game().attack_view(ua).expect("view").target, Some(ub));
}

/// Acquisition order: candidates in the cell walk, x-major and ascending. Enemies `far` (x = -120, cell -4) and
/// `near` (x = 10, cell 0). An ally of ua (x = -150, cell -5) comes first in the walk and must be skipped. The scan
/// takes the first enemy in that order, so `far` wins although `near` is closer.
const CELL_ORDER: &str = r#"
globals
    unit ua = null
    unit ally = null
    unit far = null
    unit near = null
endglobals
function setup takes nothing returns nothing
    set ua = CreateUnit(Player(0), 'hfoo', 0, 0, 0)
    set ally = CreateUnit(Player(0), 'hfoo', -150, 0, 0)
    set far = CreateUnit(Player(1), 'hfoo', -120, 0, 0)
    set near = CreateUnit(Player(1), 'hfoo', 10, 0, 0)
endfunction
"#;

#[test]
fn first_candidate_in_cell_order_wins() {
    let mut host = host_for(CELL_ORDER);
    host.start("setup", &[]).expect("setup");
    host.step();
    let ua = unit_key(&host, "ua");
    let far = unit_key(&host, "far");
    let near = unit_key(&host, "near");
    host.game_mut().unit_mut(ua).acquire_range = f(300.0);
    run(&mut host, 8);
    let view = host.game().attack_view(ua).expect("view");
    assert_eq!(
        view.target,
        Some(far),
        "the first enemy in cell order, not the nearest"
    );
    assert_ne!(view.target, Some(near));
}

/// A damage model that deals 3 per execute and counts its calls.
#[derive(Debug)]
struct ThreeDamage {
    calls: Rc<RefCell<u32>>,
}

impl DamageModel for ThreeDamage {
    fn roll(&mut self, _g: &Game, _a: ObjKey, _t: ObjKey, _w: &AttackType) -> F32 {
        *self.calls.borrow_mut() += 1;
        f(3.0)
    }
}

#[test]
fn damage_model_is_pluggable_and_called_once_per_execute() {
    let mut host = duel_host();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    let calls = Rc::new(RefCell::new(0));
    host.game_mut().set_damage_model(Box::new(ThreeDamage {
        calls: Rc::clone(&calls),
    }));
    host.start("duel", &[]).expect("duel");
    run(&mut host, 40);
    // Four execute points on each side: eight rolls, three life per roll.
    assert_eq!(*calls.borrow(), 8);
    assert_eq!(life(&host, ub), 100.0 - 12.0);
    assert_eq!(life(&host, ua), 100.0 - 12.0);
}

/// An attacker out of range walks to its target and then swings (needs the tables: the walk is the mover).
#[test]
fn out_of_range_attacker_walks_then_attacks() {
    if tables().is_none() {
        return;
    }
    let mut host = host_for(DUEL);
    host.start("setup", &[]).expect("setup");
    host.step();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    host.game_mut().unit_mut(ub).x = f(400.0);
    host.start("duel", &[]).expect("duel");
    // Both sides have the order and both walk; the first hit lands once ua closes the gap.
    let mut hit_at = None;
    for step in 1..=80 {
        host.step();
        if hit_at.is_none() && life(&host, ub) < 100.0 {
            hit_at = Some(step);
        }
    }
    assert!(hit_at.is_some(), "ua hit ub after walking");
    let (xa, xb) = (
        host.game().unit(ua).x.to_f32(),
        host.game().unit(ub).x.to_f32(),
    );
    assert!(xa > 0.0, "ua walked towards ub");
    assert!(
        xb - xa < 170.0,
        "they met inside the swing range (gap {})",
        xb - xa
    );
}

/// The reaction delay is one draw of stream 9 times the Misc `ReactionDelay` (needs `GAME_DLL` for the real table).
#[test]
fn reaction_delay_draws_from_stream_nine() {
    let Some(table) = rng_table() else {
        return;
    };
    let mut host = duel_host();
    host.set_rng_table(table.clone());
    host.start_game_rng(0x1234_5678);
    host.game_mut().set_combat_misc(CombatMisc {
        attack_half_angle: None,
        reaction_delay: f(0.5),
    });
    let ua = unit_key(&host, "ua");
    // The expected draw: the same stream state, one step of stream 9, taken before the order is issued. ua's
    // order runs first in the thread, so its reaction takes the first draw of stream 9.
    let before = host.rng().clone();
    let mut expected = before.clone();
    let u = expected.stream_mut(9).random_fraction(&table);
    let delay = arith::mul(u, f(0.5));
    host.start("duel", &[]).expect("duel");
    host.step();
    let now = host.timers().now();
    let reaction = host
        .game()
        .attack_view(ua)
        .expect("view")
        .reaction_expiry
        .expect("the reaction delay is pending");
    assert_eq!(
        reaction.to_bits(),
        arith::add(F32::from_bits(now.to_bits()), delay).to_bits(),
        "reaction expiry = now + U * ReactionDelay"
    );
    let after = host.rng().clone();
    for k in 0..sim::STREAM_COUNT {
        if k != 9 {
            assert_eq!(after.stream(k), before.stream(k), "stream {k} untouched");
        }
    }
    assert_ne!(after.stream(9), before.stream(9), "stream 9 advanced");
    assert_eq!(
        after.global, before.global,
        "the global generator is untouched"
    );
}

/// A missile attack lands on impact: distance 100 at speed 100 is 1.0 s, ten steps after the execute point.
#[test]
fn missile_damage_lands_after_the_flight_time() {
    if tables().is_none() {
        return;
    }
    let mut host = host_for(DUEL);
    host.start("setup", &[]).expect("setup");
    host.step();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    host.game_mut().set_attack_type(
        FOOTMAN,
        AttackType {
            missile_speed: Some(f(100.0)),
            ..weapon()
        },
    );
    // Only ua acts, and it does not walk: a target order on ub, which stands at x = 100 inside the swing range.
    host.start("attack_one", &[]).expect("order");
    let mut execute_step = None;
    let mut impact_step = None;
    let mut was_pending = false;
    for step in 1..=40 {
        host.step();
        let pending = host
            .game()
            .attack_view(ua)
            .is_some_and(|v| v.execute_expiry.is_some());
        if was_pending && !pending && execute_step.is_none() {
            execute_step = Some(step);
        }
        was_pending = pending;
        if impact_step.is_none() && life(&host, ub) < 100.0 {
            impact_step = Some(step);
        }
    }
    let (execute, impact) = (
        execute_step.expect("execute point"),
        impact_step.expect("impact"),
    );
    assert_eq!(impact, execute + 10, "impact after the 1.0 s flight");
}

/// An attack-move unit scans on its way and attacks the first enemy it finds (needs the tables for the walk).
#[test]
fn attack_move_takes_the_first_enemy_it_finds() {
    if tables().is_none() {
        return;
    }
    let mut host = host_for(DUEL);
    host.start("setup", &[]).expect("setup");
    host.step();
    let (ua, ub) = (unit_key(&host, "ua"), unit_key(&host, "ub"));
    host.game_mut().unit_mut(ua).acquire_range = f(300.0);
    host.game_mut().unit_mut(ub).x = f(250.0);
    host.start("amove_point", &[]).expect("order");
    run(&mut host, 8);
    let view = host.game().attack_view(ua).expect("view");
    assert_eq!(view.target, Some(ub), "the scan found ub while walking");
    assert_eq!(order_ids(&host, ua).first().copied(), Some(ORDER_ATTACK));
}

/// The facing gate turns an attacker that faces away from its target (instant turn, UNVERIFIED): ua faces pi, ub is at
/// +x, so after the first step ua faces 0. Needs the tables for the bearing.
#[test]
fn facing_turns_an_attacker_towards_its_target() {
    if tables().is_none() {
        return;
    }
    let mut host = host_for(DUEL);
    host.start("setup", &[]).expect("setup");
    host.step();
    let ua = unit_key(&host, "ua");
    host.game_mut().unit_mut(ua).facing = f(std::f32::consts::PI);
    host.game_mut().set_combat_misc(CombatMisc {
        attack_half_angle: Some(f(0.5)),
        reaction_delay: F32::ZERO,
    });
    host.start("attack_one", &[]).expect("order");
    host.step();
    let facing = host.game().unit(ua).facing.to_f32();
    assert!(
        facing.abs() < 0.01,
        "ua turned to face ub (facing {facing})"
    );
}
