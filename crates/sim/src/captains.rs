//! The AI captains: the attack captain and the defense captain of a computer player, the assault requests that fill
//! them, the creep camps and the target lookups that the melee AI uses to choose where to attack.
//!
//! Spec: `docs/spec/ai-natives.md` section 8 (written with this module). Labels: CONFIRMED = the body shows it, and for
//! two routines the control flow is also checked against the original in Unicorn (the assault request `0x6F2D6EA0`,
//! 600 cases, and the creep-camp chooser `0x6F2E3980`, 600 cases, both 0 mismatches; the vectors are replayed by
//! `crates/sim/tests/captain_vectors.rs`). LIKELY = the mechanism is read and a detail inferred. UNSURE = the model
//! chose the behaviour.
//!
//! What the original has (read from the bodies; kept in plain words):
//!
//! * The AI state holds two captain records (`+0x2AC` attack, `+0x2B4`/defense, reached by `0x6F2CFEB0` for the attack
//!   one). A captain is a mover of its own (position, home, goal, goal radius, mode word at `+0x64`, flag word at
//!   `+0x6C`, enemy references at `+0x114`/`+0x120`); its units follow it. Bit 1 of the flag word is "full".
//! * `InitAssault` (`0x6F2D4DF0`) sets bit 1. `AddAssault(qty, type)` (`0x6F2D96F0` -> `0x6F2D6EA0`) tries to supply
//!   `qty` units of the type (and of its morphed partner: head hunter/berserker, druid of the claw/bear form, druid of
//!   the talon/crow form, spirit walker/ethereal form, gargoyle/stone form) in six passes: units already in the group
//!   (`0x6F2D4F20`), then idle units (`0x6F2D50A0`) healthy first (flag 1) and the rest (flag 0), each pass first for the
//!   type and then for its partner. When the quantity reaches zero the request returns true and the bit stays; if a
//!   shortage remains after the last pass the bit is cleared and the request returns false. The native
//!   `AddAssault` returns that value (the older reading "no return value" was wrong: the body tail-calls the request).
//! * `CaptainReadiness*` (`0x6F2E91A0`) averages the group's life and mana as percentages of the maxima.
//! * `AttackMoveXY` / `AttackMoveKill` set the captain's mode to 2 (attack-moving), clear the retreat bit and move the
//!   captain; `AttackMoveKill` does nothing when the captain already has that target (`0x6F2E7530`).
//! * `GetCreepCamp(min, max, flyers_ok)` (`0x6F2E3D50` -> `0x6F2E3980`) walks the camp list; see [`choose_camp`].
//!
//! What the model does in place of the captain mover: the group members get ordinary attack-move orders through the
//! order system (UNSURE: the original moves a virtual leader and the units follow it in formation).



use softfloat::F32;

use crate::natives::game::{Game, ObjKey};
use crate::orders::{Order, OrderClass, ORDER_AIMOVE, ORDER_ATTACK, ORDER_MOVE};
use crate::production::CLASS_TOWNHALL;

/// The attack captain (script constant `ATTACK_CAPTAIN` = 1).
pub const ATTACK_CAPTAIN: usize = 0;
/// The defense captain (`DEFENSE_CAPTAIN` = 2).
pub const DEFENSE_CAPTAIN: usize = 1;

/// Flag bit 1 of the captain record: the group is full (set by `InitAssault`, cleared by an unsatisfied request).
pub const FULL: u32 = 0x1;
/// Flag bit 2: the captain is retreating. Nothing in the model sets it (UNSURE: the retreat rule is not decoded).
pub const RETREATING: u32 = 0x2;

/// The life fraction below which a unit counts as injured (CONFIRMED: the constant at `0x6FAAE4D4` is 0.5 at run time,
/// `docs/spec/runtime-constants-audit.md`).
pub const INJURED_BELOW: f32 = 0.5;
/// The player index of the neutral hostile (creep) player (CONFIRMED: the camp registration `0x6F2E52F0` compares the
/// owner with 12).
pub const NEUTRAL_HOSTILE: u32 = 12;
/// `CreepCampPathingCellDistance` (the patch's `Units\MiscData.txt` has 26): a creep joins the nearest camp whose
/// first living member is closer than this many pathing cells (squared compare, `0x6F2E38A0`).
pub const CAMP_CELL_DISTANCE: f32 = 26.0;
/// A camp holds at most 12 members (the record has 12 reference slots, `0x6F2E1AC0`).
pub const CAMP_CAPACITY: usize = 12;
/// World units per path cell, and the cost per cell of the path-cost stand-in (the region A* step cost is
/// `floor(24 * distance)`, `pathing-2.md`; whether `0x6F4758D0` returns the same unit is UNSURE).
const CELL: f32 = 32.0;
/// Movement type bit "fly" (`UnitBalance movetp`, the same value as `gamedata::unitclass::move_bits::FLY`).
const MOVE_FLY: u32 = 0x2;
const COST_PER_CELL: f32 = 24.0;
/// Radius around a town hall inside which an enemy unit threatens the town (UNSURE: the threat scan is not decoded).
pub const THREAT_RADIUS: f32 = 1500.0;
/// Radius of "the group is at home" (CONFIRMED value 500.0 at `0x6FAB4F08`, passed by `0x6F2E77C0`; the compare
/// itself is UNSURE) and the default goal radius of an attack move (200.0, `0x6FAB4F2C`).
pub const HOME_RADIUS: f32 = 500.0;
pub const GOAL_RADIUS: f32 = 200.0;

/// The mode word of a captain (`+0x64`): 2 while attack-moving (CONFIRMED value written by the attack-move routines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Idle,
    AttackMove,
    GoHome,
}

impl Mode {
    /// The original's value of the mode word, used for the "steal from a lower mode" rule of the recruit pass.
    fn rank(self) -> u32 {
        match self {
            Mode::Idle => 0,
            Mode::GoHome => 1,
            Mode::AttackMove => 2,
        }
    }
}

/// Where a captain is going.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Goal {
    Point(f32, f32),
    Unit(ObjKey),
}

/// One departure of a group (report only).
#[derive(Debug, Clone, PartialEq)]
pub struct Departure {
    /// Agile clock seconds.
    pub at: f32,
    /// Group size when the order was given.
    pub size: usize,
    pub goal: Goal,
    /// The type codes of the members, in group order.
    pub types: Vec<u32>,
}

/// A captain record as the model keeps it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Captain {
    pub created: bool,
    /// Bit 1 full, bit 2 retreating.
    pub flags: u32,
    pub members: Vec<ObjKey>,
    pub home: Option<(f32, f32)>,
    pub stage: Option<(f32, f32)>,
    pub mode: Mode,
    pub goal: Option<Goal>,
    pub goal_radius: f32,
    pub in_combat: bool,
    pub departures: Vec<Departure>,
    /// Update counter (cadence of the model's re-orders).
    pub ticks: u32,
}

// ---- helpers over the game ----

fn alive(gm: &Game, k: ObjKey) -> bool {
    let u = gm.unit(k);
    !u.dead && !u.removed
}

/// The unit's world position as plain floats (used for decisions, not for bit-exact state).
pub fn pos(gm: &Game, k: ObjKey) -> (f32, f32) {
    let u = gm.unit(k);
    (u.x.to_f32(), u.y.to_f32())
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (a.0 - b.0, a.1 - b.1);
    (dx * dx + dy * dy).sqrt()
}

fn f(x: f32) -> F32 {
    F32::from_bits(x.to_bits())
}

/// The life fraction of a unit (1.0 when it has no maximum).
pub fn hp_fraction(gm: &Game, k: ObjKey) -> f32 {
    let u = gm.unit(k);
    let max = u.max_life.to_f32();
    if max > 0.0 {
        u.life.to_f32() / max
    } else {
        1.0
    }
}

fn is_structure(gm: &Game, k: ObjKey) -> bool {
    gm.unit_types().is_building(gm.unit(k).type_id)
}

fn is_hall(gm: &Game, k: ObjKey) -> bool {
    gm.unit(k).class_flags & CLASS_TOWNHALL != 0
}

fn issue_point(gm: &mut Game, k: ObjKey, id: u32, p: (f32, f32)) {
    let order = Order {
        id,
        class: OrderClass::Point,
        point: Some((f(p.0), f(p.1))),
        target: 0,
    };
    let _ = gm.issue_order(k, order, true);
}

fn issue_target(gm: &mut Game, k: ObjKey, id: u32, target: ObjKey) {
    let handle = gm.handle_of(target);
    if handle == 0 {
        return;
    }
    let order = Order {
        id,
        class: OrderClass::Target,
        point: None,
        target: handle,
    };
    let _ = gm.issue_order(k, order, true);
}

/// The first completed town hall of `player` (its anchor), else its first unit.
pub fn anchor_of(gm: &Game, player: u32) -> Option<(f32, f32)> {
    let units = gm.owned_units(player);
    units
        .iter()
        .copied()
        .find(|&k| is_hall(gm, k) && gm.unit_completed(k))
        .or_else(|| units.first().copied())
        .map(|k| pos(gm, k))
}

// ---- the assault request ----

/// The morphed partner of a type that the request also accepts (CONFIRMED table of `0x6F2D6EA0`).
pub fn partner_type(t: u32) -> Option<u32> {
    match t {
        0x6F68_756E => Some(0x6F74_626B), // ohun -> otbk
        0x6564_6F63 => Some(0x6564_636D), // edoc -> edcm
        0x6564_6F74 => Some(0x6564_746D), // edot -> edtm
        0x6F73_7077 => Some(0x6F73_706D), // ospw -> ospm
        0x7567_6172 => Some(0x7567_726D), // ugar -> ugrm
        _ => None,
    }
}

/// One pass of the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// Units already in the group (`0x6F2D4F20`).
    Group,
    /// Recruit idle units (`0x6F2D50A0`); `true` = the first (healthy) pass.
    Recruit(bool),
}

/// The quantity step of the passes: `(*q)-- == 1`, i.e. true when the quantity was 1 before the decrement.
pub fn take(q: &mut i32) -> bool {
    let was = *q;
    *q = q.wrapping_sub(1);
    was == 1
}

/// The control flow of the assault request `0x6F2D6EA0` (CONFIRMED, Unicorn 600/600): the passes in order, the early
/// exit when the quantity is zero, and the result. `pass` runs one pass for a type and may lower the quantity.
/// Returns `true` when the request was satisfied (the "full" bit stays) and `false` when it was not (the caller clears
/// the bit).
pub fn assault_flow(typ: u32, qty: i32, mut pass: impl FnMut(Pass, u32, &mut i32)) -> bool {
    let alt = partner_type(typ);
    let mut q = qty;
    let mut seq: Vec<(Pass, u32)> = vec![(Pass::Group, typ)];
    if let Some(a) = alt {
        seq.push((Pass::Group, a));
    }
    seq.push((Pass::Recruit(true), typ));
    if let Some(a) = alt {
        seq.push((Pass::Recruit(true), a));
    }
    seq.push((Pass::Recruit(false), typ));
    if let Some(a) = alt {
        seq.push((Pass::Recruit(false), a));
    }
    for (p, t) in seq {
        pass(p, t, &mut q);
        if q == 0 {
            return true;
        }
    }
    false
}

/// What the passes need to know about the requesting AI.
#[derive(Debug, Clone, Copy)]
pub struct RequestCtx {
    pub player: u32,
    /// The AI flag word (`+0x2D0`): bit `0x1000` is `SetIgnoreInjured`.
    pub ai_flags: u32,
}

/// Releases a member: it leaves the group and goes home.
fn release(gm: &mut Game, c: &mut Captain, k: ObjKey) {
    c.members.retain(|&m| m != k);
    if alive(gm, k) {
        if let Some(h) = c.home {
            issue_point(gm, k, ORDER_MOVE, h);
        }
    }
}

/// Whether a unit of the AI may be recruited by a request. Mirrors the skips of `0x6F2D50A0` as far as the model has
/// the state: dead, hidden, under construction, a worker that is not idle or mining gold, a unit in its own attack.
fn recruitable(gm: &Game, player: u32, k: ObjKey) -> bool {
    let u = gm.unit(k);
    if u.owner != player || u.dead || u.removed || u.hidden || u.loaded {
        return false;
    }
    if u.count_state & crate::unit_counts::REGISTERED_IN_PROGRESS != 0 || is_structure(gm, k) {
        return false;
    }
    // A worker (a unit that can harvest) only when idle or mining gold (the original tests its AI mode 0 or 5).
    if gm.can_harvest(k, crate::economy::Res::Gold) || gm.can_harvest(k, crate::economy::Res::Lumber) {
        let m = crate::ai_manager::mode_of(gm, k);
        if m != crate::ai_manager::MODE_IDLE && m != crate::ai_manager::MODE_GOLD {
            return false;
        }
    }
    // A unit in an attack order of its own is left alone (the original skips orders 0xD000F and 0xD000E).
    let ids = gm.unit_order_ids(k);
    !matches!(ids.first(), Some(&id) if id == ORDER_ATTACK || id == 0x000D_000E)
}

fn member_of_other(others: &[&Captain], k: ObjKey) -> Option<usize> {
    others.iter().position(|c| c.members.contains(&k))
}

/// The six passes of `AddAssault` / `AddDefenders` over `c` (`others` are the AI's other captains, for the steal rule).
/// Returns the request result and updates the "full" bit.
pub fn request(
    gm: &mut Game,
    c: &mut Captain,
    others: &mut [&mut Captain],
    ctx: RequestCtx,
    qty: i32,
    typ: u32,
) -> bool {
    let satisfied = assault_flow(typ, qty, |pass, t, q| match pass {
        Pass::Group => group_pass(gm, c, ctx, t, q),
        Pass::Recruit(healthy) => recruit_pass(gm, c, others, ctx, t, healthy, q),
    });
    if !satisfied {
        c.flags &= !FULL;
    }
    satisfied
}

/// `0x6F2D4F20`: members of the type already in the group count toward the quantity. While the group attack-moves,
/// an injured member (below [`INJURED_BELOW`]) is released and does not count (CONFIRMED structure; the release goes
/// home, UNSURE).
fn group_pass(gm: &mut Game, c: &mut Captain, ctx: RequestCtx, t: u32, q: &mut i32) {
    for k in gm.owned_units(ctx.player) {
        if gm.unit(k).type_id != t || !c.members.contains(&k) {
            continue;
        }
        if c.mode == Mode::AttackMove && hp_fraction(gm, k) < INJURED_BELOW {
            release(gm, c, k);
            continue;
        }
        if take(q) {
            return;
        }
    }
}

/// `0x6F2D50A0`: recruit idle units of the type that are not in this group. `healthy` is the first pass (units at or
/// above [`INJURED_BELOW`]); the second pass takes the rest unless `SetIgnoreInjured` is on, in which case it takes
/// only injured units (UNSURE: the original's health compare reads either way; both orders put healthy units first).
fn recruit_pass(
    gm: &mut Game,
    c: &mut Captain,
    others: &mut [&mut Captain],
    ctx: RequestCtx,
    t: u32,
    healthy: bool,
    q: &mut i32,
) {
    for k in gm.owned_units(ctx.player) {
        if gm.unit(k).type_id != t || c.members.contains(&k) || !recruitable(gm, ctx.player, k) {
            continue;
        }
        let hp = hp_fraction(gm, k);
        if healthy {
            if hp < INJURED_BELOW {
                continue;
            }
        } else if ctx.ai_flags & 0x1000 != 0 && hp >= INJURED_BELOW {
            continue;
        }
        // Units of another captain are taken only from a lower mode (CONFIRMED compare of the mode words).
        let refs: Vec<&Captain> = others.iter().map(|o| &**o).collect();
        if let Some(i) = member_of_other(&refs, k) {
            if others[i].mode.rank() >= c.mode.rank() {
                continue;
            }
            let o = &mut *others[i];
            o.members.retain(|&m| m != k);
        }
        c.members.push(k);
        let at = c.stage.or(c.home);
        if let Some(p) = at {
            issue_point(gm, k, ORDER_MOVE, p);
        }
        if take(q) {
            return;
        }
    }
}

// ---- group queries ----

/// Drops dead and removed members.
pub fn prune(gm: &Game, c: &mut Captain) {
    c.members.retain(|&k| alive(gm, k));
}

/// The number of living members (the record's group size `+0xBC`).
pub fn group_size(gm: &Game, c: &Captain) -> i32 {
    c.members.iter().filter(|&&k| alive(gm, k)).count() as i32
}

/// One group member as `0x6F2E91A0` reads it.
#[derive(Debug, Clone, Copy)]
pub struct ReadyUnit {
    /// The unit type code; a type whose first letter is upper case (`'A'..='Z'`) is a hero.
    pub type_id: u32,
    /// Flag word `+0x5C`; bit `0x40000000` (illusion) makes a hero count as an ordinary unit.
    pub flags: u32,
    pub life: F32,
    pub max_life: F32,
    pub mana: F32,
    pub max_mana: F32,
}

/// `CaptainReadiness*` (`0x6F2E91A0`, CONFIRMED by Unicorn: `tools/emu/captain_ready_emu.py`, 600 cases, 0 mismatches,
/// replayed by `tests/captain_vectors.rs`). Only the life flag matters: with it the result is the smaller of the hero
/// life percentage and the percentage of the other units; without it the mana percentage. Percentages are
/// `div(mul(sum, 100.0), max_sum)` in soft float, every sum starts at 1.0, the others' maximum life starts at
/// `1.0 + (captain+0xB8 - captain+0xBC) * 400` (`missing` = that difference), mana is pooled over all units. The
/// result is the soft `f2i` (truncation). `div` is the soft-float division of the host (needs the reciprocal table).
pub fn readiness_with(div: impl Fn(F32, F32) -> F32, missing: i32, units: &[ReadyUnit], life_flag: bool) -> i32 {
    use softfloat::arith::{add, mul};
    use softfloat::conv::{f2i, i2r};
    let one = F32::ONE;
    let (mut hero_life, mut hero_max) = (one, one);
    let (mut other_life, mut other_max) = (one, add(one, i2r(missing.wrapping_mul(400))));
    let (mut mana, mut max_mana) = (one, one);
    for u in units {
        let hero = (b'A'..=b'Z').contains(&((u.type_id >> 24) as u8)) && u.flags & 0x4000_0000 == 0;
        if hero {
            hero_life = add(hero_life, u.life);
            hero_max = add(hero_max, u.max_life);
        } else {
            other_life = add(other_life, u.life);
            other_max = add(other_max, u.max_life);
        }
        mana = add(mana, u.mana);
        max_mana = add(max_mana, u.max_mana);
    }
    let hundred = F32::from_bits(100.0_f32.to_bits());
    let pct = |num: F32, den: F32| div(mul(num, hundred), den);
    if life_flag {
        let h = pct(hero_life, hero_max);
        let mut o = pct(other_life, other_max);
        if !(o.to_f32() <= h.to_f32()) {
            o = h;
        }
        f2i(o)
    } else {
        f2i(pct(mana, max_mana))
    }
}

/// `CaptainReadiness`, `CaptainReadinessHP` (life flag) and `CaptainReadinessMa` (mana). The wrappers push the flags
/// (1,1), (1,0), (0,1) but the routine reads only the first, so `CaptainReadiness` equals `CaptainReadinessHP`.
/// The `+0xB8` field of the record (units the captain should hold) has no model: `missing` is 0 (UNSURE). Without the
/// soft-float tables the division is plain f32 (not bit-exact).
pub fn readiness(gm: &Game, c: &Captain, life_flag: bool) -> i32 {
    let units: Vec<ReadyUnit> = c
        .members
        .iter()
        .filter(|&&k| alive(gm, k))
        .map(|&k| {
            let u = gm.unit(k);
            ReadyUnit { type_id: u.type_id, flags: 0, life: u.life, max_life: u.max_life, mana: u.mana, max_mana: u.max_mana }
        })
        .collect();
    match gm.softfloat_tables() {
        Some(t) => readiness_with(|a, b| softfloat::arith::div(a, b, t), 0, &units, life_flag),
        None => readiness_with(|a, b| F32::from_bits((a.to_f32() / b.to_f32()).to_bits()), 0, &units, life_flag),
    }
}

/// The centroid of the living members, or the home point when there are none.
pub fn center(gm: &Game, c: &Captain) -> Option<(f32, f32)> {
    let live: Vec<(f32, f32)> = c.members.iter().filter(|&&k| alive(gm, k)).map(|&k| pos(gm, k)).collect();
    if live.is_empty() {
        return c.home;
    }
    let n = live.len() as f32;
    Some((live.iter().map(|p| p.0).sum::<f32>() / n, live.iter().map(|p| p.1).sum::<f32>() / n))
}

/// A one-line description of a captain for run reports: size, centre, goal, goal distance and the lowest member life.
pub fn describe(gm: &Game, c: &Captain) -> String {
    let centre = center(gm, c).map_or("-".to_string(), |p| format!("({:.0},{:.0})", p.0, p.1));
    let goal = match c.goal {
        None => "none".to_string(),
        Some(Goal::Point(x, y)) => format!("point({x:.0},{y:.0})"),
        Some(Goal::Unit(k)) => {
            let u = gm.unit(k);
            format!(
                "unit {:?} owner {} {} at ({:.0},{:.0}) life {:.0}",
                gm.unit(k).type_id,
                u.owner,
                if alive(gm, k) { "alive" } else { "dead" },
                u.x.to_f32(),
                u.y.to_f32(),
                u.life.to_f32()
            )
        }
    };
    let low = c
        .members
        .iter()
        .filter(|&&k| alive(gm, k))
        .map(|&k| hp_fraction(gm, k))
        .fold(1.0f32, f32::min);
    let idle = c.members.iter().filter(|&&k| alive(gm, k) && gm.unit_order_ids(k).is_empty()).count();
    format!("centre {centre}, goal {goal}, lowest hp {low:.2}, idle {idle}")
}

/// `RemoveInjuries` (`0x6F2DDDA0` -> `0x6F2EEE50`, CONFIRMED structure): every non-hero member of the attack captain
/// whose life fraction is at most 0.5 (the constant at `0x6FAAE4D0`) leaves the group, joins the defense captain and is
/// sent to that captain's home. (When the AI has no defense captain the original releases only the most injured one;
/// the model always has both captains.)
pub fn remove_injuries(gm: &mut Game, attack: &mut Captain, defense: &mut Captain) {
    prune(gm, attack);
    let hurt: Vec<ObjKey> = attack
        .members
        .iter()
        .copied()
        .filter(|&k| gm.unit(k).hero.is_none() && hp_fraction(gm, k) <= 0.5)
        .collect();
    for k in hurt {
        attack.members.retain(|&m| m != k);
        if !defense.members.contains(&k) {
            defense.members.push(k);
        }
        if let Some(h) = defense.home {
            issue_point(gm, k, ORDER_MOVE, h);
        }
    }
}

/// The squared limit of the captain position test `0x6F2E76E0` (CONFIRMED by Unicorn, 600 cases, 0 mismatches,
/// `tools/emu/captain_goal_emu.py`): a radius below 100.0 gives the constant 20000.0 (`0x6FAB4F50`); otherwise
/// `add(mul(mul(2.0, r), r), 5000.0)` (constants `0x6FAAE504`, `0x6FAB4F54`), all through the soft-float routines.
pub fn goal_limit(radius: F32) -> F32 {
    use softfloat::arith::{add, mul};
    if F32::from_bits((100.0_f32).to_bits()).to_f32() > radius.to_f32() {
        return F32::from_bits((20000.0_f32).to_bits());
    }
    add(mul(mul(F32::from_bits((2.0_f32).to_bits()), radius), radius), F32::from_bits((5000.0_f32).to_bits()))
}

/// The captain position test `0x6F2E76E0`: true iff the squared distance (`0x6F6EF3B0`, from the captain's position to
/// the point) does not exceed [`goal_limit`] of the radius.
pub fn within_goal(captain: (f32, f32), point: (f32, f32), radius: f32) -> bool {
    use softfloat::arith::dist2;
    let f = |v: f32| F32::from_bits(v.to_bits());
    let d = dist2(f(captain.0), f(captain.1), f(point.0), f(point.1));
    goal_limit(f(radius)).to_f32() >= d.to_f32()
}

/// `CaptainIsHome` (`0x6F2E77C0` -> `0x6F2E76E0`, radius 500.0): the captain's position passes [`within_goal`] against
/// home. The squared limit is 2 r^2 + 5000 (about 710 units for 500.0). The captain's position (normaliser
/// `0x6F4743A0`) is the centre of the members here (UNSURE).
pub fn is_home(gm: &Game, c: &Captain) -> bool {
    match (center(gm, c), c.home) {
        (Some(p), Some(h)) => within_goal(p, h, HOME_RADIUS),
        _ => false,
    }
}

/// `CaptainAtGoal` (`0x6F2E77F0` -> `0x6F2E76E0`, the goal radius): false without a goal.
pub fn at_goal(gm: &Game, c: &Captain) -> bool {
    let Some(goal) = c.goal else { return false };
    let target = match goal {
        Goal::Point(x, y) => (x, y),
        Goal::Unit(k) => {
            if !alive(gm, k) {
                return true;
            }
            pos(gm, k)
        }
    };
    center(gm, c).is_some_and(|p| within_goal(p, target, c.goal_radius.max(GOAL_RADIUS)))
}

// ---- orders to the group ----

/// `AttackMoveXY`: every member attack-moves to the point (mode 2, retreat bit cleared).
pub fn attack_move_xy(gm: &mut Game, c: &mut Captain, p: (f32, f32), now: f32) {
    prune(gm, c);
    c.mode = Mode::AttackMove;
    c.flags &= !RETREATING;
    c.goal = Some(Goal::Point(p.0, p.1));
    c.goal_radius = GOAL_RADIUS;
    for k in c.members.clone() {
        issue_point(gm, k, ORDER_AIMOVE, p);
    }
    let types = c.members.iter().map(|&k| gm.unit(k).type_id).collect();
    c.departures.push(Departure {
        at: now,
        size: c.members.len(),
        goal: Goal::Point(p.0, p.1),
        types,
    });
}

/// `AttackMoveKill(unit)` (`0x6F2EC700`): nothing when the captain already has that target; otherwise mode 2, the
/// retreat bit cleared, and the group moves on the target.
pub fn attack_move_kill(gm: &mut Game, c: &mut Captain, target: ObjKey, now: f32) {
    if c.goal == Some(Goal::Unit(target)) {
        return;
    }
    prune(gm, c);
    c.mode = Mode::AttackMove;
    c.flags &= !RETREATING;
    c.goal = Some(Goal::Unit(target));
    c.goal_radius = GOAL_RADIUS;
    let tp = pos(gm, target);
    for k in c.members.clone() {
        issue_point(gm, k, ORDER_AIMOVE, tp);
    }
    let types = c.members.iter().map(|&k| gm.unit(k).type_id).collect();
    c.departures.push(Departure {
        at: now,
        size: c.members.len(),
        goal: Goal::Unit(target),
        types,
    });
}

/// `CaptainGoHome`: the members walk home.
pub fn go_home(gm: &mut Game, c: &mut Captain) {
    prune(gm, c);
    c.mode = Mode::GoHome;
    c.goal = None;
    if let Some(h) = c.home {
        for k in c.members.clone() {
            issue_point(gm, k, ORDER_MOVE, h);
        }
    }
}

/// One step of the captain (every AI step, 100 ms): prune, combat state, and the follow-up orders of the model.
pub fn update(gm: &mut Game, c: &mut Captain, player: u32) {
    c.ticks = c.ticks.wrapping_add(1);
    prune(gm, c);
    if c.home.is_none() {
        c.home = anchor_of(gm, player);
    }
    c.in_combat = c.members.iter().any(|&k| {
        gm.attack_view(k)
            .and_then(|v| v.target)
            .is_some_and(|t| alive(gm, t))
    });
    if c.mode != Mode::AttackMove {
        return;
    }
    // Goal reached or target dead: the group stops being on the move.
    let goal = c.goal;
    match goal {
        Some(Goal::Unit(t)) if !alive(gm, t) => {
            c.goal = None;
            c.mode = Mode::Idle;
            return;
        }
        None => {
            c.mode = Mode::Idle;
            return;
        }
        _ => {}
    }
    // Re-order idle members every second (the followers of the original stay with their leader).
    if c.ticks % 10 != 0 {
        return;
    }
    for k in c.members.clone() {
        if !gm.unit_order_ids(k).is_empty() {
            continue;
        }
        match goal {
            Some(Goal::Unit(t)) => {
                let d = dist(pos(gm, k), pos(gm, t));
                if d < 900.0 {
                    issue_target(gm, k, ORDER_ATTACK, t);
                } else {
                    issue_point(gm, k, ORDER_AIMOVE, pos(gm, t));
                }
            }
            Some(Goal::Point(x, y)) => {
                if dist(pos(gm, k), (x, y)) > GOAL_RADIUS {
                    issue_point(gm, k, ORDER_AIMOVE, (x, y));
                }
            }
            None => {}
        }
    }
}

/// `TownThreatened` (UNSURE: the threat scan of the original is not decoded): an enemy fighting unit of a real player
/// within [`THREAT_RADIUS`] of one of the AI's town halls.
pub fn town_threatened(gm: &Game, player: u32) -> bool {
    let halls: Vec<(f32, f32)> = gm
        .owned_units(player)
        .into_iter()
        .filter(|&k| is_hall(gm, k))
        .map(|k| pos(gm, k))
        .collect();
    if halls.is_empty() {
        return false;
    }
    gm.unit_objects().any(|(k, u)| {
        u.owner != player
            && u.owner < NEUTRAL_HOSTILE
            && !u.dead
            && !u.removed
            && gm.is_enemy(player, u.owner)
            && !is_structure(gm, k)
            && gm.unit_has_attack(k)
            && halls.iter().any(|&h| dist(pos(gm, k), h) < THREAT_RADIUS)
    })
}

// ---- creep camps ----

/// The camp chooser input for one camp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampView {
    /// The record's member count (`+0x24`); 0 skips the camp.
    pub count: i32,
    /// The usability test `0x6F2E1D50` returned nonzero: skip.
    pub skip: bool,
    pub level: i32,
    pub flyers: bool,
}

/// The selection of `0x6F2E3980` (CONFIRMED, Unicorn 600/600). `cost(i, limit)` is the path cost to camp `i`'s first
/// living member, called once per visited camp in list order. Returns the index of the chosen camp.
///
/// Rules: the search limit starts at 5000 and is lowered to `bound + 100` (the world sub-object's `+0x9C` plus 100); a
/// camp whose cost is above the horizon (start 999999) is ignored; a camp whose level is above `max`, or that has
/// flyers while flyers are not allowed, lowers the horizon to its cost + 200 (when that is lower than the horizon - 200
/// already) and drops the candidates beyond the new horizon; a camp with level in `min..=max` competes for the nearest
/// (strictly lower cost wins, so the first of equals stays), a camp below `min` is the fallback, nearest first. The
/// in-range candidate wins over the fallback.
pub fn choose_camp(
    camps: &[CampView],
    min: i32,
    max: i32,
    flyers_ok: bool,
    bound: u32,
    mut cost: impl FnMut(usize, u32) -> u32,
) -> Option<usize> {
    let mut horizon: u32 = 999_999;
    let (mut best_d, mut best) = (999_999u32, None);
    let (mut fb_d, mut fb) = (999_999u32, None);
    let mut limit: u32 = 5000;
    for (i, c) in camps.iter().enumerate() {
        if c.count == 0 || c.skip {
            continue;
        }
        let d = cost(i, limit);
        limit = limit.min(bound.wrapping_add(100));
        if d > horizon {
            continue;
        }
        let too_hard = c.level > max || (!flyers_ok && c.flyers);
        if !too_hard {
            if c.level < min {
                if d < fb_d {
                    fb_d = d;
                    fb = Some(i);
                }
            } else if d < best_d {
                best_d = d;
                best = Some(i);
            }
            continue;
        }
        if d < horizon.wrapping_sub(200) {
            horizon = d.wrapping_add(200);
        }
        if best_d > horizon {
            best = None;
            best_d = 999_999;
        }
        if fb_d > horizon {
            fb = None;
            fb_d = 999_999;
        }
    }
    best.or(fb)
}

/// The creep camps of a map: groups of neutral hostile fighting units (LIKELY rule of `0x6F2E51D0`/`0x6F2E38A0`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Camps {
    /// Built from the units that exist when the first camp query runs.
    pub built: bool,
    pub camps: Vec<Vec<ObjKey>>,
}

/// The level a creep counts for in its camp: hero level + 1, else the type's `level` (the pick record), at least 1
/// (UNSURE: `0x6F2A0210` reads the type level through `0x6F29DCA0`).
pub fn creep_level(gm: &Game, k: ObjKey) -> i32 {
    let u = gm.unit(k);
    if let Some(h) = &u.hero {
        return h.level + 1;
    }
    gm.unit_types()
        .pick_records()
        .iter()
        .find(|(id, _)| *id == u.type_id)
        .map_or(1, |(_, r)| r.level.max(1))
}

fn is_flyer(gm: &Game, k: ObjKey) -> bool {
    let t = gm.unit_types().get(gm.unit(k).type_id);
    t.is_some_and(|t| t.movement & MOVE_FLY != 0)
}

impl Camps {
    /// Registers every living neutral hostile non-structure unit, in creation order: it joins the nearest camp (with
    /// fewer than 12 members) whose first living member is within [`CAMP_CELL_DISTANCE`] cells (strict, squared
    /// compare), else it starts a camp.
    pub fn build(&mut self, gm: &Game) {
        if self.built {
            return;
        }
        self.built = true;
        let threshold = CAMP_CELL_DISTANCE * CAMP_CELL_DISTANCE;
        for (k, u) in gm.unit_objects() {
            if u.owner != NEUTRAL_HOSTILE || u.dead || u.removed || u.hidden || is_structure(gm, k) {
                continue;
            }
            let p = pos(gm, k);
            let mut best = threshold;
            let mut at = None;
            for (i, camp) in self.camps.iter().enumerate() {
                if camp.len() >= CAMP_CAPACITY {
                    continue;
                }
                let Some(&first) = camp.iter().find(|&&m| alive(gm, m)) else { continue };
                let q = pos(gm, first);
                let (dx, dy) = ((p.0 - q.0) / CELL, (p.1 - q.1) / CELL);
                let d2 = dx * dx + dy * dy;
                if best > d2 {
                    best = d2;
                    at = Some(i);
                }
            }
            match at {
                Some(i) => self.camps[i].push(k),
                None => self.camps.push(vec![k]),
            }
        }
    }

    /// The first living member of camp `i`.
    pub fn first_living(&self, gm: &Game, i: usize) -> Option<ObjKey> {
        self.camps.get(i)?.iter().copied().find(|&m| alive(gm, m))
    }

    /// `GetCreepCamp` for the captain at `from`: the first living member of the chosen camp.
    pub fn creep_camp(&mut self, gm: &Game, from: (f32, f32), min: i32, max: i32, flyers_ok: bool) -> Option<ObjKey> {
        self.build(gm);
        let views: Vec<CampView> = self
            .camps
            .iter()
            .map(|camp| {
                let live: Vec<ObjKey> = camp.iter().copied().filter(|&m| alive(gm, m)).collect();
                let mut level = 0;
                for &m in &live {
                    let l = creep_level(gm, m);
                    level += if is_flyer(gm, m) { 2 * l } else { l };
                }
                CampView {
                    count: live.len() as i32,
                    skip: false,
                    level,
                    flyers: live.iter().any(|&m| is_flyer(gm, m)),
                }
            })
            .collect();
        let cost = |i: usize, _limit: u32| -> u32 {
            match self.first_living(gm, i) {
                Some(m) => (dist(from, pos(gm, m)) / CELL * COST_PER_CELL) as u32,
                None => 999_999,
            }
        };
        let i = choose_camp(&views, min, max, flyers_ok, 100_000, cost)?;
        self.first_living(gm, i)
    }

    /// Whether any neutral hostile fighting unit is alive (`CreepsOnMap`: the callback of `0x6F2DC0E0` flags the first
    /// unit the builder hands it; the builder's mode 0x1B/8/0x1F filter is not decoded, UNSURE).
    pub fn creeps_on_map(gm: &Game) -> bool {
        gm.unit_objects()
            .any(|(k, u)| u.owner == NEUTRAL_HOSTILE && !u.dead && !u.removed && !is_structure(gm, k))
    }

    /// The creep guarding a site (the first living member of the camp nearest to `site` within 1100 units): the model
    /// of `GetExpansionFoe` (UNSURE).
    pub fn foe_near(&mut self, gm: &Game, site: (f32, f32)) -> Option<ObjKey> {
        self.build(gm);
        let mut best: Option<(f32, ObjKey)> = None;
        for i in 0..self.camps.len() {
            if let Some(m) = self.first_living(gm, i) {
                let d = dist(site, pos(gm, m));
                if d < 1100.0 && best.map_or(true, |(bd, _)| d < bd) {
                    best = Some((d, m));
                }
            }
        }
        best.map(|(_, m)| m)
    }
}

// ---- target lookups ----

/// Players that are enemies of `player`, ascending, among the real player slots.
fn enemy_players(gm: &Game, player: u32) -> Vec<u32> {
    (0..NEUTRAL_HOSTILE).filter(|&p| p != player && gm.is_enemy(player, p)).collect()
}

/// `GetEnemyBase` (`0x6F2DDD20`, search `0x6F2DB410`; UNSURE): the nearest enemy town hall to `from`, else the nearest
/// enemy structure. The original ranks by path cost; this uses the straight distance.
pub fn enemy_base(gm: &Game, player: u32, from: (f32, f32)) -> Option<ObjKey> {
    let enemies = enemy_players(gm, player);
    let mut best: Option<(i32, f32, ObjKey)> = None;
    for (k, u) in gm.unit_objects() {
        if u.dead || u.removed || !enemies.contains(&u.owner) || !is_structure(gm, k) {
            continue;
        }
        let rank = if is_hall(gm, k) { 0 } else { 1 };
        let d = dist(from, pos(gm, k));
        if best.map_or(true, |(r, bd, _)| (rank, d) < (r, bd)) {
            best = Some((rank, d, k));
        }
    }
    best.map(|(_, _, k)| k)
}

/// The nearest structure of `victim` to `from` (or the nearest unit when `units_only`), else the nearest of the other
/// kind: the target of `SuicidePlayer` / `SuicidePlayerUnits` in the model.
pub fn nearest_of(gm: &Game, victim: u32, from: (f32, f32), units_only: bool) -> Option<ObjKey> {
    let mut best: Option<(i32, f32, ObjKey)> = None;
    for (k, u) in gm.unit_objects() {
        if u.owner != victim || u.dead || u.removed {
            continue;
        }
        let building = is_structure(gm, k);
        let rank = i32::from(building == units_only);
        let d = dist(from, pos(gm, k));
        if best.map_or(true, |(r, bd, _)| (rank, d) < (r, bd)) {
            best = Some((rank, d, k));
        }
    }
    best.map(|(_, _, k)| k)
}

/// `GetEnemyExpansion` (`0x6F2DD4F0`; UNSURE): a second (or later) town hall of an enemy player. The latest created one.
pub fn enemy_expansion(gm: &Game, player: u32) -> Option<ObjKey> {
    let enemies = enemy_players(gm, player);
    let mut result = None;
    for e in enemies {
        let halls: Vec<ObjKey> = gm.owned_units(e).into_iter().filter(|&k| is_hall(gm, k)).collect();
        if halls.len() >= 2 {
            result = halls.last().copied();
        }
    }
    result
}

/// `IsTowered` (`0x6F2E0950`; UNSURE): the number of structures of the hall's owner that can attack, within 900 units
/// of the hall, is more than one.
pub fn is_towered(gm: &Game, hall: ObjKey) -> bool {
    let owner = gm.unit(hall).owner;
    let hp = pos(gm, hall);
    gm.unit_objects()
        .filter(|(k, u)| {
            u.owner == owner && !u.dead && !u.removed && *k != hall && is_structure(gm, *k) && gm.unit_has_attack(*k)
                && dist(pos(gm, *k), hp) < 900.0
        })
        .count()
        > 1
}

/// The expansion site the AI would take next (UNSURE): the nearest gold mine with gold left that is not within 900
/// units of a town hall of any player.
pub fn next_expansion(gm: &Game, player: u32) -> Option<(ObjKey, (f32, f32))> {
    let from = anchor_of(gm, player)?;
    let halls: Vec<(f32, f32)> = gm
        .unit_objects()
        .filter(|(k, u)| !u.dead && !u.removed && is_hall(gm, *k))
        .map(|(k, _)| pos(gm, k))
        .collect();
    let mut best: Option<(f32, ObjKey)> = None;
    for (k, u) in gm.unit_objects() {
        if u.dead || u.removed || gm.resource_amount(k).map_or(true, |a| a <= 0) {
            continue;
        }
        let p = pos(gm, k);
        if halls.iter().any(|&h| dist(h, p) < 900.0) {
            continue;
        }
        let d = dist(from, p);
        if best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| (k, pos(gm, k)))
}
