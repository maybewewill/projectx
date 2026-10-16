//! Behaviour tests of the item abilities added by S45 (`item_abilities.rs`, spec `docs/spec/abilities-items.md` section 16).
//! The level data are pinned values taken from the 1.26a `AbilityData.slk` rows; every behaviour is a model (UNSURE
//! unless the spec says otherwise), so these tests pin the model, not the original.

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
    boolean accepted = false
endglobals
function setup takes integer itemid returns nothing
    set u = CreateUnit(Player(0), 'hpk2', 0.0, 0.0, 0.0)
    set foe = CreateUnit(Player(1), 'hpk2', 100.0, 0.0, 0.0)
    set foe2 = CreateUnit(Player(1), 'hpk2', 900.0, 0.0, 0.0)
    set a = CreateItem(itemid, 0.0, 0.0)
    call UnitAddItem(u, a)
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
const TECH: u32 = 0x5267_666F; // 'Rgfo'

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

#[test]
fn every_s45_code_has_a_class() {
    let own = [
        b"AIan", b"AIbl", b"AIca", b"AIcb", b"AIcf", b"AIco", b"AIct", b"AIda", b"AIdd", b"AIdm", b"AIfa", b"AIfc", b"AIfd",
        b"AIfe", b"AIff", b"AIfg", b"AIfh", b"AIfl", b"AIfm", b"AIfn", b"AIfo", b"AIgl", b"AIil", b"AIlb", b"AIlp", b"AIpb",
        b"AIpi", b"AIpm", b"AIpv", b"AIrb", b"AIri", b"AIrt", b"AIrv", b"AIsb", b"AIso", b"AIta", b"AItb", b"AIte", b"AIzb",
    ];
    for code in own {
        assert!(Family::of_base(c(code)).is_some(), "{}", String::from_utf8_lossy(code));
    }
    // The four classes that run a unit spell's effect.
    for code in [b"AIdc", b"AIsh", b"AImo", b"AIwb"] {
        assert!(Family::of_base(c(code)).is_none());
        assert!(borrowed_class(c(code)).is_some(), "{}", String::from_utf8_lossy(code));
    }
    assert_eq!(borrowed_class(c(b"AIsh")), Some(sim::orc::FERAL_SPIRIT));
    assert_eq!(borrowed_class(c(b"AIhe")), None);
    // Passive classes are not castable; the scrolls are.
    for code in [b"AIfl", b"AIcb", b"AIcf", b"AIpi", b"AIdd"] {
        assert!(!Family::of_base(c(code)).unwrap().castable());
    }
    for code in [b"AIda", b"AIfa", b"AIgl", b"AIrt", b"AIte", b"AIan"] {
        assert!(Family::of_base(c(code)).unwrap().castable());
    }
}

#[test]
fn scroll_of_protection_gives_armor_for_its_duration() {
    let rows = [row(b"tdf1", b"AIda", b"AIda", lvl(30.0, 600.0, 2.0))];
    let mut h = started(&rows, c(b"tdf1"), &|_| {});
    let u = gu(&h, "u");
    call(&mut h, "use");
    assert!(accepted(&h));
    assert_eq!(h.game().unit(u).bonuses.armor.to_f32(), 2.0);
    for _ in 0..320 {
        h.step();
    }
    assert_eq!(h.game().unit(u).bonuses.armor.to_f32(), 0.0, "Bdef ended after 30 s");
}

#[test]
fn corruption_orb_lowers_the_victim_armor_for_its_duration() {
    let rows = [row(b"tcb1", b"AIcb", b"AIcb", lvl(5.0, 0.0, 5.0))];
    let mut h = started(&rows, c(b"tcb1"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    on_hit(&mut h.game_mut(), u, foe, f(30.0), true);
    assert_eq!(h.game().unit(foe).bonuses.armor.to_f32(), -5.0);
    for _ in 0..60 {
        h.step();
    }
    assert_eq!(h.game().unit(foe).bonuses.armor.to_f32(), 0.0);
}

#[test]
fn lightning_orb_adds_its_damage() {
    let rows = [row(b"tlb1", b"AIlb", b"AIlb", lvl(0.0, 0.0, 6.0))];
    let h = started(&rows, c(b"tlb1"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    let before = h.game().unit(foe).life.to_f32();
    on_hit(&mut h.game_mut(), u, foe, f(30.0), true);
    assert_eq!(before - h.game().unit(foe).life.to_f32(), 6.0);
}

#[test]
fn venom_orb_poisons_the_victim_over_time() {
    let rows = [row(b"tpb1", b"AIpb", b"AIpb", lvl(0.0, 0.0, 5.0))];
    let mut h = started(&rows, c(b"tpb1"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    let before = h.game().unit(foe).life.to_f32();
    on_hit(&mut h.game_mut(), u, foe, f(30.0), true);
    assert!(h.game().unit(foe).buffs.iter().any(|b| b.base == sim::neutral_spells::BUFF_POISON));
    for _ in 0..25 {
        h.step();
    }
    let lost = before - h.game().unit(foe).life.to_f32();
    assert!(lost >= 5.0, "at least the first tick, lost {lost}");
}

#[test]
fn cloak_of_flames_burns_nearby_enemies_while_carried() {
    let mut l = lvl(1.0, 160.0, 10.0);
    l.hero_dur = f(1.0);
    let rows = [row(b"tcf1", b"AIcf", b"AIcf", l)];
    let mut h = started(&rows, c(b"tcf1"), &|_| {});
    let (u, foe, far) = (gu(&h, "u"), gu(&h, "foe"), gu(&h, "foe2"));
    assert!(h.game().unit(u).buffs.iter().any(|b| b.base == BUFF_CLOAK));
    // `foe` stands 100 away (inside 160), `foe2` 900 away.
    for _ in 0..25 {
        h.step();
    }
    assert!(h.game().unit(foe).life.to_f32() <= 490.0, "one tick of 10 reached the near enemy");
    assert_eq!(h.game().unit(far).life.to_f32(), 500.0);
    call(&mut h, "drop");
    assert!(!h.game().unit(u).buffs.iter().any(|b| b.base == BUFF_CLOAK), "the buff comes off with the item");
    let now = h.game().unit(foe).life.to_f32();
    for _ in 0..40 {
        h.step();
    }
    assert_eq!(h.game().unit(foe).life.to_f32(), now, "no more burns");
}

#[test]
fn dust_of_appearance_marks_the_enemies_around_the_caster() {
    let mut l = lvl(20.0, 500.0, 3.0);
    l.hero_dur = f(20.0);
    let rows = [row(b"tdt1", b"AItb", b"AItb", l)];
    let mut h = started(&rows, c(b"tdt1"), &|_| {});
    let (foe, far) = (gu(&h, "foe"), gu(&h, "foe2"));
    call(&mut h, "use");
    assert!(accepted(&h));
    assert!(h.game().unit(foe).buffs.iter().any(|b| b.base == BUFF_DETECT));
    assert!(h.game().unit(far).buffs.iter().all(|b| b.base != BUFF_DETECT), "900 is outside Area 500");
}

#[test]
fn land_mine_scroll_creates_its_unit_at_the_point() {
    let mut l = lvl(0.0, 200.0, 0.0);
    l.unit_id = MINE;
    let rows = [row(b"tgm1", b"AIpm", b"AIpm", l)];
    let mut h = started(&rows, c(b"tgm1"), &|_| {});
    let u = gu(&h, "u");
    call(&mut h, "use_point");
    assert!(accepted(&h));
    let g = h.game();
    let owner = g.unit(u).owner;
    let mines: Vec<_> = g.owned_units(owner).into_iter().filter(|&k| g.unit(k).type_id == MINE).collect();
    assert_eq!(mines.len(), 1);
    assert_eq!(g.unit(mines[0]).x.to_f32(), 200.0);
}

#[test]
fn glyph_raises_the_owner_tech() {
    let mut l = lvl(0.0, 0.0, 1.0);
    l.unit_id = TECH;
    let rows = [row(b"tgl1", b"AIgl", b"AIgl", l)];
    let mut h = started(&rows, c(b"tgl1"), &|_| {});
    let u = gu(&h, "u");
    call(&mut h, "use");
    assert!(accepted(&h));
    let owner = h.game().unit(u).owner;
    assert_eq!(h.game().tech_level(owner, TECH), 1);
}

#[test]
fn build_scroll_creates_the_unit_of_the_owner_race() {
    let l = lvl(30.0, 0.0, 0.0);
    let rows = [row(b"tbs1", b"AIbl", b"AIbl", l)];
    let spawn = [c(b"nsce"), MINE, FIG_UNIT, MINE];
    let mut h = started(&rows, c(b"tbs1"), &|d| {
        d.unit_lists.insert(c(b"AIbl"), spawn.to_vec());
    });
    let u = gu(&h, "u");
    h.game_mut().player_mut(0).unwrap().race = 2; // orc: the second entry
    call(&mut h, "use_point");
    assert!(accepted(&h));
    let g = h.game();
    let owner = g.unit(u).owner;
    let n = g.owned_units(owner).into_iter().filter(|&k| g.unit(k).type_id == MINE).count();
    assert_eq!(n, 1);
}

#[test]
fn wand_of_illusion_makes_a_timed_copy() {
    let rows = [row(b"til1", b"AIil", b"AIil", lvl(60.0, 0.0, 0.0))];
    let mut h = started(&rows, c(b"til1"), &|_| {});
    let u = gu(&h, "u");
    call(&mut h, "use");
    assert!(accepted(&h));
    let g = h.game();
    let owner = g.unit(u).owner;
    let copies: Vec<_> = g.owned_units(owner).into_iter().filter(|&k| k != u && g.unit(k).illusion).collect();
    assert_eq!(copies.len(), 1, "one illusion of the caster");
}

#[test]
fn command_staff_takes_a_weak_enemy() {
    let rows = [row(b"tco1", b"AIco", b"AIco", lvl(0.0, 0.0, 5.0))];
    let mut h = started(&rows, c(b"tco1"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    call(&mut h, "use_foe");
    assert!(accepted(&h));
    assert_eq!(h.game().unit(foe).owner, h.game().unit(u).owner);
}

#[test]
fn animate_dead_item_raises_corpses_for_a_while() {
    let mut l = lvl(40.0, 900.0, 6.0);
    l.data[1] = f(1.0);
    let rows = [row(b"tan1", b"AIan", b"AIan", l)];
    let mut h = started(&rows, c(b"tan1"), &|_| {});
    let (u, foe) = (gu(&h, "u"), gu(&h, "foe"));
    call(&mut h, "kill_foe");
    assert!(h.game().unit(foe).dead);
    call(&mut h, "use");
    assert!(accepted(&h));
    assert!(!h.game().unit(foe).dead, "the corpse came back");
    assert_eq!(h.game().unit(foe).owner, h.game().unit(u).owner);
}

#[test]
fn figurine_subclasses_summon_like_the_base() {
    let mut l = lvl(120.0, 0.0, 1.0);
    l.data[1] = f(0.0);
    let rows = [row(b"tfd1", b"AIfd", b"AIfd", l)];
    let alias = c(b"AIfd");
    let mut h = started(&rows, c(b"tfd1"), &|d| {
        d.unit_codes.insert(alias, [FIG_UNIT, 0]);
    });
    let u = gu(&h, "u");
    call(&mut h, "use");
    assert!(accepted(&h));
    let g = h.game();
    let owner = g.unit(u).owner;
    let n = g.owned_units(owner).into_iter().filter(|&k| g.unit(k).type_id == FIG_UNIT).count();
    assert_eq!(n, 1);
}

#[test]
fn flag_items_are_markers_without_stat_effects() {
    let rows = [row(b"tfl1", b"AIfm", b"AIfm", lvl(0.0, 0.0, 0.0))];
    let h = started(&rows, c(b"tfl1"), &|_| {});
    let u = gu(&h, "u");
    assert!(h.game().unit(u).abilities.contains(c(b"AIfm")));
    assert_eq!(h.game().unit(u).bonuses.armor.to_f32(), 0.0);
}

#[test]
fn flare_gun_reveals_for_its_duration_then_stops() {
    let mut l = lvl(45.0, 1800.0, 1.0);
    l.hero_dur = f(45.0);
    let rows = [row(b"tfa1", b"AIfa", b"AIfa", l)];
    let mut h = started(&rows, c(b"tfa1"), &|_| {});
    call(&mut h, "use_point");
    assert!(accepted(&h));
    for _ in 0..500 {
        h.step();
    }
    assert!(h.misfires().is_empty());
}

#[test]
fn damage_aoe_hurts_only_the_enemies_inside_the_area() {
    let rows = [row(b"tdm1", b"AIdm", b"AIdm", lvl(0.0, 500.0, 25.0))];
    let mut h = started(&rows, c(b"tdm1"), &|_| {});
    let (foe, far) = (gu(&h, "foe"), gu(&h, "foe2"));
    call(&mut h, "use");
    assert!(accepted(&h));
    assert_eq!(h.game().unit(foe).life.to_f32(), 475.0);
    assert_eq!(h.game().unit(far).life.to_f32(), 500.0);
}

#[test]
fn borrowed_spell_classes_cast_from_the_pack() {
    // `AImo` runs the ward effect: the `UnitID` unit appears at the point.
    let mut l = lvl(60.0, 1750.0, 1.0);
    l.unit_id = MINE;
    let rows = [row(b"tmo1", b"AImo", b"AImo", l)];
    let mut h = started(&rows, c(b"tmo1"), &|_| {});
    let u = gu(&h, "u");
    call(&mut h, "use_point");
    assert!(accepted(&h), "the lure is a point cast");
    let g = h.game();
    let owner = g.unit(u).owner;
    assert_eq!(g.owned_units(owner).into_iter().filter(|&k| g.unit(k).type_id == MINE).count(), 1);
    drop(g);
    // `AIsh` runs Feral Spirit: the cast is accepted without a target.
    let mut l = lvl(60.0, 200.0, 0.0);
    l.data[1] = f(2.0);
    l.unit_id = FIG_UNIT;
    let rows = [row(b"tsh1", b"AIsh", b"AIsh", l)];
    let mut h = started(&rows, c(b"tsh1"), &|_| {});
    call(&mut h, "use");
    assert!(accepted(&h), "the troll item casts like Feral Spirit");
}
