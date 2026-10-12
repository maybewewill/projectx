//! Missile (projectile) behaviour of basic attacks in the 1.26a engine (CMissile, vtable `0x6F89147C`).
//!
//! What this module decides, and where the evidence comes from (details and addresses in
//! `docs/spec/missiles.md`):
//!
//! * **Weapon missile type** (`weapTp`, values 1 to 8): the enum reader `0x6F262D30` (CONFIRMED). Types 1 (normal)
//!   and 4 (instant) hit through the target routine at once; the others fly (LIKELY). Only the single-target flight
//!   of type 2 (missile) is modelled in full; types 3 and 5 to 8 (artillery, splash, bounce, line) are flown to their
//!   target and damage only that target, because their area and bounce logic is not decoded.
//! * **Arrival gate** ([`arrival`]): what the missile does when it reaches its end (slot 47, `0x6F2BECB0`, and the
//!   arrival routine `0x6F2BE8B0`). CONFIRMED by Unicorn: `tools/emu/missile_gate_emu.py`, 3,000 runs (1,500 per
//!   slot), 0 mismatches against the model below; the vectors are in `tests/missile/gate_data.rs`.
//! * **Impact gate** ([`impact_check`]): slot 45 (`0x6F2C42D0`). It sets the "hit done" flag `0x20000000` and launches
//!   when the gate holds, and otherwise hands over to slot 47. Same Unicorn run (CONFIRMED).
//! * **Launch miss tail** ([`chance_to_miss`], [`miss_cell`]): every missile launch that no evasion ability catches takes one
//!   stream-2 draw for `ChanceToMiss`; CONFIRMED by Unicorn (`tools/emu/miss_gate_emu.py`, 2,400 cases, 0 mismatches),
//!   `docs/spec/missiles.md` section 9.
//! * **Flight time** ([`flight_seconds`]): distance over speed, in game seconds. LIKELY in form only: the original's
//!   flight arithmetic (speed, arc, homing) is not decoded, so the value and the rounding are a model (UNSURE).
//!
//! What is NOT modelled: the arc (visual only in this model), the launch offset and impact height (`launchX/Y/Z`,
//! `impactZ`), homing turns, the ability predicates on the source and the target (`canTarget`, the target's ability
//! predicate: the "ability hits on arrival" branch), the missile's bounded death time (`BulletDeathTime`), and the
//! splash, bounce and line effects.

use softfloat::{arith, math, Tables, F32};

/// The missile flag "bound to a target" (`0x2`): the impact and arrival gates need it, and its bit 1 is the class
/// flag the arrival passes to the target routine (CONFIRMED).
pub const FLAG_TARGET_BOUND: u32 = 0x0000_0002;
/// The missile flag "blocked" (`0x10000000`): the gates skip the target when it is set (CONFIRMED).
pub const FLAG_BLOCKED: u32 = 0x1000_0000;
/// The missile flag "hit done" (`0x20000000`): set by slot 45 when the impact launches (CONFIRMED; the name is LIKELY).
pub const FLAG_HIT_DONE: u32 = 0x2000_0000;

/// The missile type of a weapon: the `weapTp` value 1 to 8 (CONFIRMED enum `0x6F262D30`, order from
/// `docs/spec/attack-damage.md` section 12.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissileType {
    /// 1: the damage routine is called at once (CONFIRMED, Unicorn, attack-damage.md 12.1).
    Normal,
    /// 2: a single projectile that flies to its target (the modelled case).
    Missile,
    /// 3: flies to a ground point and damages around it (only the target is damaged here).
    Artillery,
    /// 4: the damage routine is called at once (CONFIRMED, Unicorn).
    Instant,
    /// 5: flies and splashes (only the target is damaged here).
    MultipleSplash,
    /// 6: flies and bounces (only the first target is damaged here).
    MultipleBounce,
    /// 7: flies along a line (only the target is damaged here).
    MultipleLine,
    /// 8: an area line (only the target is damaged here).
    AreaLine,
}

impl MissileType {
    /// The type of a `weapTp` value, or `None` outside 1 to 8.
    pub fn from_weap_tp(value: u32) -> Option<Self> {
        Some(match value {
            1 => Self::Normal,
            2 => Self::Missile,
            3 => Self::Artillery,
            4 => Self::Instant,
            5 => Self::MultipleSplash,
            6 => Self::MultipleBounce,
            7 => Self::MultipleLine,
            8 => Self::AreaLine,
            _ => return None,
        })
    }

    /// Whether the attack is a flight (true) or a direct hit (false).
    pub fn flies(self) -> bool {
        !matches!(self, Self::Normal | Self::Instant)
    }
}

/// The inputs the arrival and impact gates read: the missile flags, whether the target is present (the mask
/// resolves to a live object, `0x6F2BBA80`), the target's kind check (`slot 0x1C` against `0x6F26C1C0`), the
/// source ability, `canTarget` (`0x6F284EE0`) and the predicate (`0x6F0774E0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateInputs {
    /// The missile flags (`+0x20`).
    pub flags: u32,
    /// The target resolves to an object.
    pub target_present: bool,
    /// The target's kind value equals the constant (`0x6F26C1C0`).
    pub kind_ok: bool,
    /// The source ability exists (`0x6F021A00` is non-zero).
    pub source: bool,
    /// `canTarget(source, 0, 4)` holds.
    pub can_target: bool,
    /// The predicate on the target's abilities holds.
    pub predicate: bool,
}

impl GateInputs {
    /// The inputs of an ordinary missile of this model: the target is present or not, the kind matches, the source
    /// and `canTarget` hold, and no predicate intercepts (no target abilities are modelled).
    pub fn plain(flags: u32, target_present: bool) -> Self {
        Self {
            flags,
            target_present,
            kind_ok: true,
            source: true,
            can_target: true,
            predicate: false,
        }
    }

    /// The common part of the gates of slots 45 and 47: no blocked or hit-done flag, bound to a target, the target
    /// present and the kind matches (`0x6F2BE8B0` and `0x6F2C42D0`, CONFIRMED).
    fn gate(&self) -> bool {
        self.flags & (FLAG_BLOCKED | FLAG_HIT_DONE) == 0
            && self.flags & FLAG_TARGET_BOUND != 0
            && self.target_present
            && self.kind_ok
    }

    /// The ability checks of the "ability hits" branch: a source, `canTarget` and the predicate.
    fn ability_hits(&self) -> bool {
        self.source && self.can_target && self.predicate
    }
}

/// What the missile does on arrival (slot 47 through the arrival routine `0x6F2BE8B0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// The target's damage routine is called (slot 0x120), with the class flag = bit 1 of the missile flags.
    Damage {
        /// Bit 1 of the missile flags, passed as the routine's flag argument (CONFIRMED value, meaning UNSURE).
        class_flag: bool,
    },
    /// The "ability hits" branch: the missile returns without damage (CONFIRMED structure; LIKELY meaning).
    Blocked,
    /// No target object: nothing is applied (CONFIRMED: the arrival routine is not called).
    NoTarget,
}

/// The arrival decision of a missile (slot 47, CONFIRMED by Unicorn).
pub fn arrival(g: GateInputs) -> Arrival {
    if !g.target_present {
        return Arrival::NoTarget;
    }
    if g.gate() && g.ability_hits() {
        return Arrival::Blocked;
    }
    Arrival::Damage {
        class_flag: (g.flags >> 1) & 1 != 0,
    }
}

/// What the impact check (slot 45) decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactCheck {
    /// The gate and the ability checks hold: the flags gain "hit done" and the launch routine `0x6F2C2AF0` runs.
    Launched {
        /// The missile flags after the check.
        flags: u32,
    },
    /// Anything else: the missile hands over to its arrival (slot 47, through the tail jump `0x6F2BB570`).
    Arrive,
}

/// The impact check of a missile (slot 45, CONFIRMED by Unicorn).
pub fn impact_check(g: GateInputs) -> ImpactCheck {
    if g.gate() && g.ability_hits() {
        ImpactCheck::Launched {
            flags: g.flags | FLAG_HIT_DONE,
        }
    } else {
        ImpactCheck::Arrive
    }
}

/// The combat stream of the `ChanceToMiss` draw (`0x6F2853D0` with EDX = 2, CONFIRMED: the stream of the dice and
/// of the evasion roll).
pub const MISS_STREAM: usize = 2;
/// The movement word bit of flying units (`unit +0x1FC & 2`), the first condition of the attacker exemption of the
/// uphill miss (`0x6F0C7DE0`).
pub const MOVE_FLY: u32 = 0x2;

/// One coordinate of the cell the uphill rule compares: `f2i(scale32(v - origin))`, truncated toward zero (not
/// floored: `0x6F0D8DF0` / `0x6F0D8DC0` followed by `0x6F6EEC20`, CONFIRMED by Unicorn). The x origin is the world
/// object's `+0x6C`, the y origin `+0x70`.
pub fn miss_cell(v: F32, origin: F32) -> i32 {
    softfloat::conv::f2i(crate::pathing::scale_by_1_32(arith::sub(v, origin)))
}

/// The `ChanceToMiss` tail of the miss gate `0x6F2BEAC0` for a missile launch (CONFIRMED by Unicorn, 1,200 cases,
/// `tools/emu/miss_gate_emu.py`). `draw` is the stream-2 draw already taken (it is taken whenever the tail is
/// reached), `chance` the Misc `ChanceToMiss`. The launch misses iff `!(chance <= draw)` (an unordered compare
/// misses too), the attacker is not exempt, and the target's level is strictly above the attacker's. `levels` is
/// only called when the first two conditions hold, and returns `(attacker level, target level)`.
pub fn chance_to_miss(
    chance: F32,
    draw: F32,
    attacker_exempt: bool,
    levels: impl FnOnce() -> (i32, i32),
) -> bool {
    if chance.to_f32() <= draw.to_f32() || attacker_exempt {
        return false;
    }
    let (attacker, target) = levels();
    target > attacker
}

/// The distance between two points in world units: `sqrt(dx*dx + dy*dy)` in soft float (2D; the Z of the launch and
/// of the impact is not part of the model, see the module notes).
pub fn distance(dx: F32, dy: F32, tables: &Tables) -> F32 {
    math::sqrt(
        arith::add(arith::mul(dx, dx), arith::mul(dy, dy)),
        tables,
    )
}

/// The flight time in game seconds of a missile over `distance` world units at `speed` world units per second.
/// A speed that is not positive gives zero (the arrival is at the next timer pump). UNSURE: the original's speed unit
/// and rounding (the placeholder of the earlier model rounded to whole 0.1 s steps; this model does not round).
pub fn flight_seconds(distance: F32, speed: F32, tables: &Tables) -> F32 {
    if !arith::gt(speed, F32::ZERO) {
        return F32::ZERO;
    }
    arith::div(distance, speed, tables)
}

/// The result of the path request `0x6F496D10` of a missile launch (cell units): the flight time and the velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathRequest {
    /// The flight time `T = len / A` in seconds; zero when the request is refused.
    pub time: F32,
    /// The velocity in cells per second (the move vector times `1 / T`).
    pub vx: F32,
    pub vy: F32,
}

/// The shortest flight the path request accepts, `0.1` s (`T >= 0.1` after the soft division; a shorter flight gives
/// the all-zero answer: no velocity and time 0).
const MIN_FLIGHT: F32 = F32::from_bits(0x3DCC_CCCD);

/// The path request of the move-target routine `0x6F497080` for a missile (CONFIRMED against the original in
/// Unicorn, `tools/emu/missile_cap_emu.py`, 1,500 vectors): `(dx, dy)` is the horizontal move in **cells**, `speed` the
/// horizontal speed in **cells per second** (the launch record's `+0x4C` Missilespeed divided by 32). The flight time is
/// `sqrt(dx^2 + dy^2) / speed` in soft float; the velocity is `(dx, dy) * recip(T)`. A speed that is not positive, a
/// zero move or a time below 0.1 s gives the all-zero answer. The Z of the launch and of the target only enters the
/// vertical arc terms (not modelled). Because only the exponents differ, the same time bits come out of world units over
/// world units per second.
pub fn path_request(dx: F32, dy: F32, speed: F32, tables: &Tables) -> PathRequest {
    let zero = PathRequest {
        time: F32::ZERO,
        vx: F32::ZERO,
        vy: F32::ZERO,
    };
    let s = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
    if !arith::gt(s, F32::ZERO) || !arith::gt(speed, F32::ZERO) {
        return zero;
    }
    let time = arith::div(math::sqrt(s, tables), speed, tables);
    if arith::lt(time, MIN_FLIGHT) {
        return zero;
    }
    let r = arith::recip(time, tables);
    PathRequest {
        time,
        vx: arith::mul(dx, r),
        vy: arith::mul(dy, r),
    }
}

/// The longest flight the move issue allows, `60.0` s (the float passed as the fifth argument of `0x6F497080`, taken as
/// the move's maximum time: `min(T, 299.0, 60.0)` before the mover is told the duration; `299.0` is the clock horizon
/// minus 1.0). CONFIRMED structure (disassembly of `0x6F497080`); the use of the value as a duration is LIKELY.
pub const MAX_FLIGHT: F32 = F32::from_bits(0x4270_0000);

/// The flight time in game seconds of a missile that moves `(dx, dy)` world units at `speed` world units per second:
/// [`path_request`] in cell units (exact `1/32` scaling), capped at [`MAX_FLIGHT`]. A refused request (speed not
/// positive, or a flight below 0.1 s) gives zero, so the arrival is at the next timer pump.
pub fn flight_time(dx: F32, dy: F32, speed: F32, tables: &Tables) -> F32 {
    use crate::pathing::scale_by_1_32 as cells;
    let r = path_request(cells(dx), cells(dy), cells(speed), tables);
    if arith::lt(MAX_FLIGHT, r.time) {
        MAX_FLIGHT
    } else {
        r.time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    #[test]
    fn weap_tp_maps_one_to_eight() {
        assert_eq!(MissileType::from_weap_tp(0), None);
        assert_eq!(MissileType::from_weap_tp(9), None);
        assert_eq!(MissileType::from_weap_tp(1), Some(MissileType::Normal));
        assert_eq!(MissileType::from_weap_tp(2), Some(MissileType::Missile));
        assert_eq!(MissileType::from_weap_tp(4), Some(MissileType::Instant));
        assert_eq!(MissileType::from_weap_tp(8), Some(MissileType::AreaLine));
        assert!(!MissileType::Normal.flies());
        assert!(!MissileType::Instant.flies());
        assert!(MissileType::Missile.flies());
    }

    #[test]
    fn arrival_without_target_applies_nothing() {
        assert_eq!(
            arrival(GateInputs::plain(FLAG_TARGET_BOUND, false)),
            Arrival::NoTarget
        );
    }

    #[test]
    fn arrival_damages_with_the_bit_one_class_flag() {
        assert_eq!(
            arrival(GateInputs::plain(FLAG_TARGET_BOUND, true)),
            Arrival::Damage { class_flag: true }
        );
        assert_eq!(
            arrival(GateInputs::plain(0, true)),
            Arrival::Damage { class_flag: false }
        );
    }

    #[test]
    fn arrival_ability_branch_blocks_only_when_the_gate_holds() {
        let mut g = GateInputs::plain(FLAG_TARGET_BOUND, true);
        g.predicate = true;
        assert_eq!(arrival(g), Arrival::Blocked);
        // Hit-done set: the gate fails and the damage goes through.
        g.flags |= FLAG_HIT_DONE;
        assert!(matches!(arrival(g), Arrival::Damage { .. }));
        // Not bound: the gate fails.
        let mut h = GateInputs::plain(0, true);
        h.predicate = true;
        assert!(matches!(arrival(h), Arrival::Damage { .. }));
    }

    #[test]
    fn impact_check_launches_and_sets_hit_done() {
        let mut g = GateInputs::plain(FLAG_TARGET_BOUND, true);
        g.predicate = true;
        assert_eq!(
            impact_check(g),
            ImpactCheck::Launched {
                flags: FLAG_TARGET_BOUND | FLAG_HIT_DONE
            }
        );
        g.kind_ok = false;
        assert_eq!(impact_check(g), ImpactCheck::Arrive);
        let mut h = GateInputs::plain(FLAG_TARGET_BOUND | FLAG_BLOCKED, true);
        h.predicate = true;
        assert_eq!(impact_check(h), ImpactCheck::Arrive);
    }

    /// The lookup tables of the division and the square root, from the user's `Game.dll` (`GAME_DLL`).
    fn tables() -> Option<Tables> {
        let path = std::env::var("GAME_DLL").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        Tables::from_game_dll(&bytes).ok()
    }

    #[test]
    fn flight_time_is_distance_over_speed() {
        let Some(t) = tables() else {
            return;
        };
        let s = flight_seconds(f(100.0), f(100.0), &t);
        assert_eq!(s.to_bits(), 1.0f32.to_bits());
        assert_eq!(flight_seconds(f(100.0), F32::ZERO, &t), F32::ZERO);
        // The soft-float square root is the original's approximation, not an exact root: sqrt(25) is 5.0000668
        // (bits 0x40A0008C), as the softfloat port reproduces. The exact value is not the test's business here.
        let d = distance(f(3.0), f(4.0), &t).to_f32();
        assert!((d - 5.0).abs() < 1e-3, "distance {d}");
    }
}
