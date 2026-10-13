"""Unicorn run of the point cast of Far Sight (`AOfs`, `CAbilityFarSight` vtable 0x6F88B25C, a `CAbilityDetectAoe`) of
Game.dll 1.26a (docs/spec/abilities-orc-heroes.md section 21, task S39).

The ORIGINAL slot `0x418` = `0x6F06FA50` (ECX = ability, stack = pointers to the target x and y, `ret 8`) runs on a fake
ability: the real level getters through the class slots `0x434` (`F2I(DataA)`), `0x42C` (`Area`), `0x430` (`Dur +
ability+0x9C`), the real effect id lookup `0x6F02EE80` and the real owner index slot `0xEC` of a `CUnit`-vtable caster. The
terrain height `0x6F24F3E0` is a stub (returns the case's z); the effect creator `0x6F06F8D0` records its eight stack words.

  F <x> <y> <Dur> <+0x9C> <DataA> <Area> <effect id> <z> <owner> | <8 words as recorded, floats dereferenced where they are
    pointers: id owner(?) ...>

Usage: GAME_DLL=/path/Game.dll py tools/emu/farsight_emu.py OUT.txt COUNT SEED
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
from unicorn.x86_const import UC_X86_REG_ESP  # noqa: E402

mu = emu.mu
D = emu.DATA
w32, r32, fbits, stub, ret, patch = C.w32, C.r32, C.fbits, C.stub, C.ret, C.patch

VT_FS = 0x6F88B25C
F_CAST = 0x6F06FA50
ABIL = D + 0xA000
DEF = D + 0xA400
LVL = D + 0xA500
CASTER = D + 0xB400
PX = D + 0xB500
PY = D + 0xB504
EFFIDS = D + 0xB600
VARS = D + 0xB700
calls = []


def mem(a):
    return struct.pack("<I", a)


def safe(a):
    try:
        return r32(a)
    except Exception:
        return 0xDEAD0000


def setup():
    # terrain height 0x6F24F3E0 (stack: x, y, flag; ret 0xC): returns z in st0 in the original; here eax is ignored by the
    # caller's `fstp` pair, so the stub leaves 0.0 on the FPU stack via `fldz`
    patch(0x6F24F3E0, stub(b"\xD9\xEE" + ret(0xC)))
    rec = stub(b"\x90" + ret(0x20))
    patch(0x6F06F8D0, rec)
    w32(CASTER, 0x6F931934)
    return rec


def fresh(dur, plus, data_a, area, effid, level):
    mu.mem_write(ABIL, bytes(0x400))
    mu.mem_write(DEF, bytes(0x100))
    mu.mem_write(LVL, bytes(0x400))
    w32(ABIL, VT_FS)
    w32(ABIL + 0x30, CASTER)
    w32(ABIL + 0x34, 0x77)
    w32(ABIL + 0x50, level)
    w32(ABIL + 0x54, DEF)
    w32(ABIL + 0x9C, plus)
    w32(DEF + 0x2C, 1)
    w32(DEF + 0x50, 4)
    w32(DEF + 0x54, LVL)
    for lv in range(4):
        base = LVL + lv * 0x68
        w32(base + 0x08, dur)
        w32(base + 0x18, area)
        w32(base + 0x20, data_a)
        w32(base + 0x5C, 1)
        w32(base + 0x60, EFFIDS)
    w32(EFFIDS, effid)


def on_rec(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    a = [r32(esp + 4 + 4 * i) for i in range(8)]
    calls.append((a, [safe(x) for x in a]))


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec = setup()
    mu.hook_add(UC_HOOK_CODE, on_rec, begin=rec, end=rec)
    lines = []
    for _ in range(count):
        dur = r.choice([fbits(8.0), fbits(r.uniform(0.0, 30.0))])
        plus = r.choice([0, 0, fbits(r.uniform(0.0, 2.0))])
        data_a = r.choice([fbits(3.0), fbits(r.uniform(0.0, 8.0))])
        area = r.choice([fbits(600.0), fbits(1800.0), fbits(4000.0), fbits(r.uniform(100.0, 5000.0))])
        effid = r.choice([0x58626474, 0x58626475])
        level = r.randint(0, 3)
        x = fbits(r.uniform(-4000.0, 4000.0))
        y = fbits(r.uniform(-4000.0, 4000.0))
        owner = r.randint(0, 15)
        fresh(dur, plus, data_a, area, effid, level)
        w32(CASTER + 0x58, owner)
        w32(PX, x)
        w32(PY, y)
        calls.clear()
        emu.call(F_CAST, ecx=ABIL, stack_args=(PX, PY))
        assert len(calls) == 1
        a, v = calls[0]
        lines.append("F %08x %08x %08x %08x %08x %08x %08x %d | %s | %s" % (
            x, y, dur, plus, data_a, area, effid, owner, " ".join("%08x" % t for t in a), " ".join("%08x" % t for t in v)))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
