//! Lane E natives: per-player selection, player removal and crippling, hero data cache, player score, timed
//! unit turns, shared vision, the string/name lookups, the save/load/level requests and the presentation-only
//! unit natives (animation, colour, indicator).
//!
//! Addresses are the 1.26a `Game.dll` functions in `docs/spec/natives-1.26a.tsv`; the readings are in
//! `docs/spec/natives-lane-e.md`. Labels: CONFIRMED = read in the decompile and disassembly, LIKELY = strong
//! inference, UNSURE = guessed or not read.
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | SetPlayerUnitsOwner | 0x6F3C12F0 | CONFIRMED | resolves the player, then calls a method whose body is a bare `retn 4`: a no-op |
//! | CripplePlayer | 0x6F3C1310 | LIKELY | player + force resolve; stores the force's player mask (16 bits) at player+0x328 when the flag is true, 0 otherwise; then notifies observers (UI). The simulation never reads the field |
//! | RemovePlayer | 0x6F3C9DC0 | LIKELY | clears the player's selection, and for a playing slot (+0x270 == 1) with game result NEUTRAL (word +0x40 == 3) stores the result; a non-victory result makes the slot state 2. The victory/defeat dialogs and the non-playing branch are UNSURE |
//! | CachePlayerHeroData | 0x6F3C9DE0 | LIKELY | snapshots (type id, level, xp) of the player's heroes sorted by (level*100000+xp)*10 descending, lower handle id first on a tie. The three item/ability terms of the sort key are not decoded (taken as 0, UNSURE) |
//! | GetPlayerScore | 0x6F3C9B70 | LIKELY | 25 counters; the mapping is CONFIRMED, the writers of the economy counters (+0x130..+0x1C0) and kills (+0x2C) are not in the model, so they read what a host stored |
//! | SelectUnit / ClearSelection | 0x6F3C7910 / 0x6F3BBAA0 | LIKELY | act on the LOCAL player's selection (game+0x28); add is refused at 12 units |
//! | SyncSelections | 0x6F3BB740 | CONFIRMED | realises the selection mirror of slots 0..11 (no sim state), then behaves as `TriggerSleepAction(0.0)` |
//! | GroupEnumUnitsSelected | 0x6F3CE590 | LIKELY | in `group.rs`: clears the group, then adds the player's selected units in selection order that pass the filter |
//! | SetUnitFacingTimed | 0x6F3C5B30 | UNSURE | duration > 0.1: stores a turn target and a rate of |delta| / (duration/0.03) per movement tick; otherwise an instant turn |
//! | UnitShareVision | 0x6F3C81C0 | UNSURE | stores the shared-vision flag of (unit, player); the fog model has no unit sources |
//! | GetLocalizedString | 0x6F3BACF0 | LIKELY | TRIGSTR substitution, then a global string table lookup (host table) |
//! | GetObjectName | 0x6F3BB6B0 | LIKELY | name of a raw code from the host table; null when absent or empty |
//! | GetAbilityEffectById / GetAbilitySoundById | 0x6F3BCE40 / 0x6F3BCEA0 | CONFIRMED structure | art list of the ability, index clamped to the last entry; null when the list is empty |
//! | SetUnitAnimation, ...ByIndex, ...WithRarity, SetUnitBlendTime, SetUnitColor, UnitAddIndicator | various | LIKELY | presentation only: logged to the `PresentationLog` |
//! | ChangeLevel / LoadGame / SaveGame | 0x6F3BBC30 / 0x6F3BBE60 / 0x6F3BBEE0 | CONFIRMED structure | recorded as host requests; Load/Save only act in a single-player, non-replay game |
//!
//! `GetLocalizedHotkey` and `TriggerWaitForSound` are registered in `lane_f.rs` (`docs/spec/natives-lane-f.md`).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use jass::vm::{NativeRegistry, NativeResult, Value};
use softfloat::{arith, conv, F32};

use super::game::{Game, HandleKind, ObjKey, Object, Payload};
use super::hashtable::{enum_at, int_at, real_at};
use super::presentation::{PresentationEvent, PropValue};
use super::unit_attr::{NativeEntry, Support};

/// The `PLAYER_GAME_RESULT_NEUTRAL` value a player starts with (word `+0x40`, CONFIRMED by the compare in RemovePlayer).
pub const RESULT_NEUTRAL: u32 = 3;
/// The selection limit (`< 0xC` check of the add routine, CONFIRMED).
pub const SELECTION_LIMIT: usize = 12;
/// `pi` as an f32 (the constant at `0x6FAAE600`).
const PI: F32 = F32::from_bits(0x4049_0FDB);
/// `2 * pi`.
const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);
/// `0.1`, the duration below which the turn is instant (`0x6FAAE4A0`, CONFIRMED value).
const INSTANT_LIMIT: F32 = F32::from_bits(0x3DCC_CCCD);
/// `1/100` unit of the degree conversion: `pi/180` (`0x6FAAE5E8`).
const DEG_TO_RAD: F32 = F32::from_bits(0x3C8E_FA35);
/// The `0.03` period of the movement timer.
const MOVE_PERIOD: F32 = F32::from_bits(crate::lockstep::MOVE_PERIOD_BITS);
/// The 2e-7 epsilon (`0x6FAAE614`; value from the runtime-constant audit, bits rounded from the decimal).
const EPSILON: F32 = F32::from_bits(0x3451_B717);

/// The support table of this module.
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry { name: "SetPlayerUnitsOwner", support: Support::Confirmed },
    NativeEntry { name: "CripplePlayer", support: Support::Likely },
    NativeEntry { name: "RemovePlayer", support: Support::Likely },
    NativeEntry { name: "CachePlayerHeroData", support: Support::Likely },
    NativeEntry { name: "GetPlayerScore", support: Support::Likely },
    NativeEntry { name: "SelectUnit", support: Support::Likely },
    NativeEntry { name: "ClearSelection", support: Support::Likely },
    NativeEntry { name: "SyncSelections", support: Support::Confirmed },
    NativeEntry { name: "GroupEnumUnitsSelected", support: Support::Likely },
    NativeEntry { name: "SetUnitFacingTimed", support: Support::Unsure },
    NativeEntry { name: "UnitShareVision", support: Support::Unsure },
    NativeEntry { name: "GetLocalizedString", support: Support::Likely },
    NativeEntry { name: "GetObjectName", support: Support::Likely },
    NativeEntry { name: "GetAbilityEffectById", support: Support::Likely },
    NativeEntry { name: "GetAbilitySoundById", support: Support::Likely },
    NativeEntry { name: "SetUnitAnimation", support: Support::Likely },
    NativeEntry { name: "SetUnitAnimationByIndex", support: Support::Likely },
    NativeEntry { name: "SetUnitAnimationWithRarity", support: Support::Likely },
    NativeEntry { name: "SetUnitBlendTime", support: Support::Likely },
    NativeEntry { name: "SetUnitColor", support: Support::Likely },
    NativeEntry { name: "UnitAddIndicator", support: Support::Likely },
    NativeEntry { name: "ChangeLevel", support: Support::Likely },
    NativeEntry { name: "LoadGame", support: Support::Likely },
    NativeEntry { name: "SaveGame", support: Support::Likely },
    NativeEntry { name: "GetLocalizedHotkey", support: Support::Likely },
    NativeEntry { name: "TriggerWaitForSound", support: Support::Likely },
];

/// A request for the host: the original leaves the running game or touches files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRequest {
    /// `ChangeLevel(name, doScoreScreen)`: switch the campaign level (`0x6F3A4C70`).
    ChangeLevel { name: String, score_screen: bool },
    /// `LoadGame(name, doScoreScreen)`.
    LoadGame { name: String, score_screen: bool },
    /// `SaveGame(name)`.
    SaveGame { name: String },
}

/// Other recorded effects with no script-visible result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneEvent {
    /// `CripplePlayer`: the mask now stored for the player (0 when cleared).
    Crippled { player: u32, mask: u16 },
    /// `RemovePlayer`: the result stored (or the existing one) and the slot state afterwards.
    PlayerRemoved { player: u32, result: u32, slot_state: u32 },
    /// `UnitShareVision`.
    ShareVision { unit: ObjKey, player: u32, share: bool },
}

/// One cached hero (`CachePlayerHeroData`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeroCacheEntry {
    pub type_id: u32,
    pub level: i32,
    pub xp: i32,
}

/// A running timed turn (`SetUnitFacingTimed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnState {
    /// The facing to reach, radians.
    pub target: F32,
    /// The turn per movement tick (0.03 s), radians, non-negative.
    pub rate: F32,
}

/// Which counter of the score object a score getter reads (the byte offsets of the original's player object).
pub mod stat {
    pub const GOLD_MINED: u16 = 0x130;
    pub const LUMBER_MINED: u16 = 0x140;
    pub const GOLD_UPKEEP_LOST: u16 = 0x150;
    pub const LUMBER_UPKEEP_LOST: u16 = 0x160;
    pub const GOLD_TAX_LOST: u16 = 0x170;
    pub const LUMBER_TAX_LOST: u16 = 0x180;
    pub const GOLD_RECEIVED: u16 = 0x190;
    pub const LUMBER_RECEIVED: u16 = 0x1A0;
    pub const GOLD_GIVEN: u16 = 0x1B0;
    pub const LUMBER_GIVEN: u16 = 0x1C0;
    pub const FOOD_MAX_PRODUCED: u16 = 0x248;
    pub const FOOD_MAX_USED: u16 = 0x24C;
    pub const HEROES_KILLED: u16 = 0x250;
    pub const ITEMS_GAINED: u16 = 0x254;
    pub const MERCENARIES_HIRED: u16 = 0x258;
}

/// The state of this module, held by [`Game`].
#[derive(Debug, Clone, Default)]
pub struct LaneE {
    /// The selection of each player in selection order. Both the replay select commands and the local-player
    /// natives edit it.
    pub selections: BTreeMap<u8, Vec<ObjKey>>,
    /// Crippled masks per player index.
    pub crippled: BTreeMap<u32, u16>,
    /// Game result per player index (absent = [`RESULT_NEUTRAL`]).
    pub game_result: BTreeMap<u32, u32>,
    /// Cached hero data per player index.
    pub hero_cache: BTreeMap<u32, Vec<HeroCacheEntry>>,
    /// Score counters keyed by (player, byte offset in the original's player object), see [`stat`].
    pub stats: BTreeMap<(u32, u16), u32>,
    /// The `+0x2C` count of the per-type counter records (the kill side of the unit score), keyed by (player, type).
    pub type_kills: BTreeMap<(u32, u32), i32>,
    /// Score weight of a unit type (the sum of `0x6F29B800` and `0x6F29B830`); a missing type counts as 1 (UNSURE).
    pub type_weight: BTreeMap<u32, i32>,
    /// The types counted as tech of each race, for `PLAYER_SCORE_TECH_PERCENT`.
    pub tech_types: BTreeMap<u32, BTreeSet<u32>>,
    /// Map strings from `war3map.wts`, by number.
    pub map_strings: BTreeMap<u32, String>,
    /// The global string table (key lowercased), loaded by the host from the game's text files.
    pub localized: BTreeMap<String, String>,
    /// Object names by raw code, supplied by the host.
    pub object_names: BTreeMap<u32, String>,
    /// Ability art lists: (ability raw code, effect kind 0..6) -> values in order (effectArt, targetArt, casterArt,
    /// specialArt, areaEffectArt, missileArt, lightningEffect).
    pub ability_art: BTreeMap<(u32, u32), Vec<String>>,
    /// Ability sound lists: (ability raw code, 0 effect / 1 looping) -> values in order.
    pub ability_sound: BTreeMap<(u32, u32), Vec<String>>,
    /// Running timed turns.
    pub turns: BTreeMap<ObjKey, TurnState>,
    /// Shared vision flags per (unit, player).
    pub shared_vision: BTreeMap<(ObjKey, u32), bool>,
    /// Recorded effects.
    pub events: Vec<LaneEvent>,
    /// Requests for the host.
    pub requests: Vec<HostRequest>,
    /// Whether the game is single-player and not a replay (gate of `LoadGame` and `SaveGame`, `0x6F3B28C0`).
    pub single_player: bool,
    /// `+996` of the game object: a save is in progress. Set by `SaveGame`, cleared by [`LaneE::finish_save`].
    pub save_pending: bool,
}

impl LaneE {
    /// The game result of a player.
    pub fn result_of(&self, player: u32) -> u32 {
        self.game_result.get(&player).copied().unwrap_or(RESULT_NEUTRAL)
    }

    /// Adds `amount` to a score counter (the host calls this where the original increments them).
    pub fn add_stat(&mut self, player: u32, offset: u16, amount: u32) {
        let v = self.stats.entry((player, offset)).or_insert(0);
        *v = v.wrapping_add(amount);
    }

    fn stat(&self, player: u32, offset: u16) -> u32 {
        self.stats.get(&(player, offset)).copied().unwrap_or(0)
    }

    /// Clears the "save in progress" flag (the save finished).
    pub fn finish_save(&mut self) {
        self.save_pending = false;
    }

    /// The text behind a string argument after `TRIGSTR_nnn` substitution (`0x6F3BAA40`, CONFIRMED structure).
    /// `None` is the null string (null or empty input). The prefix compare is taken as case-sensitive (UNSURE: the
    /// helper `0x6F6EB68A` is a Storm/CRT compare), the number is `atoi`, an unknown number gives "".
    pub fn resolve_text(&self, s: &str) -> Option<String> {
        if s.is_empty() {
            return None;
        }
        match s.strip_prefix("TRIGSTR_") {
            Some(rest) => Some(self.map_strings.get(&(atoi(rest) as u32)).cloned().unwrap_or_default()),
            None => Some(s.to_string()),
        }
    }
}

/// `atoi` on the text: optional leading spaces and sign, then digits; anything else stops it.
fn atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut v: i32 = 0;
    for b in t.bytes() {
        if !b.is_ascii_digit() {
            break;
        }
        v = v.wrapping_mul(10).wrapping_add((b - b'0') as i32);
    }
    if neg {
        v.wrapping_neg()
    } else {
        v
    }
}

/// Parses a `war3map.wts` file: blocks `STRING n`, an optional comment, `{`, the text lines, `}`. Line ends are
/// normalised to `\n`; the text keeps interior line breaks (LIKELY: the original joins lines as read).
pub fn parse_wts(text: &str) -> BTreeMap<u32, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        let Some(num) = line.strip_prefix("STRING") else { continue };
        let Ok(n) = num.trim().parse::<u32>() else { continue };
        // Skip comments up to the opening brace.
        let mut opened = false;
        for l in lines.by_ref() {
            let l = l.trim();
            if l == "{" {
                opened = true;
                break;
            }
            if !l.starts_with("//") && !l.is_empty() {
                break;
            }
        }
        if !opened {
            continue;
        }
        let mut body: Vec<&str> = Vec::new();
        for l in lines.by_ref() {
            if l.trim() == "}" {
                break;
            }
            body.push(l.trim_end_matches('\r'));
        }
        out.entry(n).or_insert_with(|| body.join("\n"));
    }
    out
}

/// Wraps an angle difference into (-pi, pi] with the soft float.
fn wrap_angle(mut d: F32) -> F32 {
    // Bounded loops: inputs come from facings, which stay near [0, 2pi); 8 rounds cover any sane value.
    for _ in 0..8 {
        if arith::gt(d, PI) {
            d = arith::sub(d, TWO_PI);
        } else if arith::le(d, arith::neg(PI)) {
            d = arith::add(d, TWO_PI);
        } else {
            break;
        }
    }
    d
}

fn abs(x: F32) -> F32 {
    F32::from_bits(x.to_bits() & 0x7FFF_FFFF)
}

impl Game {
    /// Advances the timed turns by one movement tick (called by the movement walk).
    pub(crate) fn tick_turns(&mut self) {
        let keys: Vec<ObjKey> = self.lane.turns.keys().copied().collect();
        for k in keys {
            let ts = self.lane.turns[&k];
            let live = matches!(self.object(k), Object::Unit(u) if !(u.dead || u.removed));
            if !live {
                self.lane.turns.remove(&k);
                continue;
            }
            let facing = self.unit(k).facing;
            let delta = wrap_angle(arith::sub(ts.target, facing));
            if arith::le(abs(delta), ts.rate) {
                self.unit_mut(k).facing = ts.target;
                self.lane.turns.remove(&k);
            } else {
                let step = if delta.to_bits() >> 31 == 1 { arith::neg(ts.rate) } else { ts.rate };
                self.unit_mut(k).facing = arith::add(facing, step);
            }
        }
    }

    /// Clears the selection of `player` (the "async clear selection" of `0x6F424EF0`).
    pub fn clear_selection_of(&mut self, player: u32) {
        self.lane.selections.remove(&(player as u8));
    }
}

fn handle_arg(a: &[Value], i: usize) -> u32 {
    a.get(i).and_then(|v| v.as_handle()).unwrap_or(0)
}

fn bool_arg(a: &[Value], i: usize) -> bool {
    match a.get(i) {
        Some(Value::Boolean(b)) => *b,
        Some(Value::Integer(n)) => *n != 0,
        _ => false,
    }
}

fn str_arg(ctx: &jass::vm::NativeCtx<'_>, a: &[Value], i: usize) -> String {
    match a.get(i) {
        Some(Value::String(id)) => ctx.string(*id).to_string(),
        _ => String::new(),
    }
}

/// The rawcode-or-integer argument as a type id.
fn code_arg(a: &[Value], i: usize) -> u32 {
    int_at(a, i) as u32
}

fn log_unit(gm: &mut Game, a: &[Value], name: &'static str, extra: Vec<PropValue>) {
    let id = handle_arg(a, 0);
    if gm.unit_of_handle(id).is_some() {
        let mut args = vec![PropValue::Handle(id)];
        args.extend(extra);
        gm.presentation_mut().log.push(PresentationEvent::Command { name, args });
    }
}

/// The score value for `which` (`0x6F3C9B70`); 0 for an unknown score.
pub fn player_score(gm: &Game, player: u32, which: u32) -> i32 {
    let l = &gm.lane;
    let s = |off: u16| l.stat(player, off);
    let tenth = |off: u16| s(off) / 10;
    let counts = &gm.player(player).counts;
    let weight = |t: u32| l.type_weight.get(&t).copied().unwrap_or(1);
    let kills = |t: u32| l.type_kills.get(&(player, t)).copied().unwrap_or(0);
    let sum_class = |class: u32, f: &dyn Fn(u32, &crate::unit_counts::TypeRecord) -> i32| -> i32 {
        counts
            .records()
            .filter(|(_, r)| r.class & class != 0)
            .fold(0i32, |acc, (t, r)| acc.wrapping_add(f(t, r)))
    };
    let hero_total = || -> i32 {
        let xp: i32 = heroes_of(gm, player).iter().map(|&k| gm.unit(k).hero.as_ref().map_or(0, |h| h.xp)).sum();
        xp.wrapping_mul(10)
    };
    let unit_total = || -> i32 {
        sum_class(0xC, &|t, r| weight(t).wrapping_mul(r.completed_flag_clear.wrapping_add(kills(t))))
    };
    let resource_total = || (tenth(stat::GOLD_MINED) + tenth(stat::LUMBER_MINED)) as i32;
    match which {
        0 => sum_class(8, &|_, r| r.completed_flag_clear),
        1 => sum_class(8, &|t, _| kills(t)),
        2 => sum_class(4, &|t, r| if weight(t) != 0 { r.completed_flag_clear } else { 0 }),
        3 => sum_class(4, &|t, _| kills(t)),
        4 => tech_percent(gm, player),
        5 => s(stat::FOOD_MAX_PRODUCED) as i32,
        6 => s(stat::FOOD_MAX_USED) as i32,
        7 => s(stat::HEROES_KILLED) as i32,
        8 => s(stat::ITEMS_GAINED) as i32,
        9 => s(stat::MERCENARIES_HIRED) as i32,
        10 => (tenth(stat::GOLD_MINED) + tenth(stat::GOLD_UPKEEP_LOST)) as i32,
        11 => tenth(stat::GOLD_MINED) as i32,
        12 => tenth(stat::GOLD_UPKEEP_LOST) as i32,
        13 => tenth(stat::GOLD_TAX_LOST) as i32,
        14 => s(stat::GOLD_GIVEN) as i32,
        15 => s(stat::GOLD_RECEIVED) as i32,
        16 => tenth(stat::LUMBER_MINED) as i32,
        17 => tenth(stat::LUMBER_UPKEEP_LOST) as i32,
        18 => tenth(stat::LUMBER_TAX_LOST) as i32,
        19 => s(stat::LUMBER_GIVEN) as i32,
        20 => s(stat::LUMBER_RECEIVED) as i32,
        21 => unit_total(),
        22 => hero_total(),
        23 => resource_total(),
        24 => hero_total().wrapping_add(resource_total()).wrapping_add(unit_total()),
        _ => 0,
    }
}

/// `PLAYER_SCORE_TECH_PERCENT` (`0x6F3F66A0`): 100.0 when the race has no tech types, otherwise
/// `f2i(100.0 * I2R(done) / I2R(total))`. Needs the soft-float division tables.
fn tech_percent(gm: &Game, player: u32) -> i32 {
    let race = gm.player(player).race;
    let types = gm.lane.tech_types.get(&race);
    let total = types.map_or(0, |t| t.len() as i32);
    if total == 0 {
        return 100;
    }
    let Some(tables) = gm.order_tables() else { return 0 };
    let done: i32 = gm
        .player(player)
        .counts
        .records()
        .filter(|(t, _)| types.is_some_and(|ts| ts.contains(t)))
        .fold(0i32, |acc, (_, r)| acc.wrapping_add(r.completed_flag_clear));
    let ratio = arith::div(conv::i2r(done), conv::i2r(total), tables);
    conv::f2i(arith::mul(F32::from_bits(0x42C8_0000), ratio))
}

/// The player's live hero units sorted as `0x6F41B620` does: type id high byte 'A'..='Z', not removed, not an
/// illusion; descending by (level*100000 + xp)*10, ties by lower handle id (UNSURE: the three extra terms of the key).
fn heroes_of(gm: &Game, player: u32) -> Vec<ObjKey> {
    let mut list: Vec<(i64, u32, ObjKey)> = gm
        .unit_objects()
        .filter(|(_, u)| u.owner == player && !u.removed && !u.illusion)
        .filter(|(_, u)| ((u.type_id >> 24) as u8).is_ascii_uppercase())
        .filter_map(|(k, u)| {
            let h = u.hero.as_ref()?;
            let key = (h.level as i64 * 100_000 + h.xp as i64) * 10;
            Some((key, gm.handle_of(k), k))
        })
        .collect();
    list.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    list.into_iter().map(|(_, _, k)| k).collect()
}

/// Registers the natives that sleep the calling thread (main VM only: they queue the wake-up on the main timers).
pub fn register_sleeping(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("SyncSelections", move |ctx, _| {
        // The mirror realise of slots 0..11 changes no simulation state (CONFIRMED); the native then behaves as
        // TriggerSleepAction(0.0): wait state 1 with the real 0.0 (0x6F44B2F0).
        let seconds = crate::timer::sleep_seconds(F32::ZERO);
        let wake = ctx.wake_time(seconds);
        g.borrow_mut().timers.start_at(wake.to_f32(), Payload::Sleep(ctx.thread()));
        NativeResult::Sleep(seconds)
    });
}

/// Registers the natives of this module on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("SetPlayerUnitsOwner", move |_, _| {
        // CONFIRMED: the player resolves, then a method that is a bare `retn 4` runs.
        let _ = &g;
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("CripplePlayer", move |_, a| {
        let mut gm = g.borrow_mut();
        let player = gm.player_index_of_handle(handle_arg(a, 0));
        let force = gm.handle_of_kind(handle_arg(a, 1), HandleKind::Force);
        if let (Some(p), Some(f)) = (player, force) {
            let mask = if bool_arg(a, 2) { (gm.force(f).members & 0xFFFF) as u16 } else { 0 };
            gm.lane.crippled.insert(p, mask);
            gm.lane.events.push(LaneEvent::Crippled { player: p, mask });
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("RemovePlayer", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(p) = gm.player_index_of_handle(handle_arg(a, 0)) else {
            return NativeResult::Return(Value::Null);
        };
        let result = enum_at(a, 1) as u32;
        gm.clear_selection_of(p);
        if gm.player(p).slot_state == 1 && gm.lane.result_of(p) == RESULT_NEUTRAL {
            gm.lane.game_result.insert(p, result);
            if result != 0 {
                if let Some(pl) = gm.player_mut(p) {
                    pl.slot_state = 2;
                }
            }
        }
        let (r, st) = (gm.lane.result_of(p), gm.player(p).slot_state);
        gm.lane.events.push(LaneEvent::PlayerRemoved { player: p, result: r, slot_state: st });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("CachePlayerHeroData", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(p) = gm.player_index_of_handle(handle_arg(a, 0)) {
            let list: Vec<HeroCacheEntry> = heroes_of(&gm, p)
                .into_iter()
                .map(|k| {
                    let u = gm.unit(k);
                    let h = u.hero.as_ref().expect("heroes_of keeps heroes");
                    HeroCacheEntry { type_id: u.type_id, level: h.level, xp: h.xp }
                })
                .collect();
            gm.lane.hero_cache.insert(p, list);
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("GetPlayerScore", move |_, a| {
        let gm = g.borrow();
        let v = match gm.player_index_of_handle(handle_arg(a, 0)) {
            Some(p) => player_score(&gm, p, enum_at(a, 1) as u32),
            None => 0,
        };
        NativeResult::Return(Value::Integer(v))
    });

    let g = Rc::clone(game);
    reg.register("SelectUnit", move |_, a| {
        let mut gm = g.borrow_mut();
        if let Some(k) = gm.unit_of_handle(handle_arg(a, 0)) {
            let local = gm.local_player() as u8;
            let sel = gm.lane.selections.entry(local).or_default();
            if bool_arg(a, 1) {
                // UNSURE: the original also requires bit 2 of the unit's flag word at +0x20 (not modelled).
                if !sel.contains(&k) && sel.len() < SELECTION_LIMIT {
                    sel.push(k);
                }
            } else {
                sel.retain(|&x| x != k);
            }
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("ClearSelection", move |_, _| {
        let mut gm = g.borrow_mut();
        let local = gm.local_player();
        gm.clear_selection_of(local);
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("SetUnitFacingTimed", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(k) = gm.unit_of_handle(handle_arg(a, 0)) else {
            return NativeResult::Return(Value::Null);
        };
        let target = arith::mul(real_at(a, 1), DEG_TO_RAD);
        let duration = real_at(a, 2);
        if !arith::lt(INSTANT_LIMIT, duration) {
            gm.unit_mut(k).facing = target;
            return NativeResult::Return(Value::Null);
        }
        let Some(tables) = gm.order_tables() else {
            // Without the soft-float tables no division is possible: nothing changes (like movement).
            return NativeResult::Return(Value::Null);
        };
        let steps = arith::div(duration, MOVE_PERIOD, tables);
        if arith::lt(abs(steps), EPSILON) {
            return NativeResult::Return(Value::Null);
        }
        let delta = wrap_angle(arith::sub(target, gm.unit(k).facing));
        let rate = abs(arith::div(delta, steps, tables));
        if arith::lt(rate, EPSILON) {
            return NativeResult::Return(Value::Null);
        }
        gm.lane.turns.insert(k, TurnState { target, rate });
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitShareVision", move |_, a| {
        let mut gm = g.borrow_mut();
        let unit = gm.unit_of_handle(handle_arg(a, 0));
        let player = gm.player_index_of_handle(handle_arg(a, 1));
        if let (Some(u), Some(p)) = (unit, player) {
            let share = bool_arg(a, 2);
            gm.lane.shared_vision.insert((u, p), share);
            gm.lane.events.push(LaneEvent::ShareVision { unit: u, player: p, share });
        }
        NativeResult::Return(Value::Null)
    });

    // --- strings and names -------------------------------------------------------------------------------

    let g = Rc::clone(game);
    reg.register("GetLocalizedString", move |ctx, a| {
        let name = str_arg(ctx, a, 0);
        let text = {
            let gm = g.borrow();
            gm.lane.resolve_text(&name).map(|t| {
                gm.lane.localized.get(&t.to_ascii_lowercase()).cloned().unwrap_or(t)
            })
        };
        NativeResult::Return(Value::String(text.map_or(0, |t| ctx.intern(&t))))
    });

    let g = Rc::clone(game);
    reg.register("GetObjectName", move |ctx, a| {
        let name = g.borrow().lane.object_names.get(&code_arg(a, 0)).cloned().filter(|n| !n.is_empty());
        NativeResult::Return(Value::String(name.map_or(0, |n| ctx.intern(&n))))
    });

    let g = Rc::clone(game);
    reg.register("GetAbilityEffectById", move |ctx, a| {
        let kind = enum_at(a, 1) as u32;
        let kind = if (1..=6).contains(&kind) { kind } else { 0 };
        let v = art_pick(&g.borrow().lane.ability_art, (code_arg(a, 0), kind), int_at(a, 2) as u32);
        NativeResult::Return(Value::String(v.map_or(0, |s| ctx.intern(&s))))
    });

    let g = Rc::clone(game);
    reg.register("GetAbilitySoundById", move |ctx, a| {
        let kind = u32::from(enum_at(a, 1) == 1);
        let v = art_pick(&g.borrow().lane.ability_sound, (code_arg(a, 0), kind), 0);
        NativeResult::Return(Value::String(v.map_or(0, |s| ctx.intern(&s))))
    });

    // StringLength / StringCase with the TRIGSTR substitution of the string fetch helper (0x6F3BAA40).
    let g = Rc::clone(game);
    reg.register("StringLength", move |ctx, a| {
        let s = str_arg(ctx, a, 0);
        let n = g.borrow().lane.resolve_text(&s).map_or(0, |t| t.len() as i32);
        NativeResult::Return(Value::Integer(n))
    });
    let g = Rc::clone(game);
    reg.register("StringCase", move |ctx, a| {
        let s = str_arg(ctx, a, 0);
        let upper = bool_arg(a, 1);
        let t = g.borrow().lane.resolve_text(&s);
        let out = t.map(|t| jass::natives::pure::string_case(&t, upper)).unwrap_or_default();
        NativeResult::Return(Value::String(ctx.intern(&out)))
    });

    // --- presentation-only unit natives ------------------------------------------------------------------

    let g = Rc::clone(game);
    reg.register("SetUnitAnimation", move |ctx, a| {
        let name = str_arg(ctx, a, 1);
        log_unit(&mut g.borrow_mut(), a, "SetUnitAnimation", vec![PropValue::Text(name), PropValue::Int(0)]);
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("SetUnitAnimationWithRarity", move |ctx, a| {
        // The rarity flag word is 16 for every rarity but RARITY_RARE (1), which is 32 (CONFIRMED, 0x6F3C61C0).
        let name = str_arg(ctx, a, 1);
        let flags = if enum_at(a, 2) == 1 { 32 } else { 16 };
        log_unit(&mut g.borrow_mut(), a, "SetUnitAnimation", vec![PropValue::Text(name), PropValue::Int(flags)]);
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("SetUnitAnimationByIndex", move |_, a| {
        log_unit(&mut g.borrow_mut(), a, "SetUnitAnimationByIndex", vec![PropValue::Int(int_at(a, 1))]);
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("SetUnitBlendTime", move |_, a| {
        log_unit(&mut g.borrow_mut(), a, "SetUnitBlendTime", vec![PropValue::Real(real_at(a, 1).to_f32())]);
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("SetUnitColor", move |_, a| {
        log_unit(&mut g.borrow_mut(), a, "SetUnitColor", vec![PropValue::Int(enum_at(a, 1))]);
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("UnitAddIndicator", move |_, a| {
        let rgba = (1..=4).map(|i| PropValue::Int(int_at(a, i))).collect();
        log_unit(&mut g.borrow_mut(), a, "UnitAddIndicator", rgba);
        NativeResult::Return(Value::Null)
    });

    // --- level / save / load -----------------------------------------------------------------------------

    let g = Rc::clone(game);
    reg.register("ChangeLevel", move |ctx, a| {
        // CONFIRMED: no mode gate; the raw string (no TRIGSTR substitution) goes to the campaign loader.
        let name = str_arg(ctx, a, 0);
        g.borrow_mut().lane.requests.push(HostRequest::ChangeLevel { name, score_screen: bool_arg(a, 1) });
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("LoadGame", move |ctx, a| {
        let name = str_arg(ctx, a, 0);
        let mut gm = g.borrow_mut();
        // CONFIRMED gate 0x6F3B28C0: single-player and not a replay; a null name does nothing.
        if gm.lane.single_player && !name.is_empty() {
            gm.lane.requests.push(HostRequest::LoadGame { name, score_screen: bool_arg(a, 1) });
        }
        NativeResult::Return(Value::Null)
    });
    let g = Rc::clone(game);
    reg.register("SaveGame", move |ctx, a| {
        let name = str_arg(ctx, a, 0);
        let mut gm = g.borrow_mut();
        let ok_name = !name.is_empty() && !name.contains("..") && !name.contains(':');
        if gm.lane.single_player && ok_name && !gm.lane.save_pending {
            gm.lane.save_pending = true;
            gm.lane.requests.push(HostRequest::SaveGame { name });
        }
        NativeResult::Return(Value::Null)
    });
}

/// The entry `index` of an art list, clamped to the last entry; `None` for a missing or empty list.
fn art_pick(map: &BTreeMap<(u32, u32), Vec<String>>, key: (u32, u32), index: u32) -> Option<String> {
    let list = map.get(&key)?;
    let last = list.len().checked_sub(1)? as u32;
    Some(list[index.min(last) as usize].clone())
}
