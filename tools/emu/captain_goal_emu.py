"""Unicorn check of the captain position test of Game.dll 1.26a (CaptainIsHome / CaptainAtGoal, ai-natives.md section 10).

Runs the original `0x6F2E76E0` (thiscall, ECX = captain; stack: &x, &y, &radius; retn 0xC): true iff V >= D where
D = dist2(captain position, (x, y)) (soft-float fused routine 0x6F6EF3B0) and V is a squared radius derived from the
radius by soft-float arithmetic (radius < 100.0 -> the constant at 0x6FAB4F50). The captain's position getter
(vtable slot 0xB8) is replaced by a hook that writes the case's position; the normaliser 0x6F4743A0 is a no-op
(UNSURE: its real effect is the cell/world conversion).

Usage: GAME_DLL=/path/Game.dll py -I tools/emu/captain_goal_emu.py [--vectors OUT.tsv] [--cases N] [--seed S]
Vector line: radius, cx, cy, x, y (f32 bits, hex) and the result 0/1.
"""
import math
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
u32 = emu.u32
F_TEST = 0x6F2E76E0
H_NORM = 0x6F4743A0
REGION = 0x00500000
mu.mem_map(REGION, 0x4000)
CAPT = REGION
VT = REGION + 0x100
GETPOS = REGION + 0x200   # fake slot function address (hooked)
ARGS = REGION + 0x300
POS = {}


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", u32(v)))


mu.mem_write(GETPOS, b"\x90" * 16)
wr(CAPT, VT)
wr(VT + 0xB8, GETPOS)


def hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    r = rd(esp)
    if address == GETPOS:
        out = rd(esp + 4)
        wr(out, POS["x"])
        wr(out + 4, POS["y"])
        wr(out + 8, 0)
        uc.reg_write(UC_X86_REG_EAX, out)
        uc.reg_write(UC_X86_REG_ESP, esp + 8)
        uc.reg_write(UC_X86_REG_EIP, r)
    elif address == H_NORM:
        uc.reg_write(UC_X86_REG_ESP, esp + 4)
        uc.reg_write(UC_X86_REG_EIP, r)


mu.hook_add(UC_HOOK_CODE, hook, begin=GETPOS, end=GETPOS)
mu.hook_add(UC_HOOK_CODE, hook, begin=H_NORM, end=H_NORM)


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def bf(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def run(radius, cx, cy, x, y):
    POS["x"], POS["y"] = cx, cy
    wr(ARGS, x)
    wr(ARGS + 4, y)
    wr(ARGS + 8, radius)
    return emu.call(F_TEST, ecx=CAPT, stack_args=(ARGS, ARGS + 4, ARGS + 8)) & 0xFF


def main():
    args = sys.argv[1:]
    out = args[args.index("--vectors") + 1] if "--vectors" in args else None
    n = int(args[args.index("--cases") + 1]) if "--cases" in args else 600
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    print("consts E504=%08x 4F50=%08x 4F54=%08x E574=%08x" % (rd(0x6FAAE504), rd(0x6FAB4F50), rd(0x6FAB4F54), rd(0x6FAAE574)))
    rng = random.Random(seed)
    lines = []
    for _ in range(n):
        radius = fb(rng.choice([0.0, 50.0, 99.0, 99.999, 100.0, 100.001, 150.0, 200.0, 500.0, 900.0, 1500.0, rng.uniform(0, 3000)]))
        cx, cy = fb(rng.uniform(-9000, 9000)), fb(rng.uniform(-9000, 9000))
        if rng.random() < 0.6:
            rr = bf(radius)
            # a distance near the boundary of the squared radius (20000 below 100.0, about 2 r^2 above)
            d = math.sqrt(20000.0 if rr < 100.0 else 2.0 * rr * rr) * rng.uniform(0.9, 1.25)
            a = rng.uniform(0, 6.283)
            x = fb(bf(cx) + d * math.cos(a))
            y = fb(bf(cy) + d * math.sin(a))
        else:
            x, y = fb(rng.uniform(-9000, 9000)), fb(rng.uniform(-9000, 9000))
        got = run(radius, cx, cy, x, y)
        lines.append("\t".join("%08x" % v for v in (radius, cx, cy, x, y)) + "\t%d" % got)
    print("cases", n, "true", sum(1 for l in lines if l.endswith("1")))
    if out:
        open(out, "w").write("\n".join(lines) + "\n")
        print("wrote", out)


if __name__ == "__main__":
    main()
