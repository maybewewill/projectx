//! The attack cycle of the 1.26a engine: target orders and attack-move, the swing gate, the reaction delay, the
//! swing timers, the idle acquisition scan and the damage at the execute point.
//!
//! Spec: `docs/spec/attack-cycle.md` (pass 1) and `docs/spec/attack-cycle-2.md` (pass 2). Labels as elsewhere:
//! CONFIRMED = read in the spec's decompile notes, LIKELY = strong inference, UNSURE = open or guessed,
//! UNVERIFIED = a model choice that the original was not checked against. PLACEHOLDER = a value or formula that
//! is not the original's and is replaced later. TODO(gamedata) marks data the engine must load from the SLK rows.
//!
//! What is modelled
//!
//! * **Attack object** ([`AttackObj`], one per unit with the attack ability `Aatk`). It holds the attack target
//!   (from a target order or from the scan), the armed flag (reaction delay finished), the planned route target and
//!   the timer ids of the cycle. The attack ability's timers are game timers ([`crate::timer::TimerQueue`]) with
//!   payload [`Payload::Attack`]; the message ids are the spec's where it names them ([`AttackMsg::spec_id`]).
//! * **Swing gate** (`0x6F0CD350`, CONFIRMED order): cooldown first (the cooldown timer must not be pending), then
//!   range ([`swing_in_range`], CONFIRMED predicate `D < (range/32 + collA + collB)^2`, strict, `D` the squared cell
//!   distance), then facing (UNSURE, see [`Game::face`]). Passing the gate starts the reaction delay (`0xD0163`,
//!   CONFIRMED structure). When the reaction timer (`0xD01AF`) ends and the gate passes again, the swing starts.
//! * **Swing** (LIKELY): the cooldown timer is armed for `cooldown`, the execute timer (`0xD01B1`) at
//!   `damage_point` and the swing-end timer (`0xD01B0`) at `damage_point + backswing`. The unit stands still until
//!   the swing ends (`0xD01B0`), then the gate runs again. The cooldown and the backswing durations are TODO(gamedata).
//! * **Reaction delay** (CONFIRMED stream 9, CONFIRMED structure): one draw from the stream 9 of the gameplay RNG per
//!   swing start (LIKELY: every swing, UNSURE: the spec does not say whether an idle unit draws only once). The draw
//!   is [`crate::rng::Rng::random_fraction`] on the stream (one generator step, the value of `GetRandomReal(0, 1)`),
//!   multiplied by the Misc value `ReactionDelay` ([`CombatMisc::reaction_delay`], host input, TODO(gamedata)).
//!   Without an RNG table the delay is zero (UNVERIFIED: the engine always sets the table).
//! * **Execute** (CONFIRMED order, LIKELY meaning): at the damage point the damage is rolled through the
//!   [`DamageModel`] (before any phase switch, CONFIRMED order) and applied to a valid target. Instant attacks
//!   (no missile speed, phase 1) apply at once. Missile attacks schedule [`AttackMsg::Impact`] after the flight time
//!   of [`crate::missiles`] (distance over speed; the flight arithmetic itself is UNSURE), and the arrival gate of
//!   [`crate::missiles::arrival`] (CONFIRMED by Unicorn) decides the damage.
//! * **Acquisition scan** (`0xD01C1`, CONFIRMED period 0.5 s, UNSURE: the 1.0 s branch is not modelled, see
//!   [`SCAN_PERIOD`]). An idle unit scans its acquire range with [`circle_candidates`], which is the cell-walk
//!   order of the range query (CONFIRMED order, groups-events-2). The first candidate that is an enemy, alive, not
//!   hidden and not the unit itself wins (CONFIRMED first-passing rule for the candidate builder, LIKELY the
//!   selection rule). No sorting, no RNG. The winner gets an attack order (replace = true). An attack-move unit with
//!   no target takes the first such candidate and keeps its move.
//! * **Death**: a dead or removed attacker does nothing (its timers fire and are dropped). A dead or removed target
//!   is not attacked: a pending execute deals nothing, a target order completes at the next tick (after the
//!   backswing when the target died during a swing).
//!
//! Not modelled: retaliation (`0x6F0CEA80`), the damage formula, armor and types (`docs/spec/attack-damage.md`),
//! the attack-move scan radius of the unit's own slot 0xEC (1500.0 in `0x6F0CCFA0`, UNSURE), the `0x6F2A0210` count
//! gate, the leash, missile flight and homing, and the per-weapon phases 3 to 8 (all missiles use one impact path).

use std::cell::RefCell;
use std::collections::BTreeMap;

use softfloat::{arith, math, Tables, F32};

use crate::abilities::ATTACK;
use crate::natives::game::{Game, ObjKey, Object, Payload};
use crate::natives::{circle_candidates, dispatch_unit_death_by, group_origin, in_ring, set_life, ScriptRuntime};
use crate::target_accept::{choose_for_unit, unit_target_flags, UnitChecks, WeaponSlot};
use crate::{bounce, splash};
use crate::orders::{Order, OrderClass, ORDER_AIMOVE, ORDER_ATTACK};
use crate::pathing::scale_by_1_32;
use crate::attack_timing;
use crate::missiles;
use crate::timer::TimerId;

/// Message id of the reaction delay end (`0xD01AF`, CONFIRMED).
pub const MSG_REACTION: u32 = 0x000D_01AF;
/// Message id of the backswing end (`0xD01B2`, CONFIRMED: handler `0x6F0CD7B0` clears the busy flags and drains the
/// order list). `0xD01B0` is the cooldown end ([`crate::attack_timing::MSG_COOLDOWN_END`], handler `0x6F0D0C70`).
pub const MSG_SWING_END: u32 = crate::attack_timing::MSG_BACKSWING_END;
/// Message id of the execute point (`0xD01B1`, CONFIRMED).
pub const MSG_EXECUTE: u32 = 0x000D_01B1;
/// Message id of the acquisition scan (`0xD01C1`, CONFIRMED).
pub const MSG_SCAN: u32 = 0x000D_01C1;

/// The period of the acquisition scan: 0.5 s (CONFIRMED constant `0x6FAB0048`). The spec's other branch, 1.0 s
/// (`0x6FAB004C`), is taken when the unit has an active aux object; that object is not modelled (UNSURE), so every
/// unit uses 0.5 s.
pub const SCAN_PERIOD: F32 = F32::from_bits(0x3F00_0000);

/// The RNG stream of the reaction delay (CONFIRMED, attack-cycle-2 section 3).
pub const REACTION_STREAM: usize = 9;

/// Alliance type index of the passive relation, the one `IsPlayerEnemy` reads (player.rs, LIKELY).
const ALLIANCE_PASSIVE: usize = 0;

/// Pi as a float32 bit pattern.
const PI: F32 = F32::from_bits(0x4049_0FDB);
/// -Pi.
const NEG_PI: F32 = F32::from_bits(0xC049_0FDB);
/// 2 * Pi (the mover's constant, `orders.rs`).
const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);
/// 0.2 cells, the arrival distance of the mover (`orders.rs`, CONFIRMED value in movement.md).
const ARRIVE_CELLS: F32 = F32::from_bits(0x3E4C_CCCD);

/// The weapon and body data of one unit type that the attack cycle needs.
///
/// TODO(gamedata): these belong to the unit type (and its weapon record). They live in a registry of their own
/// because `UnitType` has literal constructors in other crates; move them when the SLK loader fills them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AttackType {
    /// Seconds from one swing start to the next (the cooldown). TODO(gamedata): weapon cooldown.
    pub cooldown: F32,
    /// Seconds from the swing start to the execute point. TODO(gamedata): weapon damage point.
    pub damage_point: F32,
    /// Seconds from the execute point to the swing end. TODO(gamedata): weapon backswing.
    pub backswing: F32,
    /// Attack range in world units. TODO(gamedata): weapon range (the gate uses range / 32 cells).
    pub range: F32,
    /// Collision size in cells, read for both units of a swing (`+0x90`, CONFIRMED use). TODO(gamedata).
    pub collision: F32,
    /// PLACEHOLDER damage of one hit, used by [`PlaceholderDamage`]. The real damage (dice, armor, types) is
    /// `docs/spec/attack-damage.md` and is not implemented.
    pub damage: F32,
    /// Missile speed in world units per second, or `None` for an instant attack (phase 1). UNSURE: the unit and
    /// the per-step meaning of "Missilespeed" are not decoded.
    pub missile_speed: Option<F32>,
    /// Number of dice of the damage roll (`docs/spec/attack-damage.md` 3.4). Zero rolls no dice.
    pub dice: u32,
    /// Sides of each die. Each die gives `1..=sides` (the sampler's `0..sides-1` plus one, CONFIRMED by disassembly
    /// of the roll wrapper).
    pub sides: u32,
    /// Flat bonus added to the dice total (`dmgplus`; the bonus is dropped when `dice` is zero, CONFIRMED).
    pub dice_bonus: i32,
    /// Attack type row of the damage-type table: 0 Spells, 1 Normal, 2 Pierce, 3 Siege, 4 Magic, 5 Chaos, 6 Hero
    /// (binary order of the spec, section 7). This is the weapon's atkType (CONFIRMED, spec section 12.1); the
    /// weapon's missile type (`weapTp`) does not select the row.
    pub damage_type: u8,
    /// The damage class word of the attack record (`+0x14`). A basic attack is always `0x10`
    /// ([`crate::damage::ATTACK_DAMAGE_CLASS`], CONFIRMED: the execute routine pushes the literal). The armor factor
    /// applies to class `0x10` only. The default `0` is not a class of any attack; set it.
    pub damage_class: u32,
}

/// The per-weapon data beyond [`AttackType`]: the target flags the weapon chooser matches, the weapon kind and the
/// splash, bounce and line parameters (`docs/spec/weapon-target.md`, `docs/spec/weapon-splash.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WeaponExtra {
    /// `targs` of the weapon (`A+0x218+4w`).
    pub targs: u32,
    /// The `weapTp` enum (1 normal, 2 missile, 3 artillery, 4 instant, 5 msplash, 6 mbounce, 7 mline, 8 aline).
    pub kind: u32,
    /// `splashTargs`.
    pub splash_targs: u32,
    /// `Farea`, `Harea`, `Qarea`.
    pub area: [F32; 3],
    /// `Hfact`, `Qfact` (a zero is replaced by 0.5 / 0.25 at launch, [`bounce::launch_factors`]).
    pub hfact: F32,
    pub qfact: F32,
    /// `targCount` (the bounce hit count).
    pub targ_count: i32,
    /// `damageLoss` (the fraction lost per bounce).
    pub damage_loss: F32,
}

/// What a missile launch reads from the shooter's unit type besides the speed (`docs/spec/missiles.md` section 11):
/// the launch offset in the unit's frame (`launchX/Y/Z` of the weapon row, rotated by the facing and added to the unit's
/// position by `0x6F29E6A0`), the launch Z under water, the impact height added to the target's height
/// (`impactZ`, vtable slot `0x118`), and whether the missile homes (`MissileHoming`, launch record `+0x64`). The arc only
/// bends the flight visually.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MissileData {
    pub launch: [F32; 3],
    pub launch_swim_z: F32,
    pub impact_z: F32,
    pub impact_swim_z: F32,
    pub arc: F32,
    pub homing: bool,
}

/// The defense of a unit type: the armor value and the armor type column (spec section 4, `+0xE0` and `+0xE4`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Defense {
    /// Armor value. Negative armor uses the negative table in the model.
    pub armor: F32,
    /// Armor type column of the damage-type table, 0 to 7.
    pub armor_type: u8,
}

/// Misc values of the attack cycle. Host input: the engine reads them from the user's Misc data.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CombatMisc {
    /// Half angle of the facing gate in radians (Misc `AttackHalfAngle`, CONFIRMED key, UNSURE meaning). `None`
    /// skips the facing gate. TODO(gamedata): the default value is not in the container.
    pub attack_half_angle: Option<F32>,
    /// The Misc `ReactionDelay` in seconds (CONFIRMED key). TODO(gamedata): the default is not in the container.
    /// The default here is zero, a placeholder.
    pub reaction_delay: F32,
    /// The Misc `ChanceToMiss` (CONFIRMED key, `0x6F2BEAC0`): the probability of the uphill miss of a missile. A
    /// missing key reads `0.0` in the original, which is also the default here.
    pub chance_to_miss: F32,
    /// The Misc `MagicImmunesResistDamage` flag (CONFIRMED key): with it, Magic attacks cannot target magic immune units
    /// and physical attacks cannot target ethereal units (the weapon choosers and the splash filter).
    pub magic_immunes_resist: bool,
}

/// The terrain level of a pathing cell, as the miss gate reads it (`0x6F0128C0`): the host adapter over the map's
/// cliff levels (and ramps). Without a provider every cell is level 0 and the uphill rule never fires; the draw
/// of the gate is taken either way.
pub trait CliffLevels: std::fmt::Debug {
    /// The level of the cell `(cell_x, cell_y)` (32-unit cells, origin at the whole-map minimum corner).
    fn level(&self, cell_x: i32, cell_y: i32) -> i32;
}

/// The damage of one execute. The model is called once per execute, after the target is known to be valid and
/// before the attack's phase is applied (attack-cycle.md 4.4, CONFIRMED order).
///
/// `Debug` is required so that [`Game`] keeps its `Debug` derive.
pub trait DamageModel: std::fmt::Debug {
    /// The amount of life `target` loses from one execute of `attacker` with the weapon `weapon`.
    fn roll(
        &mut self,
        game: &mut Game,
        attacker: ObjKey,
        target: ObjKey,
        weapon: &AttackType,
    ) -> F32;

    /// Like [`DamageModel::roll`], but also returns the raw record amount (after the dice and Critical Strike, before
    /// the target side) when the model has one; the splash and bounce code scales it per victim.
    fn roll_split(
        &mut self,
        game: &mut Game,
        attacker: ObjKey,
        target: ObjKey,
        weapon: &AttackType,
    ) -> (F32, Option<F32>) {
        (self.roll(game, attacker, target, weapon), None)
    }

    /// The target side of a basic attack record of amount `raw` on `victim`. The default passes `raw` through.
    fn attack_side(&mut self, _game: &Game, _victim: ObjKey, _weapon: &AttackType, raw: F32) -> F32 {
        raw
    }

    /// The amount of life `target` loses from a script damage event (`UnitDamageTarget`): the target side of the
    /// record the native builds. The default passes the raw amount through (the placeholder model has no armor).
    fn native_hit(&mut self, _game: &Game, hit: &NativeHit) -> F32 {
        // The one gate of the target side the placeholder keeps: an invulnerable target takes nothing, except from the
        // classes `0x10`, `0x20`, `0x4000000` and `0x400000` (CONFIRMED, attack-damage.md section 11).
        if hit.invulnerable && hit.class & (0x10 | 0x20 | 0x400_0000 | 0x40_0000) == 0 {
            return F32::ZERO;
        }
        hit.raw
    }
}

/// The attack record `UnitDamageTarget` builds (`0x6F3C8850`, CONFIRMED by disassembly) plus the live facts of the
/// two units the target side reads.
#[derive(Debug, Clone, Copy)]
pub struct NativeHit {
    pub source: ObjKey,
    pub target: ObjKey,
    /// Record `+0x10`: the amount argument, unclamped.
    pub raw: F32,
    /// Record `+0x14`: `1 << (damage type & 31)` (the shift count is the low byte, taken mod 32 by `shl`).
    pub class: u32,
    /// Record `+0x20`: the attack type argument, the damage-type table row.
    pub row: u32,
    /// Record `+0x0C`: `0x100` for the attack argument, `|1` for the ranged argument.
    pub record_flags: u32,
    /// The source is not allied with the target (LIKELY meaning of `0x6F285B00`).
    pub not_allied: bool,
    /// The target is invulnerable (the first gate of the target side).
    pub invulnerable: bool,
}

/// PLACEHOLDER damage model: the fixed per-weapon value [`AttackType::damage`]. It ignores armor, types, dice and
/// the target's state. The real formula is `docs/spec/attack-damage.md`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlaceholderDamage;

impl DamageModel for PlaceholderDamage {
    fn roll(
        &mut self,
        _game: &mut Game,
        _attacker: ObjKey,
        _target: ObjKey,
        weapon: &AttackType,
    ) -> F32 {
        weapon.damage
    }
}

/// The payload of an attack timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackMsg {
    /// The acquisition scan (periodic).
    Scan,
    /// The reaction delay ends (`0xD01AF`).
    Reaction,
    /// The swing ends, the backswing is over (`0xD01B0`).
    SwingEnd,
    /// The execute point, the damage is applied (`0xD01B1`).
    Execute,
    /// The cooldown of the last swing ends. No message id is known for it (UNSURE: the spec's cooldown object is
    /// the attack's timer sub-object at `+0x1D8`, its message is not named).
    Cooldown,
    /// A missile hits: the damage rolled at the execute point is applied to `target` (phases 2 to 8, UNSURE timing).
    Impact { target: ObjKey, damage: F32, raw: F32, weapon: u8 },
    /// A bounce missile reaches its next target (chain `id` of [`CombatSystem`]).
    Bounce { id: u32 },
}

impl AttackMsg {
    /// The spec's message id of this timer, if the spec names one.
    pub fn spec_id(self) -> Option<u32> {
        match self {
            AttackMsg::Scan => Some(MSG_SCAN),
            AttackMsg::Reaction => Some(MSG_REACTION),
            AttackMsg::SwingEnd => Some(MSG_SWING_END),
            AttackMsg::Execute => Some(MSG_EXECUTE),
            AttackMsg::Cooldown => Some(crate::attack_timing::MSG_COOLDOWN_END),
            AttackMsg::Impact { .. } | AttackMsg::Bounce { .. } => None,
        }
    }
}

/// The two kinds of attack order the cycle runs (attack-cycle.md 2 and orders.md 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CombatKind {
    /// A target attack (`IssueTargetOrder "attack"`): the target is fixed and the order completes when it is gone.
    TargetAttack,
    /// An attack-move (`IssuePointOrder "attack"` or `"AImove"`): the unit moves to the point and attacks what it
    /// finds on the way. The order completes at the point.
    AttackMove,
}

/// The attack object of one unit (see the module notes). Timers are kept as ids; their expiries are read from the
/// timer queue ([`Game::attack_view`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AttackObj {
    /// The weapon (0 or 1) the weapon chooser picked for the current target.
    pub weapon: usize,
    /// The unit being attacked, if any.
    pub target: Option<ObjKey>,
    /// Set when the reaction delay has ended and the next gate pass starts the swing.
    pub armed: bool,
    /// The point the route was last planned for (a moving target is re-planned when it has moved).
    pub planned_for: Option<(F32, F32)>,
    /// The periodic acquisition scan.
    pub scan: Option<TimerId>,
    /// The pending reaction delay.
    pub reaction: Option<TimerId>,
    /// The pending execute point.
    pub execute: Option<TimerId>,
    /// The pending swing end (the unit is busy until it fires).
    pub swing_end: Option<TimerId>,
    /// The pending cooldown.
    pub cooldown: Option<TimerId>,
}

/// A read-only view of an attack object: its target and the expiries of its timers (for tests and hosts).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackView {
    pub target: Option<ObjKey>,
    pub armed: bool,
    pub scan_expiry: Option<f32>,
    pub reaction_expiry: Option<f32>,
    pub execute_expiry: Option<f32>,
    pub swing_end_expiry: Option<f32>,
    pub cooldown_expiry: Option<f32>,
}

/// The combat state of a game: the type data, the Misc values, the attack objects and the damage model.
#[derive(Debug, Default)]
pub struct CombatSystem {
    types: BTreeMap<u32, AttackType>,
    types2: BTreeMap<u32, AttackType>,
    extras: BTreeMap<(u32, u8), WeaponExtra>,
    bounces: BTreeMap<u32, BounceChain>,
    next_bounce: u32,
    missile_data: BTreeMap<u32, MissileData>,
    defense: BTreeMap<u32, Defense>,
    states: BTreeMap<ObjKey, AttackObj>,
    misc: CombatMisc,
    damage: Option<Box<dyn DamageModel>>,
    point_damages: BTreeMap<u32, PointDamage>,
    next_point_damage: u32,
    cliffs: Option<Box<dyn CliffLevels>>,
}

/// A bounce missile in flight between two targets.
#[derive(Debug, Clone)]
pub struct BounceChain {
    attacker: ObjKey,
    weapon: usize,
    /// The unit the missile is flying to.
    next: ObjKey,
    /// The units already hit (the primary target and every bounce target).
    hit: Vec<ObjKey>,
    /// The raw amount for the next hit.
    raw: F32,
    /// The remaining hit counter (`CMissileBounce +0xB4`).
    counter: i32,
}

/// The parameters of one `UnitDamagePoint` call (`0x6F3C8770`), kept until a delayed call comes due.
#[derive(Debug, Clone, Copy)]
pub struct PointDamage {
    pub source: ObjKey,
    pub x: F32,
    pub y: F32,
    pub radius: F32,
    pub amount: F32,
    pub attack: bool,
    pub ranged: bool,
    pub attack_type: u32,
    pub damage_type: u32,
}

/// The swing range predicate (attack-cycle-2 section 4.2, CONFIRMED by Unicorn): the attack reaches when the squared
/// cell distance `D` is strictly below `(range / 32 + collision_a + collision_b)^2`. Positions and the range are world
/// units, converted to cells by the exact `/ 32` of the pathing grid. The arithmetic is the soft float one.
pub fn swing_in_range(
    ax: F32,
    ay: F32,
    bx: F32,
    by: F32,
    range: F32,
    collision_a: F32,
    collision_b: F32,
) -> bool {
    let d = arith::dist2(
        scale_by_1_32(ax),
        scale_by_1_32(ay),
        scale_by_1_32(bx),
        scale_by_1_32(by),
    );
    let reach = arith::add(arith::add(scale_by_1_32(range), collision_a), collision_b);
    arith::lt(d, arith::mul(reach, reach))
}

impl Game {
    /// Sets the attack data of unit type `type_id` (see [`AttackType`]).
    pub fn set_attack_type(&mut self, type_id: u32, ty: AttackType) {
        self.combat.types.insert(type_id, ty);
    }

    /// Sets the attack data of weapon 2 of unit type `type_id`.
    pub fn set_attack_type2(&mut self, type_id: u32, ty: AttackType) {
        self.combat.types2.insert(type_id, ty);
    }

    /// The attack data of weapon 2 of unit type `type_id`, if it has one.
    pub fn attack_type2(&self, type_id: u32) -> Option<AttackType> {
        self.combat.types2.get(&type_id).copied()
    }

    /// Sets the per-weapon data of weapon `w` (0 or 1) of unit type `type_id`.
    pub fn set_weapon_extra(&mut self, type_id: u32, w: usize, extra: WeaponExtra) {
        self.combat.extras.insert((type_id, w as u8), extra);
    }

    /// The per-weapon data of weapon `w` of unit type `type_id`, if the host filled it.
    pub fn weapon_extra(&self, type_id: u32, w: usize) -> Option<WeaponExtra> {
        self.combat.extras.get(&(type_id, w as u8)).copied()
    }

    /// The attack data of weapon `w` of a unit type (`None` when the type has no such weapon).
    fn weapon_type(&self, type_id: u32, w: usize) -> Option<AttackType> {
        if w == 0 {
            self.combat.types.get(&type_id).copied()
        } else {
            self.combat.types2.get(&type_id).copied()
        }
    }

    /// The target flag word of a unit (`0x6F25D280`, [`crate::target_accept::unit_target_flags`]). A type without a `targType`
    /// (host did not fill it) derives it: structure `8`, flying `4`, otherwise ground `2`.
    pub fn target_flag_word(&self, k: ObjKey) -> u32 {
        let u = self.unit(k);
        let ty = self.unit_types().get(u.type_id);
        let mut targ = ty.map_or(0, |t| t.targ_type);
        if targ == 0 {
            targ = if self.unit_types().is_building(u.type_id) {
                8
            } else if ty.is_some_and(|t| t.movement & 2 != 0) {
                4
            } else {
                2
            };
        }
        unit_target_flags(u.dead, self.unit_is_invulnerable(k), targ, (u.type_id >> 24) as u8, u.class_flags)
    }

    /// The weapon chooser (`0x6F0C93E0`, [`crate::target_accept::choose_for_unit`]): the weapon of `attacker` that may attack
    /// `target`, or `None` when no weapon accepts it. A type whose weapon data the host did not fill with
    /// [`WeaponExtra`] keeps weapon 0 without a check (tests and hosts without `targs` data).
    pub fn choose_weapon(&self, attacker: ObjKey, target: ObjKey) -> Option<usize> {
        let tid = self.unit(attacker).type_id;
        if !(0..2).any(|w| self.combat.extras.contains_key(&(tid, w as u8))) {
            return Some(0);
        }
        let slot = |w: usize| match self.weapon_type(tid, w) {
            Some(a) => WeaponSlot {
                enabled: true,
                targs: self.weapon_extra(tid, w).map_or(0, |e| e.targs),
                atk_type: u32::from(a.damage_type),
            },
            None => WeaponSlot::default(),
        };
        let tu = self.unit(target);
        let checks = UnitChecks {
            magic_immune: tu.magic_immune > 0,
            ethereal: tu.ethereal > 0,
            misc_flag: self.combat.misc.magic_immunes_resist,
        };
        let c = choose_for_unit(self.target_flag_word(target), [slot(0), slot(1)], checks);
        (c.code == 0).then_some(c.index as usize)
    }

    /// Sets the missile launch data of unit type `type_id` (see [`MissileData`]).
    pub fn set_missile_data(&mut self, type_id: u32, data: MissileData) {
        self.combat.missile_data.insert(type_id, data);
    }

    /// The missile launch data of unit type `type_id`; the all-zero value (no offset, not homing) when unregistered.
    pub fn missile_data(&self, type_id: u32) -> MissileData {
        self.combat
            .missile_data
            .get(&type_id)
            .copied()
            .unwrap_or_default()
    }

    /// The attack data of unit type `type_id`; the all-zero value for an unregistered type.
    pub fn attack_type(&self, type_id: u32) -> AttackType {
        self.combat.types.get(&type_id).copied().unwrap_or_default()
    }

    /// Installs the terrain level provider of the missile miss gate (`None` removes it).
    pub fn set_cliff_levels(&mut self, provider: Option<Box<dyn CliffLevels>>) {
        self.combat.cliffs = provider;
    }

    /// Sets the Misc values of the attack cycle.
    pub fn set_combat_misc(&mut self, misc: CombatMisc) {
        self.combat.misc = misc;
    }

    /// Sets the defense of unit type `type_id` (see [`Defense`]). Unregistered types have zero armor and type 0.
    pub fn set_unit_defense(&mut self, type_id: u32, defense: Defense) {
        self.combat.defense.insert(type_id, defense);
    }

    /// The defense of `unit`, from its type; the default for an unregistered type.
    pub fn unit_defense(&self, unit: ObjKey) -> Defense {
        let u = self.unit(unit);
        let mut defense = self.defense_of_type(u.type_id);
        // The hero armor bonus (total, `docs/spec/heroes.md` section 10) is added to the type armor.
        if u.hero_armor.to_bits() & 0x7fff_ffff != 0 {
            defense.armor = softfloat::arith::add(defense.armor, u.hero_armor);
        }
        // The armor that buffs add (Devotion Aura: `0x6F2769C0` adds to the armor cell `+0xE0`).
        if u.bonuses.armor.to_bits() & 0x7fff_ffff != 0 {
            defense.armor = softfloat::arith::add(defense.armor, u.bonuses.armor);
        }
        defense
    }

    /// The defense registered for unit type `type_id`; the default for an unregistered type.
    pub fn defense_of_type(&self, type_id: u32) -> Defense {
        self.combat
            .defense
            .get(&type_id)
            .copied()
            .unwrap_or_default()
    }

    /// Replaces the damage model (default: [`PlaceholderDamage`]; the spec model is [`crate::damage::SpecDamage`]).
    pub fn set_damage_model(&mut self, model: Box<dyn DamageModel>) {
        self.combat.damage = Some(model);
    }

    /// The attack object of `unit` and its timer expiries, or `None` when the unit has none.
    pub fn attack_view(&self, unit: ObjKey) -> Option<AttackView> {
        let st = self.combat.states.get(&unit)?;
        let at = |id: Option<TimerId>| id.and_then(|i| self.timers.expiry_of(i));
        Some(AttackView {
            target: st.target,
            armed: st.armed,
            scan_expiry: at(st.scan),
            reaction_expiry: at(st.reaction),
            execute_expiry: at(st.execute),
            swing_end_expiry: at(st.swing_end),
            cooldown_expiry: at(st.cooldown),
        })
    }

    /// Creates the attack object of a new unit that has the attack ability and starts its acquisition scan.
    pub(crate) fn combat_on_create(&mut self, unit: ObjKey) {
        if !self.unit_has_attack(unit) {
            return;
        }
        let id = self.timers.start(
            SCAN_PERIOD.to_f32(),
            true,
            Payload::Attack {
                unit,
                msg: AttackMsg::Scan,
            },
        );
        self.st(unit).scan = Some(id);
    }

    /// Whether `unit` has the attack ability (its fixed `+0x1E8` slot).
    pub(crate) fn unit_has_attack(&self, unit: ObjKey) -> bool {
        matches!(self.object(unit), Object::Unit(u) if u.abilities.contains(ATTACK))
    }

    /// The kind of cycle that `order` runs for `unit`, if it is an attack order of a unit with the attack ability.
    pub(crate) fn combat_kind_of(&self, unit: ObjKey, order: &Order) -> Option<CombatKind> {
        if !self.unit_has_attack(unit) {
            return None;
        }
        match (order.id, order.class) {
            (ORDER_ATTACK, OrderClass::Target) => Some(CombatKind::TargetAttack),
            (ORDER_ATTACK, OrderClass::Point) | (ORDER_AIMOVE, OrderClass::Point) => {
                Some(CombatKind::AttackMove)
            }
            _ => None,
        }
    }

    /// The kind of cycle of the unit's active order, if the cycle owns it.
    pub(crate) fn combat_head(&self, unit: ObjKey) -> Option<CombatKind> {
        let head = self.order_head(unit)?;
        self.combat_kind_of(unit, &head)
    }

    /// Starts the cycle for the unit's active order (called by the order system when the order starts).
    pub(crate) fn combat_begin(&mut self, unit: ObjKey) {
        if self.is_stunned(unit) {
            // A stunned unit does not act: its orders wait (the stun buff of a spell, `cast.rs`).
            self.stop_route(unit);
            return;
        }
        let Some(head) = self.order_head(unit) else {
            return;
        };
        let Some(kind) = self.combat_kind_of(unit, &head) else {
            return;
        };
        let target = match kind {
            CombatKind::TargetAttack => self.unit_of_handle(head.target),
            CombatKind::AttackMove => None,
        };
        let st = self.st(unit);
        st.armed = false;
        st.planned_for = None;
        st.target = target;
        self.combat_tick(unit);
    }

    /// Ends the cycle's per-order state: the target, the armed flag, the reaction and the execute point. The
    /// cooldown and the swing end keep running (the recovery of a swing is not cancelled by a stop, UNVERIFIED).
    pub(crate) fn combat_end(&mut self, unit: ObjKey) {
        if !self.unit_has_attack(unit) {
            return;
        }
        let st = self.st(unit);
        let reaction = st.reaction.take();
        let execute = st.execute.take();
        st.target = None;
        st.armed = false;
        st.planned_for = None;
        self.disarm(reaction);
        self.disarm(execute);
    }

    /// Runs the cycle of `unit` once: the decisions of the order, without the timers' events. Called every step for
    /// units with an attack order, and by the timer handlers.
    pub(crate) fn combat_tick(&mut self, unit: ObjKey) {
        let Some(head) = self.order_head(unit) else {
            return;
        };
        let Some(kind) = self.combat_kind_of(unit, &head) else {
            return;
        };
        if !self.attacker_live(unit) {
            return;
        }
        if let Some(t) = self.snap(unit).target {
            if !self.attackable(unit, t) {
                let st = self.st(unit);
                st.target = None;
                st.planned_for = None;
                st.armed = false;
                if kind == CombatKind::TargetAttack {
                    self.complete_order_head(unit);
                    return;
                }
            }
        }
        let snap = self.snap(unit);
        if snap.reaction.is_some() || snap.execute.is_some() || snap.swing_end.is_some() {
            // The unit stands still during the reaction, the windup and the backswing.
            self.stop_route(unit);
            return;
        }
        match (kind, snap.target) {
            (CombatKind::TargetAttack, None) => self.complete_order_head(unit),
            (_, Some(target)) => self.drive_target(unit, target, &snap),
            (CombatKind::AttackMove, None) => self.drive_goal(unit, head.point),
        }
    }

    /// The decisions of an attack with a target: swing when in range and ready, else chase.
    fn drive_target(&mut self, unit: ObjKey, target: ObjKey, snap: &AttackObj) {
        // The weapon is chosen between swings (the timers are idle here, see `combat_tick`).
        if let Some(w) = self.choose_weapon(unit, target) {
            self.st(unit).weapon = w;
        }
        let ty = self.attack_type_of(unit);
        if self.in_attack_range(unit, target, &ty) {
            self.stop_route(unit);
            if snap.cooldown.is_some() {
                self.st(unit).armed = false;
                return;
            }
            self.face(unit, target);
            if snap.armed {
                self.st(unit).armed = false;
                self.start_swing(unit, &ty);
                return;
            }
            let delay = self.draw_reaction_delay();
            let id = self.arm(delay, unit, AttackMsg::Reaction);
            self.st(unit).reaction = Some(id);
        } else {
            self.st(unit).armed = false;
            let goal = self.pos(target);
            if snap.planned_for != Some(goal) || !self.route_active(unit) {
                self.st(unit).planned_for = Some(goal);
                self.chase_route(unit, goal);
            }
        }
    }

    /// The decisions of an attack-move with no target: walk to the point, complete there.
    fn drive_goal(&mut self, unit: ObjKey, goal: Option<(F32, F32)>) {
        let Some(goal) = goal else {
            self.complete_order_head(unit);
            return;
        };
        if self.route_active(unit) {
            return;
        }
        if self.at_point(unit, goal) || !self.chase_route(unit, goal) {
            self.complete_order_head(unit);
        }
    }

    /// Starts a swing (`0x6F0CD74D`/`0x6F0CD755`, CONFIRMED by Unicorn in [`crate::attack_timing`]): the cooldown timer
    /// (`0xD01B0`) gets the weapon cooldown over the attack speed multiplier and the execute timer (`0xD01B1`) the damage
    /// point over the multiplier, at most `cooldown - 0.01`. The backswing timer is armed when the execute point fires
    /// ([`Game::fire_execute`]), not here.
    fn start_swing(&mut self, unit: ObjKey, ty: &AttackType) {
        // Critical Strike arms at the swing start (`orc.rs`).
        self.orc_swing_start(unit);
        let cmds = match self.order_tables() {
            Some(t) => {
                let speed = self.attack_speed_of(unit);
                let delay = attack_timing::cycle_delay(speed, false, ty.cooldown, t);
                let remaining = self.timer_remaining(self.snap(unit).cooldown);
                attack_timing::schedule(speed, false, ty.damage_point, delay, true, remaining, t)
            }
            // Without the soft-float tables the values are used unscaled (UNVERIFIED fallback).
            None => vec![
                attack_timing::Cmd::Start { timer: attack_timing::Timer::Cooldown, seconds: ty.cooldown },
                attack_timing::Cmd::Start { timer: attack_timing::Timer::DamagePoint, seconds: ty.damage_point },
            ],
        };
        self.apply_timing(unit, &cmds);
    }

    /// The two stored attack speed values of `unit`: `base` is the buff multiplier (1.0 without buffs), `slow` is zero
    /// (the sim keeps no separate slow property; LIKELY folded into the multiplier by the buff code).
    fn attack_speed_of(&self, unit: ObjKey) -> attack_timing::Speed {
        attack_timing::Speed { base: self.unit(unit).bonuses.attack_mult, slow: F32::ZERO }
    }

    /// The time left on a timer (the getter the original's routines use), zero for an empty or expired one.
    fn timer_remaining(&self, id: Option<TimerId>) -> F32 {
        let Some(e) = id.and_then(|i| self.timers.expiry_of(i)) else {
            return F32::ZERO;
        };
        let (e, n) = (F32::from_bits(e.to_bits()), F32::from_bits(self.timers.now().to_bits()));
        let d = arith::sub(e, n);
        if d.to_f32() > 0.0 {
            d
        } else {
            F32::ZERO
        }
    }

    /// Executes the timer commands of [`attack_timing`]: a start replaces the pending timer, a stop cancels it.
    fn apply_timing(&mut self, unit: ObjKey, cmds: &[attack_timing::Cmd]) {
        use attack_timing::{Cmd, Timer};
        for c in cmds {
            let (timer, seconds) = match *c {
                Cmd::Start { timer, seconds } => (timer, Some(seconds)),
                Cmd::Stop { timer } => (timer, None),
            };
            let old = match timer {
                Timer::Cooldown => self.snap(unit).cooldown,
                Timer::DamagePoint => self.snap(unit).execute,
                Timer::Backswing => self.snap(unit).swing_end,
            };
            self.disarm(old);
            let msg = match timer {
                Timer::Cooldown => AttackMsg::Cooldown,
                Timer::DamagePoint => AttackMsg::Execute,
                Timer::Backswing => AttackMsg::SwingEnd,
            };
            let id = seconds.map(|s| self.arm(s, unit, msg));
            let st = self.st(unit);
            match timer {
                Timer::Cooldown => st.cooldown = id,
                Timer::DamagePoint => st.execute = id,
                Timer::Backswing => st.swing_end = id,
            }
        }
    }

    /// An attack speed bonus on `unit` (`0x6F0C9090`, CONFIRMED by Unicorn): the stored multiplier changes and the three
    /// pending swing timers are restarted with their remaining time divided by `new / old`. Returns false when the
    /// tables are missing (the caller then only updates the stored value).
    pub(crate) fn combat_add_attack_speed(&mut self, unit: ObjKey, delta: F32) -> bool {
        let Some(t) = self.order_tables() else {
            return false;
        };
        let speed = self.attack_speed_of(unit);
        let has_state = self.combat.states.contains_key(&unit);
        let pending = if has_state {
            let st = self.snap(unit);
            attack_timing::Pending {
                cooldown: self.timer_remaining(st.cooldown),
                damage_point: self.timer_remaining(st.execute),
                backswing: self.timer_remaining(st.swing_end),
            }
        } else {
            attack_timing::Pending { cooldown: F32::ZERO, damage_point: F32::ZERO, backswing: F32::ZERO }
        };
        let (new, cmds) = attack_timing::add_bonus(speed, delta, false, pending, t);
        self.unit_mut(unit).bonuses.attack_mult = new.base;
        if has_state {
            self.apply_timing(unit, &cmds);
        }
        true
    }

    /// Runs the handler of an attack timer. Returns the target that died from it, so the caller can run the death
    /// dispatch after its borrow is released.
    fn combat_fire(&mut self, unit: ObjKey, msg: AttackMsg) -> Vec<ObjKey> {
        match msg {
            AttackMsg::Scan => {
                self.fire_scan(unit);
                Vec::new()
            }
            AttackMsg::Reaction => {
                let st = self.st(unit);
                st.reaction = None;
                st.armed = true;
                self.combat_tick(unit);
                Vec::new()
            }
            AttackMsg::SwingEnd => {
                self.st(unit).swing_end = None;
                self.combat_tick(unit);
                Vec::new()
            }
            AttackMsg::Cooldown => {
                self.st(unit).cooldown = None;
                self.combat_tick(unit);
                Vec::new()
            }
            AttackMsg::Execute => self.fire_execute(unit),
            AttackMsg::Impact { target, damage, raw, weapon } => {
                self.pr_missile_impact();
                self.missile_arrive(unit, target, damage, raw, weapon as usize)
            }
            AttackMsg::Bounce { id } => self.bounce_hit(unit, id),
        }
    }

    /// The execute point (`0xD01B1` to `0x6F0CF660`, CONFIRMED order): the damage is rolled, then applied at once
    /// (instant) or on impact (missile).
    fn fire_execute(&mut self, unit: ObjKey) -> Vec<ObjKey> {
        self.st(unit).execute = None;
        // The backswing timer (`0xD01B2`) is armed first thing, before any check (`0x6F0CF6CF`, CONFIRMED order).
        if self.attacker_live(unit) {
            if let Some(t) = self.order_tables() {
                let ty = self.attack_type_of(unit);
                let speed = self.attack_speed_of(unit);
                let remaining = self.timer_remaining(self.snap(unit).cooldown);
                let cmds = attack_timing::backswing(speed, false, ty.backswing, remaining, t);
                self.apply_timing(unit, &cmds);
            } else {
                let ty = self.attack_type_of(unit);
                self.apply_timing(
                    unit,
                    &[attack_timing::Cmd::Start { timer: attack_timing::Timer::Backswing, seconds: ty.backswing }],
                );
            }
        }
        if !self.attacker_live(unit) {
            return Vec::new();
        }
        let Some(target) = self.snap(unit).target else {
            return Vec::new();
        };
        if !self.attackable(unit, target) {
            return Vec::new();
        }
        let ty = self.attack_type_of(unit);
        let weapon = self.snap(unit).weapon;
        self.bash_on_attack_start(unit, target);
        let (damage, raw) = self.roll_split(unit, target, &ty);
        let raw = raw.unwrap_or(damage);
        let damage = self.bash_modify_damage(unit, damage);
        // Order CONFIRMED (execute `0x6F0CF660`): the dice first, then the miss gate; an evaded attack stops before
        // the target routine. For missiles the gate runs at launch too (`0x6F2C3F70` calls it before the flight);
        // that an evaded missile deals no direct damage is UNSURE (the original scales the flight's damage by
        // MissDamageReduction, not modelled).
        if self.evasion_check(target) {
            self.bash_clear(unit);
            return Vec::new();
        }
        match ty.missile_speed {
            None => self.arrive(unit, target, damage, raw, weapon),
            Some(speed) => {
                // A launch also runs the `ChanceToMiss` tail of the gate: ONE stream-2 draw per launch, always
                // (CONFIRMED, `docs/spec/missiles.md` section 9). A missed missile is not bound to its target and
                // deals no direct damage (LIKELY).
                if self.chance_to_miss_check(unit, target) {
                    self.bash_clear(unit);
                    return Vec::new();
                }
                let delay = self.impact_delay(unit, target, speed);
                self.pr_missile_launch();
                self.arm(delay, unit, AttackMsg::Impact { target, damage, raw, weapon: weapon as u8 });
                Vec::new()
            }
        }
    }

    /// The acquisition scan (`0xD01C1`): an idle unit takes the first enemy in range as a target order; an attack-move
    /// unit with no target takes the first one as its target and keeps moving. Dead or removed units stop the scan.
    fn fire_scan(&mut self, unit: ObjKey) {
        if !self.attacker_live(unit) {
            if let Some(id) = self.st(unit).scan.take() {
                self.timers.cancel(id);
            }
            return;
        }
        match self.order_head(unit) {
            None => {
                if let Some(candidate) = self.scan_target(unit) {
                    let handle = self.handle_of(candidate);
                    if handle != 0 {
                        self.dispatch_order(
                            unit,
                            Order {
                                id: ORDER_ATTACK,
                                class: OrderClass::Target,
                                point: None,
                                target: handle,
                            },
                            true,
                        );
                    }
                }
            }
            Some(head) => {
                let attack_move = self.combat_kind_of(unit, &head) == Some(CombatKind::AttackMove);
                if attack_move && self.snap(unit).target.is_none() {
                    if let Some(candidate) = self.scan_target(unit) {
                        self.st(unit).target = Some(candidate);
                        self.combat_tick(unit);
                    }
                }
            }
        }
    }

    /// The first unit of the acquire-range query that the unit may attack (CONFIRMED first-passing rule, LIKELY
    /// selection). Returns `None` for a unit whose weapon has no range (it could never swing).
    fn scan_target(&self, unit: ObjKey) -> Option<ObjKey> {
        let ty = self.attack_type_of(unit);
        if !arith::gt(ty.range, F32::ZERO) {
            return None;
        }
        let (x, y, owner, radius) = {
            let u = self.unit(unit);
            (u.x, u.y, u.owner, u.acquire_range)
        };
        circle_candidates(self, x, y, radius)
            .into_iter()
            .find(|&c| {
                if c == unit {
                    return false;
                }
                let u = self.unit(c);
                !u.dead
                    && !u.removed
                    && !u.hidden
                    && self.is_enemy(owner, u.owner)
                    && !self.unit_is_invulnerable(c)
                    && self.choose_weapon(unit, c).is_some()
            })
    }

    /// `IsPlayerEnemy(p, q)` (LIKELY: the passive relation bit of `p` for `q` is clear, and a player is never the
    /// enemy of itself). Neutral players are enemies by default (UNSURE).
    pub(crate) fn is_enemy(&self, p: u32, q: u32) -> bool {
        p != q && self.player(p).alliances[ALLIANCE_PASSIVE] & (1 << q) == 0
    }

    /// The miss gate of the execute (`0x6F2BEAC0`, spec `attack-damage.md` section 12.3): the evasion abilities
    /// (`AEev`) in the TARGET's ability list are scored in list order, the best one is selected and rolls on combat
    /// stream 2 (one draw, `chance > draw` evades). Returns whether the attack is evaded. No draw is made when no
    /// entry scores above zero. The count gate (`+0x3C`) and the enabled slot (`0x268`) are modelled as "open"
    /// (UNSURE: their inputs are not in the model). The attacker's own list is the other walk (curse-like
    /// abilities, `BuffCurse`/`DrunkenHaze` classes), not modelled.
    fn evasion_check(&mut self, target: ObjKey) -> bool {
        let scores: Vec<F32> = self
            .unit(target)
            .abilities
            .iter()
            .filter(|a| a.base == crate::abilities::EVASION && !a.is_removed())
            // The chance is the level record's DataA (`+0x20`, `0x6F02E9E0`); a level outside the table gives 0.0.
            .map(|a| {
                self.ability_level_record(target, a.base)
                    .map_or(F32::ZERO, |record| record.data[0])
            })
            .collect();
        let Some(chance) = crate::damage::evasion_select(scores) else {
            return false;
        };
        let Some((table, streams)) = self.rng_parts() else {
            return false;
        };
        let draw = streams
            .stream_mut(crate::damage::EVASION_STREAM)
            .random_fraction(table);
        crate::damage::evades(chance, draw)
    }

    /// The `ChanceToMiss` tail of the miss gate `0x6F2BEAC0` for a missile launch (`a3 != 0`), reached when no
    /// evasion ability fired (CONFIRMED by Unicorn, `tools/emu/miss_gate_emu.py`). It always takes one draw of stream
    /// 2 (the same `0x6F2853D0` call as the evasion roll), then misses iff `!(ChanceToMiss <= draw)` (NaN misses),
    /// the attacker is not exempt, and the target's cell level is above the attacker's. Returns whether it missed.
    fn chance_to_miss_check(&mut self, attacker: ObjKey, target: ObjKey) -> bool {
        let Some((table, streams)) = self.rng_parts() else {
            return false;
        };
        let draw = streams
            .stream_mut(missiles::MISS_STREAM)
            .random_fraction(table);
        let chance = self.combat.misc.chance_to_miss;
        let (a, t) = (self.unit(attacker), self.unit(target));
        // LIKELY: the exemption (`0x6F0C7DE0`: movement word bit 1, flag `+0x5C & 0x20000000`, `+0x200 <= 0`) is a
        // flying attacker; the two other conditions are taken as met.
        let movement = self.unit_types().get(a.type_id).map_or(0, |ty| ty.movement);
        let exempt = movement & missiles::MOVE_FLY != 0;
        // The cell origin is the whole-map minimum corner (`world+0x6C/+0x70` is filled from the world-bounds rect
        // by the wpm reader `0x6F46B850`, terrain.md section 1); the playable minimum is only the fallback.
        let origin = match self.world_bounds().or_else(|| self.playable()) {
            Some(p) => (p.min_x, p.min_y),
            None => (F32::ZERO, F32::ZERO),
        };
        let cells = (
            (missiles::miss_cell(a.x, origin.0), missiles::miss_cell(a.y, origin.1)),
            (missiles::miss_cell(t.x, origin.0), missiles::miss_cell(t.y, origin.1)),
        );
        let level = |c: (i32, i32)| {
            self.combat
                .cliffs
                .as_ref()
                .map_or(0, |p| p.level(c.0, c.1))
        };
        missiles::chance_to_miss(chance, draw, exempt, || (level(cells.0), level(cells.1)))
    }

    /// The damage of one execute, through the installed model, with the raw record amount when the model has one.
    fn roll_split(&mut self, attacker: ObjKey, target: ObjKey, ty: &AttackType) -> (F32, Option<F32>) {
        let mut model: Box<dyn DamageModel> = self
            .combat
            .damage
            .take()
            .unwrap_or_else(|| Box::new(PlaceholderDamage));
        let out = model.roll_split(self, attacker, target, ty);
        self.combat.damage = Some(model);
        out
    }

    /// The target side of a basic attack record of amount `raw` on `victim` (splash and bounce victims).
    fn attack_side(&mut self, victim: ObjKey, ty: &AttackType, raw: F32) -> F32 {
        let mut model: Box<dyn DamageModel> = self
            .combat
            .damage
            .take()
            .unwrap_or_else(|| Box::new(PlaceholderDamage));
        let amount = model.attack_side(self, victim, ty, raw);
        self.combat.damage = Some(model);
        amount
    }

    /// `UnitDamageTarget` on a unit target: builds the record, runs the model's target side and applies the result
    /// to the life. Returns the target when this killed it (the native then runs the death dispatch with `source` as
    /// the killer, LIKELY). A dead or removed target takes nothing. The invulnerability is a gate of the target side
    /// (it does not apply to the classes `0x10`, `0x20`, `0x400000`, `0x4000000`), so it is not checked again here.
    #[allow(clippy::too_many_arguments)]
    pub fn unit_damage_target(
        &mut self,
        source: ObjKey,
        target: ObjKey,
        raw: F32,
        attack: bool,
        ranged: bool,
        attack_type: u32,
        damage_type: u32,
    ) -> Option<ObjKey> {
        let (dead, removed, life, t_owner) = {
            let u = self.unit(target);
            (u.dead, u.removed, u.life, u.owner)
        };
        if dead || removed {
            return None;
        }
        let rec = crate::damage::native_record(attack, ranged, attack_type, damage_type, 0)?;
        let s_owner = self.unit(source).owner;
        let hit = NativeHit {
            source,
            target,
            raw,
            class: rec.class,
            row: rec.row,
            record_flags: rec.flags,
            not_allied: self.is_enemy(s_owner, t_owner),
            invulnerable: self.unit_is_invulnerable(target),
        };
        let mut model: Box<dyn DamageModel> = self
            .combat
            .damage
            .take()
            .unwrap_or_else(|| Box::new(PlaceholderDamage));
        let amount = model.native_hit(self, &hit);
        self.combat.damage = Some(model);
        if arith::eq(amount, F32::ZERO) {
            return None;
        }
        let next = arith::sub(life, amount);
        set_life(self, target, next).then_some(target)
    }

    /// `UnitDamagePoint` body (`0x6F2C31D0`, structure CONFIRMED, membership test UNSURE): collects the live units in
    /// the circle first (the collector `0x6F2C2E80` runs for every candidate before any damage), then damages each
    /// through the same per-target path as `UnitDamageTarget` (`0x6F2BFED0` builds the record with the call's class,
    /// flags and attack type and calls the target's slot `0x120`). The source is not excluded and allies are hit
    /// (the collector only sorts them into relation buckets, UNSURE for the bucket use). Returns the units this
    /// killed, in apply order.
    pub fn apply_point_damage(&mut self, pd: &PointDamage) -> Vec<ObjKey> {
        let candidates: Vec<ObjKey> = circle_candidates(self, pd.x, pd.y, pd.radius)
            .into_iter()
            .filter(|&k| {
                let u = self.unit(k);
                !u.dead && !u.removed
            })
            .collect();
        let mut died = Vec::new();
        for k in candidates {
            if let Some(t) = self.unit_damage_target(
                pd.source,
                k,
                pd.amount,
                pd.attack,
                pd.ranged,
                pd.attack_type,
                pd.damage_type,
            ) {
                died.push(t);
            }
        }
        died
    }

    /// Queues a delayed `UnitDamagePoint` (delay above zero, `0x6F2C4D70` builds a delayed object): the damage runs
    /// when the timer of `delay` seconds expires (UNSURE: the delayed object's own timing is not decoded).
    pub fn schedule_point_damage(&mut self, delay: F32, pd: PointDamage) {
        let id = self.combat.next_point_damage;
        self.combat.next_point_damage += 1;
        self.combat.point_damages.insert(id, pd);
        self.timers
            .start(delay.to_f32(), false, Payload::DamagePoint(id));
    }

    /// The record of a due delayed `UnitDamagePoint`.
    pub fn take_point_damage(&mut self, id: u32) -> Option<PointDamage> {
        self.combat.point_damages.remove(&id)
    }

    /// The main damage of an attack, then Bash's on-hit effects of the attacker (stun and flat damage, docs/spec/
    /// abilities-human-heroes.md section 3; the order of the two is UNSURE). Returns the unit that died.
    fn hit_with_bash(&mut self, attacker: ObjKey, target: ObjKey, damage: F32) -> Option<ObjKey> {
        let killed = self.apply_damage(target, damage);
        let bash = if killed.is_none() {
            self.bash_on_hit(attacker, target)
        } else {
            None
        };
        self.bash_clear(attacker);
        killed.or(bash)
    }

    /// Applies `amount` to the life of `target` through the life setter (the death event fires from the caller when
    /// it returns the unit). Invulnerable and dead targets take nothing.
    fn apply_damage(&mut self, target: ObjKey, amount: F32) -> Option<ObjKey> {
        let (dead, removed, life) = {
            let u = self.unit(target);
            (u.dead, u.removed, u.life)
        };
        if dead || removed || self.unit_is_invulnerable(target) {
            return None;
        }
        let next = arith::sub(life, amount);
        set_life(self, target, next).then_some(target)
    }

    /// The reaction delay: one draw of stream 9 times the Misc value (see the module notes). Zero without a table.
    fn draw_reaction_delay(&mut self) -> F32 {
        let misc = self.combat.misc.reaction_delay;
        match self.rng_parts() {
            Some((table, streams)) => {
                let u = streams.stream_mut(REACTION_STREAM).random_fraction(table);
                arith::mul(u, misc)
            }
            None => F32::ZERO,
        }
    }

    /// The facing gate with an instant turn (UNSURE: the original's turn is not modelled). Without a half angle or
    /// without the trigonometry tables the gate is not applied. When the target is outside the half angle, the unit
    /// turns to it at once (movement.md: instant turn, UNVERIFIED for attacks).
    fn face(&mut self, unit: ObjKey, target: ObjKey) {
        let Some(half) = self.combat.misc.attack_half_angle else {
            return;
        };
        let Some(tables) = self.order_tables() else {
            return;
        };
        let (ux, uy, facing) = {
            let u = self.unit(unit);
            (u.x, u.y, u.facing)
        };
        let (tx, ty) = self.pos(target);
        let bearing = bearing(arith::sub(tx, ux), arith::sub(ty, uy), tables);
        let mut diff = arith::sub(bearing, facing);
        if arith::gt(diff, PI) {
            diff = arith::sub(diff, TWO_PI);
        }
        if arith::lt(diff, NEG_PI) {
            diff = arith::add(diff, TWO_PI);
        }
        let magnitude = F32::from_bits(diff.0 & 0x7FFF_FFFF);
        if !arith::le(magnitude, half) {
            self.unit_mut(unit).facing = bearing;
        }
    }

    /// The flight time of a missile: the path request of the move issue (see [`crate::missiles::flight_time`], CONFIRMED
    /// against `0x6F496D10` in Unicorn: horizontal distance over speed, refused below 0.1 s, capped at 60 s). Zero
    /// without the tables (the square root and the division need them).
    fn impact_delay(&self, attacker: ObjKey, target: ObjKey, speed: F32) -> F32 {
        let Some(tables) = self.order_tables() else {
            return F32::ZERO;
        };
        let (a, b) = (self.unit(attacker), self.unit(target));
        let data = self.missile_data(a.type_id);
        // The launch point `0x6F29E6A0`: the shooter's 3-D position plus the launch offset turned by its facing. Under
        // water (movement type 32 with the swim flag, not modelled) the Z would be `launchSwimZ`.
        let launch = missiles::launch_point(
            (a.x, a.y),
            self.unit_z(attacker),
            a.facing,
            data.launch,
            None,
            tables,
        );
        let origin = match self.world_bounds().or_else(|| self.playable()) {
            Some(p) => (p.min_x, p.min_y),
            None => (F32::ZERO, F32::ZERO),
        };
        let horizon = crate::movement::LAYER_HORIZON;
        if data.homing {
            // A homing missile (launch record `+0x64`) flies to the followed target's current cell position
            // (`0x6F497260`), without lead.
            let g = self.layer();
            let target_cell = self.kin_of(target).position(&g);
            let start = (
                scale_by_1_32(arith::sub(launch[0], origin.0)),
                scale_by_1_32(arith::sub(launch[1], origin.1)),
            );
            return missiles::flight_duration_cells(start, target_cell, speed, horizon, tables);
        }
        // The aimed point `0x6F2C3F70`: the target's position led by its step vector, at its height plus the impact offset.
        let kin = self.kin_of(target);
        // The target's own `impactZ` (its vtable slot `0x118`, `0x6F29E570`; the swimming variant is not modelled).
        let impact = self.missile_data(b.type_id).impact_z;
        let none = F32::from_bits(0xFF7F_FFFF);
        let all = F32::from_bits(0x7F7F_FFFF);
        let rect = match self.world_bounds() {
            Some(w) => [w.min_y, w.min_x, w.max_y, w.max_x],
            None => [none, none, all, all],
        };
        let aimed = missiles::launch_target(
            (launch[0], launch[1]),
            (b.x, b.y),
            (kin.vx, kin.vy),
            speed,
            self.unit_z(target),
            impact,
            rect,
            tables,
        );
        // The move issue `0x6F47A650` clamps the point into the playable area (`0x6F469490`).
        let (tx, ty) = self.clamp_unit_move(aimed.x, aimed.y);
        missiles::flight_duration((launch[0], launch[1]), (tx, ty), origin, speed, horizon, tables)
    }

    /// The arrival of a missile (slot 47, [`crate::missiles::arrival`]): the rolled damage goes through when the target
    /// is still a live unit. The attacker may be dead or gone: the missile keeps its own target (CONFIRMED gate).
    fn missile_arrive(
        &mut self,
        unit: ObjKey,
        target: ObjKey,
        damage: F32,
        raw: F32,
        weapon: usize,
    ) -> Vec<ObjKey> {
        let present = self.target_valid(unit, target);
        match missiles::arrival(missiles::GateInputs::plain(
            missiles::FLAG_TARGET_BOUND,
            present,
        )) {
            missiles::Arrival::Damage { .. } => self.arrive(unit, target, damage, raw, weapon),
            missiles::Arrival::Blocked | missiles::Arrival::NoTarget => Vec::new(),
        }
    }

    /// The hit on the primary target, then the splash or the bounce chain of the weapon kind. Returns the units that
    /// died, in order.
    fn arrive(&mut self, unit: ObjKey, target: ObjKey, damage: F32, raw: F32, weapon: usize) -> Vec<ObjKey> {
        let mut died: Vec<ObjKey> = self.hit_with_bash(unit, target, damage).into_iter().collect();
        let tid = self.unit(unit).type_id;
        let Some(extra) = self.weapon_extra(tid, weapon) else {
            return died;
        };
        match extra.kind {
            // artillery and msplash carry the ring spec (`0x6F0C5510`); the primary target was hit above with factor 1.0
            3 | 5 => self.splash_hits(unit, target, raw, weapon, &extra, &mut died),
            6 => self.bounce_start(unit, target, raw, weapon, &extra),
            _ => {}
        }
        died
    }

    /// The area part of an msplash or artillery hit (`CMissileSplash` arrival `0x6F2C2C80`, docs/spec/weapon-splash.md):
    /// every unit in the circle of the outermost ring that passes the splash filter takes `raw` times the factor of its
    /// ring, through the target side of its own armor. The primary target is excluded (it took slot 51 directly).
    fn splash_hits(
        &mut self,
        attacker: ObjKey,
        primary: ObjKey,
        raw: F32,
        weapon: usize,
        extra: &WeaponExtra,
        died: &mut Vec<ObjKey>,
    ) {
        let [farea, harea, qarea] = extra.area;
        let (hf, qf) = bounce::launch_factors(extra.hfact, extra.qfact);
        let radius = splash::query_radius(farea, harea, qarea);
        let (cx, cy) = self.pos(primary);
        let ty = self.weapon_type(self.unit(attacker).type_id, weapon).unwrap_or_default();
        let (owner, src) = match self.object(attacker) {
            Object::Unit(a) if !a.removed => (a.owner, splash::Source::Other(a.owner)),
            Object::Unit(a) => (a.owner, splash::Source::Gone),
            _ => (u32::MAX, splash::Source::Gone),
        };
        let origin = group_origin(self);
        let (mb, pb) = splash::attack_type_blocks(self.combat.misc.magic_immunes_resist, u32::from(ty.damage_type));
        let candidates = circle_candidates(self, cx, cy, radius);
        let mut hits = Vec::new();
        for c in candidates {
            if c == primary {
                continue;
            }
            let u = self.unit(c);
            if u.dead || u.removed {
                continue;
            }
            let cand = splash::Candidate {
                dead: u.dead,
                invulnerable_flag: self.unit_is_invulnerable(c),
                targ_type: self.target_flag_word(c) & 0x1E,
                type_char: (u.type_id >> 24) as u8,
                class_word: u.class_flags,
                ethereal: u.ethereal,
                magic_immune: u.magic_immune,
                invulnerable: self.unit_is_invulnerable(c),
                owner: u.owner,
            };
            let passive = |a: u32, b: u32| a < 16 && b < 16 && self.player(a).alliances[0] & (1 << b) != 0;
            let shared = |a: u32, b: u32| a < 16 && b < 16 && self.player(a).alliances[4] & (1 << b) != 0;
            if !splash::candidate_filter(&cand, extra.splash_targs, src, owner, mb, pb, passive, shared) {
                continue;
            }
            let (ux, uy) = (u.x, u.y);
            if let Some(f) = splash::ring_factor(farea, harea, qarea, hf, qf, |r| in_ring(ux, uy, cx, cy, r, origin)) {
                hits.push((c, f));
            }
        }
        for (c, f) in hits {
            let amount = self.attack_side(c, &ty, arith::mul(raw, f));
            if let Some(d) = self.apply_damage(c, amount) {
                died.push(d);
            }
        }
    }

    /// Starts the bounce chain after the first hit (`CMissileBounce` arrival `0x6F2C5ED0`).
    fn bounce_start(&mut self, attacker: ObjKey, primary: ObjKey, raw: F32, weapon: usize, extra: &WeaponExtra) {
        let mut counter = extra.targ_count;
        let Some(raw) = bounce::after_hit(&mut counter, raw, extra.damage_loss) else {
            return;
        };
        let hit = vec![primary];
        self.bounce_next(attacker, primary, hit, raw, counter, weapon, extra);
    }

    /// Picks the next bounce target (the nearest accepted unit around the current target, `0x6F2BD090`) and sends the
    /// missile after it.
    #[allow(clippy::too_many_arguments)]
    fn bounce_next(
        &mut self,
        attacker: ObjKey,
        from: ObjKey,
        hit: Vec<ObjKey>,
        raw: F32,
        counter: i32,
        weapon: usize,
        extra: &WeaponExtra,
    ) {
        let (cx, cy) = self.pos(from);
        let radius = extra.area[0];
        let w = bounce::bounce_targs(Some(extra.splash_targs));
        let ty = self.weapon_type(self.unit(attacker).type_id, weapon).unwrap_or_default();
        let (owner, src) = match self.object(attacker) {
            Object::Unit(a) if !a.removed => (a.owner, splash::Source::Other(a.owner)),
            Object::Unit(a) => (a.owner, splash::Source::Gone),
            _ => (u32::MAX, splash::Source::Gone),
        };
        let (mb, pb) = splash::attack_type_blocks(self.combat.misc.magic_immunes_resist, u32::from(ty.damage_type));
        let origin = group_origin(self);
        let mut cands: Vec<(ObjKey, bool, F32)> = Vec::new();
        for c in circle_candidates(self, cx, cy, radius) {
            let u = self.unit(c);
            let dist = arith::dist2(
                scale_by_1_32(arith::sub(u.x, origin.0)),
                scale_by_1_32(arith::sub(u.y, origin.1)),
                scale_by_1_32(arith::sub(cx, origin.0)),
                scale_by_1_32(arith::sub(cy, origin.1)),
            );
            let mut ok = !u.dead && !u.removed && !hit.contains(&c);
            if ok {
                let cand = splash::Candidate {
                    dead: u.dead,
                    invulnerable_flag: self.unit_is_invulnerable(c),
                    targ_type: self.target_flag_word(c) & 0x1E,
                    type_char: (u.type_id >> 24) as u8,
                    class_word: u.class_flags,
                    ethereal: u.ethereal,
                    magic_immune: u.magic_immune,
                    invulnerable: self.unit_is_invulnerable(c),
                    owner: u.owner,
                };
                let passive = |a: u32, b: u32| a < 16 && b < 16 && self.player(a).alliances[0] & (1 << b) != 0;
                let shared = |a: u32, b: u32| a < 16 && b < 16 && self.player(a).alliances[4] & (1 << b) != 0;
                ok = splash::candidate_filter(&cand, w, src, owner, mb, pb, passive, shared);
            }
            cands.push((c, ok, dist));
        }
        let list: Vec<(bool, F32)> = cands.iter().map(|&(_, ok, d)| (ok, d)).collect();
        let (Some(i), _) = bounce::pick_nearest(&list) else {
            return;
        };
        let next = cands[i].0;
        let Some(speed) = ty.missile_speed else {
            return;
        };
        let delay = self.flight_between((cx, cy), self.pos(next), speed);
        let id = self.combat.next_bounce;
        self.combat.next_bounce += 1;
        let mut hit = hit;
        hit.push(next);
        self.combat.bounces.insert(id, BounceChain { attacker, weapon, next, hit, raw, counter });
        self.arm(delay, attacker, AttackMsg::Bounce { id });
    }

    /// The flight time of a bounce between two points: the path request's horizontal distance over the speed.
    fn flight_between(&self, a: (F32, F32), b: (F32, F32), speed: F32) -> F32 {
        let Some(tables) = self.order_tables() else {
            return F32::ZERO;
        };
        let origin = match self.world_bounds().or_else(|| self.playable()) {
            Some(p) => (p.min_x, p.min_y),
            None => (F32::ZERO, F32::ZERO),
        };
        missiles::flight_duration(a, b, origin, speed, crate::movement::LAYER_HORIZON, tables)
    }

    /// The bounce missile arrives at its next target: the damage of the chain, then the next leg.
    fn bounce_hit(&mut self, unit: ObjKey, id: u32) -> Vec<ObjKey> {
        let Some(chain) = self.combat.bounces.remove(&id) else {
            return Vec::new();
        };
        let tid = self.unit(chain.attacker).type_id;
        let Some(extra) = self.weapon_extra(tid, chain.weapon) else {
            return Vec::new();
        };
        let ty = self.weapon_type(tid, chain.weapon).unwrap_or_default();
        let mut died = Vec::new();
        if !self.target_valid(unit, chain.next) {
            return died;
        }
        let amount = self.attack_side(chain.next, &ty, chain.raw);
        if let Some(d) = self.apply_damage(chain.next, amount) {
            died.push(d);
        }
        let mut counter = chain.counter;
        if let Some(raw) = bounce::after_hit(&mut counter, chain.raw, extra.damage_loss) {
            self.bounce_next(chain.attacker, chain.next, chain.hit, raw, counter, chain.weapon, &extra);
        }
        died
    }

    /// Whether the swing predicate holds for `unit` and `target` with the attacker's weapon `ty`.
    fn in_attack_range(&self, unit: ObjKey, target: ObjKey, ty: &AttackType) -> bool {
        let a = self.unit(unit);
        let b = self.unit(target);
        let other = self.attack_type(b.type_id).collision;
        swing_in_range(a.x, a.y, b.x, b.y, ty.range, ty.collision, other)
    }

    /// Whether the unit is at `goal` (within the mover's arrival distance).
    fn at_point(&self, unit: ObjKey, goal: (F32, F32)) -> bool {
        let u = self.unit(unit);
        let d = arith::dist2(
            scale_by_1_32(u.x),
            scale_by_1_32(u.y),
            scale_by_1_32(goal.0),
            scale_by_1_32(goal.1),
        );
        arith::le(d, arith::mul(ARRIVE_CELLS, ARRIVE_CELLS))
    }

    /// Arms a one-shot attack timer for `unit` after `seconds`.
    fn arm(&mut self, seconds: F32, unit: ObjKey, msg: AttackMsg) -> TimerId {
        self.timers
            .start(seconds.to_f32(), false, Payload::Attack { unit, msg })
    }

    /// Cancels a timer id that may be empty.
    fn disarm(&mut self, id: Option<TimerId>) {
        if let Some(id) = id {
            self.timers.cancel(id);
        }
    }

    /// The attack object of `unit`, created when missing.
    fn st(&mut self, unit: ObjKey) -> &mut AttackObj {
        self.combat.states.entry(unit).or_default()
    }

    /// A copy of the attack object of `unit` (empty when it has none).
    fn snap(&self, unit: ObjKey) -> AttackObj {
        self.combat.states.get(&unit).cloned().unwrap_or_default()
    }

    /// The attack data of the unit's type.
    fn attack_type_of(&self, unit: ObjKey) -> AttackType {
        let w = self.snap(unit).weapon;
        let tid = self.unit(unit).type_id;
        self.weapon_type(tid, w).unwrap_or_else(|| self.attack_type(tid))
    }

    /// The position of unit `k` (world units).
    fn pos(&self, k: ObjKey) -> (F32, F32) {
        let u = self.unit(k);
        (u.x, u.y)
    }

    /// Whether unit `unit` may still act: alive and not removed.
    fn attacker_live(&self, unit: ObjKey) -> bool {
        matches!(self.object(unit), Object::Unit(u) if !u.dead && !u.removed && u.bonuses.stun == 0)
    }

    /// Whether `unit` may attack `target`: a live unit other than itself that is not invulnerable. The original's
    /// attack validator (`0x6F0CBD70` -> `0x6F0C93E0`) matches the target's classification against the weapon's
    /// `targs`; an invulnerable target (ability `Avul`, e.g. a neutral shop or mercenary camp) is only accepted by a
    /// weapon whose targs name `invulnerable`, which no melee-map unit has (the sim has no targs check otherwise,
    /// UNSURE for the other classes). Without this gate a captain's members attack an invulnerable camp forever.
    fn attackable(&self, unit: ObjKey, target: ObjKey) -> bool {
        self.target_valid(unit, target)
            && !self.unit_is_invulnerable(target)
            && self.choose_weapon(unit, target).is_some()
    }

    /// Whether `target` is a live unit other than `unit`.
    fn target_valid(&self, unit: ObjKey, target: ObjKey) -> bool {
        target != unit && matches!(self.object(target), Object::Unit(u) if !u.dead && !u.removed)
    }
}

/// The bearing of the vector `(dx, dy)` in `[0, 2 Pi)` (the mover's heading convention, movement.md 4.1).
fn bearing(dx: F32, dy: F32, tables: &Tables) -> F32 {
    let a = math::atan2(dy, dx, tables);
    if arith::lt(a, F32::ZERO) {
        arith::add(a, TWO_PI)
    } else {
        a
    }
}

/// A delayed `UnitDamagePoint` comes due: the damage runs and the units it kills get their death events with the
/// source as the killer (LIKELY).
pub fn expire_point_damage(game: &RefCell<Game>, rt: &mut dyn ScriptRuntime, id: u32) {
    let (died, source) = {
        let mut gm = game.borrow_mut();
        let Some(pd) = gm.take_point_damage(id) else {
            return;
        };
        (gm.apply_point_damage(&pd), pd.source)
    };
    for t in died {
        dispatch_unit_death_by(game, rt, t, Some(source));
    }
}

/// Handles the timer of an attack object: the one place the event queue meets the combat state. Called by the script
/// host when an [`Payload::Attack`] entry is due, with the game time set to its expiry. A death that the timer causes
/// runs the death dispatch after the game borrow is released. The attacker that dealt the killing damage is the
/// killer of that death (`GetKillingUnit`, the hand-off of the attacker through `dispatch_unit_death_by`).
pub fn expire_attack_timer(
    game: &RefCell<Game>,
    rt: &mut dyn ScriptRuntime,
    unit: ObjKey,
    msg: AttackMsg,
) {
    let died = game.borrow_mut().combat_fire(unit, msg);
    for target in died {
        dispatch_unit_death_by(game, rt, target, Some(unit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f32) -> F32 {
        F32::from_bits(v.to_bits())
    }

    #[test]
    fn swing_range_is_strict_at_the_boundary() {
        // range 128 world units = 4 cells, collisions 0.5 + 0.5: reach 5 cells, so the boundary is 160 world units
        // along an axis (D = 25 cells^2 is not below 25).
        let (r, c) = (f(128.0), f(0.5));
        assert!(swing_in_range(f(0.0), f(0.0), f(159.0), f(0.0), r, c, c));
        assert!(!swing_in_range(f(0.0), f(0.0), f(160.0), f(0.0), r, c, c));
        assert!(!swing_in_range(f(0.0), f(0.0), f(161.0), f(0.0), r, c, c));
    }

    #[test]
    fn swing_range_zero_reach_never_hits() {
        // With no range and no collision the reach is zero and D < 0 is never true, even for coincident units.
        let z = F32::ZERO;
        assert!(!swing_in_range(z, z, z, z, z, z, z));
    }

    #[test]
    fn messages_keep_the_spec_ids() {
        assert_eq!(AttackMsg::Reaction.spec_id(), Some(0xD01AF));
        assert_eq!(AttackMsg::SwingEnd.spec_id(), Some(0xD01B2));
        assert_eq!(AttackMsg::Cooldown.spec_id(), Some(0xD01B0));
        assert_eq!(AttackMsg::Execute.spec_id(), Some(0xD01B1));
        assert_eq!(AttackMsg::Scan.spec_id(), Some(0xD01C1));
        assert_eq!(REACTION_STREAM, 9);
    }
}
