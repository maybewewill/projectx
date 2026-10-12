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
use crate::natives::{circle_candidates, dispatch_unit_death_by, set_life, ScriptRuntime};
use crate::orders::{Order, OrderClass, ORDER_AIMOVE, ORDER_ATTACK};
use crate::pathing::scale_by_1_32;
use crate::missiles;
use crate::timer::TimerId;

/// Message id of the reaction delay end (`0xD01AF`, CONFIRMED).
pub const MSG_REACTION: u32 = 0x000D_01AF;
/// Message id of the swing end (`0xD01B0`, CONFIRMED value; its meaning as the backswing end is LIKELY).
pub const MSG_SWING_END: u32 = 0x000D_01B0;
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

    /// The amount of life `target` loses from a script damage event (`UnitDamageTarget`): the target side of the
    /// record the native builds. The default passes the raw amount through (the placeholder model has no armor).
    fn native_hit(&mut self, _game: &Game, hit: &NativeHit) -> F32 {
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
    Impact { target: ObjKey, damage: F32 },
}

impl AttackMsg {
    /// The spec's message id of this timer, if the spec names one.
    pub fn spec_id(self) -> Option<u32> {
        match self {
            AttackMsg::Scan => Some(MSG_SCAN),
            AttackMsg::Reaction => Some(MSG_REACTION),
            AttackMsg::SwingEnd => Some(MSG_SWING_END),
            AttackMsg::Execute => Some(MSG_EXECUTE),
            AttackMsg::Cooldown | AttackMsg::Impact { .. } => None,
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
    defense: BTreeMap<u32, Defense>,
    states: BTreeMap<ObjKey, AttackObj>,
    misc: CombatMisc,
    damage: Option<Box<dyn DamageModel>>,
    point_damages: BTreeMap<u32, PointDamage>,
    next_point_damage: u32,
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

    /// The attack data of unit type `type_id`; the all-zero value for an unregistered type.
    pub fn attack_type(&self, type_id: u32) -> AttackType {
        self.combat.types.get(&type_id).copied().unwrap_or_default()
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
            if !self.target_valid(unit, t) {
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

    /// Starts a swing: the cooldown, the execute point and the swing end.
    fn start_swing(&mut self, unit: ObjKey, ty: &AttackType) {
        let cooldown = self.arm(self.swing_cooldown(unit, ty.cooldown), unit, AttackMsg::Cooldown);
        let execute = self.arm(ty.damage_point, unit, AttackMsg::Execute);
        let swing = arith::add(ty.damage_point, ty.backswing);
        let swing_end = self.arm(swing, unit, AttackMsg::SwingEnd);
        let st = self.st(unit);
        st.cooldown = Some(cooldown);
        st.execute = Some(execute);
        st.swing_end = Some(swing_end);
    }

    /// The cooldown of a new swing for `unit`: the weapon cooldown divided by the attack speed multiplier that buffs
    /// maintain (`crate::auras`; the multiplier is 1.0 without buffs, and the division needs the softfloat tables, so
    /// without them the cooldown is unscaled). LIKELY direction; the original also rescales the timers of a swing in
    /// progress (`0x6F0C9090` re-arms them in proportion), which is not modelled.
    fn swing_cooldown(&self, unit: ObjKey, cooldown: F32) -> F32 {
        let mult = self.unit(unit).bonuses.attack_mult;
        if mult == F32::ONE {
            return cooldown;
        }
        match self.order_tables() {
            Some(t) => arith::div(cooldown, mult, t),
            None => cooldown,
        }
    }

    /// Runs the handler of an attack timer. Returns the target that died from it, so the caller can run the death
    /// dispatch after its borrow is released.
    fn combat_fire(&mut self, unit: ObjKey, msg: AttackMsg) -> Option<ObjKey> {
        match msg {
            AttackMsg::Scan => {
                self.fire_scan(unit);
                None
            }
            AttackMsg::Reaction => {
                let st = self.st(unit);
                st.reaction = None;
                st.armed = true;
                self.combat_tick(unit);
                None
            }
            AttackMsg::SwingEnd => {
                self.st(unit).swing_end = None;
                self.combat_tick(unit);
                None
            }
            AttackMsg::Cooldown => {
                self.st(unit).cooldown = None;
                self.combat_tick(unit);
                None
            }
            AttackMsg::Execute => self.fire_execute(unit),
            AttackMsg::Impact { target, damage } => {
                self.pr_missile_impact();
                self.missile_arrive(unit, target, damage)
            }
        }
    }

    /// The execute point (`0xD01B1` to `0x6F0CF660`, CONFIRMED order): the damage is rolled, then applied at once
    /// (instant) or on impact (missile).
    fn fire_execute(&mut self, unit: ObjKey) -> Option<ObjKey> {
        self.st(unit).execute = None;
        if !self.attacker_live(unit) {
            return None;
        }
        let target = self.snap(unit).target?;
        if !self.target_valid(unit, target) {
            return None;
        }
        let ty = self.attack_type_of(unit);
        let damage = self.roll_damage(unit, target, &ty);
        match ty.missile_speed {
            None => self.apply_damage(target, damage),
            Some(speed) => {
                let delay = self.impact_delay(unit, target, speed);
                self.pr_missile_launch();
                self.arm(delay, unit, AttackMsg::Impact { target, damage });
                None
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
                !u.dead && !u.removed && !u.hidden && self.is_enemy(owner, u.owner)
            })
    }

    /// `IsPlayerEnemy(p, q)` (LIKELY: the passive relation bit of `p` for `q` is clear, and a player is never the
    /// enemy of itself). Neutral players are enemies by default (UNSURE).
    pub(crate) fn is_enemy(&self, p: u32, q: u32) -> bool {
        p != q && self.player(p).alliances[ALLIANCE_PASSIVE] & (1 << q) == 0
    }

    /// The damage of one execute, through the installed model.
    fn roll_damage(&mut self, attacker: ObjKey, target: ObjKey, ty: &AttackType) -> F32 {
        let mut model: Box<dyn DamageModel> = self
            .combat
            .damage
            .take()
            .unwrap_or_else(|| Box::new(PlaceholderDamage));
        let damage = model.roll(self, attacker, target, ty);
        self.combat.damage = Some(model);
        damage
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

    /// The flight time of a missile: the distance over the speed (see [`crate::missiles::flight_seconds`], LIKELY in
    /// form, UNSURE in value). Zero without the tables (the square root and the division need them).
    fn impact_delay(&self, attacker: ObjKey, target: ObjKey, speed: F32) -> F32 {
        let Some(tables) = self.order_tables() else {
            return F32::ZERO;
        };
        let (a, b) = (self.unit(attacker), self.unit(target));
        let distance = missiles::distance(arith::sub(a.x, b.x), arith::sub(a.y, b.y), tables);
        missiles::flight_seconds(distance, speed, tables)
    }

    /// The arrival of a missile (slot 47, [`crate::missiles::arrival`]): the rolled damage goes through when the target
    /// is still a live unit. The attacker may be dead or gone: the missile keeps its own target (CONFIRMED gate).
    fn missile_arrive(&mut self, unit: ObjKey, target: ObjKey, damage: F32) -> Option<ObjKey> {
        let present = self.target_valid(unit, target);
        match missiles::arrival(missiles::GateInputs::plain(
            missiles::FLAG_TARGET_BOUND,
            present,
        )) {
            missiles::Arrival::Damage { .. } => self.apply_damage(target, damage),
            missiles::Arrival::Blocked | missiles::Arrival::NoTarget => None,
        }
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
        self.attack_type(self.unit(unit).type_id)
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
    if let Some(target) = died {
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
        assert_eq!(AttackMsg::SwingEnd.spec_id(), Some(0xD01B0));
        assert_eq!(AttackMsg::Execute.spec_id(), Some(0xD01B1));
        assert_eq!(AttackMsg::Scan.spec_id(), Some(0xD01C1));
        assert_eq!(AttackMsg::Cooldown.spec_id(), None);
        assert_eq!(REACTION_STREAM, 9);
    }
}
