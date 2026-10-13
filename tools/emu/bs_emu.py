"""Unicorn run of the Bladestorm (`AOww`, `CAbilityWhirlwind`, vtable 0x6F8F2144) set-up of Game.dll 1.26a
(docs/spec/abilities-orc-heroes.md section 12, task S34).

The ORIGINAL level initialiser, slot `0x3E8` = `0x6F163570`, runs on a fake ability with a real level record (real level
getters, real soft-float): it computes the tick count stored at `+0xF8` and fills two agile cells with the damage per tick
(`+0x100`, read as `+0x104`) and the damage radius (`+0x108`, read as `+0x10C`). Cell setters are recorders.

Row (floats as hex bits):
  B <Dur> <HeroDur> <Area> <DataA> <ability+0x9C> | <ticks> <damage per tick> <radius>
and one line `K <bits of [0x6FAB1F4C]> <bits of recip(K)>` (the constant that sets the tick rate and the period).

Usage: GAME_DLL=/path/Game.dll py tools/emu/bs_emu.py OUT.txt COUNT SEED
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

mu = emu.mu
D = emu.DATA
w32, r32, stub, ret = C.w32, C.r32, C.stub, C.ret

ABIL = D + 0x8000
DEF = D + 0x8400
LVL = D + 0x8500
AVT = D + 0x8800
CVT = D + 0x9400
OWNER = D + 0x9800


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    setter = stub(b"\x8B\x44\x24\x04" b"\x8B\x00" b"\x89\x41\x04" + ret(8))   # [ecx+4] = [[esp+4]]
    w32(CVT, setter)
    mu.mem_write(AVT, bytes(mu.mem_read(0x6F8F2144, 0x448)))
    lines = []
    k = r32(0x6FAB1F4C)
    kk = struct.unpack("<f", struct.pack("<I", k))[0]
    lines.append("K %08x" % k)
    for _ in range(count):
        mu.mem_write(ABIL, bytes(0x400))
        mu.mem_write(DEF, bytes(0x100))
        mu.mem_write(LVL, bytes(0x400))
        w32(ABIL, AVT)
        w32(ABIL + 0x30, OWNER)
        w32(ABIL + 0x34, 0x77)
        w32(ABIL + 0x50, 0)
        w32(ABIL + 0x54, DEF)
        w32(ABIL + 0x100, CVT)
        w32(ABIL + 0x108, CVT)
        w32(DEF + 0x2C, 1)
        w32(DEF + 0x50, 3)
        w32(DEF + 0x54, LVL)
        dur = C.rnd(r, 0.0, 60.0)
        hdur = C.rnd(r, 0.0, 60.0)
        area = C.rnd(r, 0.0, 600.0)
        da = C.rnd(r, 0.0, 300.0)
        x9c = r.choice([0, 0, 0, C.fbits(r.uniform(-2.0, 5.0))])
        w32(LVL + 0x08, dur)
        w32(LVL + 0x0C, hdur)
        w32(LVL + 0x18, area)
        w32(LVL + 0x20, da)
        w32(ABIL + 0x9C, x9c)
        emu.call(0x6F163570, ecx=ABIL)
        lines.append("B %08x %08x %08x %08x %08x | %d %08x %08x" % (
            dur, hdur, area, da, x9c, struct.unpack("<i", struct.pack("<I", r32(ABIL + 0xF8)))[0],
            r32(ABIL + 0x104), r32(ABIL + 0x10C)))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines, K=%r" % (len(lines), kk))


if __name__ == "__main__":
    main()
