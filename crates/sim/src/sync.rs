//! Components of the per-tick sync value (block 0x22), computed from simulation state.
//!
//! Source: docs/spec/lockstep.md section 13 (Game.dll 1.26a). The arithmetic of the value itself is in
//! `FUN_6f545840` (`0x6F545840`); the instruction order was re-read from the machine code (capstone), where
//! the value is `rol3(X-xor)` applied **twice** with X, then once with Y, then a final `rol3`:
//!
//! ```text
//! c      = rol(A ^ 0x17AB, 3)                  A = registry fold (section 13.4)
//! v      = T ^ c                               T = CNetData+0x1C70 (tick clock, section 13.5)
//! v      = rol(v,3) ^ X
//! v      = rol(v,3) ^ X                        X = world+0x40, or world+0x1C when that is -1
//! v      = rol(v,3) ^ Y                        Y = world+0x50
//! sync   = rol(v,3)
//! ```
//!
//! The producers of `A`, `X` and `Y` are not reproduced yet (their objects are not in the simulation); this module
//! provides the pieces that are confirmed and takes the three game-state words as arguments.

/// Constant XORed into the registry fold (`0x6F545840`: `xor eax, 0x17ab`).
pub const REGISTRY_XOR: u32 = 0x17AB;

/// Number of participant slots of registry 13 (`0x6F007D90` allocates 20 pointers).
pub const REGISTRY_SLOTS: usize = 20;

/// Game ticks advance `CNetData+0x1C70` by this many game milliseconds (`add [esi+0x1c70], ebp`, `0x6F553770`).
///
/// CONFIRMED (machine code, section 13.7): `ebp` is loaded from `[CNetData+0x139C]` at `0x6F553516`, but on the
/// path that runs ticks it is overwritten at `0x6F553676` by `lea ebp, [eax-0x14]` with `eax = 0x19`, i.e. the
/// 25 ms unit minus 20 = 5. The tick loop is only entered when the accumulator is at least 25 ms (`0x6F55366A`).
pub const TICK_CLOCK_STEP: u32 = TICK_UNIT_MS - TICK_UNIT_MS_REMAINDER;

/// The unit of the frame driver's tick accumulator, in ms (`cmp [esi+0x2250], 0x19`, `0x6F55366A`).
pub const TICK_UNIT_MS: u32 = 0x19;

/// The value subtracted from the unit to get the tick step (`lea ebp, [eax-0x14]`, `0x6F553676`).
pub const TICK_UNIT_MS_REMAINDER: u32 = 0x14;

/// Fold over the registry 13 participant slots, as `FUN_6f005e20` does.
///
/// The walk runs from the last slot down to the first, skips empty slots, and for each participant value `v`
/// computes `acc = rol(v ^ acc, 3)` starting from `acc = 0`.
pub fn registry_fold(slots: &[Option<u32>]) -> u32 {
    let mut acc = 0u32;
    for v in slots.iter().rev().flatten() {
        acc = (v ^ acc).rotate_left(3);
    }
    acc
}

/// `c = rol(A ^ 0x17AB, 3)`.
pub fn registry_term(fold: u32) -> u32 {
    (fold ^ REGISTRY_XOR).rotate_left(3)
}

/// Value of a registry participant of class `CMiscCustom` (vtable `0x6F8750A4`), its vtable slot 0 at
/// `0x6F0020D0` (CONFIRMED arithmetic from the machine code, section 13.7).
///
/// The node has two child links (`+4`, `+8`). `a` and `b` are the child hashes (`None` for a null link):
///
/// ```text
/// s = a ? rol(H(a), 3) : 0
/// H = b ? rol(H(b) ^ s, 3) : s
/// ```
///
/// How a child hash `H(a)` is computed (the walk `0x6F6FA990` → `0x6F4D0570`) is not reproduced here.
pub fn custom_node_hash(a: Option<u32>, b: Option<u32>) -> u32 {
    let s = a.map_or(0, |h| h.rotate_left(3));
    match b {
        Some(hb) => (hb ^ s).rotate_left(3),
        None => s,
    }
}

/// The registry fold A that gives the recorded sync `value` for clock `t` and world words `x`, `y`: the inverse of
/// [`sync_value`]. Every value implies exactly one A once T, X and Y are fixed.
pub fn implied_registry_fold(value: u32, t: u32, x: u32, y: u32) -> u32 {
    let v3 = value.rotate_right(3);
    let v2 = (v3 ^ y).rotate_right(3);
    let v1 = (v2 ^ x).rotate_right(3);
    let v0 = (v1 ^ x).rotate_right(3);
    let c = v0 ^ t;
    c.rotate_right(3) ^ REGISTRY_XOR
}

/// The world words used by the sync value, read from the world object (`[0x6FAB778C]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldWords {
    /// `world+0x1C`.
    pub f1c: u32,
    /// `world+0x40`; `0xFFFFFFFF` means "use `f1c`".
    pub f40: u32,
    /// `world+0x50`.
    pub f50: u32,
}

impl WorldWords {
    /// X: `world+0x40`, or `world+0x1C` when `world+0x40` is `-1`.
    pub fn x(&self) -> u32 {
        if self.f40 == u32::MAX {
            self.f1c
        } else {
            self.f40
        }
    }

    /// Y: `world+0x50`.
    pub fn y(&self) -> u32 {
        self.f50
    }

    /// The world words after the world constructor (CONFIRMED stores in `FUN_6f46b120` at `0x6F46B120`:
    /// `+0x40 = -1`, `+0x50 = 0`). `+0x1C` is not stored by the constructor; it is taken as 0 because the object is
    /// allocated with flag 8 at `0x6F46D9xx` (zero fill, LIKELY; the allocator is Storm, not checked).
    pub fn at_start() -> Self {
        WorldWords {
            f1c: 0,
            f40: u32::MAX,
            f50: 0,
        }
    }
}

/// The tick clock `CNetData+0x1C70` (game milliseconds, one tick = [`TICK_CLOCK_STEP`]).
///
/// Reset to 0 by the CNetData init (`0x6F5505D6`, `0x6F54C5ED`); the tick loop of the frame driver adds the step
/// **before** it dispatches the tick (`0x6F553770`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickClock {
    value: u32,
}

impl TickClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current value of `CNetData+0x1C70`.
    pub fn value(&self) -> u32 {
        self.value
    }

    /// One game tick: adds [`TICK_CLOCK_STEP`] (wrapping, as the 32-bit add does).
    pub fn tick(&mut self) {
        self.value = self.value.wrapping_add(TICK_CLOCK_STEP);
    }

    /// Reset to 0 (the CNetData init).
    pub fn reset(&mut self) {
        self.value = 0;
    }
}

/// The sync value of section 13.3 from its four components (the same arithmetic as `w3g::sync::sync_value`).
pub fn sync_value(t: u32, registry_fold: u32, x: u32, y: u32) -> u32 {
    let c = registry_term(registry_fold);
    let mut v = t ^ c;
    v = v.rotate_left(3) ^ x;
    v = v.rotate_left(3) ^ x;
    v = v.rotate_left(3) ^ y;
    v.rotate_left(3)
}

/// The sync value of the current state: clock, registry slots and world words.
pub fn record_value(clock: &TickClock, registry: &[Option<u32>], world: &WorldWords) -> u32 {
    sync_value(clock.value(), registry_fold(registry), world.x(), world.y())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Step-by-step version written in the order of the machine code, used as the reference.
    fn asm_order(t: u32, a: u32, x: u32, y: u32) -> u32 {
        let c = (a ^ 0x17AB).rotate_left(3);
        let mut e = t ^ c;
        e = e.rotate_left(3);
        e ^= x;
        e = e.rotate_left(3);
        e ^= x;
        e = e.rotate_left(3);
        e ^= y;
        e.rotate_left(3)
    }

    #[test]
    fn value_matches_machine_code_order() {
        for (t, a, x, y) in [
            (0u32, 0u32, 0u32, 0u32),
            (110, 0x1234_5678, 0x9ABC_DEF0, 0x0F0F_0F0F),
            (9_749_884, 0xDEAD_BEEF, 0xFFFF_FFFF, 1),
            (u32::MAX, 7, 3, 5),
        ] {
            assert_eq!(sync_value(t, a, x, y), asm_order(t, a, x, y));
        }
    }

    #[test]
    fn value_is_affine_in_the_clock_for_fixed_state() {
        let (a, x, y) = (0x1357_9BDF, 0x2468_ACE0, 0x0BAD_F00D);
        for (t1, t2) in [(0u32, 5u32), (110, 220), (9_749_884, 9_749_775)] {
            let lhs = sync_value(t1, a, x, y) ^ sync_value(t2, a, x, y);
            assert_eq!(lhs, (t1 ^ t2).rotate_left(12));
        }
    }

    #[test]
    fn registry_fold_walks_from_the_last_slot_and_skips_empty_ones() {
        // Walk order: slot 2 first, then slot 0 (slot 1 is empty).
        let slots = [Some(0x11u32), None, Some(0x22)];
        let acc1 = 0x22u32.rotate_left(3);
        let acc2 = (0x11 ^ acc1).rotate_left(3);
        assert_eq!(registry_fold(&slots), acc2);
        assert_eq!(registry_fold(&[None; REGISTRY_SLOTS]), 0);
    }

    #[test]
    fn registry_term_uses_the_constant_and_rotation() {
        assert_eq!(registry_term(0), 0x17AB_u32.rotate_left(3));
    }

    #[test]
    fn world_x_falls_back_to_1c_only_for_minus_one() {
        let w = WorldWords { f1c: 7, f40: 9, f50: 3 };
        assert_eq!((w.x(), w.y()), (9, 3));
        let w = WorldWords { f1c: 7, f40: u32::MAX, f50: 3 };
        assert_eq!(w.x(), 7);
        let w = WorldWords { f1c: 7, f40: 0, f50: 3 };
        assert_eq!(w.x(), 0);
    }

    #[test]
    fn tick_clock_adds_five_per_tick_and_resets() {
        let mut c = TickClock::new();
        assert_eq!(c.value(), 0);
        for _ in 0..22 {
            c.tick();
        }
        assert_eq!(c.value(), 110);
        c.reset();
        assert_eq!(c.value(), 0);
        let mut w = TickClock { value: u32::MAX - 2 };
        w.tick();
        assert_eq!(w.value(), 2);
    }

    #[test]
    fn tick_step_is_the_25ms_unit_minus_20() {
        assert_eq!(TICK_UNIT_MS, 25);
        assert_eq!(TICK_CLOCK_STEP, 5);
        assert_eq!(TICK_UNIT_MS - TICK_UNIT_MS_REMAINDER, TICK_CLOCK_STEP);
    }

    #[test]
    fn custom_node_hash_follows_the_machine_code() {
        // Leaf: no children gives 0 (the object is empty).
        assert_eq!(custom_node_hash(None, None), 0);
        // Only the first link: rol(H(a), 3).
        assert_eq!(custom_node_hash(Some(0x1234_5678), None), 0x1234_5678u32.rotate_left(3));
        // Both links: rol(H(b) ^ rol(H(a),3), 3).
        let (ha, hb) = (0xDEAD_BEEFu32, 0x0BAD_CAFEu32);
        assert_eq!(custom_node_hash(Some(ha), Some(hb)), (hb ^ ha.rotate_left(3)).rotate_left(3));
        // Only the second link: its hash is not rotated before the final rol, and the first-link term is 0.
        assert_eq!(custom_node_hash(None, Some(hb)), hb.rotate_left(3));
    }

    #[test]
    fn implied_fold_inverts_the_sync_value() {
        for (a, t, x, y) in [
            (0u32, 0u32, 0u32, 0u32),
            (0x1A25_8A4E, 0, 0, 0),
            (0x1357_9BDF, 110, 0x2468_ACE0, 0x0BAD_F00D),
            (u32::MAX, u32::MAX, 3, 5),
        ] {
            let v = sync_value(t, a, x, y);
            assert_eq!(implied_registry_fold(v, t, x, y), a);
        }
        // The first real record of the 1.26a replay with T = X = Y = 0 implies A = 0x1A258A4E (not 0).
        assert_eq!(implied_registry_fold(0xCEF2_8D12, 0, 0, 0), 0x1A25_8A4E);
    }

    #[test]
    fn world_words_at_start_match_the_constructor() {
        let w = WorldWords::at_start();
        assert_eq!(w.x(), 0);
        assert_eq!(w.y(), 0);
        assert_eq!(w.f40, u32::MAX);
    }

    #[test]
    fn record_value_combines_the_components() {
        let mut clock = TickClock::new();
        clock.tick();
        let mut slots = [None; REGISTRY_SLOTS];
        slots[4] = Some(0xABCD);
        let world = WorldWords { f1c: 1, f40: 2, f50: 3 };
        let expected = sync_value(5, registry_fold(&slots), 2, 3);
        assert_eq!(record_value(&clock, &slots, &world), expected);
    }
}
