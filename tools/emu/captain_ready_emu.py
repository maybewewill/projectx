"""Unicorn check of the captain readiness routine of Game.dll 1.26a (CaptainReadiness*, ai-natives.md section 10).

Runs the original `0x6F2E91A0` (thiscall, ECX = captain record; stack: life flag, mana flag; `retn 8`). The group is a
chain of reference-counted nodes (captain +0xAC/+0xB0 = reference to the first node, node +0x24/+0x28 = reference to
the next); the engine helpers around it are stubbed with a model of what they do:
  0x6F4786B0  resolve a reference (ECX -> id) to its node            0x6F095DD0  wrap a node into a smart pointer
  0x6F472890  node -> unit                                           0x6F26C3E0 / 0x6F471910  class check (always passes)
  unit vtable +0x12C / +0x130  life / max life (out pointer, returns it)
  0x6F4773A0 / 0x6F4773E0      mana / max mana of the unit's agile block (ECX = unit + 0xB8)
What the routine itself decides (the real code runs): the hero test (first letter of the type code upper case and the
illusion flag 0x40000000 at +0x5C clear), the separate hero / other life sums, the shared mana sums, the start values
(1.0 each, the others' max life starts at 1.0 + (captain+0xB8 - captain+0xBC) * 400), the percentages
100 * x / y with the soft-float helpers, and the result: with the life flag the smaller of the hero and other
percentages, else the mana percentage, converted by the soft f2i.

Usage: GAME_DLL=/path/Game.dll py -I tools/emu/captain_ready_emu.py [--vectors OUT.tsv] [--cases N] [--seed S]
Vector line (tab separated): life flag, mana flag, B8, BC, units ('type_hex/flags_hex/life/maxlife/mana/maxmana' f32
bits in hex, joined by ';', '-' when empty), result.
"""
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import emu  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
u32 = emu.u32
F_READY = 0x6F2E91A0
H_RESOLVE = 0x6F4786B0
H_WRAP = 0x6F095DD0
H_NODE_UNIT = 0x6F472890
H_CLASS_A = 0x6F26C3E0
H_CLASS_B = 0x6F471910
H_MANA = 0x6F4773A0
H_MAXMANA = 0x6F4773E0

REGION = 0x00500000
mu.mem_map(REGION, 0x40000)
TIB = 0x00600000
mu.mem_map(TIB, 0x1000)
mu.msr_write(0xC0000100, TIB)   # fs base: the routine pushes and restores the SEH chain head (fs:[0])
CAPT = REGION
NODES = REGION + 0x1000      # 0x200 each
UNITS = REGION + 0x10000     # 0x400 each
VT_UNIT = REGION + 0x3F000
VT_NODE = REGION + 0x3F400
S_LIFE = REGION + 0x3F800
S_MAXLIFE = REGION + 0x3F810
S_NODE_FN = REGION + 0x3F820
UNIT_DATA = []


def rd(a):
    return struct.unpack("<I", bytes(mu.mem_read(a, 4)))[0]


def wr(a, v):
    mu.mem_write(a, struct.pack("<I", u32(v)))


mu.mem_write(S_LIFE, b"\x90" * 16)
mu.mem_write(S_MAXLIFE, b"\x90" * 16)
mu.mem_write(S_NODE_FN, b"\x31\xc0\xc3")  # xor eax,eax; ret
wr(VT_UNIT + 0x12C, S_LIFE)
wr(VT_UNIT + 0x130, S_MAXLIFE)
wr(VT_NODE + 0x1C, S_NODE_FN)
wr(VT_NODE + 0, S_NODE_FN)


def unit_of(addr):
    return UNIT_DATA[(addr - UNITS) // 0x400]


def ret(uc, val, pop):
    esp = uc.reg_read(UC_X86_REG_ESP)
    r = rd(esp)
    uc.reg_write(UC_X86_REG_EAX, u32(val))
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + pop)
    uc.reg_write(UC_X86_REG_EIP, r)


def hook(uc, address, size, user):
    ecx = uc.reg_read(UC_X86_REG_ECX)
    esp = uc.reg_read(UC_X86_REG_ESP)
    if address == H_RESOLVE:
        ret(uc, NODES + rd(ecx) * 0x200, 0)
    elif address == H_WRAP:
        wr(ecx, rd(esp + 4))
        ret(uc, ecx, 4)
    elif address == H_NODE_UNIT:
        ret(uc, UNITS + rd(ecx + 0x100) * 0x400, 0)
    elif address in (H_CLASS_A, H_CLASS_B):
        ret(uc, 1, 0)
    elif address in (S_LIFE, S_MAXLIFE):
        out = rd(esp + 4)
        u = unit_of(ecx)
        wr(out, u["life"] if address == S_LIFE else u["maxlife"])
        ret(uc, out, 4)
    elif address in (H_MANA, H_MAXMANA):
        out = rd(esp + 4)
        u = unit_of(ecx - 0xB8)
        wr(out, u["mana"] if address == H_MANA else u["maxmana"])
        ret(uc, out, 4)


for a in (H_RESOLVE, H_WRAP, H_NODE_UNIT, H_CLASS_A, H_CLASS_B, S_LIFE, S_MAXLIFE, H_MANA, H_MAXMANA):
    mu.hook_add(UC_HOOK_CODE, hook, begin=a, end=a)


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def build(units, b8, bc):
    UNIT_DATA.clear()
    UNIT_DATA.extend(units)
    wr(CAPT + 0xB8, b8)
    wr(CAPT + 0xBC, bc)
    n = len(units)
    if n:
        wr(CAPT + 0xAC, 0)
        wr(CAPT + 0xB0, 0x1234)
    else:
        wr(CAPT + 0xAC, 0xFFFFFFFF)
        wr(CAPT + 0xB0, 0xFFFFFFFF)
    for i, u in enumerate(units):
        node = NODES + i * 0x200
        wr(node, VT_NODE)
        wr(node + 4, 1000)       # reference count, never reaches 0
        wr(node + 0x100, i)
        wr(node + 0x24, i + 1 if i + 1 < n else 0xFFFFFFFF)
        wr(node + 0x28, 0x1234 if i + 1 < n else 0xFFFFFFFF)
        wr(UNITS + i * 0x400, VT_UNIT)
        wr(UNITS + i * 0x400 + 0x30, u["type"])
        wr(UNITS + i * 0x400 + 0x5C, u["flags"])


def run(life, mana, b8, bc, units):
    build(units, b8, bc)
    return emu.call(F_READY, ecx=CAPT, stack_args=(life, mana))


def random_unit(rng):
    hero = rng.random() < 0.3
    first = rng.choice("HOUEAN") if hero else rng.choice("hoeunz")
    typ = (ord(first) << 24) | (ord("a") << 16) | (ord("b") << 8) | ord("c")
    flags = rng.choice([0, 0, 0, 0x40000000, 0x100, 0x40000100])
    maxlife = rng.choice([100.0, 420.0, 550.0, 700.0, 1200.0, rng.uniform(50, 3000)])
    life = maxlife * rng.choice([1.0, 1.0, 0.75, 0.5, 0.33, rng.random()])
    maxmana = rng.choice([0.0, 0.0, 200.0, 300.0, 500.0, rng.uniform(0, 900)])
    mana = maxmana * rng.choice([1.0, 0.5, rng.random()])
    return {"type": typ, "flags": flags, "life": fb(life), "maxlife": fb(maxlife), "mana": fb(mana), "maxmana": fb(maxmana)}


def main():
    args = sys.argv[1:]
    out = args[args.index("--vectors") + 1] if "--vectors" in args else None
    n = int(args[args.index("--cases") + 1]) if "--cases" in args else 600
    seed = int(args[args.index("--seed") + 1]) if "--seed" in args else 1
    rng = random.Random(seed)
    lines = []
    for _ in range(n):
        k = rng.choice([0, 1, 2, 3, 5, 8, 12])
        units = [random_unit(rng) for _ in range(k)]
        bc = k if rng.random() < 0.5 else rng.randint(0, 12)
        b8 = bc if rng.random() < 0.5 else rng.randint(0, 14)
        life = rng.choice([0, 1])
        mana = rng.choice([0, 1])
        got = run(life, mana, b8, bc, units) & 0xFFFFFFFF
        us = ";".join("%08x/%08x/%08x/%08x/%08x/%08x" % (u["type"], u["flags"], u["life"], u["maxlife"], u["mana"], u["maxmana"]) for u in units)
        lines.append("\t".join([str(life), str(mana), str(b8), str(bc), us or "-", str(got)]))
    print("cases", n)
    if out:
        open(out, "w").write("\n".join(lines) + "\n")
        print("wrote", out)


if __name__ == "__main__":
    main()
