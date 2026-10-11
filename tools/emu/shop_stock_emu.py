#!/usr/bin/env python3
"""Unicorn check of the dynamic shop stock list (docs/spec/shop-stock.md).

Runs the real routines of the item shop ability on a synthetic ability object:
  add      0x6F036D60 (ECX = ability; stack: type id, current stock, stock max)
  remove   0x6F036F50 (stack: type id)
  slots    0x6F036FB0 (stack: slot count)
The unit shop twin routines (0x6F036890 / 0x6F036A80 / 0x6F036AE0) have the same instruction streams
(checked by a disassembly diff); the add twin is also run.

External calls are replaced by hooks (stand-ins, not original code): the slot registration helpers
(0x6F32DC70, 0x6F32DCB0, 0x6F332A10), the timer cancel 0x6F477D30, the timer start 0x6F477A80, the refresh
0x6F021270, the property setter (vtable slot 0 of the per-slot property object: it copies the real to
+4, an ASSUMPTION) and the type reads (ability vtable +0x330 regen, +0x334 stock max). The soft-float
helpers (i2r, sub) and everything else is the original code.

Output: crates/sim/tests/shop_stock_vectors.txt-style lines on stdout (--emit), or a summary.
Usage: GAME_DLL=path python tools/emu/shop_stock_emu.py [CASES_PER_OP] [SEED] [--emit FILE]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import capstone  # noqa: E402
import softfloat_vectors as sv  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP)

ADD_ITEM, ADD_UNIT = 0x6F036D60, 0x6F036890
REMOVE_ITEM, REMOVE_UNIT = 0x6F036F50, 0x6F036A80
SLOTS_ITEM, SLOTS_UNIT = 0x6F036FB0, 0x6F036AE0
H_REG1, H_REG2, H_REG3 = 0x6F32DC70, 0x6F32DCB0, 0x6F332A10
H_CANCEL, H_START, H_REFRESH = 0x6F477D30, 0x6F477A80, 0x6F021270

HEAP = 0x00400000
A = HEAP                  # the ability object
VT = HEAP + 0x1000        # its vtable (only +0x330 and +0x334 are read)
PVT = HEAP + 0x2000       # vtable of the per-slot property objects
SCR = HEAP + 0x3000       # scratch returned by the registration hook
STUBS = HEAP + 0x10000    # hook entry points
S330, S334, SSET = STUBS, STUBS + 0x10, STUBS + 0x20
TWO_EM7 = 0x3456BF95


def pops_of(fn):
    """Bytes the routine pops from the stack on return (ret imm), from its first ret."""
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    img, base = G.img, G.base
    off = fn - base
    for i in md.disasm(img[off:off + 0x400], fn):
        if i.mnemonic == "ret":
            return int(i.op_str, 0) if i.op_str else 0
    raise RuntimeError(hex(fn))


G = sv.Game(os.environ["GAME_DLL"])
mu = G.mu
mu.mem_map(HEAP, 0x100000)
mu.mem_write(0x6FAAE614, struct.pack("<I", TWO_EM7))
mu.mem_write(0x6FAAE4F4, struct.pack("<I", 0x3F800000))


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def ret_from(uc, value, pops):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = rd(esp)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pops)
    uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
    uc.reg_write(UC_X86_REG_EIP, ret)


class Env:
    regen = 0x3F800000
    stock_max = 0
    cancels = []
    starts = []
    refreshes = 0


E = Env()
POP = {}


def hook(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    if address == S330:           # (out, id) -> out, writes the regen real
        out = rd(esp + 4)
        wr(out, E.regen)
        ret_from(uc, out, 8)
    elif address == S334:         # (id) -> stock max
        ret_from(uc, E.stock_max, 4)
    elif address == SSET:         # (ptr, flag): property value := *ptr (assumption)
        ptr = rd(esp + 4)
        wr(ecx + 4, rd(ptr))
        ret_from(uc, 0, 8)
    elif address == H_REG1:
        ret_from(uc, SCR, 0)
    elif address == H_CANCEL:
        E.cancels.append(ecx)
        ret_from(uc, 0, 0)
    elif address == H_START:
        ptr = rd(esp + 4)
        E.starts.append((rd(ptr), rd(esp + 8)))   # (interval bits, slot)
        ret_from(uc, 0, POP[address])
    elif address == H_REFRESH:
        E.refreshes += 1
        ret_from(uc, 0, 0)
    else:                         # H_REG2, H_REG3
        ret_from(uc, 0, POP[address])


for h in (H_REG1, H_REG2, H_REG3, H_CANCEL, H_START, H_REFRESH):
    POP[h] = pops_of(h)
    mu.hook_add(UC_HOOK_CODE, hook, None, h, h)
for s in (S330, S334, SSET):
    mu.hook_add(UC_HOOK_CODE, hook, None, s, s)
wr(VT + 0x330, S330)
wr(VT + 0x334, S334)
wr(PVT, SSET)


def build(st):
    mu.mem_write(HEAP + 0x100, b"\0" * 0x400)
    wr(A, VT)
    wr(A + 0x34, 0x77)
    wr(A + 0x47C, st["cap"])
    for i in range(12):
        wr(A + 0xCC + 4 * i, st["ids"][i])
        wr(A + 0x98 + 4 * i, st["flags"][i])
        wr(A + 0x480 + 4 * i, st["ages"][i])
        o = A + 0x100 + 16 * i
        wr(o, PVT)
        wr(o + 4, st["val"][i])
        wr(o + 0xC, st["max"][i])


def dump():
    return {
        "cap": rd(A + 0x47C),
        "ids": [rd(A + 0xCC + 4 * i) for i in range(12)],
        "flags": [rd(A + 0x98 + 4 * i) for i in range(12)],
        "ages": [rd(A + 0x480 + 4 * i) for i in range(12)],
        "val": [rd(A + 0x104 + 16 * i) for i in range(12)],
        "max": [rd(A + 0x10C + 16 * i) for i in range(12)],
    }


def reset_env(regen=0x3F800000, stock_max=0):
    E.regen, E.stock_max = regen, stock_max
    E.cancels, E.starts, E.refreshes = [], [], 0


def f32(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def rand_state(rng):
    cap = rng.choice([0, 1, 2, 3, 5, 8, 11, 11, 11])
    ids = [rng.choice([0, 0, 1, 2, 3, 4, 5, 6]) + 0x30303030 if True else 0 for _ in range(12)]
    ids = [0 if rng.random() < 0.3 else rng.randrange(1, 8) for _ in range(12)]
    ages = [rng.choice([0, 1, 2, 3, 4, 5, 9]) for _ in range(12)]
    flags = [rng.choice([0, 1]) for _ in range(12)]
    val = [f32(float(rng.randrange(0, 6))) for _ in range(12)]
    mx = [f32(float(rng.randrange(0, 6))) for _ in range(12)]
    return {"cap": cap, "ids": ids, "flags": flags, "ages": ages, "val": val, "max": mx}


def fmt(st):
    return " ".join(
        [str(st["cap"])]
        + [str(x) for x in st["ids"]] + [str(x) for x in st["flags"]] + [str(x) for x in st["ages"]]
        + [str(x) for x in st["val"]] + [str(x) for x in st["max"]])


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    n = int(args[0]) if args else 1000
    seed = int(args[1]) if len(args) > 1 else 1
    emit = None
    if "--emit" in sys.argv:
        emit = sys.argv[sys.argv.index("--emit") + 1]
        args = [a for a in args if a != emit]
    rng = random.Random(seed)
    lines = []
    mism = 0
    for kind, fn_add, fn_rm, fn_sl in (("item", ADD_ITEM, REMOVE_ITEM, SLOTS_ITEM),
                                       ("unit", ADD_UNIT, REMOVE_UNIT, SLOTS_UNIT)):
        for _ in range(n):
            st = rand_state(rng)
            build(st)
            tid = rng.randrange(1, 9)
            cur = rng.choice([0, 1, 2, 3, 7, -1])
            mxa = rng.choice([0, -1, 1, 2, 5, 9])
            dmax = rng.choice([0, 1, 4, 8])
            regen = rng.choice([f32(1.0), f32(0.5), f32(30.0), 0])
            reset_env(regen, dmax)
            G.call(fn_add, ecx=A, stack=(tid, cur & 0xFFFFFFFF, mxa & 0xFFFFFFFF), count=2_000_000)
            after = dump()
            lines.append(f"add {kind} {tid} {cur} {mxa} {dmax} {regen} | {fmt(st)} | {fmt(after)} | "
                         f"{len(E.cancels)} {len(E.starts)} " + " ".join(f"{b} {s}" for b, s in E.starts)
                         + f" | {E.refreshes}")
        for _ in range(n):
            st = rand_state(rng)
            build(st)
            tid = rng.randrange(0, 9)
            reset_env()
            G.call(fn_rm, ecx=A, stack=(tid,), count=200_000)
            after = dump()
            lines.append(f"remove {kind} {tid} | {fmt(st)} | {fmt(after)} | {len(E.cancels)} 0 | {E.refreshes}")
        for _ in range(n):
            st = rand_state(rng)
            build(st)
            cap = rng.choice([0, 1, 4, 10, 11, 12, 20, -1, 0x7FFFFFFF])
            reset_env()
            G.call(fn_sl, ecx=A, stack=(cap & 0xFFFFFFFF,), count=200_000)
            after = dump()
            lines.append(f"slots {kind} {cap} | {fmt(st)} | {fmt(after)} | {len(E.cancels)} 0 | {E.refreshes}")
    print(f"{len(lines)} cases run")
    if emit:
        with open(emit, "w") as f:
            f.write("\n".join(lines) + "\n")
    return mism


if __name__ == "__main__":
    main()
