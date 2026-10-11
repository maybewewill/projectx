//! Shop stock natives through `ScriptHost` (`natives/shop.rs`, `crate::shop`, spec `docs/spec/shop-stock.md`).

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::abilities::Ability;
use sim::natives::game::{ItemType, UnitType};
use sim::shop::{TypeStock, SELL_ITEMS, SELL_UNITS};
use sim::ScriptHost;
use softfloat::{conv, F32};

const SRC: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
native Player takes integer i returns player
native I2R takes integer i returns real
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native AddItemToStock takes unit whichUnit, integer itemId, integer currentStock, integer stockMax returns nothing
native AddItemToAllStock takes integer itemId, integer currentStock, integer stockMax returns nothing
native RemoveItemFromStock takes unit whichUnit, integer itemId returns nothing
native RemoveItemFromAllStock takes integer itemId returns nothing
native SetItemTypeSlots takes unit whichUnit, integer slots returns nothing
native SetAllItemTypeSlots takes integer slots returns nothing
native AddUnitToStock takes unit whichUnit, integer unitId, integer currentStock, integer stockMax returns nothing
native AddUnitToAllStock takes integer unitId, integer currentStock, integer stockMax returns nothing
native RemoveUnitFromAllStock takes integer unitId returns nothing
native SetUnitTypeSlots takes unit whichUnit, integer slots returns nothing

globals
unit shop1
unit shop2
unit plain
endglobals

function setup takes nothing returns nothing
    set shop1 = CreateUnit(Player(0), 'nmrk', I2R(0), I2R(0), I2R(0))
    set shop2 = CreateUnit(Player(0), 'nmrk', I2R(0), I2R(0), I2R(0))
    set plain = CreateUnit(Player(0), 'nmrk', I2R(0), I2R(0), I2R(0))
endfunction

function work takes nothing returns nothing
    call AddItemToStock(shop1, 'ratc', 2, 5)
    call AddItemToStock(plain, 'ratc', 2, 5)
    call AddItemToAllStock('rcha', 1, 0)
    call AddItemToAllStock('zzzz', 1, 0)
    call SetItemTypeSlots(shop2, 1)
    call AddItemToAllStock('ratc', 3, 3)
    call RemoveItemFromStock(shop1, 'rcha')
    call AddUnitToStock(shop2, 'nmrk', 1, 1)
    call AddUnitToAllStock('nmrk', 0, 0)
    call SetUnitTypeSlots(shop2, 0)
    call SetAllItemTypeSlots(99)
endfunction
"#;

const MARKET: u32 = 0x6E6D_726B;
const PLAIN: u32 = 0x7261_7463;
const CHARGED: u32 = 0x7263_6861;

fn item_type() -> ItemType {
    ItemType { class: 1, level_ex: 1, level_plain: 1, flag_48: 1, charges: 0, flags: 0, powerup: false, sellable: true, pawnable: false }
}

#[test]
fn natives_edit_the_lists_of_shops_only() {
    let prog = jass::parse(SRC).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let mut host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(
            MARKET,
            UnitType { name: "Marketplace".into(), max_life: F32::ONE, ..Default::default() },
        );
        g.item_types_mut().insert(PLAIN, item_type());
        g.item_types_mut().insert(CHARGED, item_type());
        g.shops.item_stock.insert(CHARGED, TypeStock { stock_max: 4, regen: conv::i2r(30) });
    }
    host.start("setup", &[]).expect("start");
    for _ in 0..3 {
        host.step();
    }
    let keys: Vec<_> = host.game().unit_objects().map(|(k, _)| k).collect();
    assert_eq!(keys.len(), 3);
    {
        let mut g = host.game_mut();
        for k in &keys[..2] {
            g.unit_mut(*k).abilities.attach(Ability::new(SELL_ITEMS, None));
        }
        g.unit_mut(keys[1]).abilities.attach(Ability::new(SELL_UNITS, None));
    }
    host.start("work", &[]).expect("start");
    for _ in 0..3 {
        host.step();
    }
    assert!(host.misfires().is_empty());
    let g = host.game();
    // The unit without the ability gets nothing.
    assert!(!g.shops.items.contains_key(&keys[2]));
    let s1 = &g.shops.items[&keys[0]];
    // PLAIN (2 of 5) in slot 0, CHARGED (1 of the type default 4) in slot 1; RemoveItemFromStock freed slot 1;
    // the All-add of PLAIN with a max of 3 updated slot 0 in place; the invalid id changed nothing.
    assert_eq!(s1.slots[0].id, PLAIN);
    assert_eq!(s1.slots[0].value, conv::i2r(3));
    assert_eq!(s1.slots[0].max, conv::i2r(3));
    assert_eq!(s1.slots[1].id, 0);
    assert_eq!(s1.cap, 11);
    // SetAllItemTypeSlots(99) clamps to 11.
    let s2 = &g.shops.items[&keys[1]];
    assert_eq!(s2.cap, 11);
    // shop2 had one slot when CHARGED was added and then PLAIN replaced it (capacity 1).
    assert_eq!(s2.slots[0].id, PLAIN);
    // The unit list exists only on shop2 and its capacity is 0.
    assert!(!g.shops.units.contains_key(&keys[0]));
    assert_eq!(g.shops.units[&keys[1]].cap, 0);
    // A restock timer was started where the stock is below the maximum: CHARGED has regen 30.
    assert!(s1.slots[0].restock.is_none());
}
