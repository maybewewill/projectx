"""Unicorn run of the point cast of Serpent Ward (`AOwd` / `AOsw`, `CAbilityWard`, vtable 0x6F8F91E4) of Game.dll 1.26a
(docs/spec/abilities-orc-heroes.md section 15, task S39).

The ORIGINAL slot `0x418` = `0x6F162DC0` (ECX = ability, stack = pointers to the target x and y, `ret 8`) runs on a fake
ability: real level getters (UnitID via slot `0x434` = `0x6F190980`, DataA `0x6F02E9E0`, Dur `0x6F02E760`), the owner
index slot `0x42C` = `0x6F190960` on a `CUnit`-vtable caster, soft add, F2I and the soft multiply. Stubbed: the Misc reader
`0x6F009EE0` (returns the case's `BuildingAngle`), the unit creator `0x6F29F990` (records its arguments), the class slot
`0x430` (the ward buff, records its arguments), `0x6F1D2600`, `0x6F299390`.

  W <x> <y> <Dur> <+0x9C> <DataA> <UnitID> <BuildingAngle> <owner> | <creator calls> <per call: owner type flags |
    pointer values: a1 a2 a3 (as floats of the dereferenced words) cell> <slot 0x430 calls> <first: duration bits>

Usage: GAME_DLL=/path/Game.dll py tools/emu/ward_emu.py OUT.txt COUNT SEED
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

VT_WARD = 0x6F8F91E4
CUNIT_VT = 0x6F931934
F_CAST = 0x6F162DC0

ABIL = D + 0xA000
DEF = D + 0xA400
LVL = D + 0xA500
VT_COPY = D + 0xA800
CASTER = D + 0xB400
NEWUNIT = D + 0xBC00
VARS = D + 0xB000
PX = D + 0xB100
PY = D + 0xB104

creator_calls = []
slot430_calls = []


def mem(a):
    return struct.pack("<I", a)


def setup():
    # Misc reader: [ecx] = [VARS]; returns ecx; ret 8
    patch(0x6F009EE0, stub(b"\xA1" + mem(VARS) + b"\x89\x01" b"\x89\xC8" + ret(8)))
    patch(0x6F1D2600, stub(ret(0)))
    patch(0x6F299390, stub(ret(4)))
    rec = stub(b"\xB8" + mem(NEWUNIT) + ret(0x18))
    patch(0x6F29F990, rec)
    mu.mem_write(VT_COPY, bytes(mu.mem_read(VT_WARD, 0x448)))
    s430 = stub(b"\x90" + ret(8))
    w32(VT_COPY + 0x430, s430)
    w32(CASTER, CUNIT_VT)
    return rec, s430


def fresh_ability(dur, plus9c, data_a, unit_id):
    mu.mem_write(ABIL, bytes(0x400))
    mu.mem_write(DEF, bytes(0x100))
    mu.mem_write(LVL, bytes(0x400))
    w32(ABIL, VT_COPY)
    w32(ABIL + 0x30, CASTER)
    w32(ABIL + 0x34, 0x77)
    w32(ABIL + 0x50, 0)
    w32(ABIL + 0x54, DEF)
    w32(ABIL + 0x9C, plus9c)
    w32(DEF + 0x2C, 1)
    w32(DEF + 0x50, 3)
    w32(DEF + 0x54, LVL)
    w32(LVL + 0x08, dur)
    w32(LVL + 0x20, data_a)
    w32(LVL + 0x44, unit_id)


def safe(a):
    try:
        return r32(a)
    except Exception:
        return 0xDEAD0000


def on_creator(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    a = [r32(esp + 4 + 4 * i) for i in range(6)]
    creator_calls.append((ecx, edx, a, [safe(a[0]), safe(a[1]), safe(a[2])]))


def on_430(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    a = [r32(esp + 4 + 4 * i) for i in range(2)]
    slot430_calls.append((a, safe(a[1])))


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    rec, s430 = setup()
    mu.hook_add(UC_HOOK_CODE, on_creator, begin=rec, end=rec)
    mu.hook_add(UC_HOOK_CODE, on_430, begin=s430, end=s430)
    lines = []
    for _ in range(count):
        dur = r.choice([0, C.rnd(r, 0.0, 120.0), fbits(40.0)])
        plus = r.choice([0, 0, 0, C.rnd(r, 0.0, 5.0)])
        data_a = r.choice([fbits(1.0), fbits(2.0), fbits(3.0), fbits(0.0), fbits(r.uniform(-1.0, 6.0))])
        unit_id = r.choice([0x6F737031, 0x6F737032, 0x6F737033])
        angle = r.choice([fbits(270.0), fbits(0.0), fbits(90.0), fbits(r.uniform(-400.0, 800.0)), fbits(180.0)])
        owner = r.randint(0, 15)
        x = C.rnd(r, -3000.0, 3000.0)
        y = C.rnd(r, -3000.0, 3000.0)
        fresh_ability(dur, plus, data_a, unit_id)
        w32(CASTER + 0x58, owner)
        w32(VARS, angle)
        w32(PX, x)
        w32(PY, y)
        creator_calls.clear()
        slot430_calls.clear()
        emu.call(F_CAST, ecx=ABIL, stack_args=(PX, PY))
        parts = []
        for ecx, edx, a, vals in creator_calls:
            parts.append("%d %08x %08x | %08x %08x %08x %08x %08x %08x | %08x %08x %08x" % (
                ecx, edx, a[3], a[0], a[1], a[2], a[3], a[4], a[5], vals[0], vals[1], vals[2]))
        first = "%08x" % slot430_calls[0][1] if slot430_calls else "-"
        lines.append("W %08x %08x %08x %08x %08x %08x %08x %d | %d %s | %d %s" % (
            x, y, dur, plus, data_a, unit_id, angle, owner, len(creator_calls), " ; ".join(parts), len(slot430_calls), first))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
