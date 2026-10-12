//! Kill experience in the death path (`sim::kill_xp`, `docs/spec/heroes.md` sections 8 and 12).
//!
//! The split, the amount and the experience engine are the Unicorn-checked routines (`tests/kill_xp_vectors.rs`); these
//! tests pin the wiring: which heroes are candidates and recipients (range, global list, shared experience, passive
//! alliance, structure killers, illusions), the amount tables of hero and unit victims, and the hand-over from a combat
//! kill. The expected values are worked out by hand. They need the reciprocal table of `GAME_DLL` (the split divides in
//! softfloat) and return early without it.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::combat::AttackType;
use sim::heroes::XpTables;
use sim::kill_xp::grant_kill_xp;
use sim::natives::game::{KillXpConfig, ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const HERO: u32 = 0x4870_616C; // 'Hpal'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const TOWER: u32 = 0x6874_6F77; // 'htow'

const PRELUDE: &str = r#"
type player extends handle
type unit extends handle
native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native SetPlayerAlliance takes player a, player b, integer t, boolean v returns nothing
native IssueTargetOrder takes unit whichUnit, string order, unit target returns boolean
"#;

// h1 (player 0) at the victim, h2 (player 0) far away, h3 (player 1, the victim's side) at the victim, p2 (player 2)
// at the victim, v the victim (player 1), k the killer (player 0), t a tower (player 0).
const SCRIPT: &str = r#"
globals
unit h1
unit h2
unit h3
unit p2
unit v
unit k
unit t
endglobals
function main takes nothing returns nothing
    set h1 = CreateUnit(Player(0), 'Hpal', I2R(0), I2R(0), I2R(0))
    set h2 = CreateUnit(Player(0), 'Hpal', I2R(3000), I2R(0), I2R(0))
    set h3 = CreateUnit(Player(1), 'Hpal', I2R(100), I2R(100), I2R(0))
    set p2 = CreateUnit(Player(2), 'Hpal', I2R(120), I2R(0), I2R(0))
    set v = CreateUnit(Player(1), 'hfoo', I2R(100), I2R(0), I2R(0))
    set k = CreateUnit(Player(0), 'hfoo', I2R(50), I2R(0), I2R(0))
    set t = CreateUnit(Player(0), 'htow', I2R(60), I2R(0), I2R(0))
endfunction
function share_xp takes nothing returns nothing
    call SetPlayerAlliance(Player(0), Player(2), 3, true)
endfunction
function duel takes nothing returns nothing
    call IssueTargetOrder(k, "attack", v)
endfunction
function ally_passive takes nothing returns nothing
    call SetPlayerAlliance(Player(0), Player(1), 0, true)
endfunction
"#;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("lookup tables in GAME_DLL"))
}

fn unit_type(name: &str) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: f(100.0),
        ..Default::default()
    }
}

fn config() -> KillXpConfig {
    KillXpConfig {
        hero_exp_range: f(1200.0),
        summon_factor: f(0.5),
        max_unit_level: 3,
        ..KillXpConfig::default()
    }
}

struct World {
    host: ScriptHost,
}

impl World {
    fn new(t: Tables) -> World {
        let full = format!("{PRELUDE}\n{SCRIPT}");
        let prog = jass::parse(&full).expect("parse");
        let module = compile(&[prog]).expect("compile");
        let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
        {
            let mut g = host.game_mut();
            g.set_tables(Some(t));
            g.unit_types_mut().insert(HERO, unit_type("Paladin"));
            let mut foot = unit_type("Footman");
            foot.level = 2;
            g.unit_types_mut().insert(FOOT, foot);
            g.unit_types_mut().insert(TOWER, unit_type("Tower"));
            g.unit_types_mut().set_building(TOWER, true);
            // Need 100..500 (max level 5); hero grants 100..500; unit grants 20, 40, 60.
            g.set_hero_xp(XpTables::from_columns(
                vec![100, 200, 300, 400, 500],
                vec![F32::ZERO; 5],
                vec![100, 200, 300, 400, 500],
                vec![20, 40, 60],
            ));
            g.set_kill_xp_config(config());
            g.set_type_orders(FOOT, TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
            g.set_attack_type(
                FOOT,
                AttackType {
                    cooldown: f(1.0),
                    damage_point: f(0.5),
                    backswing: f(0.5),
                    range: f(128.0),
                    collision: f(0.5),
                    damage: f(10.0),
                    missile_speed: None,
                    ..Default::default()
                },
            );
        }
        host.start("main", &[]).expect("main");
        host.step();
        World { host }
    }

    fn call(&mut self, name: &str) {
        self.host.start(name, &[]).expect("function starts");
        self.host.step();
    }

    fn unit(&self, global: &str) -> ObjKey {
        let id = self.host.vm().global(global).and_then(|v| v.as_handle()).expect(global);
        self.host.game().unit_of_handle(id).expect("live unit")
    }

    /// (level, xp, skill points) of a hero global.
    fn hero(&self, global: &str) -> (i32, i32, i32) {
        let k = self.unit(global);
        let g = self.host.game();
        let h = g.unit(k).hero.as_ref().expect("hero");
        (h.level, h.xp, h.skill_points)
    }

    fn kill(&self, victim: &str, killer: &str) {
        let (v, k) = (self.unit(victim), self.unit(killer));
        grant_kill_xp(&mut self.host.game_mut(), v, k);
    }
}

#[test]
fn the_hero_in_range_takes_the_amount_of_the_victim_level() {
    let Some(t) = tables() else {
        eprintln!("GAME_DLL not set: skipping");
        return;
    };
    let w = World::new(t);
    // Footman level 2 -> GrantNormalXP[1] = 40. h1 is 100 away; h2 is 3000 away (range 1200); h3 and p2 do not share
    // experience with the killer's owner (player 2 is not a shared-XP ally, player 1 is an enemy).
    w.kill("v", "k");
    assert_eq!(w.hero("h1"), (1, 40, 0));
    assert_eq!(w.hero("h2"), (1, 0, 0), "out of range and GlobalExperience off");
    assert_eq!(w.hero("h3"), (1, 0, 0), "the victim's side gets nothing");
    assert_eq!(w.hero("p2"), (1, 0, 0), "a stranger without shared experience gets nothing");
}

#[test]
fn shared_experience_allies_split_the_amount() {
    let Some(t) = tables() else {
        return;
    };
    let mut w = World::new(t);
    w.call("share_xp");
    // h1 and p2 are in range and eligible: F = 40 / 2 = 20 each.
    w.kill("v", "k");
    assert_eq!(w.hero("h1"), (1, 20, 0));
    assert_eq!(w.hero("p2"), (1, 20, 0));
    assert_eq!(w.hero("h2").1, 0);
}

#[test]
fn nobody_in_range_falls_back_to_the_global_list() {
    let Some(t) = tables() else {
        return;
    };
    let w = World::new(t);
    // Move the near hero away: nobody is in range. With GlobalExperience the global list (every eligible hero) shares.
    let h1 = w.unit("h1");
    w.host.game_mut().unit_mut(h1).x = f(5000.0);
    w.host.game_mut().set_kill_xp_config(KillXpConfig {
        global_experience: true,
        ..config()
    });
    w.kill("v", "k");
    assert_eq!(w.hero("h1"), (1, 20, 0));
    assert_eq!(w.hero("h2"), (1, 20, 0));
    // Without the flag nothing is given.
    let w2 = World::new(tables().unwrap());
    let h1 = w2.unit("h1");
    w2.host.game_mut().unit_mut(h1).x = f(5000.0);
    w2.kill("v", "k");
    assert_eq!(w2.hero("h1"), (1, 0, 0));
    assert_eq!(w2.hero("h2"), (1, 0, 0));
}

#[test]
fn a_passive_ally_of_the_victims_owner_gives_nothing() {
    let Some(t) = tables() else {
        return;
    };
    let mut w = World::new(t);
    w.call("ally_passive");
    w.kill("v", "k");
    assert_eq!(w.hero("h1"), (1, 0, 0));
}

#[test]
fn a_structure_killer_gives_experience_only_with_the_misc_flag() {
    let Some(t) = tables() else {
        return;
    };
    let w = World::new(t);
    w.kill("v", "t");
    assert_eq!(w.hero("h1"), (1, 0, 0), "a tower kill gives nothing");
    w.host.game_mut().set_kill_xp_config(KillXpConfig {
        building_kills: true,
        ..config()
    });
    w.kill("v", "t");
    assert_eq!(w.hero("h1"), (1, 40, 0));
}

#[test]
fn illusions_and_removed_killers_give_nothing() {
    let Some(t) = tables() else {
        return;
    };
    let w = World::new(t);
    let v = w.unit("v");
    let k = w.unit("k");
    w.host.game_mut().unit_mut(v).illusion = true;
    w.kill("v", "k");
    assert_eq!(w.hero("h1"), (1, 0, 0), "an illusion victim gives nothing");
    w.host.game_mut().unit_mut(v).illusion = false;
    w.host.game_mut().unit_mut(k).removed = true;
    grant_kill_xp(&mut w.host.game_mut(), v, k);
    assert_eq!(w.hero("h1"), (1, 0, 0), "a removed killer gives nothing");
}

#[test]
fn a_hero_victim_uses_the_hero_table_and_levels_the_recipient_up() {
    let Some(t) = tables() else {
        return;
    };
    let w = World::new(t);
    // h3 is a level 3 hero: GrantHeroXP[2] = 300. h1 gets 300: levels 2, 3 and 4 (needs 100, 200, 300), 3 skill points.
    let h3 = w.unit("h3");
    w.host.game_mut().unit_mut(h3).hero.as_mut().unwrap().level = 3;
    w.kill("h3", "k");
    assert_eq!(w.hero("h1"), (4, 300, 3));
}

#[test]
fn a_summoned_victim_scales_the_amount() {
    let Some(t) = tables() else {
        return;
    };
    let w = World::new(t);
    let v = w.unit("v");
    w.host.game_mut().unit_mut(v).summoned = true;
    w.kill("v", "k");
    assert_eq!(w.hero("h1").1, 20, "SummonedKillFactor 0.5 on 40");
}

#[test]
fn the_skill_point_cap_follows_the_hero_slots() {
    let Some(t) = tables() else {
        return;
    };
    let w = World::new(t);
    // Two slots with 1 and 2 levels: a level-up that gains 3 points is capped at 3 learnable levels, 1 learned: slack 2.
    let h1 = w.unit("h1");
    {
        let mut g = w.host.game_mut();
        let u = g.unit_mut(h1);
        let h = u.hero.as_mut().unwrap();
        h.slots[0] = sim::natives::game::HeroSlot { code: 0x4130_0001, max_level: 1, req_level: 1 };
        h.slots[1] = sim::natives::game::HeroSlot { code: 0x4130_0002, max_level: 2, req_level: 1 };
    }
    let h3 = w.unit("h3");
    w.host.game_mut().unit_mut(h3).hero.as_mut().unwrap().level = 3;
    w.kill("h3", "k"); // 300 XP: levels 2, 3, 4 -> 3 points, capped at max(0,1-0) + max(0,2-0) = 3
    assert_eq!(w.hero("h1"), (4, 300, 3));
    // The cap helper on the same slots, by direct arithmetic.
    let g = w.host.game();
    let u = g.unit(h1);
    let slots = u.hero.as_ref().unwrap().slots;
    assert_eq!(sim::heroes::skill_point_cap(&slots, &|_| None), 3);
    // Slot 1 holds two levels (index 1): its room is 2 - 2 = 0; slot 0 still has 1.
    assert_eq!(sim::heroes::skill_point_cap(&slots, &|c| (c == 0x4130_0002).then_some(1)), 1);
}

#[test]
fn a_combat_kill_hands_the_experience_to_the_hero() {
    let Some(t) = tables() else {
        return;
    };
    let mut w = World::new(t);
    let v = w.unit("v");
    w.host.game_mut().unit_mut(v).life = f(10.0);
    w.host.start("duel", &[]).expect("duel");
    for _ in 0..60 {
        w.host.step();
    }
    assert!(w.host.game().unit(v).dead, "the victim died from the swing");
    assert_eq!(w.hero("h1"), (1, 40, 0), "the death path granted the kill experience");
}
