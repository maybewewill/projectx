"""Which level fields reach which argument of the Wind Walk buff constructor (slot 0x358 = 0x6F171630), and which buff
field each argument lands in (docs/spec/abilities-orc-heroes.md section 11).

Part 1 runs the ORIGINAL cast effect `0x6F18CAA0` on a fake ability whose level record holds marker values
(Dur 1.0, HeroDur 2.0, Area 3.0, DataA 4.0 ... DataE 8.0, Cost field irrelevant), the buff factory `0x6F189F80` is a stub
that returns a fake buff whose vtable slot 0x358 records its stack arguments.
Part 2 runs the ORIGINAL constructor `0x6F171630` with marker arguments 0x11000001 .. 0x11000009 and prints the buff
fields it fills (agile cells at +0xDC, +0xE4, +0xF4, +0xEC are stubbed with a recording setter: value +4 of the cell).

Prints text only.  Usage: GAME_DLL=... py tools/emu/ww_ctor_emu.py
"""
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import chain_emu as C  # noqa: E402
import emu  # noqa: E402
import abilities4_emu as A  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
D = emu.DATA
w32, r32, stub, ret, patch = C.w32, C.r32, C.stub, C.ret, C.patch

ABIL = D + 0x8000
DEF = D + 0x8400
LVL = D + 0x8500
AVT = D + 0x8800
OWNER = D + 0x8C00
OWNER_VT = D + 0x8D00
BUFF = D + 0x9000
BVT = D + 0x9400
REC = D + 0x9C00


def fb(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def part1():
    patch(0x6F040000, stub(ret(0)))
    patch(0x6F1D2660, stub(b"\x31\xC0" + ret(0)))                    # not a hero
    patch(0x6F042B20, stub(ret(8)))
    patch(0x6F189F80, stub(b"\xB8" + struct.pack("<I", BUFF) + ret(0)))
    patch(0x6F079CA0, stub(ret(4)))
    w32(BVT + 0x324, stub(ret(4)))
    rec = stub(b"\x90" + ret(0x24))
    w32(BVT + 0x358, rec)
    w32(BUFF, BVT)
    mu.mem_write(ABIL, bytes(0x400))
    mu.mem_write(DEF, bytes(0x100))
    mu.mem_write(LVL, bytes(0x400))
    mu.mem_write(AVT, bytes(mu.mem_read(0x6F8F2534, 0x448)))
    w32(ABIL, AVT)
    w32(ABIL + 0x30, OWNER)
    w32(ABIL + 0x34, 0x77)
    w32(ABIL + 0x50, 0)
    w32(ABIL + 0x54, DEF)
    w32(ABIL + 0x9C, fb(0.0))
    w32(DEF + 0x2C, 1)
    w32(DEF + 0x50, 3)
    w32(DEF + 0x54, LVL)
    w32(OWNER, OWNER_VT)
    for off, v in ((0x08, 1.0), (0x0C, 2.0), (0x18, 3.0), (0x20, 4.0), (0x24, 5.0), (0x28, 6.0), (0x2C, 7.0), (0x30, 8.0)):
        w32(LVL + off, fb(v))
    got = []

    def hook(uc, address, size, user):
        esp = uc.reg_read(UC_X86_REG_ESP)
        row = [r32(esp + 4 + 4 * i) for i in range(9)]
        row = [r32(a) if 0x100000 <= a < 0x110000 else a for a in row]   # float arguments are passed by pointer
        got.append(row)

    mu.hook_add(UC_HOOK_CODE, hook, begin=rec, end=rec)
    emu.call(0x6F18CAA0, ecx=ABIL)
    names = {fb(1.0): "Dur", fb(2.0): "HeroDur", fb(3.0): "Area", fb(4.0): "DataA", fb(5.0): "DataB", fb(6.0): "DataC",
             fb(7.0): "DataD", fb(8.0): "DataE", 0x77: "owner34", ABIL: "ability", OWNER: "owner", 1: "one", 0: "zero"}
    for i, a in enumerate(got[0]):
        print("ctor arg a%d = %08x %s" % (i + 1, a, names.get(a, "?")))


def part2():
    # cells at +0xDC, +0xE4, +0xEC, +0xF4: vtable pointer whose slot 0 stores the argument at cell+4
    setter = stub(b"\x8B\x44\x24\x04" b"\x89\x41\x04" + ret(8))        # mov eax,[esp+4]; mov [ecx+4],eax
    cvt = D + 0xA800
    w32(cvt, setter)
    patch(0x6F0419C0, stub(b"\x8B\x44\x24\x04" b"\x89\x01" + ret(4)))   # handle assign
    patch(0x6F123FC0, stub(ret(0xC)))
    mu.mem_write(BUFF, bytes(0x400))
    for off in (0xDC, 0xE4, 0xEC, 0xF4):
        w32(BUFF + off, cvt)
    args = [0x11000000 + i + 1 for i in range(9)]
    for i in (1, 2, 3, 4, 5):     # a2..a6 are pointers to floats
        w32(D + 0xB000 + 4 * i, args[i])
        args[i] = D + 0xB000 + 4 * i
    args[0] = OWNER
    w32(BUFF + 0x30, OWNER)
    args[6] = ABIL
    mu.mem_write(OWNER, bytes(0x400))
    emu.call(0x6F171630, ecx=BUFF, stack_args=args)
    for off in (0xDC, 0xE4, 0xEC, 0xF4, 0xFC, 0x108):
        print("buff+%#x -> value %08x" % (off, r32(BUFF + off + (4 if off in (0xDC, 0xE4, 0xEC, 0xF4) else 0))))
    print("buff flags %08x" % r32(BUFF + 0x20))


if __name__ == "__main__":
    A.patch_seh()
    part1()
    part2()
