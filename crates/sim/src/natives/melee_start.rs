//! Natives of the melee start and the creep camps: `CreateBlightedGoldmine`, `GetWorldBounds`, `ChooseRandomCreep`,
//! `ChooseRandomNPBuilding`, the sleep natives (`UnitAddSleep`, `UnitAddSleepPerm`, `UnitCanSleep`,
//! `UnitCanSleepPerm`, `UnitIsSleeping`, `UnitWakeUp`), the alarm natives (`UnitIgnoreAlarm`,
//! `UnitIgnoreAlarmToggled`) and the guard position natives (`RecycleGuardPosition`, `RemoveGuardPosition`,
//! `RemoveAllGuardPositions`). The spec is `docs/spec/melee-start-natives.md`; labels as in `unit.rs`.
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | `CreateBlightedGoldmine` | 0x6F3CD630 | LIKELY | builds `ngol` for player 15 (no handle), hides it, builds `ugol` for the player, links, handle last |
//! | `GetWorldBounds` | 0x6F3D0D60 | LIKELY | a rect equal to the whole map (corner points), no clamp |
//! | `ChooseRandomCreep` | 0x6F3B3660 | LIKELY | candidates in UnitUI row order, stream 32 |
//! | `ChooseRandomNPBuilding` | 0x6F3B3680 | LIKELY | stream 34 |
//! | `UnitAddSleep` / `UnitCanSleep` | 0x6F3C8450 / 0x6F3C84A0 | LIKELY | ability `ACsp` |
//! | `UnitAddSleepPerm` / `UnitCanSleepPerm` | 0x6F3C84D0 / 0x6F3C8520 | LIKELY | ability `Asla` |
//! | `UnitIsSleeping` | 0x6F3C8550 | LIKELY | bit 7 of the `Asla` flag word |
//! | `UnitWakeUp` | 0x6F3C8590 | UNSURE | calls slot 0xC0 of the `Asla` ability with (1, 0); modelled as clearing bit 7 |
//! | `UnitIgnoreAlarm` / `UnitIgnoreAlarmToggled` | 0x6F3C8640 / 0x6F3C8690 | CONFIRMED | bit 7 of the `Arla` flag word |
//! | `RemoveGuardPosition` / `RecycleGuardPosition` / `RemoveAllGuardPositions` | 0x6F3CB820 / 0x6F3CB800 / 0x6F3CB840 | UNSURE | the guard records belong to the creep AI, which the model does not run; no record exists, so the call changes nothing |

use std::cell::RefCell;
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, RunStatus, Value};
use softfloat::F32;

use super::game::{Game, ObjKey, Object, PickRecord, RectObj};
use super::hashtable::{bool_at, real_at};
use super::player::raw_at;
use super::unit_attr::{NativeEntry, Support};
use crate::abilities::{Ability, FLAG_LISTENER};
use crate::economy::Mine;

/// The neutral gold mine `ngol`.
pub const GOLD_MINE: u32 = 0x6E67_6F6C;
/// The haunted gold mine `ugol`.
pub const HAUNTED_MINE: u32 = 0x7567_6F6C;
/// The ability of the haunted mine that takes over the gold mine (`Abgm`, CONFIRMED: `0x6F197400` returns it).
pub const BLIGHT_MINE_ABILITY: u32 = 0x4162_676D;
/// The sleep ability (`ACsp`, `0x6F1D6180`).
pub const SLEEP_ABILITY: u32 = 0x4143_7370;
/// The permanent sleep ability (`Asla`, `0x6F1D6100`).
pub const SLEEP_PERM_ABILITY: u32 = 0x4173_6C61;
/// The alarm ability (`Arla`, `0x6F0226A0`).
pub const ALARM_ABILITY: u32 = 0x4161_6C72;
/// The player slot that owns the first mine (neutral passive, the `ecx = 0xF` of the builder call).
const NEUTRAL_SLOT: u32 = 15;
/// The RNG stream of the creep pick (`ECX = 0x20` at the draw call of `0x6F29FD90`).
const CREEP_STREAM: usize = 32;
/// The RNG stream of the neutral building pick (`0x22`).
const BUILDING_STREAM: usize = 34;
/// The race enum of creeps and of neutral (`other`) buildings (`0x6FA70FE8`).
const RACE_CREEPS: i32 = 8;
const RACE_OTHER: i32 = 7;

/// The support table of the natives registered by [`register`].
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry { name: "CreateBlightedGoldmine", support: Support::Likely },
    NativeEntry { name: "GetWorldBounds", support: Support::Likely },
    NativeEntry { name: "ChooseRandomCreep", support: Support::Likely },
    NativeEntry { name: "ChooseRandomNPBuilding", support: Support::Likely },
    NativeEntry { name: "UnitAddSleep", support: Support::Likely },
    NativeEntry { name: "UnitAddSleepPerm", support: Support::Likely },
    NativeEntry { name: "UnitCanSleep", support: Support::Likely },
    NativeEntry { name: "UnitCanSleepPerm", support: Support::Likely },
    NativeEntry { name: "UnitIsSleeping", support: Support::Likely },
    NativeEntry { name: "UnitWakeUp", support: Support::Unsure },
    NativeEntry { name: "UnitIgnoreAlarm", support: Support::Confirmed },
    NativeEntry { name: "UnitIgnoreAlarmToggled", support: Support::Confirmed },
    NativeEntry { name: "RecycleGuardPosition", support: Support::Unsure },
    NativeEntry { name: "RemoveGuardPosition", support: Support::Unsure },
    NativeEntry { name: "RemoveAllGuardPositions", support: Support::Unsure },
];

/// The tileset test of the random picks (`0x6F6EB690`): with the map's tileset character `c`, the tilesets text must
/// contain `*` or `c`. Storm ordinal 571 is `strchr` (CONFIRMED against the user's `Storm.dll`: a scan for the
/// character that stops at the terminator). A character of 0 skips the test.
pub fn tileset_accepts(tilesets: &str, map_tileset: u8) -> bool {
    if map_tileset == 0 {
        return true;
    }
    tilesets.bytes().any(|b| b == b'*' || b == map_tileset)
}

/// Whether `rec` is a candidate of the creep walk `0x6F29FD90` for `level` (CONFIRMED rules, 2,000 Unicorn cases in
/// `tools/emu/random_pick_emu.py`).
pub fn creep_candidate(rec: &PickRecord, level: i32, map_tileset: u8) -> bool {
    !rec.code_upper_first
        && (level == -1 || rec.level == level)
        && rec.race == RACE_CREEPS
        && !rec.is_building
        && rec.special == 0
        && tileset_accepts(&rec.tilesets, map_tileset)
}

/// Whether `rec` is a candidate of the neutral building walk `0x6F294740` (CONFIRMED rules).
pub fn building_candidate(rec: &PickRecord, map_tileset: u8) -> bool {
    rec.race == RACE_OTHER
        && rec.is_building
        && rec.hostile_pal == 0
        && rec.special == 0
        && rec.nb_random != 0
        && tileset_accepts(&rec.tilesets, map_tileset)
}

/// The shared draw `0x6F285320`: one step of `stream`, result `high32(r * n)`. `None` when the RNG tables are missing.
fn draw(gm: &mut Game, stream: usize, n: usize) -> Result<usize, RunStatus> {
    let (table, streams) = gm.rng_parts().ok_or(RunStatus::MissingTables)?;
    let r = streams.stream_mut(stream).next_u32(table);
    Ok(((u64::from(r) * n as u64) >> 32) as usize)
}

fn pick(gm: &mut Game, stream: usize, candidates: Vec<u32>) -> Result<i32, RunStatus> {
    if candidates.is_empty() {
        return Ok(0);
    }
    let index = draw(gm, stream, candidates.len())?;
    Ok(candidates[index] as i32)
}

/// `CreateBlightedGoldmine(player, x, y, face)` (CONFIRMED order from the disassembly of `0x6F3CD630`): the player is
/// checked; the facing is converted once (degrees times pi/180, done by the builder here); the neutral mine `ngol` is
/// built for player 15 without a handle and hidden (vtable slot 0x84, the slot `ShowUnit(false)` calls); the haunted
/// mine `ugol` is built for the player; the `Abgm` ability of the new mine takes the gold amount from the hidden
/// mine's `Agld` ability and keeps a reference to it; the handle of the new mine is allocated last. The native paints
/// no blight itself (no call of the blight writer `0x6F193470` in the native or in the `Abgm` link `0x6F1ADF30`);
/// the blight of a haunted mine comes from the unit itself (UNSURE, not modelled). The starting gold of a fresh
/// `ngol` is the `Agld` default, which the model does not load, so the new mine starts at the hidden mine's amount
/// when the host registered one, else at 0 (the melee script calls `SetResourceAmount` right after).
fn create_blighted_goldmine(gm: &mut Game, a: &[Value]) -> u32 {
    let Some(owner) = a
        .first()
        .and_then(|v| v.as_handle())
        .and_then(|h| gm.player_index_of_handle(h))
    else {
        return 0;
    };
    let (x, y, face) = (real_at(a, 1), real_at(a, 2), real_at(a, 3));
    let Some((old, _)) = super::unit::build(gm, NEUTRAL_SLOT, GOLD_MINE, x, y, face, false) else {
        return 0;
    };
    gm.unit_mut(old).hidden = true;
    let Some((new, id)) = super::unit::build(gm, owner, HAUNTED_MINE, x, y, face, true) else {
        return 0;
    };
    let amount = gm.mines.get(&old).map_or(0, |m| m.remaining);
    gm.mines.insert(new, Mine { remaining: amount });
    id
}

/// The unit behind handle argument `i`.
fn unit_arg(gm: &Game, a: &[Value], i: usize) -> Option<ObjKey> {
    gm.unit_of_handle(a.get(i).and_then(|v| v.as_handle()).unwrap_or(0))
}

/// `UnitAddSleep` / `UnitAddSleepPerm`: `add` true attaches the ability when the unit lacks it, `add` false removes it
/// when present (CONFIRMED structure). The original's two gates (a unit flag test `0x6F26DE50` and a data table test
/// on the definition) are not modelled (UNSURE). A permanent sleeper starts asleep (UNSURE: the initial state of
/// bit 7 is set by the ability class, which was not run).
fn add_sleep(gm: &mut Game, a: &[Value], code: u32) {
    let Some(k) = unit_arg(gm, a, 0) else { return };
    let add = bool_at(a, 1);
    let has = gm.unit(k).abilities.contains(code);
    if add && !has {
        let def = gm.ability_types().get(code).copied();
        let mut ability = Ability::new(code, def);
        if code == SLEEP_PERM_ABILITY {
            ability.flags |= FLAG_LISTENER;
        }
        gm.unit_mut(k).abilities.attach(ability);
    } else if !add && has {
        gm.unit_mut(k).abilities.remove(code);
    }
}

/// Registers the natives on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("CreateBlightedGoldmine", move |_, a| {
        let id = create_blighted_goldmine(&mut g.borrow_mut(), a);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("GetWorldBounds", move |_, _| {
        let mut gm = g.borrow_mut();
        // The four numbers are a plain copy of the terrain getter (`0x6F73F500`); without a host input the rect is
        // all zeros (UNSURE).
        let b = gm.world_bounds();
        let r = b.map_or(
            RectObj { min_x: F32::ZERO, min_y: F32::ZERO, max_x: F32::ZERO, max_y: F32::ZERO },
            |b| RectObj { min_x: b.min_x, min_y: b.min_y, max_x: b.max_x, max_y: b.max_y },
        );
        let (_, id) = gm.add_object(Object::Rect(r), 0);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("ChooseRandomCreep", move |_, a| {
        let mut gm = g.borrow_mut();
        let level = raw_at(a, 0);
        let tileset = gm.map_tileset();
        let candidates: Vec<u32> = gm
            .unit_types()
            .pick_records()
            .iter()
            .filter(|(_, r)| creep_candidate(r, level, tileset))
            .map(|(id, _)| *id)
            .collect();
        match pick(&mut gm, CREEP_STREAM, candidates) {
            Ok(v) => NativeResult::Return(Value::Integer(v)),
            Err(s) => NativeResult::Fail(s),
        }
    });

    let g = Rc::clone(game);
    reg.register("ChooseRandomNPBuilding", move |_, _| {
        let mut gm = g.borrow_mut();
        let tileset = gm.map_tileset();
        let candidates: Vec<u32> = gm
            .unit_types()
            .pick_records()
            .iter()
            .filter(|(_, r)| building_candidate(r, tileset))
            .map(|(id, _)| *id)
            .collect();
        match pick(&mut gm, BUILDING_STREAM, candidates) {
            Ok(v) => NativeResult::Return(Value::Integer(v)),
            Err(s) => NativeResult::Fail(s),
        }
    });

    let g = Rc::clone(game);
    reg.register("UnitAddSleep", move |_, a| {
        add_sleep(&mut g.borrow_mut(), a, SLEEP_ABILITY);
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("UnitAddSleepPerm", move |_, a| {
        add_sleep(&mut g.borrow_mut(), a, SLEEP_PERM_ABILITY);
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("UnitCanSleep", move |_, a| {
        let gm = g.borrow();
        let v = unit_arg(&gm, a, 0).is_some_and(|k| gm.unit(k).abilities.contains(SLEEP_ABILITY));
        NativeResult::Return(Value::Boolean(v))
    });
    let g = Rc::clone(game);
    reg.register("UnitCanSleepPerm", move |_, a| {
        let gm = g.borrow();
        let v = unit_arg(&gm, a, 0)
            .is_some_and(|k| gm.unit(k).abilities.contains(SLEEP_PERM_ABILITY));
        NativeResult::Return(Value::Boolean(v))
    });
    let g = Rc::clone(game);
    reg.register("UnitIsSleeping", move |_, a| {
        let gm = g.borrow();
        let v = unit_arg(&gm, a, 0)
            .and_then(|k| gm.unit(k).abilities.get(SLEEP_PERM_ABILITY))
            .is_some_and(|ab| ab.has_flag(FLAG_LISTENER));
        NativeResult::Return(Value::Boolean(v))
    });
    let g = Rc::clone(game);
    reg.register("UnitWakeUp", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = unit_arg(&gm, a, 0) {
            if let Some(ab) = gm.unit_mut(k).abilities.get_mut(SLEEP_PERM_ABILITY) {
                ab.flags &= !FLAG_LISTENER;
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitIgnoreAlarm", move |_, a| {
        let mut gm = g.borrow_mut();
        let flag = bool_at(a, 1);
        let done = match unit_arg(&gm, a, 0) {
            Some(k) => match gm.unit_mut(k).abilities.get_mut(ALARM_ABILITY) {
                Some(ab) => {
                    if flag {
                        ab.flags |= FLAG_LISTENER;
                    } else {
                        ab.flags &= !FLAG_LISTENER;
                    }
                    true
                }
                None => false,
            },
            None => false,
        };
        NativeResult::Return(Value::Boolean(done))
    });
    let g = Rc::clone(game);
    reg.register("UnitIgnoreAlarmToggled", move |_, a| {
        let gm = g.borrow();
        let v = unit_arg(&gm, a, 0)
            .and_then(|k| gm.unit(k).abilities.get(ALARM_ABILITY))
            .is_some_and(|ab| ab.has_flag(FLAG_LISTENER));
        NativeResult::Return(Value::Boolean(v))
    });

    // Guard positions: the records belong to the creep AI, which the model does not run, so there is nothing to
    // remove (see the module notes).
    for name in ["RemoveGuardPosition", "RecycleGuardPosition", "RemoveAllGuardPositions"] {
        reg.register(name, |_, _| NativeResult::Return(Value::Null));
    }
}
