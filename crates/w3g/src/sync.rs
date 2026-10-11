//! The per-tick sync value written as block 0x22 (docs/spec/lockstep.md section 12).
//!
//! CONFIRMED from the machine code of `FUN_6f545840` (Game.dll 1.26a, `CNetData` sync step, called from the
//! time applier `FUN_6f5516e0` for player 0, result stored at `CNetData+0x1C64`):
//!
//! ```text
//! A  = fold over the sync participants of registry 13 (each contributes its vtable slot 0; fold rol(v ^ acc, 3))
//! c  = rol(A ^ 0x17AB, 3)
//! T  = CNetData+0x1C70 (the tick clock, +5 per tick), or the registry-walk value when CNetData+0x2F8 != 0
//! X  = world+0x40 (world+0x1C when that is -1), Y = world+0x50        (world = the global at 0x6FAB778C)
//! v  = T ^ c
//! v  = rol(v,3) ^ X ; v = rol(v,3) ^ X ; v = rol(v,3) ^ Y
//! sync = rol(v,3)
//! ```
//!
//! The eight-byte RNG hash computed in `FUN_6f545930` is not part of this value (its result is discarded in
//! `FUN_6f545840`). The inputs `A`, `X`, `Y` are game-state values that change during a game, so this module
//! takes them as arguments; their producers are not reproduced yet (UNSURE which objects register in registry 13).
//!
//! The function is GF(2)-affine in `T`: `sync(T) ^ sync(T') = rol(T ^ T', 12)` for fixed `A`, `X`, `Y`.

/// The sync value for one tick. See the module documentation for the inputs.
pub fn sync_value(tick: u32, registry_fold: u32, world_x: u32, world_y: u32) -> u32 {
    let c = (registry_fold ^ 0x17AB).rotate_left(3);
    let mut v = tick ^ c;
    v = v.rotate_left(3) ^ world_x;
    v = v.rotate_left(3) ^ world_x;
    v = v.rotate_left(3) ^ world_y;
    v.rotate_left(3)
}

/// Folds one participant value into the registry accumulator, as the vtable walk `FUN_6f005e20` does.
pub fn registry_fold_step(acc: u32, value: u32) -> u32 {
    (value ^ acc).rotate_left(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_is_affine_in_the_tick() {
        let (a, x, y) = (0x1234_5678, 0x9ABC_DEF0, 0x0F0F_0F0F);
        for (t1, t2) in [(0u32, 5u32), (110, 220), (9_749_884, 9_749_775), (u32::MAX, 1)] {
            let lhs = sync_value(t1, a, x, y) ^ sync_value(t2, a, x, y);
            assert_eq!(lhs, (t1 ^ t2).rotate_left(12));
        }
    }

    #[test]
    fn fold_matches_the_order_of_the_walk() {
        // Two participants, walked from the last index down: acc = rol(v ^ acc, 3) each time.
        let acc1 = registry_fold_step(0, 0x11);
        let acc2 = registry_fold_step(acc1, 0x22);
        assert_eq!(acc2, (0x22 ^ acc1).rotate_left(3));
    }
}
