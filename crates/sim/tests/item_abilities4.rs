//! Behaviour tests of the item abilities of S53 (`item_abilities.rs`, spec `docs/spec/abilities-items.md` section 17): the
//! Moonstone, the Potion of Vampirism, the passive defense, the rewired orbs, the Lightning Purge and the non-stacking
//! move speed bonus. The numeric cores are pinned against the original in `item_ability_vectors3.rs`; these tests pin
//! the wiring (data from the 1.26a `AbilityData.slk` rows).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::code as code_of;
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
native UnitUseItemTarget takes unit whichUnit, item whichItem, widget target returns boolean
"#;

const SRC: &str = r#"
globals
    unit u = null
    unit foe = null
    unit foe2 = null
    item a = null
    item b = null
    boolean accepted = false
endglobals
function setup takes integer itemid returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hpk2', 100.0, 0.0, 0.0)
    set foe2 = CreateUnit(Player(1), 'hpk2', 900.0, 0.0, 0.0)
    set a = CreateItem(itemid, 0.0, 0.0)
    call UnitAddItem(u, a)
endfunction
function add_second takes integer itemid returns nothing
    set b = CreateItem(itemid, 0.0, 0.0)
    call UnitAddItem(u, b)
endfunction
function drop_b takes nothing returns nothing
    call UnitRemoveItem(u, b)
endfunction
function use takes nothing returns nothing
    set accepted = UnitUseItem(u, a)
endfunction
function use_point takes nothing returns nothing
    set accepted = UnitUseItemPoint(u, a, 200.0, 0.0)
endfunction
function use_foe takes nothing returns nothing
    set accepted = UnitUseItemTarget(u, a, foe)
endfunction
function drop takes nothing returns nothing
    call UnitRemoveItem(u, a)
endfunction
function kill_foe takes nothing returns nothing
    call SetWidgetLife(foe, 0.0)
endfunction
"#;

const PACK2: u32 = 0x6870_6B32; // 'hpk2'
const FIG_UNIT: u32 = 0x6E73_6365; // 'nsce'
const MINE: u32 = 0x6E67_6C6D; // 'nglm'

fn c(s: &[u8; 4]) -> u32 {
    code_of(s)
}

fn f(v: f32) -> F32 {
    F32::from_bits(v.to_bits())
}

fn lvl(dur: f32, area: f32, a: f32) -> SpellLevel {
    let mut data = [F32::ZERO; 9];
    data[0] = f(a);
    SpellLevel {
        dur: f(dur),
        hero_dur: f(dur),
        area: f(area),
        data,
        rng: f(99999.0),
        ..Default::default()
    }
}

fn ty() -> ItemType {
    ItemType {
        class: 0,
        level_ex: 0,
        level_plain: 0,
        flag_48: 0,
        charges: 0,
        powerup: false,
        sellable: true,
        pawnable: true,
        flags: item_flags::PERISHABLE,
    }
}

/// (item type, ability id, base, level, extra setup)
struct Row {
    item: u32,
    alias: u32,
    base: u32,
    level: SpellLevel,
}

fn host(rows: &[Row], tweak: &dyn Fn(&mut ItemAbilityData)) -> ScriptHost {
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
                inventory_size: 4,
                movement: 1,
                sight: f(1400.0),
                night_sight: f(800.0),
                ..Default::default()
            },
        );
        for u in [FIG_UNIT, MINE] {
            g.unit_types_mut().insert(
                u,
                UnitType {
                    name: "Spawn".to_string(),
                    max_life: f(100.0),
                    move_speed: f(270.0),
                    ..Default::default()
                },
            );
        }
        let mut data = ItemAbilityData::default();
        for r in rows {
            data.specs.insert(
                r.item,
                ItemSpec {
                    abilities: vec![r.alias],
                    usable: true,
                    ..Default::default()
                },
            );
            data.bases.insert(r.alias, r.base);
            data.levels.insert(r.alias, vec![r.level]);
        }
        tweak(&mut data);
        g.install_item_abilities(&data);
        for r in rows {
            g.item_types_mut().insert(r.item, ty());
        }
    }
    h
}

fn started(rows: &[Row], item: u32, tweak: &dyn Fn(&mut ItemAbilityData)) -> ScriptHost {
    let mut h = host(rows, tweak);
    h.start("setup", &[jass::vm::Value::Integer(item as i32)]).expect("setup");
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

fn gu(h: &ScriptHost, name: &str) -> ObjKey {
    let id = h.vm().global(name).and_then(|v| v.as_handle()).expect("handle global");
    h.game().unit_of_handle(id).expect("live unit")
}

fn row(item: &[u8; 4], alias: &[u8; 4], base: &[u8; 4], level: SpellLevel) -> Row {
    Row {
        item: c(item),
        alias: c(alias),
        base: c(base),
        level,
    }
}

fn data_level(dur: f32, hero_dur: f32, area: f32, data: &[(usize, f32)]) -> SpellLevel {
    let mut l = lvl(dur, area, 0.0);
    l.hero_dur = f(hero_dur);
    for &(i, v) in data {
        l.data[i] = f(v);
    }
    l
}

fn add_second(h: &mut ScriptHost, item: u32) {
    h.start("add_second", &[jass::vm::Value::Integer(item as i32)]).expect("start");
    for _ in 0..2 {
        h.step();
    }
}

#[test]
fn s53_codes_have_a_class() {
    for code in [b"AIct", b"AIpv", b"AIdd"] {
        assert!(Family::of_base(c(code)).is_some(), "{}", String::from_utf8_lossy(code));
    }
    assert!(Family::of_base(c(b"AIct")).unwrap().castable());
    assert!(Family::of_base(c(b"AIpv")).unwrap().castable());
    assert!(!Family::of_base(c(b"AIdd")).unwrap().castable(), "the passive defense is a passive");
    // The Lightning Purge runs the Purge effect (its slot 0x424 is a jump to it).
    assert!(Family::of_base(c(b"AIlp")).is_none());
    assert_eq!(borrowed_class(c(b"AIlp")), Some(sim::unit_spells::PURGE));
}

#[test]
fn moonstone_sets_the_time_of_day_and_restores_it() {
    // Stock row: Dur 30, DataA / DataB absent (0): the time of day goes to 0.0 (midnight) for 30 seconds.
    let rows = [row(b"moon", b"AIct", b"AIct", data_level(30.0, 30.0, 0.0, &[]))];
    let mut h = started(&rows, c(b"moon"), &|_| {});
    h.game_mut().set_time_of_day(f(15.0));
    call(&mut h, "use");
    assert!(accepted(&h));
    assert_eq!(h.game().time_of_day().to_f32(), 0.0, "the time of day was set at once");
    for _ in 0..150 {
        h.step();
    }
    assert_eq!(h.game().time_of_day().to_f32(), 0.0, "still night after 15 s");
    for _ in 0..200 {
        h.step();
    }
    assert_eq!(h.game().time_of_day().to_f32(), 15.0, "restored after 30 s");
    assert!(h.misfires().is_empty());
}

#[test]
fn moonstone_uses_hours_and_minutes() {
    let rows = [row(b"moon", b"AIct", b"AIct", data_level(10.0, 10.0, 0.0, &[(0, 3.0), (1, 30.0)]))];
    let mut h = started(&rows, c(b"moon"), &|_| {});
    call(&mut h, "use");
    assert!(accepted(&h));
    assert_eq!(h.game().time_of_day().to_f32(), 3.5, "DataA + DataB / 60");
}

#[test]
fn vampire_potion_adds_damage_and_steals_life_until_it_ends() {
    // Stock row `vamp`: Dur 20, HeroDur 20, DataA 12, DataB 0.75.
    let rows = [row(b"vamp", b"AIpv", b"AIpv", data_level(20.0, 20.0, 0.0, &[(0, 12.0), (1, 0.75)]))];
    let mut h = started(&rows, c(b"vamp"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    call(&mut h, "use");
    assert!(accepted(&h));
    assert!(h.game().unit(u).buffs.iter().any(|b| b.base == BUFF_VAMPIRE_POTION));
    assert_eq!(h.game().buff_damage_bonus(u), 12);
    h.game_mut().unit_mut(u).life = f(100.0);
    on_hit(&mut h.game_mut(), u, foe, f(40.0), true);
    assert_eq!(h.game().unit(u).life.to_f32(), 130.0, "75% of 40 damage");
    for _ in 0..220 {
        h.step();
    }
    assert!(h.game().unit(u).buffs.iter().all(|b| b.base != BUFF_VAMPIRE_POTION));
    assert_eq!(h.game().buff_damage_bonus(u), 0, "the bonus damage came off with the buff");
    let before = h.game().unit(u).life.to_f32();
    on_hit(&mut h.game_mut(), u, foe, f(40.0), true);
    assert_eq!(h.game().unit(u).life.to_f32(), before, "no life steal any more");
}

#[test]
fn arcanite_shield_scales_pierce_damage() {
    // Stock row `AIdd`: DataA 0.7 (Pierce), DataE 1 (Spells and Magic), DataG / DataH 1.
    let rows = [row(b"arsh", b"AIdd", b"AIdd", data_level(0.0, 0.0, 0.0, &[(0, 0.7), (1, 1.0), (4, 1.0), (6, 1.0), (7, 1.0)]))];
    let h = started(&rows, c(b"arsh"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    // JASS attack types: 2 is Pierce, 4 is Magic.
    let before = h.game().unit(u).life.to_f32();
    h.game_mut().unit_damage_target(foe, u, f(100.0), false, false, 2, 4);
    let after = h.game().unit(u).life.to_f32();
    assert!((before - after - 70.0).abs() < 0.01, "pierce damage x 0.7, lost {}", before - after);
    h.game_mut().unit_damage_target(foe, u, f(100.0), false, false, 4, 4);
    assert!((after - h.game().unit(u).life.to_f32() - 100.0).abs() < 0.01, "magic damage unchanged (DataE 1)");
}

#[test]
fn boots_do_not_stack_the_highest_bonus_counts() {
    let rows = [
        row(b"bsp1", b"AIm1", b"AIms", data_level(0.0, 0.0, 0.0, &[(0, 60.0)])),
        row(b"bsp2", b"AIm2", b"AIms", data_level(0.0, 0.0, 0.0, &[(0, 50.0)])),
    ];
    let mut h = started(&rows, c(b"bsp1"), &|_| {});
    let u = gu(&h, "u");
    let base = 270.0; // the courier type's speed
    assert_eq!(h.game().unit(u).move_speed.to_f32(), base + 60.0);
    add_second(&mut h, c(b"bsp2"));
    assert_eq!(h.game().unit(u).move_speed.to_f32(), base + 60.0, "the second pair adds nothing");
    call(&mut h, "drop");
    assert_eq!(h.game().unit(u).move_speed.to_f32(), base + 50.0, "the weaker pair still counts");
    call(&mut h, "drop_b");
    assert_eq!(h.game().unit(u).move_speed.to_f32(), base);
}

#[test]
fn corruption_orb_takes_data_b_off_the_victim_armor() {
    // Stock row `AIcb`: Dur 5, HeroDur 5, DataA 5, DataB 5; the amount is DataB.
    let rows = [row(b"ocor", b"AIcb", b"AIcb", data_level(5.0, 5.0, 0.0, &[(0, 1.0), (1, 4.0)]))];
    let mut h = started(&rows, c(b"ocor"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    on_hit(&mut h.game_mut(), u, foe, f(30.0), true);
    assert_eq!(h.game().unit(foe).bonuses.armor.to_f32(), -4.0);
    for _ in 0..60 {
        h.step();
    }
    assert_eq!(h.game().unit(foe).bonuses.armor.to_f32(), 0.0);
}

#[test]
fn freezing_orb_stuns_the_victim() {
    let rows = [row(b"ozb1", b"AIzb", b"AIzb", data_level(3.0, 1.0, 0.0, &[(0, 9.0)]))];
    let mut h = started(&rows, c(b"ozb1"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    on_hit(&mut h.game_mut(), u, foe, f(30.0), true);
    assert!(h.game().is_stunned(foe), "Bfre is a stun");
    for _ in 0..40 {
        h.step();
    }
    assert!(!h.game().is_stunned(foe), "after the 3 s");
}

#[test]
fn lightning_orb_fires_the_purge_of_its_companion_ability() {
    // `olig`: abilList `AIlb,AIlp`; the orb itself has no damage, the purge slows the victim (Dur 3, DataA 2).
    let rows = [
        row(b"olig", b"AIlb", b"AIlb", data_level(0.0, 0.0, 0.0, &[(0, 6.0)])),
        row(b"olig", b"AIlp", b"AIlp", data_level(3.0, 0.3, 0.0, &[(0, 2.0), (2, 35.0)])),
    ];
    let h = started(&rows, c(b"olig"), &|d| {
        d.specs.get_mut(&c(b"olig")).unwrap().abilities = vec![c(b"AIlb"), c(b"AIlp")];
    });
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    assert!(h.game().unit(u).abilities.contains(c(b"AIlp")));
    let before = h.game().unit(foe).life.to_f32();
    on_hit(&mut h.game_mut(), u, foe, f(30.0), true);
    assert!(h.game().unit(foe).buffs.iter().any(|b| b.base == sim::unit_spells::BUFF_PURGE), "the purge slow");
    assert_eq!(h.game().unit(foe).life.to_f32(), before, "no damage of its own");
}

#[test]
fn lightning_purge_item_casts_the_purge_on_a_target() {
    let rows = [row(b"olig", b"AIlp", b"AIlp", data_level(3.0, 0.3, 0.0, &[(0, 2.0), (2, 35.0)]))];
    let mut h = started(&rows, c(b"olig"), &|_| {});
    let foe = gu(&h, "foe");
    call(&mut h, "use_foe");
    assert!(accepted(&h), "the lightning purge is castable on a unit");
    assert!(h.game().unit(foe).buffs.iter().any(|b| b.base == sim::unit_spells::BUFF_PURGE));
}
