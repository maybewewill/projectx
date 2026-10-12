//! The game side of the unit life and mana properties (`crate::regen`, docs/spec/agile.md section 9): creation, the cache the
//! rest of the simulation reads, the write paths, the cells the hero sinks work on, and the day / night and death hooks.

use softfloat::{Tables, F32};

use crate::agile::{Agile, ModId};
use crate::heroes::LifeCells;
use crate::natives::{Game, ObjKey};
use crate::regen::{UnitStats, LIFE_REGEN_ON};

const NEG_FLT_MAX: F32 = F32(0xFF7F_FFFF);
const FLT_MAX: F32 = F32(0x7F7F_FFFF);

fn unit_keys_with_stats(gm: &Game) -> Vec<ObjKey> {
    (0..gm.object_count())
        .map(|i| ObjKey(i as u32))
        .filter(|&k| matches!(gm.object(k), crate::natives::game::Object::Unit(u) if u.stats.is_some()))
        .collect()
}

impl Game {
    /// The creation of the unit's life and mana properties and the first regeneration refresh (`0x6F2A0E30`, `0x6F2ACDE0`,
    /// `0x6F29DF40`, `0x6F28CAA0`):
    ///
    /// * life: lower limit `0.0` (vf80), upper limit the maximum (vf81), then the value (vf73);
    /// * mana: the default lower limit (`-FLT_MAX`), the upper limit, then the value;
    /// * the rates are the type's `regenHP` / `regenMana`; the life regeneration flag `0x40` is set for `regenType` always, for
    ///   day when the day flag is on and for night when it is off (blight is left to the host);
    /// * the modifiers are created by the first refresh (life, then mana).
    pub(crate) fn init_unit_stats(&mut self, k: ObjKey) {
        if self.orders.tables.is_none() {
            return;
        }
        let (type_id, life, max_life, mana, max_mana) = {
            let u = self.unit(k);
            (u.type_id, u.life, u.max_life, u.mana, u.max_mana)
        };
        let (regen_hp, regen_mana, regen_type) = self
            .unit_types()
            .get(type_id)
            .map_or((F32::ZERO, F32::ZERO, 0), |t| (t.regen_hp, t.regen_mana, t.regen_type));
        let day = self.tod.watch.day;
        let mut flags = 0;
        if regen_type == 1 || (regen_type == 3 && day) || (regen_type == 4 && !day) {
            flags |= LIFE_REGEN_ON;
        }
        let t = self.orders.tables.as_ref().expect("checked above");
        let lp = self.agile.new_property(F32::ZERO, NEG_FLT_MAX, FLT_MAX);
        self.agile.set_limits(lp, F32::ZERO, FLT_MAX, t);
        self.agile.set_limits(lp, F32::ZERO, max_life, t);
        self.agile.set_value(lp, life, t);
        let mp = self.agile.new_property(F32::ZERO, NEG_FLT_MAX, FLT_MAX);
        self.agile.set_limits(mp, NEG_FLT_MAX, max_mana, t);
        self.agile.set_value(mp, mana, t);
        let mut st = UnitStats::new(lp, mp, regen_hp, regen_mana, flags, false);
        st.refresh_both(&mut self.agile, t);
        self.unit_mut(k).stats = Some(st);
        self.recache_stats(k);
    }

    /// Re-reads the cached life, mana and maxima of one unit from its properties.
    fn recache_stats(&mut self, k: ObjKey) {
        let Some(st) = self.unit(k).stats else { return };
        let (l, ml, m, mm) = (
            self.agile.value(st.life),
            self.agile.limits(st.life).1,
            self.agile.value(st.mana),
            self.agile.limits(st.mana).1,
        );
        let u = self.unit_mut(k);
        u.life = l;
        u.max_life = ml;
        u.mana = m;
        u.max_mana = mm;
    }

    /// Refreshes the caches of every unit that has stats. Called after each agile tick (the properties change with time only
    /// when the agile clock moves).
    pub(crate) fn sync_unit_stats(&mut self) {
        for k in unit_keys_with_stats(self) {
            self.recache_stats(k);
        }
    }

    /// Writes the life through the property (`vf73` `0x6F28B1F0` after its `<= 0 -> 0.0` substitution, which the caller made):
    /// the value moves by `x - value now`, clamped to the limits. Without stats the cached field is the store.
    pub fn write_life(&mut self, k: ObjKey, x: F32) {
        match (self.unit(k).stats, self.orders.tables.as_ref()) {
            (Some(st), Some(t)) => {
                self.agile.set_value(st.life, x, t);
                self.recache_stats(k);
            }
            _ => self.unit_mut(k).life = x,
        }
    }

    /// Writes the mana through the property (`0x6F477350`).
    pub fn write_mana(&mut self, k: ObjKey, x: F32) {
        match (self.unit(k).stats, self.orders.tables.as_ref()) {
            (Some(st), Some(t)) => {
                self.agile.set_value(st.mana, x, t);
                self.recache_stats(k);
            }
            _ => self.unit_mut(k).mana = x,
        }
    }

    /// Sets the maximum life (the upper limit, `vf81`); the life is clamped into the limits.
    pub fn write_max_life(&mut self, k: ObjKey, x: F32) {
        match (self.unit(k).stats, self.orders.tables.as_ref()) {
            (Some(st), Some(t)) => {
                let (lo, _) = self.agile.limits(st.life);
                self.agile.set_limits(st.life, lo, x, t);
                self.recache_stats(k);
            }
            _ => self.unit_mut(k).max_life = x,
        }
    }

    /// The life cells for the sinks of `heroes` (the property refreshed to now: base, lower limit, upper limit).
    pub(crate) fn life_cells(&mut self, k: ObjKey) -> LifeCells {
        match self.unit(k).stats {
            Some(st) => {
                let (b, lo, hi) = self.agile.refreshed_cells(st.life);
                LifeCells { life: b, max: hi, floor: lo }
            }
            None => {
                let u = self.unit(k);
                LifeCells { life: u.life, max: u.max_life, floor: F32::ZERO }
            }
        }
    }

    /// Writes the cells back after a sink ran on them.
    pub(crate) fn put_life_cells(&mut self, k: ObjKey, c: LifeCells) {
        match self.unit(k).stats {
            Some(st) => {
                self.agile.put_cells(st.life, c.life, c.floor, c.max);
                self.recache_stats(k);
            }
            None => {
                let u = self.unit_mut(k);
                u.life = c.life;
                u.max_life = c.max;
            }
        }
    }

    /// The mana cells for the mana sink.
    pub(crate) fn mana_cells(&mut self, k: ObjKey) -> LifeCells {
        match self.unit(k).stats {
            Some(st) => {
                let (b, lo, hi) = self.agile.refreshed_cells(st.mana);
                LifeCells { life: b, max: hi, floor: lo }
            }
            None => {
                let u = self.unit(k);
                LifeCells { life: u.mana, max: u.max_mana, floor: F32::ZERO }
            }
        }
    }

    /// Writes the mana cells back.
    pub(crate) fn put_mana_cells(&mut self, k: ObjKey, c: LifeCells) {
        match self.unit(k).stats {
            Some(st) => {
                self.agile.put_cells(st.mana, c.life, c.floor, c.max);
                self.recache_stats(k);
            }
            None => {
                let u = self.unit_mut(k);
                u.mana = c.life;
                u.max_mana = c.max;
            }
        }
    }

    /// Runs `f` on the unit's stats with the agile world and the tables, then stores them. Does nothing without stats.
    fn with_stats(&mut self, k: ObjKey, f: impl FnOnce(&mut UnitStats, &mut Agile, &Tables)) {
        let Some(mut st) = self.unit(k).stats else { return };
        let Some(t) = self.orders.tables.as_ref() else { return };
        f(&mut st, &mut self.agile, t);
        self.unit_mut(k).stats = Some(st);
    }

    /// The unit died or was revived / removed: the regeneration modifiers go away or come back (`+0x5C & 0x100`).
    pub(crate) fn stats_set_dead(&mut self, k: ObjKey, dead: bool) {
        self.with_stats(k, |st, ag, t| st.set_dead(dead, ag, t));
    }

    /// `0x6F28CB10` / `0x6F28CB20`: the construction (or an unsummon) disables / re-enables the life regeneration.
    pub(crate) fn stats_set_life_disabled(&mut self, k: ObjKey, disabled: bool) {
        self.with_stats(k, |st, ag, t| st.set_life_disabled(disabled, ag, t));
    }

    /// `0x6F28D570` / `0x6F28D580`: the same for mana.
    pub(crate) fn stats_set_mana_disabled(&mut self, k: ObjKey, disabled: bool) {
        self.with_stats(k, |st, ag, t| st.set_mana_disabled(disabled, ag, t));
    }

    /// `0x6F28CAF0` / `0x6F28CB00`: the life regeneration flag (blight standing, the host's blight model).
    pub fn set_unit_life_regen(&mut self, k: ObjKey, on: bool) {
        self.with_stats(k, |st, ag, t| st.set_life_regen(on, ag, t));
    }

    /// `0x6F28CB30`: a rate change of the unit's own life regeneration cell.
    pub fn add_unit_life_regen_rate(&mut self, k: ObjKey, delta: F32) {
        self.with_stats(k, |st, ag, t| st.add_life_rate(delta, ag, t));
    }

    /// `0x6F28D590`: the same for the mana rate.
    pub fn add_unit_mana_regen_rate(&mut self, k: ObjKey, delta: F32) {
        self.with_stats(k, |st, ag, t| st.add_mana_rate(delta, ag, t));
    }

    /// The day flag turned on or off: every unit's handler of `0x80264` / `0x80265` (`0x6F2A0330` / `0x6F2A0420`) switches the
    /// regeneration flag of a day or night type. Units are visited in object order.
    pub(crate) fn units_day_changed(&mut self, day: bool) {
        for k in unit_keys_with_stats(self) {
            let ty = self.unit_types().get(self.unit(k).type_id).map_or(0, |t| t.regen_type);
            match (ty, day) {
                (3, true) | (4, false) => self.set_unit_life_regen(k, true),
                (3, false) | (4, true) => self.set_unit_life_regen(k, false),
                _ => {}
            }
        }
    }

    /// Starts a forever modifier of `rate` on the unit's life (`0x6F02DC60`): the way auras, items and heroes add regeneration.
    pub(crate) fn add_life_regen_modifier(&mut self, k: ObjKey, rate: F32) -> Option<ModId> {
        let st = self.unit(k).stats?;
        let t = self.orders.tables.as_ref()?;
        Some(self.agile.add_modifier(st.life, rate, F32::ZERO, t))
    }

    /// The same for mana (`0x6F02DD00`).
    pub(crate) fn add_mana_regen_modifier(&mut self, k: ObjKey, rate: F32) -> Option<ModId> {
        let st = self.unit(k).stats?;
        let t = self.orders.tables.as_ref()?;
        Some(self.agile.add_modifier(st.mana, rate, F32::ZERO, t))
    }

    /// Releases a modifier made by [`Game::add_life_regen_modifier`] / [`Game::add_mana_regen_modifier`] (the delayed destroy).
    pub(crate) fn release_modifier(&mut self, m: ModId) {
        self.agile.destroy_modifier(m);
    }

    /// The hero's strength regeneration changed (`0x6F209090`): the old modifier is released and a new one with the new rate
    /// is made, whatever the change.
    pub(crate) fn set_hero_regen(&mut self, k: ObjKey, rate: F32) {
        if self.unit(k).stats.is_none() {
            return;
        }
        if let Some(m) = self.unit_mut(k).hero_regen_mod.take() {
            self.release_modifier(m);
        }
        let m = self.add_life_regen_modifier(k, rate);
        self.unit_mut(k).hero_regen_mod = m;
    }
}
