"""Unicorn probe: does a unit's world position survive SetUnitX/SetUnitY + GetUnitX/GetUnitY? (heroes.md 8.8)

The JASS getters are the real normaliser `0x6F4743A0` applied to the unit's position reference (`vf46` = unit+0x164,
handle-resolved to the unit object O). The JASS setters SetUnitX/SetUnitY are the real code of `0x6F474250`
(world -> cell: exponent -5 after a softfloat subtraction of the origin, written through `0x6F4737D0` /
`0x6F4A7380` into O). This probe runs, on the synthetic unit O of hero_normaliser_emu:

  1. read the position (normaliser),
  2. call the real setter with the new coordinate and the other one read back (as SetUnitX does),
  3. read the position again (normaliser).

and counts whether the read-back equals the value that was set (bit for bit), for x and for y.

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/hero_roundtrip_emu.py [CASES] [SEED]
"""
import random
import sys
import os

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import hero_candidate_emu as HC  # noqa: E402
import hero_normaliser_emu as HN  # noqa: E402

SETTER = 0x6F474250          # cell setter used by SetUnitX/SetUnitY (via 0x6F4743C0)
VEC = HC.DATA + 0xE800       # {x, y} float pair passed by pointer, as SetUnitX builds it


REST = os.environ.get("ROUNDTRIP_REST") == "1"


def probe_once(rng, stats):
    HN.set_case(rng)
    if REST:
        # a unit with no step vector: O+0x80 = O+0x84 = 0, so the cell term v multiplies by zero
        HN.wrf(HC.OBJ + 0x80, 0.0)
        HN.wrf(HC.OBJ + 0x84, 0.0)
    HN.wr32(HN.MGR_PTR, HN.MGR)
    ox, oy = HN.normalise()
    x_new = rng.uniform(-4096.0, 4096.0)
    y_new = rng.uniform(-4096.0, 4096.0)
    if rng.random() < 0.05:
        x_new = 0.0
    axis = rng.randrange(2)
    # SetUnitX(x_new): vec = {x_new, normalised y}; SetUnitY(y_new): vec = {normalised x, y_new}
    if axis == 0:
        HN.wr32(VEC, HN.f2b(x_new))
        HN.wr32(VEC + 4, oy)
        target = HN.f2b(x_new)
        stats["setx"] = stats.get("setx", 0) + 1
    else:
        HN.wr32(VEC, ox)
        HN.wr32(VEC + 4, HN.f2b(y_new))
        target = HN.f2b(y_new)
        stats["sety"] = stats.get("sety", 0) + 1
    emu.call(SETTER, ecx=HC.POSREF, stack_args=(VEC, 1))
    nx, ny = HN.normalise()
    got = nx if axis == 0 else ny
    key = "x" if axis == 0 else "y"
    stats[key + "_total"] = stats.get(key + "_total", 0) + 1
    if got == target:
        stats[key + "_exact"] = stats.get(key + "_exact", 0) + 1
    else:
        stats[key + "_diff"] = stats.get(key + "_diff", 0) + 1
        if stats[key + "_diff"] <= 5:
            print("diff %s set=%08x got=%08x (orig=%08x)" % (key, target, got,
                  HN.f2b(x_new) if axis == 0 else HN.f2b(y_new)))
    # the other coordinate must be unchanged by a bit-exact round trip of its own value
    other_ok = (ny == oy) if axis == 0 else (nx == ox)
    stats["other_" + ("unchanged" if other_ok else "moved")] = stats.get("other_" + ("unchanged" if other_ok else "moved"), 0) + 1


NOTIFY = HC.STUB + 0x200     # stub for the position-changed notification (vtable slot 0x54 of the unit)
S_PATH = HC.DATA + 0xF000    # O+0x94: pathing object; +0x2C -> object whose +0x68 is a float (0.0 here)
S_STAT = HC.DATA + 0xF200    # O+0x98: byte +0x37 bit 0 set = pathing update suppressed (6F4A6E40)


def setup_harness():
    """Assumptions of this probe (each one is a side effect outside the position fields):
    - 0x6F4A6D70 (pathing projection, O+0x94) and 0x6F4A6E40 (O+0x98 flag gate) are stubbed as no-ops. The first
      one reads only the synthetic pathing object and writes no unit field (checked by reading its body).
    - the notification (vtable +0x54, called with &O+0x78 when the setter's flag is 1) is recorded, not run.
    - O+0x14 bit 31 and the grid values are the random ones of hero_normaliser_emu.set_case.
    """
    HC.install()
    HC.hook("p6d70", 0x6F4A6D70, 8, lambda uc, esp: None)
    HC.hook("p6e40", 0x6F4A6E40, 8, lambda uc, esp: None)
    HC.hook("notify", NOTIFY, 4, lambda uc, esp: NOTES.append(HN.rd32(esp + 4)))
    HN.wr32(HC.VT + 0x54, NOTIFY)
    HN.wr32(HC.OBJ + 0, HC.VT)
    HN.wr32(HC.OBJ + 0x94, S_PATH)
    HN.wr32(S_PATH + 0x2C, S_PATH + 0x100)
    HN.wr32(S_PATH + 0x100 + 0x68, 0)
    HN.wr32(HC.OBJ + 0x98, S_STAT)
    emu.mu.mem_write(S_STAT + 0x37, bytes([1]))


NOTES = []


def main():
    cases = int(sys.argv[1]) if len(sys.argv) > 1 else 500
    seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
    rng = random.Random(seed)
    setup_harness()
    HN.wr32(0x6FAAE478, 0x38D1B717)
    stats = {}
    for _ in range(cases):
        probe_once(rng, stats)
    print("cases=%d notifications=%d " % (cases, len(NOTES)) + " ".join("%s=%d" % kv for kv in sorted(stats.items())))


if __name__ == "__main__":
    main()
