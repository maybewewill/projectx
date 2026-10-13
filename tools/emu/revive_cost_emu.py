"""Unicorn run of the hero revive cost and time formulas (Game.dll 1.26a, docs/spec/revive.md).

* `0x6F0340A0` (stdcall, args hero unit, &gold_out, &lumber_out): the revive cost. The hero level comes from
  `0x6F2A0210` (hooked: returns the level), the type's gold / lumber cost from `0x6F29B800` / `0x6F29B830`
  (hooked: ECX = type id, returns the values we choose). Misc reals through `0x6F009EE0`, Misc ints through
  `0x6F009F90` (both hooked). The soft-float library runs for real.
* `0x6F030CF0` (fastcall ECX = result pointer, EDX = hero unit): the revive time in seconds (a real);
  build time from `0x6F29B7A0` (hooked).

Writes Rust constants to OUT (default crates/sim/tests/revive_vectors/data.rs).

Run:  GAME_DLL="C:/Program Files (x86)/Warcraft III/Game.dll" py tools/emu/revive_cost_emu.py OUT [CASES] [SEED]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP, UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX  # noqa: E402

COST = 0x6F0340A0
TIME = 0x6F030CF0
LEVEL = 0x6F2A0210
GOLD = 0x6F29B800
LUMBER = 0x6F29B830
BLDTM = 0x6F29B7A0
MISC_REAL = 0x6F009EE0
MISC_INT = 0x6F009F90

UNIT = emu.DATA + 0x1000
OUT_G = emu.DATA + 0x2000
OUT_L = emu.DATA + 0x2010
OUT_T = emu.DATA + 0x2020
state = {}


def rd32(a):
    return struct.unpack("<I", bytes(emu.mu.mem_read(a, 4)))[0]


def wr32(a, v):
    emu.mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def cstr(a):
    out = b""
    while True:
        c = bytes(emu.mu.mem_read(a, 1))
        if c == b"\0":
            return out.decode()
        out += c
        a += 1


def ret_to(uc, esp, n_args_bytes, eax):
    ret = rd32(esp)
    uc.reg_write(UC_X86_REG_EAX, eax & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + n_args_bytes)
    uc.reg_write(UC_X86_REG_EIP, ret)


def hook(uc, addr, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    if addr == LEVEL:
        ret_to(uc, esp, 0, state["level"])
    elif addr == GOLD:
        ret_to(uc, esp, 0, state["gold"])
    elif addr == LUMBER:
        ret_to(uc, esp, 0, state["lumber"])
    elif addr == BLDTM:
        ret_to(uc, esp, 0, state["bldtm"])
    elif addr == MISC_REAL:
        key = cstr(rd32(esp + 4))
        wr32(ecx, state["real"][key])
        ret_to(uc, esp, 8, ecx)
    elif addr == MISC_INT:
        key = cstr(rd32(esp + 4 - 4 + 4)) if False else None
        # __thiscall-like: ECX = Misc object, EDX = key, [esp+4] = index; ret 4
        key = cstr(uc.reg_read(UC_X86_REG_EDX))
        ret_to(uc, esp, 4, state["int"][key])


for a in (LEVEL, GOLD, LUMBER, BLDTM, MISC_REAL, MISC_INT):
    emu.mu.hook_add(UC_HOOK_CODE, hook, begin=a, end=a)

REAL_KEYS = ["ReviveBaseFactor", "ReviveLevelFactor", "ReviveMaxFactor", "ReviveBaseLumberFactor",
             "ReviveLumberLevelFactor", "ReviveTimeFactor", "ReviveMaxTimeFactor", "HeroMaxReviveTime"]
INT_KEYS = ["HeroMaxReviveCostGold", "HeroMaxReviveCostLumber"]


def f2b(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def rreal(rng, lo, hi):
    return f2b(rng.uniform(lo, hi)) if rng.random() < 0.9 else rng.choice(
        [0, 0x80000000, 0x3F800000, 0x3F000000, 0x40000000, f2b(0.1), f2b(0.5), f2b(1.5), f2b(100.0)])


def run_cost(c):
    state.update(level=c["level"], gold=c["cg"], lumber=c["cl"], real=c["real"], int=c["int"])
    wr32(OUT_G, 0xDEAD)
    wr32(OUT_L, 0xBEEF)
    emu.call(COST, stack_args=(UNIT, OUT_G, OUT_L))
    return rd32(OUT_G), rd32(OUT_L)


def run_time(c):
    state.update(level=c["level"], bldtm=c["bt"], real=c["real"], int=c["int"])
    wr32(OUT_T, 0xDEAD)
    wr32(UNIT + 0x30, 1)
    emu.call(TIME, ecx=OUT_T, edx=UNIT)
    return rd32(OUT_T)


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "crates/sim/tests/revive_vectors/data.rs"
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 400
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    rng = random.Random(seed)
    cost_rows, time_rows = [], []
    for i in range(n):
        real = {
            "ReviveBaseFactor": rreal(rng, 0.0, 1.5),
            "ReviveLevelFactor": rreal(rng, 0.0, 0.5),
            "ReviveMaxFactor": rreal(rng, 0.5, 6.0),
            "ReviveBaseLumberFactor": rreal(rng, 0.0, 1.5),
            "ReviveLumberLevelFactor": rreal(rng, 0.0, 0.5),
            "ReviveTimeFactor": rreal(rng, 0.0, 0.5),
            "ReviveMaxTimeFactor": rreal(rng, 0.5, 8.0),
            "HeroMaxReviveTime": rreal(rng, 0.0, 400.0),
        }
        ints = {"HeroMaxReviveCostGold": rng.choice([0, 100, 700, 1000, 99999, 0x7FFFFFFF]) if rng.random() < 0.5 else rng.randrange(0, 5000),
                "HeroMaxReviveCostLumber": rng.choice([0, 50, 400, 5000]) if rng.random() < 0.5 else rng.randrange(0, 3000)}
        c = {"level": rng.choice([1, 1, 2, 3, 5, 6, 8, 10, rng.randrange(1, 11), rng.randrange(0, 60)]),
             "cg": rng.randrange(0, 3000) * (1 if rng.random() < 0.8 else 10), "cl": rng.choice([0, 0, 0, rng.randrange(0, 1500)]),
             "bt": rng.randrange(0, 200), "real": real, "int": ints}
        g, l = run_cost(c)
        t = run_time(c)
        cost_rows.append((c, g, l))
        time_rows.append((c, t))
    with open(out, "w") as f:
        f.write("// Generated by tools/emu/revive_cost_emu.py (seed %d). Real Game.dll outputs; do not edit.\n" % seed)
        f.write("// Fields: level, type gold, type lumber, type build time, [base, lvl, max, lbase, llvl, tfac, tmax, hmaxtime] reals,\n")
        f.write("// [max gold, max lumber], expected gold, lumber, time bits.\n")
        f.write("pub const REVIVE: &[(i32, i32, i32, i32, [u32; 8], [u32; 2], u32, u32, u32)] = &[\n")
        for (c, g, l), (_, t) in zip(cost_rows, time_rows):
            r = c["real"]
            f.write("    (%d, %d, %d, %d, [%s], [%d, %d], %d, %d, 0x%08X),\n" % (
                c["level"], c["cg"], c["cl"], c["bt"], ", ".join("0x%08X" % r[k] for k in REAL_KEYS),
                c["int"][INT_KEYS[0]], c["int"][INT_KEYS[1]], g, l, t))
        f.write("];\n")
    print("wrote", out, n, "cases; K=0x%08X" % rd32(0x6FAAE514), "half=0x%08X" % rd32(0x6FAAE4D0), "one=0x%08X" % rd32(0x6FAAE4F4))


main()
