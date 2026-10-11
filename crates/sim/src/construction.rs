//! Building construction: the registration of a building under construction, the build payment, the
//! pending-payment refund and the construction timer (docs/spec/construction.md).
//!
//! What is here and where it comes from:
//! * [`transition`] is the unit registration transition `0x6F29F850` (CONFIRMED by reading the body; the
//!   transcription is checked against the original in Unicorn, `tools/emu/construction_counts_emu.py`, 1,000
//!   vector cases in `tests/vectors/construction_counts_seed7.tsv`). Start of a building = begin in progress
//!   (flags `0x101`) with food used; finish = remove in progress and add completed (flags `0x102`) with the
//!   food made; cancel or removal = flags `0`.
//! * [`place_check`] and [`PendingCharge`] are the placement payment and the refund of a payment that was not
//!   yet placed (`0x6F0EDA70`, `0x6F0D9400`, `0x6F0ED9F0`, `0x6F0F7750`). Checked in Unicorn by
//!   `tools/emu/construction_pay_emu.py` (3,000 cases, 0 mismatches). The "enough resources" test reads the
//!   stored gold and lumber through `0x6F473170`, which is LIKELY the stored value (see the spec).
//! * [`ConstructionTimers`] runs a construction on the shared [`TimerQueue`]. Its progress law is NOT
//!   verified: the completion time is the build time in seconds, and [`hp_fraction`] is linear in progress.
//!   Neither is what the original does (the progress tick is not found, see the spec), so these two are
//!   placeholders that the rest of the code must not treat as bit-exact.
//!
//! Not modelled here: the placement footprint and cell flags (the pyramid is rebuilt around a unit placement,
//! `pathing-build.md` 5.2, the stamp is not found), the worker's order flow, the events, the race differences.

use softfloat::F32;

use crate::economy::{credit, refund_units, Flags, PlayerEconomy, Res};
use crate::timer::{TimerId, TimerQueue};
use crate::unit_counts::PlayerCounts;

/// Bit `0x1` of the flags of the registration transition: begin in progress.
pub const FLAG_BEGIN: u32 = 0x1;
/// Bit `0x2`: add as completed (the finish).
pub const FLAG_FINISH: u32 = 0x2;
/// Bit `0x20`: the flag argument of the counter methods (the "flag set" record fields).
pub const FLAG_COUNT_SET: u32 = 0x20;
/// Bit `0x40`: skip the counter part.
pub const FLAG_SKIP_COUNTS: u32 = 0x40;
/// Bit `0x100`: add the food used at the start.
pub const FLAG_FOOD_USE_START: u32 = 0x100;
/// Bit `0x400`: add the food made at the finish.
pub const FLAG_FOOD_MADE_FINISH: u32 = 0x400;
/// Bit `0x1000`: skip the food part.
pub const FLAG_SKIP_FOOD: u32 = 0x1000;

/// Registration word bits (`unit+0x64`): `0x8` registered in progress, `0x10` registered completed, `0x1` and
/// `0x2` the state bits, `0x100`/`0x200` food used applied (`0x300` when set together), `0x400`/`0x800` food
/// made applied (`0xC00`).
pub const REG_IN_PROGRESS: u32 = 0x8;
/// See [`REG_IN_PROGRESS`].
pub const REG_COMPLETED: u32 = 0x10;

/// The food values the original reads at the time of the transition (the current type's `fmade` and `fused`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Food {
    /// Food made (supply) of the type, `UnitBalance` `fmade`.
    pub made: i32,
    /// Food used of the type, `UnitBalance` `fused`.
    pub used: i32,
}

/// One effect of the transition, in the order the original performs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// `0x6F40FD70(type, 0)`: remove an in-progress registration (flag clear).
    RemoveInProgress { type_id: u32 },
    /// `0x6F40FDF0(type)`: remove a completed registration.
    RemoveCompleted { type_id: u32 },
    /// `0x6F40FD30(type, flag)`: add a completed unit.
    AddCompleted { type_id: u32, flag: bool },
    /// `0x6F40FE30(type, flag)`: begin an in-progress unit.
    BeginInProgress { type_id: u32, flag: bool },
    /// `0x6F03F970(n)`: food made (supply) changes by `n` (script units).
    FoodMade(i32),
    /// `0x6F40F5E0(n)`: food used changes by `n`.
    FoodUsed(i32),
}

/// `0x6F29F850`: moves a unit's registration from `old` to `new` and returns the new registration word and the
/// effects, in order. `reg` is the unit's word at `unit+0x64`.
///
/// The original first reads the owner and sets the current player (`0x6F3A1650`); that call has no counter
/// effect and is left to the host.
pub fn transition(reg: u32, flags: u32, old: u32, new: u32, food: Food) -> (u32, Vec<Effect>) {
    let mut reg = reg;
    let mut out = Vec::new();
    if flags & FLAG_SKIP_COUNTS == 0 {
        if reg & REG_IN_PROGRESS != 0 {
            out.push(Effect::RemoveInProgress { type_id: old });
            reg &= !0x9;
        } else if reg & REG_COMPLETED != 0 {
            out.push(Effect::RemoveCompleted { type_id: old });
            reg &= !0x12;
        }
        if flags & FLAG_BEGIN == 0 {
            if flags & FLAG_FINISH != 0 {
                out.push(Effect::AddCompleted {
                    type_id: new,
                    flag: flags & FLAG_COUNT_SET != 0,
                });
                reg |= 0x12;
            }
        } else {
            out.push(Effect::BeginInProgress {
                type_id: new,
                flag: flags & FLAG_COUNT_SET != 0,
            });
            reg |= 0x9;
        }
    }
    if flags & FLAG_SKIP_FOOD == 0 {
        let before = reg;
        let mut clear = 0u32;
        let mut set = 0u32;
        if before & 0x800 != 0 {
            out.push(Effect::FoodMade(-food.made));
            clear = 0xC00;
        }
        if flags & FLAG_FOOD_USE_START != 0 {
            out.push(Effect::FoodUsed(food.used));
            set = 0x300;
        }
        if before & 0x200 != 0 {
            out.push(Effect::FoodUsed(-food.used));
            clear |= 0x300;
        }
        if flags & FLAG_FOOD_MADE_FINISH != 0 {
            out.push(Effect::FoodMade(food.made));
            set |= 0xC00;
        }
        reg = (reg & !clear) | set;
    }
    (reg, out)
}

/// Applies the counter effects to a player's counters. `class_of` gives the class word of a type (see
/// [`crate::unit_counts::class_of`]). Food effects are skipped: the host applies them to its food totals.
pub fn apply_counts(counts: &mut PlayerCounts, effects: &[Effect], class_of: impl Fn(u32) -> u32) {
    for e in effects {
        match *e {
            Effect::RemoveInProgress { type_id } => {
                let c = class_of(type_id);
                counts.remove_in_progress(type_id, c, false);
            }
            Effect::RemoveCompleted { type_id } => {
                let c = class_of(type_id);
                counts.remove_completed(type_id, c);
            }
            Effect::AddCompleted { type_id, flag } => {
                let c = class_of(type_id);
                counts.add_completed(type_id, c, flag);
            }
            Effect::BeginInProgress { type_id, flag } => {
                let c = class_of(type_id);
                counts.begin_in_progress(type_id, c, flag);
            }
            Effect::FoodMade(_) | Effect::FoodUsed(_) => {}
        }
    }
}

/// The build cost paid at the placement check, in script units (the values stored at the ability's `+0x94` and
/// `+0x98`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Charge {
    pub gold: i32,
    pub lumber: i32,
}

/// Result of the placement payment check (`0x6F0EDA70`, flag 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceCheck {
    /// Both resources were covered; the amounts were taken and recorded.
    Paid(Charge),
    /// Not enough gold or lumber: nothing is taken (the original shows an error message).
    Insufficient,
}

/// `0x6F0EDA70` flag 0: the cost is taken when both stored amounts cover it. The original compares
/// `cost <= stored / 10` for gold, then for lumber (the stored values are x10). Gold is tested first, so when
/// gold fails the lumber test is not made.
pub fn place_check(players: &mut [PlayerEconomy], owner: usize, cost: Charge) -> PlaceCheck {
    let gold_raw = players[owner].stored(Res::Gold);
    if cost.gold > gold_raw / 10 {
        return PlaceCheck::Insufficient;
    }
    let lumber_raw = players[owner].stored(Res::Lumber);
    if cost.lumber > lumber_raw / 10 {
        return PlaceCheck::Insufficient;
    }
    // `0x6F0D9400`: the negated amounts through the plain changers.
    credit(players, owner, Res::Gold, -cost.gold, Flags::NONE);
    credit(players, owner, Res::Lumber, -cost.lumber, Flags::NONE);
    PlaceCheck::Paid(cost)
}

/// A payment taken for a build order that has not been placed yet (the ability's `+0x94`, `+0x98`, and bit 31 of
/// `+0x20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PendingCharge {
    pub charge: Charge,
    /// Bit 31 of the ability flags word: set while a payment is recorded.
    pub recorded: bool,
}

impl PendingCharge {
    /// Records a payment (the successful branch of the placement check).
    pub fn record(&mut self, paid: Charge) {
        self.charge = paid;
        self.recorded = true;
    }

    /// `0x6F0F7750` with player `-1`: zeroes the record and refunds the full amounts (no rate) to the owner
    /// through `0x6F0ED9F0`. Returns the amounts credited (script units).
    pub fn refund(&mut self, players: &mut [PlayerEconomy], owner: usize) -> Charge {
        let c = self.charge;
        self.charge = Charge::default();
        self.recorded = false;
        if c.gold == 0 && c.lumber == 0 {
            return Charge::default();
        }
        let gold = credit(players, owner, Res::Gold, c.gold, Flags::NONE);
        let lumber = credit(players, owner, Res::Lumber, c.lumber, Flags::NONE);
        Charge { gold, lumber }
    }
}

/// The cancel refund of a building under construction: `F2I(MUL(I2R(cost), rate))` (the same arithmetic as the
/// train refund, `economy::refund_units`). When the building's flag `0x400` is set the rate is zero
/// (`0x6F031090`, CONFIRMED). The rate is the `ConstructionRefundRate` key, whose value is not known here, so the
/// caller passes it. The meaning of the cost scaling in `0x6F0223C0` is UNSURE, so the caller passes the cost.
pub fn construction_refund(cost: i32, rate: F32, flag_0x400: bool) -> i32 {
    if flag_0x400 {
        return 0;
    }
    refund_units(cost, rate)
}

/// One construction in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub type_id: u32,
    /// Build time in seconds (the type's `bldtm`).
    pub build_seconds: f32,
    /// Game time when the construction started.
    pub started_at: f32,
    timer: TimerId,
}

/// The constructions of a game, on one timer queue. Completion is the timer's expiry. The progress law is a
/// placeholder (see the module notes).
#[derive(Debug, Default)]
pub struct ConstructionTimers {
    queue: TimerQueue<u32>,
    sites: std::collections::BTreeMap<u32, Site>,
    next_id: u32,
}

impl ConstructionTimers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Game time of the queue.
    pub fn now(&self) -> f32 {
        self.queue.now()
    }

    /// Starts a construction that finishes after `build_seconds`. Returns the site id.
    pub fn start(&mut self, type_id: u32, build_seconds: i32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let secs = build_seconds.max(0) as f32;
        let timer = self.queue.start(secs, false, id);
        let started_at = self.queue.now();
        self.sites.insert(
            id,
            Site {
                type_id,
                build_seconds: secs,
                started_at,
                timer,
            },
        );
        id
    }

    /// Cancels a construction in progress; returns its record, or `None` when it is not running.
    pub fn cancel(&mut self, site: u32) -> Option<Site> {
        let s = self.sites.remove(&site)?;
        self.queue.cancel(s.timer);
        Some(s)
    }

    /// The site record.
    pub fn site(&self, site: u32) -> Option<&Site> {
        self.sites.get(&site)
    }

    /// Advances the queue to `target` and returns the sites that completed, in expiry order.
    pub fn advance(&mut self, target: f32) -> Vec<u32> {
        let mut done = Vec::new();
        while let Some(fired) = self.queue.pop_due(target) {
            if self.sites.remove(&fired.payload).is_some() {
                done.push(fired.payload);
            }
        }
        done
    }

    /// Progress in `0..=1` at the current game time (UNVERIFIED law: linear in the build time).
    pub fn progress(&self, site: u32) -> Option<f32> {
        let s = self.sites.get(&site)?;
        if s.build_seconds <= 0.0 {
            return Some(1.0);
        }
        let p = (self.queue.now() - s.started_at) / s.build_seconds;
        Some(p.clamp(0.0, 1.0))
    }
}

/// Fraction of the maximum hit points a building has at `progress` (UNVERIFIED placeholder: the original's
/// rule is not found; the value is linear from 0 to 1).
pub fn hp_fraction(progress: f32) -> f32 {
    progress.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::PlayerEconomy;

    fn players() -> Vec<PlayerEconomy> {
        vec![PlayerEconomy::new(0), PlayerEconomy::new(1)]
    }

    #[test]
    fn start_takes_the_food_used_and_begins_in_progress() {
        let food = Food { made: 8, used: 0 };
        let (reg, eff) = transition(0, 0x101, 5, 5, food);
        assert_eq!(reg, 0x9 | 0x300);
        assert_eq!(
            eff,
            vec![
                Effect::BeginInProgress { type_id: 5, flag: false },
                Effect::FoodUsed(0)
            ]
        );
    }

    #[test]
    fn finish_moves_the_count_and_adds_the_food_made() {
        let food = Food { made: 8, used: 3 };
        let (reg0, _) = transition(0, 0x101, 5, 5, food);
        // 0x502 = finish with the food made (bit 0x400). The building finish flags are UNSURE; 0x102 (upgrade)
        // has no food made.
        let (reg, eff) = transition(reg0, 0x502, 5, 5, food);
        // The food used applied at the start (bit 0x200) is taken back here, then the food made is added.
        assert_eq!(
            eff,
            vec![
                Effect::RemoveInProgress { type_id: 5 },
                Effect::AddCompleted { type_id: 5, flag: false },
                Effect::FoodUsed(3),
                Effect::FoodUsed(-3),
                Effect::FoodMade(8),
            ]
        );
        assert_eq!(reg & REG_COMPLETED, REG_COMPLETED);
        assert_eq!(reg & 0xC00, 0xC00);
        // Flag 0x100 is set on 0x502 too, so the food used marker (0x300) is set again.
        assert_eq!(reg & 0x300, 0x300);
    }

    #[test]
    fn cancel_removes_the_in_progress_registration_and_its_food() {
        let food = Food { made: 8, used: 3 };
        let (reg0, _) = transition(0, 0x101, 5, 5, food);
        let (reg, eff) = transition(reg0, 0, 5, 5, food);
        assert_eq!(reg & 0x9, 0);
        assert_eq!(
            eff,
            vec![Effect::RemoveInProgress { type_id: 5 }, Effect::FoodUsed(-3)]
        );
    }

    #[test]
    fn counts_follow_the_effects() {
        use crate::unit_counts::{PlayerCounts, CLASS_STRUCTURE};
        let mut c = PlayerCounts::new();
        let food = Food::default();
        let (reg0, e0) = transition(0, 0x101, 7, 7, food);
        apply_counts(&mut c, &e0, |_| CLASS_STRUCTURE);
        assert_eq!(c.structures_in_progress, 1);
        let (_reg1, e1) = transition(reg0, 0x102, 7, 7, food);
        apply_counts(&mut c, &e1, |_| CLASS_STRUCTURE);
        assert_eq!((c.structures_in_progress, c.structures_completed), (0, 1));
    }

    #[test]
    fn skip_flags_leave_the_word_alone_except_the_listed_bits() {
        let (reg, eff) = transition(0x9, 0x1040, 1, 1, Food { made: 2, used: 2 });
        assert_eq!((reg, eff.len()), (0x9, 0));
    }

    #[test]
    fn placement_takes_both_costs_when_both_are_covered() {
        let mut p = players();
        p[0].stock = [500 * 10, 300 * 10];
        let r = place_check(&mut p, 0, Charge { gold: 150, lumber: 100 });
        assert_eq!(r, PlaceCheck::Paid(Charge { gold: 150, lumber: 100 }));
        assert_eq!(p[0].stock, [350 * 10, 200 * 10]);
    }

    #[test]
    fn placement_refuses_without_taking_anything() {
        let mut p = players();
        p[0].stock = [500 * 10, 50 * 10];
        let r = place_check(&mut p, 0, Charge { gold: 150, lumber: 100 });
        assert_eq!(r, PlaceCheck::Insufficient);
        assert_eq!(p[0].stock, [500 * 10, 50 * 10]);
    }

    #[test]
    fn placement_compares_the_stored_value_divided_by_ten() {
        let mut p = players();
        // 99 gold stored (990 raw): a cost of 100 is refused, 99 is accepted.
        p[0].stock = [990, 0];
        assert_eq!(place_check(&mut p, 0, Charge { gold: 100, lumber: 0 }), PlaceCheck::Insufficient);
        assert_eq!(
            place_check(&mut p, 0, Charge { gold: 99, lumber: 0 }),
            PlaceCheck::Paid(Charge { gold: 99, lumber: 0 })
        );
        assert_eq!(p[0].stock[0], 0);
    }

    #[test]
    fn pending_refund_returns_the_full_recorded_amounts_and_clears_the_record() {
        let mut p = players();
        p[1].stock = [0, 0];
        let mut pending = PendingCharge::default();
        pending.record(Charge { gold: 150, lumber: 100 });
        let back = pending.refund(&mut p, 1);
        assert_eq!(back, Charge { gold: 150, lumber: 100 });
        assert_eq!(p[1].stock, [1500, 1000]);
        assert_eq!(pending, PendingCharge::default());
        // A second refund has nothing to give back.
        assert_eq!(pending.refund(&mut p, 1), Charge::default());
    }

    #[test]
    fn construction_refund_uses_the_train_arithmetic_and_the_0x400_rule() {
        // Rate 0.5 (0x3F000000): 75 of a cost of 150.
        assert_eq!(construction_refund(150, F32(0x3F00_0000), false), 75);
        assert_eq!(construction_refund(150, F32(0x3F00_0000), true), 0);
    }

    #[test]
    fn timers_complete_in_expiry_order_and_cancel_removes_the_site() {
        let mut t = ConstructionTimers::new();
        let a = t.start(10, 30);
        let b = t.start(11, 10);
        let c = t.start(12, 20);
        assert_eq!(t.cancel(c).map(|s| s.type_id), Some(12));
        assert_eq!(t.advance(9.0), Vec::<u32>::new());
        assert_eq!(t.advance(10.0), vec![b]);
        assert!(t.site(a).is_some());
        assert_eq!(t.advance(30.0), vec![a]);
        assert_eq!(t.progress(a), None);
    }

    #[test]
    fn progress_is_linear_and_clamped() {
        let mut t = ConstructionTimers::new();
        let a = t.start(10, 20);
        t.advance(5.0);
        assert_eq!(t.progress(a), Some(0.25));
        assert_eq!(hp_fraction(1.5), 1.0);
    }
}
