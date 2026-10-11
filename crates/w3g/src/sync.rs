//! The per-tick sync value written as block 0x22 (docs/spec/lockstep.md sections 12, 13 and 15).
//!
//! CONFIRMED from the machine code of `FUN_6f545840` (Game.dll 1.26a, `CNetData` sync step, called from the
//! time applier `FUN_6f5516e0` for player 0, result stored at `CNetData+0x1C64`) and re-checked by running the
//! original under Unicorn on 400 synthetic states (`tools/emu/net_checksum_emu.py`, 0 mismatches):
//!
//! ```text
//! A  = fold over the 20 engine-global slots of registry 13 (each contributes its vtable slot 0; fold rol(v ^ acc, 3))
//! c  = rol(A ^ 0x17AB, 3)
//! T  = CNetData+0x1C70 (the tick clock, +5 per tick)
//! X  = world+0x40 (world+0x1C when that is -1), Y = world+0x50        (world = the global at 0x6FAB778C)
//! R  = byte-hash sum of the 8-byte global generator state at [0x6FAB73D8]
//! v  = T ^ c
//! v  = rol(v,3) ^ X ; v = rol(v,3) ^ Y ; v = rol(v,3) ^ R
//! sync = rol(v,3)
//! ```
//!
//! Section 13 of the spec had `X` twice and called the RNG term dead; both were mistakes of the decompile reading.
//! The simulation-side pieces (the net checksum that fills registry slot 2, the stream tracker) are in
//! `sim::sync`; this module only has the arithmetic so that replay tools can use it without the simulation.
//!
//! The function is GF(2)-linear in every input: `sync(T) ^ sync(T') = rol(T ^ T', 12)` for fixed `A`, `X`, `Y`, `R`.

/// The sync value for one tick. See the module documentation for the inputs.
pub fn sync_value(tick: u32, registry_fold: u32, world_x: u32, world_y: u32, rng_hash: u32) -> u32 {
    let c = (registry_fold ^ 0x17AB).rotate_left(3);
    let mut v = tick ^ c;
    v = v.rotate_left(3) ^ world_x;
    v = v.rotate_left(3) ^ world_y;
    v = v.rotate_left(3) ^ rng_hash;
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
        let (a, x, y, r) = (0x1234_5678, 0x9ABC_DEF0, 0x0F0F_0F0F, 0x00AA_5500);
        for (t1, t2) in [(0u32, 5u32), (110, 220), (9_749_884, 9_749_775), (u32::MAX, 1)] {
            let lhs = sync_value(t1, a, x, y, r) ^ sync_value(t2, a, x, y, r);
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
