//! Behaviour tests of the corpse and dispel abilities of `unit_abilities_a2.rs`: Cannibalize (Acan), Ancestral Spirit
//! (Aast), Devour Magic (Advm) and the Graveyard (Agyd). Level rows are the 1.26a first-level rows unless marked.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{buff_predicates, code, AbilityType, BuffPredicates};
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::unit_spells::UnitMods;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native KillUnit takes unit whichUnit returns boolean
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
"#;

const SRC: &str = r#"
globals
    unit caster = null
    unit corpse = null
    unit ally_corpse = null
    unit foe_corpse = null
    unit foe_a = null
    unit foe_b = null
    unit foe_summon = null
    unit friend = null
    unit yard = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set caster = CreateUnit(Player(0), 'ugho', 0.0, 0.0, 0.0)
    set corpse = CreateUnit(Player(1), 'hfoo', 30.0, 0.0, 0.0)
    set ally_corpse = CreateUnit(Player(0), 'hfoo', 60.0, 100.0, 0.0)
    set foe_corpse = CreateUnit(Player(1), 'hfoo', 20.0, 100.0, 0.0)
    set foe_a = CreateUnit(Player(1), 'hfoo', 500.0, 0.0, 0.0)
    set foe_b = CreateUnit(Player(1), 'hfoo', 520.0, 0.0, 0.0)
    set foe_summon = CreateUnit(Player(1), 'hfoo', 540.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 480.0, 0.0, 0.0)
    set yard = CreateUnit(Player(0), 'ugrv', 3000.0, 0.0, 0.0)
endfunction
function kill_corpses takes nothing returns nothing
    call KillUnit(corpse)
    call KillUnit(ally_corpse)
    call KillUnit(foe_corpse)
endfunction
function give takes nothing returns nothing
    call UnitAddAbility(caster, 'Acan')
    call UnitAddAbility(caster, 'Aast')
    call UnitAddAbility(caster, 'Advm')
endfunction
function give_yard takes nothing returns nothing
    call UnitAddAbility(yard, 'Agyd')
endfunction
function eat takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852188)
endfunction
function eat_target takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852535, corpse)
endfunction
function revive takes nothing returns nothing
    set accepted = IssueImmediateOrderById(caster, 852490)
endfunction
function revive_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(caster, 852491, foe_corpse)
endfunction
function devour takes nothing returns nothing
    set accepted = IssuePointOrderById(caster, 852536, 520.0, 0.0)
endfunction
"#;

const FOOT: u32 = u32::from_be_bytes(*b"hfoo");
const GHOUL: u32 = u32::from_be_bytes(*b"ugho");
const YARD: u32 = u32::from_be_bytes(*b"ugrv");

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn data(v: &[f32]) -> [F32; 9] {
    let mut d = [F32::ZERO; 9];
    for (i, x) in v.iter().enumerate() {
        d[i] = f(*x);
    }
    d
}

fn host(acan: SpellLevel) -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        let ut = |name: &str, life: f32| UnitType {
            name: name.to_string(),
            max_life: f(life),
            max_mana: f(300.0),
            start_mana: f(300.0),
            move_speed: f(270.0),
            ..Default::default()
        };
        g.unit_types_mut().insert(FOOT, ut("footman", 400.0));
        g.unit_types_mut().insert(GHOUL, ut("ghoul", 1000.0));
        g.unit_types_mut().insert(YARD, ut("graveyard", 800.0));
        g.unit_types_mut().set_building(YARD, true);
        let rows = [
            (code(b"Acan"), acan),
            (code(b"Aast"), SpellLevel { cool: f(30.0), cost: 250, rng: f(350.0), data: data(&[1.0, 0.25]), ..Default::default() }),
            (
                code(b"Advm"),
                SpellLevel { cool: f(7.0), area: f(200.0), rng: f(600.0), data: data(&[50.0, 75.0, 0.0, 0.0, 180.0, 1.0]), ..Default::default() },
            ),
            (
                code(b"Agyd"),
                SpellLevel { cool: f(15.0), unit_id: GHOUL, data: data(&[5.0, 200.0, 250.0]), ..Default::default() },
            ),
        ];
        for (c, l) in rows {
            g.ability_types_mut().insert(c, AbilityType { max_level: Some(1) });
            g.set_spell_levels(c, vec![l]);
        }
    }
    h
}

fn cannibalize_row() -> SpellLevel {
    SpellLevel { dur: f(33.0), hero_dur: f(33.0), rng: f(50.0), data: data(&[10.0, 800.0]), ..Default::default() }
}

fn started(acan: SpellLevel) -> ScriptHost {
    let mut h = host(acan);
    h.start("setup", &[]).expect("setup");
    h.step();
    h.start("kill_corpses", &[]).expect("kill");
    h.step();
    h.start("give", &[]).expect("give");
    h.step();
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
}

fn call(h: &mut ScriptHost, name: &str, steps: usize) {
    h.start(name, &[]).expect("start");
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

#[test]
fn cannibalize_heals_ten_a_second_and_eats_the_corpse_at_the_end() {
    let mut h = started(cannibalize_row());
    let (caster, corpse) = (unit(&h, "caster"), unit(&h, "corpse"));
    h.game_mut().write_life(caster, f(100.0));
    call(&mut h, "eat", 2);
    assert!(accepted(&h));
    assert!(h.game().unit(corpse).dead && !h.game().unit(corpse).removed, "the corpse is eaten at the end");
    run(&mut h, 50);
    let l = life(&h, caster);
    assert!((l - 150.0).abs() <= 10.5, "about 5 s of healing: {l}");
    run(&mut h, 400);
    assert!((life(&h, caster) - 430.0).abs() < 0.5, "100 + 33 s x 10: {}", life(&h, caster));
    assert!(h.game().unit(corpse).removed, "eaten");
}

#[test]
fn cannibalize_stops_at_the_max_hit_points_and_when_the_unit_moves() {
    let mut row = cannibalize_row();
    row.data = data(&[10.0, 120.0]);
    let mut h = started(row);
    let (caster, corpse) = (unit(&h, "caster"), unit(&h, "corpse"));
    h.game_mut().write_life(caster, f(100.0));
    call(&mut h, "eat_target", 2);
    assert!(accepted(&h));
    run(&mut h, 60);
    assert_eq!(life(&h, caster), 120.0, "DataB caps the healing");
    assert!(h.game().unit(corpse).removed);
    // Moving away ends a channel: a second run with a unit that is pushed.
    let mut h = started(cannibalize_row());
    let (caster, corpse) = (unit(&h, "caster"), unit(&h, "corpse"));
    h.game_mut().write_life(caster, f(100.0));
    call(&mut h, "eat", 2);
    run(&mut h, 15);
    let before = life(&h, caster);
    h.game_mut().store_unit_position(caster, f(40.0), f(0.0));
    run(&mut h, 60);
    assert_eq!(life(&h, caster), before, "no more healing after the move");
    assert!(!h.game().unit(corpse).removed, "the corpse is left");
}

#[test]
fn ancestral_spirit_revives_the_closest_own_corpse_with_the_data_factors() {
    let mut h = started(cannibalize_row());
    let (ally, foe) = (unit(&h, "ally_corpse"), unit(&h, "foe_corpse"));
    call(&mut h, "revive", 3);
    assert!(accepted(&h));
    assert!(!h.game().unit(ally).dead, "the ally is back");
    assert_eq!(life(&h, ally), 400.0);
    assert_eq!(h.game().unit(ally).mana.to_f32(), 75.0, "25 % of 300");
    assert!(h.game().unit(foe).dead, "an enemy corpse is never revived");
    call(&mut h, "revive_foe", 3);
    assert!(!accepted(&h), "the target must be an own corpse");
}

#[test]
fn devour_magic_dispels_enemies_hurts_summons_and_feeds_the_caster() {
    let mut h = started(cannibalize_row());
    let (caster, a, b, s, friend) = (unit(&h, "caster"), unit(&h, "foe_a"), unit(&h, "foe_b"), unit(&h, "foe_summon"), unit(&h, "friend"));
    {
        let mut g = h.game_mut();
        g.write_life(caster, f(100.0));
        g.write_mana(caster, f(0.0));
        g.unit_mut(s).summoned = true;
        let buff = code(b"Binf");
        let preds = BuffPredicates { positive: true, magic: true, dispellable: true, ..buff_predicates(buff) };
        for k in [a, friend] {
            g.apply_mods_buff(k, buff, preds, UnitMods::default(), f(60.0));
        }
    }
    call(&mut h, "devour", 4);
    assert!(accepted(&h));
    let g = h.game();
    assert!(!g.buffs(a).iter().any(|x| x.base == code(b"Binf")), "the enemy's buff is gone");
    assert!(g.buffs(friend).iter().any(|x| x.base == code(b"Binf")), "friends keep theirs (DataF)");
    assert_eq!(life(&h, b), 400.0, "an unbuffed enemy is untouched");
    assert_eq!(life(&h, s), 400.0 - 180.0, "the summoned enemy takes DataE");
    // two units were affected: 2 x 50 life and 2 x 75 mana
    assert!((life(&h, caster) - 200.0).abs() < 0.5, "{}", life(&h, caster));
    assert!((g.unit(caster).mana.to_f32() - 150.0).abs() < 0.5);
}

#[test]
fn the_graveyard_makes_up_to_five_corpses_one_per_cooldown() {
    let mut h = started(cannibalize_row());
    let yard = unit(&h, "yard");
    let corpses = |h: &ScriptHost| {
        h.game()
            .unit_objects()
            .filter(|(_, u)| u.dead && !u.removed && u.type_id == GHOUL && (u.x.to_f32() - 3000.0).abs() <= 250.0)
            .count()
    };
    assert!(h.game().unit(yard).abilities.get(code(b"Agyd")).is_none());
    call(&mut h, "give_yard", 1);
    run(&mut h, 100);
    assert_eq!(corpses(&h), 0, "the first corpse waits for the 15 s cooldown");
    run(&mut h, 60);
    assert_eq!(corpses(&h), 1);
    run(&mut h, 1200);
    assert_eq!(corpses(&h), 5, "capped at DataA");
}
