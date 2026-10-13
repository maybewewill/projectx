"""Unicorn probe of the weapon target acceptance (Game.dll 1.26a): the target predicate and the weapon choosers.

Real code that runs:
  0x6F2552F0  expands a weapon targs word (every unnamed group means "any")
  0x6F25D560  predicate(target flags in ECX, weapon targs in EDX; stack -1,0,0,-1 as the choosers push), 0 = accepted
  0x6F25D280  the target flag word of a unit
  0x6F0C95D0  weapon chooser for a target-info object (flag word at +4)
  0x6F0C93E0  weapon chooser for a target unit (adds the magic-immune and ethereal checks)
  0x6F0C6F90  weapon enabled test
Stubbed: Misc flag reader 0x6F009F90 (MagicImmunesResistDamage), the list walker is never reached (list words are -1).

Usage: GAME_DLL=... py tools/emu/target_accept_emu.py [cases] [--rs out_dir]
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
EXTRA = 0x00400000
mu.mem_map(EXTRA, 0x10000)
A = EXTRA + 0x1000          # the attack object
U = EXTRA + 0x2000          # the target unit
TI = EXTRA + 0x3000         # a target-info object
VT, CODE, OUT = EXTRA + 0x100, EXTRA + 0x300, EXTRA + 0x800
MISC_READ, GLOBAL_FLAG = 0x6F009F90, 0x6FA4F6E8
F_EXPAND, F_PRED, F_UFLAGS, F_CH_INFO, F_CH_UNIT, F_ENABLED = 0x6F2552F0, 0x6F25D560, 0x6F25D280, 0x6F0C95D0, 0x6F0C93E0, 0x6F0C6F90
MISC = {"flag": 0}


def w32(a, v):
    mu.mem_write(a, struct.pack("<I", v & 0xFFFFFFFF))


def r32(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def hook(uc, eip, size, user):
    if eip == MISC_READ:
        esp = mu.reg_read(UC_X86_REG_ESP)
        mu.reg_write(UC_X86_REG_EAX, MISC["flag"])
        mu.reg_write(UC_X86_REG_EIP, r32(esp))
        mu.reg_write(UC_X86_REG_ESP, esp + 8)


mu.hook_add(UC_HOOK_CODE, hook)
mu.mem_write(CODE, b"\xB8\x75\x33\x77\x2B\xC3")  # mov eax,0x2b773375; ret  (unit class tag)
w32(VT + 0x1C, CODE)


def setup_unit(c):
    mu.mem_write(U, b"\0" * 0x400)
    w32(U, VT)
    w32(U + 0x5C, c["u5c"])
    w32(U + 0x20, c["u20"])
    w32(U + 0x24C, c["u24c"])
    mu.mem_write(U + 0x33, bytes([c["u33"]]))
    w32(U + 0x248, c["u248"])
    w32(U + 0x158, c["eth"])
    w32(U + 0x15C, c["mim"])


def setup_attack(c):
    mu.mem_write(A, b"\0" * 0x400)
    w32(A + 0x20, c["a20"])
    w32(A + 0x2BC, 0xFFFFFFFF)
    w32(A + 0x2C0, 0xFFFFFFFF)
    for w in (0, 1):
        w32(A + 0xDC + 4 * w, c["kind"][w])
        w32(A + 0x218 + 4 * w, c["wt"][w])
        w32(A + 0xF4 + 4 * w, c["atk"][w])
    w32(GLOBAL_FLAG, c["gl"])
    MISC["flag"] = c["misc"]


def run(kind, c):
    if kind == "expand":
        return [emu.call(F_EXPAND, ecx=c["w"])]
    if kind == "pred":
        return [emu.call(F_PRED, ecx=c["t"], edx=c["w"], stack_args=(0xFFFFFFFF, 0, 0, 0xFFFFFFFF))]
    if kind == "uflags":
        setup_unit(c)
        return [emu.call(F_UFLAGS, ecx=U)]
    if kind == "info":
        setup_attack(c)
        mu.mem_write(TI, b"\0" * 0x40)
        w32(TI + 4, c["t"])
        w32(OUT, 0x12345678)
        r = emu.call(F_CH_INFO, ecx=A, stack_args=(TI, OUT))
        return [r, r32(OUT)]
    if kind == "unit":
        setup_attack(c)
        setup_unit(c)
        w32(OUT, 0x12345678)
        r = emu.call(F_CH_UNIT, ecx=A, stack_args=(U, OUT))
        return [r, r32(OUT)]
    raise ValueError(kind)


TARGS = [0x1, 0x2, 0x4, 0x6, 0x8, 0xE, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400, 0x4_000, 0x1_0000, 0x1_E000,
         0x100_0000, 0x400_0000, 0x800_0000, 0x1000_0000, 0x2000_0000, 0x4000_0000, 0x8000_0000, 0x200_0000,
         0x20_0000, 0x10_0000, 0x40_0000, 0x80_0000]


def rand_w(rng):
    w = 0
    for _ in range(rng.choice([0, 1, 1, 2, 3])):
        w |= rng.choice(TARGS)
    if rng.random() < 0.1:
        w = rng.getrandbits(32)
    return w


def rand_t(rng):
    t = rng.choice([0x1000000, 0x2000000]) | rng.choice([0x100000, 0x200000]) | rng.choice([0x400000, 0x800000])
    t |= rng.choice([0x10000000, 0x20000000]) | rng.choice([0x4000000, 0x8000000]) | rng.choice([0x40000000, 0x80000000])
    t |= rng.choice([0x2, 0x4, 0x8, 0x10, 0x6, 0x0, 0xA, 0xC])
    if rng.random() < 0.15:
        t = rng.getrandbits(32)
    if rng.random() < 0.05:
        t |= rng.choice([0x1, 0x20, 0x40, 0x80, 0x100, 0x200, 0x400])
    return t


def gen(rng, kind):
    c = {}
    c["w"] = rand_w(rng)
    c["t"] = rand_t(rng)
    c["u5c"] = rng.choice([0, 0x100, 0x10000, 0x40000000, 0x100 | 0x10000])
    c["u20"] = rng.choice([0, 8, 0x10, 0x18])
    c["u24c"] = rng.choice([0x2, 0x4, 0x8, 0x10, 0x6, 0x0, 0xA])
    c["u33"] = rng.choice([0x41, 0x5A, 0x61, 0x68, 0x30, 0x40, 0x5B, 0x7A])
    c["u248"] = rng.getrandbits(12)
    c["eth"] = rng.choice([0, 0, 1, 2, 0xFFFFFFFF])
    c["mim"] = rng.choice([0, 0, 1, 3, 0xFFFFFFFF])
    c["a20"] = rng.choice([0x80000 | 0x100000, 0x80000, 0x100000, 0, 0x180000 | 4])
    c["kind"] = [rng.choice([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), rng.choice([0, 1, 2, 3, 4, 5, 6, 7, 8])]
    c["wt"] = [rand_w(rng), rand_w(rng)]
    if rng.random() < 0.1:
        c["wt"][rng.randrange(2)] = rng.choice([1, 0x40, 0x80, 0x100])
    c["atk"] = [rng.randrange(0, 8), rng.randrange(0, 8)]
    c["gl"] = rng.choice([0, 1, 0x1234])
    c["misc"] = rng.choice([0, 1])
    return c


def row(kind, c, out):
    if kind == "expand":
        head = [c["w"]]
    elif kind == "pred":
        head = [c["t"], c["w"]]
    elif kind == "uflags":
        head = [c["u5c"], c["u20"], c["u24c"], c["u33"], c["u248"]]
    elif kind == "info":
        head = [c["t"], c["a20"], c["kind"][0], c["kind"][1], c["wt"][0], c["wt"][1]]
    else:
        head = [c["u5c"], c["u20"], c["u24c"], c["u33"], c["u248"], c["eth"], c["mim"], c["a20"], c["kind"][0],
                c["kind"][1], c["wt"][0], c["wt"][1], c["atk"][0], c["atk"][1], c["gl"], c["misc"]]
    return head + out


def main():
    n = 600
    out_dir = None
    args = sys.argv[1:]
    if args and args[0].isdigit():
        n = int(args.pop(0))
    if args and args[0] == "--rs":
        out_dir = args[1]
    rng = random.Random(20261014)
    total = 0
    for kind in ("expand", "pred", "uflags", "info", "unit"):
        rows = []
        for _ in range(n):
            c = gen(rng, kind)
            rows.append(row(kind, c, run(kind, c)))
        total += len(rows)
        zeros = sum(1 for r in rows if r[-1] == 0)
        print(kind, len(rows), "rows; last output zero in", zeros)
        if out_dir:
            width = len(rows[0])
            lines = [
                "// Generated by tools/emu/target_accept_emu.py from the original Game.dll; do not edit.",
                f"pub const ROWS: &[[u32; {width}]] = &[",
            ]
            lines += ["    [" + ", ".join("0x%08X" % v for v in r) + "]," for r in rows]
            lines.append("];")
            with open(os.path.join(out_dir, f"target_accept_{kind}.rs"), "w", newline="\n") as fh:
                fh.write("\n".join(lines) + "\n")
    print("total", total)


if __name__ == "__main__":
    main()
