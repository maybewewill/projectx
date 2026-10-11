//! Ability, invulnerability, buff and timed life natives through `ScriptHost` (`crate::abilities`, `natives/ability.rs`).
//!
//! The scripts use integer-valued reals (`I2R`) so they need no lookup tables. Timer expectations are computed with the
//! soft-float helpers that the timer queue uses (`softfloat::arith`), so they follow the clock as the host runs it.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{buff_predicates, BuffPredicates};
use sim::natives::game::{AbilityType, UnitType};
use sim::natives::{ObjKey, UnitEvent};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::{arith, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type location extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer
native SetUnitAbilityLevel takes unit whichUnit, integer abilcode, integer level returns integer
native IncUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer
native DecUnitAbilityLevel takes unit whichUnit, integer abilcode returns integer
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
native SetUnitInvulnerable takes unit whichUnit, boolean flag returns nothing
native UnitApplyTimedLife takes unit whichUnit, integer buffId, real duration returns nothing
native UnitPauseTimedLife takes unit whichUnit, boolean flag returns nothing
native UnitResetCooldown takes unit whichUnit returns nothing
native UnitHasBuffsEx takes unit whichUnit, boolean removePositive, boolean removeNegative, boolean magic, boolean physical, boolean timedLife, boolean aura, boolean dispellable returns boolean
native UnitCountBuffsEx takes unit whichUnit, boolean removePositive, boolean removeNegative, boolean magic, boolean physical, boolean timedLife, boolean aura, boolean dispellable returns integer
native UnitRemoveBuffs takes unit whichUnit, boolean removePositive, boolean removeNegative returns nothing
native UnitRemoveBuffsEx takes unit whichUnit, boolean removePositive, boolean removeNegative, boolean magic, boolean physical, boolean timedLife, boolean aura, boolean dispellable returns nothing
"#;

/// Footman 'hfoo'.
const FOOTMAN: u32 = 0x6866_6F6F;
/// Ability with a max level of 3.
const ABIL_MAX3: u32 = 0x4148_627A; // 'AHbz'

/// Positive buff code of the tests (its predicates are passed by the test, not by the model's table).
const BUFF_POS: u32 = 0x4250_5353; // 'BPSS'
/// Positive magic buff code.
const BUFF_MAGIC: u32 = 0x424D_4147; // 'BMAG'
/// Negative buff code.
const BUFF_NEG: u32 = 0x424E_4547; // 'BNEG'
/// Neutral buff code.
const BUFF_NEUTRAL: u32 = 0x424E_5455; // 'BNTU'

const GLOBALS: &str = r#"
globals
unit u1
integer i1 = -1
integer i2 = -1
integer i3 = -1
integer i4 = -1
integer i5 = -1
integer i6 = -1
integer i7 = -1
integer i8 = -1
integer i9 = -1
integer i10 = -1
boolean b1 = false
boolean b2 = false
boolean b3 = false
endglobals
"#;

fn host_with(src: &str, footman_moves: bool) -> ScriptHost {
    let full = format!("{PRELUDE}\n{GLOBALS}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            FOOTMAN,
            UnitType {
                name: "Footman".to_string(),
                max_life: F32::from_bits(0x43D2_0000), // 420.0
                ..Default::default()
            },
        );
        g.ability_types_mut()
            .insert(ABIL_MAX3, AbilityType { max_level: Some(3) });
        if footman_moves {
            g.set_type_orders(
                FOOTMAN,
                TypeOrderData {
                    abilities: vec![AbilityKind::Move],
                },
            );
            g.ability_types_mut()
                .insert(0x416D_6F76, AbilityType { max_level: None });
        }
    }
    host
}

fn host_for(src: &str) -> ScriptHost {
    host_with(src, false)
}

/// Starts `name` and runs `steps` host steps.
fn call(host: &mut ScriptHost, name: &str, steps: usize) {
    host.start(name, &[]).expect("thread starts");
    for _ in 0..steps {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| v.as_boolean())
        .unwrap_or_else(|| panic!("global {name} is not a boolean"))
}

/// The model key of the unit in global `name`.
fn unit_key(host: &ScriptHost, name: &str) -> ObjKey {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .expect("unit handle");
    host.game().unit_of_handle(id).expect("live unit")
}

fn is_dead(host: &ScriptHost, k: ObjKey) -> bool {
    host.game().unit(k).dead
}

#[test]
fn ability_levels_follow_the_native_rules() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set b1 = UnitAddAbility(u1, 'AHbz')
    set b2 = UnitAddAbility(u1, 'AHbz')
    set i1 = GetUnitAbilityLevel(u1, 'AHbz')
    set i2 = IncUnitAbilityLevel(u1, 'AHbz')
    set i3 = IncUnitAbilityLevel(u1, 'AHbz')
    set i4 = IncUnitAbilityLevel(u1, 'AHbz')
    set i5 = IncUnitAbilityLevel(u1, 'AHbz')
    set i6 = DecUnitAbilityLevel(u1, 'AHbz')
    set i7 = SetUnitAbilityLevel(u1, 'AHbz', -2)
    set i8 = DecUnitAbilityLevel(u1, 'AHbz')
    set i9 = SetUnitAbilityLevel(u1, 'AHbz', 9)
    set i10 = GetUnitAbilityLevel(u1, 'AHbz')
endfunction
"#,
    );
    call(&mut host, "main", 3);
    assert!(boolean(&host, "b1"));
    assert!(!boolean(&host, "b2"), "a second add fails");
    assert_eq!(int(&host, "i1"), 1);
    assert_eq!(int(&host, "i2"), 2);
    assert_eq!(int(&host, "i3"), 3);
    assert_eq!(int(&host, "i4"), 4, "inc reaches max plus one");
    assert_eq!(int(&host, "i5"), 4, "and stops there");
    assert_eq!(int(&host, "i6"), 3);
    assert_eq!(int(&host, "i7"), 1, "set clamps to at least 1");
    assert_eq!(int(&host, "i8"), 1, "dec stops at level 1");
    assert_eq!(int(&host, "i9"), 3, "set clamps to the max");
    assert_eq!(int(&host, "i10"), 3);
}

#[test]
fn invulnerability_adds_and_removes_the_ability() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set i1 = GetUnitAbilityLevel(u1, 'Avul')
    call SetUnitInvulnerable(u1, true)
    call SetUnitInvulnerable(u1, true)
    set i2 = GetUnitAbilityLevel(u1, 'Avul')
    call SetUnitInvulnerable(u1, false)
    set i3 = GetUnitAbilityLevel(u1, 'Avul')
    call SetUnitInvulnerable(u1, false)
    set i4 = GetUnitAbilityLevel(u1, 'Avul')
    call SetUnitInvulnerable(u1, true)
    set i5 = GetUnitAbilityLevel(u1, 'Avul')
    set b1 = UnitAddAbility(u1, 'Avul')
endfunction
"#,
    );
    call(&mut host, "main", 3);
    assert_eq!(int(&host, "i1"), 0, "not invulnerable at creation");
    assert_eq!(int(&host, "i2"), 1, "true adds the ability once");
    assert_eq!(int(&host, "i3"), 0, "false removes it");
    assert_eq!(int(&host, "i4"), 0, "false again does nothing");
    assert_eq!(int(&host, "i5"), 1);
    assert!(
        !boolean(&host, "b1"),
        "a factory add fails: 'Avul' has no registry record here, and the unit already has it"
    );
    let k = unit_key(&host, "u1");
    assert!(host.game().unit_is_invulnerable(k));
}

#[test]
fn invulnerability_is_read_from_the_ability_list() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call SetUnitInvulnerable(u1, true)
endfunction
"#,
    );
    call(&mut host, "main", 2);
    let k = unit_key(&host, "u1");
    assert!(host.game().unit_is_invulnerable(k));
    host.game_mut().set_unit_invulnerable(k, false);
    assert!(!host.game().unit_is_invulnerable(k));
}

#[test]
fn move_ability_is_a_fixed_slot_and_not_a_factory_ability() {
    let mut host = host_with(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set i1 = GetUnitAbilityLevel(u1, 'Amov')
    set b1 = UnitAddAbility(u1, 'Amov')
    set i2 = GetUnitAbilityLevel(u1, 'Amov')
endfunction
"#,
        true,
    );
    call(&mut host, "main", 2);
    assert_eq!(
        int(&host, "i1"),
        1,
        "the type's move order data attaches Amov at creation"
    );
    assert!(
        !boolean(&host, "b1"),
        "Amov has no factory leaf, even when registered"
    );
    assert_eq!(int(&host, "i2"), 1);
}

#[test]
fn buff_counts_and_removals_follow_the_flag_rules() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(100))
endfunction
function count takes nothing returns nothing
    set i1 = UnitCountBuffsEx(u1, false, false, false, false, false, false, false)
    set i2 = UnitCountBuffsEx(u1, true, false, false, false, false, false, false)
    set i3 = UnitCountBuffsEx(u1, false, true, false, false, false, false, false)
    set i4 = UnitCountBuffsEx(u1, true, true, false, false, false, false, false)
    set i5 = UnitCountBuffsEx(u1, false, false, false, false, true, false, false)
    set i6 = UnitCountBuffsEx(u1, false, false, true, false, false, false, false)
    set b1 = UnitHasBuffsEx(u1, false, false, false, false, false, false, false)
    set b2 = UnitHasBuffsEx(u1, false, false, false, true, false, false, false)
endfunction
function remove_none takes nothing returns nothing
    call UnitRemoveBuffs(u1, false, false)
    set i7 = UnitCountBuffsEx(u1, false, false, false, false, true, true, false)
endfunction
function remove_positive takes nothing returns nothing
    call UnitRemoveBuffs(u1, true, false)
    set i8 = UnitCountBuffsEx(u1, false, false, false, false, true, true, false)
endfunction
function remove_negative_untimed takes nothing returns nothing
    call UnitRemoveBuffsEx(u1, false, true, false, false, false, false, false)
    set i9 = UnitCountBuffsEx(u1, false, false, false, false, true, true, false)
endfunction
function remove_negative_timed takes nothing returns nothing
    call UnitRemoveBuffsEx(u1, false, true, false, false, true, false, false)
    set i10 = UnitCountBuffsEx(u1, false, false, false, false, true, true, false)
endfunction
"#,
    );
    call(&mut host, "main", 2);
    let k = unit_key(&host, "u1");
    // The buffs of the test: positive, magic positive, negative, neutral (the timed life buff is added by the script).
    host.game_mut().add_buff(
        k,
        BUFF_POS,
        BuffPredicates {
            positive: true,
            ..Default::default()
        },
    );
    host.game_mut().add_buff(
        k,
        BUFF_MAGIC,
        BuffPredicates {
            positive: true,
            magic: true,
            ..Default::default()
        },
    );
    host.game_mut().add_buff(
        k,
        BUFF_NEG,
        BuffPredicates {
            negative: true,
            ..Default::default()
        },
    );
    host.game_mut()
        .add_buff(k, BUFF_NEUTRAL, BuffPredicates::default());
    assert_eq!(host.game().buffs(k).len(), 5);
    assert_eq!(
        host.game().buffs(k)[0].predicates,
        buff_predicates(0x4254_4C46),
        "the timed life buff (added first, by the script) is negative and timed"
    );

    call(&mut host, "count", 1);
    // With every flag false the polarity test passes every buff, but timed life is excluded by its own filter (the
    // flag reads "include when true", see `abilities.rs`; UNSURE).
    assert_eq!(
        int(&host, "i1"),
        4,
        "all flags false: timed life is excluded"
    );
    assert_eq!(
        int(&host, "i2"),
        3,
        "positive only: the two positive buffs and the neutral one"
    );
    assert_eq!(
        int(&host, "i3"),
        2,
        "negative only: the untimed negative buff and the neutral one"
    );
    assert_eq!(
        int(&host, "i4"),
        4,
        "both polarities, timed life excluded (its flag is clear)"
    );
    assert_eq!(int(&host, "i5"), 5, "timed life included with its flag set");
    assert_eq!(int(&host, "i6"), 1, "magic only");
    assert!(boolean(&host, "b1"));
    assert!(!boolean(&host, "b2"), "no physical buff");

    call(&mut host, "remove_none", 1);
    // The counts from here on use timed life and aura included, so they count every buff left on the unit.
    call(&mut host, "remove_none", 1);
    assert_eq!(
        int(&host, "i7"),
        5,
        "both polarity flags false removes nothing"
    );

    call(&mut host, "remove_positive", 1);
    assert_eq!(int(&host, "i8"), 3, "the two positive buffs are removed");

    call(&mut host, "remove_negative_untimed", 1);
    assert_eq!(
        int(&host, "i9"),
        2,
        "the untimed negative buff is removed, timed life kept"
    );

    call(&mut host, "remove_negative_timed", 1);
    assert_eq!(
        int(&host, "i10"),
        1,
        "the timed life buff is removed, the neutral buff stays"
    );
    assert!(host.game().buffs(k).iter().all(|b| b.base == BUFF_NEUTRAL));
}

#[test]
fn timed_life_expires_at_the_expected_step_and_kills_the_unit() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(1))
endfunction
"#,
    );
    host.start("main", &[]).expect("main starts");
    host.step();
    let k = unit_key(&host, "u1");
    assert!(!is_dead(&host, k));
    // The buff was applied at the queue time after the first step (0.1 s); the duration is 1.0 s.
    // Main ran at once at clock 0.0, so the timed life was applied at 0.0 (lockstep.md section 12.1).
    let start = F32::ZERO;
    let expiry = arith::add(
        F32::from_bits(start.to_bits()),
        F32::from_bits(0x3F80_0000), // 1.0
    );
    assert_eq!(host.game().buffs(k).len(), 1);

    let mut prev = host.clock().now();
    let mut died_at = None;
    for _ in 0..30 {
        host.step();
        let now = host.clock().now();
        if is_dead(&host, k) {
            died_at = Some((prev, now));
            break;
        }
        assert!(
            now < expiry.to_f32(),
            "still alive after the expiry at step {now}"
        );
        prev = now;
    }
    let (before, after) = died_at.expect("the unit dies within 30 steps");
    assert!(before < expiry.to_f32(), "not dead one step earlier");
    assert!(
        after >= expiry.to_f32(),
        "dead on the step that reaches the expiry"
    );
    assert!(
        host.game().buffs(k).is_empty(),
        "the buff is gone after its expiry"
    );
    let deaths = host
        .game_mut()
        .take_unit_events()
        .iter()
        .filter(|e| matches!(e, UnitEvent::Death(u) if *u == k))
        .count();
    assert_eq!(deaths, 1, "one death event, through the KillUnit path");
}

#[test]
fn timed_life_pause_holds_the_unit_and_resume_adds_the_time_left() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(2))
endfunction
function pause_on takes nothing returns nothing
    call UnitPauseTimedLife(u1, true)
endfunction
function pause_off takes nothing returns nothing
    call UnitPauseTimedLife(u1, false)
endfunction
"#,
    );
    host.start("main", &[]).expect("main starts");
    host.step();
    let k = unit_key(&host, "u1");
    // Main ran at once at clock 0.0, so the timed life was applied at 0.0 (lockstep.md section 12.1).
    let start = F32::ZERO;
    let expiry0 = arith::add(
        F32::from_bits(start.to_bits()),
        F32::from_bits(0x4000_0000), // 2.0
    );
    for _ in 0..5 {
        host.step();
    }

    // Pause in a step of its own: the pause runs at the queue time of that step.
    call(&mut host, "pause_on", 1);
    let paused_at = host.clock().now();
    let left = arith::sub(expiry0, F32::from_bits(paused_at.to_bits()));
    for _ in 0..40 {
        host.step();
        assert!(!is_dead(&host, k), "a paused timed life does not expire");
    }

    call(&mut host, "pause_off", 1);
    let resumed_at = host.clock().now();
    let new_expiry = arith::add(F32::from_bits(resumed_at.to_bits()), left);
    let mut died = false;
    for _ in 0..40 {
        let before = host.clock().now();
        host.step();
        if is_dead(&host, k) {
            assert!(
                before < new_expiry.to_f32(),
                "not dead before the new expiry"
            );
            assert!(
                host.clock().now() >= new_expiry.to_f32(),
                "dead at the new expiry"
            );
            died = true;
            break;
        }
    }
    assert!(died, "the unit dies once the remaining time has run out");
}

#[test]
fn removing_timed_life_cancels_its_expiry() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(1))
endfunction
function remove_timed takes nothing returns nothing
    call UnitRemoveBuffs(u1, false, true)
endfunction
"#,
    );
    call(&mut host, "main", 1);
    let k = unit_key(&host, "u1");
    call(&mut host, "remove_timed", 1);
    assert!(host.game().buffs(k).is_empty());
    for _ in 0..30 {
        host.step();
    }
    assert!(
        !is_dead(&host, k),
        "the cancelled timer does not kill the unit"
    );
}

/// A zero duration starts no expiry timer (CONFIRMED by Unicorn, `docs/spec/abilities-2.md` section 2.1): the buff
/// stays on the unit, and the unit does not die on later steps. The earlier model, which expired it on the next
/// step, was wrong.
#[test]
fn zero_duration_timed_life_starts_no_timer_and_stays() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(0))
endfunction
"#,
    );
    host.start("main", &[]).expect("main starts");
    let k = unit_key(&host, "u1");
    assert_eq!(host.game().buffs(k).len(), 1, "the buff is applied");
    host.step_ticks(200);
    assert!(!is_dead(&host, k), "no expiry without a timer");
    assert_eq!(host.game().buffs(k).len(), 1, "the buff stays");
}

/// A negative duration behaves like zero (CONFIRMED by Unicorn): no timer, no expiry.
#[test]
fn negative_duration_timed_life_starts_no_timer() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(-1))
endfunction
"#,
    );
    host.start("main", &[]).expect("main starts");
    let k = unit_key(&host, "u1");
    host.step_ticks(200);
    assert!(!is_dead(&host, k));
    assert_eq!(host.game().buffs(k).len(), 1);
}

/// Applying the same timed life twice adds two entries (CONFIRMED: the creator does not search the unit's list),
/// and each has its own expiry: the first one to run out removes only its own entry.
#[test]
fn second_timed_life_adds_an_entry_with_its_own_expiry() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(1))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(3))
endfunction
"#,
    );
    host.start("main", &[]).expect("main starts");
    let k = unit_key(&host, "u1");
    assert_eq!(
        host.game().buffs(k).len(),
        2,
        "two entries, no stacking rule"
    );
    // Past the first expiry (1.0 s = 200 ticks): only the entry of the shorter timer is removed. Whether the unit
    // dies at that point is not asserted: the death route of the original is UNSURE (abilities-2.md section 3.3).
    host.step_ticks(220);
    assert_eq!(
        host.game().buffs(k).len(),
        1,
        "only the shorter one expired"
    );
}

/// The unit's life of a `UnitResetCooldown` call is untouched and no timer changes (CONFIRMED: the native is a no-op on
/// the ability list in the original).
#[test]
fn reset_cooldown_leaves_timed_life_and_the_unit_alone() {
    let mut host = host_for(
        r#"
function main takes nothing returns nothing
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    call UnitApplyTimedLife(u1, 'BTLF', I2R(1))
    call UnitResetCooldown(u1)
endfunction
"#,
    );
    host.start("main", &[]).expect("main starts");
    let k = unit_key(&host, "u1");
    assert_eq!(host.game().buffs(k).len(), 1);
    assert!(!is_dead(&host, k));
    // The timer set by UnitApplyTimedLife still runs out at its expiry.
    host.step_ticks(220);
    assert!(is_dead(&host, k), "the timed life expiry is unchanged");
}
