//! Behaviour tests of the unit abilities batch A (`unit_abilities_a.rs`, spec `docs/spec/abilities-unit-a.md`). The level
//! data are pinned first-level values of the 1.26a `AbilityData.slk`.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::{code, AbilityType};
use sim::cast::SpellLevel;
use sim::natives::game::{ObjKey, UnitType};
use sim::unit_abilities_a::*;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitAddAbility takes unit whichUnit, integer abilityId returns boolean
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
native SetWidgetLife takes widget wid, real newLife returns nothing
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native KillUnit takes unit whichUnit returns boolean
"#;

const SRC: &str = r#"
globals
    unit src = null
    unit near = null
    unit far = null
    unit foe = null
    unit mate = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set src = CreateUnit(Player(0), 'hfoo', 0.0, 0.0, 0.0)
    set near = CreateUnit(Player(0), 'hfoo', 200.0, 0.0, 0.0)
    set far = CreateUnit(Player(0), 'hfoo', 3000.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 100.0, 100.0, 0.0)
    set mate = CreateUnit(Player(0), 'hfoo', 50.0, 0.0, 0.0)
endfunction
function give_aabr takes nothing returns nothing
    call UnitAddAbility(src, 'Aabr')
endfunction
function take_aabr takes nothing returns nothing
    call UnitRemoveAbility(src, 'Aabr')
endfunction
function give_aarm takes nothing returns nothing
    call UnitAddAbility(src, 'Aarm')
endfunction
function give_aasl takes nothing returns nothing
    call UnitAddAbility(src, 'Aasl')
endfunction
function give_aakb takes nothing returns nothing
    call UnitAddAbility(src, 'Aakb')
endfunction
function give_aetl takes nothing returns nothing
    call UnitAddAbility(src, 'Aetl')
endfunction
function take_aetl takes nothing returns nothing
    call UnitRemoveAbility(src, 'Aetl')
endfunction
"#;

pub const FOOT: u32 = 0x6866_6F6F;

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

pub fn level(area: f32, data: &[f32]) -> SpellLevel {
    let mut d = [F32::ZERO; 9];
    for (i, v) in data.iter().enumerate() {
        d[i] = f(*v);
    }
    SpellLevel { area: f(area), data: d, ..Default::default() }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(
            FOOT,
            UnitType {
                name: "Footman".to_string(),
                max_life: f(1000.0),
                max_mana: f(200.0),
                move_speed: f(270.0),
                ..Default::default()
            },
        );
        for (c, l) in [
            // Statue regeneration: 0.4 % of the max life per second, radius 700.
            (code(b"Aabr"), level(700.0, &[0.004, 1.0])),
            // Neutral regen (mana only): 1 % of the max mana per second, radius 500.
            (code(b"Aarm"), level(500.0, &[0.01, 1.0])),
            // Slow aura: movement -60 %, radius 600.
            (code(b"Aasl"), level(600.0, &[-0.6, 0.0])),
            // War drums: +10 % damage, radius 900.
            (code(b"Aakb"), level(900.0, &[0.1, 1.0, 1.0])),
            (code(b"Aetl"), level(0.0, &[])),
        ] {
            g.ability_types_mut().insert(c, AbilityType { max_level: Some(1) });
            g.set_spell_levels(c, vec![l]);
        }
    }
    h
}

fn unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
    assert!(h.misfires().is_empty(), "no thread was refused");
}

fn started() -> ScriptHost {
    let mut h = host();
    call(&mut h, "setup");
    run(&mut h, 1);
    h
}

fn has_buff(h: &ScriptHost, k: ObjKey, c: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == c)
}

#[test]
fn registry_covers_every_code_once() {
    let wanted = "Aabr Aabs Aaha Aakb Aall Aami Aamk Aams Aapl Aarm Aasl Aast Aave Aawa Abar Abdt Abgm Abli Ablp Abof Abrf Absk Abtl Abun Abur Acan Acar Acha Achd Achl Aclf Acmg Acoa Acoh Acoi Acor Acpf Adch Adda Adec Adef Adet Adev Adri Adro Adta Adtn Adts Advc Advm Aeat Aegm Aenc Aent Aesn Aetf Aeth Aetl Aexh Afak Afla Aflk Afrb Afrz Afsh Afzy Agho Agld Agra Agyb Agyd Agyv Ahar Ahid Ahrl Aimp Alam Alea Aliq Alit Aloa";
    let mut seen = std::collections::BTreeSet::new();
    for w in wanted.split_whitespace() {
        let c = code(w.as_bytes().try_into().unwrap());
        assert!(info(c).is_some(), "{w} registered");
        assert!(seen.insert(c));
    }
    assert_eq!(ABILITIES.len(), seen.len());
    assert!(ABILITIES.iter().all(|i| !i.class.is_empty() && !i.note.is_empty()));
}

#[test]
fn ethereal_passive_counts_once_and_releases() {
    let mut h = started();
    let src = unit(&h, "src");
    call(&mut h, "give_aetl");
    run(&mut h, 1);
    assert_eq!(h.game().unit(src).ethereal, 1);
    call(&mut h, "take_aetl");
    run(&mut h, 1);
    assert_eq!(h.game().unit(src).ethereal, 0);
}

#[test]
fn blight_regen_aura_heals_friends_in_range_by_fraction_of_max_life() {
    let mut h = started();
    let (src, near, far, foe) = (unit(&h, "src"), unit(&h, "near"), unit(&h, "far"), unit(&h, "foe"));
    call(&mut h, "give_aabr");
    run(&mut h, 8);
    assert!(has_buff(&h, near, BUFF_BLIGHT_REGEN));
    assert!(!has_buff(&h, src, BUFF_BLIGHT_REGEN), "targs lack self: the relation stage gives the caster the self bit only");
    let _ = src;
    assert!(!has_buff(&h, far, BUFF_BLIGHT_REGEN));
    assert!(!has_buff(&h, foe, BUFF_BLIGHT_REGEN), "the row names friend, not enemy");
    // 0.004 * 1000 = 4 life per second on the buff.
    assert_eq!(h.game().unit(near).bonuses.life_regen.to_f32(), 4.0);
    // the source loses the ability: the buff runs out within the 4.0 s duration
    call(&mut h, "take_aabr");
    run(&mut h, 60);
    assert!(!has_buff(&h, near, BUFF_BLIGHT_REGEN));
    assert_eq!(h.game().unit(near).bonuses.life_regen.to_f32(), 0.0);
}

#[test]
fn regen_mana_aura_has_no_relation_so_it_reaches_enemies() {
    let mut h = started();
    let (near, foe) = (unit(&h, "near"), unit(&h, "foe"));
    call(&mut h, "give_aarm");
    run(&mut h, 8);
    assert!(has_buff(&h, near, BUFF_REGEN_MANA));
    assert!(has_buff(&h, foe, BUFF_REGEN_MANA));
    assert!((h.game().unit(near).bonuses.mana_regen.to_f32() - 2.0).abs() < 1e-5);
}

#[test]
fn slow_aura_slows_enemies_only() {
    let mut h = started();
    let (near, foe) = (unit(&h, "near"), unit(&h, "foe"));
    call(&mut h, "give_aasl");
    run(&mut h, 8);
    assert!(has_buff(&h, foe, BUFF_AURA_SLOW));
    assert!(!has_buff(&h, near, BUFF_AURA_SLOW));
    assert!((h.game().unit(foe).bonuses.move_mult.to_f32() - 0.4).abs() < 1e-6);
}

#[test]
fn war_drums_raise_the_damage_of_friends() {
    let mut h = started();
    let (src, near, foe) = (unit(&h, "src"), unit(&h, "near"), unit(&h, "foe"));
    call(&mut h, "give_aakb");
    run(&mut h, 8);
    assert!(has_buff(&h, src, BUFF_WAR_DRUMS), "self is in the targs");
    assert!(has_buff(&h, near, BUFF_WAR_DRUMS));
    assert!(!has_buff(&h, foe, BUFF_WAR_DRUMS));
    assert!((h.game().unit(near).bonuses.damage_pct.to_f32() - 0.1).abs() < 1e-6);
}

// ---- cargo ----

const CARGO_SRC: &str = r#"
function give_cargo takes nothing returns nothing
    call UnitAddAbility(src, 'Acar')
    call UnitAddAbility(src, 'Aloa')
    call UnitAddAbility(src, 'Adri')
    call UnitAddAbility(src, 'Adro')
endfunction
function load_near takes nothing returns nothing
    set accepted = IssueTargetOrderById(src, 852046, near)
endfunction
function load_mate takes nothing returns nothing
    set accepted = IssueTargetOrderById(src, 852046, mate)
endfunction
function load_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(src, 852046, foe)
endfunction
function unload_all takes nothing returns nothing
    set accepted = IssueImmediateOrderById(src, 852049)
endfunction
function kill_src takes nothing returns nothing
    call SetWidgetLife(src, 0.0)
endfunction
function give_death_hold takes nothing returns nothing
    call UnitAddAbility(src, 'Achd')
endfunction
function give_devour takes nothing returns nothing
    call UnitAddAbility(src, 'Adev')
    call UnitAddAbility(src, 'Advc')
endfunction
function devour_foe takes nothing returns nothing
    set accepted = IssueTargetOrderById(src, 852104, foe)
endfunction
"#;

fn cargo_host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}\n{CARGO_SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(
            FOOT,
            UnitType {
                name: "Footman".to_string(),
                max_life: f(1000.0),
                max_mana: f(200.0),
                move_speed: f(270.0),
                ..Default::default()
            },
        );
        for (c, l) in [
            (code(b"Acar"), level(0.0, &[2.0])),
            (code(b"Achd"), level(0.0, &[10.0])),
            (code(b"Advc"), level(0.0, &[2.0, 5.0, 5.0])),
            (code(b"Aloa"), SpellLevel { rng: f(400.0), ..level(0.0, &[]) }),
            (code(b"Adev"), SpellLevel { rng: f(400.0), ..level(0.0, &[5.0]) }),
            (code(b"Adri"), level(0.0, &[])),
            (code(b"Adro"), level(0.0, &[])),
        ] {
            g.ability_types_mut().insert(c, AbilityType { max_level: Some(1) });
            g.set_spell_levels(c, vec![l]);
        }
    }
    h
}

fn cargo_started() -> ScriptHost {
    let mut h = cargo_host();
    call(&mut h, "setup");
    run(&mut h, 1);
    call(&mut h, "give_cargo");
    run(&mut h, 1);
    h
}

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
}

#[test]
fn load_fills_the_hold_up_to_its_capacity_and_unload_all_empties_it() {
    let mut h = cargo_started();
    let (src, near, mate, foe) = (unit(&h, "src"), unit(&h, "near"), unit(&h, "mate"), unit(&h, "foe"));
    assert_eq!(h.game().cargo_capacity(src), 2);
    call(&mut h, "load_near");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(h.game().carrier_of(near), Some(src));
    assert!(h.game().unit(near).loaded);
    call(&mut h, "load_mate");
    run(&mut h, 2);
    assert_eq!(h.game().cargo_of(src), &[near, mate]);
    // an enemy is no friend; a full hold refuses
    call(&mut h, "load_foe");
    run(&mut h, 2);
    assert!(!accepted(&h));
    call(&mut h, "unload_all");
    run(&mut h, 2);
    assert!(h.game().cargo_of(src).is_empty());
    assert!(!h.game().unit(near).loaded && !h.game().unit(mate).loaded);
    assert_eq!(h.game().carrier_of(near), None);
    // the two come out on the first two ring slots, 96 units from the holder
    let g = h.game();
    let (sx, sy) = (g.unit(src).x.to_f32(), g.unit(src).y.to_f32());
    let n = g.unit(near);
    assert!(((n.x.to_f32() - sx) - 96.0).abs() < 0.01 && (n.y.to_f32() - sy).abs() < 0.01);
    let m = g.unit(mate);
    assert!((m.x.to_f32() - sx).abs() < 0.01 && ((m.y.to_f32() - sy) - 96.0).abs() < 0.01);
}

#[test]
fn a_dying_death_hold_takes_the_cargo_with_it_and_a_plain_hold_lets_it_out() {
    let mut h = cargo_started();
    let (src, near) = (unit(&h, "src"), unit(&h, "near"));
    call(&mut h, "load_near");
    run(&mut h, 2);
    call(&mut h, "kill_src");
    run(&mut h, 8);
    assert!(h.game().cargo_of(src).is_empty());
    assert!(!h.game().unit(near).dead, "a plain hold frees its cargo");
    assert!(!h.game().unit(near).loaded);

    let mut h = cargo_started();
    let (src, near) = (unit(&h, "src"), unit(&h, "near"));
    call(&mut h, "give_death_hold");
    run(&mut h, 1);
    call(&mut h, "load_near");
    run(&mut h, 2);
    call(&mut h, "kill_src");
    run(&mut h, 8);
    assert!(h.game().cargo_of(src).is_empty());
    assert!(h.game().unit(near).dead, "the death hold kills its cargo");
}

#[test]
fn devour_digests_the_target_until_it_dies() {
    let mut h = cargo_host();
    call(&mut h, "setup");
    run(&mut h, 1);
    call(&mut h, "give_devour");
    run(&mut h, 1);
    let (src, foe) = (unit(&h, "src"), unit(&h, "foe"));
    h.game_mut().write_life(foe, f(20.0));
    call(&mut h, "devour_foe");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert_eq!(h.game().carrier_of(foe), Some(src));
    assert!(has_buff(&h, foe, BUFF_DEVOURED) && has_buff(&h, src, BUFF_DIGEST));
    // 5 per second: three ticks leave 5 life, the fourth kills.
    run(&mut h, 35);
    assert!(!h.game().unit(foe).dead);
    assert!((h.game().unit(foe).life.to_f32() - 5.0).abs() < 0.01);
    run(&mut h, 10);
    assert!(h.game().unit(foe).dead);
    assert!(h.game().cargo_of(src).is_empty());
}

// ---- detection and ghost hooks ----

const DETECT_SRC: &str = r#"
function give_detector takes nothing returns nothing
    call UnitAddAbility(src, 'Adts')
endfunction
function drop_detector takes nothing returns nothing
    call UnitRemoveAbility(src, 'Adts')
endfunction
function give_ghost takes nothing returns nothing
    call UnitAddAbility(foe, 'Agho')
    call UnitAddAbility(src, 'Aeth')
endfunction
"#;

fn detect_host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}\n{DETECT_SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(
            FOOT,
            UnitType { name: "Footman".to_string(), max_life: f(1000.0), move_speed: f(270.0), ..Default::default() },
        );
        for (c, l) in [
            // Magic Sentry: radius 900.
            (code(b"Adts"), SpellLevel { rng: f(900.0), ..level(0.0, &[3.0]) }),
            (code(b"Agho"), level(0.0, &[1.0, 1.0, 0.0])),
            (code(b"Aeth"), level(0.0, &[1.0, 1.0])),
        ] {
            g.ability_types_mut().insert(c, AbilityType { max_level: Some(1) });
            g.set_spell_levels(c, vec![l]);
        }
    }
    h
}

#[test]
fn a_detector_detects_units_in_its_range_for_its_player_and_loses_them_when_removed() {
    let mut h = detect_host();
    call(&mut h, "setup");
    run(&mut h, 1);
    let (near, far, foe) = (unit(&h, "near"), unit(&h, "far"), unit(&h, "foe"));
    call(&mut h, "give_detector");
    run(&mut h, 2);
    assert!(h.game().unit_detected_by(foe, 0));
    assert!(h.game().unit_detected_by(near, 0));
    assert!(!h.game().unit_detected_by(far, 0), "3000 away");
    assert!(!h.game().unit_detected_by(foe, 1), "the bit is the detector's player");
    call(&mut h, "drop_detector");
    run(&mut h, 2);
    assert!(!h.game().unit_detected_by(foe, 0));
}

#[test]
fn ghost_and_ghost_seer_are_tracked_for_the_invisibility_module() {
    let mut h = detect_host();
    call(&mut h, "setup");
    run(&mut h, 1);
    let (src, foe) = (unit(&h, "src"), unit(&h, "foe"));
    call(&mut h, "give_ghost");
    run(&mut h, 1);
    assert!(h.game().is_ghost(foe) && h.game().sees_ghosts(src));
    assert!(!h.game().ghost_visible_to(foe, 0), "player 0 does not detect the ghost");
    assert!(h.game().ghost_visible_to(foe, 1), "its owner always sees it");
    call(&mut h, "give_detector");
    run(&mut h, 2);
    assert!(h.game().ghost_visible_to(foe, 0), "a detector of player 0 reveals it");
}
