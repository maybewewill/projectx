//! Item natives through `ScriptHost` (`natives/item.rs`, the model in `natives/game.rs`, spec
//! `docs/spec/items.md`).
//!
//! The scripts use integer-valued reals (`I2R`), so they need no lookup tables for literals. The random item
//! selection is checked against direct calls on the generator's stream 35. The permutation table is the user's
//! `Game.dll` when `GAME_DLL` is set, and an identity stand-in otherwise: the checks compare the model with the
//! same generator, so they hold for either table.

use jass::compile::compile;
use jass::vm::NativeRegistry;
use sim::natives::game::{item_flags, ItemOrderKind, ItemType, ObjKey, UnitType};
use sim::{RngTable, ScriptHost, Streams, HANDLE_BASE};
use softfloat::F32;

const PRELUDE: &str = r#"
type player extends handle
type widget extends handle
type unit extends widget
type item extends handle
type location extends handle
type rect extends handle
type boolexpr extends handle
type filterfunc extends boolexpr
type hashtable extends handle

native I2R takes integer i returns real
native R2I takes real r returns integer
native GetHandleId takes handle h returns integer
native Player takes integer i returns player
native GetPlayerId takes player p returns integer
native Rect takes real minx, real miny, real maxx, real maxy returns rect
native Filter takes code c returns filterfunc
native InitHashtable takes nothing returns hashtable
native SaveItemHandle takes hashtable t, integer p, integer c, item whichItem returns boolean
native LoadItemHandle takes hashtable t, integer p, integer c returns item
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native GetUnitX takes unit whichUnit returns real
native SetUnitPosition takes unit whichUnit, real newX, real newY returns nothing
native CreateItem takes integer itemid, real x, real y returns item
native RemoveItem takes item whichItem returns nothing
native SetItemPosition takes item whichItem, real x, real y returns nothing
native GetItemX takes item whichItem returns real
native GetItemY takes item whichItem returns real
native GetItemTypeId takes item whichItem returns integer
native GetItemCharges takes item whichItem returns integer
native SetItemCharges takes item whichItem, integer charges returns nothing
native GetItemPlayer takes item whichItem returns player
native SetItemPlayer takes item whichItem, player whichPlayer, boolean changeColor returns nothing
native IsItemOwned takes item whichItem returns boolean
native SetItemVisible takes item whichItem, boolean show returns nothing
native IsItemVisible takes item whichItem returns boolean
native SetItemInvulnerable takes item whichItem, boolean flag returns nothing
native IsItemInvulnerable takes item whichItem returns boolean
native SetItemDropOnDeath takes item whichItem, boolean flag returns nothing
native SetItemDroppable takes item whichItem, boolean flag returns nothing
native SetItemPawnable takes item whichItem, boolean flag returns nothing
native IsItemPowerup takes item whichItem returns boolean
native IsItemPawnable takes item whichItem returns boolean
native UnitAddItem takes unit whichUnit, item whichItem returns boolean
native UnitAddItemById takes unit whichUnit, integer itemId returns item
native UnitAddItemToSlotById takes unit whichUnit, integer itemId, integer itemSlot returns boolean
native UnitRemoveItem takes unit whichUnit, item whichItem returns nothing
native UnitRemoveItemFromSlot takes unit whichUnit, integer itemSlot returns item
native UnitHasItem takes unit whichUnit, item whichItem returns boolean
native UnitItemInSlot takes unit whichUnit, integer itemSlot returns item
native UnitInventorySize takes unit whichUnit returns integer
native UnitDropItemPoint takes unit whichUnit, item whichItem, real x, real y returns boolean
native UnitDropItemSlot takes unit whichUnit, item whichItem, integer slot returns boolean
native UnitDropItemTarget takes unit whichUnit, item whichItem, widget target returns boolean
native EnumItemsInRect takes rect r, boolexpr filter, code actionFunc returns nothing
native GetEnumItem takes nothing returns item
native GetFilterItem takes nothing returns item
native ChooseRandomItem takes integer level returns integer
native ChooseRandomItemEx takes integer itemType, integer level returns integer
native GetRandomInt takes integer lo, integer hi returns integer
"#;

/// Unit type ids: `'hfoo'` has no inventory, `'hpk2'` has two slots.
const FOOTMAN: u32 = 0x6866_6F6F;
const PACK2: u32 = 0x6870_6B32;

/// Item type ids: a plain item, a charged item (4 charges), and a powerup.
const PLAIN: u32 = 0x7261_7463; // 'ratc'
const CHARGED: u32 = 0x7263_6861; // 'rcha'
const POWERUP: u32 = 0x7270_7772; // 'rpwr'

/// The permutation table of the user's `Game.dll` (`GAME_DLL`), or an identity stand-in.
fn rng_table() -> RngTable {
    if let Ok(path) = std::env::var("GAME_DLL") {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading GAME_DLL={path}: {e}"));
        return RngTable::from_game_dll(&bytes).expect("RNG table in GAME_DLL");
    }
    let mut bytes = [0u8; 256];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = i as u8;
    }
    RngTable::from_bytes(bytes).expect("identity is a permutation")
}

fn ut(name: &str, inventory_size: u32) -> UnitType {
    UnitType {
        name: name.to_string(),
        max_life: F32::from_bits(0x43D2_0000),
        max_mana: F32::ZERO,
        start_mana: F32::ZERO,
        inventory_size,
        ..Default::default()
    }
}

fn item_type(
    class: u32,
    level_plain: i32,
    level_ex: i32,
    flag_48: u32,
    charges: i32,
    flags: u32,
) -> ItemType {
    ItemType {
        class,
        level_ex,
        level_plain,
        flag_48,
        charges,
        flags,
        powerup: false,
        sellable: false,
        pawnable: false,
    }
}

/// A host with the unit types and the RNG table, and no item types (see [`add_items`]).
fn base_host(src: &str) -> ScriptHost {
    let full = format!("{PRELUDE}\n{src}");
    let prog = jass::parse(&full).expect("parse");
    let module = compile(&[prog]).expect("compile");
    let host = ScriptHost::new(module, NativeRegistry::with_builtins(), None).expect("vm");
    host.set_rng_table(rng_table());
    {
        let mut g = host.game_mut();
        g.unit_types_mut().insert(FOOTMAN, ut("Footman", 0));
        g.unit_types_mut().insert(PACK2, ut("Packhorse", 2));
    }
    host
}

/// The standard item types: a plain item, a charged item, and a powerup.
fn add_standard_items(host: &ScriptHost) {
    let mut g = host.game_mut();
    let types = g.item_types_mut();
    types.insert(PLAIN, item_type(1, 1, 1, 1, 0, 0));
    types.insert(CHARGED, item_type(1, 1, 1, 1, 4, 0));
    types.insert(POWERUP, item_type(1, 1, 1, 1, 0, item_flags::POWERUP));
}

fn host_for(src: &str) -> ScriptHost {
    let host = base_host(src);
    add_standard_items(&host);
    host
}

/// Runs the host for a few steps: the main thread and any chain it started.
fn run(host: &mut ScriptHost) {
    for _ in 0..3 {
        host.step();
    }
    assert!(host.misfires().is_empty(), "no thread was refused");
}

fn int(host: &ScriptHost, name: &str) -> i32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_integer())
        .unwrap_or_else(|| panic!("global {name} is not an integer"))
}

fn boolean(host: &ScriptHost, name: &str) -> bool {
    host.vm()
        .global(name)
        .and_then(|v| v.as_boolean())
        .unwrap_or_else(|| panic!("global {name} is not a boolean"))
}

/// The item object behind an item handle id.
fn item_key(host: &ScriptHost, id: u32) -> ObjKey {
    host.game()
        .handle_object(id)
        .unwrap_or_else(|| panic!("handle {id} has no object"))
}

fn item_flags_of(host: &ScriptHost, id: u32) -> u32 {
    host.game().item(item_key(host, id)).flags
}

/// The `GetHandleId` of an integer global.
fn id_of(host: &ScriptHost, name: &str) -> i32 {
    int(host, name)
}

/// The handle id of a `unit` or `item` global.
fn handle_of(host: &ScriptHost, name: &str) -> u32 {
    host.vm()
        .global(name)
        .and_then(|v| v.as_handle())
        .unwrap_or_else(|| panic!("global {name} is not a handle"))
}

/// Globals shared by the scripts. Items are `item` globals, units are `unit` globals.
const COMMON: &str = r#"
globals
integer acc = 0
integer cnt = 0
integer fcnt = 0
integer same = 0
integer ivar = 0
integer ivar2 = 0
integer ivar3 = 0
boolean b1 = false
boolean b2 = false
boolean b3 = false
boolean b4 = false
boolean b5 = false
boolean b6 = false
boolean b7 = false
boolean b8 = false
boolean b9 = false
boolean b10 = false
boolean b11 = false
boolean b12 = false
unit u1
unit u2
unit pk
item it1
item it2
item it3
item it4
hashtable ht
integer i1 = 0
integer u2id = 0
integer i2 = 0
integer u3id = 0
integer badid = -1
integer itid = 0
integer loaded = -1
integer uid = 0
integer pid = 0
integer nid = 0
integer single = 0
integer none = -1
integer array P
integer array E
integer array X
endglobals

function rec takes nothing returns nothing
    set acc = acc * 10 + R2I(GetItemX(GetEnumItem()))
    set cnt = cnt + 1
    if GetFilterItem() == GetEnumItem() then
        set same = same + 1
    endif
endfunction

function filt_x_ge_two takes nothing returns boolean
    set fcnt = fcnt * 10 + R2I(GetItemX(GetFilterItem()))
    return R2I(GetItemX(GetFilterItem())) >= 2
endfunction
"#;

fn script(body: &str) -> String {
    format!("{COMMON}\n{body}")
}

/// Ids of items and units: a handle-id-first style. A freed id comes back in the next creation (LIFO).
#[test]
fn freed_item_id_is_reused_by_the_next_unit_and_failed_creation_takes_no_id() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    local unit u
    local unit uu
    local item bad
    set u1 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set it1 = CreateItem('ratc', I2R(1), I2R(1))
    set i1 = GetHandleId(it1)
    call RemoveItem(it1)
    set uu = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u2id = GetHandleId(uu)
    set it2 = CreateItem('ratc', I2R(2), I2R(2))
    set i2 = GetHandleId(it2)
    set bad = CreateItem('zzzz', I2R(0), I2R(0))
    set badid = GetHandleId(bad)
    set u = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set u3id = GetHandleId(u)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    let i1 = id_of(&host, "i1");
    assert!(i1 as u32 >= HANDLE_BASE, "items get ids from the base");
    assert_eq!(
        id_of(&host, "u2id"),
        i1,
        "the unit takes the id the removed item freed"
    );
    assert_ne!(id_of(&host, "i2"), i1, "a new item gets a fresh id");
    assert_eq!(
        id_of(&host, "badid"),
        0,
        "a failed creation gives the null handle"
    );
    assert_eq!(
        id_of(&host, "u3id"),
        id_of(&host, "i2") + 1,
        "a failed creation consumed no id"
    );
}

/// A saved handle keeps its id after `RemoveItem`: the table holds the reference, so the id is not reused, but
/// the load gives null (the object is gone).
#[test]
fn a_saved_item_keeps_its_id_and_loads_as_null_after_removal() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    local unit u
    set ht = InitHashtable()
    set it1 = CreateItem('ratc', I2R(1), I2R(1))
    set itid = GetHandleId(it1)
    set b1 = SaveItemHandle(ht, 0, 0, it1)
    call RemoveItem(it1)
    set loaded = GetHandleId(LoadItemHandle(ht, 0, 0))
    set u = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set uid = GetHandleId(u)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(boolean(&host, "b1"), "a live item can be saved");
    assert_eq!(id_of(&host, "loaded"), 0, "a removed item loads as null");
    assert_ne!(
        id_of(&host, "uid"),
        id_of(&host, "itid"),
        "the saved id is still reserved"
    );
}

#[test]
fn unit_pack_fills_lowest_slot_and_a_full_pack_refuses_items() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set pk = CreateUnit(Player(0), 'hpk2', I2R(5), I2R(6), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set it1 = CreateItem('ratc', I2R(0), I2R(0))
    set it2 = CreateItem('ratc', I2R(0), I2R(0))
    set it3 = CreateItem('ratc', I2R(0), I2R(0))
    set b1 = UnitAddItem(pk, it1)
    set b2 = UnitAddItem(pk, it2)
    set b3 = UnitAddItem(pk, it3)
    set ivar = UnitInventorySize(pk)
    set ivar2 = UnitInventorySize(u2)
    set b4 = UnitHasItem(pk, it1)
    set b5 = UnitHasItem(pk, it3)
    set b6 = (GetHandleId(UnitItemInSlot(pk, 2)) == 0)
    set b7 = (GetHandleId(UnitItemInSlot(pk, 0)) == GetHandleId(it1))
    set b8 = (GetHandleId(UnitItemInSlot(pk, 1)) == GetHandleId(it2))
    set b9 = UnitAddItem(u2, CreateItem('ratc', I2R(0), I2R(0)))
    set ivar3 = GetHandleId(UnitRemoveItemFromSlot(pk, 0))
    set b10 = (ivar3 == GetHandleId(it1))
    set b11 = UnitAddItem(pk, it3)
    set b12 = (GetHandleId(UnitItemInSlot(pk, 0)) == GetHandleId(it3))
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(boolean(&host, "b2"));
    assert!(!boolean(&host, "b3"), "a full pack refuses the third item");
    assert_eq!(int(&host, "ivar"), 2);
    assert_eq!(
        int(&host, "ivar2"),
        0,
        "a unit without an inventory has no slots"
    );
    assert!(boolean(&host, "b4"));
    assert!(!boolean(&host, "b5"), "the refused item is not in the pack");
    assert!(boolean(&host, "b6"), "a slot past the capacity is empty");
    assert!(boolean(&host, "b7"));
    assert!(boolean(&host, "b8"));
    assert!(
        !boolean(&host, "b9"),
        "a unit without an inventory refuses items"
    );
    assert_eq!(
        int(&host, "ivar3"),
        handle_of(&host, "it1") as i32,
        "slot 0 returns its item"
    );
    assert!(boolean(&host, "b10"));
    assert!(
        boolean(&host, "b11"),
        "the freed slot 0 takes the next item"
    );
    assert!(boolean(&host, "b12"), "the lowest free slot wins");
}

#[test]
fn powerup_is_consumed_and_never_takes_a_slot() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    local unit u
    set pk = CreateUnit(Player(0), 'hpk2', I2R(0), I2R(0), I2R(0))
    set u2 = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set it1 = CreateItem('ratc', I2R(0), I2R(0))
    set it2 = CreateItem('ratc', I2R(0), I2R(0))
    set b1 = UnitAddItem(pk, it1)
    set b2 = UnitAddItem(pk, it2)
    set it3 = CreateItem('rpwr', I2R(0), I2R(0))
    set pid = GetHandleId(it3)
    set b3 = UnitAddItem(pk, it3)
    set b4 = UnitHasItem(pk, it3)
    set b5 = (GetItemTypeId(it3) == 0)
    set ivar = UnitInventorySize(pk)
    set u = CreateUnit(Player(0), 'hfoo', I2R(0), I2R(0), I2R(0))
    set nid = GetHandleId(u)
    set it4 = CreateItem('rpwr', I2R(0), I2R(0))
    set b6 = UnitAddItem(u2, it4)
    set b7 = IsItemPowerup(it4)
    set b8 = UnitAddItem(pk, it4)
    set b9 = IsItemOwned(it4)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(
        boolean(&host, "b3"),
        "a powerup is taken even by a full pack"
    );
    assert!(!boolean(&host, "b4"), "a consumed powerup is in no pack");
    assert!(
        boolean(&host, "b5"),
        "a consumed powerup is no longer a valid item"
    );
    assert_eq!(int(&host, "ivar"), 2);
    assert_eq!(
        int(&host, "nid"),
        int(&host, "pid"),
        "the consumed id is freed and reused"
    );
    assert!(
        !boolean(&host, "b6"),
        "a unit without a pack cannot take a powerup"
    );
    assert!(boolean(&host, "b7"));
    assert!(
        boolean(&host, "b8"),
        "the refused powerup is still valid and is consumed by a unit with a pack"
    );
    assert!(!boolean(&host, "b9"));
}

#[test]
fn charges_follow_the_zero_and_negative_rule() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set it1 = CreateItem('rcha', I2R(0), I2R(0))
    set it2 = CreateItem('ratc', I2R(0), I2R(0))
    set ivar = GetItemCharges(it1)
    set ivar2 = GetItemCharges(it2)
    call SetItemCharges(it1, 9)
    set ivar3 = GetItemCharges(it1)
    call SetItemCharges(it1, -3)
    set b1 = (GetItemCharges(it1) == 0)
    call SetItemCharges(it1, 2)
    set b2 = (GetItemCharges(it1) == 2)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    let charged = int(&host, "ivar");
    assert_eq!(charged, 4, "the template charges");
    assert_eq!(
        int(&host, "ivar2"),
        0,
        "a plain item starts with no charges"
    );
    assert_eq!(int(&host, "ivar3"), 9);
    assert!(boolean(&host, "b1"));
    assert!(boolean(&host, "b2"));
    // The flag word: a positive charge clears the no-charges bit, zero or less sets it.
    let it2 = handle_of(&host, "it2");
    assert_ne!(item_flags_of(&host, it2) & item_flags::NO_CHARGES, 0);
    let it1 = handle_of(&host, "it1");
    assert_eq!(item_flags_of(&host, it1) & item_flags::NO_CHARGES, 0);
}

#[test]
fn visibility_invulnerability_flags_and_owner() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set it1 = CreateItem('ratc', I2R(3), I2R(4))
    set b1 = IsItemVisible(it1)
    call SetItemVisible(it1, false)
    set b2 = IsItemVisible(it1)
    call SetItemVisible(it1, true)
    set b3 = IsItemVisible(it1)
    call SetItemInvulnerable(it1, true)
    set b4 = IsItemInvulnerable(it1)
    call SetItemPawnable(it1, true)
    set b5 = IsItemPawnable(it1)
    set b6 = IsItemPowerup(it1)
    set it2 = CreateItem('rpwr', I2R(0), I2R(0))
    set b7 = IsItemPowerup(it2)
    set ivar = GetPlayerId(GetItemPlayer(it1))
    call SetItemPlayer(it1, Player(3), true)
    set ivar2 = GetPlayerId(GetItemPlayer(it1))
    set b8 = IsItemOwned(it1)
    set pk = CreateUnit(Player(0), 'hpk2', I2R(0), I2R(0), I2R(0))
    set b9 = UnitAddItem(pk, it1)
    set b10 = IsItemOwned(it1)
    call SetItemVisible(it1, false)
    set b11 = IsItemVisible(it1)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(boolean(&host, "b1"), "a new item is visible");
    assert!(!boolean(&host, "b2"));
    assert!(boolean(&host, "b3"));
    assert!(boolean(&host, "b4"));
    assert!(boolean(&host, "b5"));
    assert!(!boolean(&host, "b6"), "a plain item is not a powerup");
    assert!(boolean(&host, "b7"));
    assert_eq!(
        int(&host, "ivar"),
        0,
        "a new item belongs to player 0 (UNSURE)"
    );
    assert_eq!(int(&host, "ivar2"), 3);
    assert!(!boolean(&host, "b8"), "an item on the ground is not owned");
    assert!(boolean(&host, "b9"));
    assert!(boolean(&host, "b10"));
    assert!(boolean(&host, "b11"), "a carried item cannot be hidden");
}

#[test]
fn removing_from_a_pack_puts_the_item_at_the_unit_and_moving_it_detaches_it() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set pk = CreateUnit(Player(0), 'hpk2', I2R(5), I2R(6), I2R(0))
    set it1 = CreateItem('ratc', I2R(0), I2R(0))
    set b1 = UnitAddItem(pk, it1)
    call SetItemPosition(it1, I2R(9), I2R(9))
    set b2 = UnitHasItem(pk, it1)
    set b3 = IsItemOwned(it1)
    set ivar = R2I(GetItemX(it1))
    set b4 = UnitAddItem(pk, it1)
    call UnitRemoveItem(pk, it1)
    set b5 = UnitHasItem(pk, it1)
    set ivar2 = R2I(GetItemX(it1))
    set ivar3 = R2I(GetItemY(it1))
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(
        !boolean(&host, "b2"),
        "moving the item takes it out of the pack"
    );
    assert!(!boolean(&host, "b3"));
    assert_eq!(int(&host, "ivar"), 9);
    assert!(boolean(&host, "b4"));
    assert!(!boolean(&host, "b5"));
    assert_eq!(
        int(&host, "ivar2"),
        5,
        "a removed item lands at the unit's position"
    );
    assert_eq!(int(&host, "ivar3"), 6);
}

#[test]
fn add_by_id_creates_the_item_at_the_unit_and_returns_its_handle_even_on_a_full_pack() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set pk = CreateUnit(Player(0), 'hpk2', I2R(7), I2R(8), I2R(0))
    set it1 = UnitAddItemById(pk, 'ratc')
    set it2 = UnitAddItemById(pk, 'ratc')
    set it3 = UnitAddItemById(pk, 'ratc')
    set b1 = (GetHandleId(it3) != 0)
    set b2 = IsItemOwned(it3)
    set ivar = R2I(GetItemX(it3))
    set ivar2 = R2I(GetItemY(it3))
    set b3 = UnitHasItem(pk, it1)
    set b4 = UnitHasItem(pk, it2)
    set ivar3 = UnitInventorySize(pk)
    set b5 = UnitAddItemToSlotById(pk, 'ratc', 1)
    set b6 = UnitAddItemToSlotById(pk, 'ratc', 5)
    set b7 = (GetHandleId(UnitAddItemById(pk, 'zzzz')) == 0)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(
        boolean(&host, "b1"),
        "the handle comes back even when the add fails"
    );
    assert!(
        !boolean(&host, "b2"),
        "the refused item stays on the ground"
    );
    assert_eq!(int(&host, "ivar"), 7);
    assert_eq!(int(&host, "ivar2"), 8);
    assert!(boolean(&host, "b3"));
    assert!(boolean(&host, "b4"));
    assert_eq!(int(&host, "ivar3"), 2);
    assert!(
        !boolean(&host, "b5"),
        "slot 1 is taken, so the slot form fails"
    );
    assert!(!boolean(&host, "b6"), "a slot past the capacity fails");
    assert!(
        boolean(&host, "b7"),
        "an unknown type gives the null handle"
    );
    let refused = host
        .vm()
        .global("it3")
        .and_then(|v| v.as_handle())
        .expect("it3");
    assert_ne!(item_flags_of(&host, refused) & item_flags::ADDED_BY_ID, 0);
}

#[test]
fn drop_natives_queue_orders_and_check_the_pack_and_the_slot() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set pk = CreateUnit(Player(0), 'hpk2', I2R(0), I2R(0), I2R(0))
    set it1 = CreateItem('ratc', I2R(0), I2R(0))
    set b1 = UnitDropItemPoint(pk, it1, I2R(3), I2R(4))
    call UnitAddItem(pk, it1)
    set b2 = UnitDropItemPoint(pk, it1, I2R(3), I2R(4))
    set b3 = UnitDropItemSlot(pk, it1, 1)
    set b4 = UnitDropItemSlot(pk, it1, 2)
    set b5 = UnitDropItemSlot(pk, it1, -1)
    set b6 = UnitDropItemTarget(pk, it1, pk)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(
        !boolean(&host, "b1"),
        "an item outside the pack cannot be dropped"
    );
    assert!(boolean(&host, "b2"));
    assert!(boolean(&host, "b3"));
    assert!(!boolean(&host, "b4"), "a slot past the capacity fails");
    assert!(!boolean(&host, "b5"), "a negative slot fails");
    assert!(boolean(&host, "b6"));

    let game = host.game();
    let orders = game.item_orders();
    assert_eq!(orders.len(), 3, "only the accepted drops are queued");
    assert_eq!(
        orders[0].kind,
        ItemOrderKind::DropPoint {
            x: F32::from_bits(0x4040_0000),
            y: F32::from_bits(0x4080_0000)
        }
    );
    assert_eq!(orders[0].kind.order_id(), Some(0x000D_0021));
    assert_eq!(orders[1].kind, ItemOrderKind::DropSlot { slot: 1 });
    assert_eq!(orders[1].kind.order_id(), Some(0x000D_0023));
    assert!(matches!(orders[2].kind, ItemOrderKind::DropTarget { .. }));
    assert_eq!(orders[0].unit, orders[1].unit);
}

/// The random item selection (spec 7): the candidate lists are checked by the sequence of results. Each
/// `ChooseRandom*` call draws once when it has candidates, and the expected picks are computed from the same
/// stream 35 by hand here.
#[test]
fn choose_random_item_picks_from_stream_35_over_the_candidates() {
    const A: u32 = 0x726E_6461; // 'rnda'
    const B: u32 = 0x726E_6462; // 'rndb'
    const C: u32 = 0x726E_6463; // 'rndc'
    const D: u32 = 0x726E_6464; // 'rndd'
    let mut host = base_host(&script(
        r#"
function main takes nothing returns nothing
    local integer i = 0
    loop
        exitwhen i == 20
        set P[i] = ChooseRandomItem(1)
        set E[i] = ChooseRandomItemEx(1, -1)
        set X[i] = ChooseRandomItemEx(8, -1)
        set i = i + 1
    endloop
    set single = ChooseRandomItemEx(1, 1)
    set none = ChooseRandomItemEx(9, -1)
endfunction
"#,
    ));
    {
        let mut g = host.game_mut();
        let types = g.item_types_mut();
        // class, level_plain, level_ex, flag_48, charges, flags
        types.insert(A, item_type(1, 1, 2, 1, 0, 0));
        types.insert(B, item_type(2, 1, 3, 1, 0, 0));
        types.insert(C, item_type(1, 2, 1, 1, 0, 0));
        types.insert(D, item_type(1, 1, 1, 0, 0, 0));
    }
    let seed = 0x1234_5678;
    assert!(host.start_game_rng(seed));
    host.start("main", &[]).expect("start");
    run(&mut host);

    // The expected picks, from the same generator state and stream 35.
    let t = rng_table();
    let mut s = Streams::default();
    s.start_game(&t, seed);
    let pick = |s: &mut Streams, cands: &[u32]| -> i32 {
        if cands.is_empty() {
            return 0;
        }
        let r = s.stream_mut(35).next_u32(&t);
        let index = ((u64::from(r) * cands.len() as u64) >> 32) as usize;
        cands[index] as i32
    };
    let plain = [A, B];
    let ex_class1 = [A, C];
    let ex_any = [A, B, C];
    let (mut p, mut e, mut x) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..20 {
        p.push(pick(&mut s, &plain));
        e.push(pick(&mut s, &ex_class1));
        x.push(pick(&mut s, &ex_any));
    }
    let single = pick(&mut s, &[C]);
    let none = pick(&mut s, &[]);

    for i in 0..20 {
        assert_eq!(
            host.vm()
                .global_element("P", i)
                .and_then(|v| v.as_integer()),
            Some(p[i]),
            "P[{i}]"
        );
        assert_eq!(
            host.vm()
                .global_element("E", i)
                .and_then(|v| v.as_integer()),
            Some(e[i]),
            "E[{i}]"
        );
        assert_eq!(
            host.vm()
                .global_element("X", i)
                .and_then(|v| v.as_integer()),
            Some(x[i]),
            "X[{i}]"
        );
    }
    assert_eq!(int(&host, "single"), single, "one candidate still draws");
    assert_eq!(int(&host, "none"), 0, "no candidates gives 0");
    assert_eq!(none, 0);
    assert_eq!(
        *host.rng(),
        s,
        "the stream state matches, with no draw for the empty call"
    );
    // The picks are drawn from the candidate lists only.
    assert!(p.iter().all(|&v| v == A as i32 || v == B as i32));
    assert!(e.iter().all(|&v| v == A as i32 || v == C as i32));
}

/// Encodes the visit order of `EnumItemsInRect`: the callback adds each item's x to an accumulator.
#[test]
fn enum_items_in_rect_visits_ground_items_in_creation_order() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set it1 = CreateItem('ratc', I2R(1), I2R(1))
    set it2 = CreateItem('ratc', I2R(2), I2R(2))
    set it3 = CreateItem('ratc', I2R(5), I2R(5))
    set it4 = CreateItem('ratc', I2R(1), I2R(1))
    call RemoveItem(it4)
    set pk = CreateUnit(Player(0), 'hpk2', I2R(0), I2R(0), I2R(0))
    set it4 = CreateItem('ratc', I2R(1), I2R(1))
    call UnitAddItem(pk, it4)
    call EnumItemsInRect(Rect(I2R(0), I2R(0), I2R(3), I2R(3)), null, function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(
        int(&host, "acc"),
        12,
        "items at x = 1 then x = 2, the removed and carried ones skipped"
    );
    assert_eq!(int(&host, "cnt"), 2);
    assert_eq!(
        int(&host, "same"),
        2,
        "GetFilterItem is the candidate for the action too"
    );
}

/// The boolexpr sees `GetFilterItem` as the candidate, as for units; a failed filter skips the action.
#[test]
fn enum_items_filter_is_evaluated_per_candidate_and_gates_the_action() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set it1 = CreateItem('ratc', I2R(1), I2R(1))
    set it2 = CreateItem('ratc', I2R(2), I2R(2))
    set it3 = CreateItem('ratc', I2R(5), I2R(5))
    call EnumItemsInRect(Rect(I2R(0), I2R(0), I2R(3), I2R(3)), Filter(function filt_x_ge_two), function rec)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert_eq!(
        int(&host, "fcnt"),
        12,
        "the filter sees the candidates in order"
    );
    assert_eq!(int(&host, "acc"), 2, "only the item at x = 2 passes");
    assert_eq!(int(&host, "cnt"), 1);
    assert_eq!(int(&host, "same"), 1);
    // Outside the enumeration the getters return the null handle.
    let game = host.game();
    assert_eq!(game.enum_item(), 0);
    assert_eq!(game.filter_item(), 0);
}

/// `GetEnumItem` and `GetFilterItem` are null outside the callbacks.
#[test]
fn enum_getters_are_null_outside_the_callbacks() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set b1 = (GetHandleId(GetEnumItem()) == 0)
    set b2 = (GetHandleId(GetFilterItem()) == 0)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(boolean(&host, "b1"));
    assert!(boolean(&host, "b2"));
}

/// A removed item reads as invalid, and removing it twice does nothing more.
#[test]
fn removed_item_reads_as_invalid_and_double_removal_is_harmless() {
    let mut host = host_for(&script(
        r#"
function main takes nothing returns nothing
    set it1 = CreateItem('ratc', I2R(0), I2R(0))
    call RemoveItem(it1)
    call RemoveItem(it1)
    set b1 = IsItemVisible(it1)
    set b2 = IsItemOwned(it1)
endfunction
"#,
    ));
    host.start("main", &[]).expect("start");
    run(&mut host);
    assert!(
        !boolean(&host, "b1"),
        "a removed item reads as invalid (false)"
    );
    assert!(!boolean(&host, "b2"));
}
