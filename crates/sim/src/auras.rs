//! Buff lifecycle, auras and the stat modifiers they apply (`docs/spec/buffs.md`).
//!
//! Labels as elsewhere: CONFIRMED = read in the disassembly and, where the text says so, run in Unicorn
//! (`tools/emu/buffs_emu.py`); LIKELY = strong inference; UNSURE = a guess or an open point.
//!
//! # What the original does (CONFIRMED structure)
//!
//! * **Emitter.** An aura ability (`CAbilityAura*`) is a listener object on its unit. When it starts
//!   (`CAbilityAura::vf57` at `0x6F053B30`) it arms a **repeating 2.0 s timer** (message `0xD01C1`, whose handler applies
//!   the aura to the owner again) and a **proximity listener around the owner** with radius `Area` of the current level
//!   and a **period of `DIV(2.0, 4.0) = 0.5 s`** (Unicorn: the soft DIV gives exactly `0x3F000000`). Each hit is the
//!   message `0xD01C7` carrying a unit; its handler applies the aura to that unit (`0x6F043770`).
//! * **Apply** (`0x6F043770`). Gates on the source (not loaded into a transport, not dead/removed; an illusion source
//!   emits to itself only), then the ability's target filter (`vf208`: the level's `targs` and the enabled test), then:
//!   the target's list is searched for a buff with the level's `BuffID`; found, the buff is **refreshed** (`vf211` ->
//!   buff `vf212`), otherwise a buff is created (`vf212` creator + `vf213` apply) and the strength is stored
//!   (`vf210` -> buff `vf216`).
//! * **Duration.** Every aura buff is applied with `I2R(4)` = **4.0 s** (`0x6F0D5010`) through the common duration
//!   branch (`0x6F0D4CB0`): flag `0x80` and a one-shot expiry timer (message `0xD01C4`, queue A). The set-duration rule
//!   (`0x6F0D4EF0`, 1,200 Unicorn cases) never shortens: a duration at or below zero cancels the timer and clears `0x80`; a
//!   positive one restarts the timer only when it exceeds the time left.
//! * **Refresh** (`CBuffAura::vf212` at `0x6F0D5070`, 2,400 Unicorn cases). With tolerance `0.001` (`0x6FAAE47C`):
//!   if the stored strength is greater than the new one, nothing happens (not even the duration); otherwise the duration
//!   is set to 4.0 and the strength is stored again when the new one is greater, or (equal) when the class answers yes
//!   in `vf215` (always no, except the Command and Trueshot buffs, which answer "the target is a hero"). The strongest aura
//!   of a kind wins; equal strengths refresh.
//! * **Effect on the unit.** Applying a buff class' effect is guarded by its own `applied` flag, so it is idempotent;
//!   the effect is undone by `vf207` when the buff is destroyed (`CBuff::vf41`). The stat sinks are: armor (`0x6F2769C0`
//!   adds to the unit's armor cell `+0xE0`), movement (`0x6F202FF0` adds to a multiplier that starts at 1.0, snapping to 1.0 within
//!   0.001; the speed is clamp(base times multiplier)), attack speed (`0x6F0C9090`, the same multiplier rule on the attack
//!   ability) and regeneration (a rate modifier object).
//! * **Slow and stun** (`CBuffSlow`, `CBuffStun`). A slow stores two factors, a move and an attack bonus (both negative)
//!   and *replaces* them on every refresh (`vf214` has no strength compare); a stun counts on the unit
//!   (`0x6F296680` / `0x6F284A50`), so `IsUnitType(STUNNED)` is true while a `BSTN` buff exists.
//!
//! # What the model does
//!
//! The behaviour above on the simulation's own timers and units, with these stand-ins (all UNSURE): the weapon filter
//! of the damage auras is not modelled (the bonus goes to the unit's one attack); the percent variants use the unit
//! type's armor (Devotion), the unit's max life (Unholy) or max mana (Brilliance); the attack-speed multiplier scales the
//! cooldown of new swings only (the original also rescales the timers of a swing in progress); regeneration is stored as a
//! per-second rate (`Game::tick_regen` applies it when the host calls it; the simulation has no base regeneration).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN semantics follow the original's x87 compares

use std::collections::BTreeMap;

use softfloat::{arith, conv, F32};

use crate::abilities::{code, Buff, BuffPredicates};
use crate::natives::circle_candidates;
use crate::natives::game::{Game, ObjKey, Payload};
use crate::timer::TimerId;

/// Movement type bits (`unit +0x1FC`): flying, and the ground movers (foot, horse, hover).
const MOVE_FLY: u32 = 0x2;
const MOVE_GROUND: u32 = 0x1 | 0x4 | 0x8;

/// The comparison tolerance `0x6FAAE47C` after start-up: `0.001`.
pub const TOLERANCE: F32 = F32::from_bits(0x3A83_126F);
/// The duration of an aura buff: `I2R(4)`.
pub const AURA_BUFF_DURATION: F32 = F32::from_bits(0x4080_0000);
/// The period of the repeating timer that applies the aura to its owner: `I2R(2)`.
pub const AURA_PULSE_PERIOD: F32 = F32::from_bits(0x4000_0000);
/// The period of the proximity listener: soft `DIV(2.0, 4.0)`, bit-exact `0.5` (Unicorn).
pub const AURA_SCAN_PERIOD: F32 = F32::from_bits(0x3F00_0000);
/// Message id of the owner re-apply timer (`0xD01C1`), of a proximity hit (`0xD01C7`) and of a buff expiry (`0xD01C4`).
pub const MSG_PULSE: u32 = 0xD01C1;
pub const MSG_PROXIMITY: u32 = 0xD01C7;
pub const MSG_EXPIRE: u32 = 0xD01C4;

/// `'BSTN'`: the stun buff.
pub const STUN: u32 = code(b"BSTN");
/// `'Bslo'`: the slow buff.
pub const SLOW: u32 = code(b"Bslo");

// ---------------------------------------------------------------------------------------------------------------
// The strength comparisons (CONFIRMED, 3,600 Unicorn cases: `0x6F07C450`, `0x6F0D2290`, `0x6F0D2380`).

/// `|new - stored|` through the soft-float subtract (the sign bit cleared), as the three routines compute it.
fn abs_diff(new: F32, stored: F32) -> f32 {
    f32::from_bits(arith::sub(new, stored).to_bits() & 0x7FFF_FFFF)
}

/// Whether the new strength exceeds the stored one by the tolerance (`0x6F07C450`): the stored value is below the new one
/// (an ordered compare, so NaN is false) and `0.001 <= |new - stored|` (the original's compare also passes an unordered
/// difference).
pub fn new_exceeds(new: F32, stored: F32) -> bool {
    stored.to_f32() < new.to_f32() && !(TOLERANCE.to_f32() > abs_diff(new, stored))
}

/// Whether the stored strength exceeds the new one by the tolerance (`0x6F0D2290`).
pub fn stored_exceeds(new: F32, stored: F32) -> bool {
    stored.to_f32() > new.to_f32() && !(TOLERANCE.to_f32() > abs_diff(new, stored))
}

/// Whether the two strengths are within the tolerance (`0x6F0D2380`): `0.001 > |new - stored|`.
pub fn within_tolerance(new: F32, stored: F32) -> bool {
    TOLERANCE.to_f32() > abs_diff(new, stored)
}

/// What `CBuffAura::vf212` (`0x6F0D5070`) does with one stored strength: `(refresh the duration, store the new value)`.
/// `equal_stores` is the answer of the class' `vf215`.
pub fn refresh_decision(new: F32, stored: F32, equal_stores: bool) -> (bool, bool) {
    if stored_exceeds(new, stored) {
        return (false, false);
    }
    let store = new_exceeds(new, stored) || (within_tolerance(new, stored) && equal_stores);
    (true, store)
}

/// The refresh rule of a buff with several strengths (Endurance, Unholy: `0x6F1B6400`, `0x6F1B6750`): nothing happens
/// only when every stored strength exceeds the new one; otherwise the duration is set, and each strength that the new one
/// exceeds is replaced. Returns `(refresh the duration, per component: replace)`.
pub fn refresh_multi(new: &[F32], stored: &[F32]) -> (bool, Vec<bool>) {
    let all_stronger = new
        .iter()
        .zip(stored)
        .all(|(&n, &s)| stored_exceeds(n, s));
    if all_stronger {
        return (false, vec![false; new.len()]);
    }
    let replace = new
        .iter()
        .zip(stored)
        .map(|(&n, &s)| new_exceeds(n, s))
        .collect();
    (true, replace)
}

/// The set-duration rule of `CBuff::vf212` (`0x6F0D4EF0`, 1,200 Unicorn cases): `(duration kept on, restart the timer)`.
/// A duration at or below zero (or NaN) cancels the timer and clears flag `0x80`; a positive one sets the flag and
/// restarts the timer only when it exceeds the time left. `remaining` is the timer's time left (ignored when none runs).
pub fn set_duration_rule(duration: F32, remaining: Option<F32>) -> (bool, bool) {
    if !(duration.to_f32() > 0.0) {
        return (false, false);
    }
    let restart = match remaining {
        Some(r) => duration.to_f32() > r.to_f32(),
        // No timer runs: the timer holder answers a zero time left (a stopped timer), so any positive duration restarts it.
        None => true,
    };
    (true, restart)
}

// ---------------------------------------------------------------------------------------------------------------
// Aura classes and their data.

/// The aura classes the model knows (the melee set). Each is a `CAbilityAura*` ability with a `CBuffAura*` buff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuraKind {
    /// `AHad` (Devotion Aura): armor `DataA`; `DataB != 0` makes it a fraction of the unit type's armor.
    Devotion,
    /// `AOae` (Endurance Aura): movement `DataA`, attack speed `DataB`.
    Endurance,
    /// `AUau` (Unholy Aura): movement `DataA`, life regeneration `DataB`; `DataC != 0` makes the regeneration a fraction
    /// of the max life (UNSURE which stat).
    Unholy,
    /// `AHab` (Brilliance Aura): mana regeneration `DataA`; `DataB != 0` makes it a fraction of the max mana.
    Brilliance,
    /// `AOac` (Command Aura): damage bonus `DataA` (a fraction of the base damage; `DataD != 0` makes it flat).
    Command,
    /// `AEar` (Trueshot Aura): the same, for the attacker's weapon.
    Trueshot,
    /// `AUav` (Vampiric Aura, S28): the buff carries the life steal fraction `DataA` (`CBuffAuraVampiric` `+0xD0`); its
    /// effect is the damage handler `undead_ne::vampiric_heal`, not a stat sink.
    Vampiric,
}

impl AuraKind {
    /// The kind of ability `base`.
    pub fn of(base: u32) -> Option<AuraKind> {
        Some(match base {
            b if b == code(b"AHad") => AuraKind::Devotion,
            b if b == code(b"AOae") => AuraKind::Endurance,
            b if b == code(b"AUau") => AuraKind::Unholy,
            b if b == code(b"AHab") => AuraKind::Brilliance,
            b if b == code(b"AOac") => AuraKind::Command,
            b if b == code(b"AEar") => AuraKind::Trueshot,
            b if b == code(b"AUav") => AuraKind::Vampiric,
            _ => return None,
        })
    }

    /// The ability code of the kind.
    pub fn ability(self) -> u32 {
        match self {
            AuraKind::Devotion => code(b"AHad"),
            AuraKind::Endurance => code(b"AOae"),
            AuraKind::Unholy => code(b"AUau"),
            AuraKind::Brilliance => code(b"AHab"),
            AuraKind::Command => code(b"AOac"),
            AuraKind::Trueshot => code(b"AEar"),
            AuraKind::Vampiric => code(b"AUav"),
        }
    }

    /// The default buff code (the level's `BuffID` overrides it). UNSURE: the stock ids follow the `Bxxx` naming.
    pub fn default_buff(self) -> u32 {
        match self {
            AuraKind::Devotion => code(b"BHad"),
            AuraKind::Endurance => code(b"BOae"),
            AuraKind::Unholy => code(b"BUau"),
            AuraKind::Brilliance => code(b"BHab"),
            AuraKind::Command => code(b"BOac"),
            AuraKind::Trueshot => code(b"BEar"),
            // CONFIRMED: the code getter of `CBuffAuraVampiric` (`0x6F196740`) returns 'BUav'.
            AuraKind::Vampiric => code(b"BUav"),
        }
    }

    /// The number of strengths the buff stores (CONFIRMED: Endurance and Unholy store two, the others one).
    pub fn values(self) -> usize {
        match self {
            AuraKind::Endurance | AuraKind::Unholy => 2,
            _ => 1,
        }
    }
}

/// One level of an aura ability: the fields of the level record the aura reads.
#[derive(Debug, Clone, PartialEq)]
pub struct AuraLevel {
    /// `Area`: the radius of the proximity listener.
    pub area: F32,
    /// `DataA` to `DataD`.
    pub data: [F32; 4],
    /// The buff code (`BuffID`), 0 for the kind's default.
    pub buff: u32,
    /// The `targs` text (`air,ground,friend,self,...`).
    pub targs: String,
}

impl Default for AuraLevel {
    fn default() -> Self {
        AuraLevel {
            area: F32::ZERO,
            data: [F32::ZERO; 4],
            buff: 0,
            targs: String::new(),
        }
    }
}

/// The aura ability data by ability code (index 0 is level 1). Filled by the host from the ability table.
#[derive(Debug, Clone, Default)]
pub struct AuraTypes {
    by_ability: BTreeMap<u32, Vec<AuraLevel>>,
}

impl AuraTypes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the levels of ability `base`.
    pub fn insert(&mut self, base: u32, levels: Vec<AuraLevel>) {
        self.by_ability.insert(base, levels);
    }

    /// Level `index` (zero based) of ability `base`; the last level when the index is past the end.
    pub fn level(&self, base: u32, index: i32) -> Option<&AuraLevel> {
        let levels = self.by_ability.get(&base)?;
        let i = usize::try_from(index.max(0)).unwrap_or(0);
        levels.get(i).or_else(|| levels.last())
    }

    /// Whether data for `base` exists.
    pub fn contains(&self, base: u32) -> bool {
        self.by_ability.contains_key(&base)
    }
}

/// The target filter of the aura (`vf208` -> `0x6F26D590`), reduced to the tokens of the stock `targs` text.
/// UNSURE: the original decodes a flag word; the relation tokens (`self`, `player`, `friend`/`allies`, `enemy`,
/// `neutral`) must match one of those listed, the kind tokens (`air`, `ground`, `structure`) one of those listed
/// (none listed: any), and an invulnerable target needs `invu`.
pub fn targs_match(game: &Game, source: ObjKey, target: ObjKey, targs: &str) -> bool {
    let tokens: Vec<&str> = targs
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return true;
    }
    let has = |names: &[&str]| tokens.iter().any(|t| names.iter().any(|n| t.eq_ignore_ascii_case(n)));
    let s = game.unit(source);
    let t = game.unit(target);
    let owner = s.owner;
    let relation_listed = has(&["self", "player", "friend", "allies", "ally", "enemy", "enemies", "neutral"]);
    let relation_ok = !relation_listed
        || (has(&["self"]) && source == target)
        || (has(&["player"]) && t.owner == owner)
        || (has(&["friend", "allies", "ally"]) && (t.owner == owner || !game.is_enemy(owner, t.owner)))
        || (has(&["enemy", "enemies"]) && game.is_enemy(owner, t.owner))
        || (has(&["neutral"]) && t.owner >= 12);
    if !relation_ok {
        return false;
    }
    let kind_listed = has(&["air", "ground", "structure"]);
    let ty = game.unit_types().get(t.type_id);
    let movement = ty.map_or(0, |ty| ty.movement);
    let structure = game.unit_types().is_building(t.type_id);
    let kind_ok = !kind_listed
        || (has(&["air"]) && movement & MOVE_FLY != 0)
        || (has(&["ground"])
            && movement & MOVE_GROUND != 0
            && !structure)
        || (has(&["structure"]) && structure);
    if !kind_ok {
        return false;
    }
    if game.unit_is_invulnerable(target) && !has(&["invu", "invulnerable"]) {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------------------------------------------
// Buff effects.

/// The stat modifiers of a unit that buffs write (CONFIRMED sinks, see the module notes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatBonuses {
    /// Armor added to the unit's armor value (`0x6F2769C0`).
    pub armor: F32,
    /// The movement multiplier, 1.0 when unmodified (`0x6F202FF0`).
    pub move_mult: F32,
    /// The attack speed multiplier, 1.0 when unmodified (`0x6F0C9090`).
    pub attack_mult: F32,
    /// Life regeneration per second added by buffs.
    pub life_regen: F32,
    /// Mana regeneration per second added by buffs.
    pub mana_regen: F32,
    /// Damage added to the dice bonus of the unit's attacks.
    pub damage: i32,
    /// The number of stun buffs on the unit.
    pub stun: u32,
}

impl Default for StatBonuses {
    fn default() -> Self {
        StatBonuses {
            armor: F32::ZERO,
            move_mult: F32::ONE,
            attack_mult: F32::ONE,
            life_regen: F32::ZERO,
            mana_regen: F32::ZERO,
            damage: 0,
            stun: 0,
        }
    }
}

/// `x` snapped to 1.0 when it is within the tolerance of it (the multiplier sinks `0x6F202FF0` and `0x6F0C9090`).
fn snap_to_one(x: F32) -> F32 {
    let d = f32::from_bits(arith::sub(F32::ONE, x).to_bits() & 0x7FFF_FFFF);
    if TOLERANCE.to_f32() <= d {
        x
    } else {
        F32::ONE
    }
}

/// An aura buff on a unit.
#[derive(Debug, Clone, PartialEq)]
pub struct AuraBuff {
    pub kind: AuraKind,
    /// The unit whose ability created the buff.
    pub source: ObjKey,
    /// The stored strengths (`+0xD0`, `+0xE8`...). Component meaning per kind: see [`AuraKind`].
    pub values: [F32; 2],
    /// The flags the creator passes (`DataB`/`DataC`/`DataD` nonzero), meaning per kind.
    pub flags: [bool; 3],
    /// Which components currently have their effect on the unit, and the amount added (undone exactly on removal).
    pub applied: [Option<F32>; 2],
    /// The damage bonus currently added (Command and Trueshot).
    pub applied_damage: Option<i32>,
    /// The regeneration modifier the buff owns (Unholy life, Brilliance mana: `0x6F1AD810`, `0x6F1AC990`); a forever modifier on
    /// the unit's life or mana property, released (delayed destroy) when the effect is taken off.
    pub regen_mod: Option<crate::agile::ModId>,
}

/// What a buff does to its unit beyond being listed.
#[derive(Debug, Clone, PartialEq)]
pub enum BuffEffect {
    Aura(AuraBuff),
    /// A slow: the move and attack factors (negative), and whether they are applied.
    Slow {
        move_factor: F32,
        attack_factor: F32,
        applied: bool,
    },
    /// A stun: counted on the unit while the buff exists.
    Stun,
    /// Avatar (`CBuffAvatar`): the armor, maximum life and damage it added, and one magic immunity count
    /// (`docs/spec/abilities-human-heroes.md` section 4).
    Avatar {
        armor: F32,
        life: F32,
        damage: i32,
    },
    /// Divine Shield (`CBuffDivineShield`): one invulnerability count and one magic immunity count.
    DivineShield,
}

/// The state of one running aura emitter (a unit's aura ability).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraEmitter {
    pub pulse: TimerId,
    pub scan: TimerId,
}

impl Game {
    /// The aura data registry (read only).
    pub fn aura_types(&self) -> &AuraTypes {
        &self.aura_types
    }

    /// The aura data registry, for the host and the tests.
    pub fn aura_types_mut(&mut self) -> &mut AuraTypes {
        &mut self.aura_types
    }

    /// Whether `unit` is stunned: a `BSTN` buff counted by the aura/buff model, or a spell stun of the cast core
    /// (`cast.rs`, which keeps its own buff entries until the two models are merged).
    pub fn is_stunned(&self, unit: ObjKey) -> bool {
        self.unit(unit).bonuses.stun > 0 || self.is_spell_stunned(unit)
    }

    /// The speed of `unit` as the mover uses it: the base speed times the buff multiplier (LIKELY: the speed recompute
    /// `0x6F2019F0` clamps base times the multiplier at `+0x78`; the flat part at `+0x70` is not modelled).
    pub fn effective_move_speed(&self, unit: ObjKey) -> F32 {
        let u = self.unit(unit);
        if u.bonuses.move_mult == F32::ONE {
            u.move_speed
        } else {
            arith::mul(u.move_speed, u.bonuses.move_mult)
        }
    }

    /// The life steal fraction of `unit`'s Vampiric buff (`'BUav'`), if it has one.
    pub fn vampiric_fraction(&self, unit: ObjKey) -> Option<F32> {
        self.unit(unit).buffs.iter().find_map(|b| match &b.effect {
            Some(BuffEffect::Aura(a)) if a.kind == AuraKind::Vampiric => Some(a.values[0]),
            _ => None,
        })
    }

    /// The damage bonus buffs add to the dice bonus of the unit's attacks.
    pub fn buff_damage_bonus(&self, unit: ObjKey) -> i32 {
        self.unit(unit).bonuses.damage
    }

    /// The (life, mana) regeneration per second that buffs add to `unit`.
    pub fn buff_regen(&self, unit: ObjKey) -> (F32, F32) {
        let b = &self.unit(unit).bonuses;
        (b.life_regen, b.mana_regen)
    }

    /// Applies the regeneration rates of all live units for `seconds`: life and mana grow by rate times seconds, capped at
    /// the maximum. Not called by the simulation itself (it has no base regeneration, so enabling this alone would regenerate
    /// only the buff part); the host opts in.
    pub fn tick_regen(&mut self, seconds: F32) {
        let keys: Vec<ObjKey> = (0..self.object_count())
            .map(|i| ObjKey(i as u32))
            .filter(|&k| matches!(self.object(k), crate::natives::game::Object::Unit(_)))
            .collect();
        for k in keys {
            let u = self.unit(k);
            if u.dead || u.removed || u.stats.is_some() {
                // Units with stats regenerate through the agile engine (`crate::regen`).
                continue;
            }
            let (lr, mr) = (u.bonuses.life_regen, u.bonuses.mana_regen);
            if lr.to_bits() & 0x7FFF_FFFF != 0 {
                let add = arith::mul(lr, seconds);
                let u = self.unit_mut(k);
                let v = arith::add(u.life, add);
                u.life = if arith::gt(v, u.max_life) { u.max_life } else { v };
            }
            if mr.to_bits() & 0x7FFF_FFFF != 0 {
                let add = arith::mul(mr, seconds);
                let u = self.unit_mut(k);
                let v = arith::add(u.mana, add);
                u.mana = if arith::gt(v, u.max_mana) { u.max_mana } else { v };
            }
        }
    }

    // --- stat sinks ---------------------------------------------------------------------------------------------

    pub(crate) fn add_armor(&mut self, unit: ObjKey, delta: F32) {
        let u = self.unit_mut(unit);
        u.bonuses.armor = arith::add(u.bonuses.armor, delta);
    }

    fn add_move(&mut self, unit: ObjKey, delta: F32) {
        let u = self.unit_mut(unit);
        u.bonuses.move_mult = snap_to_one(arith::add(u.bonuses.move_mult, delta));
        self.mover_speed_changed(unit);
    }

    fn add_attack_speed(&mut self, unit: ObjKey, delta: F32) {
        // With the soft-float tables the bonus also rescales the pending swing timers (`0x6F0C9090`).
        if self.combat_add_attack_speed(unit, delta) {
            return;
        }
        let u = self.unit_mut(unit);
        u.bonuses.attack_mult = snap_to_one(arith::add(u.bonuses.attack_mult, delta));
    }

    fn add_life_regen(&mut self, unit: ObjKey, delta: F32) {
        let u = self.unit_mut(unit);
        u.bonuses.life_regen = arith::add(u.bonuses.life_regen, delta);
    }

    fn add_mana_regen(&mut self, unit: ObjKey, delta: F32) {
        let u = self.unit_mut(unit);
        u.bonuses.mana_regen = arith::add(u.bonuses.mana_regen, delta);
    }

    pub(crate) fn add_damage(&mut self, unit: ObjKey, delta: i32) {
        let u = self.unit_mut(unit);
        u.bonuses.damage = u.bonuses.damage.saturating_add(delta);
    }

    // --- aura buff effects --------------------------------------------------------------------------------------

    /// Whether the aura effect on `unit` is suspended (`0x6F1ACF90`: the unit is an illusion).
    fn aura_suspended(&self, unit: ObjKey) -> bool {
        self.unit(unit).illusion
    }

    /// The amount component `i` of aura buff `a` adds to `unit` (CONFIRMED structure: a flag makes the value a multiple of
    /// a unit stat). `None` for a component the kind does not have.
    fn aura_amount(&self, unit: ObjKey, a: &AuraBuff, i: usize) -> Option<F32> {
        let v = a.values[i];
        match (a.kind, i) {
            (AuraKind::Devotion, 0) => Some(if a.flags[0] {
                // 0x6F29BF60: the type record's armor (+0x90).
                arith::mul(v, self.defense_of_type(self.unit(unit).type_id).armor)
            } else {
                v
            }),
            (AuraKind::Endurance, 0 | 1) | (AuraKind::Unholy, 0) => Some(v),
            (AuraKind::Unholy, 1) => Some(if a.flags[1] {
                arith::mul(v, self.unit(unit).max_life)
            } else {
                v
            }),
            (AuraKind::Brilliance, 0) => Some(if a.flags[0] {
                arith::mul(v, self.unit(unit).max_mana)
            } else {
                v
            }),
            _ => None,
        }
    }

    /// The damage bonus a Command or Trueshot buff gives `unit` (`0x6F1ACC40`): flat `trunc(value)` when flag D is set,
    /// otherwise `round(value times the base damage)` (`f2i(add(mul(v, base), 0.5))`). The base damage is the weapon's
    /// minimum roll, dice plus bonus (UNSURE; the original reads a weapon base field).
    fn aura_damage_amount(&self, unit: ObjKey, a: &AuraBuff) -> i32 {
        if a.flags[2] {
            return conv::f2i(a.values[0]);
        }
        let ty = self.attack_type(self.unit(unit).type_id);
        let base = conv::i2r(ty.dice_bonus.saturating_add(i32::try_from(ty.dice).unwrap_or(0)));
        let half = F32::from_bits(0x3F00_0000);
        conv::f2i(arith::add(arith::mul(a.values[0], base), half))
    }

    /// Puts the aura buff's effects on `unit` (idempotent per component: `0x6F1ACFD0`, `0x6F1AD240`, `0x6F1AD150`...).
    fn aura_apply_effects(&mut self, unit: ObjKey, buff_index: usize) {
        if self.aura_suspended(unit) {
            return;
        }
        let Some(BuffEffect::Aura(a)) = self.unit(unit).buffs[buff_index].effect.clone() else {
            return;
        };
        let mut a = a;
        for i in 0..a.kind.values() {
            if a.applied[i].is_some() {
                continue;
            }
            let Some(amount) = self.aura_amount(unit, &a, i) else {
                continue;
            };
            match (a.kind, i) {
                (AuraKind::Devotion, 0) => self.add_armor(unit, amount),
                (AuraKind::Endurance, 0) | (AuraKind::Unholy, 0) => self.add_move(unit, amount),
                (AuraKind::Endurance, 1) => self.add_attack_speed(unit, amount),
                (AuraKind::Unholy, 1) => {
                    self.add_life_regen(unit, amount);
                    a.regen_mod = self.add_life_regen_modifier(unit, amount);
                }
                (AuraKind::Brilliance, 0) => {
                    self.add_mana_regen(unit, amount);
                    a.regen_mod = self.add_mana_regen_modifier(unit, amount);
                }
                _ => continue,
            }
            a.applied[i] = Some(amount);
        }
        if matches!(a.kind, AuraKind::Command | AuraKind::Trueshot) && a.applied_damage.is_none() {
            let amount = self.aura_damage_amount(unit, &a);
            self.add_damage(unit, amount);
            a.applied_damage = Some(amount);
        }
        self.unit_mut(unit).buffs[buff_index].effect = Some(BuffEffect::Aura(a));
    }

    /// Takes the amounts an aura buff added off `unit` (`0x6F1AD0B0` and its siblings: the amount that was added is
    /// subtracted). The buff record `a` is updated (nothing applied afterwards).
    fn aura_undo(&mut self, unit: ObjKey, a: &mut AuraBuff) {
        for i in 0..2 {
            let Some(amount) = a.applied[i].take() else {
                continue;
            };
            let neg = arith::neg(amount);
            match (a.kind, i) {
                (AuraKind::Devotion, 0) => self.add_armor(unit, neg),
                (AuraKind::Endurance, 0) | (AuraKind::Unholy, 0) => self.add_move(unit, neg),
                (AuraKind::Endurance, 1) => self.add_attack_speed(unit, neg),
                (AuraKind::Unholy, 1) => self.add_life_regen(unit, neg),
                (AuraKind::Brilliance, 0) => self.add_mana_regen(unit, neg),
                _ => {}
            }
        }
        if let Some(d) = a.applied_damage.take() {
            self.add_damage(unit, d.saturating_neg());
        }
        if let Some(m) = a.regen_mod.take() {
            self.release_modifier(m);
        }
    }

    /// Takes the effects of the aura buff at `buff_index` off `unit`.
    fn aura_remove_effects(&mut self, unit: ObjKey, buff_index: usize) {
        let Some(BuffEffect::Aura(mut a)) = self.unit(unit).buffs[buff_index].effect.clone() else {
            return;
        };
        self.aura_undo(unit, &mut a);
        self.unit_mut(unit).buffs[buff_index].effect = Some(BuffEffect::Aura(a));
    }

    /// Undoes every effect of buff `b` of `unit` and cancels its expiry timer. The buff is already out of the list.
    pub(crate) fn finish_buff(&mut self, unit: ObjKey, b: &mut Buff) {
        if let Some(id) = b.expiry.take() {
            self.timers.cancel(id);
        }
        match b.effect.take() {
            Some(BuffEffect::Aura(mut a)) => self.aura_undo(unit, &mut a),
            Some(BuffEffect::Slow {
                move_factor,
                attack_factor,
                applied,
            }) => {
                if applied {
                    self.add_move(unit, arith::neg(move_factor));
                    self.add_attack_speed(unit, arith::neg(attack_factor));
                }
            }
            Some(BuffEffect::Stun) => {
                let u = self.unit_mut(unit);
                u.bonuses.stun = u.bonuses.stun.saturating_sub(1);
            }
            Some(BuffEffect::Avatar { armor, life, damage }) => self.undo_avatar(unit, armor, life, damage),
            Some(BuffEffect::DivineShield) => {
                let u = self.unit_mut(unit);
                u.invuln_count -= 1;
                u.magic_immune -= 1;
            }
            None => {}
        }
    }

    /// The buff expiry timer fired (message `0xD01C4`): the buff is removed and its effects undone (CONFIRMED path).
    pub(crate) fn expire_buff(&mut self, unit: ObjKey, serial: u32) {
        if self.unit(unit).removed {
            return;
        }
        let Some(i) = self.unit(unit).buffs.iter().position(|b| b.serial == serial) else {
            return;
        };
        let mut b = self.unit_mut(unit).buffs.remove(i);
        // The timer is the one that fired; do not cancel it again.
        b.expiry = None;
        self.finish_buff(unit, &mut b);
    }

    /// Starts (or restarts, per the set-duration rule) the expiry timer of buff `serial` on `unit` for `duration`.
    /// Returns whether the buff has a running duration (flag `0x80`).
    pub(crate) fn set_buff_duration(&mut self, unit: ObjKey, serial: u32, duration: F32) -> bool {
        let Some(i) = self.unit(unit).buffs.iter().position(|b| b.serial == serial) else {
            return false;
        };
        let now = self.timers.now();
        let current = self.unit(unit).buffs[i].expiry;
        let remaining = current
            .and_then(|id| self.timers.expiry_of(id))
            .map(|e| arith::sub(F32::from_bits(e.to_bits()), F32::from_bits(now.to_bits())));
        let (on, restart) = set_duration_rule(duration, remaining);
        if !on {
            if let Some(id) = self.unit_mut(unit).buffs[i].expiry.take() {
                self.timers.cancel(id);
            }
            return false;
        }
        if restart {
            if let Some(id) = self.unit_mut(unit).buffs[i].expiry.take() {
                self.timers.cancel(id);
            }
            let id = self
                .timers
                .start(duration.to_f32(), false, Payload::BuffExpire { unit, serial });
            self.unit_mut(unit).buffs[i].expiry = Some(id);
        }
        true
    }

    pub(crate) fn buff_position(&self, unit: ObjKey, base: u32) -> Option<usize> {
        self.unit(unit).buffs.iter().position(|b| b.base == base)
    }

    // --- aura emitter -------------------------------------------------------------------------------------------

    /// Whether the aura source `unit` may emit (the gates of `0x6F043770` on the source, reduced): alive and not removed.
    fn aura_source_active(&self, unit: ObjKey) -> bool {
        let u = self.unit(unit);
        !u.dead && !u.removed && !u.loaded
    }

    /// Applies the aura ability `base` of `source` to `target` (`0x6F043770`): the gates, the target filter, then refresh
    /// the existing buff of the level's buff code or create it. Returns whether the target has the buff afterwards.
    pub fn apply_aura(&mut self, source: ObjKey, base: u32, target: ObjKey) -> bool {
        let Some(kind) = AuraKind::of(base) else {
            return false;
        };
        if !self.aura_source_active(source) {
            return false;
        }
        let t = self.unit(target);
        if t.removed || t.dead {
            return false;
        }
        // An illusion source emits to itself only (`0x6F043770`: a2 == source or the source flags 0x40000000/0x1000000 clear).
        if source != target && self.unit(source).illusion {
            return false;
        }
        let Some(ability) = self.unit(source).abilities.get(base) else {
            return false;
        };
        let Some(level) = self.aura_types.level(base, ability.level_index).cloned() else {
            return false;
        };
        if !targs_match(self, source, target, &level.targs) {
            return false;
        }
        let buff_code = if level.buff != 0 { level.buff } else { kind.default_buff() };
        let d = &level.data;
        let values = match kind {
            AuraKind::Endurance | AuraKind::Unholy => [d[0], d[1]],
            _ => [d[0], F32::ZERO],
        };
        let nonzero = |x: F32| x.to_f32() != 0.0;
        let flags = match kind {
            AuraKind::Devotion | AuraKind::Brilliance => [nonzero(d[1]), false, false],
            AuraKind::Unholy => [false, nonzero(d[2]), false],
            AuraKind::Command | AuraKind::Trueshot => [nonzero(d[1]), nonzero(d[2]), nonzero(d[3])],
            AuraKind::Endurance | AuraKind::Vampiric => [false; 3],
        };
        match self.buff_position(target, buff_code) {
            Some(i) => self.refresh_aura_buff(source, target, i, kind, values, flags),
            None => self.create_aura_buff(source, target, buff_code, kind, values, flags),
        }
        true
    }

    fn create_aura_buff(
        &mut self,
        source: ObjKey,
        target: ObjKey,
        buff_code: u32,
        kind: AuraKind,
        values: [F32; 2],
        flags: [bool; 3],
    ) {
        let serial = self.next_buff_serial();
        self.unit_mut(target).buffs.push(Buff {
            serial,
            base: buff_code,
            predicates: BuffPredicates {
                positive: true,
                aura: true,
                ..BuffPredicates::default()
            },
            timed_life: None,
            effect: Some(BuffEffect::Aura(AuraBuff {
                kind,
                source,
                values,
                flags,
                applied: [None; 2],
                regen_mod: None,
                applied_damage: None,
            })),
            expiry: None,
        });
        // The duration branch (`0x6F0D4CB0`) with I2R(4), then the strength is stored and applied (`vf216`).
        self.set_buff_duration(target, serial, AURA_BUFF_DURATION);
        let i = self.unit(target).buffs.len() - 1;
        self.aura_apply_effects(target, i);
    }

    fn refresh_aura_buff(
        &mut self,
        _source: ObjKey,
        target: ObjKey,
        i: usize,
        kind: AuraKind,
        new: [F32; 2],
        flags: [bool; 3],
    ) {
        let Some(BuffEffect::Aura(stored)) = self.unit(target).buffs[i].effect.clone() else {
            return;
        };
        let n = kind.values();
        let serial = self.unit(target).buffs[i].serial;
        let (refresh, replace): (bool, Vec<bool>) = if n == 1 {
            let equal_stores = match kind {
                AuraKind::Command | AuraKind::Trueshot => crate::unitclass::id_is_hero(self.unit(target).type_id),
                _ => false,
            };
            let (r, s) = refresh_decision(new[0], stored.values[0], equal_stores);
            (r, vec![s])
        } else {
            refresh_multi(&new[..n], &stored.values[..n])
        };
        if !refresh {
            return;
        }
        self.set_buff_duration(target, serial, AURA_BUFF_DURATION);
        let Some(i) = self.unit(target).buffs.iter().position(|b| b.serial == serial) else {
            return;
        };
        if replace.iter().any(|&r| r) {
            // Remove the old amounts, store the new strengths (and flags), apply again (`vf218`).
            self.aura_remove_effects(target, i);
            if let Some(BuffEffect::Aura(a)) = self.unit_mut(target).buffs[i].effect.as_mut() {
                for (c, &rep) in replace.iter().enumerate() {
                    if rep {
                        a.values[c] = new[c];
                    }
                }
                a.flags = flags;
            }
            self.aura_apply_effects(target, i);
        }
    }

    /// The aura ability `base` of `unit` has data and the unit has it: starts its emitter when it is not running
    /// (`0x6F053B30`): the 2.0 s repeating timer, the 0.5 s proximity scan, and an immediate apply to the owner.
    pub fn start_aura(&mut self, unit: ObjKey, base: u32) {
        if AuraKind::of(base).is_none()
            || !self.aura_types.contains(base)
            || self.aura_emitters.contains_key(&(unit, base))
            || !self.unit(unit).abilities.contains(base)
        {
            return;
        }
        let pulse = self
            .timers
            .start(AURA_PULSE_PERIOD.to_f32(), true, Payload::AuraPulse { unit, base });
        let scan = self
            .timers
            .start(AURA_SCAN_PERIOD.to_f32(), true, Payload::AuraScan { unit, base });
        self.aura_emitters.insert((unit, base), AuraEmitter { pulse, scan });
        self.apply_aura(unit, base, unit);
    }

    /// Stops the emitter of ability `base` on `unit` (the ability was removed). Buffs it applied expire by themselves.
    pub fn stop_aura(&mut self, unit: ObjKey, base: u32) {
        if let Some(e) = self.aura_emitters.remove(&(unit, base)) {
            self.timers.cancel(e.pulse);
            self.timers.cancel(e.scan);
        }
    }

    /// Starts the emitters of the aura abilities `unit` has and stops those of abilities it no longer has. Called by the
    /// natives that add or remove abilities and when a unit is created.
    pub fn sync_auras(&mut self, unit: ObjKey) {
        let have: Vec<u32> = self
            .unit(unit)
            .abilities
            .iter()
            .map(|a| a.base)
            .filter(|&b| AuraKind::of(b).is_some())
            .collect();
        let running: Vec<u32> = self
            .aura_emitters
            .keys()
            .filter(|(u, _)| *u == unit)
            .map(|&(_, b)| b)
            .collect();
        for b in running {
            if !have.contains(&b) {
                self.stop_aura(unit, b);
            }
        }
        for b in have {
            self.start_aura(unit, b);
        }
    }

    /// The 2.0 s timer (message `0xD01C1`): apply the aura to the owner again.
    pub(crate) fn aura_pulse(&mut self, unit: ObjKey, base: u32) {
        if self.unit(unit).removed || !self.unit(unit).abilities.contains(base) {
            self.stop_aura(unit, base);
            return;
        }
        self.apply_aura(unit, base, unit);
    }

    /// The 0.5 s proximity scan (message `0xD01C7` per unit): every unit within `Area` of the owner, in the order of the
    /// circle query, gets the aura.
    pub(crate) fn aura_scan(&mut self, unit: ObjKey, base: u32) {
        if self.unit(unit).removed || !self.unit(unit).abilities.contains(base) {
            self.stop_aura(unit, base);
            return;
        }
        if !self.aura_source_active(unit) {
            return;
        }
        let level_index = self.unit(unit).abilities.get(base).map_or(0, |a| a.level_index);
        let Some(area) = self.aura_types.level(base, level_index).map(|l| l.area) else {
            return;
        };
        let (x, y) = (self.unit(unit).x, self.unit(unit).y);
        for target in circle_candidates(self, x, y, area) {
            self.apply_aura(unit, base, target);
        }
    }

    // --- slow and stun ------------------------------------------------------------------------------------------

    /// Applies a slow to `target`: `move_factor` and `attack_factor` are the (negative) bonuses added to the move and
    /// attack multipliers, `duration` seconds. An existing slow keeps one buff and has its factors **replaced** (CONFIRMED,
    /// `vf214` has no strength compare) and its duration set by the never-shorten rule.
    pub fn apply_slow(&mut self, target: ObjKey, duration: F32, move_factor: F32, attack_factor: F32) -> u32 {
        self.apply_slow_as(SLOW, target, duration, move_factor, attack_factor)
    }

    /// [`Game::apply_slow`] for the slow buff with base code `base` (Thunder Clap applies `BHtc`).
    pub fn apply_slow_as(
        &mut self,
        base: u32,
        target: ObjKey,
        duration: F32,
        move_factor: F32,
        attack_factor: F32,
    ) -> u32 {
        let serial = match self.buff_position(target, base) {
            Some(i) => {
                let serial = self.unit(target).buffs[i].serial;
                self.set_buff_duration(target, serial, duration);
                if let Some(BuffEffect::Slow {
                    move_factor: m,
                    attack_factor: a,
                    applied,
                }) = self.unit(target).buffs[i].effect.clone()
                {
                    if applied {
                        self.add_move(target, arith::neg(m));
                        self.add_attack_speed(target, arith::neg(a));
                    }
                }
                serial
            }
            None => {
                let serial = self.next_buff_serial();
                self.unit_mut(target).buffs.push(Buff {
                    serial,
                    base,
                    predicates: BuffPredicates {
                        negative: true,
                        magic: true,
                        dispellable: true,
                        ..BuffPredicates::default()
                    },
                    timed_life: None,
                    effect: None,
                    expiry: None,
                });
                self.set_buff_duration(target, serial, duration);
                serial
            }
        };
        if let Some(i) = self.unit(target).buffs.iter().position(|b| b.serial == serial) {
            self.add_move(target, move_factor);
            self.add_attack_speed(target, attack_factor);
            self.unit_mut(target).buffs[i].effect = Some(BuffEffect::Slow {
                move_factor,
                attack_factor,
                applied: true,
            });
        }
        serial
    }

    /// Applies a stun to `target` for `duration` seconds. An existing stun is kept and its duration is set by the
    /// never-shorten rule (UNSURE: whether the caster looks the buff up before creating one is not read).
    pub fn apply_stun(&mut self, target: ObjKey, duration: F32) -> u32 {
        self.apply_stun_as(STUN, target, duration)
    }

    /// [`Game::apply_stun`] for the disabling buff with base code `base` (Bash applies `BPSE`, a pause buff).
    pub fn apply_stun_as(&mut self, base: u32, target: ObjKey, duration: F32) -> u32 {
        if let Some(i) = self.buff_position(target, base) {
            let serial = self.unit(target).buffs[i].serial;
            self.set_buff_duration(target, serial, duration);
            return serial;
        }
        let serial = self.next_buff_serial();
        self.unit_mut(target).buffs.push(Buff {
            serial,
            base,
            predicates: BuffPredicates {
                negative: true,
                magic: true,
                dispellable: true,
                ..BuffPredicates::default()
            },
            timed_life: None,
            effect: Some(BuffEffect::Stun),
            expiry: None,
        });
        self.set_buff_duration(target, serial, duration);
        self.unit_mut(target).bonuses.stun += 1;
        self.mover_halt(target);
        serial
    }
}
