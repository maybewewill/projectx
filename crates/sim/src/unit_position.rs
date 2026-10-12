//! Unit position storage in cell coordinates (`docs/spec/unit-position.md`).
//!
//! The original keeps a unit's position as cell coordinates (unit+0x78 for x, unit+0x7C for y), not as world
//! coordinates. Writes go through the cell setter `0x6F474250` (CONFIRMED): the world origin is subtracted (x with
//! the world float at `W+0x6C`, y with `W+0x70`, CONFIRMED by the normaliser), the difference is multiplied by 1/32
//! by subtracting 5 from its exponent, and a result whose exponent is below 6 becomes `+0.0` (CONFIRMED bit rule).
//! Reads (`GetUnitX/Y`, the normaliser `0x6F4743A0`, CONFIRMED) multiply the cell coordinate by 32 (add 5 to a
//! non-zero exponent) and add the origin.
//!
//! The normaliser also has a cell term `v` (unit+0x70/+0x74 against the grid `G+0x40/+0x44`, scaled by the step
//! vector unit+0x80/+0x84). Every position write refreshes the term from the grid it uses (`0x6F497440`,
//! CONFIRMED callers), so at read time `v` is `+0.0` for a unit on grid A (unit+0x14 bit 31 clear), whatever the
//! grid values are (the grid values and the step vector drop out). The model therefore has no cell term: it holds
//! the cell coordinates and the cached world coordinates `Unit.x/y`, which are always the normaliser's output for
//! the current cells. Grid B (bit 31 set) and a stale snapshot are not modelled (UNSURE, see the spec).
//!
//! Model-level rules (LIKELY, each checked in the spec): SetUnitX/SetUnitY send both coordinates to the setter, the
//! other one as read back by the normaliser (`0x6F4743C0` pair, CONFIRMED in the harness), so that coordinate is
//! re-rounded too. Creation stores the placement and then runs the init's read-and-write-back of the same position
//! (`0x6F2A0E30` reads through `vf46` and writes with `0x6F474250`, CONFIRMED).
//!
//! Arithmetic: softfloat (`softfloat::arith`), the same truncating operations as the original.

use softfloat::{arith, F32};

use crate::natives::game::{Game, ObjKey, Object, Playable};

/// The margin subtracted from the playable maximum by the clamp (the float at `0x6FAAE564`, 32.0 after start-up,
/// CONFIRMED by the Unicorn vectors of `tools/emu/unit_clamp_emu.py`).
pub const PLAYABLE_CLAMP_MARGIN: F32 = F32::from_bits(0x4200_0000);

/// Clamps the pair (x, y) into the playable area, as the real `0x6F469490` does (CONFIRMED, Unicorn 3,000 cases,
/// `docs/spec/unit-position.md` 3a). x is kept in [min x, max x - 32], y in [min y, max y - 32]. The lower bound is
/// tested first: a value below it becomes the bound and the upper bound is not applied. Otherwise a value above the
/// upper bound becomes that bound. The compares are IEEE (NaN is kept), as the original's x87 `fcompp` sequence.
pub fn clamp_to_playable(p: Playable, x: F32, y: F32) -> (F32, F32) {
    (
        clamp_axis(x, p.min_x, p.max_x),
        clamp_axis(y, p.min_y, p.max_y),
    )
}

fn clamp_axis(v: F32, lo: F32, hi: F32) -> F32 {
    if lo.to_f32() > v.to_f32() {
        return lo;
    }
    let hi = arith::sub(hi, PLAYABLE_CLAMP_MARGIN);
    if hi.to_f32() < v.to_f32() {
        return hi;
    }
    v
}

/// The cell coordinate of a world coordinate `v` for the origin `origin` (the setter's arithmetic, CONFIRMED).
pub fn world_to_cell(v: F32, origin: F32) -> F32 {
    let d = arith::sub(v, origin);
    // The original adds 0xFD000000 and tests the sign bit. That is "magnitude < 6 << 23", i.e. exponent < 6: the
    // result becomes +0.0 (the mask yields zero). Otherwise the exponent drops by 5 (an exact multiply by 1/32).
    if d.0 & 0x7FFF_FFFF < 0x0300_0000 {
        F32::ZERO
    } else {
        F32(d.0.wrapping_add(0xFD80_0000))
    }
}

/// The stored cell coordinate after a write of world coordinate `v` over the stored cell `old` (CONFIRMED by the
/// Unicorn vectors, `docs/spec/unit-position.md` 3): the setter `0x6F4737D0` forms `delta = P - F` with `P` the new
/// cell (`world_to_cell`) and `F = old + step*v` (`0x6F38B0B0`), and the core writer `0x6F4A6FD0` stores
/// `(old + step*v) + delta`. All steps are softfloat, so the old magnitude sets the rounding.
pub fn stored_cell(old: F32, v: F32, origin: F32) -> F32 {
    // F = old + step * cell term; the step term is +0 at rest (the zero vector is written as +0.0), so F = old + 0.
    // The add keeps the original's zero-sign behaviour (a zero first operand returns the second one).
    let f = arith::add(old, F32::ZERO);
    let p = world_to_cell(v, origin);
    arith::add(f, arith::sub(p, f))
}

/// The world coordinate of a cell coordinate `c` for the origin `origin` (the normaliser with a zero cell term,
/// CONFIRMED): `32 * c + origin`, where the multiply adds 5 to a non-zero exponent.
pub fn cell_to_world(c: F32, origin: F32) -> F32 {
    let scaled = if c.0 & 0x7F80_0000 != 0 {
        F32(c.0.wrapping_add(0x0280_0000))
    } else {
        c
    };
    arith::add(scaled, origin)
}

/// The registrations behind the movement objects (docs/spec/lockstep.md section 17).
#[derive(Debug, Default)]
pub struct PrObjects {
    /// Each unit's own path object (unit+0xA8), created with the unit.
    pub(crate) unit_path: std::collections::BTreeMap<ObjKey, crate::sync::PrHandle>,
    /// The (cluster, path) pair of each unit that is part of a movement cluster.
    pub(crate) cluster: std::collections::BTreeMap<ObjKey, (crate::sync::PrHandle, crate::sync::PrHandle)>,
    /// Pairs the next walk releases.
    pub(crate) release: Vec<(crate::sync::PrHandle, crate::sync::PrHandle)>,
    /// Pairs of the missiles in flight, oldest first.
    pub(crate) missiles: std::collections::VecDeque<(crate::sync::PrHandle, crate::sync::PrHandle)>,
}

impl Game {
    /// Sync-registry hook, unit creation (docs/spec/lockstep.md section 17.3): `CUnit::vf107` (`0x6F2A0E30`) calls
    /// `0x6F474C10` on the unit's mover (unit+0x164) with a clear flag; that creates the mover's own `CLrPath` (one
    /// registration) because unit+0xA8 is still empty. CONFIRMED structure and count (Unicorn, `births_emu.py` C6).
    pub(crate) fn pr_unit_created(&mut self, k: ObjKey) {
        if self.pr_objs.unit_path.contains_key(&k) {
            return;
        }
        let h = self.pr.register();
        self.pr_objs.unit_path.insert(k, h);
    }

    /// Sync-registry hook, a mover start (`0x6F474D40` and its siblings `0x6F475030`, `0x6F475230`, `0x6F493620`):
    /// a `CPrClusterGen` is created and attached (one registration), the unit joins it as its only member, and the
    /// flush `0x6F493590` creates one `CPrCluster` plus its `CLrPath` for the member (two registrations) before it
    /// releases the generator. Net: three registrations (Y + 3), the generator's slot is the first free one again.
    /// CONFIRMED counts (Unicorn C1, C2, C5); that a unit's previous cluster pair is released at the next walk is
    /// UNSURE (`pr_walk_release`).
    pub(crate) fn pr_mover_start(&mut self, k: ObjKey) {
        let generator = self.pr.register();
        let cluster = self.pr.register();
        let path = self.pr.register();
        self.pr.unregister(generator);
        if let Some(old) = self.pr_objs.cluster.insert(k, (cluster, path)) {
            self.pr_objs.release.push(old);
        }
    }

    /// Sync-registry hook, a unit stops moving for good (order finished or cleared, removal): its cluster pair is
    /// released by the next walk (`0x6F493410` -> slot 4 `0x6F493130`, which unregisters the path and then the
    /// cluster, CONFIRMED count 2 by Unicorn C7). UNSURE: the trigger (the walk releases a cluster without a live member).
    pub(crate) fn pr_mover_idle(&mut self, k: ObjKey) {
        if let Some(old) = self.pr_objs.cluster.remove(&k) {
            self.pr_objs.release.push(old);
        }
    }

    /// Sync-registry hook, a missile launch: the missile's mover starts like a unit's (three registrations); the pair
    /// is released at the impact. UNSURE: the missile object's own registrations (behaviour object) are not modelled.
    pub(crate) fn pr_missile_launch(&mut self) {
        let generator = self.pr.register();
        let cluster = self.pr.register();
        let path = self.pr.register();
        self.pr.unregister(generator);
        self.pr_objs.missiles.push_back((cluster, path));
    }

    /// Sync-registry hook, a missile impact: the oldest launched missile's pair is released at the next walk.
    pub(crate) fn pr_missile_impact(&mut self) {
        if let Some(old) = self.pr_objs.missiles.pop_front() {
            self.pr_objs.release.push(old);
        }
    }

    /// Sync-registry hook, a unit removal: the mover path goes with the unit; the cluster pair is released by the walk.
    pub(crate) fn pr_unit_removed(&mut self, k: ObjKey) {
        self.pr_mover_idle(k);
        if let Some(h) = self.pr_objs.unit_path.remove(&k) {
            self.pr.unregister(h);
        }
    }

    /// The movement walk (`0x6F483300`) releases the clusters that lost their members: path first, then the cluster.
    pub(crate) fn pr_walk_release(&mut self) {
        for (cluster, path) in std::mem::take(&mut self.pr_objs.release) {
            self.pr.unregister(path);
            self.pr.unregister(cluster);
        }
    }

    /// The world origin of the cell storage: `(min x, min y)` of the playable area, or zero before a host sets one
    /// (the original's `W+0x6C` / `W+0x70`; the value is the playable minimum, see `docs/spec/unit-position.md` 2).
    pub fn world_origin(&self) -> (F32, F32) {
        self.playable()
            .map_or((F32::ZERO, F32::ZERO), |p| (p.min_x, p.min_y))
    }

    /// The position a move to (x, y) stores: the playable clamp when a playable area is set (SetUnitPosition path,
    /// `docs/spec/unit-position.md` 3a), otherwise the point as it is. SetUnitX/SetUnitY do not clamp (CONFIRMED,
    /// `0x6F3C5970` goes straight to the cell setter).
    pub fn clamp_unit_move(&self, x: F32, y: F32) -> (F32, F32) {
        match self.playable() {
            Some(p) => clamp_to_playable(p, x, y),
            None => (x, y),
        }
    }

    /// Stores a world position of unit `k` through the cell setter: the cells are kept and the cached world
    /// coordinates become the normaliser's output for them. Does nothing for a non-unit key.
    pub fn store_unit_position(&mut self, k: ObjKey, x: F32, y: F32) {
        let (ox, oy) = self.world_origin();
        if self.movers.get(&k).is_some_and(|m| m.in_motion()) {
            // A unit in motion: the setter forms `delta = P - F` against the position evaluated at the game time
            // (`F = cell + V * v`, 0x6F4737D0) and the core writer re-bases the cells and the snapshot with it
            // (0x6F4A6FD0); the velocity is unchanged.
            let g = self.layer();
            let mut kin = self.kin_of(k);
            let f = kin.position(&g);
            let p = (world_to_cell(x, ox), world_to_cell(y, oy));
            let delta = (arith::sub(p.0, f.0), arith::sub(p.1, f.1));
            kin.rebase(&g, delta);
            self.commit_kin(k, &kin);
            return;
        }
        let u = self.unit_mut(k);
        let (cx, cy) = (stored_cell(u.cell_x, x, ox), stored_cell(u.cell_y, y, oy));
        u.cell_x = cx;
        u.cell_y = cy;
        u.x = cell_to_world(cx, ox);
        u.y = cell_to_world(cy, oy);
    }

    /// Recomputes every unit's cached world position from its cells (after the origin changes).
    pub fn refresh_unit_positions(&mut self) {
        let (ox, oy) = self.world_origin();
        for i in 0..self.object_count() {
            let k = ObjKey(i as u32);
            if let Object::Unit(u) = self.object_mut(k) {
                u.x = cell_to_world(u.cell_x, ox);
                u.y = cell_to_world(u.cell_y, oy);
            }
        }
    }

    /// The creation placement of unit `k`. UNSURE: the original's route of the placement into the cells is not
    /// found (`docs/spec/unit-position.md` 8.2); only the read-back of `vf107` (CONFIRMED) is reproduced. The placement is stored, then
    /// the init reads the position back through the normaliser and writes it again (CONFIRMED structure of
    /// `0x6F2A0E30`).
    pub fn place_new_unit(&mut self, k: ObjKey, x: F32, y: F32) {
        self.pr_unit_created(k);
        self.store_unit_position(k, x, y);
        let (x, y) = {
            let u = self.unit(k);
            (u.x, u.y)
        };
        self.store_unit_position(k, x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

const EMBEDDED: &[[u32; 8]] = &[
    [0x46d92566, 0xc4facf2f, 0x46f8c03d, 0xc4f3fa7d, 0xc525496b, 0x45c85e44, 0x46d92566, 0xc4f7fa7d],
    [0x454a9c36, 0xc53d94e2, 0x454e3866, 0x458b19fd, 0xc5e104f4, 0xc583d5b0, 0x454a9c36, 0xc53d94e2],
    [0x43cfb825, 0xc4d0d941, 0x44372d0b, 0xc4a6e631, 0xc5fc2417, 0x459b28ad, 0x43cfb825, 0xc4aae631],
    [0xb99821f0, 0xc51bb408, 0x422b0785, 0xc51a5c48, 0xb7d0891a, 0xb914dc3c, 0xb7d0891a, 0xc51c5c48],
    [0xc50412e7, 0x453e1a7e, 0xc4fe2dde, 0x45e71ac2, 0xba4d4952, 0xc6107a22, 0xc50116ef, 0x453e1a7e],
    [0x4504ed50, 0x44e38d9f, 0x4599aaaf, 0x45ffd221, 0x458290dc, 0x459ed4f4, 0x458290dc, 0x459ed4f4],
    [0x455e81af, 0x454baf3b, 0x45d3c3dc, 0x454aea2a, 0xc60c5693, 0xc529ee56, 0x455e81af, 0x454baf3b],
    [0x80000000, 0x3a150116, 0x42881072, 0xc21da35d, 0xb960a12c, 0x449dd4f0, 0x80000000, 0xc28ed1ae],
    [0x80000000, 0x455107ab, 0x4143ef3d, 0x45542702, 0x458ec2a9, 0xc4c0b399, 0xc19e0862, 0x455107ab],
    [0x44e339ae, 0xc5657589, 0x45ea7e1b, 0xc5635c38, 0xc62b3b92, 0x00000000, 0x44e339ae, 0xc5655c38],
    [0xc5586b35, 0x457fef90, 0x43473894, 0x45f0c0da, 0x45c964ae, 0xb9cd9a2a, 0x43273894, 0x457fef90],
    [0xc51be291, 0x00000000, 0xc51c13b6, 0x41bd3b2b, 0x45e72ee7, 0xc628fbdf, 0xc51e13b6, 0x00000000],
    [0xc5000dbd, 0xc6ac4584, 0x450c8dd0, 0xc6ab9013, 0xc5001dbd, 0x45c64acf, 0xc5000dbd, 0xc6abd013],
    [0xc54cfc08, 0x450310e8, 0xc5482b30, 0x4502a6c3, 0xc4cd0373, 0xc5a2db08, 0xc54a2b30, 0x450310e8],
    [0x44f25817, 0x454031e0, 0x4608bce9, 0x4544ed43, 0x00000000, 0x4544fd43, 0x44f25817, 0x4542ed43],
    [0xc31a4928, 0xc558b145, 0xc327d822, 0xc5567649, 0x44efdfe9, 0x46143b78, 0xc347d822, 0xc5587649],
    [0x448387b9, 0xc53d36a6, 0x45f14c00, 0x458bd96b, 0xc62c8884, 0x45ca1ba6, 0x448387b9, 0x458ad96b],
    [0x452add55, 0xc5299249, 0x461a7a94, 0xc526b6f5, 0x4611b3d4, 0x00000000, 0x4611b3d4, 0xc528b6f5],
    [0x4556f03b, 0xc5642d0e, 0x459ce817, 0xc5664c00, 0x4551291a, 0xc4b896c1, 0x4556f03b, 0xc5684c00],
    [0x454e2e9c, 0x3a456bbf, 0x4570517d, 0x45d67453, 0x467679c5, 0x46053467, 0x456e517d, 0x45d57453],
    [0x3a0f644f, 0x00000000, 0x4581766e, 0x42197966, 0x00000000, 0x451513b7, 0x3a0f644f, 0x40cbcb30],
    [0x41aabac9, 0xc579537f, 0xc1277b5e, 0xc5797239, 0xc615b043, 0xc602e735, 0x41aabac9, 0xc579537f],
    [0x3a2f47cf, 0xc42fa893, 0x45b0fe8c, 0xc438f5bc, 0xc589f4f9, 0x4407ce5d, 0x3a2f47cf, 0xc440f5bc],
    [0x45117b21, 0x80000000, 0x45100254, 0x4286d417, 0x00000000, 0x00000000, 0x45117b21, 0x00000000],
    [0x43f0a37f, 0x4520d66b, 0x43e8580a, 0x4521e338, 0xc60dbf82, 0x45fc0d7a, 0x43f0a37f, 0x451fe338],
    [0x454b4147, 0xc62175f8, 0x45c3c94c, 0xc57a298e, 0xc5804a5a, 0x4555ccb6, 0x454b4147, 0xc57c298e],
    [0xc5123446, 0x453a9f24, 0xc50e0a98, 0x45d8cc21, 0x458eabd3, 0xc70f5994, 0xc5100a98, 0x453a9f24],
    [0x446e5cb8, 0xc535fcda, 0x4617078d, 0x44817acd, 0x45b7c60d, 0x45c5277d, 0x45b7c60d, 0x447af59a],
    [0xc4e5a0fb, 0x445e6970, 0x45893c17, 0x458c11b2, 0xc611d753, 0x45d19422, 0xc4e5a0fb, 0x458b11b2],
    [0xc5587393, 0x44c416d8, 0xc485f7d3, 0x44c274f2, 0xc62f6213, 0xc5ca20de, 0xc5587393, 0x44c416d8],
    [0x3a41cd87, 0x4504a5ee, 0x417e8f54, 0x45e001d8, 0xba58b36e, 0x45ed86ea, 0x3a41cd87, 0x45df01d8],
    [0xc566c1d9, 0x442e2c60, 0xc561e3a9, 0x44315461, 0xc719cc42, 0xc506225e, 0xc566c1d9, 0x442e2c60],
    [0xc5147b5b, 0x451663ba, 0x438b614d, 0x460f6b11, 0xb9f9454f, 0xc534b180, 0xb9f9454f, 0x451663ba],
    [0x454fde54, 0xc462d298, 0x454f85cd, 0xc4699318, 0xc627e88a, 0x4401f0ee, 0x454fde54, 0xc4719318],
    [0x43b1db87, 0x44aab924, 0x43a648bb, 0x44afd170, 0x45c380c0, 0xc61de5d6, 0x439648bb, 0x44aab924],
    [0xc2458918, 0xc5520604, 0x4565d668, 0xc4316e6c, 0x456dd7bc, 0x44da423c, 0x4563d668, 0xc4396e6c],
    [0x440c48d1, 0x44fe14e7, 0x458e055e, 0x44fa5410, 0x39c755f5, 0x461abc8b, 0x440c48d1, 0x44f65410],
    [0x424e06d3, 0xc49baefb, 0x42257bb0, 0x45330c9f, 0x45a78f11, 0x4569ab68, 0x4115eec0, 0x45310c9f],
    [0x44dc39f4, 0x45562c2d, 0x44dbfa31, 0x4554b20b, 0x44cc92e7, 0x44bd62ac, 0x44dc39f4, 0x45562c2d],
    [0xc571294e, 0xc4600c26, 0xc56eb461, 0x45d3e35e, 0xc509a27d, 0xc4a2dc1c, 0xc570b461, 0xc4600c26],
    [0x3a810264, 0xc5361852, 0xbfabcc88, 0xc5345bff, 0xbf7fbf7f, 0x461c8f3b, 0x3a810264, 0xc5365bff],
    [0xc3052576, 0x45272de1, 0x44bd51c8, 0x46037d68, 0x455cd7fc, 0x460bc7a8, 0x44b951c8, 0x4602fd68],
    [0x44f43345, 0xc434d51d, 0x453880a4, 0xc42e5099, 0x458eff05, 0xc636d5fa, 0x453680a4, 0xc434d51d],
    [0xc1f9685b, 0xc48e70ca, 0x45973156, 0xc48ef717, 0x00000000, 0x45b1f843, 0x00000000, 0xc492f717],
    [0x00000000, 0x453a5b87, 0x40b64eca, 0x45e9d63e, 0x45aa7f74, 0x45e9de3e, 0xc1d26c4e, 0x45e8d63e],
    [0x406e859a, 0xc4477092, 0x415ed968, 0xc43823de, 0xc5d808fd, 0x45f1a8bf, 0x406e859a, 0xc44023de],
    [0x431cdb85, 0x453993c9, 0x45f4c00e, 0x453a4396, 0xb9a77bca, 0x4617d18c, 0x431cdb85, 0x45384396],
    [0x43280289, 0x4468b007, 0x43399d4e, 0x4594e030, 0x00000000, 0x462c527e, 0x43280289, 0x4593e030],
    [0xc554a23b, 0xc4b18691, 0x458f7e02, 0xc4b1df3b, 0x4513d496, 0xc6370957, 0x4513d496, 0xc4b18691],
    [0x44be25bb, 0x80000000, 0x45c0a0e9, 0xc1007dd5, 0xc5c19fc3, 0xc714bb95, 0x44be25bb, 0x80000000],
    [0xc4857086, 0x4538fc0a, 0xc47ad3b5, 0x453a9e04, 0x4613ff6d, 0x453aae04, 0xc48169da, 0x45389e04],
    [0x3a44c4d8, 0xc4898d46, 0x458e4216, 0xc484d5af, 0x43af341a, 0x45f08495, 0x43af341a, 0xc488d5af],
    [0xc550f843, 0xc4b1e161, 0x459e1157, 0xc4b6b91d, 0xc586d63a, 0x45c770cf, 0xc550f843, 0xc4bab91d],
    [0x451586dd, 0x45361b23, 0x45164a65, 0x4537db6a, 0x00000000, 0x4537eb6a, 0x451586dd, 0x4535db6a],
    [0x00000000, 0x3a585eef, 0x454c935c, 0x459b07bc, 0xc51869a4, 0xc510e3e8, 0x00000000, 0x3a585eef],
    [0xc576e889, 0x00000000, 0xc574525e, 0x45f6d6b4, 0x458452a9, 0xc6275581, 0xc576525e, 0x00000000],
    [0xc5186b72, 0x4482df47, 0xc516b94d, 0x44f061b3, 0x4482b0b1, 0x80000000, 0xc518b94d, 0x4482df47],
    [0xc520d829, 0x46d09ebc, 0xc51f976c, 0x46d0be17, 0x45d98382, 0x00000000, 0xc521976c, 0x46d09ebc],
    [0xc4a382bc, 0x44a325e9, 0xc4a2b122, 0x44a874d9, 0xc6f36061, 0x45cabdfb, 0xc4a382bc, 0x44a474d9],
    [0xc56a3478, 0x00000000, 0x436cf959, 0x45b5ba00, 0x47107ac0, 0x45f84777, 0x434cf959, 0x45b4ba00],
    [0xc477ab9e, 0x44e22660, 0xc472ed24, 0x461e669b, 0xc5617a9d, 0xc5975f9f, 0xc477ab9e, 0x44e22660],
    [0xc5585745, 0xc4e4711b, 0x45819c04, 0xc4e277b4, 0x462061b3, 0xc62372c3, 0x45809c04, 0xc4e4711b],
    [0xc57cd811, 0xb9e225b6, 0xc57b444e, 0x426a56a9, 0xc583c010, 0x462bc69a, 0xc57cd811, 0x41d4ad52],
    [0x4567c4d1, 0x3a71e7f5, 0x456b3914, 0xc1cf34e6, 0x45698df1, 0x46203802, 0x45693914, 0xc2679a73],
];


    fn f(v: f32) -> F32 {
        F32(v.to_bits())
    }

    #[test]
    fn a_zero_origin_round_trip_is_exact_for_normal_values() {
        // With origin 0 the subtraction and the addition are exact, and the 1/32 and 32 scalings are exact.
        for v in [1.0f32, -3.5, 4096.0, 0.25, 1234.5678, -0.001, 1.0e-20] {
            let c = world_to_cell(f(v), F32::ZERO);
            assert_eq!(cell_to_world(c, F32::ZERO), f(v), "value {v}");
        }
    }

    #[test]
    fn small_cells_are_flushed_to_positive_zero() {
        // An exponent below 6 after the subtraction gives +0.0 (the magnitude test of the setter).
        assert_eq!(world_to_cell(f(1.0e-38), F32::ZERO), F32::ZERO);
        assert_eq!(world_to_cell(f(-1.0e-38), F32::ZERO), F32::ZERO);
        assert_eq!(world_to_cell(f(0.0), F32::ZERO), F32::ZERO);
        assert_eq!(world_to_cell(F32(0x8000_0000), F32::ZERO), F32::ZERO);
        // Exponent 6 is the smallest kept value; it is scaled down by 5 to exponent 1.
        assert_eq!(world_to_cell(F32(0x0300_0000), F32::ZERO), F32(0x0080_0000));
        assert_eq!(world_to_cell(F32(0x82FF_FFFF), F32::ZERO), F32::ZERO);
    }

    #[test]
    fn a_far_origin_loses_low_bits_as_the_original_does() {
        // (0.1 - 4096) is rounded by the truncating subtraction, so the read-back is not 0.1 (the ulp of 4096 is
        // 2^-12), while the 1/32 and 32 scalings are exact.
        let origin = f(4096.0);
        let x = f(0.1);
        let back = cell_to_world(world_to_cell(x, origin), origin);
        assert_ne!(back, x);
        assert!((back.0 as i64 - x.0 as i64).abs() > 0);
        let err = (f32::from_bits(back.0) - 0.1).abs();
        assert!(err <= 2.0f32.powi(-12), "error {err}");
    }

    // Unicorn vectors of the real `0x6F469490` (tools/emu/unit_clamp_emu.py, seed 1, every 47th of 3,000 cases):
    // [w6c, w70, w74, w78, x, y, out_x, out_y], as IEEE bit patterns.
    #[test]
    fn playable_clamp_matches_the_original_embedded() {
        for v in EMBEDDED {
            let p = Playable {
                min_x: F32(v[0]),
                min_y: F32(v[1]),
                max_x: F32(v[2]),
                max_y: F32(v[3]),
            };
            let (x, y) = clamp_to_playable(p, F32(v[4]), F32(v[5]));
            assert_eq!((x, y), (F32(v[6]), F32(v[7])), "case {v:08x?}");
        }
    }

    /// Full vector file from the Unicorn harness (set `UNIT_CLAMP_VECTORS` to its path; skipped otherwise).
    #[test]
    fn playable_clamp_matches_the_original_file() {
        let Ok(path) = std::env::var("UNIT_CLAMP_VECTORS") else {
            return;
        };
        let text = std::fs::read_to_string(path).expect("vector file");
        let mut n = 0;
        for line in text.lines() {
            let f: Vec<u32> = line
                .split_whitespace()
                .skip(1)
                .map(|h| u32::from_str_radix(h, 16).expect("hex"))
                .collect();
            let p = Playable {
                min_x: F32(f[0]),
                min_y: F32(f[1]),
                max_x: F32(f[2]),
                max_y: F32(f[3]),
            };
            let (x, y) = clamp_to_playable(p, F32(f[4]), F32(f[5]));
            assert_eq!((x, y), (F32(f[6]), F32(f[7])), "line {n}");
            n += 1;
        }
        assert!(n > 0);
    }

    #[test]
    fn clamp_keeps_inside_points_and_applies_the_margin() {
        let p = Playable {
            min_x: F32::from_bits(0x4400_0000), // 512.0
            min_y: F32::from_bits(0x4400_0000),
            max_x: F32::from_bits(0x4500_0000), // 2048.0
            max_y: F32::from_bits(0x4500_0000),
        };
        let inside = (F32::from_bits(0x4480_0000), F32::from_bits(0x4480_0000)); // 1024.0
        assert_eq!(clamp_to_playable(p, inside.0, inside.1), inside);
        // Above the upper bound: max - 32 = 2016.0 (0x44FC0000).
        let (x, _) = clamp_to_playable(p, F32::from_bits(0x4600_0000), inside.1);
        assert_eq!(x, F32::from_bits(0x44FC_0000));
        // Below the lower bound: min, and the upper bound is not applied.
        let (x, _) = clamp_to_playable(p, F32::from_bits(0x4300_0000), inside.1);
        assert_eq!(x, p.min_x);
    }
    #[test]
    fn registrations_follow_the_movement_object_counts() {
        // docs/spec/lockstep.md section 17 (counts CONFIRMED by tools/emu/births_emu.py): a unit's mover path is one
        // registration, a mover start is three (generator, cluster, path) with the generator released again, and a
        // cluster pair is released path first, then cluster.
        let mut g = Game::new();
        let u = ObjKey(0);
        g.pr_unit_created(u);
        assert_eq!(g.pr.world_words().f50, 1);
        g.pr_unit_created(u); // creating twice is not a second birth
        assert_eq!(g.pr.world_words().f50, 1);
        g.pr_mover_start(u);
        let w = g.pr.world_words();
        assert_eq!((w.f50, w.f1c, w.f40), (4, 4, 1)); // slots: path 0, generator 1 (free), cluster 2, path 3
        g.pr_mover_start(u); // the old pair waits for the walk
        let w = g.pr.world_words();
        assert_eq!((w.f50, w.f1c, w.f40), (7, 6, 1));
        g.pr_walk_release();
        let w = g.pr.world_words();
        // released path (3), then cluster (2): the last freed slot is the free-list head
        assert_eq!((w.f50, w.f40), (7, 2));
        g.pr_unit_removed(u);
        assert_eq!(g.pr.world_words().f50, 7);
    }

    #[test]
    fn missiles_release_oldest_first() {
        let mut g = Game::new();
        g.pr_missile_launch();
        g.pr_missile_launch();
        assert_eq!(g.pr.world_words().f50, 6);
        g.pr_missile_impact();
        g.pr_walk_release();
        assert_eq!(g.pr_objs.missiles.len(), 1);
    }
}
