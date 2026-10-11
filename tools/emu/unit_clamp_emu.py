"""Unicorn vectors for the playable clamp used by the unit move path (docs/spec/unit-position.md, section 3a).

Runs the real code of `0x6F469490` (__fastcall, ECX = pointer to an (x, y) float pair). It clamps the pair into the
playable area of the world object (`[0x6FAB7368]`): x into [W+0x6C, W+0x74 - 32], y into [W+0x70, W+0x78 - 32], the
lower bound first, the upper bound only when the value was not raised. The constant 32.0 comes from the start-up
initialiser (`[0x6FAAE564]`, applied by `emu.py`). Callers: the core move `0x6F2AC220` (SetUnitPosition path, via
the move slot `0x6F2A5D50`) and `0x6F3E0B50`; the pair is passed in place.

Each output line (hex bit patterns, one case per line):
  C w6c w70 w74 w78 x y out_x out_y

Run:  GAME_DLL=/path/to/Game.dll python3 tools/emu/unit_clamp_emu.py OUT.txt [N_CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
import hero_candidate_emu as HC  # noqa: E402

CLAMP = 0x6F469490
PAIR = HC.DATA + 0xD000


def f2b(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def pick(rng, span):
    r = rng.random()
    if r < 0.04:
        return 0.0
    if r < 0.06:
        return -0.0
    if r < 0.12:
        return rng.uniform(-1e-3, 1e-3)
    if r < 0.18:
        return rng.uniform(-40000.0, 40000.0)
    return rng.uniform(-span, span)


def main():
    out_path = sys.argv[1]
    cases = int(sys.argv[2]) if len(sys.argv) > 2 else 500
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    HC.install()
    n_keep = 0
    with open(out_path, "w") as out:
        for _ in range(cases):
            w6c = pick(rng, 4096.0)
            w70 = pick(rng, 4096.0)
            # the upper edges are sometimes narrower than the 32-unit margin (the upper bound then lies below the
            # lower one, which the code also has to handle)
            w74 = w6c + rng.choice([rng.uniform(-40.0, 80.0), rng.uniform(0.0, 9000.0)])
            w78 = w70 + rng.choice([rng.uniform(-40.0, 80.0), rng.uniform(0.0, 9000.0)])
            x = pick(rng, 12000.0)
            y = pick(rng, 12000.0)
            if rng.random() < 0.05:
                x = w6c - 1.0
            if rng.random() < 0.05:
                y = w78 + 1.0
            for off, v in ((0x6C, w6c), (0x70, w70), (0x74, w74), (0x78, w78)):
                wr32(HC.WORLD + off, f2b(v))
            wr32(HC.WORLD_PTR, HC.WORLD)
            wr32(PAIR, f2b(x))
            wr32(PAIR + 4, f2b(y))
            emu.call(CLAMP, ecx=PAIR)
            ox, oy = rd32(PAIR), rd32(PAIR + 4)
            if ox == f2b(x):
                n_keep += 1
            out.write("C %08x %08x %08x %08x %08x %08x %08x %08x\n" % (
                f2b(w6c), f2b(w70), f2b(w74), f2b(w78), f2b(x), f2b(y), ox, oy))
    print("cases=%d x_unchanged=%d -> %s" % (cases, n_keep, out_path))


if __name__ == "__main__":
    main()
