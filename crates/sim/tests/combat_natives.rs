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
use sim::lockstep::{TICKS_PER_BATCH, TICK_INCREMENT_BITS};
use sim::{AgileClock, ScriptHost};
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
        ..Default::default()
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
                // The walk only accelerates inside the propulsion window (movement-6.md): real footman values.
                turn_speed: f(0.6),
                prop_window: F32::from_bits(0x3F86_0A92),
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

/// The steps of the hits in the duel (model values, pinned): the duel is ordered at once at the end of the
/// setup step (clock 0.1), so the first hit is 0.6 s after the order (one step earlier than when orders waited
/// for the batch), and each swing repeats one cooldown (1.0 s) after the last.
const HIT_STEPS: [usize; 4] = [6, 16, 26, 36];

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

/// Soft-float add of two f32 values, as the timers compute expiries.
fn soft_add(a: f32, b: f32) -> f32 {
    arith::add(F32::from_bits(a.to_bits()), F32::from_bits(b.to_bits())).to_f32()
}

/// The step (1-based, as the test counts them) in which the first scan after tick 80 acquires. The scan timer
/// is armed at tick 0 (setup runs at once, lockstep.md section 12.1) with expiry clock(0) + 0.5, then re-armed
/// every 0.5 s with the soft-float add.
/// A timer fires at the first tick whose clock reaches its expiry. Step k ends at tick 20 * (k + 1).
fn first_acquire_step() -> usize {
    let clock_at = |ticks: u64| {
        let mut c = AgileClock::new();
        for _ in 0..ticks {
            c.advance(TICK_INCREMENT_BITS);
        }
        c.now()
    };
    let fires_at = |expiry: f32| (1u64..).find(|&t| clock_at(t) >= expiry).expect("reached");
    let mut expiry = soft_add(clock_at(0), 0.5);
    let mut tick = fires_at(expiry);
    while tick <= 80 {
        expiry = soft_add(expiry, 0.5);
        tick = fires_at(expiry);
    }
    tick.div_ceil(TICKS_PER_BATCH) as usize - 1
}

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
        Some(first_acquire_step()),
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
    fn roll(&mut self, _g: &mut Game, _a: ObjKey, _t: ObjKey, _w: &AttackType) -> F32 {
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
        ..Default::default()
    });
    let ua = unit_key(&host, "ua");
    // The expected draw: the same stream state, one step of stream 9, taken before the order is issued. ua's
    // order runs first in the thread, so its reaction takes the first draw of stream 9.
    let before = host.rng().clone();
    let mut expected = before.clone();
    let u = expected.stream_mut(9).random_fraction(&table);
    let delay = arith::mul(u, f(0.5));
    // `duel` runs at once, so the reaction is armed at the game time of `start`, before the step.
    host.start("duel", &[]).expect("duel");
    let now = host.timers().now();
    host.step();
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

/// A missile whose target is removed in flight deals no damage: the arrival gate needs a live target (missiles.rs).
#[test]
fn missile_into_a_target_removed_in_flight_deals_nothing() {
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
    host.start("attack_one", &[]).expect("order");
    let mut was_pending = false;
    let mut in_flight = false;
    for _ in 1..=40 {
        host.step();
        let pending = host
            .game()
            .attack_view(ua)
            .is_some_and(|v| v.execute_expiry.is_some());
        if was_pending && !pending {
            // The missile is in the air: the target leaves the game before it lands.
            host.game_mut().unit_mut(ub).removed = true;
            in_flight = true;
            break;
        }
        was_pending = pending;
    }
    assert!(in_flight, "the execute point passed");
    for _ in 0..40 {
        host.step();
    }
    assert_eq!(life(&host, ub), 100.0, "a removed target takes no missile damage");
}

/// Runs the missile duel with the given launch data and returns the steps from the execute point to the impact.
fn missile_flight_steps(data: sim::combat::MissileData) -> usize {
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
    host.game_mut().set_missile_data(FOOTMAN, data);
    host.start("attack_one", &[]).expect("order");
    let (mut execute_step, mut impact_step, mut was_pending) = (None, None, false);
    for step in 1..=60 {
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
    impact_step.expect("impact") - execute_step.expect("execute point")
}

/// The projectile starts at the shooter's position plus the launch offset turned by its facing (`0x6F29E6A0`): a
/// 50-unit forward offset halves the 100-unit flight. A homing missile flies to the target's current cell position
/// (`0x6F497260`), which for a standing target takes as long as the plain flight.
#[test]
fn the_launch_offset_shortens_the_flight_and_homing_flies_to_the_target() {
    if tables().is_none() {
        return;
    }
    use sim::combat::MissileData;
    assert_eq!(missile_flight_steps(MissileData::default()), 10);
    let forward = MissileData {
        launch: [f(50.0), F32::ZERO, F32::ZERO],
        ..Default::default()
    };
    assert_eq!(missile_flight_steps(forward), 5);
    let homing = MissileData {
        homing: true,
        ..Default::default()
    };
    assert_eq!(missile_flight_steps(homing), 10);
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
        ..Default::default()
    });
    host.start("attack_one", &[]).expect("order");
    host.step();
    let facing = host.game().unit(ua).facing.to_f32();
    assert!(
        facing.abs() < 0.01,
        "ua turned to face ub (facing {facing})"
    );
}

/// A cliff provider where the level is the x cell: a unit to the east stands higher than one to the west.
#[derive(Debug)]
struct EastIsHigher;

impl sim::combat::CliffLevels for EastIsHigher {
    fn level(&self, cell_x: i32, _cell_y: i32) -> i32 {
        cell_x
    }
}

/// A host for a missile attack of `ua` on `ub` (distance 100, speed 100) with the real RNG, seeded, and `ChanceToMiss`.
fn missile_host(chance: f32) -> Option<(ScriptHost, RngTable)> {
    tables()?;
    let table = rng_table()?;
    let mut host = host_for(DUEL);
    host.set_rng_table(table.clone());
    host.start_game_rng(0x0BAD_CAFE);
    host.start("setup", &[]).expect("setup");
    host.step();
    host.game_mut().set_attack_type(
        FOOTMAN,
        AttackType {
            missile_speed: Some(f(100.0)),
            ..weapon()
        },
    );
    host.game_mut().set_combat_misc(CombatMisc {
        chance_to_miss: f(chance),
        ..Default::default()
    });
    Some((host, table))
}

/// The steps until `ua`'s first execute point has passed.
fn run_to_first_execute(host: &mut ScriptHost, ua: ObjKey) {
    let mut was_pending = false;
    for _ in 1..=40 {
        host.step();
        let pending = host
            .game()
            .attack_view(ua)
            .is_some_and(|v| v.execute_expiry.is_some());
        if was_pending && !pending {
            return;
        }
        was_pending = pending;
    }
    panic!("no execute point");
}

/// Every missile launch takes exactly one stream-2 draw (the `ChanceToMiss` tail of the miss gate), whatever the
/// Misc value; a direct (melee) attack takes none. The placeholder damage model rolls no dice, so the draw is the only
/// stream-2 use (needs `GAME_DLL`).
#[test]
fn a_missile_launch_takes_one_stream_two_draw() {
    for chance in [0.0f32, 0.25, 1.0] {
        let Some((mut host, table)) = missile_host(chance) else {
            return;
        };
        let ua = unit_key(&host, "ua");
        host.start("attack_one", &[]).expect("order");
        let before = host.rng().clone();
        run_to_first_execute(&mut host, ua);
        let mut expected = before.clone();
        let _ = expected.stream_mut(2).random_fraction(&table);
        let after = host.rng().clone();
        assert_eq!(
            after.stream(2),
            expected.stream(2),
            "ChanceToMiss {chance}: exactly one draw of stream 2"
        );
    }
    // A melee attack (no missile speed) draws nothing from stream 2.
    let Some((mut host, _)) = missile_host(0.5) else {
        return;
    };
    host.game_mut().set_attack_type(FOOTMAN, weapon());
    let ua = unit_key(&host, "ua");
    host.start("attack_one", &[]).expect("order");
    let before = host.rng().clone();
    run_to_first_execute(&mut host, ua);
    assert_eq!(
        host.rng().stream(2),
        before.stream(2),
        "a direct attack has no miss draw"
    );
}

/// With `ChanceToMiss` 1.0 and the target on higher ground the draw (always below 1.0) misses: no damage. On level
/// ground, or with a flying attacker, or with chance 0.0, the missile lands (needs `GAME_DLL`).
#[test]
fn uphill_missiles_miss_with_the_misc_chance() {
    if tables().is_none() || rng_table().is_none() {
        return;
    }
    let landed = |chance: f32, cliffs: bool, flyer: bool| -> bool {
        let (mut host, _) = missile_host(chance).expect("host");
        if cliffs {
            host.game_mut().set_cliff_levels(Some(Box::new(EastIsHigher)));
        }
        if flyer {
            let mut g = host.game_mut();
            let mut ty = g.unit_types().get(FOOTMAN).cloned().expect("type");
            ty.movement = 2;
            g.unit_types_mut().insert(FOOTMAN, ty);
        }
        let ub = unit_key(&host, "ub");
        host.start("attack_one", &[]).expect("order");
        for _ in 0..25 {
            host.step();
        }
        life(&host, ub) < 100.0
    };
    assert!(!landed(1.0, true, false), "uphill with chance 1.0 misses");
    assert!(landed(1.0, false, false), "level ground never misses");
    assert!(landed(0.0, true, false), "chance 0.0 never misses");
    assert!(landed(1.0, true, true), "a flying attacker is exempt");
}

/// The uphill rule fed by a real terrain grid (`Game::set_terrain`, cliff layer of the nearest corner): the attacker at
/// cell 0 stands on layer 4, the target at cell 3 (corner column 1) on layer 5, so a ChanceToMiss of 1.0 misses;
/// with the target on a lower layer it lands (needs `GAME_DLL`).
#[test]
fn uphill_miss_from_a_terrain_grid() {
    use sim::terrain::{Corner, TerrainGrid};
    if tables().is_none() || rng_table().is_none() {
        return;
    }
    let landed = |target_layer: u8| -> bool {
        let (mut host, _) = missile_host(1.0).expect("host");
        let corners: Vec<Corner> = (0..12)
            .map(|i| {
                let col = i % 4;
                Corner::from_file(0x2000, 0x2000, 0, if col == 0 { 4 } else { target_layer })
            })
            .collect();
        let grid = TerrainGrid::new(4, 3, [0.0, 0.0], corners).expect("grid");
        host.game_mut().set_terrain(Some(std::sync::Arc::new(grid)));
        let ub = unit_key(&host, "ub");
        host.start("attack_one", &[]).expect("order");
        for _ in 0..25 {
            host.step();
        }
        life(&host, ub) < 100.0
    };
    assert!(!landed(5), "the target one layer higher: the missile misses");
    assert!(landed(3), "the target lower: the missile lands");
}
