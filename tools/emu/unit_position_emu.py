"""Unicorn vectors for the unit position storage (docs/spec/unit-position.md).

Runs the real cell setter `0x6F474250` (used by SetUnitX/SetUnitY and by the creation init `0x6F2A0E30`) and the real
position normaliser `0x6F4743A0` (used by GetUnitX/GetUnitY) on the synthetic unit object of hero_normaliser_emu.

Realistic state (what the original leaves after any position write):
  - the setter refreshes the cell term snapshot (`0x6F497440` writes unit+0x70/+0x74 from the grid it uses),
    so the cell term `v` of the normaliser is +0 at read time. The harness forces this by writing the snapshot from
    grid A, and clears unit+0x14 bit 31 (grid A). The setter itself re-does it, so the forcing is a check, not a cause.
  - the grid values, the origin (world+0x6C/+0x70) and the step vector are random; the output must not depend on the
    grid values, which the model checks.

Each case is written as one line of hex bit patterns (cells are the stored unit+0x78/+0x7C values):

  P mode w6c w70 cx0 cy0 xin yin nx ny cx1 cy1

  mode 0 = SetUnitX(xin): the setter gets {xin, normalised y}; mode 1 = SetUnitY(yin): {normalised x, yin};
  mode 2 = raw pair {xin, yin} (the same setter; SetUnitPosition's arithmetic is LIKELY the same, not run here).
  nx, ny = GetUnitX/GetUnitY after the set (the real normaliser); cx1, cy1 = unit+0x78/+0x7C after the set.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/unit_position_emu.py [N_CASES] [SEED] [OUT.txt]
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import hero_candidate_emu as HC  # noqa: E402
import hero_normaliser_emu as HN  # noqa: E402
import hero_roundtrip_emu as RT  # noqa: E402

SETTER = RT.SETTER
VEC = RT.VEC


def pick_world(rng):
    r = rng.random()
    if r < 0.03:
        return 0.0
    if r < 0.05:
        return -0.0
    if r < 0.07:
        return rng.uniform(-1e-3, 1e-3)
    if r < 0.09:
        return rng.uniform(-30000.0, 30000.0)
    return rng.uniform(-4096.0, 4096.0)


def probe(rng, out, counts):
    HN.set_case(rng)
    # grid A: the snapshot of the cell term equals the grid, and unit+0x14 bit 31 is clear (grid A)
    HN.wr32(HC.OBJ + 0x14, HN.rd32(HC.OBJ + 0x14) & 0x7FFFFFFF)
    HN.wr32(HC.OBJ + 0x70, HN.rd32(HN.MGR + 0x14 + 0x40))
    HN.wr32(HC.OBJ + 0x74, HN.rd32(HN.MGR + 0x14 + 0x44))
    HN.wr32(HN.MGR_PTR, HN.MGR)
    w6c = HN.rd32(HC.WORLD + 0x6C)
    w70 = HN.rd32(HC.WORLD + 0x70)
    # an earlier write, so that the stored cells come from the setter
    HN.wr32(VEC, HN.f2b(pick_world(rng)))
    HN.wr32(VEC + 4, HN.f2b(pick_world(rng)))
    emu.call(SETTER, ecx=HC.POSREF, stack_args=(VEC, 1))
    cx0 = HN.rd32(HC.OBJ + 0x78)
    cy0 = HN.rd32(HC.OBJ + 0x7C)
    ox, oy = HN.normalise()
    xin = pick_world(rng)
    yin = pick_world(rng)
    mode = rng.randrange(3)
    if mode == 0:
        HN.wr32(VEC, HN.f2b(xin))
        HN.wr32(VEC + 4, oy)
    elif mode == 1:
        HN.wr32(VEC, ox)
        HN.wr32(VEC + 4, HN.f2b(yin))
    else:
        HN.wr32(VEC, HN.f2b(xin))
        HN.wr32(VEC + 4, HN.f2b(yin))
    emu.call(SETTER, ecx=HC.POSREF, stack_args=(VEC, 1))
    nx, ny = HN.normalise()
    cx1 = HN.rd32(HC.OBJ + 0x78)
    cy1 = HN.rd32(HC.OBJ + 0x7C)
    counts["mode%d" % mode] = counts.get("mode%d" % mode, 0) + 1
    if mode == 0 and ny != oy:
        counts["setx_other_moved"] = counts.get("setx_other_moved", 0) + 1
    if mode == 1 and nx != ox:
        counts["sety_other_moved"] = counts.get("sety_other_moved", 0) + 1
    out.write("P %d %08x %08x %08x %08x %08x %08x %08x %08x %08x %08x\n" % (
        mode, w6c, w70, cx0, cy0, HN.f2b(xin), HN.f2b(yin), nx, ny, cx1, cy1))


def main():
    cases = int(sys.argv[1]) if len(sys.argv) > 1 else 500
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
    path = sys.argv[3] if len(sys.argv) > 3 else os.path.join(HERE, "unit_position_vectors.txt")
    rng = random.Random(seed)
    RT.setup_harness()
    HN.wr32(0x6FAAE478, 0x38D1B717)
    counts = {}
    with open(path, "w") as out:
        for _ in range(cases):
            probe(rng, out, counts)
    print("cases=%d -> %s " % (cases, path) + " ".join("%s=%d" % kv for kv in sorted(counts.items())))


if __name__ == "__main__":
    main()
