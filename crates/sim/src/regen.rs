//! Life and mana of a unit as agile properties, and their base regeneration (`docs/spec/agile.md` section 9).
//!
//! A unit's life (wrapper `Float` embedded at unit `+0x98`) and mana (`+0xB8`) are `CPoReal` properties. Their regeneration is a
//! *forever continuous modifier* whose rate is a plain float of the unit (life: `+0xB0` behind the cell object `+0xAC`,
//! modifier pointer `+0xB4`; mana: `+0xD4` behind `+0xD0`, modifier pointer `+0xD8`). Nothing is applied per tick: the
//! property is evaluated lazily (`value(now) = clamp(B + S * w)`), the modifier only adds its rate to the slope.
//!
//! * Life refresh `0x6F282FC0`: no modifier when the unit is dead (`+0x5C & 0x100`), when the regeneration flag `0x40` of `+0x20`
//!   is clear (a unit type whose `regenType` has not switched it on: night elves by day, units off blight), or when `0x80` is set
//!   (under construction, unsummoning). Otherwise a live modifier gets its rate re-set (`0x6F4A9B00`), else a new forever
//!   modifier of rate `+0xB0` is created.
//! * Mana refresh `0x6F283F60`: no modifier when dead, when the maximum mana (the property's upper limit) is not above 0.0, or
//!   when `+0x20 & 0x100` is set. Otherwise as for life with the rate `+0xD4`.
//! * Releasing a modifier is the delayed destruction of the agile engine (`Agile::destroy_modifier`).
//!
//! CONFIRMED by Unicorn against the real routines: `tools/emu/regen_emu.py` (700 scenarios, 5,898 operations incl. three 160 s
//! runs; `tests/regen_vectors.rs`).

use softfloat::{arith, Tables, F32};

use crate::agile::{Agile, ModId, PropId};

/// `unit+0x20` bit: the life regeneration is switched on (`0x6F28CAF0` sets it, `0x6F28CB00` clears it).
pub const LIFE_REGEN_ON: u32 = 0x40;
/// `unit+0x20` bit: the life regeneration is disabled (`0x6F28CB10` sets it: building in progress, unsummon).
pub const LIFE_REGEN_DISABLED: u32 = 0x80;
/// `unit+0x20` bit: the mana regeneration is disabled (`0x6F28D570` sets it).
pub const MANA_REGEN_DISABLED: u32 = 0x100;

/// The regeneration state of one unit's life and mana properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitStats {
    /// The life property (unit `+0x98`); its upper limit is the maximum life.
    pub life: PropId,
    /// The mana property (unit `+0xB8`); its upper limit is the maximum mana.
    pub mana: PropId,
    /// Life regeneration per second (unit `+0xB0`).
    pub life_rate: F32,
    /// Mana regeneration per second (unit `+0xD4`).
    pub mana_rate: F32,
    life_mod: Option<ModId>,
    mana_mod: Option<ModId>,
    /// The bits of unit `+0x20` that the refreshes read (`LIFE_REGEN_ON`, `LIFE_REGEN_DISABLED`, `MANA_REGEN_DISABLED`).
    pub flags: u32,
    /// Unit `+0x5C & 0x100`: dead.
    pub dead: bool,
}

impl UnitStats {
    /// The stats of a unit whose properties exist: rates as the creation wrote them (`vt[0]` of the cell objects), no modifier
    /// yet (the first refresh creates them, `0x6F28CAA0`).
    pub fn new(life: PropId, mana: PropId, life_rate: F32, mana_rate: F32, flags: u32, dead: bool) -> Self {
        UnitStats { life, mana, life_rate, mana_rate, life_mod: None, mana_mod: None, flags, dead }
    }

    /// Whether a life regeneration modifier exists.
    pub fn has_life_modifier(&self) -> bool {
        self.life_mod.is_some()
    }

    /// Whether a mana regeneration modifier exists.
    pub fn has_mana_modifier(&self) -> bool {
        self.mana_mod.is_some()
    }

    /// The life modifier, if any.
    pub fn life_modifier(&self) -> Option<ModId> {
        self.life_mod
    }

    /// The mana modifier, if any.
    pub fn mana_modifier(&self) -> Option<ModId> {
        self.mana_mod
    }

    /// `0x6F282FC0`.
    pub fn refresh_life(&mut self, ag: &mut Agile, t: &Tables) {
        let off = self.dead || self.flags & LIFE_REGEN_ON == 0 || self.flags & LIFE_REGEN_DISABLED != 0;
        Self::refresh(ag, t, off, self.life, self.life_rate, &mut self.life_mod);
    }

    /// `0x6F283F60`: the maximum mana must be above 0.0 (an unordered compare counts as not above).
    pub fn refresh_mana(&mut self, ag: &mut Agile, t: &Tables) {
        let (_, hi) = ag.limits(self.mana);
        let has_mana = f32::from_bits(hi.0) > 0.0;
        let off = self.dead || !has_mana || self.flags & MANA_REGEN_DISABLED != 0;
        Self::refresh(ag, t, off, self.mana, self.mana_rate, &mut self.mana_mod);
    }

    fn refresh(ag: &mut Agile, t: &Tables, off: bool, prop: PropId, rate: F32, slot: &mut Option<ModId>) {
        if off {
            if let Some(m) = slot.take() {
                ag.destroy_modifier(m);
            }
        } else if let Some(m) = *slot {
            ag.set_modifier_rate(m, rate, t);
        } else {
            *slot = Some(ag.add_modifier(prop, rate, F32::ZERO, t));
        }
    }

    /// The creation-time refresh of both modifiers (`0x6F28CAA0`: life, then mana).
    pub fn refresh_both(&mut self, ag: &mut Agile, t: &Tables) {
        self.refresh_life(ag, t);
        self.refresh_mana(ag, t);
    }

    /// `0x6F28CAF0` / `0x6F28CB00`: the regeneration flag on / off, then the life refresh.
    pub fn set_life_regen(&mut self, on: bool, ag: &mut Agile, t: &Tables) {
        if on {
            self.flags |= LIFE_REGEN_ON;
        } else {
            self.flags &= !LIFE_REGEN_ON;
        }
        self.refresh_life(ag, t);
    }

    /// `0x6F28CB10` / `0x6F28CB20`: the disable flag, then the life refresh.
    pub fn set_life_disabled(&mut self, disabled: bool, ag: &mut Agile, t: &Tables) {
        if disabled {
            self.flags |= LIFE_REGEN_DISABLED;
        } else {
            self.flags &= !LIFE_REGEN_DISABLED;
        }
        self.refresh_life(ag, t);
    }

    /// `0x6F28D570` / `0x6F28D580`: the mana disable flag, then the mana refresh.
    pub fn set_mana_disabled(&mut self, disabled: bool, ag: &mut Agile, t: &Tables) {
        if disabled {
            self.flags |= MANA_REGEN_DISABLED;
        } else {
            self.flags &= !MANA_REGEN_DISABLED;
        }
        self.refresh_mana(ag, t);
    }

    /// `0x6F28CB30`: the life rate changes by `delta` (`add(rate, delta)`), then the life refresh.
    pub fn add_life_rate(&mut self, delta: F32, ag: &mut Agile, t: &Tables) {
        self.life_rate = arith::add(self.life_rate, delta);
        self.refresh_life(ag, t);
    }

    /// `0x6F28D590`: the mana rate changes by `delta`, then the mana refresh.
    pub fn add_mana_rate(&mut self, delta: F32, ag: &mut Agile, t: &Tables) {
        self.mana_rate = arith::add(self.mana_rate, delta);
        self.refresh_mana(ag, t);
    }

    /// The dead flag changes (`+0x5C & 0x100`): the death and revive paths refresh life then mana.
    pub fn set_dead(&mut self, dead: bool, ag: &mut Agile, t: &Tables) {
        self.dead = dead;
        self.refresh_both(ag, t);
    }
}

/// The enum of the `regenType` name table at `0x6FA70F88` (CONFIRMED, read from the table): `always` 1, `blight` 2, `day` 3,
/// `night` 4; anything else 0. The comparison is taken as case-insensitive (UNSURE: the compare routine was not read).
pub fn regen_type_from_name(name: &str) -> u32 {
    match name.trim().to_ascii_lowercase().as_str() {
        "always" => 1,
        "blight" => 2,
        "day" => 3,
        "night" => 4,
        _ => 0,
    }
}
