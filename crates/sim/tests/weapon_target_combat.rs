//! Weapon 2, the weapon chooser, splash and bounce in the attack cycle (`docs/spec/weapon-target.md`,
//! `docs/spec/weapon-splash.md`). The selection routines are pinned against the original by the Unicorn vector tests
//! (`target_accept.rs`, `splash.rs`, `bounce.rs`); these tests pin how the cycle uses them. The damage model is the
//! placeholder (the weapon's flat `damage`), so the expected amounts are the soft-float products of the rules.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::combat::{AttackType, WeaponExtra};
use sim::natives::game::{UnitType, UnitTypes};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::{arith, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
native GetHandleId takes handle h returns integer
"#;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

const GROUND: u32 = 2;
const AIR: u32 = 4;
const VICTIM: u32 = 0x6876_6963; // 'hvic'
const DUAL: u32 = 0x6872_666C; // 'hrfl': ground weapon 1, air weapon 2
const MORTAR: u32 = 0x686D_6F72; // 'hmor': msplash
const HUNTRESS: u32 = 0x6568_756E; // 'ehun': mbounce
const GRYPHON: u32 = 0x6867_7279; // 'hgry'

fn weapon(damage: f32, cooldown: f32) -> AttackType {
    AttackType {
        cooldown: f(cooldown),
        damage_point: f(0.5),
        backswing: f(0.5),
        range: f(600.0),
        collision: f(0.5),
        damage: f(damage),
        missile_speed: None,
        ..Default::default()
    }
}

fn host_with(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        let types: &mut UnitTypes = g.unit_types_mut();
        let plain = |name: &str, targ: u32, movement: u32| UnitType {
            name: name.to_string(),
            max_life: f(10000.0),
            targ_type: targ,
            movement,
            ..Default::default()
        };
        types.insert(VICTIM, plain("Victim", GROUND, 1));
        types.insert(GRYPHON, plain("Gryphon", AIR, 2));
        for id in [DUAL, MORTAR, HUNTRESS] {
            types.insert(id, plain("Shooter", GROUND, 1));
        }
        g.set_type_orders(VICTIM, TypeOrderData { abilities: vec![AbilityKind::Move] });
        g.set_type_orders(GRYPHON, TypeOrderData { abilities: vec![AbilityKind::Move] });
        for id in [DUAL, MORTAR, HUNTRESS] {
            g.set_type_orders(id, TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        }
        // The dual weapon unit: ground (10 damage) and air (25 damage); one swing every 100 s.
        g.set_attack_type(DUAL, weapon(10.0, 100.0));
        g.set_attack_type2(DUAL, weapon(25.0, 100.0));
        g.set_weapon_extra(DUAL, 0, WeaponExtra { targs: GROUND, kind: 1, ..Default::default() });
        g.set_weapon_extra(DUAL, 1, WeaponExtra { targs: AIR, kind: 2, ..Default::default() });
        // The mortar: msplash, rings 50 / 100 / 150, factors 0.5 and 0.25, splash on enemy ground units.
        g.set_attack_type(MORTAR, weapon(100.0, 100.0));
        g.set_weapon_extra(
            MORTAR,
            0,
            WeaponExtra {
                targs: GROUND,
                kind: 5,
                splash_targs: GROUND | 0x1_0000,
                area: [f(50.0), f(100.0), f(150.0)],
                hfact: f(0.5),
                qfact: f(0.25),
                ..Default::default()
            },
        );
        // The huntress: mbounce, three hits, 40 percent lost per bounce, search radius 300, a fast missile.
        let mut hw = weapon(100.0, 100.0);
        hw.missile_speed = Some(f(1000.0));
        g.set_attack_type(HUNTRESS, hw);
        g.set_weapon_extra(
            HUNTRESS,
            0,
            WeaponExtra {
                targs: GROUND,
                kind: 6,
                splash_targs: GROUND,
                area: [f(300.0), f(0.0), f(0.0)],
                targ_count: 3,
                damage_loss: f(0.4),
                ..Default::default()
            },
        );
    }
    host
}

fn run(host: &mut ScriptHost, n: usize) {
    for _ in 0..n {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn key(host: &ScriptHost, name: &str) -> sim::natives::game::ObjKey {
    let id = host
        .vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"));
    host.game().unit_of_handle(id).unwrap_or_else(|| panic!("{name} is not a live unit"))
}

/// The life a unit of 10000 should have after taking `amount` (one soft subtraction, as the life setter does).
fn after(amount: F32) -> F32 {
    arith::sub(f(10000.0), amount)
}

fn life(host: &ScriptHost, name: &str) -> F32 {
    host.game().unit(key(host, name)).life
}

const DUAL_SRC: &str = r#"
globals
    unit r1 = null
    unit r2 = null
    unit g = null
    unit a = null
endglobals
function setup takes nothing returns nothing
    set r1 = CreateUnit(Player(0), 'hrfl', 0, 0, 0)
    set r2 = CreateUnit(Player(0), 'hrfl', 0, 50, 0)
    set g = CreateUnit(Player(1), 'hvic', 100, 0, 180)
    set a = CreateUnit(Player(1), 'hgry', 100, 50, 180)
endfunction
function duel takes nothing returns nothing
    call IssueTargetOrder(r1, "attack", g)
    call IssueTargetOrder(r2, "attack", a)
endfunction
"#;

#[test]
fn the_chooser_picks_the_weapon_that_accepts_the_target() {
    let mut host = host_with(DUAL_SRC);
    host.start("setup", &[]).expect("setup");
    host.step();
    let g = host.game();
    let (r1, gnd, air) = (key(&host, "r1"), key(&host, "g"), key(&host, "a"));
    assert_eq!(g.choose_weapon(r1, gnd), Some(0), "the ground weapon takes the ground unit");
    assert_eq!(g.choose_weapon(r1, air), Some(1), "the air weapon takes the flyer");
}

#[test]
fn a_dual_weapon_unit_hits_each_target_with_its_own_weapon() {
    let mut host = host_with(DUAL_SRC);
    host.start("setup", &[]).expect("setup");
    host.step();
    host.start("duel", &[]).expect("duel");
    run(&mut host, 20);
    assert_eq!(life(&host, "g"), after(f(10.0)), "weapon 1 on the ground unit");
    assert_eq!(life(&host, "a"), after(f(25.0)), "weapon 2 on the flyer");
}

#[test]
fn a_target_no_weapon_accepts_is_not_attacked() {
    // A ground-only unit cannot hit a flyer: the attack order completes without damage.
    let mut host = host_with(DUAL_SRC);
    {
        let mut g = host.game_mut();
        // drop weapon 2: only the ground weapon remains
        g.set_weapon_extra(DUAL, 1, WeaponExtra { targs: AIR, kind: 2, ..Default::default() });
    }
    host.start("setup", &[]).expect("setup");
    host.step();
    let (r1, air) = (key(&host, "r1"), key(&host, "a"));
    assert_eq!(host.game().choose_weapon(r1, air), Some(1));
    // now remove the second attack type by pointing weapon 2 at ground only
    host.game_mut().set_weapon_extra(DUAL, 1, WeaponExtra { targs: GROUND, kind: 2, ..Default::default() });
    assert_eq!(host.game().choose_weapon(r1, air), None, "no weapon accepts a flyer");
}

const SPLASH_SRC: &str = r#"
globals
    unit m = null
    unit p = null
    unit full = null
    unit half = null
    unit quarter = null
    unit far = null
    unit friend = null
endglobals
function setup takes nothing returns nothing
    set m = CreateUnit(Player(0), 'hmor', 0, 0, 0)
    set p = CreateUnit(Player(1), 'hvic', 100, 0, 180)
    set full = CreateUnit(Player(1), 'hvic', 100, 30, 180)
    set half = CreateUnit(Player(1), 'hvic', 100, -80, 180)
    set quarter = CreateUnit(Player(1), 'hvic', 100, 130, 180)
    set far = CreateUnit(Player(1), 'hvic', 100, 200, 180)
    set friend = CreateUnit(Player(2), 'hvic', 100, 10, 180)
endfunction
function fire takes nothing returns nothing
    call IssueTargetOrder(m, "attack", p)
endfunction
"#;

#[test]
fn msplash_scales_the_damage_by_ring_and_spares_allies() {
    let mut host = host_with(SPLASH_SRC);
    {
        // player 2 is passively allied with player 0 and shares spells: an "ally" for the splash filter
        let mut g = host.game_mut();
        g.player_mut(0).unwrap().alliances[0] |= 1 << 2;
        g.player_mut(0).unwrap().alliances[4] |= 1 << 2;
    }
    host.start("setup", &[]).expect("setup");
    host.step();
    host.start("fire", &[]).expect("fire");
    run(&mut host, 20);
    assert_eq!(life(&host, "p"), after(f(100.0)), "the primary target takes the full hit");
    assert_eq!(life(&host, "full"), after(arith::mul(f(100.0), F32::ONE)), "inside Farea: factor 1.0");
    assert_eq!(life(&host, "half"), after(arith::mul(f(100.0), f(0.5))), "inside Harea: Hfact");
    assert_eq!(life(&host, "quarter"), after(arith::mul(f(100.0), f(0.25))), "inside Qarea: Qfact");
    assert_eq!(life(&host, "far"), after(F32::ZERO), "outside the outermost ring");
    assert_eq!(life(&host, "friend"), after(F32::ZERO), "an ally is not in the enemy splash");
}

const BOUNCE_SRC: &str = r#"
globals
    unit h = null
    unit p = null
    unit e1 = null
    unit e2 = null
    unit e3 = null
endglobals
function setup takes nothing returns nothing
    set h = CreateUnit(Player(0), 'ehun', 0, 0, 0)
    set p = CreateUnit(Player(1), 'hvic', 100, 0, 180)
    set e1 = CreateUnit(Player(1), 'hvic', 250, 0, 180)
    set e2 = CreateUnit(Player(1), 'hvic', 400, 0, 180)
    set e3 = CreateUnit(Player(1), 'hvic', 550, 0, 180)
endfunction
function fire takes nothing returns nothing
    call IssueTargetOrder(h, "attack", p)
endfunction
"#;

#[test]
fn mbounce_hits_the_nearest_units_with_the_loss_applied_each_bounce() {
    let mut host = host_with(BOUNCE_SRC);
    host.start("setup", &[]).expect("setup");
    host.step();
    host.start("fire", &[]).expect("fire");
    run(&mut host, 30);
    let keep = arith::sub(F32::ONE, f(0.4));
    let second = arith::mul(f(100.0), keep);
    let third = arith::mul(second, keep);
    assert_eq!(life(&host, "p"), after(f(100.0)), "the primary target takes the rolled amount");
    assert_eq!(life(&host, "e1"), after(second), "the nearest unit takes (1 - loss) of it");
    assert_eq!(life(&host, "e2"), after(third), "then the next nearest takes (1 - loss) of that");
    assert_eq!(life(&host, "e3"), after(F32::ZERO), "a target count of three ends the chain");
}
