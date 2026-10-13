//! Behaviour tests of the item abilities added by S41 (`item_abilities.rs`, spec `docs/spec/abilities-items.md`): the
//! cooldown group, the potions of invisibility and invulnerability, the item copies of unit spells, the regeneration
//! items. The level data are pinned values of the tests (the editor's numbers of the 1.26a items).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::cast::SpellLevel;
use sim::item_abilities::*;
use sim::natives::game::{item_flags, ItemType, ObjKey, UnitType};
use sim::ScriptHost;
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type item extends handle

native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateItem takes integer itemid, real x, real y returns item
native SetWidgetLife takes widget wid, real newLife returns nothing
native UnitAddItem takes unit whichUnit, item whichItem returns boolean
native UnitRemoveItem takes unit whichUnit, item whichItem returns nothing
native UnitUseItem takes unit whichUnit, item whichItem returns boolean
native UnitUseItemPoint takes unit whichUnit, item whichItem, real x, real y returns boolean
native GetItemCharges takes item whichItem returns integer
native GetUnitX takes unit whichUnit returns real
"#;

const SRC: &str = r#"
globals
    unit u = null
    unit foe = null
    unit foe2 = null
    unit foe3 = null
    item a = null
    item b = null
    item ring = null
    boolean accepted = false
    real x = 0.0
endglobals
function setup takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set a = CreateItem('cdq1', 0.0, 0.0)
    set b = CreateItem('cdq2', 0.0, 0.0)
    call UnitAddItem(u, a)
    call UnitAddItem(u, b)
endfunction
function use_a takes nothing returns nothing
    set accepted = UnitUseItem(u, a)
endfunction
function use_b takes nothing returns nothing
    set accepted = UnitUseItem(u, b)
endfunction
function setup_pot takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set a = CreateItem('pvul', 0.0, 0.0)
    call UnitAddItem(u, a)
endfunction
function setup_blink takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set a = CreateItem('dagg', 0.0, 0.0)
    call UnitAddItem(u, a)
endfunction
function blink takes nothing returns nothing
    set accepted = UnitUseItemPoint(u, a, 300.0, 0.0)
endfunction
function read_x takes nothing returns nothing
    set x = GetUnitX(u)
endfunction
function setup_duel takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hpk2', 100.0, 0.0, 0.0)
    set a = CreateItem('mdth', 0.0, 0.0)
    set b = CreateItem('brac', 0.0, 0.0)
    call UnitAddItem(u, a)
    call UnitAddItem(foe, b)
    call SetWidgetLife(u, 100.0)
endfunction
function setup_orbs takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hpk2', 100.0, 0.0, 0.0)
    set foe2 = CreateUnit(Player(1), 'hpk2', 160.0, 0.0, 0.0)
    set foe3 = CreateUnit(Player(1), 'hpk2', 900.0, 0.0, 0.0)
    set a = CreateItem('ofir', 0.0, 0.0)
    call UnitAddItem(u, a)
endfunction
function setup_ring takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set ring = CreateItem('rlif', 0.0, 0.0)
endfunction
function pick_ring takes nothing returns nothing
    call UnitAddItem(u, ring)
endfunction
function drop_ring takes nothing returns nothing
    call UnitRemoveItem(u, ring)
endfunction
"#;

const PACK2: u32 = 0x6870_6B32; // 'hpk2'
const CDQ1: u32 = 0x6364_7131; // 'cdq1'
const CDQ2: u32 = 0x6364_7132; // 'cdq2'
const PVUL: u32 = 0x7076_756C; // 'pvul'
const DAGG: u32 = 0x6461_6767; // 'dagg'
const RLIF: u32 = 0x726C_6966; // 'rlif'
const A_Q1: u32 = 0x4149_7131; // 'AIq1'
const A_Q2: u32 = 0x4149_7132; // 'AIq2'
const A_VUL: u32 = 0x4149_7675; // 'AIvu'
const A_BLINK: u32 = 0x4149_626B; // 'AIbk'
const A_REL: u32 = 0x4172_656C; // 'Arel'
const MDTH: u32 = 0x6D64_7468; // 'mdth' Mask of Death
const BRAC: u32 = 0x6272_6163; // 'brac' Runed Bracers
const OFIR: u32 = 0x6F66_6972; // 'ofir' Orb of Fire
const OFRO: u32 = 0x6F66_726F; // 'ofro' Orb of Frost
const A_FIRE: u32 = 0x4149_6662; // 'AIfb'
const A_FROST: u32 = 0x4149_6F62; // 'AIob'
const A_VAMP: u32 = 0x4149_7661; // 'AIva'
const A_SR: u32 = 0x4149_7372; // 'AIsr'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn level(cool: f32, dur: f32, a: f32) -> SpellLevel {
    let mut data = [F32::ZERO; 9];
    data[0] = f(a);
    SpellLevel {
        cool: f(cool),
        dur: f(dur),
        hero_dur: f(dur),
        data,
        rng: f(99999.0),
        ..Default::default()
    }
}

fn item_type(charges: i32, perishable: bool) -> ItemType {
    ItemType {
        class: 0,
        level_ex: 0,
        level_plain: 0,
        flag_48: 0,
        charges,
        powerup: false,
        sellable: true,
        pawnable: true,
        flags: if perishable { item_flags::PERISHABLE } else { 0 },
    }
}

fn host() -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        g.unit_types_mut().insert(
            PACK2,
            UnitType {
                name: "Courier".to_string(),
                max_life: f(500.0),
                max_mana: f(300.0),
                move_speed: f(270.0),
                inventory_size: 2,
                sight: f(1400.0),
                night_sight: f(800.0),
                ..Default::default()
            },
        );
        let mut data = ItemAbilityData::default();
        let mut add = |item: u32, ability: u32, base: u32, lv: SpellLevel, tweak: &dyn Fn(&mut ItemSpec)| {
            let mut spec = ItemSpec {
                abilities: vec![ability],
                usable: true,
                ..Default::default()
            };
            tweak(&mut spec);
            data.specs.insert(item, spec);
            data.bases.insert(ability, base);
            data.levels.insert(ability, vec![lv]);
        };
        // Two healing items of one cooldown group `AIhe`; the second ignores the group's start (`ignoreCD`).
        add(CDQ1, A_Q1, ITEM_HEAL, level(10.0, 0.0, 10.0), &|s| s.cooldown_id = ITEM_HEAL);
        add(CDQ2, A_Q2, ITEM_HEAL, level(10.0, 0.0, 10.0), &|s| {
            s.cooldown_id = ITEM_HEAL;
            s.ignore_cooldown = true;
        });
        add(PVUL, A_VUL, ITEM_INVUL, level(45.0, 15.0, 0.0), &|s| s.perishable = true);
        add(DAGG, A_BLINK, sim::undead_ne::BLINK, level(0.0, 0.0, 1000.0), &|_| {});
        add(RLIF, A_REL, BONUS_LIFE_REGEN, level(0.0, 0.0, 2.0), &|_| {});
        let mut vamp = level(0.0, 0.0, 0.5);
        vamp.data[0] = f(0.5);
        add(MDTH, A_VAMP, BONUS_VAMPIRE, vamp, &|_| {});
        let mut bracers = level(0.0, 0.0, 0.0);
        bracers.data[1] = f(0.33);
        add(BRAC, A_SR, BONUS_SPELL_REDUCTION, bracers, &|_| {});
        let mut fire = level(0.0, 0.0, 5.0);
        fire.area = f(140.0);
        add(OFIR, A_FIRE, ORB_FIRE, fire, &|_| {});
        let mut frost = level(0.0, 3.0, 6.0);
        frost.hero_dur = f(1.0);
        add(OFRO, A_FROST, ORB_FROST, frost, &|_| {});
        g.install_item_abilities(&data);
        g.item_types_mut().insert(OFIR, item_type(0, false));
        g.item_types_mut().insert(OFRO, item_type(0, false));
        g.item_types_mut().insert(MDTH, item_type(0, false));
        g.item_types_mut().insert(BRAC, item_type(0, false));
        g.item_types_mut().insert(CDQ1, item_type(0, false));
        g.item_types_mut().insert(CDQ2, item_type(0, false));
        g.item_types_mut().insert(PVUL, item_type(1, true));
        g.item_types_mut().insert(DAGG, item_type(0, false));
        g.item_types_mut().insert(RLIF, item_type(0, false));
    }
    h
}

fn started(name: &str) -> ScriptHost {
    let mut h = host();
    h.start(name, &[]).expect("setup");
    h.step();
    h
}

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
    for _ in 0..2 {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
}

fn global_unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

#[test]
fn ignore_cd_item_does_not_start_the_group_but_is_blocked_by_it() {
    let mut h = started("setup");
    // `cdq2` has ignoreCD: using it starts no group cooldown.
    call(&mut h, "use_b");
    assert!(accepted(&h));
    call(&mut h, "use_a");
    assert!(accepted(&h), "the group was not started by the ignoreCD item");
    // `cdq1` started the group `AIhe`: `cdq2` (same group) is blocked although its own cooldown ended.
    for _ in 0..20 {
        h.step();
    }
    let u = global_unit(&h, "u");
    assert!(h.game().is_cooling(u, ITEM_HEAL), "the group of cdq1's cast is cooling");
    call(&mut h, "use_b");
    assert!(!accepted(&h), "the group cools: cdq2 cannot be used");
}

#[test]
fn group_cooldown_is_keyed_by_the_item_type_not_the_ability() {
    let mut h = started("setup");
    let u = global_unit(&h, "u");
    call(&mut h, "use_a");
    assert!(accepted(&h));
    assert!(h.game().is_cooling(u, ITEM_HEAL));
    assert!(!h.game().is_cooling(u, 0), "no group 0");
}

#[test]
fn invulnerability_potion_makes_the_holder_invulnerable_for_its_duration() {
    let mut h = started("setup_pot");
    let u = global_unit(&h, "u");
    assert!(!h.game().unit_is_invulnerable(u));
    call(&mut h, "use_a");
    assert!(accepted(&h));
    assert!(h.game().unit_is_invulnerable(u), "Bvul: invulnerable");
    assert!(h.game().unit(u).buffs.iter().any(|b| b.base == BUFF_INVUL));
    // 15 s of game time: 150 steps of 0.1 s... the steps of the host are 0.1 s batches.
    for _ in 0..170 {
        h.step();
    }
    assert!(!h.game().unit_is_invulnerable(u), "the buff ended");
}

#[test]
fn dagger_of_escape_blinks_through_the_borrowed_spell_class() {
    let mut h = started("setup_blink");
    call(&mut h, "blink");
    assert!(accepted(&h), "the item copy of Blink is castable");
    for _ in 0..10 {
        h.step();
    }
    call(&mut h, "read_x");
    let x = h.vm().global("x").and_then(|v| v.as_real()).expect("real");
    assert_eq!(x.to_f32(), 300.0, "the caster blinked to the point");
}

#[test]
fn life_regeneration_ring_adds_its_rate_while_carried() {
    let mut h = started("setup_ring");
    let u = global_unit(&h, "u");
    assert_eq!(h.game().unit(u).bonuses.life_regen.to_f32(), 0.0);
    call(&mut h, "pick_ring");
    // Without the softfloat tables the unit has no agile life property: the aura bonus is the fallback.
    assert_eq!(h.game().unit(u).bonuses.life_regen.to_f32(), 2.0);
    call(&mut h, "drop_ring");
    assert_eq!(h.game().unit(u).bonuses.life_regen.to_f32(), 0.0, "taken off with the item");
}

#[test]
fn mask_of_death_heals_half_of_the_damage_dealt() {
    let h = started("setup_duel");
    let (u, foe) = (global_unit(&h, "u"), global_unit(&h, "foe"));
    {
        let mut g = h.game_mut();
        assert_eq!(g.unit(u).life.to_f32(), 100.0);
        on_hit(&mut g, u, foe, f(60.0), true);
        assert_eq!(g.unit(u).life.to_f32(), 130.0, "DataA 0.5 of 60 damage");
        // An ally is not a target of `air,ground,enemy`.
        on_hit(&mut g, u, u, f(60.0), true);
        assert_eq!(g.unit(u).life.to_f32(), 130.0);
    }
}

#[test]
fn runed_bracers_scale_spells_damage_only() {
    let h = started("setup_duel");
    let (u, foe) = (global_unit(&h, "u"), global_unit(&h, "foe"));
    let mut g = h.game_mut();
    let before = g.unit(foe).life.to_f32();
    // Attack type argument 0 is the Spells row: scaled by 1 - 0.33.
    g.unit_damage_target(u, foe, f(100.0), false, false, 0, 5);
    let lost = before - g.unit(foe).life.to_f32();
    assert!((lost - 67.0).abs() < 0.01, "spells damage reduced to 67, lost {lost}");
    let before = g.unit(foe).life.to_f32();
    // A Normal-row record (attack type 1) is untouched by the bracers.
    g.unit_damage_target(u, foe, f(100.0), false, false, 1, 5);
    let lost = before - g.unit(foe).life.to_f32();
    assert!(lost > 67.5, "a Normal record is not reduced, lost {lost}");
}

#[test]
fn orb_of_fire_hits_the_victim_and_the_enemies_around_it() {
    let h = started("setup_orbs");
    let (u, foe, foe2, foe3) = (global_unit(&h, "u"), global_unit(&h, "foe"), global_unit(&h, "foe2"), global_unit(&h, "foe3"));
    let mut g = h.game_mut();
    let before: Vec<f32> = [foe, foe2, foe3].iter().map(|&k| g.unit(k).life.to_f32()).collect();
    let died = on_hit(&mut g, u, foe, f(30.0), true);
    assert!(died.is_empty());
    let after: Vec<f32> = [foe, foe2, foe3].iter().map(|&k| g.unit(k).life.to_f32()).collect();
    assert_eq!(before[0] - after[0], 5.0, "DataA to the victim");
    assert_eq!(before[1] - after[1], 5.0, "an enemy inside Area 140 of the victim");
    assert_eq!(before[2], after[2], "an enemy outside the area");
}

#[test]
fn orb_of_frost_slows_the_victim_for_its_duration() {
    let mut h = started("setup_orbs");
    let (u, foe) = (global_unit(&h, "u"), global_unit(&h, "foe"));
    {
        // Swap the carried orb: the pack holds the fire orb; give the unit the frost ability directly.
        let mut g = h.game_mut();
        g.unit_mut(u).abilities.attach(sim::abilities::Ability::new(A_FROST, None));
        let died = on_hit(&mut g, u, foe, f(30.0), true);
        assert!(died.is_empty());
        assert!(g.unit(foe).buffs.iter().any(|b| b.base == sim::unit_spells::BUFF_FROST), "Bfro on the victim");
    }
    for _ in 0..50 {
        h.step();
    }
    let g = h.game();
    assert!(!g.unit(foe).buffs.iter().any(|b| b.base == sim::unit_spells::BUFF_FROST), "the 3 s buff ended");
}
