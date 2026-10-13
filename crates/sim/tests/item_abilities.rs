//! Behaviour tests of the item abilities (`item_abilities.rs`, spec `docs/spec/abilities-items.md`): the use path with
//! its charge, removal and cooldown rules, the restoration items, the tomes (powerups) and the persistent bonuses.
//! The level data are pinned values of the tests (the editor's numbers of the 1.26a items). Tests that need the
//! softfloat tables (division, the experience engine) run only when `GAME_DLL` is set.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::item_abilities::*;
use sim::natives::game::{item_flags, ItemType, ObjKey, UnitType};
use sim::cast::SpellLevel;
use sim::heroes::{HeroAttributes, XpTables};
use sim::ScriptHost;
use softfloat::{Tables, F32};

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type item extends handle

native I2R takes integer i returns real
native Player takes integer i returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateItem takes integer itemid, real x, real y returns item
native SetWidgetLife takes widget wid, real newLife returns nothing
native SetUnitState takes unit whichUnit, integer whichState, real newVal returns nothing
native UnitAddItem takes unit whichUnit, item whichItem returns boolean
native UnitRemoveItem takes unit whichUnit, item whichItem returns nothing
native UnitUseItem takes unit whichUnit, item whichItem returns boolean
native UnitUseItemTarget takes unit whichUnit, item whichItem, widget target returns boolean
native GetItemCharges takes item whichItem returns integer
native GetItemTypeId takes item whichItem returns integer
native GetHeroStr takes unit whichHero, boolean includeBonuses returns integer
native GetHeroAgi takes unit whichHero, boolean includeBonuses returns integer
native GetHeroInt takes unit whichHero, boolean includeBonuses returns integer
native GetHeroLevel takes unit whichHero returns integer
native GetHeroXP takes unit whichHero returns integer
native SetHeroXP takes unit whichHero, integer xp, boolean showEyeCandy returns nothing
native GetPlayerState takes player whichPlayer, integer whichPlayerState returns integer
"#;

const SRC: &str = r#"
globals
    unit u = null
    unit friend = null
    unit friend2 = null
    unit foe = null
    unit far = null
    item potion = null
    item potion2 = null
    item claw = null
    item boots = null
    item tome = null
    boolean accepted = false
    integer charges = -9
    integer typeid = -9
    integer gold = -9
endglobals
function setup takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    call SetWidgetLife(u, 100.0)
    call SetUnitState(u, 2, 50.0)
    set potion = CreateItem('ph01', 0.0, 0.0)
    call UnitAddItem(u, potion)
endfunction
function setup_aoe takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    set friend2 = CreateUnit(Player(0), 'hfoo', 0.0, 100.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 0.0, -100.0, 0.0)
    set far = CreateUnit(Player(0), 'hfoo', 2000.0, 0.0, 0.0)
    call SetWidgetLife(u, 100.0)
    call SetWidgetLife(friend, 100.0)
    call SetWidgetLife(friend2, 100.0)
    call SetWidgetLife(foe, 100.0)
    call SetWidgetLife(far, 100.0)
    set potion = CreateItem('ps01', 0.0, 0.0)
    call UnitAddItem(u, potion)
endfunction
function setup_speed takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 0.0, -100.0, 0.0)
    set potion = CreateItem('psp1', 0.0, 0.0)
    call UnitAddItem(u, potion)
endfunction
function setup_regen takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    call SetWidgetLife(u, 100.0)
    call SetUnitState(u, 2, 50.0)
    set potion = CreateItem('prj1', 0.0, 0.0)
    call UnitAddItem(u, potion)
endfunction
function setup_portal takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set friend = CreateUnit(Player(0), 'hfoo', 100.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hfoo', 0.0, -100.0, 0.0)
    set far = CreateUnit(Player(0), 'htow', 4000.0, 0.0, 0.0)
    set potion = CreateItem('stwp', 0.0, 0.0)
    call UnitAddItem(u, potion)
endfunction
function use_potion_on_far takes nothing returns nothing
    set accepted = UnitUseItemTarget(u, potion, far)
endfunction
function setup_bonus takes nothing returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set claw = CreateItem('cl01', 0.0, 0.0)
    set boots = CreateItem('bt01', 0.0, 0.0)
endfunction
function pick_claw takes nothing returns nothing
    call UnitAddItem(u, claw)
endfunction
function drop_claw takes nothing returns nothing
    call UnitRemoveItem(u, claw)
endfunction
function pick_boots takes nothing returns nothing
    call UnitAddItem(u, boots)
endfunction
function drop_boots takes nothing returns nothing
    call UnitRemoveItem(u, boots)
endfunction
function setup_hero takes nothing returns nothing
    set u = CreateUnit(Player(0), 'Hpal', 0.0, 0.0, 0.0)
    set tome = CreateItem('tm01', 0.0, 0.0)
endfunction
function take_tome takes nothing returns nothing
    set accepted = UnitAddItem(u, tome)
endfunction
function use_potion takes nothing returns nothing
    set accepted = UnitUseItem(u, potion)
endfunction
function use_potion2 takes nothing returns nothing
    set accepted = UnitUseItem(u, potion2)
endfunction
function add_potion2 takes nothing returns nothing
    set potion2 = CreateItem('ph02', 0.0, 0.0)
    call UnitAddItem(u, potion2)
endfunction
function read_potion takes nothing returns nothing
    set charges = GetItemCharges(potion)
    set typeid = GetItemTypeId(potion)
endfunction
function read_tome takes nothing returns nothing
    set typeid = GetItemTypeId(tome)
endfunction
"#;

const PACK2: u32 = 0x6870_6B32; // 'hpk2'
const FOOT: u32 = 0x6866_6F6F; // 'hfoo'
const HERO: u32 = 0x4870_616C; // 'Hpal'
const POTION: u32 = 0x7068_3031; // 'ph01' healing, 3 charges, perishable
const POTION2: u32 = 0x7068_3032; // 'ph02' shares the cooldown group of ph01
const MANA: u32 = 0x706D_3031; // 'pm01'
const RESTORE: u32 = 0x7072_3031; // 'pr01'
const SCROLL: u32 = 0x7073_3031; // 'ps01' healing area scroll
const SPEED_POTION: u32 = 0x7073_7031; // 'psp1'
const PORTAL: u32 = 0x7374_7770; // 'stwp'
const REJUV: u32 = 0x7072_6A31; // 'prj1' potion of rejuvenation: 100 life and 25 mana over 30 s
const TOWN: u32 = 0x6874_6F77; // 'htow'
const CLAW: u32 = 0x636C_3031; // 'cl01' +15 damage
const BOOTS: u32 = 0x6274_3031; // 'bt01' +10 armor
const TOME: u32 = 0x746D_3031; // 'tm01' attribute tome powerup
const A_HEAL: u32 = 0x4149_6831; // 'AIh1'
const A_HEAL2: u32 = 0x4149_6832; // 'AIh2'
const A_MANA: u32 = 0x4149_6D31; // 'AIm1'
const A_REST: u32 = 0x4149_7231; // 'AIr1'
const A_SCROLL: u32 = 0x4149_6832 + 0x100; // 'AIh3'
const A_SPEED: u32 = 0x4149_7370; // 'AIsp'
const A_PORTAL: u32 = 0x4149_7470; // 'AItp'
const A_REJUV: u32 = 0x4149_7031; // 'AIp1'
const A_CLAW: u32 = 0x4149_6133; // 'AIa3'
const A_BOOTS: u32 = 0x4149_6434; // 'AId4'
const A_TOME: u32 = 0x4149_7331; // 'AIs1'

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn tables() -> Option<Tables> {
    let path = std::env::var("GAME_DLL").ok()?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
    Some(Tables::from_game_dll(&bytes).expect("tables are in the image"))
}

fn level(cost: i32, cool: f32, area: f32, a: f32, b: f32, c: f32) -> SpellLevel {
    let mut data = [F32::ZERO; 9];
    data[0] = f(a);
    data[1] = f(b);
    data[2] = f(c);
    SpellLevel {
        cost,
        cool: f(cool),
        area: f(area),
        data,
        ..Default::default()
    }
}

fn item_type(charges: i32, perishable: bool, powerup: bool) -> ItemType {
    let mut flags = 0;
    if perishable {
        flags |= item_flags::PERISHABLE;
    }
    if powerup {
        flags |= item_flags::POWERUP;
    }
    ItemType {
        class: 0,
        level_ex: 0,
        level_plain: 0,
        flag_48: 0,
        charges,
        powerup,
        sellable: true,
        pawnable: true,
        flags,
    }
}

fn host(with_tables: bool) -> ScriptHost {
    let full = format!("{PRELUDE}\n{SRC}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let h = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = h.game_mut();
        for (id, name, life, inv) in [(PACK2, "Courier", 500.0, 2u32), (FOOT, "Footman", 500.0, 0), (HERO, "Paladin", 600.0, 2), (TOWN, "Town Hall", 1500.0, 0)] {
            g.unit_types_mut().insert(
                id,
                UnitType {
                    name: name.to_string(),
                    max_life: f(life),
                    max_mana: f(300.0),
                    move_speed: f(270.0),
                    inventory_size: inv,
                    sight: f(1400.0),
                    night_sight: f(800.0),
                    ..Default::default()
                },
            );
        }
        g.set_hero_xp(XpTables::from_need_list(&[200, 400, 600]));
        g.set_hero_attributes(
            HERO,
            HeroAttributes {
                str_base: 20,
                str_plus: f(2.5),
                int_base: 12,
                int_plus: f(1.5),
                agi_base: 10,
                agi_plus: f(1.0),
            },
        );
        if with_tables {
            g.set_tables(tables());
        }
        let mut specs = ItemAbilityData::default();
        let mut add = |item: u32, abilities: &[u32], base: u32, lv: SpellLevel, spec_tweak: &dyn Fn(&mut ItemSpec)| {
            let mut spec = ItemSpec {
                abilities: abilities.to_vec(),
                usable: true,
                ..Default::default()
            };
            spec_tweak(&mut spec);
            specs.specs.insert(item, spec);
            for &a in abilities {
                specs.bases.insert(a, base);
                specs.levels.insert(a, vec![lv]);
            }
        };
        add(POTION, &[A_HEAL], ITEM_HEAL, level(0, 5.0, 0.0, 150.0, 0.0, 0.0), &|s| s.perishable = true);
        add(POTION2, &[A_HEAL2], ITEM_HEAL, level(0, 5.0, 0.0, 50.0, 0.0, 0.0), &|s| {
            s.perishable = true;
            s.cooldown_id = A_HEAL;
        });
        add(MANA, &[A_MANA], ITEM_MANA, level(0, 5.0, 0.0, 100.0, 0.0, 0.0), &|s| s.perishable = true);
        add(RESTORE, &[A_REST], ITEM_RESTORE, level(0, 5.0, 0.0, 70.0, 40.0, 0.0), &|s| s.perishable = true);
        add(SCROLL, &[A_SCROLL], ITEM_HEAL_AOE, level(0, 5.0, 500.0, 200.0, 0.0, 0.0), &|s| s.perishable = true);
        let mut speed_level = level(0, 0.0, 0.0, 0.6, 0.3, 0.0);
        speed_level.dur = f(45.0);
        speed_level.hero_dur = f(45.0);
        add(SPEED_POTION, &[A_SPEED], ITEM_SPEED, speed_level, &|s| s.perishable = true);
        let mut rejuv_level = level(0, 0.0, 0.0, 100.0, 25.0, 0.0);
        rejuv_level.dur = f(30.0);
        rejuv_level.hero_dur = f(30.0);
        add(REJUV, &[A_REJUV], ITEM_REGEN, rejuv_level, &|s| s.perishable = true);
        add(PORTAL, &[A_PORTAL], TOWN_PORTAL, level(0, 0.0, 1100.0, 90.0, 1.0, 0.0), &|s| s.perishable = true);
        add(CLAW, &[A_CLAW], BONUS_ATTACK, level(0, 0.0, 0.0, 15.0, 0.0, 0.0), &|_| {});
        add(BOOTS, &[A_BOOTS], BONUS_DEFENSE, level(0, 0.0, 0.0, 10.0, 0.0, 0.0), &|_| {});
        add(TOME, &[A_TOME], TOME_ATTRS, level(0, 0.0, 0.0, 1.0, 2.0, 3.0), &|_| {});
        g.install_item_abilities(&specs);
        g.item_types_mut().insert(POTION, item_type(3, true, false));
        g.item_types_mut().insert(POTION2, item_type(1, true, false));
        g.item_types_mut().insert(MANA, item_type(2, true, false));
        g.item_types_mut().insert(RESTORE, item_type(1, true, false));
        g.item_types_mut().insert(SCROLL, item_type(1, true, false));
        g.item_types_mut().insert(SPEED_POTION, item_type(1, true, false));
        g.item_types_mut().insert(PORTAL, item_type(1, true, false));
        g.item_types_mut().insert(REJUV, item_type(1, true, false));
        g.item_types_mut().insert(CLAW, item_type(0, false, false));
        g.item_types_mut().insert(BOOTS, item_type(0, false, false));
        g.item_types_mut().insert(TOME, item_type(0, false, true));
    }
    h
}

fn started(name: &str, with_tables: bool) -> ScriptHost {
    let mut h = host(with_tables);
    h.start(name, &[]).expect("setup");
    h.step();
    h
}

fn global_unit(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn accepted(h: &ScriptHost) -> bool {
    h.vm().global("accepted").and_then(|v| v.as_boolean()).expect("boolean global")
}

fn int_global(h: &ScriptHost, name: &str) -> i32 {
    h.vm().global(name).and_then(|v| v.as_integer()).expect("integer global")
}

fn call(h: &mut ScriptHost, name: &str) {
    h.start(name, &[]).expect("start");
    for _ in 0..2 {
        h.step();
    }
    assert!(h.misfires().is_empty(), "no thread was refused");
}

fn run(h: &mut ScriptHost, steps: usize) {
    for _ in 0..steps {
        h.step();
    }
}

fn life(h: &ScriptHost, k: ObjKey) -> f32 {
    h.game().unit(k).life.to_f32()
}


#[test]
fn heal_potion_heals_uses_a_charge_and_cools_down() {
    let mut h = started("setup", false);
    let u = global_unit(&h, "u");
    assert_eq!(life(&h, u), 100.0);
    call(&mut h, "use_potion");
    assert!(accepted(&h));
    assert_eq!(life(&h, u), 250.0, "DataA 150 added");
    call(&mut h, "read_potion");
    assert_eq!(int_global(&h, "charges"), 2, "one charge used");
    // The cooldown (5 s) refuses a second use.
    call(&mut h, "use_potion");
    assert!(!accepted(&h), "the potion is cooling down");
    assert_eq!(life(&h, u), 250.0);
    run(&mut h, 60);
    call(&mut h, "use_potion");
    assert!(accepted(&h));
    assert_eq!(life(&h, u), 400.0);
    call(&mut h, "read_potion");
    assert_eq!(int_global(&h, "charges"), 1);
}

#[test]
fn perishable_item_is_destroyed_with_its_last_charge() {
    let mut h = started("setup", false);
    for _ in 0..3 {
        call(&mut h, "use_potion");
        assert!(accepted(&h));
        run(&mut h, 60);
    }
    call(&mut h, "read_potion");
    assert_eq!(int_global(&h, "typeid"), 0, "the used-up potion is gone");
    let u = global_unit(&h, "u");
    assert_eq!(life(&h, u), 500.0, "3 x 150 added to 100, clamped to the maximum 500");
    assert!(!h.game().unit(u).abilities.contains(A_HEAL), "its ability left the unit with it");
}

#[test]
fn heal_is_clamped_to_the_maximum_life() {
    let mut h = started("setup", false);
    let u = global_unit(&h, "u");
    h.game_mut().unit_mut(u).life = f(450.0);
    call(&mut h, "use_potion");
    assert_eq!(life(&h, u), 500.0);
}

#[test]
fn items_of_one_cooldown_group_share_the_cooldown() {
    let mut h = started("setup", false);
    let u = global_unit(&h, "u");
    call(&mut h, "add_potion2");
    call(&mut h, "use_potion");
    assert!(accepted(&h));
    assert_eq!(life(&h, u), 250.0);
    // ph02 names AIh1 as its cooldown id: it cannot be used while ph01 cools.
    call(&mut h, "use_potion2");
    assert!(!accepted(&h), "shared cooldown group");
    run(&mut h, 60);
    call(&mut h, "use_potion2");
    assert!(accepted(&h));
    assert_eq!(life(&h, u), 300.0);
}

#[test]
fn item_without_charges_flag_keeps_its_count() {
    // `charge_step` / `destroys` are the charge routine (CONFIRMED structure, Unicorn vectors in item_ability_vectors).
    assert_eq!(charge_step(item_flags::NO_CHARGES, 0), (item_flags::NO_CHARGES, Some(0)));
    assert_eq!(charge_step(0, 3), (0, Some(2)));
    assert_eq!(charge_step(0, 0).1, None);
    assert_eq!(charge_step(0, -2).1, None);
    assert!(destroys(item_flags::PERISHABLE, 0));
    assert!(!destroys(item_flags::PERISHABLE | item_flags::NO_CHARGES, 0));
    assert!(!destroys(0, 0));
    assert!(!destroys(item_flags::PERISHABLE, 1));
}

#[test]
fn area_heal_scroll_heals_friends_in_the_circle() {
    let mut h = started("setup_aoe", false);
    let (u, friend, friend2, foe, far) = (
        global_unit(&h, "u"),
        global_unit(&h, "friend"),
        global_unit(&h, "friend2"),
        global_unit(&h, "foe"),
        global_unit(&h, "far"),
    );
    call(&mut h, "use_potion");
    assert!(accepted(&h));
    assert_eq!(life(&h, u), 300.0, "the user is healed too");
    assert_eq!(life(&h, friend), 300.0);
    assert_eq!(life(&h, friend2), 300.0);
    assert_eq!(life(&h, foe), 100.0, "an enemy is not healed");
    assert_eq!(life(&h, far), 100.0, "outside the 500 area");
}

#[test]
fn attack_bonus_follows_the_pack() {
    let mut h = started("setup_bonus", false);
    let u = global_unit(&h, "u");
    assert_eq!(h.game().buff_damage_bonus(u), 0);
    call(&mut h, "pick_claw");
    assert_eq!(h.game().buff_damage_bonus(u), 15);
    assert!(h.game().unit(u).abilities.contains(A_CLAW));
    call(&mut h, "drop_claw");
    assert_eq!(h.game().buff_damage_bonus(u), 0);
    assert!(!h.game().unit(u).abilities.contains(A_CLAW));
}

#[test]
fn defense_bonus_adds_armor_while_carried() {
    let mut h = started("setup_bonus", false);
    let u = global_unit(&h, "u");
    call(&mut h, "pick_boots");
    assert_eq!(h.game().unit(u).bonuses.armor.to_f32(), 10.0);
    call(&mut h, "drop_boots");
    assert_eq!(h.game().unit(u).bonuses.armor.to_f32(), 0.0);
}

#[test]
fn attribute_tome_powerup_adds_attributes_to_a_hero() {
    let mut h = started("setup_hero", false);
    let u = global_unit(&h, "u");
    let before = {
        let hero = h.game().unit(u).hero.clone().expect("hero");
        (hero.agi_base, hero.int_base, hero.str_base)
    };
    call(&mut h, "take_tome");
    assert!(accepted(&h));
    let after = {
        let hero = h.game().unit(u).hero.clone().expect("hero");
        (hero.agi_base, hero.int_base, hero.str_base)
    };
    // DataA agility, DataB intelligence, DataC strength (the order of the original's three adders).
    assert_eq!((after.0 - before.0, after.1 - before.1, after.2 - before.2), (1, 2, 3));
    call(&mut h, "read_tome");
    assert_eq!(int_global(&h, "typeid"), 0, "the powerup is consumed");
}

#[test]
fn tome_of_experience_levels_a_hero() {
    let Some(_) = tables() else {
        return;
    };
    // Experience tome: DataA 200 at level 1 (editor value of `AIem`).
    let mut h = host(true);
    {
        let mut g = h.game_mut();
        let mut data = ItemAbilityData::default();
        data.specs.insert(
            TOME,
            ItemSpec {
                abilities: vec![A_TOME],
                usable: true,
                ..Default::default()
            },
        );
        data.bases.insert(A_TOME, TOME_XP);
        data.levels.insert(A_TOME, vec![level(0, 0.0, 0.0, 200.0, 0.0, 0.0)]);
        g.install_item_abilities(&data);
    }
    h.start("setup_hero", &[]).expect("setup");
    h.step();
    call(&mut h, "take_tome");
    assert!(accepted(&h));
    let u = global_unit(&h, "u");
    let hero = h.game().unit(u).hero.clone().expect("hero");
    assert_eq!(hero.xp, 200);
    assert_eq!(hero.level, 2);
}

#[test]
fn speed_potion_gives_the_speed_buff_for_its_duration() {
    let mut h = started("setup_speed", false);
    let u = global_unit(&h, "u");
    assert_eq!(h.game().unit(u).bonuses.move_mult.to_f32(), 1.0);
    call(&mut h, "use_potion");
    assert!(accepted(&h));
    // Movement +DataA (0.6), attack speed +DataB (0.3): the buff `Bspe` for Dur 45 s (the user is not a hero).
    assert!((h.game().unit(u).bonuses.move_mult.to_f32() - 1.6).abs() < 1e-5);
    assert!((h.game().unit(u).bonuses.attack_mult.to_f32() - 1.3).abs() < 1e-5);
    assert!(h.game().buffs(u).iter().any(|b| b.base == BUFF_SPEED));
    run(&mut h, 440);
    assert!(h.game().buffs(u).iter().any(|b| b.base == BUFF_SPEED), "still running at 44 s");
    run(&mut h, 20);
    assert!(!h.game().buffs(u).iter().any(|b| b.base == BUFF_SPEED), "ended at 45 s");
    assert_eq!(h.game().unit(u).bonuses.move_mult.to_f32(), 1.0);
}

#[test]
fn town_portal_moves_the_caster_and_nearby_allies_to_the_structure() {
    let mut h = started("setup_portal", false);
    let (u, friend, foe, town) = (
        global_unit(&h, "u"),
        global_unit(&h, "friend"),
        global_unit(&h, "foe"),
        global_unit(&h, "far"),
    );
    h.game_mut().unit_mut(town).x = f(4000.0);
    call(&mut h, "use_potion_on_far");
    assert!(accepted(&h));
    // Cast time 0 in this setup: the effect is immediate. The caster stands at the structure, the ally keeps its
    // offset (clustering), the enemy stays.
    let ux = h.game().unit(u).x.to_f32();
    let fx = h.game().unit(friend).x.to_f32();
    assert!((ux - 4000.0).abs() < 1.0, "caster at {ux}");
    assert!((fx - 4100.0).abs() < 1.0, "ally at {fx}");
    assert_eq!(h.game().unit(foe).x.to_f32(), 0.0);
}

#[test]
fn regeneration_potion_restores_life_and_mana_over_its_duration() {
    let mut h = started("setup_regen", false);
    let u = global_unit(&h, "u");
    call(&mut h, "use_potion");
    assert!(accepted(&h));
    assert!(h.game().buffs(u).iter().any(|b| b.base == BUFF_REGEN));
    run(&mut h, 105);
    // After about 10 ticks: 10 x (100 / 30) life and 10 x (25 / 30) mana.
    let l = life(&h, u);
    assert!((l - (100.0 + 10.0 * 100.0 / 30.0)).abs() < 1.0, "life {l}");
    let m = h.game().unit(u).mana.to_f32();
    assert!((m - (50.0 + 10.0 * 25.0 / 30.0)).abs() < 1.0, "mana {m}");
    run(&mut h, 400);
    // All 30 ticks done: the totals are in, the buff has ended.
    assert!((life(&h, u) - 200.0).abs() < 1.0);
    assert!((h.game().unit(u).mana.to_f32() - 75.0).abs() < 1.0);
    assert!(!h.game().buffs(u).iter().any(|b| b.base == BUFF_REGEN));
}
