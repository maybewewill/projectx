//! Gold and lumber economy of the 1.26a engine: harvest carry, mine withdrawal, deposit, upkeep tax
//! and cancel refunds. Spec: `docs/spec/economy-3.md` (addresses and confidence per item).
//!
//! What is modelled here, in short:
//! - the resource changer (`0x6F4167D0`, wrappers `0x6F416A00` gold / `0x6F416A40` lumber): stored
//!   amounts are x10 with a cap of 10,000,000; the upkeep tax percent, the ally share table and the
//!   remainder credit;
//! - the upkeep tier and tax percent (`0x6F40F620`), from the UpkeepUsage / UpkeepGoldTax /
//!   UpkeepLumberTax tables, with the soft-float chain `F2I(ROUND(MUL(tax, 100.0)))`;
//! - the worker's carry (`0x6F055610`), the deposit (`0x6F047C80`), the gold trip (`0x6F060890`),
//!   the mine withdrawal (`0x6F09E220`), the lumber hit (`0x6F060780`) and the swing timer;
//! - the cancel refund of a train slot (`0x6F062320`, train branch), `F2I(MUL(I2R(cost), rate))`.
//!
//! Not modelled (see the spec): the training progress tick, spawn and rally, the nearest town hall
//! choice, the statistic counters that `0x6F02D660` adds to, the event 0x6F02D660 keys, the floating
//! text, the sound calls, and the hook `0x6F09E220` calls when a mine is empty.
//!
//! Units: a stored amount is "x10" (`stock`, `amount10`); the script value is the stored value / 10.
//! Upkeep percentages are integers (30 means 30 percent). Everything integer is wrapping 32-bit like the
//! original; division truncates toward zero like the C code. No floating point is used except through
//! `softfloat`, and the swing interval compares two f32 values exactly like the x87 compare.

use softfloat::{arith, conv, math, F32};

use crate::timer::{TimerId, TimerQueue};

/// Upper bound of a stored gold or lumber amount, in x10 units (`0x989680`, CONFIRMED).
pub const RAW_CAP: i32 = 10_000_000;

/// Number of ally slots the share table covers (CONFIRMED: the loop runs 12 slots).
pub const PLAYER_SLOTS: usize = 12;

/// Number of upkeep tiers (CONFIRMED: the readers take an index clamped to 9).
pub const TIERS: usize = 10;

/// Bits of the float 100.0 at `0x6FAAE574` (CONFIRMED value, `attack-damage.md`).
pub const PERCENT_CONST_BITS: u32 = 0x42C8_0000;

/// Training queue capacity (CONFIRMED: `0x6F05DF60` stores 7 at +0x28 and +0x30).
pub const TRAIN_QUEUE_CAPACITY: usize = 7;

/// The swing interval floor, 0.01 (CONFIRMED constant at `0x6FAAE480`).
pub const SWING_FLOOR: f32 = 0.01;

/// Gold or lumber. The discriminant is the resource id the original passes (1 and 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Res {
    Gold = 0,
    Lumber = 1,
}

impl Res {
    fn idx(self) -> usize {
        self as usize
    }
}

/// The flags of the changer. The third flag of the original (event on the remainder) only feeds
/// statistic counters that are not modelled, so it is not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags {
    /// Upkeep tax (the percent path, flag A).
    pub tax: bool,
    /// Ally share (flag B).
    pub share: bool,
}

impl Flags {
    /// The flags the deposit and the harvest income use: tax and share on (`1, 1, 1`).
    pub const INCOME: Flags = Flags { tax: true, share: true };
    /// No flags: a plain credit (refunds).
    pub const NONE: Flags = Flags { tax: false, share: false };
}

/// The per-player resource state the changer reads and writes.
///
/// `stock[r]` is the x10 stored amount (the value at object key `r + 4`, CONFIRMED). `tax_percent[r]` is the
/// upkeep rate written by [`UpkeepTable::refresh_rates`] (key `r + 16`, CONFIRMED). `ally_share[r][i]` is the
/// percent this player gives to player slot `i` (the table at object `+0x1E4` for gold and `+0x1E4 + 0x34` for
/// lumber, CONFIRMED layout, meaning LIKELY ally sharing).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerEconomy {
    /// The player's slot index (object `+0x30`, a byte). The share loop skips this slot.
    pub index: usize,
    pub stock: [i32; 2],
    pub tax_percent: [i32; 2],
    pub ally_share: [[i32; PLAYER_SLOTS]; 2],
    /// Flag `0x40` of the player's tracked object (`0x6F2D3290`): a lumber deposit is worth 1 and a gold
    /// trip takes 1 unit. Meaning UNSURE, the flag is read as data.
    pub one_per_trip: bool,
    /// Flag `0x10000` of the player's tracked object (`0x6F2D44C0`): the deposit is doubled when the worker
    /// also has its own flag bit `0x4` (see [`Worker::owner_double`]). Meaning UNSURE.
    pub double_bonus: bool,
}

impl PlayerEconomy {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            ..Self::default()
        }
    }

    /// The x10 stored amount of `res`.
    pub fn stored(&self, res: Res) -> i32 {
        self.stock[res.idx()]
    }
}

/// The resource changer `0x6F4167D0` for player `p`, with the amount already x10.
///
/// Returns the part of `amount10` that was credited to the stock (after the tax and the ally shares), x10.
/// The wrappers divide it by 10. Order of the steps, as in the original:
/// 1. `amount10 == 0` returns 0.
/// 2. Tax (flag A): `tax10 = rate * amount10 / 100` (integer, wrapping multiply, truncating divide).
/// 3. Ally share (flag B), slots 0..11 in order, skipping the owner slot: `share = pct * amount10 / 100`,
///    clamped to what is left, credited to that player with flags `tax = true, share = false`. Only
///    resources 0 and 1 (gold, lumber) have a table; the remaining amount shrinks by each share.
/// 4. Tax taken from what is left: `tax = min(tax10, left)`.
/// 5. The left amount is added to the stock; for gold and lumber the sum is clamped to `[0, RAW_CAP]`.
pub fn change(players: &mut [PlayerEconomy], p: usize, res: Res, amount10: i32, flags: Flags) -> i32 {
    if amount10 == 0 {
        return 0;
    }
    let mut left = amount10;
    let mut tax10 = 0;
    if flags.tax {
        tax10 = players[p].tax_percent[res.idx()].wrapping_mul(amount10) / 100;
    }
    if flags.share {
        let owner = players[p].index;
        // The original always has 12 slots. A shorter list has no player past its end, so no share goes there.
        for slot in 0..players.len().min(PLAYER_SLOTS) {
            if left == 0 {
                break;
            }
            if slot == owner {
                continue;
            }
            let pct = players[p].ally_share[res.idx()][slot];
            if pct == 0 {
                continue;
            }
            let mut share = pct.wrapping_mul(amount10) / 100;
            if share == 0 {
                continue;
            }
            if share >= left {
                share = left;
            }
            change(players, slot, res, share, Flags { tax: true, share: false });
            left -= share;
        }
    }
    if flags.tax {
        if tax10 >= left {
            tax10 = left;
        }
        left -= tax10;
    }
    if left != 0 {
        let sum = players[p].stock[res.idx()].wrapping_add(left);
        players[p].stock[res.idx()] = sum.clamp(0, RAW_CAP);
    }
    left
}

/// The script-level credit: `amount` in script units, through the changer (the wrapper `0x6F416A00` /
/// `0x6F416A40`), returning the credited amount in script units (integer division by 10).
pub fn credit(players: &mut [PlayerEconomy], p: usize, res: Res, amount: i32, flags: Flags) -> i32 {
    change(players, p, res, amount.wrapping_mul(10), flags) / 10
}

/// The upkeep tables (`UpkeepUsage`, `UpkeepGoldTax`, `UpkeepLumberTax`).
///
/// The defaults are the values the 1.26a image gives when the MiscData keys are absent: the thresholds of
/// row 0 (CONFIRMED from the image), and the tax floats the start-up initialisers store (CONFIRMED by the
/// Unicorn run in `tools/emu/economy_vectors.py`: 0.0, 0.30, then 0.60 for tiers 2 to 9).
#[derive(Debug, Clone, PartialEq)]
pub struct UpkeepTable {
    pub thresholds: [i32; TIERS],
    pub gold_tax: [F32; TIERS],
    pub lumber_tax: [F32; TIERS],
}

impl Default for UpkeepTable {
    fn default() -> Self {
        let mut thresholds = [10_000; TIERS];
        thresholds[0] = 40;
        thresholds[1] = 70;
        let mut gold_tax = [F32(0x0000_0000); TIERS];
        gold_tax[1] = F32(0x3E99_999A);
        for t in gold_tax.iter_mut().skip(2) {
            *t = F32(0x3F19_999A);
        }
        Self {
            thresholds,
            gold_tax,
            lumber_tax: [F32(0); TIERS],
        }
    }
}

impl UpkeepTable {
    /// The threshold at `index`, 0 past the table (the reader returns 0 for index > 9).
    fn threshold(&self, index: usize) -> i32 {
        self.thresholds.get(index).copied().unwrap_or(0)
    }

    /// The upkeep tier for `food_used` (`0x6F40F620`, CONFIRMED from the instructions).
    ///
    /// Starting at index 0, the tier is the first index whose threshold is at least `food_used`
    /// (signed compare). The walk also ends at the first zero entry, and that index is the tier. With
    /// no threshold at all (`thresholds[0] == 0`) the tier is 0.
    pub fn tier_for_food(&self, food_used: i32) -> usize {
        let mut i = 0usize;
        if self.threshold(0) == 0 {
            return 0;
        }
        loop {
            if self.threshold(i) >= food_used {
                return i;
            }
            i += 1;
            if self.threshold(i) == 0 {
                return i;
            }
        }
    }

    /// The tax percent of a tax float: `F2I(ROUND(MUL(tax, 100.0)))` (CONFIRMED order of the soft-float calls).
    pub fn tax_percent(tax: F32) -> i32 {
        conv::f2i(math::round(arith::mul(tax, F32(PERCENT_CONST_BITS))))
    }

    /// Recomputes both tax percents of `player` for `food_used` (the body of `0x6F40F620`). The tier index is
    /// clamped to 9 for the tax read (the reader clamps its index).
    pub fn refresh_rates(&self, player: &mut PlayerEconomy, food_used: i32) {
        let tier = self.tier_for_food(food_used).min(TIERS - 1);
        player.tax_percent[Res::Gold.idx()] = Self::tax_percent(self.gold_tax[tier]);
        player.tax_percent[Res::Lumber.idx()] = Self::tax_percent(self.lumber_tax[tier]);
    }
}

/// The economy constants that the host reads from Misc (`docs/spec/economy-3.md`, sections 2 and 4), or the built-in
/// defaults when no `[Misc]` layer exists. The engine chooses between the two and reports which one it used.
#[derive(Debug, Clone, PartialEq)]
pub struct EconomyConstants {
    /// `UpkeepUsage`, `UpkeepGoldTax` and `UpkeepLumberTax`.
    pub upkeep: UpkeepTable,
    /// `TrainRefundRate`.
    pub train_refund_rate: F32,
    /// `ResearchRefundRate`.
    pub research_refund_rate: F32,
    /// `ReviveRefundRate`.
    pub revive_refund_rate: F32,
    /// `LowGoldAmount` as the integer the mine compare uses (`F2I` of the real, UNSURE: the compare type is not
    /// traced, the reader family returns a real).
    pub low_gold_amount: i32,
}

impl Default for EconomyConstants {
    /// The built-in defaults: the upkeep table of [`UpkeepTable::default`], zero refund rates, and no low-gold
    /// threshold (UNSURE: the original's default for `LowGoldAmount` is not traced; 0 is the reader's default).
    fn default() -> Self {
        Self {
            upkeep: UpkeepTable::default(),
            train_refund_rate: F32::ZERO,
            research_refund_rate: F32::ZERO,
            revive_refund_rate: F32::ZERO,
            low_gold_amount: 0,
        }
    }
}

/// Which cancelled slot a refund comes from: the rate key differs, the arithmetic does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundKind {
    Train,
    Research,
    Revive,
}

impl EconomyConstants {
    /// The refund rate for a cancelled slot of `kind`.
    pub fn refund_rate(&self, kind: RefundKind) -> F32 {
        match kind {
            RefundKind::Train => self.train_refund_rate,
            RefundKind::Research => self.research_refund_rate,
            RefundKind::Revive => self.revive_refund_rate,
        }
    }
}

/// The cancel refund of one resource: `F2I(MUL(I2R(cost), rate))` (`0x6F062320`, train branch, CONFIRMED order).
pub fn refund_units(cost: i32, rate: F32) -> i32 {
    conv::f2i(arith::mul(conv::i2r(cost), rate))
}

/// Refunds a cancelled train slot to `owner`: gold, then lumber, through the plain changer (no tax, no share).
///
/// `refund_applies` is `false` when the slot's bit in the owner's mask is set. The original then adds 1 to a
/// counter instead of refunding (CONFIRMED path, the counter is not modelled), so nothing is credited here.
/// Returns the credited (gold, lumber) in script units.
pub fn cancel_train_refund(
    players: &mut [PlayerEconomy],
    owner: usize,
    gold_cost: i32,
    lumber_cost: i32,
    rate: F32,
    refund_applies: bool,
) -> (i32, i32) {
    if !refund_applies {
        return (0, 0);
    }
    let gold = credit(players, owner, Res::Gold, refund_units(gold_cost, rate), Flags::NONE);
    let lumber = credit(players, owner, Res::Lumber, refund_units(lumber_cost, rate), Flags::NONE);
    (gold, lumber)
}

/// The food room check of `0x6F416A80`: `true` when `used + need <= cap` in unsigned compare, or when the
/// owner is exempt (flag `0x10` of the owner's type, LIKELY "does not use food"). `cap` is the food cap.
pub fn food_room(used: i32, cap: i32, need: i32, exempt: bool) -> bool {
    if exempt {
        return true;
    }
    !((cap as u32) < (used.wrapping_add(need) as u32))
}

/// The food cap of a player (`0x6F40F850`, CONFIRMED): the minimum of the two supply values and the
/// `FoodCeiling` value.
pub fn food_cap(supply_a: i32, supply_b: i32, ceiling: i32) -> i32 {
    supply_a.min(supply_b).min(ceiling)
}

/// A gold mine's remaining gold (the `Agld` ability, `+0x6C`, CONFIRMED).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mine {
    pub remaining: i32,
}

impl Mine {
    /// The withdrawal `0x6F09E220`: takes `min(amount, remaining)` (signed compare) and returns what was taken.
    /// A negative request is taken as is (the original does the same subtraction).
    pub fn take(&mut self, amount: i32) -> i32 {
        let mut a = amount;
        if self.remaining < a {
            a = self.remaining;
        }
        self.remaining -= a;
        a
    }
}

/// The advisor notice a gold trip can raise (`0x6F060890`): event 0xDE when the mine is empty, 0xDF when a
/// mine that was above the low-gold threshold falls to it or below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MineNotice {
    None,
    /// Mine low (event 0xDF).
    Low,
    /// Mine depleted (event 0xDE).
    Depleted,
}

/// A worker's harvest state (the ability object of the gatherer). Fields are named after the offsets they came
/// from (CONFIRMED offsets, see the spec).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worker {
    /// The worker's player slot (the owner the deposit credits; `0x6F3A1650` in the original).
    pub owner: usize,
    /// Carried amount in script units, `+0x1C4`. Zero means empty.
    pub carry: i32,
    /// Carry type, bit `0x100` of `+0x20`: `true` = lumber.
    pub carrying_lumber: bool,
    /// Gold trip size, `+0x194` (CONFIRMED read; the value is data).
    pub cap_gold: i32,
    /// Lumber carry cap, `+0x198` (CONFIRMED).
    pub cap_lumber: i32,
    /// Lumber per hit, `+0x1D8` (CONFIRMED: the int that `0x6F060780` adds per hit).
    pub lumber_per_hit: i32,
    /// Bit `0x4` of the worker unit's flag byte at `+0x5C` (CONFIRMED). Together with
    /// [`PlayerEconomy::double_bonus`] it doubles the deposit.
    pub owner_double: bool,
    /// Bit `0x8000` of `+0x20`: the gold trip is running (CONFIRMED gate of `0x6F060890`).
    pub mining: bool,
}

/// A timer payload for the lumber swing timer (event 0xD01B2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingTimer {
    pub worker: usize,
}

impl Worker {
    /// A new, empty worker for player `owner`.
    pub fn new(owner: usize, cap_gold: i32, cap_lumber: i32, lumber_per_hit: i32) -> Self {
        Self {
            owner,
            carry: 0,
            carrying_lumber: false,
            cap_gold,
            cap_lumber,
            lumber_per_hit,
            owner_double: false,
            mining: false,
        }
    }

    /// The deposit `0x6F047C80`: credits the carry to the owner through the income changer (tax, share), and
    /// clears the carry. Returns the credited amount in script units.
    ///
    /// The amount is the carry, replaced by 1 for lumber when the player's `one_per_trip` is set, and doubled when
    /// both `owner_double` and the player's `double_bonus` are set (CONFIRMED order of the two checks).
    pub fn deposit(&mut self, players: &mut [PlayerEconomy]) -> i32 {
        if self.carry == 0 {
            return 0;
        }
        let p = self.owner;
        let res = if self.carrying_lumber { Res::Lumber } else { Res::Gold };
        let mut amount = self.carry;
        if self.carrying_lumber && players[p].one_per_trip {
            amount = 1;
        }
        if self.owner_double && players[p].double_bonus {
            amount = amount.wrapping_mul(2);
        }
        let credited = credit(players, p, res, amount, Flags::INCOME);
        self.carry = 0;
        credited
    }

    /// The carry add `0x6F055610`. Returns `true` when the carry reached the cap (`cap <= carry + amount`).
    ///
    /// If the worker carries the other resource, that carry is deposited first. The new carry is
    /// `min(carry + amount, cap)`. A zero amount returns `false` and changes nothing.
    pub fn carry_add(&mut self, players: &mut [PlayerEconomy], amount: i32, lumber: bool) -> bool {
        if amount == 0 {
            return false;
        }
        if self.carry != 0 && lumber != self.carrying_lumber {
            self.deposit(players);
        }
        let cap = if lumber { self.cap_lumber } else { self.cap_gold };
        let sum = self.carry.wrapping_add(amount);
        let full = cap <= sum;
        self.carry = if full { cap } else { sum };
        self.carrying_lumber = lumber;
        full
    }

    /// The gold trip `0x6F060890`, run when the worker is at a mine.
    ///
    /// Gate: [`Worker::mining`]. The request is `1` when the owner's `one_per_trip` is set, else `cap_gold`.
    /// The mine gives `min(request, remaining)`, the worker carries it (`carry_add`, gold), and the notice
    /// is raised when `notify` (the original's visibility check). `low_gold` is the `LowGoldAmount` value.
    /// Returns `(taken, notice)`.
    pub fn gold_trip(
        &mut self,
        players: &mut [PlayerEconomy],
        mine: &mut Mine,
        low_gold: i32,
        notify: bool,
    ) -> (i32, MineNotice) {
        if !self.mining {
            return (0, MineNotice::None);
        }
        let was_above = low_gold < mine.remaining;
        let request = if players[self.owner].one_per_trip { 1 } else { self.cap_gold };
        let taken = mine.take(request);
        self.carry_add(players, taken, false);
        if taken == 0 || !notify {
            return (taken, MineNotice::None);
        }
        if mine.remaining <= 0 {
            return (taken, MineNotice::Depleted);
        }
        if was_above && mine.remaining <= low_gold {
            return (taken, MineNotice::Low);
        }
        (taken, MineNotice::None)
    }

    /// The swing interval the handler arms: `max(configured, 0.01)`, with NaN kept (CONFIRMED compare of the
    /// `fcompp` form, `0x6F060780`).
    pub fn swing_interval(configured: f32) -> f32 {
        if configured < SWING_FLOOR {
            SWING_FLOOR
        } else {
            configured
        }
    }

    /// One lumber swing `0x6F060780`. The next swing is armed first (one-shot, so the handler re-arms it), then, if
    /// the harvest target is valid, one hit adds `lumber_per_hit` to the carry as lumber. Returns `true` on a hit.
    pub fn lumber_swing(
        &mut self,
        timers: &mut TimerQueue<SwingTimer>,
        players: &mut [PlayerEconomy],
        configured_interval: f32,
        target_valid: bool,
    ) -> (TimerId, bool) {
        let id = timers.start(Self::swing_interval(configured_interval), false, SwingTimer { worker: self.owner });
        if !target_valid {
            return (id, false);
        }
        self.carry_add(players, self.lumber_per_hit, true);
        (id, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_players() -> Vec<PlayerEconomy> {
        vec![PlayerEconomy::new(0), PlayerEconomy::new(1)]
    }

    #[test]
    fn default_upkeep_tiers_follow_the_thresholds() {
        let u = UpkeepTable::default();
        assert_eq!(u.tier_for_food(0), 0);
        assert_eq!(u.tier_for_food(40), 0);
        assert_eq!(u.tier_for_food(41), 1);
        assert_eq!(u.tier_for_food(70), 1);
        assert_eq!(u.tier_for_food(71), 2);
        assert_eq!(u.tier_for_food(9_999), 2);
        // Past the last real threshold the walk reaches the zero entry past index 9. The raw tier is 10; the tax
        // read clamps it to 9 (see refresh_rates).
        assert_eq!(u.tier_for_food(10_001), 10);
        let mut p = PlayerEconomy::new(0);
        u.refresh_rates(&mut p, 10_001);
        assert_eq!(p.tax_percent[0], 60);
    }

    #[test]
    fn tier_walk_stops_at_the_first_zero_entry() {
        let u = UpkeepTable {
            thresholds: [10, 20, 0, 99, 0, 0, 0, 0, 0, 0],
            ..UpkeepTable::default()
        };
        assert_eq!(u.tier_for_food(5), 0);
        assert_eq!(u.tier_for_food(15), 1);
        // 25 is above every non-zero entry before index 2: the zero entry at 2 ends the walk.
        assert_eq!(u.tier_for_food(25), 2);
        let none = UpkeepTable {
            thresholds: [0; TIERS],
            ..UpkeepTable::default()
        };
        assert_eq!(none.tier_for_food(1_000), 0);
    }

    #[test]
    fn default_rates_are_zero_thirty_sixty() {
        let u = UpkeepTable::default();
        let mut p = PlayerEconomy::new(0);
        u.refresh_rates(&mut p, 10);
        assert_eq!(p.tax_percent, [0, 0]);
        u.refresh_rates(&mut p, 50);
        assert_eq!(p.tax_percent, [30, 0]);
        u.refresh_rates(&mut p, 200);
        assert_eq!(p.tax_percent, [60, 0]);
    }

    #[test]
    fn income_is_taxed_by_the_rate_and_truncates() {
        let mut ps = two_players();
        ps[0].tax_percent[0] = 30;
        // 10 gold = 100 x10. Tax 30 x10 = 3 gold; 7 gold credited.
        let credited = credit(&mut ps, 0, Res::Gold, 10, Flags::INCOME);
        assert_eq!(credited, 7);
        assert_eq!(ps[0].stock[0], 70);
        // Truncation: 33 percent of 30 (x10) is 990 / 100 = 9 (9.9 truncates), so 21 is credited.
        ps[0].tax_percent[0] = 33;
        let c = credit(&mut ps, 0, Res::Gold, 3, Flags::INCOME);
        assert_eq!(c, 2);
        assert_eq!(ps[0].stock[0], 70 + 21);
    }

    #[test]
    fn plain_credit_has_no_tax_and_clamps_at_the_cap() {
        let mut ps = two_players();
        ps[0].tax_percent[1] = 90;
        ps[0].stock[1] = RAW_CAP - 5;
        let c = credit(&mut ps, 0, Res::Lumber, 10, Flags::NONE);
        assert_eq!(c, 10);
        assert_eq!(ps[0].stock[1], RAW_CAP);
        // A negative stock result clamps to zero.
        let c2 = credit(&mut ps, 0, Res::Lumber, -100_000_000, Flags::NONE);
        assert_eq!(c2, -100_000_000);
        assert_eq!(ps[0].stock[1], 0);
    }

    #[test]
    fn ally_share_goes_first_and_is_taxed_on_the_other_side() {
        let mut ps = vec![PlayerEconomy::new(0), PlayerEconomy::new(1), PlayerEconomy::new(2)];
        ps[0].ally_share[0][1] = 50;
        ps[0].tax_percent[0] = 20;
        // 100 x10 in. Share 50 percent to slot 1 = 50 x10 (player 1 gets it, with its own tax 0).
        // Then tax 20 percent of the whole amount = 20 x10, clamped to the 50 that is left: 20 taken.
        let left = change(&mut ps, 0, Res::Gold, 100, Flags::INCOME);
        assert_eq!(ps[1].stock[0], 50);
        assert_eq!(left, 30);
        assert_eq!(ps[0].stock[0], 30);
    }

    #[test]
    fn owner_slot_is_never_shared_to() {
        let mut ps = two_players();
        ps[0].ally_share[0][0] = 100;
        let left = change(&mut ps, 0, Res::Gold, 100, Flags { tax: false, share: true });
        assert_eq!(left, 100);
        assert_eq!(ps[0].stock[0], 100);
    }

    #[test]
    fn refund_is_truncated_product() {
        assert_eq!(refund_units(100, F32(0x3E80_0000)), 25); // 100 * 0.25
        assert_eq!(refund_units(75, F32(0x3F00_0000)), 37); // 37.5 truncates
        assert_eq!(refund_units(0, F32(0x3F80_0000)), 0);
    }

    #[test]
    fn cancel_refund_credits_gold_then_lumber_without_tax() {
        let mut ps = two_players();
        ps[0].tax_percent = [50, 50];
        let (g, l) = cancel_train_refund(&mut ps, 0, 400, 200, F32(0x3F00_0000), true);
        assert_eq!((g, l), (200, 100));
        assert_eq!(ps[0].stock, [2000, 1000]);
        let (g2, l2) = cancel_train_refund(&mut ps, 0, 400, 200, F32(0x3F00_0000), false);
        assert_eq!((g2, l2), (0, 0));
        assert_eq!(ps[0].stock, [2000, 1000]);
    }

    #[test]
    fn food_room_is_an_unsigned_compare() {
        assert!(food_room(10, 20, 10, false));
        assert!(!food_room(10, 20, 11, false));
        assert!(food_room(100, 20, 500, true));
        // A negative cap (as unsigned) is huge: the check passes, like the original.
        assert!(food_room(0, -1, 1, false));
    }

    #[test]
    fn food_cap_takes_the_minimum_then_the_ceiling() {
        assert_eq!(food_cap(50, 40, 90), 40);
        assert_eq!(food_cap(120, 100, 90), 90);
        assert_eq!(food_cap(95, 200, 90), 90);
        assert_eq!(food_cap(95, 200, 100), 95);
    }

    #[test]
    fn mine_withdrawal_takes_the_smaller_amount() {
        let mut m = Mine { remaining: 25 };
        assert_eq!(m.take(10), 10);
        assert_eq!(m.remaining, 15);
        assert_eq!(m.take(10), 10);
        assert_eq!(m.take(10), 5);
        assert_eq!(m.remaining, 0);
        assert_eq!(m.take(10), 0);
    }

    #[test]
    fn carry_fills_to_the_cap_and_changes_type_by_depositing() {
        let mut ps = two_players();
        let mut w = Worker::new(0, 10, 20, 5);
        assert!(!w.carry_add(&mut ps, 0, false));
        assert!(!w.carry_add(&mut ps, 6, false));
        assert_eq!(w.carry, 6);
        assert!(w.carry_add(&mut ps, 6, false));
        assert_eq!(w.carry, 10);
        // Lumber now: the gold carry is deposited first (no tax set, so 10 gold is credited).
        assert!(!w.carry_add(&mut ps, 5, true));
        assert_eq!(ps[0].stock[0], 100);
        assert_eq!(w.carry, 5);
        assert!(w.carrying_lumber);
    }

    #[test]
    fn lumber_swing_arms_then_hits_when_target_is_valid() {
        let mut ps = two_players();
        let mut timers: TimerQueue<SwingTimer> = TimerQueue::new();
        let mut w = Worker::new(0, 10, 20, 5);
        let (_, hit) = w.lumber_swing(&mut timers, &mut ps, 0.001, false);
        assert!(!hit);
        assert_eq!(w.carry, 0);
        assert_eq!(timers.len(), 1);
        let (_, hit) = w.lumber_swing(&mut timers, &mut ps, 1.0, true);
        assert!(hit);
        assert_eq!(w.carry, 5);
        assert!(w.carrying_lumber);
        // The floor: 0.001 was raised to 0.01.
        assert_eq!(timers.next_expiry(), Some(0.01));
    }

    #[test]
    fn swing_interval_keeps_nan_and_large_values() {
        assert_eq!(Worker::swing_interval(0.0), 0.01);
        assert_eq!(Worker::swing_interval(0.5), 0.5);
        assert!(Worker::swing_interval(f32::NAN).is_nan());
    }

    #[test]
    fn deposit_doubles_and_replaces_lumber_by_one_per_flags() {
        let mut ps = two_players();
        let mut w = Worker::new(0, 10, 20, 5);
        w.carry = 8;
        w.owner_double = true;
        ps[0].double_bonus = true;
        assert_eq!(w.deposit(&mut ps), 16);
        assert_eq!(ps[0].stock[0], 160);
        assert_eq!(w.carry, 0);
        w.carry_add(&mut ps, 8, true);
        ps[0].one_per_trip = true;
        ps[0].double_bonus = false;
        assert_eq!(w.deposit(&mut ps), 1);
        assert_eq!(ps[0].stock[1], 10);
    }

    #[test]
    fn gold_trip_takes_the_cap_and_raises_low_then_depleted() {
        let mut ps = two_players();
        let mut w = Worker::new(0, 10, 20, 5);
        w.mining = true;
        let mut mine = Mine { remaining: 25 };
        let (t, n) = w.gold_trip(&mut ps, &mut mine, 10, true);
        assert_eq!((t, n), (10, MineNotice::None));
        assert_eq!(w.carry, 10);
        w.carry = 0;
        // 15 left, above 10, takes 10 -> 5 <= 10 and was above: low notice.
        let (t, n) = w.gold_trip(&mut ps, &mut mine, 10, true);
        assert_eq!((t, n), (10, MineNotice::Low));
        w.carry = 0;
        let (t, n) = w.gold_trip(&mut ps, &mut mine, 10, true);
        assert_eq!((t, n), (5, MineNotice::Depleted));
        assert_eq!(w.carry, 5);
    }

    #[test]
    fn gold_trip_without_mining_or_notify_does_nothing_visible() {
        let mut ps = two_players();
        let mut w = Worker::new(0, 10, 20, 5);
        let mut mine = Mine { remaining: 25 };
        assert_eq!(w.gold_trip(&mut ps, &mut mine, 10, true), (0, MineNotice::None));
        w.mining = true;
        assert_eq!(w.gold_trip(&mut ps, &mut mine, 10, false), (10, MineNotice::None));
        assert_eq!(mine.remaining, 15);
    }

    #[test]
    fn swing_handler_runs_on_the_agile_clock_regardless_of_speed() {
        // Two workers swinging every second of agile time. The hit count after N ticks depends only on the
        // clock, not on how many ticks a turn holds.
        use crate::lockstep::TICK_INCREMENT_BITS;
        let run = |ticks_per_turn: u64| {
            let mut ps = two_players();
            let mut w = Worker::new(0, 10, 20, 5);
            let mut timers: TimerQueue<SwingTimer> = TimerQueue::new();
            let mut hits = 0;
            let mut clock = crate::timer::AgileClock::new();
            w.lumber_swing(&mut timers, &mut ps, 1.0, true);
            let mut done = 0u64;
            while done < 400 {
                for _ in 0..ticks_per_turn {
                    clock.advance(TICK_INCREMENT_BITS);
                }
                done += ticks_per_turn;
                let target = clock.now();
                timers.pump(target, |q, _| {
                    let (_, hit) = w.lumber_swing(q, &mut ps, 1.0, true);
                    if hit {
                        hits += 1;
                    }
                });
            }
            hits
        };
        assert_eq!(run(1), run(40));
        let h = run(1);
        assert!((1..=3).contains(&h), "about two seconds of swings, got {h}");
    }
}
