"""Unicorn run of Feral Spirit (`AOsf`, `CAbilitySpiritWolf`, vtable 0x6F927B2C) of Game.dll 1.26a
(docs/spec/abilities-orc-heroes.md section 14, task S39).

Two groups of cases, both running ORIGINAL code on fake objects (the harness helpers of `chain_emu.py`):

  E  the cast effect `0x6F1DF330` (ECX = ability). Real: the level getters (UnitID `0x6F02EDD0`, Dur `0x6F02E760`,
     Area `0x6F02E900`, DataB `0x6F02EA50`), the soft add `0x6F6EF000`, F2I `0x6F6EEC20`, the `CUnit` vtable slot `0xEC`.
     Stubbed: the event registration `0x6F1D7990` / `0x6F46D1B0`, the info builder `0x6F042B20`, the summon creator
     wrapper `0x6F1D27A0` (records its arguments), `0x6F053040`, `0x6F1D2600`, the class slot `0x3E4` (records its
     arguments) and the release `0x6F299390`.
       E <Dur> <+0x9C> <DataB> <Area> <UnitID> <owner idx> | <creator calls> <per call: type is-caster owner-arg
         pointer-is-Area count-arg flag> ... | <slot 0x3E4 calls> <first call: 4 argument words>
  P  the placement of one summon: the real creator wrapper `0x6F1D27A0` -> `0x6F1D26B0` with a fake caster position
     object (real rotation `0x6F4AFCD0`, transform `0x6F07B950`, soft add `0x6F6EF000`); the position normaliser
     `0x6F4743A0` copies the object's x, y, z, the cell id `0x6F2ABF20` is a constant, the unit creator `0x6F29F990`
     records its arguments:
       P <caster x> <caster y> <facing> <Area> <arg3> <owner> <flag> | <owner arg> <x'> <y'> <z'> <flags arg> <arg5> <cell> <type>

Usage: GAME_DLL=/path/Game.dll py tools/emu/feral_emu.py OUT.txt COUNT SEED
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

VT_SF = 0x6F927B2C
CUNIT_VT = 0x6F931934
F_EFFECT = 0x6F1DF330

ABIL = D + 0xA000
DEF = D + 0xA400
LVL = D + 0xA500
VT_COPY = D + 0xA800
CASTER = D + 0xB400
NEWUNIT = D + 0xBC00
POSOBJ = D + 0xC000
POSVT = D + 0xC100
CASTER2 = D + 0xC200
CASTERVT = D + 0xC300
AREA = D + 0xC400

ORIG_WRAPPER = b""
creator_calls = []
slot3e4_calls = []
place_calls = []


def mem(a):
    return struct.pack("<I", a)


def setup():
    patch(0x6F1D7990, stub(b"\x31\xC0" + ret(8)))        # event registration helper (returns 0)
    patch(0x6F46D1B0, stub(ret(0)))
    patch(0x6F042B20, stub(b"\x89\xC8" + ret(8)))        # info builder: mov eax, ecx
    patch(0x6F053040, stub(ret(4)))
    patch(0x6F1D2600, stub(ret(0)))
    patch(0x6F299390, stub(ret(4)))
    global ORIG_WRAPPER
    ORIG_WRAPPER = bytes(mu.mem_read(0x6F1D27A0, 5))
    rec = stub(b"\xB8" + mem(NEWUNIT) + ret(0x10))        # the summon creator wrapper: returns a new unit
    patch(0x6F1D27A0, rec)
    mu.mem_write(VT_COPY, bytes(mu.mem_read(VT_SF, 0x448)))
    s3e4 = stub(b"\x90" + ret(0x10))
    w32(VT_COPY + 0x3E4, s3e4)
    w32(CASTER, CUNIT_VT)
    return rec, s3e4


def fresh_ability(dur, plus9c, data_b, area, unit_id):
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
    w32(LVL + 0x18, area)
    w32(LVL + 0x24, data_b)
    w32(LVL + 0x44, unit_id)


def on_creator(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    args = [r32(esp + 4 + 4 * i) for i in range(4)]
    creator_calls.append((ecx, edx, args))


def on_3e4(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    a = [r32(esp + 4 + 4 * i) for i in range(4)]
    slot3e4_calls.append(a + [r32(a[1]), r32(a[2]), r32(a[1] + 4), r32(a[2] + 4)])


def setup_place():
    # 0x6F4743A0 (ECX = position object, stack = out): copy x, y, z; returns the out pointer
    patch(0x6F4743A0, stub(b"\x8B\x44\x24\x04" b"\x8B\x51\x10\x89\x10" b"\x8B\x51\x14\x89\x50\x04"
                           b"\x8B\x51\x18\x89\x50\x08" + ret(4)))
    patch(0x6F2ABF20, stub(b"\xB8\x77\x00\x00\x00" + ret(0)))   # cell id of a position: a constant
    rec = stub(b"\x31\xC0" + ret(0x18))                          # the unit creator: recorder
    patch(0x6F29F990, rec)
    # fake position object, slot 0x1C: writes the facing (obj+0x1C) to [esp+4]
    w32(POSVT + 0x1C, stub(b"\x8B\x44\x24\x04" b"\x8B\x51\x1C\x89\x10" + ret(4)))
    w32(POSOBJ, POSVT)
    w32(CASTERVT + 0xB8, stub(b"\xB8" + mem(POSOBJ) + ret(0)))  # the caster's position object (plain ret: the argument stays for the next call)
    w32(CASTER2, CASTERVT)
    return rec


def on_place(uc, address, size, user):
    esp = uc.reg_read(UC_X86_REG_ESP)
    ecx = uc.reg_read(UC_X86_REG_ECX)
    edx = uc.reg_read(UC_X86_REG_EDX)
    a = [r32(esp + 4 + 4 * i) for i in range(6)]
    place_calls.append((ecx, edx, a, r32(a[0]), r32(a[1]), r32(a[2])))


def main():
    out_path, count, seed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
    r = random.Random(seed)
    A.patch_seh()
    # Unicorn keeps translated blocks, so the two groups cannot share one process (the wrapper is patched in E):
    # run `feral_emu.py OUT COUNT SEED E` and `... P` and concatenate the files.
    group = sys.argv[4] if len(sys.argv) > 4 else "E"
    lines = []
    if group == "E":
        rec, s3e4 = setup()
        mu.hook_add(UC_HOOK_CODE, on_creator, begin=rec, end=rec)
        mu.hook_add(UC_HOOK_CODE, on_3e4, begin=s3e4, end=s3e4)
    for _ in range(count if group == "E" else 0):
        dur = r.choice([0, 0, C.rnd(r, 0.0, 120.0)])
        plus = r.choice([0, 0, 0, C.rnd(r, 0.0, 5.0)])
        data_b = r.choice([fbits(2.0), fbits(1.0), fbits(3.0), fbits(0.0), fbits(r.uniform(-1.0, 9.0))])
        area = r.choice([fbits(200.0), C.rnd(r, 0.0, 600.0)])
        unit_id = r.choice([0x6F737731, 0x6F737732, 0x6F737733])
        fresh_ability(dur, plus, data_b, area, unit_id)
        w32(CASTER + 0x58, r.randint(0, 15))
        creator_calls.clear()
        slot3e4_calls.clear()
        emu.call(F_EFFECT, ecx=ABIL)
        owner = r32(CASTER + 0x58)
        parts = []
        for ecx, edx, args in creator_calls:
            parts.append("%08x %d %d %d %d %08x" % (ecx, edx == CASTER, args[0] == owner, args[1] == ABIL + 0 or r32(args[1]) == area, args[2], args[3]))
        first = " ".join("%08x" % x for x in slot3e4_calls[0]) if slot3e4_calls else "-"
        lines.append("E %08x %08x %08x %08x %08x %d | %d %s | %d %s" % (
            dur, plus, data_b, area, unit_id, owner, len(creator_calls), " ; ".join(parts), len(slot3e4_calls), first))
    prec = setup_place() if group == "P" else 0
    if group == "P":
        mu.hook_add(UC_HOOK_CODE, on_place, begin=prec, end=prec)
    special = [0x00000000, 0x80000000, 0x3F800000, 0x43480000, 0x43C80000]
    for _ in range(count if group == "P" else 0):
        cx = C.rnd(r, -3000.0, 3000.0)
        cy = C.rnd(r, -3000.0, 3000.0)
        facing = fbits(r.uniform(0.0, 6.2831855)) if r.random() < 0.8 else C.rnd(r, -7.0, 7.0)
        area = r.choice(special) if r.random() < 0.5 else C.rnd(r, 0.0, 600.0)
        arg3 = r.choice([1])
        flag = r.choice([2, 0x502, r.getrandbits(16)])
        owner = r.randint(0, 15)
        w32(POSOBJ + 0x10, cx)
        w32(POSOBJ + 0x14, cy)
        w32(POSOBJ + 0x18, r.choice([0, 0x41000000]))
        w32(POSOBJ + 0x1C, facing)
        w32(AREA, area)
        place_calls.clear()
        emu.call(0x6F1D27A0, ecx=0x6F737731, edx=CASTER2, stack_args=(owner, AREA, arg3, flag))
        head = "P %08x %08x %08x %08x %d %d %04x |" % (cx, cy, facing, area, arg3, owner, flag)
        if not place_calls:
            lines.append(head + " none")
            continue
        ecx, edx, a, x, y, z = place_calls[0]
        lines.append(head + " %d %08x %08x %08x %08x %08x %08x %08x" % (ecx, x, y, z, a[3], a[4], a[5], edx))
    with open(out_path, "w") as f:
        f.write("\n".join(lines) + "\n")
    print("wrote %d lines" % len(lines))


if __name__ == "__main__":
    main()
