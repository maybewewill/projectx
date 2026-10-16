//! Behaviour tests of the neutral and creep abilities (`neutral_spells.rs`, spec `docs/spec/abilities-neutral.md`).
//! The level numbers are pinned test values. The Pulverize rolls need the RNG table (`GAME_DLL`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::AbilityType;
use sim::cast::SpellLevel;
use sim::combat::AttackType;
use sim::natives::game::{ObjKey, UnitType};
use sim::neutral_spells::*;
use sim::orders::{AbilityKind, TypeOrderData};
use sim::rng::RngTable;
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native UnitRemoveAbility takes unit whichUnit, integer abilityId returns boolean
native SetWidgetLife takes widget wid, real newLife returns nothing
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native IssueTargetOrderById takes unit whichUnit, integer order, widget target returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget target returns boolean
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
"#;

const SRC: &str = r#"
globals
    unit shaman = null
    unit grunt = null
    unit priest = null
    unit hurt = null
    unit immune = null
    unit bolter = null
    unit foe = null
    unit spear = null
    unit victim = null
    unit pulv = null
    unit near = null
    unit mid = null
    unit far = null
    unit ally = null
    unit cleaver = null
    unit cl_victim = null
    unit cl_next = null
    unit cl_far = null
    unit cl_ally = null
    unit shielded = null
    unit attacker = null
    unit cyc = null
    unit cyc_tgt = null
    unit silencer = null
    unit charmer = null
    unit charm_tgt = null
    unit charm_big = null
    unit howler = null
    unit howl_tgt = null
    unit rain = null
    boolean accepted = false
endglobals
function setup takes nothing returns nothing
    set shaman = CreateUnit(Player(0), 'ngsh', 0.0, 0.0, 0.0)
    set grunt = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    set priest = CreateUnit(Player(0), 'nprt', 2000.0, 0.0, 0.0)
    call IssueImmediateOrderById(priest, 852250)
    set hurt = CreateUnit(Player(0), 'hfoo', 2100.0, 0.0, 0.0)
    call SetWidgetLife(hurt, 400.0)
    set immune = CreateUnit(Player(1), 'nimm', 3000.0, 0.0, 0.0)
    set bolter = CreateUnit(Player(0), 'nblt', 4000.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 4100.0, 0.0, 0.0)
    set spear = CreateUnit(Player(0), 'nspr', 6000.0, 0.0, 0.0)
    set victim = CreateUnit(Player(1), 'hfoo', 6050.0, 0.0, 180.0)
endfunction
function setup_pulv takes nothing returns nothing
    set pulv = CreateUnit(Player(0), 'npul', 8000.0, 0.0, 0.0)
    set near = CreateUnit(Player(1), 'hfoo', 8080.0, 0.0, 0.0)
    set mid = CreateUnit(Player(1), 'hfoo', 8200.0, 0.0, 0.0)
    set far = CreateUnit(Player(1), 'hfoo', 8400.0, 0.0, 0.0)
    set ally = CreateUnit(Player(0), 'hfoo', 8100.0, 50.0, 0.0)
endfunction
function lust takes nothing returns nothing
    set accepted = IssueTargetOrderById(shaman, 852101, grunt)
endfunction
function heal_hurt takes nothing returns nothing
    set accepted = IssueTargetOrderById(priest, 852248, hurt)
endfunction
function heal_off takes nothing returns nothing
    set accepted = IssueImmediateOrderById(priest, 852250)
endfunction
function heal_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(priest, 852249)
endfunction
function drop_immunity takes nothing returns nothing
    call UnitRemoveAbility(immune, 'ACmi')
endfunction
function bolt takes nothing returns nothing
    set accepted = IssueTargetOrderById(bolter, 852252, foe)
endfunction
function stab takes nothing returns nothing
    call IssueTargetOrder(spear, "attack", victim)
endfunction
function bash_it takes nothing returns nothing
    call IssueTargetOrder(pulv, "attack", near)
endfunction
function setup_s43 takes nothing returns nothing
    set cleaver = CreateUnit(Player(0), 'ncle', 20000.0, 0.0, 0.0)
    set cl_victim = CreateUnit(Player(1), 'ndmy', 20050.0, 0.0, 0.0)
    set cl_next = CreateUnit(Player(1), 'ndmy', 20050.0, 120.0, 0.0)
    set cl_far = CreateUnit(Player(1), 'ndmy', 20050.0, 600.0, 0.0)
    set cl_ally = CreateUnit(Player(0), 'ndmy', 20050.0, -100.0, 0.0)
    set shielded = CreateUnit(Player(0), 'nshl', 22000.0, 0.0, 0.0)
    set attacker = CreateUnit(Player(1), 'natk', 22050.0, 0.0, 180.0)
    set cyc = CreateUnit(Player(0), 'ncyc', 24000.0, 0.0, 0.0)
    set cyc_tgt = CreateUnit(Player(1), 'ndmy', 24300.0, 0.0, 0.0)
    set silencer = CreateUnit(Player(1), 'nsil', 500.0, 0.0, 0.0)
    set charmer = CreateUnit(Player(0), 'nchr', 28000.0, 0.0, 0.0)
    set charm_tgt = CreateUnit(Player(1), 'nlow', 28300.0, 0.0, 0.0)
    set charm_big = CreateUnit(Player(1), 'nbig', 28300.0, 100.0, 0.0)
    set howler = CreateUnit(Player(0), 'nhwl', 30000.0, 0.0, 0.0)
    set howl_tgt = CreateUnit(Player(1), 'ndmy', 30200.0, 0.0, 0.0)
    set rain = CreateUnit(Player(0), 'nrfp', 32000.0, 0.0, 0.0)
endfunction
function cleave_it takes nothing returns nothing
    call IssueTargetOrder(cleaver, "attack", cl_victim)
endfunction
function shield_on takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shielded, 852589)
endfunction
function shield_off takes nothing returns nothing
    set accepted = IssueImmediateOrderById(shielded, 852590)
endfunction
function hit_shielded takes nothing returns nothing
    call IssueTargetOrder(attacker, "attack", shielded)
endfunction
function cyclone_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(cyc, 852144, cyc_tgt)
endfunction
function silence_it takes nothing returns nothing
    set accepted = IssuePointOrderById(silencer, 852592, 0.0, 0.0)
endfunction
function charm_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(charmer, 852581, charm_tgt)
endfunction
function charm_big_it takes nothing returns nothing
    set accepted = IssueTargetOrderById(charmer, 852581, charm_big)
endfunction
function howl_it takes nothing returns nothing
    set accepted = IssueImmediateOrderById(howler, 852588)
endfunction
"#;

const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const SHAMAN: u32 = 0x6E67_7368; // 'ngsh'
const PRIEST: u32 = 0x6E70_7274; // 'nprt'
const IMMUNE: u32 = 0x6E69_6D6D; // 'nimm'
const BOLTER: u32 = 0x6E62_6C74; // 'nblt'
const SPEAR: u32 = 0x6E73_7072; // 'nspr'
const PULV: u32 = 0x6E70_756C; // 'npul'

const CREEP_BLOODLUST: u32 = u32::from_be_bytes(*b"ACbl");
const CREEP_IMMUNITY: u32 = u32::from_be_bytes(*b"ACmi");
const CREEP_VENOM: u32 = u32::from_be_bytes(*b"ACvs");
const CREEP_PULVERIZE: u32 = u32::from_be_bytes(*b"ACpv");
const CREEP_THUNDER_BOLT: u32 = u32::from_be_bytes(*b"ACtb");
const CREEP_RAIN: u32 = u32::from_be_bytes(*b"ANrf");
const DUMMY: u32 = u32::from_be_bytes(*b"ndmy");
const CLEAVER: u32 = u32::from_be_bytes(*b"ncle");
const SHIELDED: u32 = u32::from_be_bytes(*b"nshl");
const ATTACKER: u32 = u32::from_be_bytes(*b"natk");
const CYCLONER: u32 = u32::from_be_bytes(*b"ncyc");
const SILENCER: u32 = u32::from_be_bytes(*b"nsil");
const CHARMER: u32 = u32::from_be_bytes(*b"nchr");
const HOWLER: u32 = u32::from_be_bytes(*b"nhwl");
const RAINER: u32 = u32::from_be_bytes(*b"nrfp");
const LOW: u32 = u32::from_be_bytes(*b"nlow");
const BIG: u32 = u32::from_be_bytes(*b"nbig");

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

fn ut(name: &str, life: f32, abilities: Vec<u32>) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: f(life),
        max_mana: f(300.0),
        start_mana: f(300.0),
        move_speed: f(270.0),
        abilities,
        ..Default::default()
    }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, ab) in [
            (FOOT, vec![]),
            (SHAMAN, vec![CREEP_BLOODLUST]),
            (PRIEST, vec![NEUTRAL_HEAL]),
            (IMMUNE, vec![CREEP_IMMUNITY]),
            (BOLTER, vec![CREEP_THUNDER_BOLT]),
            (SPEAR, vec![CREEP_VENOM]),
            (PULV, vec![CREEP_PULVERIZE]),
        ] {
            g.unit_types_mut().insert(id, ut("unit", 500.0, ab));
        }
        for (id, ab, life) in [
            (DUMMY, vec![], 500.0),
            (CLEAVER, vec![sim::neutral_spells::CLEAVE], 500.0),
            (SHIELDED, vec![sim::neutral_spells::MANA_SHIELD], 500.0),
            (ATTACKER, vec![], 500.0),
            (CYCLONER, vec![sim::neutral_spells::CYCLONE], 500.0),
            (SILENCER, vec![sim::neutral_spells::SILENCE], 500.0),
            (CHARMER, vec![sim::neutral_spells::CHARM], 500.0),
            (HOWLER, vec![sim::neutral_spells::HOWL], 500.0),
            (RAINER, vec![CREEP_RAIN], 500.0),
        ] {
            g.unit_types_mut().insert(id, ut("unit", life, ab));
        }
        for (id, level) in [(LOW, 2), (BIG, 9)] {
            let mut t = ut("unit", 500.0, vec![]);
            t.level = level;
            g.unit_types_mut().insert(id, t);
        }
        for id in [FOOT, SPEAR, PULV, CLEAVER, ATTACKER] {
            g.set_type_orders(id, TypeOrderData { abilities: vec![AbilityKind::Move, AbilityKind::Attack] });
            g.set_attack_type(
                id,
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
        }
        // The alias rows (the engine fills these from the `code` column / the static parents).
        for (alias, base) in [
            (CREEP_BLOODLUST, sim::unit_spells::BLOODLUST),
            (CREEP_IMMUNITY, MAGIC_IMMUNITY),
            (CREEP_VENOM, VENOM_SPEAR),
            (CREEP_PULVERIZE, PULVERIZE),
            (CREEP_THUNDER_BOLT, sim::cast::STORM_BOLT),
            (NEUTRAL_HEAL, sim::unit_spells::HEAL),
        ] {
            g.set_spell_alias(alias, base);
        }
        // Creep Bloodlust: its own numbers (attack +0.6, move +0.3), not the base row's.
        g.set_spell_levels(
            CREEP_BLOODLUST,
            vec![SpellLevel { cost: 40, cool: f(1.0), dur: f(5.0), hero_dur: f(5.0), rng: f(600.0), data: data(&[0.6, 0.3]), ..Default::default() }],
        );
        g.set_spell_levels(
            NEUTRAL_HEAL,
            vec![SpellLevel { cost: 5, cool: f(1.0), rng: f(250.0), data: data(&[12.0]), ..Default::default() }],
        );
        g.set_spell_levels(CREEP_IMMUNITY, vec![SpellLevel::default()]);
        g.set_spell_levels(
            CREEP_THUNDER_BOLT,
            vec![SpellLevel { cost: 25, cool: f(1.0), dur: f(5.0), hero_dur: f(3.0), rng: f(600.0), data: data(&[100.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            CREEP_VENOM,
            vec![SpellLevel { dur: f(20.0), hero_dur: f(10.0), data: data(&[2.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            CREEP_PULVERIZE,
            vec![SpellLevel { data: data(&[100.0, 40.0, 150.0, 250.0]), ..Default::default() }],
        );
        g.set_spell_alias(CREEP_RAIN, sim::human_heroes::BLIZZARD);
        g.set_spell_levels(
            sim::neutral_spells::CLEAVE,
            vec![SpellLevel { area: f(200.0), data: data(&[0.5]), ..Default::default() }],
        );
        g.set_spell_levels(
            sim::neutral_spells::MANA_SHIELD,
            vec![SpellLevel { cost: 25, cool: f(1.0), data: data(&[2.0, 1.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            sim::neutral_spells::CYCLONE,
            vec![SpellLevel { cost: 50, cool: f(1.0), dur: f(5.0), hero_dur: f(3.0), rng: f(600.0), ..Default::default() }],
        );
        g.set_spell_levels(
            sim::neutral_spells::SILENCE,
            vec![SpellLevel { cost: 50, cool: f(1.0), dur: f(10.0), hero_dur: f(5.0), area: f(300.0), rng: f(900.0), ..Default::default() }],
        );
        g.set_spell_levels(
            sim::neutral_spells::CHARM,
            vec![SpellLevel { cost: 50, cool: f(1.0), rng: f(700.0), data: data(&[5.0]), ..Default::default() }],
        );
        g.set_spell_levels(
            sim::neutral_spells::HOWL,
            vec![SpellLevel { cost: 50, cool: f(1.0), dur: f(15.0), hero_dur: f(15.0), area: f(500.0), data: data(&[0.4]), ..Default::default() }],
        );
        for id in [
            CREEP_BLOODLUST,
            NEUTRAL_HEAL,
            CREEP_IMMUNITY,
            CREEP_THUNDER_BOLT,
            CREEP_VENOM,
            CREEP_PULVERIZE,
            sim::neutral_spells::CLEAVE,
            sim::neutral_spells::MANA_SHIELD,
            sim::neutral_spells::CYCLONE,
            sim::neutral_spells::SILENCE,
            sim::neutral_spells::CHARM,
            sim::neutral_spells::HOWL,
            CREEP_RAIN,
        ] {
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
    assert!(h.misfires().is_empty(), "no thread was refused");
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}

fn has_buff(h: &ScriptHost, k: ObjKey, code: u32) -> bool {
    h.game().buffs(k).iter().any(|b| b.base == code)
}

#[test]
fn an_alias_row_is_attached_as_the_modelled_ability_and_reads_its_own_levels() {
    let h = started();
    let shaman = unit(&h, "shaman");
    let g = h.game();
    let a = g.unit(shaman).abilities.get(sim::unit_spells::BLOODLUST).expect("modelled ability attached");
    assert_eq!(a.base, sim::unit_spells::BLOODLUST);
    assert_eq!(a.source, CREEP_BLOODLUST);
    assert!(g.unit(shaman).abilities.get(CREEP_BLOODLUST).is_none());
    assert_eq!(g.spell_alias_base(CREEP_BLOODLUST), sim::unit_spells::BLOODLUST);
    assert_eq!(g.spell_alias_base(FOOT), FOOT);
}

#[test]
fn creep_bloodlust_runs_the_bloodlust_effect_with_the_alias_numbers() {
    let mut h = started();
    let (shaman, grunt) = (unit(&h, "shaman"), unit(&h, "grunt"));
    h.start("lust", &[]).expect("lust");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, grunt, sim::unit_spells::BUFF_BLOODLUST));
    assert_eq!(h.game().unit(shaman).mana.to_f32(), 260.0, "cost 40 from the alias row");
    assert!((h.game().unit(grunt).bonuses.attack_mult.to_f32() - 1.6).abs() < 1e-5, "alias DataA 0.6");
    run(&mut h, 55);
    assert!(!has_buff(&h, grunt, sim::unit_spells::BUFF_BLOODLUST));
}

#[test]
fn neutral_heal_uses_its_own_orders_and_the_heal_effect() {
    let mut h = started();
    let (priest, hurt) = (unit(&h, "priest"), unit(&h, "hurt"));
    // The setup switched autocast off with the neutral `healoff` order (852250) before the first scan.
    assert!(!h.game().autocast_on(priest, sim::unit_spells::HEAL));
    run(&mut h, 80);
    assert_eq!(life(&h, hurt), 400.0, "no autocast heal while it is off");
    h.start("heal_hurt", &[]).expect("heal");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert_eq!(life(&h, hurt), 412.0);
    assert_eq!(h.game().unit(priest).mana.to_f32(), 295.0);
    assert!(h.game().is_cooling(priest, sim::unit_spells::HEAL));
}

#[test]
fn magic_immunity_counts_once_and_is_taken_back_with_the_ability() {
    let mut h = started();
    let imm = unit(&h, "immune");
    assert_eq!(h.game().unit(imm).magic_immune, 1);
    h.start("drop_immunity", &[]).expect("drop");
    run(&mut h, 1);
    assert_eq!(h.game().unit(imm).magic_immune, 0);
}

#[test]
fn the_creep_thunder_bolt_parent_runs_storm_bolt_with_the_creep_orders() {
    let mut h = started();
    let (bolter, foe) = (unit(&h, "bolter"), unit(&h, "foe"));
    h.start("bolt", &[]).expect("bolt");
    run(&mut h, 20);
    assert!(accepted(&h), "order 852252 is the creep thunder bolt");
    assert_eq!(life(&h, foe), 400.0, "alias DataA 100 damage");
    assert_eq!(h.game().unit(bolter).mana.to_f32(), 275.0);
    assert!(has_buff(&h, foe, sim::cast::BUFF_STUN), "Dur 5 stun");
}

#[test]
fn envenomed_spears_poison_the_target_after_a_hit() {
    let mut h = started();
    let victim = unit(&h, "victim");
    h.start("stab", &[]).expect("stab");
    run(&mut h, 12);
    assert!(has_buff(&h, victim, BUFF_POISON), "poisoned after the first hit");
    let after_hit = life(&h, victim);
    run(&mut h, 10);
    assert!(life(&h, victim) < after_hit, "damage over time");
}

fn rng_table() -> Option<RngTable> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL"))
}

fn pulv_scene(chance: f32) -> Option<ScriptHost> {
    let table = rng_table()?;
    let mut h = host();
    h.set_rng_table(table);
    h.start_game_rng(0x0BAD_5EED);
    h.game_mut().set_spell_levels(
        CREEP_PULVERIZE,
        vec![SpellLevel { data: data(&[chance, 40.0, 150.0, 250.0]), ..Default::default() }],
    );
    h.start("setup", &[]).expect("setup");
    h.step();
    h.start("setup_pulv", &[]).expect("setup_pulv");
    h.step();
    Some(h)
}

#[test]
fn pulverize_with_a_certain_chance_hits_two_rings_around_the_caster() {
    let Some(mut h) = pulv_scene(100.0) else { return };
    let (pulv, near, mid, far, ally) = (unit(&h, "pulv"), unit(&h, "near"), unit(&h, "mid"), unit(&h, "far"), unit(&h, "ally"));
    assert_eq!(h.game().pulverize_count(pulv), 1);
    h.start("bash_it", &[]).expect("attack");
    run(&mut h, 8);
    // Inside the full ring (150): 40; inside the outer ring (250): 20; outside: nothing; allies: nothing.
    assert!(life(&h, near) <= 460.0, "full ring (plus the normal hit)");
    assert_eq!(life(&h, mid), 480.0, "half damage in the outer ring");
    assert_eq!(life(&h, far), 500.0, "outside both rings");
    assert_eq!(life(&h, ally), 500.0, "allies are not hit");
    assert_eq!(life(&h, pulv), 500.0);
    assert!(!h.game().pulverize_armed(pulv), "the flag is spent at the execute point");
}

#[test]
fn pulverize_without_a_chance_only_counts_swings() {
    let Some(mut h) = pulv_scene(0.0) else { return };
    let (pulv, mid) = (unit(&h, "pulv"), unit(&h, "mid"));
    h.start("bash_it", &[]).expect("attack");
    run(&mut h, 20);
    assert!(h.game().pulverize_count(pulv) >= 2, "each unarmed swing raises the counter");
    assert_eq!(life(&h, mid), 500.0);
}

#[test]
fn the_static_parents_name_modelled_abilities() {
    for (_, parent, _) in STATIC_PARENTS {
        assert!(MODELLED.iter().any(|m| u32::from_be_bytes(*m) == parent), "{parent:08X}");
    }
    // The group rule: a static parent wins, then the code itself, then a modelled member of the group.
    let aowd = u32::from_be_bytes(*b"AOwd");
    let aosw = u32::from_be_bytes(*b"AOsw");
    let arsw = u32::from_be_bytes(*b"Arsw");
    assert_eq!(standard_for_group(aowd, &[aosw, arsw]), Some(aosw));
    assert_eq!(standard_for_group(u32::from_be_bytes(*b"ACtb"), &[u32::from_be_bytes(*b"ACtb")]), Some(u32::from_be_bytes(*b"AHtb")));
    assert_eq!(standard_for_group(u32::from_be_bytes(*b"Zzzz"), &[u32::from_be_bytes(*b"Zzzz")]), None);
    assert!(is_item_row(u32::from_be_bytes(*b"AIcl")));
    assert!(!is_item_row(u32::from_be_bytes(*b"ACcl")));
}

fn started_s43() -> ScriptHost {
    let mut h = started();
    h.start("setup_s43", &[]).expect("setup_s43");
    h.step();
    h
}

#[test]
fn cleaving_attack_hits_the_enemies_around_the_target_with_a_share_of_the_damage() {
    let mut h = started_s43();
    let (victim, next, far, ally, cleaver) =
        (unit(&h, "cl_victim"), unit(&h, "cl_next"), unit(&h, "cl_far"), unit(&h, "cl_ally"), unit(&h, "cleaver"));
    h.start("cleave_it", &[]).expect("attack");
    run(&mut h, 14);
    let hit = 500.0 - life(&h, victim);
    assert!(hit > 0.0, "the main target was hit");
    let side = 500.0 - life(&h, next);
    assert!(side > 0.0, "the unit beside the target took the cleave");
    assert!((side - 0.5 * hit).abs() < 1e-3, "DataA 0.5 of the damage: {side} vs {hit}");
    assert_eq!(life(&h, far), 500.0, "outside the area");
    assert_eq!(life(&h, ally), 500.0, "allies are not cleaved");
    assert_eq!(life(&h, cleaver), 500.0);
}

#[test]
fn mana_shield_pays_damage_with_mana_until_it_is_switched_off() {
    let mut h = started_s43();
    let (shielded, _attacker) = (unit(&h, "shielded"), unit(&h, "attacker"));
    h.start("shield_on", &[]).expect("on");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(has_buff(&h, shielded, sim::neutral_spells::BUFF_MANA_SHIELD));
    assert_eq!(h.game().unit(shielded).mana.to_f32(), 275.0, "cost 25");
    h.start("hit_shielded", &[]).expect("hit");
    run(&mut h, 12);
    assert_eq!(life(&h, shielded), 500.0, "the shield takes all the damage");
    let mana = h.game().unit(shielded).mana.to_f32();
    assert!(mana < 275.0, "damage 10 per hit costs 5 mana at 2 damage per mana point: {mana}");
    h.start("shield_off", &[]).expect("off");
    run(&mut h, 1);
    assert!(accepted(&h));
    assert!(!has_buff(&h, shielded, sim::neutral_spells::BUFF_MANA_SHIELD));
    run(&mut h, 20);
    assert!(life(&h, shielded) < 500.0, "without the shield the damage reaches the life");
}

#[test]
fn cyclone_disables_and_protects_the_target_for_the_duration() {
    let mut h = started_s43();
    let target = unit(&h, "cyc_tgt");
    h.start("cyclone_it", &[]).expect("cyclone");
    run(&mut h, 20);
    assert!(accepted(&h));
    assert!(h.game().is_stunned(target), "cannot act");
    assert!(h.game().unit_is_invulnerable(target), "cannot be hurt");
    run(&mut h, 60);
    assert!(!h.game().is_stunned(target));
    assert!(!h.game().unit_is_invulnerable(target));
}

#[test]
fn silence_stops_spell_casting_in_the_area() {
    let mut h = started_s43();
    let shaman = unit(&h, "shaman");
    h.start("silence_it", &[]).expect("silence");
    run(&mut h, 10);
    assert!(accepted(&h));
    assert!(has_buff(&h, shaman, sim::neutral_spells::BUFF_SILENCE));
    assert!(h.game().is_silenced(shaman));
    h.start("lust", &[]).expect("lust");
    run(&mut h, 1);
    assert!(!accepted(&h), "a silenced caster cannot start a spell");
    run(&mut h, 120);
    assert!(!h.game().is_silenced(shaman), "Dur 10 s");
}

#[test]
fn charm_takes_a_creep_of_a_low_enough_level() {
    let mut h = started_s43();
    {
        // V6: the Charm rule refuses only a unit of the `creeps` race (8) above `f2i(DataA)`, with the level capped by Misc
        // `MaxUnitLevel`: give the two test types the race and the Misc value the engine would install.
        use sim::natives::game::{KillXpConfig, PickRecord};
        let mut g = h.game_mut();
        for code in [b"nlow", b"nbig"] {
            g.unit_types_mut().set_pick_record(sim::abilities::code(code), PickRecord { race: 8, ..Default::default() });
        }
        g.set_kill_xp_config(KillXpConfig { max_unit_level: 100, ..Default::default() });
    }
    let (low, big) = (unit(&h, "charm_tgt"), unit(&h, "charm_big"));
    h.start("charm_big_it", &[]).expect("charm big");
    run(&mut h, 5);
    assert!(!accepted(&h), "level 9 is above DataA 5");
    assert_eq!(h.game().unit(big).owner, 1);
    h.start("charm_it", &[]).expect("charm");
    run(&mut h, 20);
    assert!(accepted(&h));
    assert_eq!(h.game().unit(low).owner, 0, "changed owner");
}

#[test]
fn howl_of_terror_weakens_the_enemies_around_the_caster() {
    let mut h = started_s43();
    let tgt = unit(&h, "howl_tgt");
    h.start("howl_it", &[]).expect("howl");
    run(&mut h, 2);
    assert!(accepted(&h));
    assert!(has_buff(&h, tgt, sim::neutral_spells::BUFF_HOWL));
}

#[test]
fn rain_of_fire_is_blizzard_with_the_pit_lord_order() {
    let h = started_s43();
    let rain = unit(&h, "rain");
    assert_eq!(h.game().spell_for_order(rain, 852_238), Some(sim::human_heroes::BLIZZARD));
}
