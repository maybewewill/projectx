"""Unicorn run of one Bladestorm tick (`AOww`, `CAbilityWhirlwind`, vtable 0x6F8F2144; docs/spec/abilities-orc-heroes.md
section 19, task S39).

The ORIGINAL tick `0x6F1912A0` (ECX = ability; the handler of the repeating timer message `0xD01C1`) runs on a fake ability.
Stubbed: the end routine `0x6F171450` (counted), the area damage body `0x6F2C31D0` (records its arguments), the caster's
position (`vf 0xB8` + `0x6F4743A0` copy x, y, z of the fake unit). Real: the counter arithmetic and the gates.

  T <ticks left before> <pause counter +0x3C> <radius> <damage> <targs word> | <ticks left after> <end calls>
    <body calls> | <ring count> <radius> <amount> <class> <flags word> <source is caster> <callback> <x> <y> <z>

Usage: GAME_DLL=/path/Game.dll py tools/emu/bladetick_emu.py OUT.txt COUNT SEED
"""
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import chain_emu as C  # noqa: E402
import emu  # noqa: E402
import abilities4_emu as A  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_ECX, UC_X86_REG_EDX  # noqa: E402

mu = emu.mu
D = emu.DATA
w32, r32, fbits, stub, ret, patch = C.w32, C.r32, C.fbits, C.stub, C.ret, C.patch

F_TICK = 0x6F1912A0
VT = 0x6F8F2144
ABIL = D + 0xA000
CASTER = D + 0xA800
CVT = D + 0xA900
POS = D + 0xAA00
calls = []
ends = [0]


def mem(a):
    return struct.pack("<I", a)


def setup():
    w32(CVT + 0xB8, stub(b"\xB8" + mem(POS) + ret(0)))                 # position object (plain ret)
    patch(0x6F4743A0, stub(b"\x8B\x44\x24\x04" b"\x8B\x11\x89\x10" b"\x8B\x51\x04\x89\x50\x04"
                           b"\x8B\x51\x08\x89\x50\x08" + ret(4)))      # out = x, y, z from the object
    w32(CASTER, CVT)
    end = stub(b"\xFF\x05" + mem(D + 0xAB00) + ret(0))                  # inc [D+0xAB00]
    patch(0x6F171450, end)
    body = stub(b"\x90" + ret(0x30))
    patch(0x6F2C31D0, body)
    mu.mem_write(D + 0xAC00, bytes(mu.mem_read(VT, 0x448)))
    return body


def on_body(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    a = [r32(esp + 4 + 4 * i) for i in range(12)]
    calls.append((ecx, edx, a))


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    body = setup()
    mu.hook_add(UC_HOOK_CODE, on_body, begin=body, end=body)
    lines = []
    for _ in range(count):
        left = r.choice([1, 1, 2, 3, 5, 24, r.randint(-2, 40)])
        pause = r.choice([0, 0, 0, 1, 2, -1])
        radius = C.rnd(r, 0.0, 600.0)
        damage = C.rnd(r, 0.0, 300.0)
        word = r.choice([0x0001_8A0A, 0x4018002, r.getrandbits(32)])
        mu.mem_write(ABIL, bytes(0x400))
        w32(ABIL, D + 0xAC00)
        w32(ABIL + 0x30, CASTER)
        w32(ABIL + 0xFC, left)
        w32(ABIL + 0x3C, pause)
        w32(ABIL + 0x10C, radius)
        w32(ABIL + 0x104, damage)
        w32(ABIL + 0x4C, word)
        w32(POS + 0, C.rnd(r, -3000.0, 3000.0))
        w32(POS + 4, C.rnd(r, -3000.0, 3000.0))
        w32(POS + 8, r.choice([0, 0x41000000]))
        w32(D + 0xAB00, 0)
        calls.clear()
        emu.call(F_TICK, ecx=ABIL)
        after = struct.unpack("<i", struct.pack("<I", r32(ABIL + 0xFC)))[0]
        head = "T %d %d %08x %08x %08x | %d %d %d" % (left, pause, radius, damage, word, after, r32(D + 0xAB00), len(calls))
        if calls:
            ecx, edx, a = calls[0]
            # stack: (&ring ptr?, ...) record the raw words and the two floats of the ring
            ringp = a[0]
            head += " | " + " ".join("%08x" % x for x in a) + " | %08x %08x | %08x %08x %08x" % (
                r32(ringp), r32(ringp + 4), r32(ecx), r32(ecx + 4), r32(ecx + 8))
        lines.append(head)
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
