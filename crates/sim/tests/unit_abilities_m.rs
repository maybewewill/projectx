//! Behaviour tests of `unit_abilities_m.rs` (spec `docs/spec/abilities-unit-m.md`). Level numbers are pinned test
//! values; nothing here needs the game files.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{buff_predicates, AbilityType};
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::orders::{AbilityKind, TypeOrderData};
use sim::unit_abilities_m::*;
use sim::unit_spells::UnitMods;
use sim::ScriptHost;
use softfloat::F32;

const SRC: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
globals
    unit banshee = null
    unit foe = null
    unit foe_hero = null
    unit leash = null
    unit summon = null
    unit plain = null
    unit linker = null
    unit a = null
    unit b = null
    unit c = null
    unit thief = null
    unit buffed = null
    unit hall = null
    unit pea = null
    unit sting = null
    unit prey = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set banshee = CreateUnit(Player(0), 'nbns', 0.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'ndmy', 200.0, 0.0, 0.0)
    set foe_hero = CreateUnit(Player(1), 'nhro', 220.0, 0.0, 0.0)
    set leash = CreateUnit(Player(0), 'nlsh', 5000.0, 0.0, 0.0)
    set summon = CreateUnit(Player(1), 'ndmy', 5200.0, 0.0, 0.0)
    set plain = CreateUnit(Player(1), 'ndmy', 5220.0, 0.0, 0.0)
    set linker = CreateUnit(Player(0), 'nlnk', 8000.0, 0.0, 0.0)
    set a = CreateUnit(Player(0), 'ndmy', 8100.0, 0.0, 0.0)
    set b = CreateUnit(Player(0), 'ndmy', 8150.0, 0.0, 0.0)
    set c = CreateUnit(Player(0), 'ndmy', 8200.0, 0.0, 0.0)
    set thief = CreateUnit(Player(0), 'nthf', 12000.0, 0.0, 0.0)
    set buffed = CreateUnit(Player(1), 'ndmy', 12100.0, 0.0, 0.0)
    set hall = CreateUnit(Player(0), 'nhal', 16000.0, 0.0, 0.0)
    set pea = CreateUnit(Player(0), 'npea', 16100.0, 0.0, 0.0)
    set sting = CreateUnit(Player(0), 'nstg', 20000.0, 0.0, 0.0)
    set prey = CreateUnit(Player(1), 'ndmy', 20050.0, 0.0, 180.0)
endfunction
function possess takes nothing returns nothing
    set accepted = IssueTargetOrderById(banshee, 852196, foe)
endfunction
function possess_hero takes nothing returns nothing
    set accepted = IssueTargetOrderById(banshee, 852196, foe_hero)
endfunction
function leash_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(leash, 852480, summon)
endfunction
function leash_plain takes nothing returns nothing
    set accepted = IssueTargetOrderById(leash, 852480, plain)
endfunction
function link_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(linker, 852499, a)
endfunction
function steal_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(thief, 852483, buffed)
endfunction
function arms takes nothing returns nothing
    set accepted = IssueImmediateOrderById(hall, 852999)
endfunction
function stab takes nothing returns nothing
    call IssueTargetOrder(sting, "attack", prey)
endfunction
"#;

fn c(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

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

fn ut(life: f32, abilities: Vec<u32>) -> UnitType {
    UnitType {
        name: "unit".to_string(),
        max_life: f(life),
        max_mana: f(300.0),
        start_mana: f(300.0),
        move_speed: f(270.0),
        abilities,
        ..Default::default()
    }
}

fn host() -> ScriptHost {
    let prog = jass::parse(SRC).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(c(b"nbns"), ut(500.0, vec![POSSESSION]));
        g.unit_types_mut().insert(c(b"ndmy"), ut(500.0, vec![]));
        g.unit_types_mut().insert(c(b"nhro"), ut(500.0, vec![]));
        g.unit_types_mut().insert(c(b"nlsh"), ut(500.0, vec![MAGIC_LEASH]));
        g.unit_types_mut().insert(c(b"nlnk"), ut(500.0, vec![SPIRIT_LINK]));
        g.unit_types_mut().insert(c(b"nthf"), ut(500.0, vec![SPELL_STEAL]));
        g.unit_types_mut().insert(c(b"nhal"), ut(500.0, vec![MILITIA]));
        g.unit_types_mut().insert(c(b"npea"), ut(220.0, vec![MILITIA_CONVERT]));
        g.unit_types_mut().insert(MILITIA_TYPE, ut(240.0, vec![]));
        g.unit_types_mut().insert(c(b"nstg"), ut(500.0, vec![POISON_STING]));
        g.set_type_orders(c(b"nstg"), TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
        g.set_attack_type(
            c(b"nstg"),
            AttackType {
                cooldown: f(1.0),
                damage_point: f(0.5),
                backswing: f(0.5),
                range: f(100.0),
                collision: f(0.5),
                damage: f(10.0),
                missile_speed: None,
                ..Default::default()
            },
        );
        let lv = |cost, rng, area, dur, d: &[f32]| SpellLevel {
            cost,
            cool: f(1.0),
            rng: f(rng),
            area: f(area),
            dur: f(dur),
            hero_dur: f(dur),
            data: data(d),
            ..Default::default()
        };
        g.set_spell_levels(POSSESSION, vec![lv(100, 600.0, 0.0, 0.0, &[])]);
        g.set_spell_levels(MAGIC_LEASH, vec![lv(50, 600.0, 0.0, 0.0, &[1.0])]);
        g.set_spell_levels(SPIRIT_LINK, vec![lv(50, 600.0, 300.0, 60.0, &[0.5, 3.0])]);
        g.set_spell_levels(SPELL_STEAL, vec![lv(75, 600.0, 0.0, 30.0, &[1.0])]);
        g.set_spell_levels(MILITIA, vec![lv(0, 0.0, 1000.0, 45.0, &[])]);
        g.set_spell_levels(POISON_STING, vec![lv(0, 0.0, 0.0, 10.0, &[5.0])]);
        g.set_spell_order(852_999, MILITIA);
        for id in [POSSESSION, MAGIC_LEASH, SPIRIT_LINK, SPELL_STEAL, MILITIA, MILITIA_CONVERT, POISON_STING] {
            g.ability_types_mut().insert(id, AbilityType { max_level: Some(1) });
        }
    }
    h
}

fn started() -> ScriptHost {
    let mut h = host();
    h.start("setup", &[]).expect("setup");
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

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

#[test]
fn every_code_of_the_batch_is_listed_once_with_its_class() {
    assert_eq!(CODES.len(), 79);
    let mut seen = std::collections::BTreeSet::new();
    for (code, class, parent, _) in CODES {
        assert!(seen.insert(**code), "{}", String::from_utf8_lossy(*code));
        assert!(class.starts_with("CAbility") && (parent.starts_with("CAbility") || *parent == "CBonusBase"));
    }
    for (code, _) in ORDER_IDS_LISTED {
        assert!(CODES.iter().any(|c| c.0 == *code));
    }
}

#[test]
fn possession_flips_a_non_hero_and_consumes_the_caster() {
    let mut h = started();
    let (banshee, foe) = (unit(&h, "banshee"), unit(&h, "foe"));
    h.start("possess", &[]).expect("possess");
    run(&mut h, 3);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(foe).owner, 0);
    assert!(h.game().unit(banshee).removed);
}

#[test]
fn magic_leash_takes_only_summoned_units() {
    let mut h = started();
    let (summon, plain) = (unit(&h, "summon"), unit(&h, "plain"));
    h.game_mut().unit_mut(summon).summoned = true;
    h.start("leash_plain", &[]).expect("leash");
    run(&mut h, 3);
    assert!(!accepted(&h), "a plain unit is refused");
    assert_eq!(h.game().unit(plain).owner, 1);
    h.start("leash_it", &[]).expect("leash");
    run(&mut h, 3);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(summon).owner, 0);
}

#[test]
fn spirit_link_shares_a_hit_between_the_linked_units() {
    let mut h = started();
    let (a, b, cc) = (unit(&h, "a"), unit(&h, "b"), unit(&h, "c"));
    let linker = unit(&h, "linker");
    h.start("link_it", &[]).expect("link");
    run(&mut h, 3);
    assert!(accepted(&h));
    for k in [a, b, cc] {
        assert!(has_buff(&h, k, BUFF_SPIRIT_LINK));
    }
    let foe = unit(&h, "foe");
    let _ = h.game_mut().unit_damage_target(foe, a, f(90.0), false, false, 0, 1);
    // DataA 0.5: the struck unit keeps 45 of 90; the other 45 split over three linked units (15 each, the struck one too).
    assert_eq!(life(&h, a), 440.0);
    assert_eq!(life(&h, b), 485.0);
    assert_eq!(life(&h, cc), 485.0);
    let _ = linker;
}

#[test]
fn an_unlinked_unit_takes_the_whole_hit() {
    let h = started();
    let (a, foe) = (unit(&h, "a"), unit(&h, "foe"));
    let _ = h.game_mut().unit_damage_target(foe, a, f(90.0), false, false, 0, 1);
    assert_eq!(life(&h, a), 410.0);
}

#[test]
fn spell_steal_moves_a_positive_buff_from_an_enemy_to_the_caster() {
    let mut h = started();
    let (thief, buffed) = (unit(&h, "thief"), unit(&h, "buffed"));
    let bl = c(b"Bblo");
    {
        let mut g = h.game_mut();
        let p = sim::abilities::BuffPredicates { positive: true, magic: true, dispellable: true, ..buff_predicates(bl) };
        g.apply_mods_buff(buffed, bl, p, UnitMods::default(), f(40.0));
    }
    assert!(has_buff(&h, buffed, bl));
    h.start("steal_it", &[]).expect("steal");
    run(&mut h, 3);
    assert!(accepted(&h));
    assert!(!has_buff(&h, buffed, bl));
    assert!(has_buff(&h, thief, bl));
}

#[test]
fn call_to_arms_turns_peasants_into_militia_and_back() {
    let mut h = started();
    let pea = unit(&h, "pea");
    h.start("arms", &[]).expect("arms");
    run(&mut h, 3);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(pea).type_id, MILITIA_TYPE);
    assert!(has_buff(&h, pea, BUFF_MILITIA));
    run(&mut h, 600);
    assert_eq!(h.game().unit(pea).type_id, c(b"npea"));
}

#[test]
fn poison_sting_poisons_after_a_hit() {
    let mut h = started();
    let prey = unit(&h, "prey");
    h.start("stab", &[]).expect("stab");
    run(&mut h, 12);
    assert!(has_buff(&h, prey, BUFF_POISON));
    let l = life(&h, prey);
    run(&mut h, 10);
    assert!(life(&h, prey) < l);
}
