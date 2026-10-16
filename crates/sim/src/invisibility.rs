//! Shared invisibility model (S44). Spec: `docs/spec/abilities-invisibility.md`.
//!
//! What the original does (read from the machine code, IDA on the user's `game.dll`):
//!
//! * **The unit side** (CONFIRMED). A unit has an invisibility counter (unit `+0x114`) and a flag word bit `0x1000000`
//!   (unit `+0x5C`). The increment `0x6F28D7D0(fade)` raises the counter; on the 0 -> 1 step the flag is set at once
//!   (`0x6F284950`, the `or 0x1000000`) when the fade time is zero, otherwise after the fade (a timer, message
//!   `852436`). The decrement `0x6F28D9C0(fade)` lowers it and, on reaching 0, clears the flag at once and starts the
//!   fade-in. The users: `CAbilityPermInvis` (`Apiv`, slot `0x12C` = `0x6F118250`: increment with fade `Dur`, only while
//!   its level counter is not positive), `CAbilityGhost` (`Agho`, a `PermInvis`), the buff effects of `AIvi` / `Aivs`
//!   (`Binv`), Wind Walk (`BOwk`), Shadow Meld (`Ashm`, a button ability).
//! * **The viewer side** (CONFIRMED structure, `0x6F3A15F0`): a unit with the flag is seen by a viewer player when the
//!   viewer's player mask (word at player `+0x2E0`) contains the unit's owner, or when the unit's detection mask
//!   (unit sub-object `+0x24`, read by `0x6F27A380(1)`) intersects the viewer's mask. A unit without the flag is always
//!   passed by this gate (the fog test of `0x6F3A38F0` follows). `IsUnitInvisible(unit, player)` is slot `0xFC` with
//!   mode 4, `IsUnitDetected` reads the per-player detection counters (`+0x74`), `UnitInvis` (AI) is the same gate.
//! * **Detection**: the detector abilities (`Adet`, `Adts` Magic Sentry, `Atru` True Sight, `Agyv` Gyro Vision) are
//!   `CAbilityDetector` descendants whose init `0x6F055070` registers the unit as a detector; the level `Rng` (900 for
//!   the unit abilities) is the detection radius (LIKELY).
//!
//! Model: the counter is *derived* from the unit's state on every query (buffs of [`INVIS_BUFFS`], the permanent
//! abilities, Shadow Meld at night while idle), so removing a buff or an ability needs no extra hook. The detection mask
//! is derived from the live detectors in range. UNSURE: the fade-in time (`Dur` of `Apiv`, `DataA` of Wind Walk) is not
//! modelled (the unit is invisible at once), the exact meaning of the viewer mask word (taken as: own and allied
//! players, plus shared vision), the Shadow Meld rule (idle at night, UNSURE, 21 overrides, only the button / order
//! parts read), who may target an invisible unit (the acquisition scan and attack orders of an enemy that cannot see it).

use std::cell::RefCell;

use softfloat::{arith, F32};

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::cast::{effect_duration, CastRec, CastTarget};
use crate::natives::game::{Game, ObjKey};
use crate::natives::ScriptRuntime;
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// Sorceress Invisibility (`CAbilityInvisibility < CAbilitySimpleSpell`, order `invisibility`, 852069).
pub const INVISIBILITY: u32 = code(b"Aivs");
/// Permanent Invisibility (`CAbilityPermInvis`, `CBonusBase` family) and Ghost (`CAbilityGhost < CAbilityPermInvis`).
pub const PERM_INVIS: u32 = code(b"Apiv");
pub const GHOST: u32 = code(b"Agho");
/// Shadow Meld (`CAbilityShadowMeld < CAbilityButton`).
pub const SHADOW_MELD: u32 = code(b"Ashm");
/// The invisibility buff of the potion and of Sorceress Invisibility (`Binv`) and of Wind Walk (`BOwk`).
pub const BUFF_INVIS: u32 = code(b"Binv");
pub const BUFF_WIND_WALK: u32 = code(b"BOwk");
/// Buffs that each add one to the invisibility counter.
pub const INVIS_BUFFS: [u32; 2] = [BUFF_INVIS, BUFF_WIND_WALK];
/// Abilities that each add one for as long as the unit has them.
pub const PERMANENT: [u32; 2] = [PERM_INVIS, GHOST];
/// Detector abilities (`CAbilityDetector`): Sentry (`Adet`), Magic Sentry, True Sight, Gyro Vision.
pub const DETECTORS: [u32; 4] = [code(b"Adet"), code(b"Adts"), code(b"Atru"), code(b"Agyv")];
/// Detection radius when a detector level carries none (the unit abilities use 900).
const DEFAULT_DETECT_RANGE: u32 = 0x4461_0000; // 900.0f

pub(crate) fn register_orders(ids: &mut std::collections::BTreeMap<u32, u32>) {
    ids.insert(852_069, INVISIBILITY);
}

/// Extra target rule of Invisibility: `air,ground,organic,friend,nonsapper,neutral` (`AbilityData.slk`).
pub(crate) fn target_error(gm: &Game, caster: ObjKey, base: u32, target: ObjKey) -> Option<crate::cast::CastError> {
    if base != INVISIBILITY {
        return None;
    }
    let w = targs_word("air,ground,organic,friend,nonsapper,neutral");
    (gm.unit(target).removed || !targs_ok(gm, caster, target, w)).then_some(crate::cast::CastError::BadTarget)
}

/// The class effect of Invisibility (`0x6F13C080`, same shape as the potion effect `0x6F13C180`): the buff `Binv` on the
/// target for the hero-rule duration. Returns false for any other ability.
pub(crate) fn run_effect(game: &RefCell<Game>, _rt: &mut dyn ScriptRuntime, rec: &CastRec) -> bool {
    if rec.base != INVISIBILITY {
        return false;
    }
    if let CastTarget::Unit(t) = rec.target {
        let mut gm = game.borrow_mut();
        let hero = gm.target_hero(t);
        let dur = effect_duration(&rec.level, hero, F32::ZERO);
        gm.apply_invisibility_buff(t, dur);
    }
    true
}

impl Game {
    /// The buff `Binv` for `dur` seconds (the potion and Sorceress paths). Positive, magic, dispellable (UNSURE).
    pub(crate) fn apply_invisibility_buff(&mut self, target: ObjKey, dur: F32) {
        if !self.alive(target) || !arith::lt(F32::ZERO, dur) {
            return;
        }
        let preds = BuffPredicates {
            positive: true,
            magic: true,
            dispellable: true,
            ..buff_predicates(BUFF_INVIS)
        };
        self.apply_mods_buff(target, BUFF_INVIS, preds, UnitMods::default(), dur);
    }

    /// The invisibility counter of a unit (object `+0x114`), derived: every invisibility buff, every permanent ability,
    /// and Shadow Meld while it is night and the unit has no order.
    pub fn invisibility_count(&self, unit: ObjKey) -> i32 {
        let u = self.unit(unit);
        if u.removed {
            return 0;
        }
        let mut n = u.buffs.iter().filter(|b| INVIS_BUFFS.contains(&b.base)).count() as i32;
        n += PERMANENT
            .iter()
            .filter(|&&c| u.abilities.get(c).is_some_and(|a| !a.is_removed()))
            .count() as i32;
        let melds = u.abilities.get(SHADOW_MELD).is_some_and(|a| !a.is_removed());
        if melds && !u.dead && self.is_night() && self.unit_order_ids(unit).is_empty() {
            n += 1;
        }
        n
    }

    /// Whether the invisibility flag (`0x1000000`) is set.
    pub fn is_invisible(&self, unit: ObjKey) -> bool {
        self.invisibility_count(unit) > 0
    }

    /// The players that detect `unit` (the unit's detection mask, one bit per player slot): the owners of the live
    /// detectors whose radius covers it.
    pub fn detecting_players(&self, unit: ObjKey) -> u32 {
        let (ux, uy) = {
            let u = self.unit(unit);
            (u.x, u.y)
        };
        let mut mask = 0u32;
        for i in 0..self.object_count() {
            let k = ObjKey(i as u32);
            if !matches!(self.object(k), crate::natives::game::Object::Unit(_)) || k == unit {
                continue;
            }
            let d = self.unit(k);
            if d.dead || d.removed || d.hidden || mask & (1u32 << d.owner) != 0 {
                continue;
            }
            for c in DETECTORS {
                if !d.abilities.get(c).is_some_and(|a| !a.is_removed()) {
                    continue;
                }
                let range = self.ability_level_record(k, c).map_or(F32::from_bits(DEFAULT_DETECT_RANGE), |l| {
                    let r = if arith::lt(l.area, l.rng) { l.rng } else { l.area };
                    if arith::lt(F32::ZERO, r) {
                        r
                    } else {
                        F32::from_bits(DEFAULT_DETECT_RANGE)
                    }
                });
                let (dx, dy) = (arith::sub(ux, d.x), arith::sub(uy, d.y));
                let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
                if arith::le(d2, arith::mul(range, range)) {
                    mask |= 1u32 << d.owner;
                    break;
                }
            }
        }
        mask
    }

    /// The gate `0x6F3A15F0`: whether `viewer` passes the invisibility test of `unit` (true for a unit that is not
    /// invisible, for the viewer's own and allied units, and for units a player of the viewer's mask detects).
    pub fn invisibility_gate(&self, unit: ObjKey, viewer: u32) -> bool {
        if !self.is_invisible(unit) {
            return true;
        }
        let owner = self.unit(unit).owner;
        if owner == viewer || !self.is_enemy(viewer, owner) {
            return true;
        }
        let mask = u32::from(self.owner_vision_mask(viewer)) | (1 << viewer);
        mask & (1 << owner) != 0 || self.detecting_players(unit) & mask != 0
    }

    /// `IsUnitInvisible(unit, player)`: the unit is invisible and the player does not pass the gate.
    pub fn is_unit_invisible_for(&self, unit: ObjKey, viewer: u32) -> bool {
        !self.invisibility_gate(unit, viewer)
    }
}
