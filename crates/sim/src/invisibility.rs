//! Shared invisibility model (S44, S50). Spec: `docs/spec/abilities-invisibility.md`.
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
//!   mode 4, `UnitInvis` (AI) is the same gate.
//! * **Detection**: the detector abilities (`Adet`, `Adts` Magic Sentry, `Atru` True Sight, `Agyv` Gyro Vision) are
//!   `CAbilityDetector` descendants whose init `0x6F055070` registers the unit as a detector; the level `Rng` (900 / 1100)
//!   is the detection radius and `DataA` (3) the editor's "Detection Type" (S50: bit 1 = invisible units, bit 2 =
//!   burrowed; a detector without the invisible bit does not reveal).
//!
//! ## S50: the pieces the unit-ability batches left as hooks
//!
//! * **Fade-in** (`Apiv` `Dur`, Wind Walk `DataA` "Transition Time" 0.6 s, Shadow Meld `DataA` "Fade Duration" 1.5 s,
//!   `Aivs` `DataA`, Ghost `Dur`): the flag is set `fade` seconds after the counter rose from zero. The model records the
//!   time of the 0 -> positive step ([`Game::invisibility_note`], called where a source appears: the ability sync, the
//!   potion and Sorceress paths, Wind Walk) and [`Game::is_invisible`] needs `now >= start + fade`. A unit with no record
//!   (a source added by a path without the hook) is invisible at once. A scan every 0.1 s (started when the first
//!   source appears) drops the records of units whose counter fell back to zero and runs the Shadow Meld conditions.
//! * **Ghost** (`Agho`, `unit_abilities_a::is_ghost`) and **Permanent Invisibility** (`Apiv`,
//!   `unit_abilities_m::m_perm_invisible`) are two counts. `Aeth` is *not* a ghost seer (the S46 reading was wrong): the
//!   editor labels `ETH1/2` are "Immune to Morph Effects" and "Does Not Block Buildings", it is the visible ghost form, so
//!   it adds no invisibility and reveals nothing. Ghost `DataA` is the editor's "Auto-Acquire Attack Targets":
//!   [`Game::invisibility_allows_acquire`].
//! * **Shadow Meld** (`Ashm`, `unit_abilities_m::m_shadow_meld_active`): idle (no orders) at night for `DataC` ("Action
//!   Duration", 0.5 s), then the fade `DataA`. `DataB` ("Day/Night Duration", 2.5 s) is not used (UNSURE).
//! * **True Sight** (`Atru`, `m_true_sight_reveals`): the radius is `Rng` (900), the query read `Area` (0) before S50.
//! * **Ultravision** (`Ault`, `CAbilityUltraVision < CAbilityNightVision`): the night sight radius equals the day sight
//!   radius while the unit has it (UNSURE; no data row).
//! * **Orders**: an order that targets an enemy unit the issuer's player cannot see (invisible and undetected) is dropped
//!   by the dispatcher ([`Game::order_target_unseen`]); spell targets go through the same gate in the cast core.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use softfloat::{arith, conv, F32};

use crate::abilities::{buff_predicates, code, BuffPredicates};
use crate::cast::{effect_duration, CastRec, CastTarget, SpellMsg};
use crate::natives::game::{Game, ObjKey, Payload};
use crate::natives::ScriptRuntime;
use crate::orders::{Order, OrderClass};
use crate::timer::TimerId;
use crate::unit_spells::{targs_ok, targs_word, UnitMods};

/// Sorceress Invisibility (`CAbilityInvisibility < CAbilitySimpleSpell`, order `invisibility`, 852069).
pub const INVISIBILITY: u32 = code(b"Aivs");
/// Permanent Invisibility (`CAbilityPermInvis`, `CBonusBase` family) and Ghost (`CAbilityGhost < CAbilityPermInvis`).
pub const PERM_INVIS: u32 = code(b"Apiv");
pub const GHOST: u32 = code(b"Agho");
/// The visible ghost form (`CAbilityGhostVisible`): no invisibility (S50 correction).
pub const GHOST_VISIBLE: u32 = code(b"Aeth");
/// Shadow Meld (`CAbilityShadowMeld < CAbilityButton`).
pub const SHADOW_MELD: u32 = code(b"Ashm");
/// Wind Walk (the fade-in time is its `DataA`).
pub const WIND_WALK: u32 = code(b"AOwk");
/// Ultravision (`CAbilityUltraVision`).
pub const ULTRAVISION: u32 = code(b"Ault");
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
/// Timer kind of the scan (the module uses the S50 range `0x60..=0x6F` of `SpellMsg::UnitSpell`).
pub const KIND_SCAN: u8 = 0x6D;
/// The scan period.
const SCAN_PERIOD: f32 = 0.1;

/// State of the model (owned by `neutral_spells::State`).
#[derive(Debug, Clone, Default)]
pub struct State {
    /// When the counter of a unit rose from zero and the fade of that step: `unit -> (start, fade)`.
    since: BTreeMap<ObjKey, (F32, F32)>,
    /// Shadow Meld: when the unit became idle at night, and the units that stood idle long enough.
    meld_idle: BTreeMap<ObjKey, F32>,
    meld_ready: BTreeSet<ObjKey>,
    /// The running scan timer.
    scan: Option<TimerId>,
    /// Ultravision: the night sight a unit had before the ability raised it.
    ultra: BTreeMap<ObjKey, F32>,
}

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

/// The scan timer (`kind` [`KIND_SCAN`]).
pub(crate) fn on_scan(game: &RefCell<Game>) {
    game.borrow_mut().invisibility_scan();
}

/// The ability sync (`sync_neutral_spells`): a unit that got or lost an invisibility ability, and Ultravision.
pub(crate) fn sync(gm: &mut Game, unit: ObjKey) {
    gm.invisibility_note(unit);
    let has_meld = gm.unit(unit).abilities.get(SHADOW_MELD).is_some_and(|a| !a.is_removed());
    if has_meld {
        gm.invisibility_ensure_scan();
    } else {
        gm.spells.neutral.inv.meld_idle.remove(&unit);
        gm.spells.neutral.inv.meld_ready.remove(&unit);
    }
    let ultra = gm.unit(unit).abilities.get(ULTRAVISION).is_some_and(|a| !a.is_removed());
    let had = gm.spells.neutral.inv.ultra.contains_key(&unit);
    if ultra && !had {
        let (day, night) = (gm.unit(unit).sight, gm.unit(unit).night_sight);
        gm.spells.neutral.inv.ultra.insert(unit, night);
        if arith::lt(night, day) {
            gm.unit_mut(unit).night_sight = day;
        }
    } else if !ultra && had {
        if let Some(night) = gm.spells.neutral.inv.ultra.remove(&unit) {
            gm.unit_mut(unit).night_sight = night;
        }
    }
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
        self.invisibility_note(target);
    }

    /// The invisibility counter of a unit (object `+0x114`), derived: every invisibility buff, Permanent Invisibility,
    /// Ghost, and Shadow Meld once the unit stood idle at night for its action duration.
    pub fn invisibility_count(&self, unit: ObjKey) -> i32 {
        let u = self.unit(unit);
        if u.removed {
            return 0;
        }
        let mut n = u.buffs.iter().filter(|b| INVIS_BUFFS.contains(&b.base)).count() as i32;
        if self.m_perm_invisible(unit) {
            n += 1;
        }
        if self.is_ghost(unit) {
            n += 1;
        }
        if !u.dead && self.spells.neutral.inv.meld_ready.contains(&unit) && self.m_shadow_meld_active(unit, true) {
            n += 1;
        }
        n
    }

    /// The fade-in time of the sources a unit has now (the largest): Permanent Invisibility `Dur`, Ghost `Dur`, Wind Walk
    /// `DataA` while the buff lasts, Sorceress Invisibility `DataA`, Shadow Meld `DataA`.
    pub fn invisibility_fade(&self, unit: ObjKey) -> F32 {
        let mut fade = F32::ZERO;
        let mut take = |v: F32| {
            if arith::lt(fade, v) {
                fade = v;
            }
        };
        if self.m_perm_invisible(unit) {
            take(self.spell_level(unit, PERM_INVIS).dur);
        }
        if self.is_ghost(unit) {
            take(self.spell_level(unit, GHOST).dur);
        }
        let u = self.unit(unit);
        if u.buffs.iter().any(|b| b.base == BUFF_WIND_WALK) && u.abilities.contains(WIND_WALK) {
            take(self.spell_level(unit, WIND_WALK).data[0]);
        }
        if u.buffs.iter().any(|b| b.base == BUFF_INVIS) {
            if let Some(l) = self.spell_levels_of(INVISIBILITY).first() {
                take(l.data[0]);
            }
        }
        if self.spells.neutral.inv.meld_ready.contains(&unit) {
            take(self.spell_level(unit, SHADOW_MELD).data[0]);
        }
        fade
    }

    /// Records the 0 -> positive step of the counter of `unit` (the increment `0x6F28D7D0`): the start time and the fade.
    /// Called where a source appears; a unit that already has a record keeps it.
    pub fn invisibility_note(&mut self, unit: ObjKey) {
        if self.invisibility_count(unit) > 0 {
            if !self.spells.neutral.inv.since.contains_key(&unit) {
                let fade = self.invisibility_fade(unit);
                let now = self.game_time();
                self.spells.neutral.inv.since.insert(unit, (now, fade));
            }
            self.invisibility_ensure_scan();
        }
    }

    pub(crate) fn invisibility_ensure_scan(&mut self) {
        if self.spells.neutral.inv.scan.is_some() {
            return;
        }
        let id = self.timers.start(
            SCAN_PERIOD,
            true,
            Payload::Spell(SpellMsg::UnitSpell { kind: KIND_SCAN, unit: ObjKey(0), base: 0, aux: 0 }),
        );
        self.spells.neutral.inv.scan = Some(id);
    }

    /// One scan: the Shadow Meld conditions, the records of units whose counter fell to zero. Stops when nothing is left.
    fn invisibility_scan(&mut self) {
        let now = self.game_time();
        let units: Vec<ObjKey> = (0..self.object_count())
            .map(|i| ObjKey(i as u32))
            .filter(|&k| matches!(self.object(k), crate::natives::game::Object::Unit(_)))
            .collect();
        let mut melders = false;
        for k in units {
            if self.unit(k).abilities.get(SHADOW_MELD).is_none_or(|a| a.is_removed()) {
                continue;
            }
            melders = true;
            let still = self.alive(k) && self.unit_order_ids(k).is_empty();
            if still && self.m_shadow_meld_active(k, true) {
                let since = *self.spells.neutral.inv.meld_idle.entry(k).or_insert(now);
                let need = self.spell_level(k, SHADOW_MELD).data[2];
                if !arith::lt(arith::sub(now, since), need) && self.spells.neutral.inv.meld_ready.insert(k) {
                    self.invisibility_note(k);
                }
            } else {
                self.spells.neutral.inv.meld_idle.remove(&k);
                self.spells.neutral.inv.meld_ready.remove(&k);
            }
        }
        let stale: Vec<ObjKey> = self
            .spells
            .neutral
            .inv
            .since
            .keys()
            .copied()
            .filter(|&k| self.invisibility_count(k) == 0)
            .collect();
        for k in stale {
            self.spells.neutral.inv.since.remove(&k);
        }
        if !melders && self.spells.neutral.inv.since.is_empty() {
            if let Some(id) = self.spells.neutral.inv.scan.take() {
                self.timers.cancel(id);
            }
        }
    }

    /// Whether the invisibility flag (`0x1000000`) is set: the counter is positive and the fade of its first step has run
    /// out. A unit without a record is invisible at once.
    pub fn is_invisible(&self, unit: ObjKey) -> bool {
        if self.invisibility_count(unit) <= 0 {
            return false;
        }
        match self.spells.neutral.inv.since.get(&unit) {
            Some(&(start, fade)) => !arith::lt(self.game_time(), arith::add(start, fade)),
            None => true,
        }
    }

    /// The players that detect `unit` (the unit's detection mask, one bit per player slot): the owners of the live
    /// detectors whose radius covers it and whose detection type has the invisible bit.
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
                if d.abilities.get(c).is_none_or(|a| a.is_removed()) {
                    continue;
                }
                let level = self.ability_level_record(k, c);
                // The editor's detection type: 0 (no row) counts as "all", otherwise bit 1 is the invisible bit.
                let ty = level.map_or(0, |l| conv::f2i(l.data[0]));
                if ty != 0 && ty & 1 == 0 {
                    continue;
                }
                let covers = if c == code(b"Atru") {
                    // True Sight: the query of the unit abilities (S50: radius `Rng`).
                    self.m_true_sight_reveals(k, unit)
                } else {
                    let range = level.map_or(F32::from_bits(DEFAULT_DETECT_RANGE), |l| {
                        let r = if arith::lt(l.area, l.rng) { l.rng } else { l.area };
                        if arith::lt(F32::ZERO, r) {
                            r
                        } else {
                            F32::from_bits(DEFAULT_DETECT_RANGE)
                        }
                    });
                    let (dx, dy) = (arith::sub(ux, d.x), arith::sub(uy, d.y));
                    let d2 = arith::add(arith::mul(dx, dx), arith::mul(dy, dy));
                    arith::le(d2, arith::mul(range, range))
                };
                if covers {
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

    /// Whether an order of `unit` targets an enemy unit its player cannot see (invisible and not detected): the dispatcher
    /// drops such an order (the natives still answer true, "quietly dropped").
    pub(crate) fn order_target_unseen(&self, unit: ObjKey, order: &Order) -> bool {
        if order.class != OrderClass::Target || order.target == 0 {
            return false;
        }
        let Some(target) = self.unit_of_handle(order.target) else {
            return false;
        };
        if target == unit {
            return false;
        }
        let owner = self.unit(unit).owner;
        self.is_enemy(owner, self.unit(target).owner) && !self.invisibility_gate(target, owner)
    }

    /// Whether `unit` may acquire attack targets by itself: a Ghost whose `DataA` ("Auto-Acquire Attack Targets") is zero
    /// may not while it is a ghost.
    pub fn invisibility_allows_acquire(&self, unit: ObjKey) -> bool {
        if !self.is_ghost(unit) {
            return true;
        }
        arith::lt(F32::ZERO, self.spell_level(unit, GHOST).data[0]) || !self.is_invisible(unit)
    }

    /// The viewer-side gate for a spell target (the cast core): `caster`'s player must see the unit.
    pub(crate) fn spell_target_unseen(&self, caster: ObjKey, target: ObjKey) -> bool {
        if target == caster {
            return false;
        }
        let owner = self.unit(caster).owner;
        self.is_enemy(owner, self.unit(target).owner) && !self.invisibility_gate(target, owner)
    }
}
