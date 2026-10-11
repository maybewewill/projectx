//! Lane F natives: the last eight natives a real melee map references that were unbound: `SetUnitUseFood`,
//! `UnitSuspendDecay`, `CreateCorpse`, `IssueBuildOrder`, `IssueBuildOrderById`, `GetRescuer` and `GetSellingUnit`
//! (the two getters live in `event.rs`), `GetLocalizedHotkey` and `TriggerWaitForSound`.
//!
//! Addresses are the 1.26a `Game.dll` functions in `docs/spec/natives-1.26a.tsv`; the readings are in
//! `docs/spec/natives-lane-f.md`. Labels: CONFIRMED = read in the machine code, LIKELY = strong inference,
//! UNSURE = guessed or not read.
//!
//! | native | address | support | notes |
//! |---|---|---|---|
//! | SetUnitUseFood | 0x6F3C54A0 | CONFIRMED | true: add the type's food used to the owner unless bit `0x200` of the registration word (`+0x64`) is already set, then set `0x300`; false: take it back when `0x200` is set, then clear `0x300` |
//! | UnitSuspendDecay | 0x6F3C81F0 | LIKELY | needs a dead corpse (CONFIRMED gate: word `+0x5C` has `0xC00` and `0x100`; the sim treats a dead, unremoved unit as that); runs the stop-everything step `0x6F2964E0`, and for `false` also posts `0xD015A` and `0xD0159` and drains |
//! | CreateCorpse | 0x6F3C5350 | LIKELY | the `CreateUnit` builder `0x6F24F1F0`, then the teardown `0x6F2A63B0(unit,0,0)`, then `0xD015A` (+`0xD0159`, +`0xD0158`), drain, handle last. That the result is dead (life 0) is UNSURE |
//! | IssueBuildOrderById / IssueBuildOrder | 0x6F3C8BE0 / 0x6F3C8E80 | UNSURE | unit resolves; placement validator `0x6F2A50C0` must give 0 or 0x45; ability acceptance `0x6F279560`; point order with the type id as order id through the dispatcher `0x6F2A4AB0`. The validator body is a host hook here |
//! | GetLocalizedHotkey | 0x6F3B2D80 | CONFIRMED structure | non-empty string, then the first value of that key in the `HotKeys` section of the string tables, parsed as `strtol(base 0)` or a quoted raw code; 0 when missing |
//! | TriggerWaitForSound | 0x6F3C3F20 | CONFIRMED structure | sleep of `max(0, (T - offset) - elapsed)` seconds with `T` = sound length / 1000; a bad handle makes it `TriggerSleepAction(offset)` |
//! | GetSellingUnit / GetRescuer | 0x6F3C2560 / 0x6F3C2390 | CONFIRMED | event response getters in `event.rs` |

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use jass::vm::{clamp_sleep, NativeRegistry, NativeResult, Value};
use softfloat::{arith, conv, F32};

use super::game::{Game, ObjKey, Payload, UnitEvent};
use super::lane_e::stat;
use super::unit_attr::{NativeEntry, Support};
use crate::orders::{Order, OrderClass};

/// The support table of this module. The event getters `GetSellingUnit` and `GetRescuer` are registered in `event.rs`.
/// `SetUnitUseFood` and `UnitSuspendDecay` stay in the table of `unit_attr.rs`, `GetLocalizedHotkey` and
/// `TriggerWaitForSound` in that of `lane_e.rs` (their rows only changed support).
pub const NATIVES: &[NativeEntry] = &[
    NativeEntry { name: "CreateCorpse", support: Support::Likely },
    NativeEntry { name: "IssueBuildOrderById", support: Support::Unsure },
    NativeEntry { name: "IssueBuildOrder", support: Support::Unsure },
    NativeEntry { name: "GetRescuer", support: Support::Confirmed },
    NativeEntry { name: "GetSellingUnit", support: Support::Confirmed },
];

/// Bit `0x200` of the registration word: the type's food used is applied to the owner.
pub const REG_USED_APPLIED: u32 = 0x200;
/// Both food-used bits (`0x100` + `0x200`).
pub const REG_USED_MASK: u32 = 0x300;
/// Bit `0x800` of the registration word: the type's food made is applied to the owner.
pub const REG_MADE_APPLIED: u32 = 0x800;
/// Both food-made bits (`0x400` + `0x800`).
pub const REG_MADE_MASK: u32 = 0xC00;
/// `PLAYER_STATE_RESOURCE_FOOD_USED`: the state `GetPlayerState` answers from the food-used counter.
const STATE_FOOD_USED: usize = 5;

/// The placement validator hook of the build orders (`0x6F2A50C0`): `(game, unit, type id, x, y)` and the original's
/// result code, where `0` and `0x45` let the order go on.
pub type PlacementFn = fn(&Game, ObjKey, u32, F32, F32) -> u32;

/// The result codes of the placement validator that let a build order go on (CONFIRMED compare in `0x6F3C8BE0`).
pub const PLACEMENT_OK: u32 = 0;
/// The result `0x45`: the placement is fine once the units in the way have moved.
pub const PLACEMENT_BLOCKERS_MOVE: u32 = 0x45;
/// `D` (68): the failure code the validator uses for an unusable placement.
pub const PLACEMENT_FAILED: u32 = 68;

/// The two food counters of a player (the original's sub-objects at `+0x90` used and `+0x80` made).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerFood {
    pub used: i32,
    pub made: i32,
}

/// What the model knows of a corpse made by `CreateCorpse` (or any dead unit that `UnitSuspendDecay` touched).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CorpseState {
    /// The unit's pathing-disabled counter (object `+0x198`, bumped by the teardown `0x6F2A3CC0`).
    pub pathing_off: u32,
    /// The last `UnitSuspendDecay` argument.
    pub decay_suspended: bool,
}

/// Effects with no script-visible result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneFEvent {
    /// `UnitSuspendDecay` ran the stop step; `suspend` is the argument.
    DecayStop { unit: ObjKey, suspend: bool },
    /// A build order was accepted for `unit`: the type to place and the point.
    BuildOrder { unit: ObjKey, type_id: u32, x: F32, y: F32 },
}

/// The state of this module, held by [`Game`].
#[derive(Clone, Default)]
pub struct LaneF {
    /// Food counters per player slot (see [`PlayerFood`]).
    pub food: BTreeMap<u32, PlayerFood>,
    /// The `HotKeys` section of the string tables: lower-cased key to its values (comma separated list). The host
    /// fills it from the game's text files.
    pub hotkeys: BTreeMap<String, Vec<String>>,
    /// Corpse state per unit.
    pub corpses: BTreeMap<ObjKey, CorpseState>,
    /// The placement validator of the build orders. `None`: a registered type is accepted.
    pub placement: Option<PlacementFn>,
    /// Recorded effects, oldest first.
    pub events: Vec<LaneFEvent>,
}

impl std::fmt::Debug for LaneF {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaneF")
            .field("food", &self.food)
            .field("hotkeys", &self.hotkeys.len())
            .field("corpses", &self.corpses)
            .field("placement", &self.placement.is_some())
            .field("events", &self.events)
            .finish()
    }
}

impl Game {
    /// `0x6F40F5E0(n)`: the owner's food used changes by `n`; the peak word `+0x24C` is raised when the new value is
    /// above it (an unsigned compare, CONFIRMED). The state `GetPlayerState` reads for food used follows.
    pub(crate) fn food_add_used(&mut self, p: u32, n: i32) {
        let e = self.lane_f.food.entry(p).or_default();
        e.used = e.used.wrapping_add(n);
        let used = e.used;
        if let Some(pl) = self.player_mut(p) {
            pl.states[STATE_FOOD_USED] = used;
        }
        let peak = self.lane.stats.entry((p, stat::FOOD_MAX_USED)).or_insert(0);
        if used as u32 > *peak {
            *peak = used as u32;
        }
    }

    /// `0x6F03F970(n)`: the owner's food made changes by `n`; the peak word `+0x248` is raised the same way.
    pub(crate) fn food_add_made(&mut self, p: u32, n: i32) {
        let e = self.lane_f.food.entry(p).or_default();
        e.made = e.made.wrapping_add(n);
        let made = e.made;
        let peak = self.lane.stats.entry((p, stat::FOOD_MAX_PRODUCED)).or_insert(0);
        if made as u32 > *peak {
            *peak = made as u32;
        }
    }

    /// The type's food used and made (`0x6F29BAF0` / `0x6F29BB20`: the type record's `+0x5C` / `+0x60`; 0 for a type
    /// that is not registered).
    fn type_food(&self, type_id: u32) -> (i32, i32) {
        self.unit_types()
            .get(type_id)
            .map_or((0, 0), |t| (t.food_used, t.food_made))
    }

    /// The food part of the creation tail `0x6F2A64A0` with flags `0x502`: flag `0x100` adds the food used and sets
    /// `0x300`, then flag `0x400` adds the food made and sets `0xC00` (CONFIRMED order).
    pub(crate) fn food_apply_creation(&mut self, k: ObjKey) {
        let (owner, ty) = {
            let u = self.unit(k);
            (u.owner, u.type_id)
        };
        let (used, made) = self.type_food(ty);
        self.food_add_used(owner, used);
        self.unit_mut(k).food_reg |= REG_USED_MASK;
        self.food_add_made(owner, made);
        self.unit_mut(k).food_reg |= REG_MADE_MASK;
    }

    /// The food part of the unit teardown `0x6F29D320`: the food made comes off first (bit `0x800`, clearing
    /// `0xC00`), then the food used (bit `0x200`, clearing `0x300`).
    pub(crate) fn food_release(&mut self, k: ObjKey) {
        let (owner, ty, reg) = {
            let u = self.unit(k);
            (u.owner, u.type_id, u.food_reg)
        };
        let (used, made) = self.type_food(ty);
        let mut reg = reg;
        if reg & REG_MADE_APPLIED != 0 {
            self.food_add_made(owner, made.wrapping_neg());
            reg &= !REG_MADE_MASK;
        }
        if reg & REG_USED_APPLIED != 0 {
            self.food_add_used(owner, used.wrapping_neg());
            reg &= !REG_USED_MASK;
        }
        self.unit_mut(k).food_reg = reg;
    }

    /// An owner change moves the applied food from the old owner to the new one (the two `0x6F40F5E0` calls of
    /// `0x6F2A4BE0`; the order inside it is not read, LIKELY).
    pub(crate) fn food_move_owner(&mut self, k: ObjKey, to: u32) {
        let (from, ty, reg) = {
            let u = self.unit(k);
            (u.owner, u.type_id, u.food_reg)
        };
        if from == to {
            return;
        }
        let (used, made) = self.type_food(ty);
        if reg & REG_USED_APPLIED != 0 {
            self.food_add_used(from, used.wrapping_neg());
            self.food_add_used(to, used);
        }
        if reg & REG_MADE_APPLIED != 0 {
            self.food_add_made(from, made.wrapping_neg());
            self.food_add_made(to, made);
        }
    }

    /// The food used the sim holds for player slot `p` (the counter at `+0x90`).
    pub fn food_used_of(&self, p: u32) -> i32 {
        self.lane_f.food.get(&p).map_or(0, |f| f.used)
    }

    /// The food made the sim holds for player slot `p` (the counter at `+0x80`).
    pub fn food_made_of(&self, p: u32) -> i32 {
        self.lane_f.food.get(&p).map_or(0, |f| f.made)
    }

    /// Sets the placement validator of the build orders.
    pub fn set_placement_validator(&mut self, f: Option<PlacementFn>) {
        self.lane_f.placement = f;
    }

    /// Fills one key of the `HotKeys` section (`GetLocalizedHotkey`); the key is matched case-insensitively.
    pub fn set_hotkey(&mut self, key: &str, values: &[&str]) {
        self.lane_f.hotkeys.insert(
            key.to_ascii_lowercase(),
            values.iter().map(|s| (*s).to_string()).collect(),
        );
    }
}

/// The text-to-integer rule of the hotkey lookup (`0x6F4CE050`): a text that starts with a quote is a raw code of up
/// to four characters (stops at the closing quote or the end), anything else goes through `strtol(text, 0, 0)`.
pub fn parse_hotkey_text(text: &str) -> i32 {
    let b = text.as_bytes();
    if b.first() == Some(&b'\'') {
        let mut r: u32 = 0;
        for &c in b[1..].iter().take(4) {
            if c == 0 || c == b'\'' {
                break;
            }
            r = (r << 8) | u32::from(c);
        }
        return r as i32;
    }
    c_strtol(b)
}

/// The C `strtol(s, NULL, 0)` of a 32-bit `long`: leading white space, an optional sign, `0x` hex or leading-zero
/// octal or decimal, saturating at the `long` limits, 0 when no digits.
pub fn c_strtol(s: &[u8]) -> i32 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base: u64 = 10;
    if i < s.len() && s[i] == b'0' {
        if i + 2 < s.len() && (s[i + 1] == b'x' || s[i + 1] == b'X') && s[i + 2].is_ascii_hexdigit() {
            base = 16;
            i += 2;
        } else {
            base = 8;
        }
    }
    let mut acc: u64 = 0;
    let mut over = false;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => u64::from(c - b'0'),
            c @ b'a'..=b'z' => u64::from(c - b'a') + 10,
            c @ b'A'..=b'Z' => u64::from(c - b'A') + 10,
            _ => break,
        };
        if d >= base {
            break;
        }
        acc = acc * base + d;
        if acc > 0x1_0000_0000 {
            over = true;
            acc = 0x1_0000_0000;
        }
        i += 1;
    }
    if neg {
        if over || acc > 0x8000_0000 {
            i32::MIN
        } else {
            (acc as i64).wrapping_neg() as i32
        }
    } else if over || acc > 0x7FFF_FFFF {
        i32::MAX
    } else {
        acc as i32
    }
}

/// The wait of `TriggerWaitForSound` in seconds (the arithmetic of the trigger-execution state machine `0x6F447340`,
/// case 3). `T` is the sound length in seconds (`I2R(ms) / 1000.0`); `elapsed` is `T` for a sound that is not playing
/// (the original reads the playing instance's position, and a sound with no instance counts as fully played); the
/// wait is `0` when `elapsed > T - offset`, else `(T - offset) - elapsed`, and never below `0`. A handle that is not a
/// live sound sleeps `offset` like `TriggerSleepAction`.
pub fn sound_wait_seconds(gm: &mut Game, handle: u32, offset: F32) -> F32 {
    let Some((dur_ms, playing, pos_ms)) = super::presentation::sound_timing(gm, handle) else {
        return clamp_sleep(offset);
    };
    let Some(tables) = gm.order_tables() else {
        // No soft-float tables: the division is not possible, the wait is the minimum (UNSURE).
        return F32::ZERO;
    };
    wait_from_timing(dur_ms, playing.then_some(pos_ms), offset, tables)
}

/// The arithmetic of [`sound_wait_seconds`] on plain numbers: `dur_ms` the sound length, `elapsed_ms` the position of
/// the playing instance (`None`: not playing), `offset` the script's real. Checked against the machine code in Unicorn
/// (`tools/emu/sound_wait_emu.py`, `tests/sound_wait_vectors.rs`).
pub fn wait_from_timing(dur_ms: i32, elapsed_ms: Option<i32>, offset: F32, tables: &softfloat::Tables) -> F32 {
    let thousand = F32::from_bits(0x447A_0000);
    let total = arith::div(conv::i2r(dur_ms), thousand, tables);
    let elapsed = match elapsed_ms {
        Some(ms) => arith::div(conv::i2r(ms), thousand, tables),
        None => total,
    };
    let d = arith::sub(total, offset);
    let mut w = if arith::gt(elapsed, d) { F32::ZERO } else { arith::sub(d, elapsed) };
    if arith::lt(w, F32::ZERO) {
        w = F32::ZERO;
    }
    w
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

fn int_arg(a: &[Value], i: usize) -> i32 {
    match a.get(i) {
        Some(Value::Integer(n)) => *n,
        Some(Value::Handle(h)) => *h as i32,
        _ => 0,
    }
}

fn real_arg(a: &[Value], i: usize) -> F32 {
    a.get(i).and_then(|v| v.as_real()).unwrap_or(F32::ZERO)
}

/// Registers the natives that sleep the calling thread (main VM only).
pub fn register_sleeping(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("TriggerWaitForSound", move |ctx, a| {
        let seconds = {
            let mut gm = g.borrow_mut();
            sound_wait_seconds(&mut gm, handle_arg(a, 0), real_arg(a, 1))
        };
        let wake = ctx.wake_time(seconds);
        g.borrow_mut()
            .timers
            .start_at(wake.to_f32(), Payload::Sleep(ctx.thread()));
        NativeResult::Sleep(seconds)
    });
}

/// The build order flow shared by `IssueBuildOrder` and `IssueBuildOrderById` (`0x6F3C8BE0`).
fn issue_build(
    g: &Rc<RefCell<Game>>,
    rt: &mut dyn super::ScriptRuntime,
    unit_handle: u32,
    type_id: u32,
    x: F32,
    y: F32,
) -> bool {
    let unit = {
        let gm = g.borrow();
        let Some(unit) = gm.unit_of_handle(unit_handle) else {
            return false;
        };
        // The placement validator (`0x6F2A50C0`): 0 and 0x45 let the order go on, anything else refuses it.
        let code = match gm.lane_f.placement {
            Some(f) => f(&gm, unit, type_id, x, y),
            None if gm.unit_types().get(type_id).is_some() => PLACEMENT_OK,
            None => PLACEMENT_FAILED,
        };
        if code != PLACEMENT_OK && code != PLACEMENT_BLOCKERS_MOVE {
            return false;
        }
        unit
    };
    let order = Order { id: type_id, class: OrderClass::Point, point: Some((x, y)), target: 0 };
    // The ability acceptance `0x6F279560` is the order validation of the dispatcher entry.
    let (validated, index) = g.borrow_mut().issue_order(unit, order, true);
    if !validated {
        return false;
    }
    g.borrow_mut().lane_f.events.push(LaneFEvent::BuildOrder { unit, type_id, x, y });
    if let Some(index) = index {
        super::order::fire_issued(g, rt, unit, OrderClass::Point, index);
    }
    true
}

/// Registers the natives of this module on `reg`, bound to `game`.
pub fn register(reg: &mut NativeRegistry, game: &Rc<RefCell<Game>>) {
    let g = Rc::clone(game);
    reg.register("SetUnitUseFood", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(k) = gm.unit_of_handle(handle_arg(a, 0)) else {
            return NativeResult::Return(Value::Null);
        };
        let (owner, ty, reg_word) = {
            let u = gm.unit(k);
            (u.owner, u.type_id, u.food_reg)
        };
        let used = gm.type_food(ty).0;
        if bool_arg(a, 1) {
            if reg_word & REG_USED_APPLIED == 0 {
                gm.food_add_used(owner, used);
            }
            gm.unit_mut(k).food_reg |= REG_USED_MASK;
        } else {
            if reg_word & REG_USED_APPLIED != 0 {
                gm.food_add_used(owner, used.wrapping_neg());
            }
            gm.unit_mut(k).food_reg &= !REG_USED_MASK;
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("UnitSuspendDecay", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(k) = gm.unit_of_handle(handle_arg(a, 0)) else {
            return NativeResult::Return(Value::Null);
        };
        // Gate (CONFIRMED): word +0x5C has a corpse bit (0xC00) and 0x100. The sim treats a dead unit as that.
        if !gm.unit(k).dead {
            return NativeResult::Return(Value::Null);
        }
        let suspend = bool_arg(a, 1);
        gm.stop_all_orders(k);
        gm.lane_f.corpses.entry(k).or_default().decay_suspended = suspend;
        gm.lane_f.events.push(LaneFEvent::DecayStop { unit: k, suspend });
        if !suspend {
            gm.push_unit_event(UnitEvent::Notify { unit: k, id: 0xD_015A });
            gm.push_unit_event(UnitEvent::Notify { unit: k, id: 0xD_0159 });
        }
        NativeResult::Return(Value::Null)
    });

    let g = Rc::clone(game);
    reg.register("CreateCorpse", move |_, a| {
        let mut gm = g.borrow_mut();
        let Some(owner) = gm.player_index_of_handle(handle_arg(a, 0)) else {
            return NativeResult::Return(Value::Handle(0));
        };
        let type_id = int_arg(a, 1) as u32;
        let (x, y, face) = (real_arg(a, 2), real_arg(a, 3), real_arg(a, 4));
        // The builder `0x6F24F1F0` (the one `CreateUnit` uses); it allocates no handle.
        let Some((k, _)) = super::unit::build(&mut gm, owner, type_id, x, y, face, false) else {
            return NativeResult::Return(Value::Handle(0));
        };
        // The teardown `0x6F2A63B0(unit, 0, 0)`: the stop step (0xD0144 and the order lists), the pathing-disabled
        // counter, then 0xD0164, 0xD0178 and 0xD0144 and the drain.
        gm.stop_all_orders(k);
        gm.lane_f.corpses.entry(k).or_default().pathing_off += 1;
        for id in [0xD_0164, 0xD_0178, 0xD_0144] {
            gm.push_unit_event(UnitEvent::Notify { unit: k, id });
        }
        // Then 0xD015A, and the two decay notifications depending on the type flags (0x200, 0x1000). The registry has
        // no such flags, so only the first one is posted (UNSURE).
        gm.push_unit_event(UnitEvent::Notify { unit: k, id: 0xD_015A });
        // UNSURE: the result is a dead unit (life 0, no death event).
        {
            let u = gm.unit_mut(k);
            u.life = F32::ZERO;
            u.dead = true;
        }
        let id = gm.handle_or_create(k);
        NativeResult::Return(Value::Handle(id))
    });

    let g = Rc::clone(game);
    reg.register("IssueBuildOrderById", move |ctx, a| {
        let ok = issue_build(
            &g,
            ctx,
            handle_arg(a, 0),
            int_arg(a, 1) as u32,
            real_arg(a, 2),
            real_arg(a, 3),
        );
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("IssueBuildOrder", move |ctx, a| {
        let name = match a.get(1) {
            Some(Value::String(id)) => ctx.string(*id).to_string(),
            _ => String::new(),
        };
        // `0x6F3BB650` (the `UnitId` lookup): 0 when the name is unknown, and the native returns false.
        let id = g.borrow().unit_types().id_by_name(&name).unwrap_or(0);
        let ok = id != 0
            && issue_build(&g, ctx, handle_arg(a, 0), id, real_arg(a, 2), real_arg(a, 3));
        NativeResult::Return(Value::Boolean(ok))
    });

    let g = Rc::clone(game);
    reg.register("GetLocalizedHotkey", move |ctx, a| {
        let key = match a.first() {
            Some(Value::String(id)) => ctx.string(*id).to_string(),
            _ => String::new(),
        };
        if key.is_empty() {
            return NativeResult::Return(Value::Integer(0));
        }
        let gm = g.borrow();
        let n = gm
            .lane_f
            .hotkeys
            .get(&key.to_ascii_lowercase())
            .and_then(|v| v.first())
            .map_or(0, |s| parse_hotkey_text(s));
        NativeResult::Return(Value::Integer(n))
    });
}
